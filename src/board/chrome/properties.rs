//! The styles panel, after Excalidraw `Actions.tsx` `SelectedShapeActions`,
//! `shapeActionPredicates.ts`, the actions' `PanelComponent`s
//! (`actionProperties.tsx`, `actionZindex.tsx`, `actionAlign.tsx`, …),
//! `IconPicker.tsx`, `FontPicker/*`, `Range.tsx` @ 3a317b5: a panel under the
//! main menu with upstream's sections in upstream's order; which sections show
//! follows upstream's predicates, each control shows `getFormValue` and
//! dispatches the matching `BoardAction`. Looks: a `design::Popover` 200 wide
//! (padded 16, scrolling under the shared scrollbar when short of room), each
//! section a `text-xs` legend over its controls, 12 apart: a choice is a row
//! of outline `Toggle`s in a `ButtonGroup` (the current one pressed) showing
//! upstream's pictures of what it draws, an action a `ButtonGroup` of outline
//! icon `Button`s, the opacity a `design::Slider`, the pickers `Popover`s
//! beside or under their trigger. Differs: no freedraw pressure, link or
//! bucket-fill controls; the font list has no top-picks drag & drop; the
//! arrowhead picker has no letter keys.

use super::*;
use super::color_picker::color_picker;
use super::tip::ExTip;
use crate::board::action::{BoardAction, End, Layer, Tool};
use crate::board::actions::{action_change_font_family, align_actions_predicate, can_group, can_ungroup, get_form_value, get_stroke_width_key_for_element, resolve_color_target, ColorTargetKind};
use crate::board::align::{Axis, Position};
use crate::board::app_state::{AppState, ArrowType, StrokeRoundness, StrokeWidthKey};
use crate::board::consts::{font_family, FONT_SIZES, STROKE_WIDTH};
use crate::board::selection::{get_selected_elements, get_target_elements, SelectedOpts};
use crate::board::sticky::{get_color_target_element, ColorProperty};
use crate::board::text::{should_allow_vertical_align, supports_horizontal_align};
use crate::board::types::{Arrowhead, Element, ElementType, FillStyle, StrokeStyle, TextAlign, VerticalAlign};
use crate::board::utils::*;
use crate::design::{self, Badge, Button, ButtonGroup, ButtonGroupSeparator, ButtonSize, MenuItem, MenuLabel, Toggle};
use crate::input::{TextField, TextFieldEvent};
use crate::settings::tr;
use std::rc::Rc;

/// The panel: padded as a popover (p-4), 200 wide, so its controls get 168.
pub(crate) const PAD: f32 = 16.;
pub(crate) const PANEL_W: f32 = 200.;
const W: f32 = PANEL_W - 2. * PAD;
/// Between its sections.
const GAP: f32 = 12.;
/// Below the main menu's bar, 16 apart.
const TOP: f32 = style::TOP_PADDING + design::toolbar::TOOLBAR_H + 16.;

fn tool_type(t: Tool) -> Option<ElementType> {
    Some(match t {
        Tool::Rectangle => ElementType::Rectangle,
        Tool::Diamond => ElementType::Diamond,
        Tool::Ellipse => ElementType::Ellipse,
        Tool::Arrow => ElementType::Arrow,
        Tool::Line => ElementType::Line,
        Tool::FreeDraw => ElementType::FreeDraw,
        Tool::Text => ElementType::Text,
        Tool::Image => ElementType::Image,
        Tool::Frame => ElementType::Frame,
        Tool::StickyNote => ElementType::StickyNote,
        Tool::Selection | Tool::Lasso | Tool::Hand | Tool::Eraser => return None,
    })
}

/// `getShapeActionPredicates`, for the sections we show.
#[derive(Clone)]
struct Show {
    extra: bool,
    stroke_color: bool,
    background_color: bool,
    fill: bool,
    stroke_width: bool,
    stroke_style: bool,
    sloppiness: bool,
    roundness: bool,
    arrow_type: bool,
    arrowheads: bool,
    text: bool,
    text_align: bool,
    vertical_align: bool,
    layers: bool,
    align: bool,
    distribute: bool,
    crop_editor: bool,
    line_editor: bool,
}

fn show(me: &BoardView, tool: Tool, a: &AppState, elements: &[Element], targets: &[&Element]) -> Show {
    let tt = tool_type(tool);
    let for_tool_or_sel = |p: fn(ElementType) -> bool| tt.is_some_and(p) || targets.iter().any(|e| p(e.ty()));
    let map = array_to_map(elements);
    let has_selection = !targets.is_empty();
    let single = targets.len() == 1;
    let single_container = targets.len() == 2 && (targets[0].has_bound_text() || targets[1].has_bound_text());
    // canChangeStrokeColor: one common type that isn't an image or a frame
    let common = targets.first().map(|e| e.ty()).filter(|t| targets.iter().all(|e| e.ty() == *t));
    let stroke_color = (tt.is_some_and(has_stroke_color) && !matches!(common, Some(ElementType::Image | ElementType::Frame | ElementType::MagicFrame)))
        || targets.iter().any(|e| has_stroke_color(e.ty()));
    let background_color = tt.is_some_and(has_background) || targets.iter().any(|e| has_background(get_color_target_element(e, ColorProperty::Background, &map).ty()));
    let freedraw_sel = get_selected_elements(elements, &a.selected_element_ids, SelectedOpts::default()).iter().any(|e| e.is_freedraw());
    Show {
        extra: has_selection && a.editing_text_element.is_none(),
        stroke_color,
        background_color,
        fill: (tt.is_some_and(has_fill_style) && !is_transparent(&a.current_item_background_color)) || targets.iter().any(|e| has_fill_style(e.ty()) && !is_transparent(&e.background_color)),
        stroke_width: for_tool_or_sel(has_stroke_width),
        stroke_style: for_tool_or_sel(has_stroke_style),
        sloppiness: for_tool_or_sel(has_roughness),
        roundness: for_tool_or_sel(can_change_roundness),
        arrow_type: for_tool_or_sel(tool_is_arrow),
        arrowheads: for_tool_or_sel(can_have_arrowheads),
        text: tool == Tool::Text || targets.iter().any(|e| e.is_text()),
        text_align: tool == Tool::Text || supports_horizontal_align(targets, &map),
        vertical_align: should_allow_vertical_align(targets, &map),
        layers: (tool != Tool::FreeDraw && !targets.iter().any(|e| e.is_freedraw())) || freedraw_sel,
        align: !single_container && align_actions_predicate(elements, a),
        distribute: targets.len() > 2,
        crop_editor: a.cropping_element_id.is_none() && single && targets[0].is_image(),
        line_editor: !me.linear.as_ref().is_some_and(|l| l.is_editing) && single && targets[0].is_linear() && !targets[0].is_elbow_arrow(),
    }
}

