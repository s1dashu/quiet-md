//! Three-way sync with a Feishu cloud document, without the network: the
//! caller fetches with `lark-cli docs +fetch --api-version v2 --detail
//! with-ids` and runs the [`Op`]s this plans with `docs +update`.
//!
//! Three versions of one document: the **base** (the cloud at the last
//! successful sync), the **local** file and the **remote** (the cloud now).
//! The unit is a top-level block (each list item is one, as in Feishu).
//! Blocks are compared by their compact DocxXML after both sides went
//! through `docxml`, so formatting differences are not changes.
//!
//! Cloud ids live only in the cloud versions: `docxml` drops unknown
//! attributes on known blocks, so the local file never carries them. A
//! push is planned against the remote, whose ids anchor every operation.

use crate::docxml;
use crate::model::*;
use crate::palette;
use anyhow::{Result, anyhow, bail};
use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesStart, Event};
use std::collections::BTreeMap;

/// Cloud image token → library-relative local path (`assets/…`).
pub type Media = BTreeMap<String, String>;

/// A fetched cloud document, cleaned and parsed.
#[derive(Clone, Debug)]
pub struct CloudDoc {
    pub revision: u64,
    /// The `<title>`'s id, which is the document id.
    pub title_id: String,
    /// One per top-level block of `doc`, in order.
    pub ids: Vec<Option<String>>,
    pub doc: Document,
}

impl CloudDoc {
    /// `xml`: the `content` of a `--detail with-ids` fetch.
    pub fn parse(xml: &str, revision: u64, media: &Media) -> Result<CloudDoc> {
        let (clean, title_id, ids) = clean_cloud_xml(xml)?;
        let mut doc = docxml::parse(&clean)?;
        if doc.blocks.len() != ids.len() {
            bail!("云端文档有 {} 个顶层块，解析出 {} 个，无法对齐", ids.len(), doc.blocks.len());
        }
        localize_media(&mut doc.blocks, media);
        Ok(CloudDoc { revision, title_id: title_id.unwrap_or_default(), ids, doc })
    }
}

