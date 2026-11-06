//! Vue : petits composants réutilisables (boutons, champs, séparateurs).

use gpui::{AnyElement, AnyView, App, AppContext, Context, Div, ElementId, SharedString, Stateful, Window, div, prelude::*, px};

use super::theme;

/// Base commune des boutons, sans états de survol.
fn plain_button(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    let id = id.into();
    div()
        // sélecteur des tests d'interface (sans effet hors des tests)
        .debug_selector({
            let id = id.clone();
            move || id.to_string()
        })
        .id(ElementId::Name(id))
        .px_2()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .text_color(theme::text())
        .cursor_pointer()
        .child(label.into())
}

/// Bouton plat de barre d'outils.
pub fn button(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    plain_button(id, label).hover(|s| s.bg(theme::hover())).active(|s| s.bg(theme::pressed()))
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
    let b = plain_button(id, label).border_1();
    if selected {
        b.bg(theme::accent()).text_color(theme::on_accent()).border_color(theme::accent())
    } else {
        b.bg(theme::surface())
            .border_color(theme::field_border())
            .hover(|s| s.bg(theme::hover()))
            .active(|s| s.bg(theme::pressed()))
    }
}

/// Infobulle simple (texte), pour `.tooltip(...)`.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    move |_, cx| cx.new(|_| Tooltip(text.clone())).into()
}

struct Tooltip(SharedString);

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
            .child(self.0.clone())
    }
}

pub fn separator() -> Div {
    div().w(px(1.)).h(px(18.)).bg(theme::pressed()).mx_1()
}

pub fn hint(text: impl Into<SharedString>) -> AnyElement {
    div().text_size(px(12.)).text_color(theme::muted()).child(text.into()).into_any_element()
}