fn run(a: BoardAction, cx: &Context<BoardView>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |me, _, window, cx| me.run(a.clone(), window, cx))
}

/// A `<fieldset>`: the legend over its controls.
fn fieldset(t: &'static str) -> Div { div().flex().flex_col().child(legend(t)) }

/// A row of choices or actions as wide as the panel, each as wide as the next.
fn options<E: Styled + IntoElement + 'static>(items: impl IntoIterator<Item = E>) -> ButtonGroup {
    items.into_iter().fold(ButtonGroup::new().w_full(), |g, e| g.child(e.flex_1().min_w(px(0.))))
}

/// A `RadioSelection` option: an outline toggle showing upstream's picture.
fn opt(id: impl Into<ElementId>, pic: &str, on: bool, tip: &str, keys: Option<&str>, a: BoardAction, cx: &Context<BoardView>) -> Toggle {
    Toggle::new(id).outline().pressed(on).child(picture(pic, 16., design::color::foreground())).ex_tip(tip, keys).on_click(run(a, cx))
}

/// An action: an outline icon button (`glyph`: upstream's picture, or one of the app's icons).
fn act(id: impl Into<ElementId>, glyph: Svg, tip: &str, keys: Option<&str>, a: BoardAction, cx: &Context<BoardView>) -> Button {
    Button::outline(id).sized(ButtonSize::Sm).child(glyph).ex_tip(tip, keys).on_click(run(a, cx))
}

// ---- arrowheads (`IconPicker`)

/// `getArrowheadOptions`: the four shown, then "More options".
const VISIBLE: [(Option<Arrowhead>, &str); 4] = [(None, "q"), (Some(Arrowhead::Arrow), "w"), (Some(Arrowhead::Triangle), "e"), (Some(Arrowhead::TriangleOutline), "r")];
const HIDDEN: [(Option<Arrowhead>, &str); 5] = [(Some(Arrowhead::Circle), "a"), (Some(Arrowhead::CircleOutline), "s"), (Some(Arrowhead::Diamond), "d"), (Some(Arrowhead::DiamondOutline), "f"), (Some(Arrowhead::Bar), "z")];
const CARDINALITY: [Arrowhead; 6] = [Arrowhead::CardinalityOne, Arrowhead::CardinalityMany, Arrowhead::CardinalityOneOrMany, Arrowhead::CardinalityExactlyOne, Arrowhead::CardinalityZeroOrOne, Arrowhead::CardinalityZeroOrMany];

fn arrowhead_icon(h: Option<Arrowhead>) -> String { format!("arrowhead-{}", h.map(|h| h.as_str()).unwrap_or("none")) }

fn arrowhead_label(h: Option<Arrowhead>) -> &'static str {
    use Arrowhead::*;
    match h {
        None => tr("无", "None"),
        Some(Arrow) => tr("箭头", "Arrow"),
        Some(Bar) => tr("条状", "Bar"),
        Some(Circle) => tr("圆点", "Circle"),
        Some(CircleOutline) => tr("圆点（空心）", "Circle (outline)"),
        Some(Triangle) => tr("三角箭头", "Triangle"),
        Some(TriangleOutline) => tr("三角箭头（空心）", "Triangle (outline)"),
        Some(Diamond) => tr("菱形", "Diamond"),
        Some(DiamondOutline) => tr("菱形（空心）", "Diamond (outline)"),
        Some(CardinalityOne) => tr("基数（一个）", "Cardinality (one)"),
        Some(CardinalityMany) => tr("基数（多个）", "Cardinality (many)"),
        Some(CardinalityOneOrMany) => tr("基数（一个或多个）", "Cardinality (one or many)"),
        Some(CardinalityExactlyOne) => tr("基数（仅有一个）", "Cardinality (exactly one)"),
        Some(CardinalityZeroOrOne) => tr("基数（零个或一个）", "Cardinality (zero or one)"),
        Some(CardinalityZeroOrMany) => tr("基数（零个或多个）", "Cardinality (zero or many)"),
    }
}

/// The start picker shows the icons flipped (`getArrowheadOptions(!isRTL)`).
fn head_icon(h: Option<Arrowhead>, start: bool, size: Size<Pixels>, color: Hsla) -> Svg {
    let g = svg().path(SharedString::from(format!("icons/excalidraw/{}.svg", arrowhead_icon(h)))).w(size.width).h(size.height).flex_none().text_color(color);
    if start { g.with_transformation(Transformation::scale(gpui::size(-1., 1.))) } else { g }
}

