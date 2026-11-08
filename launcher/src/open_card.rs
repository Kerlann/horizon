//! The sidebar's card above the links: what Horizon is, and a way into the mod API's open tasks.
//! No picture, no tracking, nothing fetched: the words are the card.
use gpui::{div, prelude::*, px, ClickEvent, Div, FontWeight, Hsla, SharedString, Stateful};
use horizon_launcher::i18n::tr;

/// The board's task view, where a visitor picks something to build.
pub const TASKS_URL: &str = "https://github.com/users/arismoko/projects/1/views/3";

/// The colours the card takes from the sidebar it sits in.
#[derive(Clone, Copy)]
pub struct Palette {
    pub ink: Hsla,
    pub soft: Hsla,
    pub accent: Hsla,
}

/// The card: a short accent rule, the headline (its second line in the accent), a line of
/// small print and an underlined link to `TASKS_URL`. `font` is the sidebar's face.
pub fn card(
    id: &'static str,
    font: &'static str,
    p: Palette,
    t: &crate::theme::Theme,
) -> Stateful<Div> {
    let headline = |size: f32, color: Hsla, text: &'static str| {
        div()
            .text_size(px(size))
            .line_height(px(size * 1.1))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .child(text)
    };
    div()
        .id(id)
        .flex()
        .flex_col()
        .px(px(8.))
        .font_family(font)
        .child(div().w(px(32.)).h(px(3.)).rounded(px(2.)).bg(p.accent))
        .child(
            div()
                .mt(px(14.))
                .flex()
                .flex_col()
                .child(headline(30., p.ink, tr("This launcher is open.")))
                .child(headline(30., p.accent, tr("So is the game."))),
        )
        .child(
            div()
                .mt(px(14.))
                .text_size(px(12.))
                .text_color(p.soft)
                .child(tr("MIT licensed. No ads, no telemetry.")),
        )
        .child(crate::keyboard(
            div()
                .id(SharedString::from(format!("{id}-link")))
                .mt(px(18.))
                .flex()
                .items_center()
                .gap(px(8.))
                .pb(px(3.))
                .border_b_1()
                .border_color(p.accent)
                .cursor_pointer()
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.ink)
                .child(tr("Help build the mod API"))
                .child(crate::glass_ui::icon("arrow-right-01", 14., p.ink))
                .on_click(|_: &ClickEvent, _, cx| cx.open_url(TASKS_URL)),
            t,
        ))
}
