use super::*;

/// A real `--detail with-ids` fetch of all-blocks.xml pushed to Feishu
/// (ids and the image token replaced).
const CLOUD: &str = include_str!("../tests/fixtures/feishu-cloud.xml");

fn refs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

fn doc(blocks: &[&str]) -> Document {
    docxml::parse(&format!("<title>t</title>{}", blocks.concat())).unwrap()
}

fn texts(d: &Document) -> Vec<String> {
    d.blocks.iter().map(|b| b.text.text.clone()).collect()
}

/// A cloud document whose block `i` has id `b{i}`.
fn cloud(blocks: &[&str]) -> CloudDoc {
    let d = doc(blocks);
    CloudDoc { revision: 1, title_id: "doc".into(), ids: (0..d.blocks.len()).map(|i| Some(format!("b{i}"))).collect(), doc: d }
}

#[test]
fn cloud_fetch_parses_and_aligns() {
    let media = Media::from([("imgTOKEN1".to_string(), "assets/blue.png".to_string())]);
    let c = CloudDoc::parse(CLOUD, 4, &media).unwrap();
    assert_eq!(c.title_id, "docTOKEN");
    assert_eq!(c.ids.len(), c.doc.blocks.len());
    assert!(c.ids.iter().all(Option::is_some));
    // Each list item is its own block, as in the cloud.
    let ul = c.doc.blocks.iter().filter(|b| matches!(b.kind, BlockKind::Bullet)).count();
    assert_eq!(ul, 3);
    let xml = docxml::serialize(&c.doc);
    assert!(!xml.contains("id=\""), "{xml}");
    assert!(!xml.contains("rgb"), "colors are names again: {xml}");
    assert!(xml.contains("text-color=\"red\"") && xml.contains("background-color=\"light-yellow\""), "{xml}");
    assert!(xml.contains("<callout emoji=\"💡\" background-color=\"light-orange\" border-color=\"orange\">"), "{xml}");
    assert!(xml.contains("<th background-color=\"light-gray\""), "{xml}");
    assert!(xml.contains("caption=\"例\"") && xml.contains("caption=\"蓝图\""), "captions lose the cloud's newline: {xml}");
    assert!(xml.contains("<img path=\"assets/blue.png\""), "{xml}");
    assert!(!xml.contains("seq-marker"), "{xml}");
    assert_eq!(image_tokens(CLOUD), vec!["imgTOKEN1".to_string()]);
}

#[test]
fn compact_has_no_layout_whitespace() {
    let d = docxml::parse(include_str!("../tests/fixtures/all-blocks.xml")).unwrap();
    let s = docxml::serialize_blocks_compact(&d.blocks);
    assert!(!s.contains('\n'));
    assert!(!s.contains(">  <"), "{s}");
    // And it reads back to the same blocks.
    let back = docxml::parse(&format!("<title></title>{s}")).unwrap();
    assert_eq!(keys(&back.blocks), keys(&d.blocks));
}

#[test]
fn merge_one_sided_and_disjoint_changes() {
    let base = doc(&["<p>a</p>", "<p>b</p>", "<p>c</p>", "<p>d</p>"]);
    // Nothing changed.
    let m = merge(&base, &base, &base);
    assert!(!m.local_changed && !m.remote_changed && m.conflicts.is_empty());
    // Local edits b, the cloud deletes d and appends e: both kept.
    let local = doc(&["<p>a</p>", "<p>b!</p>", "<p>c</p>", "<p>d</p>"]);
    let remote = doc(&["<p>a</p>", "<p>b</p>", "<p>c</p>", "<p>e</p>"]);
    let m = merge(&base, &local, &remote);
    assert!(m.conflicts.is_empty());
    assert_eq!(texts(&m.doc), ["a", "b!", "c", "e"]);
    assert!(m.local_changed && m.remote_changed);
    // Inserts at both ends.
    let local = doc(&["<p>0</p>", "<p>a</p>", "<p>b</p>", "<p>c</p>", "<p>d</p>"]);
    let remote = doc(&["<p>a</p>", "<p>b</p>", "<p>c</p>", "<p>d</p>", "<p>z</p>"]);
    assert_eq!(texts(&merge(&base, &local, &remote).doc), ["0", "a", "b", "c", "d", "z"]);
    // The same edit on both sides is not a conflict.
    let same = doc(&["<p>a</p>", "<p>B</p>", "<p>c</p>", "<p>d</p>"]);
    let m = merge(&base, &same, &same);
    assert!(m.conflicts.is_empty());
    assert_eq!(texts(&m.doc), ["a", "B", "c", "d"]);
}