thread_local! {
    /// The arrowhead picker's "More options" is open.
    static MORE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The trigger shows the current head; its picker hangs 4 under it: the
/// options as ghost toggles (the current one pressed) with their letters,
/// "More options" opening the rest and the cardinality heads.
fn arrowhead_picker(end: End, current: Option<Option<Arrowhead>>, cx: &Context<BoardView>) -> Toggle {
    let (p, start, id) = if end == End::Start { (Pop::StartArrowhead, true, "board-head-start") } else { (Pop::EndArrowhead, false, "board-head-end") };
    let open = pop(cx) == Some(p);
    let cur = current.flatten();
    let fg = design::color::foreground();
    let b = Toggle::new(id).outline().pressed(open).relative().child(head_icon(cur, start, size(px(16.), px(16.)), fg))
        .on_click(cx.listener(move |_, _, _, cx| { let o = pop(cx) == Some(p); if !o { MORE.with(|m| m.set(HIDDEN.iter().any(|(h, _)| current == Some(*h)) || current.flatten().is_some_and(|h| CARDINALITY.contains(&h)))); } set_pop(if o { None } else { Some(p) }, cx) }));
    if !open { return b; }
    let option = |i: usize, h: Option<Arrowhead>, k: Option<&'static str>| {
        Toggle::new((id, i)).pressed(current == Some(h)).relative().child(head_icon(h, start, size(px(36.), px(18.)), fg))
            .when_some(k, |d, k| d.child(keybinding(k)))
            .ex_tip(&match k { Some(k) => format!("{} — {}", arrowhead_label(h), k.to_uppercase()), None => arrowhead_label(h).to_string() }, None)
            .on_click(cx.listener(move |me, _, window, cx| me.run(BoardAction::Arrowhead(end, h), window, cx)))
    };
    let grid = |items: Vec<Toggle>| div().grid().grid_cols(4).gap(px(4.)).children(items);
    let more = MORE.with(|m| m.get());
    let mut sections = div().flex().flex_col().gap(px(4.))
        .child(grid(VISIBLE.iter().enumerate().map(|(i, (h, k))| option(i, *h, Some(*k))).collect()))
        .child(Button::ghost((id, 50usize)).sized(ButtonSize::Sm).label(tr("更多选项", "More options")).icon_end(if more { "chevron-down" } else { "chevron-right" }).w_full().justify_between()
            .on_click(cx.listener(|_, _, _, cx| { MORE.with(|m| m.set(!m.get())); cx.notify() })));
    if more {
        sections = sections.child(grid(HIDDEN.iter().enumerate().map(|(i, (h, k))| option(10 + i, *h, Some(*k))).collect()))
            .child(MenuLabel::new(tr("基数", "Cardinality")))
            .child(grid(CARDINALITY.iter().enumerate().map(|(i, h)| option(20 + i, Some(*h), None)).collect()));
    }
    let picker = panel((id, 99usize)).p(px(design::menu::PAD)).child(sections)
        .on_mouse_down_out(cx.listener(move |_, _, _, cx| if pop(cx) == Some(p) { set_pop(None, cx) }));
    b.child(div().absolute().top(px(design::toolbar::TOOLBAR_H - 2. * design::toolbar::TOOLBAR_PAD + design::menu::SIDE_OFFSET)).left_0()
        .child(deferred(anchored().snap_to_window_with_margin(px(design::menu::EDGE)).child(picker)).with_priority(3)))
}

// ---- fonts (`FontPicker`)

/// `DEFAULT_FONTS`: the top picks.
const FONT_PICKS: [u32; 3] = [font_family::EXCALIFONT, font_family::NUNITO, font_family::COMIC_SHANNS];

/// `getFontFamilyIcon`.
fn font_icon(f: u32) -> &'static str {
    match f {
        font_family::EXCALIFONT | font_family::VIRGIL => "freedraw",
        font_family::LILITA_ONE => "font-family-heading",
        font_family::COMIC_SHANNS | font_family::CASCADIA => "font-family-code",
        _ => "font-family-normal",
    }
}

/// `FONT_FAMILY` names (also the faces' family names).
fn font_name(f: u32) -> &'static str {
    match f {
        font_family::VIRGIL => "Virgil", font_family::HELVETICA => "Helvetica", font_family::CASCADIA => "Cascadia", font_family::EXCALIFONT => "Excalifont",
        font_family::NUNITO => "Nunito", font_family::LILITA_ONE => "Lilita One", font_family::COMIC_SHANNS => "Comic Shanns", font_family::LIBERATION_SANS => "Liberation Sans",
        _ => "Assistant",
    }
}

fn font_pick_label(f: u32) -> &'static str {
    match f { font_family::EXCALIFONT => tr("手写", "Hand-drawn"), font_family::NUNITO => tr("普通", "Normal"), font_family::COMIC_SHANNS => tr("代码", "Code"), _ => font_name(f) }
}

thread_local! {
    /// The open font list's search field and the font it hovers (by mouse
    /// or arrow keys).
    static FONT_LIST: RefCell<HashMap<EntityId, FontList>> = RefCell::default();
}

/// `snap`: the selection's family and the scene's families as the list
/// opened (upstream caches them so a preview doesn't reorder the list).
struct FontList { field: Entity<TextField>, _sub: Subscription, hovered: Option<u32>, focused: bool, snap: Option<(Option<u32>, Vec<u32>)> }

/// The font list opened or closed (`onPopupChange`): opening makes its
/// search field and caches the elements for the hover previews; closing
/// drops the preview.
pub(crate) fn font_popup_changed(open: bool, cx: &mut Context<BoardView>) {
    let id = cx.entity_id();
    let had = FONT_LIST.with(|m| m.borrow().contains_key(&id));
    let e = cx.entity().downgrade();
    if open && !had {
        let field = cx.new(|cx| { let mut t = TextField::new(tr("快速搜索", "Quick search"), cx); t.size = 14.; t.line_height = 16.; t.placeholder_color = design::color::placeholder(); t });
        let sub = cx.subscribe(&field, |me, f, ev: &TextFieldEvent, cx| font_list_key(me, &f, ev, cx));
        FONT_LIST.with(|m| m.borrow_mut().insert(id, FontList { field, _sub: sub, hovered: None, focused: false, snap: None }));
        cx.defer(move |cx| { let _ = e.update(cx, |me, _| { me.font_cache = Some(me.scene.elements_including_deleted().to_vec()); me.font_previewing = false; }); });
    } else if !open && had {
        FONT_LIST.with(|m| { m.borrow_mut().remove(&id); });
        cx.defer(move |cx| { let _ = e.update(cx, |me, cx| { crate::board::run::font_preview(me, None, cx); me.font_cache = None; me.refocus = true; cx.notify(); }); });
    }
}

/// `FontPickerList`'s fonts: those in the scene (deprecated ones too), then
/// the rest but the deprecated, by name, filtered by the search.
fn scene_families(me: &BoardView) -> Vec<u32> { me.elements().iter().filter(|e| !e.is_deleted).filter_map(|e| e.text().map(|t| t.font_family)).collect() }

fn font_lists(used: &[u32], term: &str) -> (Vec<u32>, Vec<u32>) {
    let all = [font_family::COMIC_SHANNS, font_family::EXCALIFONT, font_family::LILITA_ONE, font_family::NUNITO, font_family::CASCADIA, font_family::HELVETICA, font_family::VIRGIL];
    let mut sorted: Vec<u32> = all.to_vec();
    sorted.sort_by_key(|f| font_name(*f).to_lowercase());
    sorted.retain(|f| font_name(*f).to_lowercase().contains(term));
    let scene = sorted.iter().copied().filter(|f| used.contains(f)).collect();
    let rest = sorted.iter().copied().filter(|f| !used.contains(f) && !font_deprecated(*f)).collect();
    (scene, rest)
}