/// Image tokens in a cloud fetch, for downloading the ones `Media` lacks
/// before [`CloudDoc::parse`].
pub fn image_tokens(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"img" => {
                if let Some(v) = e.attributes().flatten().find(|a| a.key.as_ref() == b"src").and_then(|a| a.unescape_value().ok())
                    && !v.is_empty()
                    && !out.contains(&v.to_string())
                {
                    out.push(v.into_owned());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------- cleaning

/// Drops `id`s (returning the title's and the top-level blocks'), turns
/// the cloud's `rgb()` colors back into DocxXML names and trims the
/// newline the cloud appends to captions.
fn clean_cloud_xml(xml: &str) -> Result<(String, Option<String>, Vec<Option<String>>)> {
    let mut r = Reader::from_str(xml);
    r.config_mut().trim_text(false);
    let mut w = Writer::new(Vec::new());
    let mut stack: Vec<String> = Vec::new();
    let (mut title_id, mut ids) = (None, Vec::new());
    loop {
        let ev = r.read_event().map_err(|e| anyhow!("云端 XML 解析失败：{e}"))?;
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let (el, id) = clean_element(e, &name, stack.last().map(String::as_str))?;
                let parent = stack.last().map(String::as_str);
                let top = stack.is_empty() && name != "ul" && name != "ol";
                let item = stack.len() == 1 && matches!(parent, Some("ul" | "ol")) && name == "li";
                if stack.is_empty() && name == "title" {
                    title_id = id;
                } else if top || item {
                    ids.push(id);
                }
                if matches!(ev, Event::Start(_)) {
                    stack.push(name);
                    w.write_event(Event::Start(el))?;
                } else {
                    w.write_event(Event::Empty(el))?;
                }
            }
            Event::End(e) => {
                stack.pop();
                w.write_event(Event::End(e))?;
            }
            Event::Eof => break,
            other => w.write_event(other)?,
        }
    }
    Ok((String::from_utf8(w.into_inner())?, title_id, ids))
}

fn clean_element(e: &BytesStart, name: &str, parent: Option<&str>) -> Result<(BytesStart<'static>, Option<String>)> {
    let mut el = BytesStart::new(name.to_string());
    let mut id = None;
    for a in e.attributes() {
        let a = a?;
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let mut val = a.unescape_value()?.into_owned();
        match key.as_str() {
            "id" => {
                id = Some(val);
                continue;
            }
            // Feishu's computed numbering, not content.
            "seq-marker" => continue,
            "caption" => val = val.trim_end_matches('\n').to_string(),
            "text-color" | "background-color" | "border-color" => {
                if let Some(n) = color_name(name, parent, &key, &val) {
                    val = n;
                }
            }
            _ => {}
        }
        el.push_attribute((key.as_str(), val.as_str()));
    }
    Ok((el, id))
}

fn parse_rgb(v: &str) -> Option<palette::Rgba> {
    let inner = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb("))?.strip_suffix(')')?;
    let n: Vec<&str> = inner.split(',').map(str::trim).collect();
    let c = |i: usize| n.get(i)?.parse::<u8>().ok();
    let a = match n.get(3) { Some(a) => a.parse().ok()?, None => 1. };
    Some(palette::rgba(c(0)?, c(1)?, c(2)?, a))
}

/// The DocxXML name whose palette value the cloud wrote, for the palette
/// that applies to this tag and attribute.
fn color_name(tag: &str, _parent: Option<&str>, attr: &str, v: &str) -> Option<String> {
    let got = parse_rgb(v)?;
    let names: Vec<&str> = palette::HUES.iter().copied()
        .chain(palette::HUES.iter().map(|h| match *h { "gray" => "light-gray", "red" => "light-red", "orange" => "light-orange", "yellow" => "light-yellow", "green" => "light-green", "blue" => "light-blue", _ => "light-purple" }))
        .chain(["medium-gray"]).collect();
    let tables: &[fn(&str) -> Option<palette::Rgba>] = match (tag, attr) {
        (_, "text-color") => &[palette::text_color],
        ("callout", "background-color") => &[palette::callout_bg, palette::block_bg],
        ("callout", "border-color") => &[palette::callout_border_swatch, palette::callout_border],
        ("span", "background-color") => &[palette::bg_color],
        (_, "background-color") => &[palette::block_bg, palette::bg_color],
        _ => return None,
    };
    let close = |c: palette::Rgba| c.r.abs_diff(got.r) <= 3 && c.g.abs_diff(got.g) <= 3 && c.b.abs_diff(got.b) <= 3 && (c.a - got.a).abs() <= 0.03;
    tables.iter().find_map(|t| names.iter().find(|n| t(n).is_some_and(close)).map(|n| n.to_string()))
}

// ---------------------------------------------------------------- media

fn walk_mut(blocks: &mut [Block], f: &mut dyn FnMut(&mut Block)) {
    for b in blocks {
        f(b);
        if let BlockKind::Table(t) = &mut b.kind {
            for row in &mut t.rows {
                for cell in row {
                    walk_mut(&mut cell.blocks, f);
                }
            }
        }
        walk_mut(&mut b.children, f);
    }
}

/// Cloud image tokens the library has a file for become `path` images.
pub fn localize_media(blocks: &mut [Block], media: &Media) {
    walk_mut(blocks, &mut |b| {
        if let BlockKind::Image { src_kind, src, .. } = &mut b.kind
            && src_kind == "src"
            && let Some(p) = media.get(src.as_str())
        {
            *src_kind = "path".into();
            *src = p.clone();
        }
    });
}

/// For sending: known images go back to their token (the cloud copies
/// it); new local files become `path="@./…"`, which `lark-cli` uploads
/// from the library folder it runs in.
fn cloudify_media(blocks: &mut [Block], media: &Media) {
    walk_mut(blocks, &mut |b| {
        if let BlockKind::Image { src_kind, src, .. } = &mut b.kind
            && src_kind == "path"
        {
            if let Some((t, _)) = media.iter().find(|(_, p)| *p == src) {
                *src_kind = "src".into();
                *src = t.clone();
            } else if !src.starts_with("@./") {
                *src = format!("@./{src}");
            }
        }
    });
}

/// Blocks as they are sent: cloud images, no local folding.
fn for_cloud(blocks: &mut [Block], media: &Media) {
    cloudify_media(blocks, media);
    walk_mut(blocks, &mut |b| b.folded = false);
}

/// All of `doc` as the cloud takes it, for `docs +create`.
pub fn cloud_xml(doc: &Document, media: &Media) -> String {
    let mut sent = doc.blocks.clone();
    for_cloud(&mut sent, media);
    format!("<title>{}</title>{}", docxml::serialize_inline(&doc.title), docxml::serialize_blocks_compact(&sent))
}

// ---------------------------------------------------------------- diff

/// One comparison key per top-level block. Consecutive list items are
/// keyed one by one (each is its own cloud block). Folding is left out:
/// it is a local view state the cloud does not keep.
pub fn keys(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(|b| {
        let mut b = [b.clone()];
        walk_mut(&mut b, &mut |b| b.folded = false);
        docxml::serialize_blocks_compact(&b)
    }).collect()
}

/// Longest common subsequence: matched index pairs, increasing.
fn lcs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let pre = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (am, bm) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut out: Vec<(usize, usize)> = (0..pre).map(|i| (i, i)).collect();
    // The middle is what changed; quadratic is fine unless both sides
    // rewrote thousands of blocks, and then it is all one change.
    if !am.is_empty() && !bm.is_empty() && am.len() * bm.len() <= 4_000_000 {
        let (n, m) = (am.len(), bm.len());
        let mut t = vec![0u32; (n + 1) * (m + 1)];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                t[i * (m + 1) + j] = if am[i] == bm[j] { t[(i + 1) * (m + 1) + j + 1] + 1 } else { t[(i + 1) * (m + 1) + j].max(t[i * (m + 1) + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if am[i] == bm[j] {
                out.push((pre + i, pre + j));
                i += 1;
                j += 1;
            } else if t[(i + 1) * (m + 1) + j] >= t[i * (m + 1) + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }
    out.extend((0..suf).map(|k| (a.len() - suf + k, b.len() - suf + k)));
    out
}

// ---------------------------------------------------------------- merge

#[derive(Clone, Debug, PartialEq)]
pub struct Conflict {
    /// Base block range both sides changed differently.
    pub base: std::ops::Range<usize>,
    pub local: std::ops::Range<usize>,
    pub remote: std::ops::Range<usize>,
}

#[derive(Clone, Debug)]
pub struct Merge {
    /// Base, local and remote merged; on conflicts the local side is kept
    /// there and the conflicts are listed.
    pub doc: Document,
    pub local_changed: bool,
    pub remote_changed: bool,
    pub conflicts: Vec<Conflict>,
    pub title_conflict: bool,
}

/// diff3 over top-level blocks. Between blocks unchanged on both sides,
/// a stretch changed on one side takes that side; changed the same way
/// on both, either; changed differently, a conflict.
pub fn merge(base: &Document, local: &Document, remote: &Document) -> Merge {
    let (kb, kl, kr) = (keys(&base.blocks), keys(&local.blocks), keys(&remote.blocks));
    let ml: BTreeMap<usize, usize> = lcs(&kb, &kl).into_iter().collect();
    let mr: BTreeMap<usize, usize> = lcs(&kb, &kr).into_iter().collect();
    let mut blocks = Vec::new();
    let mut conflicts = Vec::new();
    let (mut ib, mut il, mut ir) = (0, 0, 0);
    let stable: Vec<(usize, usize, usize)> = kb.iter().enumerate().filter_map(|(b, _)| Some((b, *ml.get(&b)?, *mr.get(&b)?))).chain([(kb.len(), kl.len(), kr.len())]).collect();
    for (sb, sl, sr) in stable {
        if sl < il || sr < ir {
            continue;
        }
        let (cb, cl, cr) = (&kb[ib..sb], &kl[il..sl], &kr[ir..sr]);
        if cl == cb {
            blocks.extend_from_slice(&remote.blocks[ir..sr]);
        } else if cr == cb || cl == cr {
            blocks.extend_from_slice(&local.blocks[il..sl]);
        } else if cb.len() == cl.len() && cb.len() == cr.len() {
            // Same shape on all sides (edits in place): settle it block by
            // block, so a clash in one block does not hold back its neighbours.
            for k in 0..cb.len() {
                let (b, l, r) = (&cb[k], &cl[k], &cr[k]);
                if l == b {
                    blocks.push(remote.blocks[ir + k].clone());
                } else {
                    if r != b && l != r {
                        conflicts.push(Conflict { base: ib + k..ib + k + 1, local: il + k..il + k + 1, remote: ir + k..ir + k + 1 });
                    }
                    blocks.push(local.blocks[il + k].clone());
                }
            }
        } else {
            conflicts.push(Conflict { base: ib..sb, local: il..sl, remote: ir..sr });
            blocks.extend_from_slice(&local.blocks[il..sl]);
        }
        if sb < kb.len() {
            blocks.push(local.blocks[sl].clone());
        }
        (ib, il, ir) = (sb + 1, sl + 1, sr + 1);
    }
    let t = |d: &Document| docxml::serialize_inline(&d.title);
    let (tb, tl, tr) = (t(base), t(local), t(remote));
    let title_conflict = tl != tb && tr != tb && tl != tr;
    let title = if tl == tb { remote.title.clone() } else { local.title.clone() };
    let doc = Document { title, icon: local.icon.clone(), cover: local.cover.clone(), blocks };
    Merge { local_changed: kl != kb || tl != tb, remote_changed: kr != kb || tr != tb, conflicts, title_conflict, doc }
}

// ---------------------------------------------------------------- push

/// One `lark-cli docs +update --command …` call.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `block_replace` on the title block (the document id).
    Title { id: String, xml: String },
    /// `block_replace --start-block-id --end-block-id`.
    Replace { start: String, end: String, xml: String },
    /// `block_insert_after --block-id` (`0`: the document start).
    InsertAfter { anchor: String, xml: String },
    /// `block_delete --start-block-id --end-block-id`.
    Delete { start: String, end: String },
}

/// Turns `remote` into `target` with block operations, anchored on the
/// remote's ids. Untouched blocks are not sent, so their comments and
/// cloud-only attributes stay. Ops are in document order; each one only
/// uses ids of blocks no earlier op touched.
pub fn plan(remote: &CloudDoc, target: &Document, media: &Media) -> Result<Vec<Op>> {
    Ok(plan_sent(remote, target, media)?.0)
}

/// [`plan`], and the ranges of `target` blocks its ops send (indices into
/// `target.blocks`): the blocks the cloud has to accept, for [`blockers_in`].
pub fn plan_sent(remote: &CloudDoc, target: &Document, media: &Media) -> Result<(Vec<Op>, Vec<std::ops::Range<usize>>)> {
    let mut out = Vec::new();
    let mut ranges = Vec::new();
    let mut sent = target.clone();
    for_cloud(&mut sent.blocks, media);
    let (kr, kt) = (keys(&remote.doc.blocks), keys(&target.blocks));
    if docxml::serialize_inline(&remote.doc.title) != docxml::serialize_inline(&target.title) {
        out.push(Op::Title { id: remote.title_id.clone(), xml: format!("<title>{}</title>", docxml::serialize_inline(&target.title)) });
    }
    let id = |i: usize| remote.ids[i].clone().ok_or_else(|| anyhow!("云端第 {} 个块没有 id", i + 1));
    let (mut ir, mut it) = (0, 0);
    for (sr, st) in lcs(&kr, &kt).into_iter().chain([(kr.len(), kt.len())]) {
        let xml = docxml::serialize_blocks_compact(&sent.blocks[it..st]);
        if it < st {
            ranges.push(it..st);
        }
        match (ir < sr, it < st) {
            (true, true) => out.push(Op::Replace { start: id(ir)?, end: id(sr - 1)?, xml }),
            (true, false) => out.push(Op::Delete { start: id(ir)?, end: id(sr - 1)? }),
            (false, true) => out.push(Op::InsertAfter { anchor: if ir == 0 { "0".into() } else { id(ir - 1)? }, xml }),
            (false, false) => {}
        }
        (ir, it) = (sr + 1, st + 1);
    }
    Ok((out, ranges))
}

/// What the cloud cannot hold, by block: pushing is refused until these
/// are gone (quiet.md's own blocks, and Feishu's limits on callouts).
pub fn blockers(blocks: &[Block]) -> Vec<String> {
    check_blockers(blocks, |_| true)
}

/// [`blockers`] for the blocks in `ranges` only (indices into `blocks`,
/// numbered as in the document). Blocks a push does not send are not checked.
pub fn blockers_in(blocks: &[Block], ranges: &[std::ops::Range<usize>]) -> Vec<String> {
    check_blockers(blocks, |i| ranges.iter().any(|r| r.contains(&i)))
}

fn check_blockers(blocks: &[Block], checked: impl Fn(usize) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if !checked(i) {
            continue;
        }
        let n = i + 1;
        let xml = docxml::serialize_blocks_compact(std::slice::from_ref(b));
        if xml.contains("<countdown") {
            out.push(format!("第 {n} 块：倒计时只在 Quiet.md 里有，飞书文档里没有对应的块"));
        }
        if xml.contains("is-whole-day=\"true\"") {
            out.push(format!("第 {n} 块：全天提醒只在 Quiet.md 里有"));
        }
        if matches!(b.kind, BlockKind::Callout { .. })
            && let Some(c) = b.children.iter().find(|c| !matches!(c.kind, BlockKind::Text | BlockKind::Bullet | BlockKind::Ordered { .. } | BlockKind::Todo { .. }))
        {
            out.push(format!("第 {n} 块：飞书的高亮块里只能放正文、列表和待办，不能放{}", kind_name(&c.kind)));
        }
    }
    out
}

fn kind_name(k: &BlockKind) -> &'static str {
    match k {
        BlockKind::Heading { .. } => "标题",
        BlockKind::Code { .. } => "代码块",
        BlockKind::Quote => "引用",
        BlockKind::Divider => "分割线",
        BlockKind::Image { .. } => "图片",
        BlockKind::Table(_) => "表格",
        BlockKind::Grid | BlockKind::Column { .. } => "分栏",
        BlockKind::File(_) => "文件",
        _ => "这种块",
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
