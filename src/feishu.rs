//! 飞书同步: a document linked to a Feishu cloud document, kept in step by
//! `lark-cli` and the three-way merge in `fdl_core::sync`.
//!
//! Per linked document the library keeps `sync/<id>.json` (the link, the
//! cloud revision last seen, cloud image token → local file) and
//! `sync/<id>.base.xml`: the cloud as of the last sync, in local DocxXML,
//! which is the base of the next merge. Older builds ignore the folder.
//!
//! Every sync starts from a fresh fetch. 拉取 merges the cloud into the
//! open document (a clash keeps the local side and saves the cloud side as
//! a 冲突副本 under it); 推送 does that first, then sends what the cloud
//! lacks as block operations and fetches again for the new base. The two
//! 覆盖 commands take one side whole.

use crate::app::Workspace;
use crate::model::{BlockKind, Document, RichText};
use crate::store::Store;
use crate::sync::{self, CloudDoc, Media, Op};
use crate::ui::ToastKind;
use anyhow::{Context as _, Result, anyhow, bail};
use gpui::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- state

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Link {
    pub url: String,
    /// The docx document token (for a 知识库 link, the document it resolves to).
    pub token: String,
    /// The 知识库 node token the link was pasted with (empty for a docx link).
    #[serde(default)]
    pub wiki_node: String,
    /// Cloud revision at the last sync (0: never synced, no base yet).
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub media: Media,
    /// When the last sync finished.
    #[serde(default)]
    pub synced: u64,
}

fn sync_dir(root: &Path) -> PathBuf {
    root.join("sync")
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn link_of(root: &Path, id: &str) -> Option<Link> {
    serde_json::from_slice(&std::fs::read(sync_dir(root).join(format!("{id}.json"))).ok()?).ok()
}

fn save_link(root: &Path, id: &str, link: &Link) -> Result<()> {
    write_atomic(&sync_dir(root).join(format!("{id}.json")), &serde_json::to_vec_pretty(link)?)
}

fn base_of(root: &Path, id: &str) -> Option<Document> {
    crate::docxml::parse(&std::fs::read_to_string(sync_dir(root).join(format!("{id}.base.xml"))).ok()?).ok()
}

fn save_base(root: &Path, id: &str, doc: &Document) -> Result<()> {
    write_atomic(&sync_dir(root).join(format!("{id}.base.xml")), crate::docxml::serialize(doc).as_bytes())
}

pub fn unlink(root: &Path, id: &str) {
    for ext in ["json", "base.xml"] {
        let _ = std::fs::remove_file(sync_dir(root).join(format!("{id}.{ext}")));
    }
}

/// What a pasted link points at.
#[derive(Clone, Debug, PartialEq)]
pub enum Pasted {
    /// A docx token (`/docx/…`, or a bare token).
    Docx(String),
    /// A 知识库 node token (`/wiki/…`), resolved by `resolve_wiki`.
    Wiki(String),
}

/// What a pasted link points at. No network: a wiki node is resolved later.
pub fn parse_link(url: &str) -> Result<Pasted> {
    let url = url.trim();
    let tail = |key: &str| url.split(key).nth(1).map(|t| t.split(['?', '#', '/']).next().unwrap_or("").to_string());
    let token_ok = |t: &String| t.len() >= 20 && t.chars().all(|c| c.is_ascii_alphanumeric());
    if url.contains("/wiki/") {
        return match tail("/wiki/") {
            Some(t) if token_ok(&t) => Ok(Pasted::Wiki(t)),
            _ => bail!("没有识别出飞书知识库链接"),
        };
    }
    let token = match tail("/docx/") {
        Some(t) => t,
        None if url.contains("/docs/") || url.contains("/sheets/") || url.contains("/base/") => bail!("只支持新版飞书文档（链接里带 /docx/）"),
        None => url.to_string(),
    };
    if !token_ok(&token) {
        bail!("没有识别出飞书文档链接");
    }
    Ok(Pasted::Docx(token))
}

/// The docx token of a pasted docx link (or a bare token).
pub fn token_of(url: &str) -> Result<String> {
    match parse_link(url)? {
        Pasted::Docx(t) => Ok(t),
        Pasted::Wiki(_) => bail!("这里需要云空间里的文档链接"),
    }
}

/// The docx behind a 知识库 node, from `wiki +node-get`. A shortcut is
/// followed to its origin; other kinds of document are refused.
fn resolve_wiki(root: &Path, node: &str) -> Result<String> {
    let get = |n: &str| -> Result<serde_json::Value> { Ok(lark(root, &["wiki", "+node-get", "--node-token", n]).map_err(|e| anyhow!("找不到这篇知识库文档，或没有访问权限（{e}）"))?["data"].clone()) };
    let mut d = get(node)?;
    if d["node_type"].as_str() == Some("shortcut") {
        let origin = d["origin_node_token"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("知识库快捷方式没有指向原文档"))?;
        d = get(origin)?;
    }
    let kind = d["obj_type"].as_str().unwrap_or_default();
    if kind != "docx" {
        bail!("知识库里的这篇文档是{}（{kind}），目前只支持新版文档（docx）", wiki_kind(kind));
    }
    d["obj_token"].as_str().filter(|s| !s.is_empty()).map(str::to_string).ok_or_else(|| anyhow!("知识库没有返回文档的标识"))
}

fn wiki_kind(t: &str) -> &str {
    match t {
        "sheet" => "表格",
        "bitable" => "多维表格",
        "mindnote" => "思维笔记",
        "doc" => "旧版文档",
        "file" => "文件",
        "slides" => "幻灯片",
        _ => "其他类型",
    }
}

// ---------------------------------------------------------------- lark-cli

/// One `lark-cli … --as user` call from the library folder (image uploads
/// name files relative to it). Returns the reply's JSON.
fn lark(root: &Path, args: &[&str]) -> Result<serde_json::Value> {
    let bin = std::env::var("QUIET_MD_LARK_CLI").unwrap_or_else(|_| "lark-cli".into());
    // An app started from Finder has a bare PATH; lark-cli is a node script.
    let mut path = std::env::var("PATH").unwrap_or_default();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if !path.split(':').any(|p| p == extra) {
            path.push(':');
            path.push_str(extra);
        }
    }
    let out = std::process::Command::new(&bin)
        .args(args)
        .args(["--as", "user"])
        .current_dir(root)
        .env("PATH", path)
        .output()
        .map_err(|e| anyhow!("无法运行 lark-cli（{e}），请先安装并登录：npm i -g @larksuite/cli，lark-cli auth login"))?;
    let reply: Option<serde_json::Value> = serde_json::from_slice(&out.stdout).ok();
    match reply {
        Some(v) if out.status.success() && v["ok"].as_bool() != Some(false) => Ok(v),
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            // lark-cli prints a failure as JSON on stderr; older versions used stdout.
            let msg = [&out.stdout, &out.stderr].into_iter().filter_map(|b| serde_json::from_slice::<serde_json::Value>(b).ok())
                .find_map(|v| ["/error/message", "/error/msg", "/msg", "/message"].iter().find_map(|p| v.pointer(p)?.as_str().map(str::to_string)))
                .or_else(|| err.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
                .unwrap_or_else(|| format!("退出码 {}", out.status));
            bail!("lark-cli {}：{msg}", args.iter().take(2).copied().collect::<Vec<_>>().join(" "))
        }
    }
}

