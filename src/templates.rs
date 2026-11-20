//! 模板: built-in starting points for a new page. A template is a DocxXML
//! file under `.quiet/templates/`, so it can be edited like any document and is
//! copied into the page — never linked to it.
//!
//! 新建 makes a blank document and the page offers the templates as bands under
//! it: 空白文档, then the first few templates in file order. Picking one fills
//! that same document in and renames it. 模板库 opens the whole shelf in a
//! dialog — names on the left, the template's own content on the right — and
//! `⋯` › 选择模板 brings the bands back for a page that already has content.

use crate::app::Workspace;
use crate::model::Document;
use crate::settings::tr;
use gpui::*;

mod shelf;

include!(concat!(env!("OUT_DIR"), "/templates.rs"));

/// A template as the bands and 模板库 show it: what it looks like, what it is
/// for, and the shelf it sits on.
pub struct TemplateCard {
    pub name: String,
    pub title: String,
    pub emoji: String,
    pub desc: String,
    pub group: String,
}

/// The shelves of 模板库, in order. Templates the user saved are last.
const GROUPS: &[(&str, &str)] = &[
    ("会议与记录", "Meetings & records"),
    ("项目与规划", "Projects & planning"),
    ("决策与问题", "Decisions & issues"),
    ("思考与复盘", "Thinking & review"),
    ("我的模板", "Mine"),
];

/// The built-ins' faces: file name, emoji, one-line description, shelf.
struct Look {
    file: &'static str,
    emoji: &'static str,
    desc: (&'static str, &'static str),
    group: &'static str,
}

const LOOKS: &[Look] = &[
    Look { file: "01-meeting-notes.xml", emoji: "📝", desc: ("同步和共享", "Share it with the room"), group: "会议与记录" },
    Look { file: "02-project-status.xml", emoji: "📊", desc: ("进展、风险、下一步", "Progress, risks, next"), group: "项目与规划" },
    Look { file: "03-daily-report.xml", emoji: "✅", desc: ("今天做了什么", "What got done today"), group: "会议与记录" },
    Look { file: "04-weekly-report.xml", emoji: "🗓", desc: ("一周的目标与达成", "The week's goals, met"), group: "会议与记录" },
    Look { file: "05-prd.xml", emoji: "📋", desc: ("背景、目标、方案", "Why, what, how"), group: "项目与规划" },
    Look { file: "06-retro.xml", emoji: "🔁", desc: ("做对了什么，改什么", "Keep, drop, try"), group: "思考与复盘" },
    Look { file: "07-reading-notes.xml", emoji: "📚", desc: ("摘录与想法", "Quotes and thoughts"), group: "思考与复盘" },
    Look { file: "08-okr.xml", emoji: "🎯", desc: ("目标与关键结果", "Objectives and results"), group: "项目与规划" },
    Look { file: "09-project-kickoff.xml", emoji: "🚀", desc: ("目标、范围、里程碑", "Goal, scope, milestones"), group: "项目与规划" },
    Look { file: "10-project-plan.xml", emoji: "📅", desc: ("里程碑与任务", "Milestones and tasks"), group: "项目与规划" },
    Look { file: "11-team-decision.xml", emoji: "⚖️", desc: ("问题、选项、结论", "Question, options, call"), group: "决策与问题" },
    Look { file: "12-decision-record.xml", emoji: "🧾", desc: ("为什么这么定", "Why we chose it"), group: "决策与问题" },
    Look { file: "13-issue-tracker.xml", emoji: "🐞", desc: ("谁在修，修到哪", "Who fixes what"), group: "决策与问题" },
    Look { file: "14-problem-framing.xml", emoji: "🔍", desc: ("把感觉拆成问题", "From feeling to question"), group: "决策与问题" },
    Look { file: "15-brainstorm.xml", emoji: "💡", desc: ("先求量，不评判", "Quantity before quality"), group: "思考与复盘" },
    Look { file: "16-bad-idea.xml", emoji: "🙃", desc: ("怎样让它失败", "How to fail, reversed"), group: "思考与复盘" },
    Look { file: "17-standup.xml", emoji: "☕", desc: ("昨天、今天、阻塞", "Yesterday, today, blocked"), group: "会议与记录" },
    Look { file: "18-action-list.xml", emoji: "✅", desc: ("谁在何时交付", "Who delivers when"), group: "项目与规划" },
    Look { file: "19-product-wiki.xml", emoji: "📘", desc: ("一页看懂产品", "The product on one page"), group: "项目与规划" },
    Look { file: "20-event-plan.xml", emoji: "🎪", desc: ("时间线、预算、预案", "Timeline, budget, risks"), group: "项目与规划" },
    Look { file: "21-onboarding.xml", emoji: "🧭", desc: ("第一周做什么", "The first week"), group: "项目与规划" },
    Look { file: "22-research-notes.xml", emoji: "🔬", desc: ("问题、来源、结论", "Question, sources, answer"), group: "思考与复盘" },
];

