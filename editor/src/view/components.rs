//! Vue : petits composants réutilisables (boutons, champs, séparateurs).

use gpui::{AnyElement, AnyView, App, AppContext, Context, Div, ElementId, SharedString, Stateful, Window, div, prelude::*, px};

use super::theme;

/// Bouton plat de barre d'outils.
pub fn button(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .px_2()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .text_color(theme::text())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .active(|s| s.bg(theme::pressed()))
        .child(label.into())
}

/// Cadre d'un champ de saisie d'une ligne.
pub fn field(content: impl IntoElement, width: Option<f32>) -> Div {
    let d = div()
        .h(px(26.))
        .px_2()
        .py(px(3.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::field_border())
        .rounded_md()
        .overflow_hidden()
        .child(content);
    match width {
        Some(w) => d.w(px(w)),
        None => d.flex_1(),
    }
}

/// Choix d'un groupe de boutons exclusifs ; le choix courant est mis en évidence.
pub fn choice(id: impl Into<SharedString>, label: impl Into<SharedString>, selected: bool) -> Stateful<Div> {
    let b = button(id, label).border_1().border_color(theme::field_border());
    if selected {
        b.bg(theme::accent()).text_color(theme::on_accent()).border_color(theme::accent()).hover(|s| s.bg(theme::accent()))
    } else {
        b.bg(theme::surface())
    }
}

/// Infobulle simple (texte), pour `.tooltip(...)`.
pub fn tooltip(text: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| cx.new(|_| Tooltip(text)).into()
}

struct Tooltip(&'static str);

impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(theme::panel())
            .border_1()
            .border_color(theme::field_border())
            .text_size(px(12.))
            .text_color(theme::text())
            .child(self.0)
    }
}

pub fn separator() -> Div {
    div().w(px(1.)).h(px(18.)).bg(theme::pressed()).mx_1()
}

pub fn hint(text: impl Into<SharedString>) -> AnyElement {
    div().text_size(px(12.)).text_color(theme::muted()).child(text.into()).into_any_element()
}
