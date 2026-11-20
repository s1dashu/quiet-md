//! The two surfaces of 模板: the bands under a blank page, and 模板库.

use crate::app::Workspace;
use crate::editor_theme::Theme as Page;
use crate::settings::tr;
use crate::ui::{icon, scrim};
use gpui::*;

/// One band under a blank page: what it is, and the click that starts it.
fn band(
    id: SharedString,
    emoji: String,
    title: String,
    desc: String,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_w(px(150.))
        .h(px(60.))
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(11.))
        .rounded(px(10.))
        .border_1()
        .border_color(Page::line_border())
        .bg(Page::bg_body())
        .shadow(crate::theme::shadow_button())
        .cursor_pointer()
        .hover(|s| s.bg(Page::fill_hover()).border_color(Page::button_border()))
        .child(
            div()
                .size(px(30.))
                .flex_none()
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .bg(Page::fill_hover())
                .child(div().text_size(px(16.)).child(emoji)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(Page::text())
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(Page::text_caption())
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(desc),
                ),
        )
        .on_click(on_click)
}

impl Workspace {
    /// 选择模板 under the page: a blank document offers it right after 新建, and
    /// `⋯` › 选择模板 opens it again for a page that already has content.
    ///
    /// The bands sit below the text rather than over it: the page being chosen
    /// for stays readable and writable while they are on screen.
    pub(crate) fn render_template_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let id = self.template_picker.as_ref()?;
        if self.view != crate::app::View::Doc {
            return None;
        }
        // The bands are about one page; leaving that page puts them away.
        if self.current.as_deref() != Some(id.as_str()) {
            return None;
        }

        // 空白文档 first — the page as it already is — then a few templates,
        // and 更多模板 at the end for the whole shelf.
        let mut tiles = div()
            .id("tpl-bands")
            .flex()
            .gap(px(10.))
            // A narrow window keeps the bands readable and scrolls the row.
            .overflow_x_scroll()
            .child(band(
                "tpl-blank".into(),
                "📄".into(),
                tr("空白文档", "Blank page").into(),
                tr("从头开始", "Start from scratch").into(),
                cx.listener(|w, _, _, cx| w.close_template_picker(cx)),
            ));
        for (i, card) in self.template_cards.iter().take(3).enumerate() {
            let name = card.name.clone();
            tiles = tiles.child(band(
                SharedString::from(format!("tpl-band-{i}")),
                card.emoji.clone(),
                card.title.clone(),
                card.desc.clone(),
                cx.listener(move |w, _, window, cx| w.apply_template(&name, window, cx)),
            ));
        }
        // 更多模板: everything, with room to browse and preview.
        tiles = tiles.child(band(
            "tpl-more".into(),
            "🗂".into(),
            tr("更多模板", "More templates").into(),
            tr("打开模板库", "Open the library").into(),
            cx.listener(|w, _, window, cx| w.open_template_lib(0, window, cx)),
        ));

        let strip = div()
            .flex_shrink_0()
            .w_full()
            .border_t_1()
            .border_color(Page::line_border())
            .bg(Page::bg_body())
            .px(px(24.))
            .pt(px(14.))
            .pb(px(14.))
            .flex()
            .flex_col()
            .child(tiles);