/// The built-ins as shipped: (file name, XML). File-name order is menu order.
fn builtin() -> impl Iterator<Item = (&'static str, &'static str)> {
    FILES.iter().filter_map(|(path, bytes)| Some((path.strip_prefix("src/")?, std::str::from_utf8(bytes).ok()?)))
}

/// A bundled cover (`preset:gradient-1`) written into the library's images once,
/// so the page can show it like any other picture.
fn preset_cover(name: &str) -> Option<std::path::PathBuf> {
    let bytes = crate::assets::get(&format!("covers/{name}.jpg"))?;
    let path = crate::store::Store::assets_dir().join(format!("preset-{name}.jpg"));
    if !path.exists() {
        fdl_core::vfs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// A template's 封面 as shipped is a name in our gallery, not a file: swap it
/// for the picture before the document goes anywhere.
fn resolve_cover(doc: &mut Document) {
    let Some(cover) = &mut doc.cover else { return };
    let Some(name) = cover.src.strip_prefix("preset:") else { return };
    if let Some(path) = preset_cover(name) {
        cover.src = path.to_string_lossy().into_owned();
    }
}

/// What we last wrote under each built-in name (`.seeded`, one `hash name` a
/// line). It is what tells our seed from the user's own edit.
fn seed_hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

fn read_seeded(dir: &std::path::Path) -> std::collections::HashMap<String, u64> {
    std::fs::read_to_string(dir.join(".seeded"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (hash, name) = l.split_once(' ')?;
            Some((name.to_string(), hash.parse().ok()?))
        })
        .collect()
}

fn write_seeded(dir: &std::path::Path, map: &std::collections::HashMap<String, u64>) {
    let mut lines: Vec<String> = map.iter().map(|(name, hash)| format!("{hash} {name}")).collect();
    lines.sort();
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(".seeded"), lines.join("\n"));
}

impl Workspace {
    /// Write the built-ins into `.quiet/templates/`, and keep them current.
    ///
    /// A built-in the user edited (or replaced) is their copy and is left
    /// alone; one that still holds the text we wrote is updated when this
    /// build ships a better version. `.seeded` remembers what we wrote so the
    /// two can be told apart. Deleting a file brings it back on the next
    /// launch.
    pub(crate) fn seed_templates(&mut self) {
        let dir = self.store.with(|s| s.templates_dir());
        let mut seeded = read_seeded(&dir);
        for (name, xml) in builtin() {
            let current = std::fs::read_to_string(dir.join(name)).ok();
            let ours = match &current {
                // Never written, or still exactly what we last wrote.
                Some(text) => seeded.get(name).is_none_or(|old| *old == seed_hash(text)),
                None => true,
            };
            if !ours {
                continue;
            }
            if current.as_deref() != Some(xml)
                && let Err(e) = self.store.with(|s| s.save_template(name, xml)) {
                    eprintln!("无法写入模板 {name}：{e}");
                    continue;
                }
            seeded.insert(name.to_string(), seed_hash(xml));
        }
        write_seeded(&dir, &seeded);
        self.reload_template_cards();
    }

    /// Read every template's title, for the bands and 模板库. File order is the
    /// built-ins' own order (`01-` … `08-`), then the user's (`90-`); 模板库
    /// keeps that order inside each shelf.
    pub(crate) fn reload_template_cards(&mut self) {
        self.template_cards = self
            .store
            .with(|s| s.templates())
            .into_iter()
            .map(|t| {
                let doc = self
                    .store
                    .with(|s| s.template_xml(&t.name))
                    .ok()
                    .and_then(|xml| crate::docxml::parse(&xml).ok());
                let (emoji, desc, group) = match LOOKS.iter().find(|l| l.file == t.name) {
                    // The page's own icon wins, so a card always shows what the
                    // template will put on the page.
                    Some(l) => (
                        doc.as_ref().and_then(|d| d.icon.clone()).unwrap_or_else(|| l.emoji.to_string()),
                        tr(l.desc.0, l.desc.1).to_string(),
                        l.group.to_string(),
                    ),
                    // A template the user saved: its own icon, if the page had one.
                    None => (
                        doc.as_ref().and_then(|d| d.icon.clone()).unwrap_or_else(|| "📄".to_string()),
                        tr("自己保存的模板", "Saved by you").to_string(),
                        GROUPS[GROUPS.len() - 1].0.to_string(),
                    ),
                };
                TemplateCard { name: t.name, title: t.title, emoji, desc, group }
            })
            .collect();
        // File order: the bands take the first few built-ins, and 模板库
        // groups by shelf on its own.
        self.template_cards.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// The template `name`, ready to show or to copy into a page: its cover
    /// resolved to a real picture.
    fn template_doc(&self, name: &str) -> Option<Document> {
        let xml = self.store.with(|s| s.template_xml(name)).ok()?;
        let mut doc = crate::docxml::parse(&xml).ok()?;
        resolve_cover(&mut doc);
        Some(doc)
    }

    /// 选择模板 for this page: the bands open under it.
    pub(crate) fn pick_template(&mut self, id: &str, cx: &mut Context<Self>) {
        self.template_picker = Some(id.to_string());
        cx.notify();
    }

    pub(crate) fn close_template_picker(&mut self, cx: &mut Context<Self>) {
        self.template_picker = None;
        self.template_lib = None;
        self.template_preview = None;
        cx.notify();
    }

    /// 模板库: the whole shelf, opening on `ix`.
    pub(crate) fn open_template_lib(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.template_lib = Some(ix);
        self.show_template_preview(ix, window, cx);
        cx.notify();
    }

    pub(crate) fn close_template_lib(&mut self, cx: &mut Context<Self>) {
        self.template_lib = None;
        self.template_preview = None;
        cx.notify();
    }

    /// Pick another template in 模板库: the preview keeps its page state, so
    /// switching cards reads as turning pages rather than rebuilding a list.
    pub(crate) fn choose_template(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.template_lib = Some(ix);
        self.show_template_preview(ix, window, cx);
        cx.notify();
    }

    /// The right pane of 模板库 is a real page: the template in a read-only
    /// editor, so what is previewed is exactly what 使用模板 makes.
    fn show_template_preview(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(card) = self.template_cards.get(ix) else { return };
        let Some(doc) = self.template_doc(&card.name) else { return };
        match self.template_preview.clone() {
            Some(ed) => ed.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc), cx)),
            None => {
                let preview = cx.new(|cx| {
                    let mut e = crate::editor::view::Editor::new(doc, crate::time::now(), crate::store::PageWidth::Standard, window, cx);
                    e.set_read_only(true, cx);
                    e
                });
                self.template_preview = Some(preview);
            }
        }
    }

    /// Fill the page `id` with a template: its content, its name, its icon and
    /// its cover.
    ///
    /// The document keeps its id, so its sidebar row and any link to it stay
    /// put — only its content and its title change.
    pub(crate) fn apply_template(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.template_picker.clone() else { return };
        let Some(doc) = self.template_doc(name) else {
            return self.show_toast_kind(crate::ui::ToastKind::Error, tr("模板读不出来", "Could not read the template"), cx);
        };
        let file_before = self.store.track_file_changes();
        // The page on disk may be cached: drop it so `open` reads what we wrote.
        self.store.with(|s| s.forget_loaded(&id));
        match self.store.with(|s| s.save(&id, &doc)) {
            Ok(_) => {
                self.template_picker = None;
                self.template_lib = None;
                self.template_preview = None;
                self.entries = self.store.list();
                self.sort();
                if self.current.as_deref() == Some(id.as_str()) {
                    // The page is already open, so `open` would return at once
                    // and the editor would keep the old document until another
                    // page was visited. Hand it the template directly instead.
                    if let Some(ed) = self.editor.clone() {
                        ed.update(cx, |e, cx| e.receive_document(&crate::editor::core::Core::new(doc), cx));
                        let f = ed.read(cx).focus.clone();
                        window.focus(&f, cx);
                    }
                    // The template is already on disk: nothing to autosave.
                    self.unsaved = false;
                    self.status = crate::app::SaveStatus::Saved;
                    self.sync_document(window, cx);
                } else {
                    self.open(&id, window, cx);
                }
                self.remember_files(file_before, tr("套用模板", "Template applied"));
            }
            Err(e) => self.status = crate::app::SaveStatus::Failed(crate::localized_format!("套用模板失败：{}", "Could not apply the template: {}", crate::settings::diagnostic(&e.to_string()))),
        }
        cx.notify();
    }

    /// 另存为模板: the open document becomes a starting point for 选择模板.
    pub(crate) fn save_as_template(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.doc_of(id, cx) else { return };
        let title = crate::settings::display_title(&doc.title.text);
        let stem: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '\0') { ' ' } else { c }).collect();
        let stem = stem.trim();
        let stem = if stem.is_empty() { crate::settings::text("未命名模板") } else { stem };
        // `90-` keeps user templates after the built-ins (`01-` … `08-`).
        let mut name = format!("90-{stem}.xml");
        let mut n = 2;
        while self.store.with(|s| s.templates_dir().join(&name).exists()) {
            name = format!("90-{stem}-{n}.xml");
            n += 1;
        }
        match self.store.with(|s| s.save_template(&name, &crate::docxml::serialize(&doc))) {
            Ok(()) => {
                self.reload_template_cards();
                self.show_toast(&crate::localized_format!("已保存为模板「{}」", "Saved as template “{}”", title), cx)
            }
            Err(e) => self.show_toast_kind(crate::ui::ToastKind::Error, &e.to_string(), cx),
        }
    }
}