fn font_deprecated(f: u32) -> bool { matches!(f, font_family::CASCADIA | font_family::HELVETICA | font_family::VIRGIL) }

fn font_term(f: &Entity<TextField>, cx: &App) -> String { f.read(cx).text.trim().to_lowercase() }

/// `fontPickerKeyHandler` in the search field: ↑ / ↓ move the hover (and
/// its preview), Enter picks it, Escape closes the list.
fn font_list_key(me: &mut BoardView, f: &Entity<TextField>, ev: &TextFieldEvent, cx: &mut Context<BoardView>) {
    let id = cx.entity_id();
    let term = font_term(f, cx);
    let (hovered, snap) = FONT_LIST.with(|m| m.borrow().get(&id).map(|l| (l.hovered, l.snap.clone())).unwrap_or_default());
    let (current, used) = snap.unwrap_or_else(|| (Some(me.app.current_item_font_family), scene_families(me)));
    let (a, b) = font_lists(&used, &term);
    let list: Vec<u32> = a.into_iter().chain(b).collect();
    let at = hovered.or(current).and_then(|h| list.iter().position(|x| *x == h));
    let next = match ev {
        TextFieldEvent::Changed => {
            // a search hovers its first result (`hoveredFont`)
            if term.is_empty() || hovered.is_some_and(|h| list.contains(&h)) { cx.notify(); return; }
            list.first().copied()
        }
        TextFieldEvent::Down => at.map_or(list.first(), |i| list.get(i + 1).or(list.first())).copied(),
        TextFieldEvent::Up => at.map_or(list.last(), |i| if i == 0 { list.last() } else { list.get(i - 1) }).copied(),
        TextFieldEvent::Submit => {
            if let Some(h) = hovered { set_pop(None, cx); crate::board::run::font_preview(me, None, cx); pick_font(me, h, cx); }
            return;
        }
        TextFieldEvent::Cancel => { set_pop(None, cx); return; }
        _ => return,
    };
    FONT_LIST.with(|m| if let Some(l) = m.borrow_mut().get_mut(&id) { l.hovered = next; });
    crate::board::run::font_preview(me, next, cx);
    cx.notify();
}

/// `onSelect`: the family, and the list closes.
fn pick_font(me: &mut BoardView, f: u32, cx: &mut Context<BoardView>) {
    let r = { let m = me.m.clone(); let hooks = crate::board::Hooks(&*m); action_change_font_family(me.scene.elements_including_deleted(), &me.app, f, false, &*m, &hooks) };
    me.apply(r, cx);
    if let Some(t) = &mut me.text { t.restyle = true; }
}

fn font_picker(me: &BoardView, current: Option<u32>, window: &mut Window, cx: &mut Context<BoardView>) -> Div {
    let open = pop(cx) == Some(Pop::Font);
    let picks = options(FONT_PICKS.map(|f| opt(("board-font-pick", f as usize), font_icon(f), current == Some(f), font_pick_label(f), None, BoardAction::FontFamily(f), cx)));
    // the trigger (`FontPickerTrigger`); the list opens beside the panel, level with it
    let eid = cx.entity_id();
    let trig = Toggle::new("board-font-more").outline().icon("type").pressed(open)
        .child(canvas(move |b, _, _| TRIGGER.with(|t| { t.borrow_mut().insert(eid, b); }), |_, _, _, _| {}).absolute().size_full())
        .when(!open, |d| d.ex_tip(tr("显示字体选择器", "Show font picker"), None))
        .on_click(cx.listener(|_, _, _, cx| { let o = pop(cx) == Some(Pop::Font); set_pop(if o { None } else { Some(Pop::Font) }, cx) }));
    let state = FONT_LIST.with(|m| m.borrow_mut().get_mut(&cx.entity_id()).map(|l| {
        let snap = l.snap.get_or_insert_with(|| (current, scene_families(me))).clone();
        (l.field.clone(), l.hovered, l.focused, snap)
    }));
    let at = TRIGGER.with(|t| t.borrow().get(&eid).copied());
    let mut list_el = None;
    if open && let Some((field, hovered, focused, (current, used))) = state && let Some(b) = at {
        // the search field takes the keyboard (`onOpenAutoFocus`)
        if !focused {
            let fh = field.read(cx).focus_handle(cx);
            window.focus(&fh, cx);
            FONT_LIST.with(|m| if let Some(l) = m.borrow_mut().get_mut(&cx.entity_id()) { l.focused = true; });
        }
        field.update(cx, |t, _| { t.color = design::color::foreground(); t.placeholder_color = design::color::placeholder(); });
        let (scene, rest) = font_lists(&used, &font_term(&field, cx));
        let muted = design::color::muted_foreground();
        let item = |f: u32| {
            MenuItem::new(("board-font", f as usize)).checked(current == Some(f))
                // the list's hover is its own (mouse or keys), for the preview
                .selected(hovered == Some(f))
                .leading(picture(font_icon(f), 16., muted))
                .child(div().flex_1().min_w_0().font_family(font_name(f)).child(font_name(f)))
                .when(font_deprecated(f), |d| d.child(Badge::destructive(tr("旧的", "old"))))
                .on_hover(cx.listener(move |me, h: &bool, _, cx| if *h {
                    FONT_LIST.with(|m| if let Some(l) = m.borrow_mut().get_mut(&cx.entity_id()) { l.hovered = Some(f); });
                    crate::board::run::font_preview(me, Some(f), cx);
                    cx.notify();
                }))
                .on_click(cx.listener(move |me, _, _, cx| { set_pop(None, cx); crate::board::run::font_preview(me, None, cx); pick_font(me, f, cx) }))
        };
        let mut list = div().flex().flex_col();
        if !scene.is_empty() { list = list.child(MenuLabel::new(tr("画布中的", "In this scene"))).children(scene.iter().map(|f| item(*f))); }
        if !rest.is_empty() { list = list.child(MenuLabel::new(tr("可用字体", "Available fonts"))).children(rest.iter().map(|f| item(*f))); }
        if scene.is_empty() && rest.is_empty() {
            list = list.child(div().py(px(24.)).flex().justify_center().text_size(px(design::geo().text)).text_color(muted).child(tr("未找到字体", "No fonts found")));
        }
        // shadcn's `CommandInput`: h-9, the search icon, a rule under it
        let search = div().h(px(36.)).px(px(12.)).flex().items_center().gap(px(8.)).border_b_1().border_color(design::color::border())
            .child(design::icon("search", 16., muted)).child(div().flex_1().child(field));
        let content = panel("board-font-list").menu().pt(px(0.)).w(px(design::menu_w::SM)).child(search).child(list.pt(px(design::menu::PAD)))
            // `onPointerLeave`: the preview goes
            .on_hover(cx.listener(|me, h: &bool, _, cx| if !*h {
                FONT_LIST.with(|m| if let Some(l) = m.borrow_mut().get_mut(&cx.entity_id()) { l.hovered = None; });
                crate::board::run::font_preview(me, None, cx);
                cx.notify();
            }))
            .on_mouse_down_out(cx.listener(|_, _, _, cx| if pop(cx) == Some(Pop::Font) { set_pop(None, cx) }));
        list_el = Some(beside(b, content));
    }
    fieldset(tr("字体", "Font family")).child(div().flex().items_center()
        .child(picks.flex_1())
        .child(div().w(px(17.)).h(px(16.)).flex().justify_center().child(ButtonGroupSeparator::new().my(px(0.))))
        .child(trig))
        .children(list_el)
}