#[test]
fn merge_conflicts_keep_local() {
    let base = doc(&["<p>a</p>", "<p>b</p>", "<p>c</p>"]);
    let local = doc(&["<p>a</p>", "<p>本地</p>", "<p>c</p>"]);
    let remote = doc(&["<p>a</p>", "<p>云端</p>", "<p>c</p>"]);
    let m = merge(&base, &local, &remote);
    assert_eq!(m.conflicts, vec![Conflict { base: 1..2, local: 1..2, remote: 1..2 }]);
    assert_eq!(texts(&m.doc), ["a", "本地", "c"]);
    // Local deletes what the cloud edited: also a conflict.
    let local = doc(&["<p>a</p>", "<p>c</p>"]);
    assert_eq!(merge(&base, &local, &remote).conflicts.len(), 1);
}

#[test]
fn merge_title() {
    let mut base = doc(&["<p>a</p>"]);
    let mut local = base.clone();
    let mut remote = base.clone();
    base.title = RichText::plain("t");
    local.title = RichText::plain("t");
    remote.title = RichText::plain("云端标题");
    let m = merge(&base, &local, &remote);
    assert_eq!(m.doc.title.text, "云端标题");
    assert!(!m.title_conflict && m.remote_changed && !m.local_changed);
    local.title = RichText::plain("本地标题");
    assert!(merge(&base, &local, &remote).title_conflict);
}

/// Runs `ops` on the cloud's (id, key) list the way Feishu does.
fn apply(remote: &CloudDoc, ops: &[Op]) -> Vec<String> {
    let mut v: Vec<(String, String)> = remote.ids.iter().flatten().cloned().zip(keys(&remote.doc.blocks)).collect();
    let mut fresh = 0;
    let mut new = |xml: &str| -> Vec<(String, String)> {
        let d = docxml::parse(&format!("<title></title>{xml}")).unwrap();
        keys(&d.blocks).into_iter().map(|k| { fresh += 1; (format!("new{fresh}"), k) }).collect()
    };
    let pos = |v: &[(String, String)], id: &str| v.iter().position(|(i, _)| i == id).unwrap_or_else(|| panic!("no block {id}"));
    for op in ops {
        match op {
            Op::Title { .. } => {}
            Op::Replace { start, end, xml } => {
                let (a, b) = (pos(&v, start), pos(&v, end));
                v.splice(a..=b, new(xml));
            }
            Op::Delete { start, end } => {
                let (a, b) = (pos(&v, start), pos(&v, end));
                v.drain(a..=b);
            }
            Op::InsertAfter { anchor, xml } => {
                let at = if anchor == "0" { 0 } else { pos(&v, anchor) + 1 };
                v.splice(at..at, new(xml));
            }
        }
    }
    v.into_iter().map(|(_, k)| k).collect()
}