#[cfg(test)]
mod tests {
    // Not `use super::*`: it would bring in gpui's `test` attribute macro.
    use super::builtin;

    /// Every shipped template is a document the parser accepts, with a title
    /// and something in it, a face for its card, and survives a write/read
    /// round trip unchanged.
    #[test]
    fn every_builtin_is_a_usable_document() {
        let mut seen = vec![];
        for (name, xml) in builtin() {
            let doc = crate::docxml::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
            let title = crate::model::display_title(&doc.title.text);
            assert!(!title.trim().is_empty(), "{name} has no title");
            assert!(!doc.blocks.is_empty(), "{name} is empty");
            assert!(doc.icon.is_some(), "{name} has no icon for its card");
            let cover = doc.cover.as_ref().unwrap_or_else(|| panic!("{name} has no cover"));
            assert!(cover.src.starts_with("preset:"), "{name} cover is not a gallery name: {}", cover.src);
            let again = crate::docxml::parse(&crate::docxml::serialize(&doc)).unwrap_or_else(|e| panic!("{name} round trip: {e}"));
            assert_eq!(again.blocks.len(), doc.blocks.len(), "{name} loses blocks on a round trip");
            assert_eq!(again.icon, doc.icon, "{name} loses its icon on a round trip");
            assert_eq!(again.cover, doc.cover, "{name} loses its cover on a round trip");
            seen.push(name);
        }
        assert_eq!(seen.len(), 22, "{seen:?}");
        assert!(seen.first().is_some_and(|n| n.starts_with("01-")), "built-ins are named in menu order");
    }

    /// Every `preset:` a template names is a picture the app ships.
    #[test]
    fn every_preset_cover_exists() {
        for (name, xml) in builtin() {
            let doc = crate::docxml::parse(xml).unwrap_or_else(|e| panic!("{name}: {e}"));
            let Some(cover) = &doc.cover else { continue };
            let Some(preset) = cover.src.strip_prefix("preset:") else { continue };
            assert!(crate::assets::get(&format!("covers/{preset}.jpg")).is_some(), "{name}: {preset}");
        }
    }
}
