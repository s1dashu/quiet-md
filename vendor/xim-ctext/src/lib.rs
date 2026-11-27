//! COMPOUND_TEXT en/decoder. Encodes as one UTF-8 segment; decodes any
//! compound text (quiet.md: the decoder is rewritten, see README.md).

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use encoding_rs::{Encoding, EUC_JP, EUC_KR, GBK, UTF_8};

#[cfg(feature = "std")]
use std::io::{self, Write};

const UTF8_START: &[u8] = &[0x1B, 0x25, 0x47];
const UTF8_END: &[u8] = &[0x1B, 0x25, 0x40];

/// Wrapper for reduce allocation
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct CText<'s> {
    utf8: &'s str,
}

impl fmt::Debug for CText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl fmt::Display for CText<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.utf8)
    }
}

impl<'s> CText<'s> {
    pub const fn new(utf8: &'s str) -> Self {
        Self { utf8 }
    }

    #[allow(clippy::len_without_is_empty)]
    pub const fn len(self) -> usize {
        self.utf8.len() + UTF8_START.len() + UTF8_END.len()
    }

    #[cfg(feature = "std")]
    pub fn write(self, mut out: impl Write) -> io::Result<usize> {
        let mut writed = 0;
        writed += out.write(UTF8_START)?;
        writed += out.write(self.utf8.as_bytes())?;
        writed += out.write(UTF8_END)?;
        Ok(writed)
    }
}

/// Encoding utf8 to COMPOUND_TEXT with utf8 escape
pub fn utf8_to_compound_text(text: &str) -> Vec<u8> {
    let mut ret = Vec::with_capacity(text.len() + 6);
    ret.extend_from_slice(UTF8_START);
    ret.extend_from_slice(text.as_bytes());
    ret.extend_from_slice(UTF8_END);
    ret
}

/// Kept for API compatibility: `compound_text_to_utf8` no longer fails.
#[derive(Debug, Clone)]
pub enum DecodeError {
    InvalidEncoding,
    UnsupportedEncoding,
    Utf8Error(alloc::string::FromUtf8Error),
}

impl From<alloc::string::FromUtf8Error> for DecodeError {
    fn from(err: alloc::string::FromUtf8Error) -> Self {
        DecodeError::Utf8Error(err)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEncoding => write!(f, "Invalid compound text"),
            Self::UnsupportedEncoding => write!(f, "This encoding is not supported yet"),
            Self::Utf8Error(e) => write!(f, "Not a valid utf8 {}", e),
        }
    }
}

const ESC: u8 = 0x1B;
const CSI: u8 = 0x9B;