/// Image files a push uploads (local paths with no cloud token yet).
fn new_images(doc: &Document, media: &Media) -> Vec<String> {
    doc.asset_sources().into_iter().filter(|s| s.contains('/') && !s.contains(':') && !media.values().any(|p| p == s)).collect()
}

/// `docs +media-download` for each token, four at a time. Replies come
/// back in the order of `tokens`; the first failure is the error.
fn download(root: &Path, tokens: &[String]) -> Result<Vec<serde_json::Value>> {
    const WORKERS: usize = 4;
    let mut got: Vec<(usize, Result<serde_json::Value>)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..WORKERS.min(tokens.len()))
            .map(|w| {
                s.spawn(move || {
                    (w..tokens.len()).step_by(WORKERS).map(|i| {
                        let out = format!("assets/feishu-{}", tokens[i]);
                        (i, lark(root, &["docs", "+media-download", "--token", &tokens[i], "--output", &out, "--overwrite"]))
                    }).collect::<Vec<_>>()
                })
            })
            .collect();
        // A panicked download must not shift the replies: propagate it.
        workers.into_iter().flat_map(|h| h.join().expect("media download thread")).collect()
    });
    got.sort_by_key(|(i, _)| *i);
    got.into_iter().map(|(_, r)| r).collect()
}

