//! Vue : menu d'autocomplétion affiché sous le curseur, au-dessus du reste
//! de l'interface. La sélection et l'insertion sont des commandes du
//! `EditorViewModel` ; cette vue ne fait que les présenter.

use gpui::{AnyElement, App, Entity, Pixels, Point, anchored, deferred, div, prelude::*, px};

use super::theme;
use crate::model::completion::CompletionKind;
use crate::viewmodel::editor_vm::{CompletionMenu, EditorViewModel};

/// Nombre maximal de lignes visibles ; la fenêtre suit la sélection.
const VISIBLE: usize = 10;

fn kind_color(kind: CompletionKind) -> gpui::Rgba {
    gpui::rgb(match kind {
        CompletionKind::Snippet => 0x8e3fb5,
        CompletionKind::Keyword => 0x1d5fc4,
        CompletionKind::Name => 0x1c7c3a,
        CompletionKind::Arrow => 0xc0560c,
        CompletionKind::Directive => 0xb3261e,
    })
}

pub fn completion_menu(menu: &CompletionMenu, anchor: Point<Pixels>, vm: Entity<EditorViewModel>) -> AnyElement {
    let items = &menu.list.items;
    let first = menu.selected.saturating_sub(VISIBLE - 1).min(items.len().saturating_sub(VISIBLE));
    let rows = items.iter().enumerate().skip(first).take(VISIBLE).map(|(i, item)| {
        let selected = i == menu.selected;
        let vm = vm.clone();
        div()
            .id(i)
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(2.))
            .rounded_sm()
            .cursor_pointer()
            .when(selected, |s| s.bg(gpui::rgb(0xdbe6f6)))
            .when(!selected, |s| s.hover(|s| s.bg(theme::current_line())))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx: &mut App| {
                cx.stop_propagation();
                vm.update(cx, |vm, cx| vm.confirm_completion(Some(i), cx));
            })
            .child(
                div()
                    .w(px(64.))
                    .flex_none()
                    .text_size(px(10.5))
                    .text_color(kind_color(item.kind))
                    .child(item.kind.label()),
            )
            .child(div().flex_1().font_family(theme::CODE_FONT).text_size(px(13.)).text_color(theme::text()).child(item.label.clone()))
            .child(div().pl_3().text_size(px(11.5)).text_color(theme::muted()).child(item.detail))
    });
    let footer = (items.len() > VISIBLE).then(|| {
        div()
            .px_2()
            .pt_1()
            .text_size(px(10.5))
            .text_color(theme::faint())
            .child(format!("{}/{} — ↑↓ choisir, Entrée/Tab insérer, Échap fermer", menu.selected + 1, items.len()))
    });
    deferred(
        anchored().position(anchor).snap_to_window_with_margin(px(8.)).child(
            div()
                .id("completions")
                .occlude()
                .min_w(px(320.))
                .max_w(px(520.))
                .p_1()
                .bg(theme::surface())
                .border_1()
                .border_color(gpui::rgb(0xc9d0dd))
                .rounded_md()
                .shadow_lg()
                .font_family(theme::UI_FONT)
                .children(rows)
                .children(footer),
        ),
    )
    .with_priority(1)
    .into_any_element()
}