/// What a half (GL 0x21–0x7E, GR 0xA1–0xFE) currently draws from.
#[derive(Clone, Copy)]
enum Set {
    Ascii,
    /// JIS X 0201 katakana, mapped straight to the halfwidth forms.
    Kana,
    /// Decoded by `encoding_rs` from the byte with its high bit set (EUC-CN,
    /// EUC-JP, EUC-KR and the ISO 8859 upper halves all read GR that way).
    Table(&'static Encoding),
    Unknown,
}

/// `ESC ( F` / `ESC ) F`: a 94-character set.
fn set94(f: u8) -> Set {
    match f { b'B' | b'J' => Set::Ascii, b'I' => Set::Kana, _ => Set::Unknown }
}

/// `ESC - F`: the upper half of an ISO 8859 part (windows-1252 and -1254 agree
/// with 8859-1 and -9 from 0xA0 up).
fn set96(f: u8) -> Set {
    use encoding_rs::*;
    Set::Table(match f {
        b'A' => WINDOWS_1252, b'B' => ISO_8859_2, b'C' => ISO_8859_3, b'D' => ISO_8859_4, b'F' => ISO_8859_7,
        b'G' => ISO_8859_6, b'H' => ISO_8859_8, b'L' => ISO_8859_5, b'M' => WINDOWS_1254, _ => return Set::Unknown,
    })
}

/// `ESC $ ( F` / `ESC $ ) F` (and the old `ESC $ F`): a 94×94 set.
fn set94n(f: u8) -> Set {
    match f { b'A' => Set::Table(GBK), b'@' | b'B' => Set::Table(EUC_JP), b'C' => Set::Table(EUC_KR), _ => Set::Unknown }
}

/// Decoded text so far, and the bytes of a run in one table encoding that
/// is decoded once it ends (a double-byte character is never split).
#[derive(Default)]
struct Out { s: String, run: Vec<u8>, enc: Option<&'static Encoding>, bad: bool }

impl Out {
    fn flush(&mut self) {
        if let Some(e) = self.enc.take() { self.s.push_str(&e.decode_without_bom_handling(&self.run).0); self.run.clear() }
    }
    fn ch(&mut self, c: char) { self.flush(); self.bad = false; self.s.push(c) }
    fn table(&mut self, e: &'static Encoding, b: u8) {
        if self.enc != Some(e) { self.flush(); self.enc = Some(e) }
        self.bad = false;
        self.run.push(b)
    }
    /// One U+FFFD for a run of what cannot be read.
    fn bad(&mut self) { if !self.bad { self.flush(); self.s.push('\u{FFFD}'); self.bad = true } }
    fn set(&mut self, set: Set, b: u8) {
        match set {
            Set::Ascii => self.ch(char::from(b & 0x7F)),
            Set::Kana => match b & 0x7F { v @ 0x21..=0x5F => self.ch(char::from_u32(0xFF61 + u32::from(v - 0x21)).unwrap_or('\u{FFFD}')), _ => self.bad() },
            Set::Table(e) => self.table(e, b | 0x80),
            Set::Unknown => self.bad(),
        }
    }
    /// `ESC % / F M L`'s data: an encoding's name, STX, then text in it.
    fn extended(&mut self, seg: &[u8]) {
        let enc = seg.iter().position(|&b| b == 0x02).and_then(|stx| Some((Encoding::for_label(&seg[..stx])?, &seg[stx + 1..])));
        match enc { Some((e, text)) => { self.flush(); self.bad = false; self.s.push_str(&e.decode_without_bom_handling(text).0) } None => self.bad() }
    }
}

/// Decodes COMPOUND_TEXT (X11 Compound Text Encoding, version 1.1). Never
/// fails: what cannot be read — invalid or truncated bytes, an unknown
/// character set — comes out as U+FFFD. Text with no escape that is valid
/// UTF-8 is taken as it is (some input methods send raw UTF-8).
pub fn compound_text_to_utf8(bytes: &[u8]) -> Result<String, DecodeError> {
    if !bytes.contains(&ESC) { if let Ok(s) = core::str::from_utf8(bytes) { return Ok(s.into()) } }
    let mut out = Out::default();
    let (mut gl, mut gr, mut utf8) = (Set::Ascii, Set::Table(encoding_rs::WINDOWS_1252), false);
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b == ESC {
            let start = i;
            while bytes.get(i).is_some_and(|b| (0x20..=0x2F).contains(b)) { i += 1 }
            let Some(&f) = bytes.get(i).filter(|f| (0x30..=0x7E).contains(*f)) else { out.bad(); continue };
            i += 1;
            match (&bytes[start..i - 1], f) {
                (b"%", b'G') => utf8 = true,
                (b"%", b'@') => utf8 = false,
                (b"%/", _) => {
                    // M L give the data's length, 7 bits each, high bit set.
                    let (Some(&m), Some(&l)) = (bytes.get(i), bytes.get(i + 1)) else { out.bad(); break };
                    let end = (i + 2 + ((usize::from(m & 0x7F) << 7) | usize::from(l & 0x7F))).min(bytes.len());
                    out.extended(&bytes[i + 2..end]);
                    i = end;
                }
                (b"(", f) => gl = set94(f),
                (b")", f) => gr = set94(f),
                (b"-", f) => gr = set96(f),
                (b"$" | b"$(", f) => gl = set94n(f),
                (b"$)", f) => gr = set94n(f),
                _ => out.bad(),
            }
        } else if utf8 {
            out.table(UTF_8, b)
        } else {
            match b {
                b'\t' | b'\n' | b' ' => out.ch(char::from(b)),
                0x21..=0x7E => out.set(gl, b),
                0xA0..=0xFF => out.set(gr, b),
                // Direction markers (CSI 1 ], CSI 2 ], CSI ]): nothing to draw.
                CSI => {
                    while bytes.get(i).is_some_and(|b| (0x20..=0x3F).contains(b)) { i += 1 }
                    if bytes.get(i).is_some_and(|b| (0x40..=0x7E).contains(b)) { i += 1 }
                }
                _ => {}
            }
        }
    }
    out.flush();
    Ok(out.s)
}

#[cfg(test)]
mod tests {
    use super::compound_text_to_utf8 as dec;
    use alloc::{string::String, vec::Vec};