thread_local! {
    /// The font list's trigger, as last laid out (the list opens beside it).
    static TRIGGER: RefCell<HashMap<EntityId, Bounds<Pixels>>> = RefCell::default();
}

/// A picker beside the panel (shadcn's `side="right"`, 4 off the panel's
/// edge), its top level with its trigger `b` (at the panel's content edge).
pub(crate) fn beside(b: Bounds<Pixels>, picker: impl IntoElement) -> AnyElement {
    deferred(anchored().position(point(b.right() + px(PAD + design::menu::SIDE_OFFSET), b.top())).snap_to_window_with_margin(px(design::menu::EDGE)).child(picker)).with_priority(3).into_any_element()
}

// ---- opacity (`Range`)

/// The opacity range (`Range`): its legend with the value at the other end,
/// over a `design::Slider` in steps of 10; a mixed selection shows no range.
fn opacity(value: Option<f64>, fallback: f64, cx: &Context<BoardView>) -> Div {
    let v = value.unwrap_or(fallback).clamp(0., 100.);
    const THUMB: f32 = design::control::SLIDER_THUMB;
    // what the drag last set: several moves can land before the panel renders again, and `value` is the frame's
    let last = Rc::new(std::cell::Cell::new(value));
    let set = move |x: Pixels, bounds: Bounds<Pixels>, me: &mut BoardView, window: &mut Window, cx: &mut Context<BoardView>| {
        let t = ((f32::from(x - bounds.left()) - THUMB / 2.) / (f32::from(bounds.size.width) - THUMB)).clamp(0., 1.);
        let nv = ((t * 10.).round() * 10.) as f64;
        if slid_to(&last, nv) { me.run(BoardAction::Opacity(nv), window, cx) }
    };
    let bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::default()));
    let (b1, b2, b3, set2) = (bounds.clone(), bounds.clone(), bounds.clone(), set.clone());
    let g = design::geo();
    div().flex().flex_col()
        .child(legend(tr("透明度", "Opacity")).flex().justify_between().child(div().child(format!("{}", v.round() as i64))))
        .child(div().id("board-opacity").relative().w_full().py(px(2.))
        .child(design::Slider::new("board-opacity-slider", (v / 100.) as f32).mixed(value.is_none()))
        .child(canvas(move |b, _, _| b1.set(b), |_, _, _, _| {}).absolute().inset_0())
        // one undo step per drag: the steps aren't captured till the release
        .on_mouse_down(MouseButton::Left, cx.listener(move |me, ev: &MouseDownEvent, window, cx| { me.begin_slider(); set(ev.position.x, b2.get(), me, window, cx) }))
        .on_mouse_move(cx.listener(move |me, ev: &MouseMoveEvent, window, cx| if ev.pressed_button == Some(MouseButton::Left) && me.sliding() { set2(ev.position.x, b3.get(), me, window, cx) }))
        .on_mouse_up(MouseButton::Left, cx.listener(|me, _, _, cx| me.end_slider(cx)))
        .on_mouse_up_out(MouseButton::Left, cx.listener(|me, _, _, cx| me.end_slider(cx))))
        .text_size(px(g.text_xs))
}

/// The slider moved to `nv`: it differs from what was last set (the frame's value at first).
fn slid_to(last: &std::cell::Cell<Option<f64>>, nv: f64) -> bool { last.replace(Some(nv)) != Some(nv) }

/// What the panel shows, worked out from the selection (each value is
/// `getFormValue`'s; `None` is a mixed selection).
#[derive(Clone)]
struct Model {
    s: Show,
    stroke_kind: ColorTargetKind,
    bg_kind: ColorTargetKind,
    stroke: Option<String>,
    background: Option<String>,
    fill: Option<FillStyle>,
    width: Option<StrokeWidthKey>,
    style_v: Option<StrokeStyle>,
    rough: Option<i64>,
    round: Option<StrokeRoundness>,
    arrow_type: Option<ArrowType>,
    start_head: Option<Option<Arrowhead>>,
    end_head: Option<Option<Arrowhead>>,
    family: Option<u32>,
    font_size: Option<f64>,
    align: Option<TextAlign>,
    valign: Option<VerticalAlign>,
    opacity_v: Option<f64>,
    zigzag: bool,
    all_hachure: bool,
    can_group: bool,
    editing_arrow: bool,
}

