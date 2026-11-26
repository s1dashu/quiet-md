//! 设置 › 连接 Agent: how to let an AI agent (Claude Code, Codex, …)
//! read and write the library: the skill and the `quiet` CLI it drives, a
//! pair. First the line that installs the skill (`npx skills add`, pasted to
//! the agent), then a sentence that has the agent set up both by following
//! docs/AGENT_INSTALL.md, then the CLI.

use crate::app::Workspace;
use crate::design::{Badge, Button, color, geo};
#[cfg(target_os = "macos")]
use crate::design::ButtonSize;
use crate::settings::tr;
use crate::ui::ToastKind;
use gpui::{prelude::FluentBuilder, *};
#[cfg(target_os = "macos")]
use std::path::PathBuf;

/// What the agent follows to install the skill and the CLI (`docs/AGENT_INSTALL.md`).
const GUIDE: &str = "https://github.com/s1dashu/quiet-md/blob/main/docs/AGENT_INSTALL.md";

/// The sentence to paste to the agent.
fn prompt() -> String {
    crate::localized_format!("按照 {} 安装 Quiet.md 的 skill 和 CLI。", "Install the Quiet.md skill and CLI by following {}", GUIDE)
}

/// The skill's install line (Vercel's `skills` CLI, the agents' common installer).
const SKILL_ADD: &str = "npx skills add s1dashu/quiet-md";

/// The `quiet` beside the app's own binary (in the .app on macOS, the install
/// folder on Windows, `bin/` on Linux). Only the Mac links it from here.
#[cfg(target_os = "macos")]
fn bundled_cli() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(if cfg!(windows) { "quiet.exe" } else { "quiet" });
    p.is_file().then_some(p)
}

/// macOS: the link 安装命令行工具 makes, in a folder of the user's own (no
/// administrator password).
#[cfg(target_os = "macos")]
fn link_path() -> Option<PathBuf> { dirs::home_dir().map(|h| h.join(".local/bin/quiet")) }

/// Whether `quiet` in the link folder is this app's.
#[cfg(target_os = "macos")]
fn linked() -> bool {
    match (link_path().and_then(|l| std::fs::canonicalize(l).ok()), bundled_cli().and_then(|b| std::fs::canonicalize(b).ok())) { (Some(a), Some(b)) => a == b, _ => false }
}

#[cfg(target_os = "macos")]
fn install_link() -> std::io::Result<PathBuf> {
    let (link, cli) = (link_path().ok_or(std::io::ErrorKind::NotFound)?, bundled_cli().ok_or(std::io::ErrorKind::NotFound)?);
    std::fs::create_dir_all(link.parent().unwrap())?;
    // Only a link (an older one, or one to another copy of the app) is replaced, never a file.
    if std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink()) { std::fs::remove_file(&link)?; }
    std::os::unix::fs::symlink(&cli, &link)?;
    Ok(link)
}