/// Fetch the cloud document. Images the library lacks are downloaded to
/// `assets/`; one identical to a file just `uploaded` maps to that file.
fn fetch(root: &Path, token: &str, media: &mut Media, uploaded: &[String]) -> Result<CloudDoc> {
    let v = lark(root, &["docs", "+fetch", "--api-version", "v2", "--doc", token, "--detail", "with-ids"])?;
    let d = &v["data"]["document"];
    let xml = d["content"].as_str().ok_or_else(|| anyhow!("飞书返回的内容里没有文档正文"))?;
    let revision = d["revision_id"].as_u64().unwrap_or(0);
    let todo: Vec<String> = sync::image_tokens(xml).into_iter().filter(|t| !media.contains_key(t)).collect();
    let replies = download(root, &todo)?;
    for (t, v) in todo.into_iter().zip(replies) {
        let out = format!("assets/feishu-{t}");
        let saved = v["data"]["saved_path"].as_str().map(PathBuf::from).unwrap_or_else(|| root.join(&out));
        let saved = saved.canonicalize().unwrap_or(saved);
        let rel = saved.strip_prefix(root.canonicalize().unwrap_or_else(|_| root.to_path_buf())).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| saved.to_string_lossy().into_owned());
        let bytes = std::fs::read(&saved).ok();
        let same = uploaded.iter().find(|p| bytes.is_some() && std::fs::read(root.join(p)).ok() == bytes);
        if let Some(p) = same {
            let _ = std::fs::remove_file(&saved);
            media.insert(t, p.clone());
        } else {
            media.insert(t, rel);
        }
    }
    CloudDoc::parse(xml, revision, media)
}

/// Run `ops` against the cloud document, in order.
fn send(root: &Path, token: &str, ops: &[Op]) -> Result<()> {
    for op in ops {
        let mut a: Vec<&str> = vec!["docs", "+update", "--api-version", "v2", "--doc", token, "--command"];
        match op {
            Op::Title { id, xml } => a.extend(["block_replace", "--block-id", id, "--content", xml]),
            Op::Replace { start, end, xml } => a.extend(["block_replace", "--start-block-id", start, "--end-block-id", end, "--content", xml]),
            Op::InsertAfter { anchor, xml } => a.extend(["block_insert_after", "--block-id", anchor, "--content", xml]),
            Op::Delete { start, end } => a.extend(["block_delete", "--start-block-id", start, "--end-block-id", end]),
        }
        lark(root, &a)?;
    }
    Ok(())
}

/// A new cloud document holding `doc`: (token, url).
fn create(root: &Path, id: &str, doc: &Document, media: &Media) -> Result<(String, String)> {
    let file = format!("sync/{id}.create.xml");
    write_atomic(&root.join(&file), sync::cloud_xml(doc, media).as_bytes())?;
    let r = lark(root, &["docs", "+create", "--content", &format!("@{file}")]);
    let _ = std::fs::remove_file(root.join(&file));
    let v = r?;
    let find = |key: &str| -> Option<String> {
        fn walk(v: &serde_json::Value, key: &str) -> Option<String> {
            match v {
                serde_json::Value::Object(m) => m.get(key).and_then(|x| x.as_str().map(str::to_string)).or_else(|| m.values().find_map(|x| walk(x, key))),
                serde_json::Value::Array(a) => a.iter().find_map(|x| walk(x, key)),
                _ => None,
            }
        }
        walk(&v["data"], key)
    };
    let url = find("url").unwrap_or_default();
    let token = find("document_id").or_else(|| token_of(&url).ok()).ok_or_else(|| anyhow!("飞书没有返回新文档的链接"))?;
    Ok((token, url))
}

// ---------------------------------------------------------------- sync

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 拉取云端更新.
    Pull,
    /// 推送到飞书 (after a pull).
    Push,
    /// 用云端覆盖本地.
    TakeCloud,
    /// 用本地覆盖云端.
    TakeLocal,
    /// 上传为新的飞书文档.
    Create,
}

/// A document with nothing typed in it (a link to it just pulls).
pub fn is_blank(doc: &Document) -> bool {
    doc.title.text.trim().is_empty() && doc.blocks.iter().all(|b| b.children.is_empty() && b.text.text.trim().is_empty() && matches!(b.kind, BlockKind::Text))
}

/// Between the notes of one toast.
fn sep() -> &'static str {
    if crate::settings::english() { "; " } else { "；" }
}

/// Refuses what the cloud cannot hold (`blockers`: the problems found).
fn refuse(blockers: Vec<String>) -> Result<()> {
    if blockers.is_empty() {
        return Ok(());
    }
    bail!("文档里有飞书不支持的内容，未推送：{}", blockers.join("、"))
}

impl Workspace {
    /// The document as it stands now (the open editor's copy, unsaved edits too).
    fn sync_doc_now(&mut self, id: &str, cx: &mut Context<Self>) -> Result<Document> {
        if self.current.as_deref() == Some(id)
            && let Some(ed) = &self.editor
        {
            return Ok(ed.read(cx).core.doc.clone());
        }
        self.store.load(id)
    }