fn model(me: &BoardView) -> Option<Model> {
    let scope = me.selection_scope();
    let (a, elements, tool) = (me.app_state(), scope.as_deref().unwrap_or(me.elements()), me.tool());
    if !crate::board::selection::show_selected_shape_actions(a, elements) { return None; }
    let targets = get_target_elements(elements, a);
    let s = show(me, tool, a, elements, &targets);
    let stroke_target = resolve_color_target(a, elements, ColorProperty::Stroke);
    let bg_target = resolve_color_target(a, elements, ColorProperty::Background);
    let stroke = get_form_value(elements, a, |e| has_stroke_color(e.ty()).then(|| e.stroke_color.clone()), |e| has_stroke_color(e.ty()), |sel| (!sel).then(|| stroke_target.current_value.clone()));
    let background = get_form_value(elements, a, |e| has_background(e.ty()).then(|| e.background_color.clone()), |e| has_background(e.ty()), |sel| (!sel).then(|| bg_target.current_value.clone()));
    let fill = get_form_value(elements, a, |e| has_fill_style(e.ty()).then_some(e.fill_style), |e| has_fill_style(e.ty()), |sel| (!sel).then_some(a.current_item_fill_style));
    let width = get_form_value(elements, a, |e| if has_stroke_width(e.ty()) { get_stroke_width_key_for_element(e) } else { None }, |e| has_stroke_width(e.ty()), |sel| (!sel).then_some(a.current_item_stroke_width_key));
    let style_v = get_form_value(elements, a, |e| has_stroke_style(e.ty()).then_some(e.stroke_style), |e| has_stroke_style(e.ty()), |sel| (!sel).then_some(a.current_item_stroke_style));
    let rough = get_form_value(elements, a, |e| Some(e.roughness.round() as i64), |_| true, |sel| (!sel).then_some(a.current_item_roughness.round() as i64));
    let round = get_form_value(elements, a, |e| can_change_roundness(e.ty()).then(|| if e.roundness.is_some() { StrokeRoundness::Round } else { StrokeRoundness::Sharp }), |e| can_change_roundness(e.ty()), |sel| (!sel).then_some(a.current_item_roundness));
    let arrow_type = get_form_value(elements, a, |e| e.is_arrow().then(|| if e.is_elbow_arrow() { ArrowType::Elbow } else if e.roundness.is_some() { ArrowType::Round } else { ArrowType::Sharp }), |e| e.is_arrow(), |sel| (!sel).then_some(a.current_item_arrow_type));
    let start_head = get_form_value(elements, a, |e| if e.is_linear() && can_have_arrowheads(e.ty()) { Some(e.linear().and_then(|l| l.start_arrowhead)) } else { Some(a.current_item_start_arrowhead) }, |_| true, |sel| (!sel).then_some(a.current_item_start_arrowhead));
    let end_head = get_form_value(elements, a, |e| if e.is_linear() && can_have_arrowheads(e.ty()) { Some(e.linear().and_then(|l| l.end_arrowhead)) } else { Some(a.current_item_end_arrowhead) }, |_| true, |sel| (!sel).then_some(a.current_item_end_arrowhead));
    // text values come from the text itself or a container's label
    let map = array_to_map(elements);
    let text_of = |e: &Element| -> Option<Element> { if e.is_text() { Some(e.clone()) } else { crate::board::bounds::get_bound_text_element(e, &map).cloned() } };
    let family = get_form_value(elements, a, |e| text_of(e).and_then(|t| t.text().map(|t| t.font_family)), |e| e.is_text() || e.has_bound_text(), |sel| (!sel).then_some(a.current_item_font_family));
    let font_size = get_form_value(elements, a, |e| text_of(e).and_then(|t| t.text().map(|t| t.font_size)), |e| e.is_text() || e.has_bound_text(), |sel| (!sel).then_some(a.current_item_font_size));
    let align = get_form_value(elements, a, |e| text_of(e).and_then(|t| t.text().map(|t| t.text_align)), |e| e.is_text() || e.has_bound_text(), |sel| (!sel).then_some(a.current_item_text_align));
    let valign = get_form_value(elements, a, |e| text_of(e).and_then(|t| t.text().map(|t| t.vertical_align)), |e| e.is_text() || e.has_bound_text(), |sel| (!sel).then_some(VerticalAlign::Middle));
    let opacity_v = get_form_value(elements, a, |e| Some(e.opacity), |_| true, |sel| (!sel).then_some(a.current_item_opacity));
    // the first fill option is hachure, or zigzag when all are; ⌥-click on
    // hachure when all are hachure makes zigzag
    let fills: Vec<FillStyle> = if s.fill { get_selected_elements(elements, &a.selected_element_ids, SelectedOpts::default()).iter().filter(|e| has_fill_style(e.ty())).map(|e| e.fill_style).collect() } else { vec![] };
    Some(Model {
        stroke_kind: stroke_target.kind, bg_kind: bg_target.kind, stroke, background, fill, width, style_v, rough, round, arrow_type, start_head, end_head,
        family, font_size, align, valign, opacity_v,
        zigzag: !fills.is_empty() && fills.iter().all(|f| *f == FillStyle::Zigzag),
        all_hachure: fills.iter().all(|f| *f == FillStyle::Hachure),
        can_group: s.extra && can_group(elements, a),
        editing_arrow: targets.first().is_some_and(|e| e.is_arrow()),
        s,
    })
}

/// The last `Model` and what it was worked out from: the board, the
/// selection scope but for moves (the panel doesn't depend on where elements
/// are), the app state but for the viewport, and whether a line editor is open.
struct Memo { board: EntityId, scope: crate::board::ScopeStamp, app: AppState, line_editing: bool, model: Option<Rc<Model>> }

thread_local! { static MEMO: RefCell<Option<Memo>> = const { RefCell::new(None) }; }

/// `model`, kept while the board changes only by moves, panning or zooming
/// (each drag step would otherwise go over the whole selection a dozen times).
fn cached_model(me: &BoardView, board: EntityId) -> Option<Rc<Model>> {
    let a = me.app_state();
    let line_editing = me.linear.as_ref().is_some_and(|l| l.is_editing);
    MEMO.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(c) = m.as_mut().filter(|c| c.board == board && c.line_editing == line_editing && me.scope_unchanged(&c.scope)) {
            (c.app.scroll_x, c.app.scroll_y, c.app.zoom) = (a.scroll_x, a.scroll_y, a.zoom);
            if c.app == *a { return c.model.clone(); }
        }
        let model = model(me).map(Rc::new);
        *m = Some(Memo { board, scope: me.scope_stamp(), app: a.clone(), line_editing, model: model.clone() });
        model
    })
}

