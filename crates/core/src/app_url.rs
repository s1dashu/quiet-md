//! `quietmd://` links: how another program (the `quiet` CLI's `quiet open`) asks the
//! app to show something. They only navigate: open a document (at one block) or a
//! folder's page. Nothing in one writes, runs or reaches outside what is already
//! open, and anything else in one (another action, an unknown or repeated
//! parameter, a value that is not an id) is refused whole.

use crate::block_ref;

pub const SCHEME: &str = "quietmd";

/// What a link shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Document `doc`, at the block pinned ref `block` names (`#3.1~k7f2`) when given;
    /// `top`: the top-level folder it is in, when the sender knew it.
    Doc { top: Option<String>, doc: String, block: Option<String> },
    /// Folder `folder`'s page in top-level folder `top`; None: that top-level folder's own.
    Folder { top: String, folder: Option<String> },
    /// 回收站 (every top-level folder's).
    Trash,
}

/// A key or id as the library writes them (uuids, hashes, `local:` ids): nothing to escape into.
fn is_id(s: &str) -> bool { !s.is_empty() && s.len() <= 200 && s.bytes().all(|c| c.is_ascii_alphanumeric() || b"._:-".contains(&c)) }

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `%xx` undone; a stray `%` or bytes that are not UTF-8 refuse the link.
fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => { out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?); i += 3; }
            b'+' => { out.push(b' '); i += 1; }
            c => { out.push(c); i += 1; }
        }
    }
    String::from_utf8(out).ok()
}

impl Target {
    pub fn to_url(&self) -> String {
        let mut q: Vec<(&str, &str)> = vec![];
        match self {
            Target::Doc { top, doc, block } => {
                if let Some(t) = top { q.push(("top", t)); }
                q.push(("doc", doc));
                if let Some(b) = block { q.push(("block", b)); }
            }
            Target::Folder { top, folder } => {
                q.push(("top", top));
                if let Some(f) = folder { q.push(("folder", f)); }
            }
            Target::Trash => q.push(("view", "trash")),
        }
        format!("{SCHEME}://open?{}", q.iter().map(|(k, v)| format!("{k}={}", encode(v))).collect::<Vec<_>>().join("&"))
    }

    /// The target of link `url`, or why it is refused.
    pub fn parse(url: &str) -> Result<Target, String> {
        if url.len() > 2048 { return Err("the link is too long".into()); }
        let (scheme, rest) = url.split_once("://").ok_or("not a link")?;
        if !scheme.eq_ignore_ascii_case(SCHEME) { return Err(format!("not a {SCHEME}:// link")); }
        let (action, query) = rest.split_once('?').unwrap_or((rest, ""));
        let query = query.split_once('#').map_or(query, |(q, _)| q);
        if action.trim_end_matches('/') != "open" { return Err(format!("unknown action {:?}: links only open things", action.trim_end_matches('/'))); }
        let (mut top, mut doc, mut block, mut folder, mut view) = (None, None, None, None, None);
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = decode(v).ok_or_else(|| format!("{k}: not a valid value"))?;
            let slot = match k { "top" => &mut top, "doc" => &mut doc, "block" => &mut block, "folder" => &mut folder, "view" => &mut view, _ => return Err(format!("unknown parameter {k:?}")) };
            if slot.is_some() { return Err(format!("{k} given twice")); }
            *slot = Some(v);
        }
        for (k, v) in [("top", &top), ("doc", &doc), ("folder", &folder)] {
            if let Some(v) = v && !is_id(v) { return Err(format!("{k}: {v:?} is not an id")); }
        }
        if let Some(b) = &block && block_ref::parse(b).is_none() { return Err(format!("block: {b:?} is not a block ref (#3.1~k7f2)")); }
        if let Some(v) = view {
            if v != "trash" { return Err(format!("view: {v:?} is not a page a link opens")); }
            if top.is_some() || doc.is_some() || block.is_some() || folder.is_some() { return Err("view=trash takes nothing else".into()); }
            return Ok(Target::Trash);
        }
        match (doc, folder) {
            (Some(_), Some(_)) => Err("a link opens a document or a folder, not both".into()),
            (Some(doc), None) => Ok(Target::Doc { top, doc, block }),
            (None, _) if block.is_some() => Err("block needs doc".into()),
            (None, folder) => Ok(Target::Folder { top: top.ok_or("nothing to open: give doc, folder or top")?, folder }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_round_trip() {
        for t in [
            Target::Doc { top: Some("6f1c0d2e-1b7a-4c1e-9f0a-2d3c4b5a6978".into()), doc: "a1b2c3d4-0000-4000-8000-123456789abc".into(), block: Some("#3.1~k7f2".into()) },
            Target::Doc { top: None, doc: "local:abc".into(), block: None },
            Target::Folder { top: "0123456789abcdef".into(), folder: Some("f-1".into()) },
            Target::Folder { top: "k".into(), folder: None },
            Target::Trash,
        ] {
            let url = t.to_url();
            assert!(url.starts_with("quietmd://open?"), "{url}");
            assert_eq!(Target::parse(&url), Ok(t), "{url}");
        }
        assert_eq!(Target::parse("QuietMD://open/?doc=d&block=%232"), Ok(Target::Doc { top: None, doc: "d".into(), block: Some("#2".into()) }));
    }

    #[test]
    fn anything_but_opening_is_refused() {
        for bad in [
            "quietmd://delete?doc=d", "quietmd://open?doc=d&run=x", "quietmd://open?doc=d&doc=e", "quietmd://open?doc=..%2F..%2Fetc", "quietmd://open?doc=a%20b",
            "quietmd://open?doc=d&block=rm%20-rf", "quietmd://open?doc=d&block=%230", "quietmd://open?doc=d&block=%231~toolong", "quietmd://open?block=%231",
            "quietmd://open?doc=d&folder=f", "quietmd://open?", "quietmd://open?doc=%ZZ", "quietmd://open?view=settings", "quietmd://open?view=trash&doc=d", "quietmd://open?doc=%E4", "http://open?doc=d", "file:///tmp/a.md", "quietmd:open?doc=d",
        ] { assert!(Target::parse(bad).is_err(), "{bad}"); }
        assert!(Target::parse(&format!("quietmd://open?doc={}", "a".repeat(3000))).is_err());
    }
}