#[test]
fn plan_turns_remote_into_target() {
    let remote = cloud(&["<p>a</p>", "<ul><li>x</li><li>y</li></ul>", "<p>c</p>", "<p>d</p>"]);
    let target = doc(&["<p>0</p>", "<p>a</p>", "<ul><li>x</li><li>Y</li></ul>", "<p>d</p>", "<p>e</p>"]);
    let ops = plan(&remote, &target, &Media::new()).unwrap();
    // Remote b0..b4 = a, x, y, c, d. y and c become Y; 0 and e are new.
    assert_eq!(ops, vec![
        Op::InsertAfter { anchor: "0".into(), xml: "<p>0</p>".into() },
        Op::Replace { start: "b2".into(), end: "b3".into(), xml: "<ul><li>Y</li></ul>".into() },
        Op::InsertAfter { anchor: "b4".into(), xml: "<p>e</p>".into() },
    ]);
    assert_eq!(apply(&remote, &ops), keys(&target.blocks));
    // Untouched blocks are never sent.
    assert!(ops.iter().all(|o| !matches!(o, Op::Replace { xml, .. } | Op::InsertAfter { xml, .. } if xml.contains("<p>a</p>") || xml.contains("<p>d</p>"))));
    // Nothing to do.
    assert!(plan(&remote, &remote.doc, &Media::new()).unwrap().is_empty());
}

#[test]
fn plan_title_and_media() {
    let remote = cloud(&["<p>a</p>"]);
    let mut target = doc(&["<p>a</p>", "<img path=\"assets/old.png\"/>", "<img path=\"assets/new.png\"/>"]);
    target.title = RichText::plain("新标题");
    let media = Media::from([("tokOLD".to_string(), "assets/old.png".to_string())]);
    let ops = plan(&remote, &target, &media).unwrap();
    assert_eq!(ops[0], Op::Title { id: "doc".into(), xml: "<title>新标题</title>".into() });
    let Op::InsertAfter { anchor, xml } = &ops[1] else { panic!("{ops:?}") };
    assert_eq!(anchor, "b0");
    assert!(xml.contains("<img src=\"tokOLD\"") && xml.contains("<img path=\"@./assets/new.png\""), "{xml}");
}

struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

fn random_edit(r: &mut Rng, blocks: &mut Vec<String>, tag: &str) {
    for _ in 0..1 + r.next(4) {
        let i = r.next(blocks.len() + 1);
        match r.next(3) {
            0 => blocks.insert(i, format!("<p>{tag}{}</p>", r.next(1000))),
            1 if i < blocks.len() => {
                blocks.remove(i);
            }
            _ if i < blocks.len() => blocks[i] = format!("<p>{tag}{}</p>", r.next(1000)),
            _ => {}
        }
    }
}

#[test]
fn random_plans_and_merges() {
    for seed in 0..300 {
        let mut r = Rng(seed);
        let n = r.next(12);
        let base: Vec<String> = (0..n).map(|i| if r.next(4) == 0 { format!("<ul><li>i{i}</li></ul>") } else { format!("<p>p{i}</p>") }).collect();
        let (mut l, mut rm) = (base.clone(), base.clone());
        random_edit(&mut r, &mut l, "L");
        random_edit(&mut r, &mut rm, "R");
        let (bd, ld, rd) = (doc(&refs(&base)), doc(&refs(&l)), doc(&refs(&rm)));
        // Planning from any cloud state reaches the target exactly.
        let c = cloud(&refs(&rm));
        let ops = plan(&c, &ld, &Media::new()).unwrap();
        assert_eq!(apply(&c, &ops), keys(&ld.blocks), "seed {seed}: {ops:?}");
        // A merge without conflicts keeps every block either side added.
        let m = merge(&bd, &ld, &rd);
        if m.conflicts.is_empty() {
            let mk = keys(&m.doc.blocks);
            for k in keys(&ld.blocks).iter().chain(&keys(&rd.blocks)) {
                if !keys(&bd.blocks).contains(k) {
                    assert!(mk.contains(k), "seed {seed}: lost {k}");
                }
            }
            // One side unchanged: the merge is the other side.
            if l == base {
                assert_eq!(mk, keys(&rd.blocks), "seed {seed}");
            }
            if rm == base {
                assert_eq!(mk, keys(&ld.blocks), "seed {seed}");
            }
        }
    }
}