        Some(strip.into_any_element())
    }

    /// 模板库: names on the left (by shelf), the page itself on the right,
    /// 使用模板 in the corner.
    pub(crate) fn render_template_lib(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sel = self.template_lib?;
        if self.view != crate::app::View::Doc {
            return None;
        }
        let card = self.template_cards.get(sel)?;

        let rail = {
            let mut list = div().id("tpl-lib-list").flex_1().min_h_0().overflow_y_scroll().px(px(10.)).pb(px(14.));
            for (zh, en) in super::GROUPS.iter() {
                let items: Vec<usize> = self.template_cards.iter().enumerate().filter(|(_, c)| c.group == *zh).map(|(i, _)| i).collect();
                if items.is_empty() {
                    continue;
                }
                list = list.child(
                    div()
                        .px(px(9.))
                        .pt(px(12.))
                        .pb(px(4.))
                        .text_size(px(10.5))
                        .text_color(Page::text_caption())
                        .child(format!("{} ({})", tr(zh, en), items.len())),
                );
                for i in items {
                    let c = &self.template_cards[i];
                    let (emoji, title, on) = (c.emoji.clone(), c.title.clone(), i == sel);
                    let mut row = div()
                        .id(SharedString::from(format!("tpl-lib-{i}")))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(9.))
                        .py(px(6.))
                        .rounded(px(7.))
                        .border_1()
                        // The open one is a card of its own; the rest are rows.
                        .border_color(if on { Page::button_border() } else { transparent_black() })
                        .bg(if on { Page::bg_body() } else { transparent_black() })
                        .cursor_pointer()
                        .hover(|s| s.bg(Page::fill_hover()))
                        .child(div().text_size(px(13.)).child(emoji))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(12.))
                                .text_color(Page::text())
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(title),
                        )
                        .on_click(cx.listener(move |w, _, window, cx| w.choose_template(i, window, cx)));
                    if on {
                        row = row.shadow(crate::theme::shadow_button());
                    }
                    list = list.child(row);
                }
            }
            div()
                .w(px(240.))
                .flex_none()
                .h_full()
                .flex()
                .flex_col()
                .bg(Page::bg_overlay())
                .border_r_1()
                .border_color(Page::line_border())
                .child(div().px(px(18.)).pt(px(18.)).pb(px(10.)).text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Page::text()).child(tr("模板库", "Template library")))
                .child(list)
        };

        let name = card.name.clone();
        let right = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            // The page itself: the template in a read-only editor, cover, icon
            // and all. What is previewed is exactly what 使用模板 makes.
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .children(self.template_preview.clone()),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(Page::line_border())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .min_w_0()
                            .child(div().text_size(px(14.)).child(card.emoji.clone()))
                            .child(
                                div()
                                    .min_w_0()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .text_size(px(12.))
                                    .text_color(Page::text_caption())
                                    .child(card.desc.clone()),
                            ),
                    )
                    .child(
                        crate::ui::primary_button("tpl-lib-use", tr("使用模板", "Use this template"))
                            .on_click(cx.listener(move |w, _, window, cx| w.apply_template(&name, window, cx))),
                    ),
            )
            .child(
                div()
                    .id("tpl-lib-close")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .size(px(26.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(Page::bg_body())
                    .shadow(crate::theme::shadow_button())
                    .cursor_pointer()
                    .hover(|s| s.bg(Page::fill_hover()))
                    .child(icon("x", 14., Page::icon()))
                    .on_click(cx.listener(|w, _, _, cx| w.close_template_lib(cx))),
            );

        // Fit the window, but stay clear of the traffic lights: the dialog is
        // centred in what is left below the title bar.
        let (vw, vh) = (f32::from(window.viewport_size().width), f32::from(window.viewport_size().height));
        let panel = div()
            .id("tpl-lib-panel")
            .w(px(1060.))
            .h(px(640.))
            .max_w(px((vw - 40.).max(360.)))
            .max_h(px((vh - crate::app::HEADER_H - 40.).max(320.)))
            .flex()
            .overflow_hidden()
            .rounded(px(14.))
            .bg(Page::bg_body())
            .shadow(crate::theme::shadow_dialog())
            // Clicks inside belong to the dialog; the mask closes it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(rail)
            .child(right);

        Some(
            deferred(
                div()
                    .id("tpl-lib-mask")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pt(px(crate::app::HEADER_H))
                    .px(px(20.))
                    .pb(px(20.))
                    .bg(scrim())
                    .on_mouse_down(MouseButton::Left, cx.listener(|w, _, _, cx| w.close_template_lib(cx)))
                    .child(panel),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}