pub(crate) fn render(me: &BoardView, window: &mut Window, cx: &mut Context<BoardView>) -> Option<AnyElement> {
    let model = cached_model(me, cx.entity_id())?;
    let a = me.app_state();
    let Model { s, stroke_kind, bg_kind, stroke, background, fill, width, style_v, rough, round, arrow_type, start_head, end_head, family, font_size, align, valign, opacity_v, zigzag, all_hachure, can_group, editing_arrow } = (*model).clone();

    // `.selected-shape-actions`: a column of sections
    let mut body = div().w(px(W)).flex().flex_col().gap(px(GAP));
    let fg = design::color::foreground();
    if s.stroke_color {
        let title = if stroke_kind == ColorTargetKind::Sticky { tr("文字颜色", "Text color") } else { tr("描边", "Stroke") };
        body = body.child(color_picker("board-stroke", Some(title), Pop::Stroke, stroke, colors::STROKE_PICKS, BoardAction::StrokeColor, me.elements(), window, cx));
    }
    if s.background_color {
        let picks = if bg_kind == ColorTargetKind::Sticky { colors::STICKY_BACKGROUND_PICKS } else { colors::BACKGROUND_PICKS };
        body = body.child(color_picker("board-bg", Some(tr("背景", "Background")), Pop::Background, background, picks, BoardAction::BackgroundColor, me.elements(), window, cx));
    }
    if s.fill {
        let first_tip = format!("{} ({}-Click)", if zigzag { tr("之字形折线", "Zigzag") } else { tr("斜线", "Hachure") }, if cfg!(target_os = "macos") { "Option" } else { "Alt" });
        let first_on = zigzag || fill == Some(FillStyle::Hachure);
        body = body.child(fieldset(tr("填充", "Fill")).child(options([
            Toggle::new(("board-fill", 0usize)).outline().pressed(first_on).child(picture(if zigzag { "fill-zigzag" } else { "fill-hachure" }, 16., fg))
                .ex_tip(&first_tip, None)
                .on_click(cx.listener(move |me, ev: &ClickEvent, window, cx| { let f = if ev.modifiers().alt && all_hachure { FillStyle::Zigzag } else { FillStyle::Hachure }; me.run(BoardAction::FillStyle(f), window, cx) })),
            opt(("board-fill", 1usize), "fill-cross-hatch", fill == Some(FillStyle::CrossHatch), tr("交叉线条", "Cross-hatch"), None, BoardAction::FillStyle(FillStyle::CrossHatch), cx),
            opt(("board-fill", 2usize), "fill-solid", fill == Some(FillStyle::Solid), tr("实心", "Solid"), None, BoardAction::FillStyle(FillStyle::Solid), cx)])));
    }
    if s.stroke_width {
        let o = |k: StrokeWidthKey, i: usize, ic, tip| opt(("board-width", i), ic, width == Some(k), tip, None, BoardAction::StrokeWidth(STROKE_WIDTH[i]), cx);
        body = body.child(fieldset(tr("描边宽度", "Stroke width")).child(options([
            o(StrokeWidthKey::Thin, 0, "stroke-width-base", tr("细", "Thin")),
            o(StrokeWidthKey::Medium, 1, "stroke-width-bold", tr("粗", "Bold")),
            o(StrokeWidthKey::Bold, 2, "stroke-width-extra-bold", tr("特粗", "Extra bold"))])));
    }
    if s.stroke_style {
        let o = |v: StrokeStyle, ic, tip| opt(("board-style", v as usize), ic, style_v == Some(v), tip, None, BoardAction::StrokeStyle(v), cx);
        body = body.child(fieldset(tr("边框样式", "Stroke style")).child(options([
            o(StrokeStyle::Solid, "stroke-width-base", tr("实线", "Solid")),
            o(StrokeStyle::Dashed, "stroke-style-dashed", tr("虚线", "Dashed")),
            o(StrokeStyle::Dotted, "stroke-style-dotted", tr("点虚线", "Dotted"))])));
    }
    if s.sloppiness {
        let o = |v: i64, ic, tip| opt(("board-rough", v as usize), ic, rough == Some(v), tip, None, BoardAction::Roughness(v as f64), cx);
        body = body.child(fieldset(tr("线条风格", "Sloppiness")).child(options([
            o(0, "sloppiness-architect", tr("朴素", "Architect")),
            o(1, "sloppiness-artist", tr("艺术", "Artist")),
            o(2, "sloppiness-cartoonist", tr("漫画家", "Cartoonist"))])));
    }
    if s.roundness {
        body = body.child(fieldset(tr("边角", "Edges")).child(options([
            opt("board-edge-sharp", "edge-sharp", round == Some(StrokeRoundness::Sharp), tr("尖锐", "Sharp"), None, BoardAction::Roundness(false), cx),
            opt("board-edge-round", "edge-round", round == Some(StrokeRoundness::Round), tr("圆润", "Round"), None, BoardAction::Roundness(true), cx)])));
    }
    if s.arrow_type {
        let o = |v: ArrowType, ic, tip| opt(("board-arrow-type", v as usize), ic, arrow_type == Some(v), tip, None, BoardAction::ArrowType(v), cx);
        body = body.child(fieldset(tr("箭头类型", "Arrow type")).child(options([
            o(ArrowType::Sharp, "arrow-sharp", tr("直箭头", "Sharp arrow")),
            o(ArrowType::Round, "arrow-round", tr("曲线箭头", "Curved arrow")),
            o(ArrowType::Elbow, "arrow-elbow", tr("拐角箭头", "Elbow arrow"))])));
    }
    if s.text {
        body = body.child(font_picker(me, family, window, cx));
        let sizes = [("font-size-small", tr("小", "Small")), ("font-size-medium", tr("中", "Medium")), ("font-size-large", tr("大", "Large")), ("font-size-extra-large", tr("加大", "Very large"))];
        body = body.child(fieldset(tr("字体大小", "Font size")).child(options(sizes.into_iter().enumerate()
            .map(|(i, (ic, tip))| opt(("board-font-size", i), ic, font_size == Some(FONT_SIZES[i]), tip, None, BoardAction::FontSize(FONT_SIZES[i]), cx)))));
        if s.text_align {
            let o = |v: TextAlign, ic, tip| opt(("board-align-text", v as usize), ic, align == Some(v), tip, None, BoardAction::TextAlign(v), cx);
            body = body.child(fieldset(tr("文本对齐", "Text align")).child(options([
                o(TextAlign::Left, "text-align-left", tr("左对齐", "Left")),
                o(TextAlign::Center, "text-align-center", tr("居中", "Center")),
                o(TextAlign::Right, "text-align-right", tr("右对齐", "Right"))])));
        }
    }
    if s.vertical_align {
        // (no legend upstream)
        let o = |v: VerticalAlign, ic, tip| opt(("board-valign", v as usize), ic, valign == Some(v), tip, None, BoardAction::VerticalAlign(v), cx);
        body = body.child(options([
            o(VerticalAlign::Top, "text-align-top", tr("顶部对齐", "Align top")),
            o(VerticalAlign::Middle, "text-align-middle", tr("垂直居中", "Center vertically")),
            o(VerticalAlign::Bottom, "text-align-bottom", tr("底端对齐", "Align bottom"))]));
    }
    if s.arrowheads {
        body = body.child(fieldset(tr("端点", "Arrowheads")).child(options([arrowhead_picker(End::Start, start_head, cx), arrowhead_picker(End::End, end_head, cx)])));
    }
    body = body.child(opacity(opacity_v, a.current_item_opacity, cx));
    if s.layers {
        let o = |l: Layer, ic, tip, k: &str| act(("board-layer", l as usize), picture(ic, 16., fg), tip, Some(&shortcut(k)), BoardAction::Layer(l), cx);
        body = body.child(fieldset(tr("图层", "Layers")).child(options([
            o(Layer::Back, "send-to-back", tr("置于底层", "Send to back"), if cfg!(target_os = "macos") { "CtrlOrCmd+Alt+[" } else { "CtrlOrCmd+Shift+[" }),
            o(Layer::Backward, "send-backward", tr("下移一层", "Send backward"), "CtrlOrCmd+["),
            o(Layer::Forward, "bring-forward", tr("上移一层", "Bring forward"), "CtrlOrCmd+]"),
            o(Layer::Front, "bring-to-front", tr("置于顶层", "Bring to front"), if cfg!(target_os = "macos") { "CtrlOrCmd+Alt+]" } else { "CtrlOrCmd+Shift+]" })])));
    }
    if s.align {
        let o = |i: usize, ic, tip, k: Option<&str>, a| act(("board-align", i), picture(ic, 16., fg), tip, k.map(shortcut).as_deref(), a, cx);
        // two rows
        let mut x = vec![o(0, "align-left", tr("左对齐", "Align left"), Some("CtrlOrCmd+Shift+Left"), BoardAction::Align(Axis::X, Position::Start)),
            o(1, "center-horizontally", tr("水平居中", "Center horizontally"), None, BoardAction::Align(Axis::X, Position::Center)),
            o(2, "align-right", tr("右对齐", "Align right"), Some("CtrlOrCmd+Shift+Right"), BoardAction::Align(Axis::X, Position::End))];
        let mut y = vec![o(4, "align-top", tr("顶部对齐", "Align top"), Some("CtrlOrCmd+Shift+Up"), BoardAction::Align(Axis::Y, Position::Start)),
            o(5, "center-vertically", tr("垂直居中", "Center vertically"), None, BoardAction::Align(Axis::Y, Position::Center)),
            o(6, "align-bottom", tr("底端对齐", "Align bottom"), Some("CtrlOrCmd+Shift+Down"), BoardAction::Align(Axis::Y, Position::End))];
        if s.distribute {
            x.push(o(3, "distribute-horizontally", tr("水平等距分布", "Distribute horizontally"), Some("Alt+H"), BoardAction::Distribute(Axis::X)));
            y.push(o(7, "distribute-vertically", tr("垂直等距分布", "Distribute vertically"), Some("Alt+V"), BoardAction::Distribute(Axis::Y)));
        }
        body = body.child(fieldset(tr("对齐", "Align")).child(div().flex().flex_col().gap(px(8.)).child(options(x)).child(options(y))));
    }
    if s.extra {
        let mut v = vec![act("board-act-duplicate", design::icon("copy", 16., fg), tr("创建副本", "Duplicate"), Some(&shortcut("CtrlOrCmd+D")), BoardAction::Duplicate, cx),
            act("board-act-delete", design::icon("bin", 16., fg), tr("删除", "Delete"), None, BoardAction::Delete, cx)];
        if can_group { v.push(act("board-act-group", picture("group", 16., fg), tr("编组", "Group selection"), Some(&shortcut("CtrlOrCmd+G")), BoardAction::Group, cx)); }
        if can_ungroup(a) { v.push(act("board-act-ungroup", picture("ungroup", 16., fg), tr("解除编组", "Ungroup selection"), Some(&shortcut("CtrlOrCmd+Shift+G")), BoardAction::Ungroup, cx)); }
        if s.crop_editor { v.push(act("board-act-crop", picture("crop", 16., fg), tr("裁剪图片", "Crop image"), None, BoardAction::ToggleCrop, cx)); }
        if s.line_editor { v.push(act("board-act-line", picture("line-editor", 16., fg), if editing_arrow { tr("编辑箭头", "Edit arrow") } else { tr("编辑线条", "Edit line") }, None, BoardAction::ToggleLinearEditor, cx)); }
        body = body.child(fieldset(tr("操作", "Actions")).child(options(v)));
    }

    // `appState.height - 166`
    let max_h = f32::from((me.bounds.get().size.height - px(166.)).max(px(120.)));
    Some(div().absolute().left(px(style::PADDING)).top(px(TOP))
        .child(panel("board-props").fit(max_h).inset(PAD).w(px(PANEL_W)).child(div().p(px(PAD)).child(body)))
        .into_any_element())
}

#[cfg(test)]
mod tests {
    use super::slid_to;
    use std::cell::Cell;

    #[test]
    fn a_slider_dragged_back_within_one_frame_lands_where_it_was_let_go() {
        // the frame showed 100; one burst of moves: 60, 90, back to 100, 100 again
        let last = Cell::new(Some(100.));
        assert_eq!([60., 90., 100., 100.].map(|v| slid_to(&last, v)), [true, true, true, false]);
        // a mixed selection (no value) takes the first step whatever it is
        let mixed = Cell::new(None);
        assert!(slid_to(&mixed, 100.));
    }
}
