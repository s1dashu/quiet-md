//! The color picker, after Excalidraw `components/ColorPicker/*`
//! (`ColorPicker.tsx`, `TopPicks.tsx`, `Picker.tsx`, `PickerColorList.tsx`,
//! `ShadeList.tsx`, `ColorInput.tsx`, `ColorPicker.scss`) and
//! `PropertiesPopover.tsx` @ 3a317b5: the five top picks, a separator and the
//! active-color trigger; the trigger opens a picker beside the panel: "Colors"
//! (the 5×3 palette at the active shade), "Shades" (the hue's five) and the
//! hex input. Swatches show the color as the canvas draws it (dark mode
//! mapped), with their hotkey labels; above them the scene's most used custom
//! colors (1–5). Looks: `design::Swatch`es (the current one ringed), the
//! picker a `design::Popover` with `text-xs` headings, the hex input a small
//! `design::Field`. Hotkeys (`keyboardNavHandlers.ts`): the
//! palette's q…b at the active shade, ⇧1–5 the shades, 1–5 the custom
//! colors. Differs: no top-picks drag & drop or reset, no eye dropper, no
//! arrow-key / Tab navigation, no error message under an invalid hex (the
//! border turns red).

use super::*;
use super::colors;
use super::tip::ExTip;
use crate::board::action::BoardAction;
use crate::design::{self, Field, FieldSize, Swatch};
use crate::board::utils::is_transparent;
use crate::input::{TextField, TextFieldEvent};
use crate::settings::tr;

thread_local! {
    /// The color triggers' window bounds at the last layout.
    static TRIGGER: RefCell<HashMap<(EntityId, Pop), Bounds<Pixels>>> = RefCell::default();
    /// The open picker's hex field.
    static HEX: RefCell<HashMap<EntityId, (Pop, Entity<TextField>, Subscription)>> = RefCell::default();
    /// The open picker, for its hotkeys: which one, its custom colors (taken
    /// when it opened, like upstream's `useState`), the current color, the
    /// active shade and the action.
    static OPEN: RefCell<HashMap<EntityId, Open>> = RefCell::default();
}

#[derive(Clone)]
struct Open { p: Pop, customs: Vec<String>, current: Option<String>, shade: usize, action: fn(String) -> BoardAction }

/// `isCustomColor`: not one of the palette's values.
fn is_custom(c: &str) -> bool { !(c == colors::TRANSPARENT || c == colors::BLACK || c == colors::WHITE || colors::HUES.iter().any(|(_, s)| s.contains(&c))) }

/// `getMostUsedCustomColors`: the five most used custom stroke / background
/// colors in the scene.
fn most_used_custom(elements: &[crate::board::types::Element], p: Pop) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = vec![];
    for e in elements.iter().filter(|e| !e.is_deleted) {
        let c = if p == Pop::Stroke { &e.stroke_color } else { &e.background_color };
        if is_transparent(c) || !is_custom(c) { continue; }
        match counts.iter_mut().find(|(x, _)| x == c) { Some(n) => n.1 += 1, None => counts.push((c.clone(), 1)) }
    }
    counts.sort_by_key(|c| std::cmp::Reverse(c.1));
    counts.into_iter().take(5).map(|c| c.0).collect()
}

/// `colorPickerKeyNavHandler`'s hotkeys while a picker is open (and its hex
/// field isn't focused). True if the key was the picker's.
pub(crate) fn hotkey(me: &mut BoardView, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<BoardView>) -> bool {
    let Some(p) = pop(cx).filter(|p| matches!(p, Pop::Stroke | Pop::Background)) else { return false };
    let Some(o) = OPEN.with(|m| m.borrow().get(&cx.entity_id()).cloned()).filter(|o| o.p == p) else { return false };
    if HEX.with(|h| h.borrow().get(&cx.entity_id()).is_some_and(|(_, f, _)| f.read(cx).focus_handle(cx).is_focused(window))) { return false; }
    let m = &ev.keystroke.modifiers;
    if m.platform || m.control || m.alt { return false; }
    let key = ev.keystroke.key.as_str();
    let digit = key.parse::<usize>().ok().filter(|d| (1..=5).contains(d));
    let pick = if let Some(d) = digit && m.shift {
        o.current.as_deref().and_then(colors::find).map(|(h, _)| colors::HUES[h].1[d - 1].to_string())
    } else if let Some(d) = digit {
        o.customs.get(d - 1).cloned()
    } else if let Some(i) = HOTKEYS.iter().position(|k| *k == key).filter(|_| !m.shift) {
        Some(colors::grid(o.shade)[i].to_string())
    } else { None };
    let Some(c) = pick else { return false };
    me.run((o.action)(c), window, cx);
    true
}