#[test]
fn blockers_name_what_the_cloud_lacks() {
    let d = doc(&["<p>ok</p>", "<countdown duration=\"60\" start-time=\"0\" notify=\"false\" color=\"red\" timing=\"false\"/>", "<callout emoji=\"💡\"><p>x</p><pre lang=\"rust\"><code>a</code></pre></callout>"]);
    let b = blockers(&d.blocks);
    assert_eq!(b.len(), 2, "{b:?}");
    assert!(b[0].contains("第 2 块") && b[1].contains("代码块"), "{b:?}");
    assert!(blockers(&doc(&["<callout emoji=\"💡\"><p>x</p><ul><li>y</li></ul></callout>"]).blocks).is_empty());
}

#[test]
fn folding_is_not_a_change() {
    let base = doc(&["<h1>h</h1>", "<p>a</p>"]);
    let local = doc(&["<h1 folded=\"true\">h</h1>", "<p>a</p>"]);
    let remote = doc(&["<h1>h</h1>", "<p>a</p>", "<p>b</p>"]);
    let m = merge(&base, &local, &remote);
    assert!(!m.local_changed && m.remote_changed);
    assert!(m.doc.blocks[0].folded, "the stable block is the local one, fold and all");
    assert_eq!(texts(&m.doc), ["h", "a", "b"]);
}

#[test]
fn neighbouring_edits_settle_block_by_block() {
    // Seen in the app: a clash in block 0 held back the cloud's edit of block 1.
    let base = doc(&["<p>a</p>", "<p>b</p>", "<p>c</p>"]);
    let local = doc(&["<p>a 本地</p>", "<p>b</p>", "<p>c</p>"]);
    let remote = doc(&["<p>a 云端</p>", "<p>b 云端</p>", "<p>c</p>"]);
    let m = merge(&base, &local, &remote);
    assert_eq!(texts(&m.doc), ["a 本地", "b 云端", "c"]);
    assert_eq!(m.conflicts, vec![Conflict { base: 0..1, local: 0..1, remote: 0..1 }]);
    // Different shapes stay one hunk.
    let remote = doc(&["<p>a 云端</p>", "<p>新</p>", "<p>b</p>", "<p>c</p>"]);
    let m = merge(&base, &local, &remote);
    assert_eq!(texts(&m.doc), ["a 本地", "b", "c"]);
    assert_eq!(m.conflicts.len(), 1);
}

#[test]
fn blockers_only_for_what_is_sent() {
    // A callout holding a quote is already in the cloud: pushing other
    // blocks does not send it, so it does not block.
    let callout = "<callout emoji=\"💡\"><blockquote><p>q</p></blockquote></callout>";
    let remote = cloud(&["<p>a</p>", callout, "<p>c</p>"]);
    let target = doc(&["<p>a</p>", callout, "<p>C!</p>"]);
    let (ops, sent) = plan_sent(&remote, &target, &Media::new()).unwrap();
    assert_eq!(ops.len(), 1);
    assert_eq!(sent, vec![2..3]);
    assert!(blockers_in(&target.blocks, &sent).is_empty());
    // The whole document does name it.
    assert_eq!(blockers(&target.blocks).len(), 1);
    // A new callout with a quote is sent, and blocks.
    let target = doc(&["<p>a</p>", callout, "<p>c</p>", callout]);
    let (_, sent) = plan_sent(&remote, &target, &Media::new()).unwrap();
    assert_eq!(sent, vec![3..4]);
    let b = blockers_in(&target.blocks, &sent);
    assert!(b.len() == 1 && b[0].contains("第 4 块"), "{b:?}");
    // Editing the old callout sends it again, and blocks too.
    let edited = "<callout emoji=\"💡\"><blockquote><p>q2</p></blockquote></callout>";
    let target = doc(&["<p>a</p>", edited, "<p>c</p>"]);
    let (_, sent) = plan_sent(&remote, &target, &Media::new()).unwrap();
    assert_eq!(sent, vec![1..2]);
    assert!(blockers_in(&target.blocks, &sent)[0].contains("第 2 块"));
    // Nothing to send, nothing checked.
    let (ops, sent) = plan_sent(&remote, &remote.doc, &Media::new()).unwrap();
    assert!(ops.is_empty() && sent.is_empty());
}
