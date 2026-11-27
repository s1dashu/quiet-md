# xim-ctext (vendored)

[xim-ctext](https://crates.io/crates/xim-ctext) 0.3.0 from [xim-rs](https://github.com/Riey/xim-rs), MIT (see `LICENSE`). GPUI's X11 input method client (`zed-xim`) passes everything the input method sends through `compound_text_to_utf8` and unwraps the result with `.expect()`.

Upstream decodes only text that is a single UTF-8 or JIS X 0208 segment. For anything else it returns an error, and that error crashes the app. IBus with Rime under a Chinese locale sends GB2312 segments (`ESC $ ) A`), which crashed Quiet.md as soon as a candidate was committed (issue #465, `zed-xim-0.4.0-zed/src/client.rs:190`).

Local changes, all in `src/lib.rs`:

- `compound_text_to_utf8` decodes COMPOUND_TEXT as the X11 spec defines it: ASCII/ISO 8859-1 by default, any number of segments, designations to GL and GR, the ISO 8859 96-character sets, GB2312, JIS X 0208, KS C 5601, JIS X 0201, UTF-8 segments (`ESC % G … ESC % @`), extended segments in any encoding `encoding_rs` knows, and direction markers (dropped).
- It never fails. Invalid or truncated bytes and unknown character sets become U+FFFD, and `DecodeError` is kept only for API compatibility.
- Text with no escape that is valid UTF-8 is returned unchanged, as before. Some input methods send raw UTF-8.
- Tests feed every case above plus truncated and invalid input.

`utf8_to_compound_text` and `CText`, the encoding side, are unchanged.