/// `colorPickerHotkeyBindings`.
const HOTKEYS: [&str; 15] = ["q", "w", "e", "r", "t", "a", "s", "d", "f", "g", "z", "x", "c", "v", "b"];

/// `HotkeyLabel`: bottom right, 10px monospace, light on dark colors (it's
/// drawn on the color, as the canvas shows it).
fn hotkey_label(color: &str, label: String) -> Div {
    div().absolute().right(px(3.)).bottom(px(1.)).font_family(crate::theme::MONO_FONT).text_size(px(10.)).line_height(px(12.))
        .text_color(if is_dark(color, 160.) { gpui::white().opacity(0.85) } else { gpui::black().opacity(0.6) }).child(label)
}

/// A popup opened or closed: the hex field goes with its picker.
pub(crate) fn popup_changed(p: Option<Pop>, cx: &mut Context<BoardView>) {
    let id = cx.entity_id();
    HEX.with(|h| { let mut h = h.borrow_mut(); if h.get(&id).is_some_and(|(q, _, _)| Some(*q) != p) { h.remove(&id); } });
}

/// `isColorDark` (YIQ below `threshold`; transparent isn't dark).
fn is_dark(color: &str, threshold: f64) -> bool {
    if is_transparent(color) { return false; }
    let c = crate::board::color::color_or_black(color);
    (c.r as f64 * 299. + c.g as f64 * 587. + c.b as f64 * 114.) / 1000. < threshold
}

/// The color as the canvas shows it (`applyDarkModeFilter`).
fn display(color: &str) -> Hsla { crate::board::render::color(crate::board::color::color_or_black(color), 1.) }

/// A `design::Swatch` of `color` as the canvas shows it; no color (transparent)
/// as the checker pattern.
fn swatch(id: impl Into<ElementId>, color: &str, size: f32, active: bool) -> Swatch {
    if !is_transparent(color) { return Swatch::new(id, display(color)).sized(size).selected(active); }
    // 8pt squares at 8% of the text color, clipped to the corners
    let mut cells = div().absolute().inset_0().rounded(px(design::geo().menu_row_r)).overflow_hidden();
    let n = (size / 8.).ceil() as i32;
    for i in 0..n { for j in 0..n {
        if (i + j).rem_euclid(2) == 0 { cells = cells.child(div().absolute().left(px(i as f32 * 8.)).top(px(j as f32 * 8.)).size(px(8.)).bg(design::alpha(design::color::foreground(), 0.08))); }
    } }
    Swatch::new(id, design::color::background()).sized(size).selected(active).child(cells)
}

/// The active-color trigger (`ColorPickerTrigger`): its color, ringed while
/// its picker is open; no color (a mixed selection) shows the slash.
fn trigger(id: &'static str, color: Option<&str>, open: bool) -> Swatch {
    let s = swatch((id, 900usize), color.unwrap_or(colors::TRANSPARENT), 28., open);
    if color.is_none() { s.child(div().absolute().inset_0().flex().items_center().justify_center().child(picture("slash", 18., design::color::muted_foreground()))) } else { s }
}

fn heading(t: &'static str) -> Div { legend(t) }

/// `getColorNameAndShadeFromColor` for a hue (shaded colors only).
fn same(a: &str, b: &str) -> bool { a.eq_ignore_ascii_case(b) }

/// Upstream's `colors.*` name of a palette entry.
fn palette_name(i: usize) -> &'static str {
    match i { 0 => tr("透明", "Transparent"), 1 => tr("白", "White"), 3 => tr("黑", "Black"), _ => colors::name(colors::grid(0)[i]) }
}