    fn d(bytes: &[u8]) -> String { dec(bytes).unwrap() }

    #[test]
    fn korean() {
        const UTF8: &str = "가나다";
        const COMP: &[u8] = &[27, 37, 71, 234, 176, 128, 235, 130, 152, 235, 139, 164, 27, 37, 64];
        assert_eq!(crate::utf8_to_compound_text(UTF8), COMP);
        assert_eq!(d(COMP), UTF8);
    }

    #[test]
    fn iso_2011_jp() {
        assert_eq!(d(&[27, 36, 40, 66, 69, 108, 53, 126]), "東京");
    }

    /// What IBus sends for Rime under zh_CN (#465): GB2312 in GR, or in GL.
    #[test]
    fn gb2312() {
        assert_eq!(d(b"\x1b$)A\xd6\xd0\xce\xc4"), "中文");
        assert_eq!(d(b"\x1b$(AVPND"), "中文");
        assert_eq!(d(b"ab\x1b$)A\xd6\xd0c \xce\xc4\x1b(B!"), "ab中c 文!");
    }

    #[test]
    fn other_sets() {
        assert_eq!(d(b"\x1b$)C\xb0\xa1"), "가");
        assert_eq!(d(b"\x1b$)B\xc5\xec"), "東");
        assert_eq!(d(b"caf\xe9"), "café");
        assert_eq!(d(b"\x1b-B\xb1"), "ą");
        assert_eq!(d(b"\x1b)I\xb1\x1b(IB"), "ｱﾂ");
        assert_eq!(d(b"\x9b1]a\x9b]"), "a");
        assert_eq!(d(b"x\x1b%/1\x80\x8agb18030\x02\xd6\xd0y"), "x中y");
        assert_eq!(d("你好".as_bytes()), "你好");
    }

    #[test]
    fn truncated_and_invalid() {
        assert_eq!(d(b"\x1b%G\xe4\xb8"), "\u{FFFD}");
        assert_eq!(d(b"\x1b%G\xe4\xb8\xad"), "中");
        assert_eq!(d(b"\x1b$)A\xd6\xd0\xce"), "中\u{FFFD}");
        assert_eq!(d(b"\x1b$)Z\xd6\xd0\xce\xc4ok"), "\u{FFFD}ok");
        assert_eq!(d(b"a\x1b"), "a\u{FFFD}");
        assert_eq!(d(b"a\x1b$"), "a\u{FFFD}");
        assert_eq!(d(b"\x1b%/1\x80"), "\u{FFFD}");
        assert_eq!(d(b"\x1b%/1\x80\x7fnope\x02x"), "\u{FFFD}");
        // Upstream panicked on these (a UTF-8 segment shorter than its end).
        for b in [&b"\x1b"[..], b"\x1b%", b"\x1b%G", b"\x1b%G\x1b", b"\x1b%Ga"] { dec(b).unwrap(); }
    }

    /// Random escapes, set changes and bytes never panic.
    #[test]
    fn random_bytes() {
        const BITS: &[&[u8]] = &[b"\x1b", b"\x1b%G", b"\x1b%@", b"\x1b$)A", b"\x1b$(B", b"\x1b-A", b"\x1b)I", b"\x1b%/1", b"\x9b", b"\x02", b"\xd6", b"a", b"\xff"];
        let mut seed = 0x2545F4914F6CDD1Du64;
        for _ in 0..20_000 {
            let mut v: Vec<u8> = Vec::new();
            for _ in 0..(seed % 12) {
                seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
                if seed.is_multiple_of(3) { v.push((seed >> 32) as u8) } else { v.extend_from_slice(BITS[(seed >> 40) as usize % BITS.len()]) }
            }
            dec(&v).unwrap();
            seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
        }
    }
}