/// Text in the monospace face on the muted fill, with its copy button.
fn code(id: &'static str, text: String, cx: &mut Context<Workspace>) -> Div { snippet(id, text, true, cx) }

/// `code` for a sentence: the interface's own face.
fn prose(id: &'static str, text: String, cx: &mut Context<Workspace>) -> Div { snippet(id, text, false, cx) }

fn snippet(id: &'static str, text: String, mono: bool, cx: &mut Context<Workspace>) -> Div {
    let g = geo();
    let copied = text.clone();
    div().w_full().flex().items_start().gap(px(8.)).pl(px(12.)).pr(px(4.)).py(px(4.)).rounded(px(g.field_r)).bg(color::muted())
        .child(div().flex_1().min_w_0().py(px(4.)).when(mono, |d| d.font_family(crate::theme::MONO_FONT).text_size(px(g.text_xs)).line_height(px(g.text_xs * 1.6))).when(!mono, |d| d.text_size(px(g.text)).line_height(px(g.text * 1.5))).text_color(color::foreground()).child(text))
        .child(Button::ghost(id).icon("copy").on_click(cx.listener(move |w, _, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
            w.show_toast_kind(ToastKind::Success, tr("已复制", "Copied"), cx);
        })))
}

/// A block of the section: title (with a badge), a short line, then what to copy.
fn part(title: &'static str, badge: Option<&'static str>, line: &'static str, body: Vec<AnyElement>) -> AnyElement {
    let g = geo();
    div().w_full().px(px(g.item_px)).py(px(g.item_py)).flex().flex_col().gap(px(10.))
        .child(div().flex().flex_col().gap(px(4.))
            .child(div().flex().items_center().gap(px(8.)).text_size(px(g.text)).line_height(px(g.text * 1.375)).text_color(color::foreground()).child(title).when_some(badge, |d, b| d.child(Badge::new(b))))
            .child(div().text_size(px(g.text)).line_height(px(g.text * 1.5)).text_color(color::muted_foreground()).child(line)))
        .children(body)
        .into_any_element()
}

fn small(s: &'static str) -> Div { div().text_size(px(geo().text_xs)).line_height(px(geo().text_xs * 1.5)).text_color(color::muted_foreground()).child(s) }

/// 设置 › 连接 Agent: the skill's install line, a prompt for the agent, the CLI.
pub(crate) fn page(cx: &mut Context<Workspace>) -> AnyElement {
    let skill = part(tr("安装 Skill", "Install the skill"), Some(tr("推荐", "Recommended")), tr("把这行粘贴给你的 Agent，它会自己装好，之后就知道怎么用 Quiet.md。", "Paste this line to your agent. It installs itself and learns how to use Quiet.md."), vec![
        code("copy-skill", SKILL_ADD.into(), cx).into_any_element(),
    ]);
    let ask = part(tr("让 Agent 帮你安装", "Let your agent set it up"), None, tr("或者把这句话发给你的 Agent。", "Or copy this to your agent."), vec![
        prose("copy-prompt", prompt(), cx).into_any_element(),
    ]);
    #[cfg(target_os = "macos")]
    let how = {
        let done = linked();
        div().flex().items_center().gap(px(10.))
            .child(Button::outline("install-cli").sized(ButtonSize::Sm).label(if done { tr("已安装", "Installed") } else { tr("安装命令行工具", "Install Command") }).disabled(done || bundled_cli().is_none())
                .on_click(cx.listener(|w, _, _, cx| match install_link() {
                    Ok(_) => { w.show_toast_kind(ToastKind::Success, tr("已安装到 ~/.local/bin", "Installed in ~/.local/bin"), cx); cx.notify(); }
                    Err(e) => w.show_toast_kind(ToastKind::Error, &crate::localized_format!("没有装上：{}", "Could not install: {}", e), cx),
                })))
            .child(small(tr("装到 ~/.local/bin，不需要管理员密码", "Goes in ~/.local/bin, no password needed")))
            .into_any_element()
    };
    #[cfg(target_os = "windows")]
    let how = small(tr("安装程序已把 quiet 加入 PATH，新开的终端里就能用。", "The installer puts quiet on your PATH; open a new terminal to use it.")).into_any_element();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let how = small(tr("quiet 在安装包的 bin 文件夹里，和 Quiet.md 放在一起。", "quiet is in the package's bin folder, beside Quiet.md.")).into_any_element();
    let cli_body = vec![how, code("copy-cli", tr("quiet ls\nquiet read 文件夹/文档\nquiet help", "quiet ls\nquiet read Folder/Note\nquiet help").into(), cx).into_any_element()];
    let cli = part("CLI", None, tr("Skill 配套的命令行工具：在终端里用 quiet 命令列出、读写、搜索文档和数据表。", "The command the skill uses: list, read, write and search documents and data tables with quiet."), cli_body);
    let card = |id: &'static str, part: AnyElement| crate::design::Card::new(id).w_full().overflow_hidden().child(part).into_any_element();
    div().w_full().flex().flex_col().gap(px(geo().section_gap))
        .child(card("agents-skill", skill)).child(card("agents-ask", ask)).child(card("agents-cli", cli))
        .into_any_element()
}