/// One color control: the heading, the top picks and the trigger with its
/// picker. `action` turns a color into the `BoardAction`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn color_picker(id: &'static str, title: Option<&'static str>, p: Pop, current: Option<String>, picks: [&'static str; 5], action: fn(String) -> BoardAction, elements: &[crate::board::types::Element], window: &mut Window, cx: &mut Context<BoardView>) -> Div {
    let open = pop(cx) == Some(p);
    let eid = cx.entity_id();
    if open && p != Pop::CanvasBackground {
        let found = current.as_deref().and_then(colors::find);
        let shade = found.map(|f| f.1).unwrap_or(if p == Pop::Stroke { colors::STROKE_SHADE } else { colors::BACKGROUND_SHADE });
        OPEN.with(|m| {
            let mut m = m.borrow_mut();
            let customs = m.get(&eid).filter(|o| o.p == p).map(|o| o.customs.clone()).unwrap_or_else(|| most_used_custom(elements, p));
            m.insert(eid, Open { p, customs, current: current.clone(), shade, action });
        });
    } else if pop(cx).is_none_or(|q| !matches!(q, Pop::Stroke | Pop::Background)) {
        OPEN.with(|m| { m.borrow_mut().remove(&eid); });
    }
    let mut top = div().flex().items_center().justify_between();
    for (i, c) in picks.into_iter().enumerate() {
        let on = current.as_deref().is_some_and(|x| same(x, c));
        top = top.child(swatch((id, i), c, 20., on).ex_tip(c, None).on_click(cx.listener(move |me, _, window, cx| me.run(action(c.into()), window, cx))));
    }
    let at = TRIGGER.with(|t| t.borrow().get(&(eid, p)).copied());
    let label = if p == Pop::Stroke { tr("显示描边颜色选择器", "Show stroke color picker") } else { tr("显示背景颜色选择器", "Show background color picker") };
    let trig = trigger(id, current.as_deref(), open)
        .child(canvas(move |b, _, _| TRIGGER.with(|t| { t.borrow_mut().insert((eid, p), b); }), |_, _, _, _| {}).absolute().size_full())
        .when(!open, |d| d.ex_tip(label, None))
        .on_click(cx.listener(move |_, _, _, cx| { let open = pop(cx) == Some(p); set_pop(if !open { Some(p) } else if p == Pop::CanvasBackground { Some(Pop::MainMenu) } else { None }, cx) }));
    let popup = at.filter(|_| open).map(|b| picker(id, p, current.as_deref(), action, b, window, cx));
    div().flex().flex_col()
        .when_some(title, |d, t| d.child(legend(t)))
        // `.color-picker-container`: the picks, a rule, the trigger
        .child(div().flex().items_center().child(div().flex_1().child(top)).child(div().w(px(17.)).h(px(16.)).flex().justify_center().child(design::ButtonGroupSeparator::new().my(px(0.)))).child(trig))
        .children(popup)
}

fn set(c: String, action: fn(String) -> BoardAction, cx: &Context<BoardView>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |me: &mut BoardView, _: &ClickEvent, window, cx| me.run(action(c.clone()), window, cx))
}