    /// Replace the document's contents (one undo step when it is open).
    fn sync_put_doc(&mut self, id: &str, doc: Document, window: &mut Window, cx: &mut Context<Self>) -> Result<()> {
        if self.current.as_deref() == Some(id)
            && let Some(ed) = self.editor.clone()
        {
            ed.update(cx, |e, cx| {
                e.core.replace_document(doc);
                // 阅读模式 would put the old contents back.
                if e.read_only {
                    e.accept_read_only_contents();
                }
                e.changed(window, cx);
            });
            return Ok(());
        }
        let meta = self.store.save(id, &doc)?;
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.title = doc.title.text.clone();
            e.body = doc.plain_text();
            e.meta = meta;
        }
        Ok(())
    }

    /// `doc` (the cloud side of a clash) as a page under `id`.
    fn sync_conflict_copy(&mut self, id: &str, mut doc: Document) -> Option<String> {
        let (_, mo, d, h, mi) = crate::time::civil(crate::time::now());
        let title = crate::settings::display_title(&doc.title.text);
        doc.title = RichText::plain(crate::localized_format!("{}（云端版本 {:02}-{:02} {:02}:{:02}）", "{} (cloud version {:02}-{:02} {:02}:{:02})", title, mo, d, h, mi));
        let copy = self.store.create_from(&doc).ok()?;
        let _ = self.store.set_parent(&copy, Some(id));
        self.entries = self.store.list();
        self.sort();
        Some(copy)
    }

    /// Start a sync of `id`. `quiet` (the pull on opening) only speaks up
    /// when something came in or went wrong.
    pub fn feishu_sync(&mut self, id: &str, mode: Mode, quiet: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.doc_menu = false;
        self.doc_sub = None;
        if !self.feishu_busy.insert(id.to_string()) {
            return;
        }
        if !quiet {
            self.show_toast_kind(ToastKind::Info, crate::settings::text("正在同步飞书文档…"), cx);
        }
        cx.notify();
        let id = id.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let r = Self::feishu_run(this.clone(), id.clone(), mode, cx).await;
            log::info!("feishu sync {mode:?} {id}: {r:?}");
            if std::env::var_os("FDL_SCRIPT").is_some() {
                eprintln!("feishu sync {mode:?}: {r:?}");
            }
            let _ = this.update(cx, |w, cx| {
                w.feishu_busy.remove(&id);
                match r {
                    Ok(Some(msg)) => w.show_toast_kind(ToastKind::Info, &msg, cx),
                    Ok(None) if !quiet => w.show_toast_kind(ToastKind::Info, crate::settings::text("已是最新"), cx),
                    Ok(None) => {}
                    Err(e) => w.show_toast_kind(ToastKind::Error, &crate::localized_format!("飞书同步失败：{}", "Feishu sync failed: {}", e), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The steps of a sync; the message to show, if any.
    async fn feishu_run(this: WeakEntity<Self>, id: String, mode: Mode, cx: &mut AsyncWindowContext) -> Result<Option<String>> {
        let root = Store::root_dir();
        let bg = cx.background_executor().clone();
        let mut notes: Vec<String> = vec![];
        let (mut link, target, remote) = if mode == Mode::Create {
            let doc = this.update(cx, |w, cx| w.sync_doc_now(&id, cx))??;
            refuse(sync::blockers(&doc.blocks))?;
            let (r, i, d) = (root.clone(), id.clone(), doc.clone());
            let (token, url) = bg.spawn(async move { create(&r, &i, &d, &Media::new()) }).await?;
            (Link { url, token, ..Default::default() }, doc, None)
        } else {
            let link = link_of(&root, &id).ok_or_else(|| anyhow!("这篇文档没有关联飞书文档"))?;
            let (r, t, mut media) = (root.clone(), link.token.clone(), link.media.clone());
            let remote = bg.spawn(async move { fetch(&r, &t, &mut media, &[]).map(|c| (c, media)) }).await?;
            let (remote, media) = remote;
            let mut link = Link { media, ..link };
            let local = this.update(cx, |w, cx| w.sync_doc_now(&id, cx))??;
            let target = match mode {
                Mode::TakeCloud => {
                    this.update_in(cx, |w, window, cx| w.sync_put_doc(&id, remote.doc.clone(), window, cx))??;
                    Self::sync_done(&root, &id, &mut link, &remote)?;
                    return Ok(Some(crate::settings::text("已用云端版本覆盖本地").to_string()));
                }
                Mode::TakeLocal => local,
                _ => {
                    let base = match base_of(&root, &id) {
                        Some(b) if link.revision > 0 => b,
                        _ if is_blank(&local) => {
                            this.update_in(cx, |w, window, cx| w.sync_put_doc(&id, remote.doc.clone(), window, cx))??;
                            Self::sync_done(&root, &id, &mut link, &remote)?;
                            return Ok(Some(crate::settings::text("已从飞书拉取文档").to_string()));
                        }
                        _ => bail!("还没有同步过：请先选择“用云端覆盖本地”或“用本地覆盖云端”"),
                    };
                    let m = sync::merge(&base, &local, &remote.doc);
                    let clashes = m.conflicts.len() + m.title_conflict as usize;
                    if m.remote_changed {
                        let doc = m.doc.clone();
                        this.update_in(cx, |w, window, cx| w.sync_put_doc(&id, doc, window, cx))??;
                        notes.push(crate::settings::text("已合并云端的改动").to_string());
                    }
                    if clashes > 0 {
                        let copy = remote.doc.clone();
                        let made = this.update(cx, |w, _| w.sync_conflict_copy(&id, copy))?;
                        notes.push(match made {
                            Some(_) => crate::localized_format!("{} 处改动两边都改过：保留了本地的，云端版本另存为子页面", "Edited on both sides in {} places: kept the local version, saved the cloud version as a sub-page", clashes),
                            None => crate::localized_format!("{} 处改动两边都改过：保留了本地的，云端版本副本未能保存", "Edited on both sides in {} places: kept the local version, but the cloud copy could not be saved", clashes),
                        });
                    }
                    Self::sync_done(&root, &id, &mut link, &remote)?;
                    if mode == Mode::Pull || clashes > 0 {
                        if mode == Mode::Push {
                            notes.push(crate::settings::text("推送已暂停，核对后再推送一次").to_string());
                        }
                        return Ok((!notes.is_empty()).then(|| notes.join(sep())));
                    }
                    this.update(cx, |w, cx| w.sync_doc_now(&id, cx))??
                }
            };
            // Only the blocks a push sends: the rest are already in the cloud.
            let (_, sent) = sync::plan_sent(&remote, &target, &link.media)?;
            refuse(sync::blockers_in(&target.blocks, &sent))?;
            (link, target, Some(remote))
        };
        // Send what the cloud lacks, then read it back as the new base.
        let (r, t, media, sent) = (root.clone(), link.token.clone(), link.media.clone(), target.clone());
        let before = remote.as_ref().map(|c| c.revision);
        let pushed = bg.spawn(async move {
            let mut media = media;
            let mut n = 0;
            if let Some(remote) = remote {
                let ops = sync::plan(&remote, &sent, &media)?;
                if ops.is_empty() {
                    return Ok::<_, anyhow::Error>(None);
                }
                send(&r, &t, &ops)?;
                n = ops.len();
            }
            let up = new_images(&sent, &media);
            let after = fetch(&r, &t, &mut media, &up)?;
            Ok(Some((after, media, n)))
        });
        let Some((after, media, n)) = pushed.await? else {
            return Ok((!notes.is_empty()).then(|| notes.join(sep())));
        };
        link.media = media;
        let local = this.update(cx, |w, cx| w.sync_doc_now(&id, cx))??;
        let m = sync::merge(&target, &local, &after.doc);
        if m.remote_changed {
            this.update_in(cx, |w, window, cx| w.sync_put_doc(&id, m.doc, window, cx))??;
        }
        Self::sync_done(&root, &id, &mut link, &after)?;
        notes.push(match mode {
            Mode::Create => crate::settings::text("已上传为新的飞书文档").to_string(),
            _ => crate::localized_format!("已推送 {} 处改动到飞书", "Pushed {} changes to Feishu", n),
        });
        if before.is_some_and(|b| after.revision != b + n as u64) {
            notes.push(crate::settings::text("推送期间云端另有改动，已一并合并").to_string());
        }
        Ok(Some(notes.join(sep())))
    }

    /// Record `cloud` as the new base.
    fn sync_done(root: &Path, id: &str, link: &mut Link, cloud: &CloudDoc) -> Result<()> {
        save_base(root, id, &cloud.doc).context("无法保存同步记录")?;
        link.revision = cloud.revision.max(1);
        link.synced = crate::time::now();
        save_link(root, id, link).context("无法保存同步记录")
    }

    /// 关联飞书文档: remember the link, then take the cloud (an empty
    /// document) or the side the user chose. A 知识库 link is resolved in
    /// the background first, and `Ok(true)` says the dialog waits for it.
    pub fn feishu_link(&mut self, id: &str, url: &str, mode: Mode, window: &mut Window, cx: &mut Context<Self>) -> Result<bool> {
        let pasted = parse_link(url)?;
        let url = if url.trim().starts_with("http") { url.trim().to_string() } else { String::new() };
        let node = match pasted {
            Pasted::Docx(token) => {
                save_link(&Store::root_dir(), id, &Link { url, token, ..Default::default() })?;
                self.feishu_sync(id, mode, false, window, cx);
                return Ok(false);
            }
            Pasted::Wiki(node) => node,
        };
        let (root, id) = (Store::root_dir(), id.to_string());
        cx.spawn_in(window, async move |this, cx| {
            let bg = cx.background_executor().clone();
            let (r, n) = (root.clone(), node.clone());
            let linked = match bg.spawn(async move { resolve_wiki(&r, &n) }).await {
                Ok(token) => save_link(&root, &id, &Link { url, token, wiki_node: node, ..Default::default() }),
                Err(e) => Err(e),
            };
            let _ = this.update_in(cx, |w, window, cx| match linked {
                Ok(()) => {
                    w.feishu_dlg = None;
                    w.feishu_sync(&id, mode, false, window, cx);
                }
                Err(e) => w.feishu_link_failed(&e, cx),
            });
        })
        .detach();
        Ok(true)
    }

    /// Show `e` in the link dialog, which stays open for another try.
    fn feishu_link_failed(&mut self, e: &anyhow::Error, cx: &mut Context<Self>) {
        if let Some(d) = self.feishu_dlg.as_mut() {
            d.busy = false;
            d.error = Some(crate::settings::diagnostic(&e.to_string()));
        }
        cx.notify();
    }

    pub fn feishu_unlink(&mut self, id: &str, cx: &mut Context<Self>) {
        unlink(&Store::root_dir(), id);
        self.doc_menu = false;
        self.doc_sub = None;
        self.show_toast_kind(ToastKind::Info, crate::settings::text("已取消关联飞书文档"), cx);
        cx.notify();
    }
}

// ---------------------------------------------------------------- ui

/// The 关联飞书文档 dialog.
pub struct LinkDialog {
    pub id: String,
    pub field: Entity<crate::input::TextField>,
    /// The document is empty: linking just pulls.
    pub blank: bool,
    /// A 知识库 link is being resolved.
    pub busy: bool,
    pub error: Option<String>,
    _subs: Vec<Subscription>,
}

impl Workspace {
    pub fn open_feishu_link(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.doc_menu = false;
        self.doc_sub = None;
        let blank = self.sync_doc_now(id, cx).is_ok_and(|d| is_blank(&d));
        let field = cx.new(|cx| crate::input::TextField::new("粘贴飞书文档或知识库链接", cx));
        window.focus(&field.focus_handle(cx));
        let subs = vec![cx.subscribe_in(&field, window, |w, _, ev: &crate::input::TextFieldEvent, window, cx| match ev {
            crate::input::TextFieldEvent::Submit if w.feishu_dlg.as_ref().is_some_and(|d| d.blank) => w.feishu_link_submit(Mode::TakeCloud, window, cx),
            crate::input::TextFieldEvent::Cancel => {
                w.feishu_dlg = None;
                cx.notify();
            }
            _ => {}
        })];
        self.feishu_dlg = Some(LinkDialog { id: id.to_string(), field, blank, busy: false, error: None, _subs: subs });
        cx.notify();
    }

    fn feishu_link_submit(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.feishu_dlg else { return };
        if d.busy {
            return;
        }
        let (id, url) = (d.id.clone(), d.field.read(cx).text.clone());
        match self.feishu_link(&id, &url, mode, window, cx) {
            Ok(false) => self.feishu_dlg = None,
            Ok(true) => {
                if let Some(d) = self.feishu_dlg.as_mut() {
                    d.busy = true;
                    d.error = None;
                }
            }
            Err(e) => {
                if let Some(d) = self.feishu_dlg.as_mut() {
                    d.error = Some(crate::settings::diagnostic(&e.to_string()));
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn render_feishu_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::theme::Theme;
        use crate::trash::{close_button, dialog, primary_button, secondary_button};
        let d = self.feishu_dlg.as_ref()?;
        let close = |w: &mut Workspace, cx: &mut Context<Workspace>| {
            w.feishu_dlg = None;
            cx.notify();
        };
        let hint = if d.blank {
            crate::settings::text("关联后会把云端文档拉取到这篇空白文档里。之后打开文档时自动拉取云端更新，改完用“推送到飞书”同步回去。")
        } else {
            crate::settings::text("这篇文档已有内容，第一次同步需要选择以哪一边为准；另一边的内容会被覆盖。")
        };
        let mut buttons = div().px(px(24.)).pb(px(24.)).flex().justify_end().gap(px(12.)).child(
            secondary_button("feishu-cancel", crate::settings::text("取消")).min_w(px(80.)).on_click(cx.listener(move |w, _, _, cx| close(w, cx))),
        );
        buttons = if d.blank {
            buttons.child(primary_button("feishu-pull", crate::settings::text("关联并拉取")).min_w(px(80.)).on_click(cx.listener(|w, _, window, cx| w.feishu_link_submit(Mode::TakeCloud, window, cx))))
        } else {
            buttons
                .child(secondary_button("feishu-take-local", crate::settings::text("用本地覆盖云端")).min_w(px(80.)).on_click(cx.listener(|w, _, window, cx| w.feishu_link_submit(Mode::TakeLocal, window, cx))))
                .child(primary_button("feishu-take-cloud", crate::settings::text("用云端覆盖本地")).min_w(px(80.)).on_click(cx.listener(|w, _, window, cx| w.feishu_link_submit(Mode::TakeCloud, window, cx))))
        };
        let dialog = dialog("feishu-link")
            .relative()
            .w(px(520.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div().pt(px(24.)).pb(px(16.)).pl(px(24.)).pr(px(56.)).text_size(px(15.)).line_height(px(24.)).font_weight(FontWeight::SEMIBOLD)
                    .child(crate::settings::text("关联飞书文档")),
            )
            .child(close_button("feishu-close").absolute().top(px(27.)).right(px(25.)).on_click(cx.listener(move |w, _, _, cx| close(w, cx))))
            .child(
                div().px(px(24.)).pb(px(20.)).flex().flex_col().gap(px(10.)).text_size(px(13.)).line_height(px(20.))
                    .child(div().text_color(Theme::text_caption()).child(hint))
                    .children(d.busy.then(|| div().text_color(Theme::text_caption()).child(crate::settings::text("正在查找知识库文档…"))))
                    .child(div().id("feishu-url").h(px(34.)).px(px(10.)).flex().items_center().rounded(px(6.)).border_1().border_color(Theme::line_border()).child(d.field.clone()))
                    .children(d.error.clone().map(|e| div().text_color(Theme::danger()).child(e))),
            )
            .child(buttons);
        Some(crate::transfer::mask("feishu-mask", dialog, close, cx))
    }

    /// The "…" menu's 飞书同步 submenu.
    pub(crate) fn feishu_menu(&self, id: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        use crate::docmenu::sub_item;
        let menu = crate::ui::popover().id("dm-feishu-menu").occlude().w(px(240.)).px(px(5.)).py(px(crate::ui::MENU_PAD_Y));
        if self.feishu_busy.contains(id) {
            return menu.child(sub_item("fs-busy", crate::settings::text("正在同步…")));
        }
        let note = |s: String| div().px(px(8.)).py(px(6.)).text_size(px(12.)).line_height(px(18.)).text_color(crate::theme::Theme::text_caption()).child(s);
        let act = |item: &'static str, label: &'static str, mode: Mode, id: &str| {
            let id = id.to_string();
            sub_item(item, label).on_click(cx.listener(move |w, _, window, cx| w.feishu_sync(&id, mode, false, window, cx)))
        };
        match link_of(&Store::root_dir(), id) {
            Some(l) if l.revision > 0 => {
                let (_, _, _, h, mi) = crate::time::civil(l.synced);
                let unlink_id = id.to_string();
                let linked = if l.wiki_node.is_empty() {
                    crate::localized_format!("已关联飞书 · {:02}:{:02} 同步", "Linked to Feishu · synced {:02}:{:02}", h, mi)
                } else {
                    crate::localized_format!("已关联飞书知识库 · {:02}:{:02} 同步", "Linked to a Feishu wiki · synced {:02}:{:02}", h, mi)
                };
                menu.child(note(linked))
                    .child(act("fs-pull", crate::settings::text("拉取云端更新"), Mode::Pull, id))
                    .child(act("fs-push", crate::settings::text("推送到飞书"), Mode::Push, id))
                    .child(act("fs-take-cloud", crate::settings::text("用云端覆盖本地"), Mode::TakeCloud, id))
                    .child(act("fs-take-local", crate::settings::text("用本地覆盖云端"), Mode::TakeLocal, id))
                    .child(sub_item("fs-unlink", crate::settings::text("取消关联")).on_click(cx.listener(move |w, _, _, cx| w.feishu_unlink(&unlink_id, cx))))
            }
            Some(l) => {
                let unlink_id = id.to_string();
                let linked = if l.wiki_node.is_empty() { crate::settings::text("已关联，尚未同步") } else { crate::settings::text("已关联飞书知识库，尚未同步") };
                menu.child(note(linked.to_string()))
                    .child(act("fs-take-cloud", crate::settings::text("用云端覆盖本地"), Mode::TakeCloud, id))
                    .child(act("fs-take-local", crate::settings::text("用本地覆盖云端"), Mode::TakeLocal, id))
                    .child(sub_item("fs-unlink", crate::settings::text("取消关联")).on_click(cx.listener(move |w, _, _, cx| w.feishu_unlink(&unlink_id, cx))))
            }
            None => {
                let link_id = id.to_string();
                menu.child(sub_item("fs-link", crate::settings::text("关联飞书文档…")).on_click(cx.listener(move |w, _, window, cx| w.open_feishu_link(&link_id, window, cx))))
                    .child(act("fs-create", crate::settings::text("上传为新的飞书文档"), Mode::Create, id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Not a glob: `gpui::test` would shadow `#[test]`.
    use super::{Document, Link, Media, Pasted, base_of, is_blank, link_of, parse_link, save_base, save_link, sync, token_of, unlink};

    #[test]
    fn links_and_tokens() {
        assert_eq!(token_of("https://x.feishu.cn/docx/DoxcnExampleDocumentToken1?from=from_copylink").unwrap(), "DoxcnExampleDocumentToken1");
        assert_eq!(token_of(" DoxcnExampleDocumentToken1 ").unwrap(), "DoxcnExampleDocumentToken1");
        assert!(token_of("https://x.feishu.cn/wiki/DoxcnExampleDocumentToken1").is_err());
        assert!(token_of("https://x.feishu.cn/docs/doccnExampleLegacyToken01").is_err());
        assert!(token_of("hello").is_err());
    }

    #[test]
    fn pasted_links_by_shape() {
        let docx = Pasted::Docx("DoxcnExampleDocumentToken1".into());
        let wiki = Pasted::Wiki("WikcnExampleWikiNodeToken1".into());
        assert_eq!(parse_link("https://x.feishu.cn/docx/DoxcnExampleDocumentToken1?from=from_copylink").unwrap(), docx);
        assert_eq!(parse_link(" DoxcnExampleDocumentToken1 ").unwrap(), docx);
        assert_eq!(parse_link("https://example.feishu.cn/wiki/WikcnExampleWikiNodeToken1").unwrap(), wiki);
        assert_eq!(parse_link("https://x.feishu.cn/wiki/WikcnExampleWikiNodeToken1?from=from_copylink#share").unwrap(), wiki);
        assert!(parse_link("https://x.feishu.cn/wiki/").is_err());
        assert!(parse_link("https://x.feishu.cn/wiki/short").is_err());
        assert!(parse_link("https://x.feishu.cn/sheets/shtcnExampleLegacyToken01").is_err());
        assert!(parse_link("hello").is_err());
    }

    #[test]
    fn state_round_trips() {
        let root = std::env::temp_dir().join(format!("fdl-feishu-{}", std::process::id()));
        let link = Link { url: "u".into(), token: "t".into(), wiki_node: "w".into(), revision: 7, media: Media::from([("tok".into(), "assets/a.png".into())]), synced: 1 };
        save_link(&root, "d", &link).unwrap();
        let back = link_of(&root, "d").unwrap();
        assert_eq!((back.revision, back.wiki_node.as_str(), back.media.get("tok").map(String::as_str)), (7, "w", Some("assets/a.png")));
        // Links saved before wiki_node existed still load.
        std::fs::write(root.join("sync/old.json"), r#"{"url":"","token":"t","revision":1}"#).unwrap();
        assert!(link_of(&root, "old").unwrap().wiki_node.is_empty());
        let doc = crate::docxml::parse("<title>T</title><p>a</p><ul><li>b</li></ul>").unwrap();
        save_base(&root, "d", &doc).unwrap();
        assert_eq!(sync::keys(&base_of(&root, "d").unwrap().blocks), sync::keys(&doc.blocks));
        unlink(&root, "d");
        assert!(link_of(&root, "d").is_none() && base_of(&root, "d").is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blank_documents() {
        assert!(is_blank(&Document::default()));
        assert!(is_blank(&crate::docxml::parse("<title></title><p></p>").unwrap()));
        assert!(!is_blank(&crate::docxml::parse("<title></title><p>x</p>").unwrap()));
        assert!(!is_blank(&crate::docxml::parse("<title>T</title><p></p>").unwrap()));
    }
}