/// `Picker` in a `PropertiesPopover` beside the trigger `b`.
fn picker(id: &'static str, p: Pop, current: Option<&str>, action: fn(String) -> BoardAction, b: Bounds<Pixels>, window: &mut Window, cx: &mut Context<BoardView>) -> AnyElement {
    let found = current.and_then(colors::find);
    let shade = found.map(|f| f.1).unwrap_or(if p == Pop::Stroke { colors::STROKE_SHADE } else { colors::BACKGROUND_SHADE });
    // the 5×3 palette: 28pt swatches 4 apart
    let mut grid = div().flex().flex_wrap().gap(px(4.)).w(px(5. * 28. + 4. * 4.));
    for (i, c) in colors::grid(shade).into_iter().enumerate() {
        let on = current.is_some_and(|x| same(x, c)) || found.is_some_and(|(h, _)| colors::HUES[h].1.contains(&c));
        let tip = if c.starts_with('#') { format!("{} {c}", palette_name(i)) } else { palette_name(i).to_string() };
        grid = grid.child(swatch((id, 100 + i), c, 28., on).child(hotkey_label(c, HOTKEYS[i].into())).ex_tip(&format!("{tip} — {}", HOTKEYS[i].to_uppercase()), None).on_click(set(c.into(), action, cx)));
    }
    let shades: AnyElement = match found {
        Some((h, s)) => {
            let mut r = div().flex().gap(px(4.));
            for (i, c) in colors::HUES[h].1.iter().enumerate() {
                r = r.child(swatch((id, 200 + i), c, 28., i == s).child(hotkey_label(c, format!("⇧{}", i + 1))).ex_tip(&format!("{} - {}", colors::HUES[h].0, i + 1), None).on_click(set(c.to_string(), action, cx)));
            }
            r.into_any_element()
        }
        None => div().h(px(28.)).flex().items_center().text_size(px(design::geo().text_xs)).text_color(design::color::muted_foreground()).child(tr("此颜色没有可用的明暗变化", "No shades available for this color")).into_any_element(),
    };
    // the hex input (`ColorInput`), made with the picker
    let eid = cx.entity_id();
    let field = HEX.with(|h| h.borrow().get(&eid).filter(|(q, _, _)| *q == p).map(|(_, f, _)| f.clone()));
    let field = field.unwrap_or_else(|| {
        let f = cx.new(|cx| { let mut t = TextField::new("", cx); t.size = 14.; t.line_height = 20.; t.max_len = Some(9); t });
        let sub = cx.subscribe_in(&f, window, move |me, f, ev: &TextFieldEvent, window, cx| match ev {
            TextFieldEvent::Changed => {
                let t = f.read(cx).text.trim().trim_start_matches('#').to_ascii_lowercase();
                if matches!(t.len(), 3 | 4 | 6 | 8) && t.chars().all(|c| c.is_ascii_hexdigit()) { me.run(action(format!("#{t}")), window, cx); }
            }
            TextFieldEvent::Submit | TextFieldEvent::Cancel => me.focus(window, cx),
            _ => {}
        });
        HEX.with(|h| h.borrow_mut().insert(eid, (p, f.clone(), sub)));
        f
    });
    let focused = field.read(cx).focus_handle(cx).is_focused(window);
    let want = current.unwrap_or("").trim_start_matches('#').to_string();
    let text = field.read(cx).text.clone();
    if !focused && text != want { field.update(cx, |t, cx| t.set_text(&want, cx)); }
    let valid = { let t = text.trim().trim_start_matches('#'); t.is_empty() || (matches!(t.len(), 3 | 4 | 6 | 8) && t.chars().all(|c| c.is_ascii_hexdigit())) };
    field.update(cx, |t, _| t.color = design::color::foreground());
    let hex = Field::new((id, 950usize)).sized(FieldSize::Sm).focused(focused).invalid(!valid).w_full().gap(px(6.))
        .child(div().text_color(design::color::muted_foreground()).child("#"))
        .child(div().flex_1().child(field));
    let customs = OPEN.with(|m| m.borrow().get(&eid).map(|o| o.customs.clone())).unwrap_or_default();
    let custom = (!customs.is_empty()).then(|| {
        let mut r = div().flex().flex_wrap().gap(px(4.));
        for (i, c) in customs.iter().enumerate() {
            let on = current.is_some_and(|x| same(x, c));
            r = r.child(swatch((id, 300 + i), c, 28., on).child(hotkey_label(c, (i + 1).to_string())).ex_tip(c, None).on_click(set(c.clone(), action, cx)));
        }
        div().flex().flex_col().child(heading(tr("常用自定义颜色", "Most used custom colors"))).child(r)
    });
    // the canvas background has no palette (`palette={null}`): just the hex
    let palette = p != Pop::CanvasBackground;
    let content = super::panel((id, 999usize)).padded().gap(px(12.)).w(px(5. * 28. + 4. * 4. + 2. * design::geo().popover_pad))
        // (the main menu stays open under the canvas background's)
        .on_mouse_down_out(cx.listener(move |_, _, _, cx| if pop(cx) == Some(p) { set_pop((p == Pop::CanvasBackground).then_some(Pop::MainMenu), cx) }))
        .when(palette, |d| d.children(custom)
        .child(div().flex().flex_col().child(heading(tr("颜色", "Colors"))).child(grid))
        .child(div().flex().flex_col().child(heading(tr("色调明暗", "Shades"))).child(shades)))
        .child(div().flex().flex_col().child(heading(tr("十六进制值", "Hex code"))).child(hex));
    // beside the panel (or the main menu), its top level with the trigger
    super::properties::beside(b, content)
}
