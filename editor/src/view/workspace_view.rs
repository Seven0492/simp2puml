//! Vue de l'espace de travail : compose barre d'outils, onglets, éditeur,
//! aperçu, diagnostics et barre d'état ; répond aux demandes du ViewModel
//! (sélecteurs de fichiers, confirmations, focus).

use std::collections::{HashMap, HashSet};

use gpui::{
    App, ClickEvent, Context, Entity, EntityId, FocusHandle, Focusable, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, PromptLevel, SharedString,
    Subscription, Window, div, prelude::*, px, uniform_list,
};

use super::actions::*;
use super::components::{button, choice, field, hint, separator, tooltip};
use super::editor_view::EditorView;
use super::preview_view::PreviewView;
use super::theme;
use crate::model::preferences::ThemeChoice;
use crate::model::templates::TEMPLATES;
use crate::viewmodel::document_vm::{DocumentEvent, DocumentViewModel};
use crate::viewmodel::editor_vm::EditorViewModel;
use crate::viewmodel::workspace_vm::{PathPurpose, WorkspaceEvent, WorkspaceViewModel};

pub struct WorkspaceView {
    vm: Entity<WorkspaceViewModel>,
    /// Une vue par ViewModel d'édition (documents, recherche, chemin).
    editors: HashMap<EntityId, Entity<EditorView>>,
    previews: HashMap<EntityId, Entity<PreviewView>>,
    document_subscriptions: HashMap<EntityId, Subscription>,
    focus_handle: FocusHandle,
    dragging_split: bool,
    /// Dernier titre donné à la fenêtre (il suit le texte et l'onglet actif).
    title: String,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for WorkspaceView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl WorkspaceView {
    pub fn new(vm: Entity<WorkspaceViewModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe_in(&vm, window, |this, _, window, cx| {
                this.forget_closed_documents(cx);
                // aussi hors du rendu : une fenêtre cachée ne se redessine pas, sa barre de titre reste à jour
                this.sync_title(window, cx);
                cx.notify();
            }),
            cx.subscribe_in(&vm, window, |this, _, ev: &WorkspaceEvent, window, cx| this.on_workspace_event(ev, window, cx)),
            // thème « Système » : suivre les changements d'apparence
            cx.observe_window_appearance(window, |_, window, cx| {
                window.refresh();
                cx.notify();
            }),
        ];
        window.on_window_should_close(cx, {
            let vm = vm.downgrade();
            move |_, cx| {
                let Some(vm) = vm.upgrade() else { return true };
                if !vm.read(cx).has_unsaved(cx) {
                    return true;
                }
                vm.update(cx, |vm, cx| vm.request_quit(cx));
                false
            }
        });
        let mut view = Self {
            vm: vm.clone(),
            editors: HashMap::new(),
            previews: HashMap::new(),
            document_subscriptions: HashMap::new(),
            focus_handle: cx.focus_handle(),
            dragging_split: false,
            title: String::new(),
            _subscriptions: subs,
        };
        let editor = vm.read(cx).active_editor(cx);
        let ev = view.editor_view(&editor, window, cx);
        window.focus(&ev.focus_handle(cx));
        view
    }

    #[cfg(test)]
    pub fn view_model(&self) -> Entity<WorkspaceViewModel> {
        self.vm.clone()
    }

    fn sync_title(&mut self, window: &mut Window, cx: &App) {
        let title = self.vm.read(cx).window_title(cx);
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
    }

    /// Vue d'édition associée à un ViewModel (créée à la demande).
    fn editor_view(&mut self, vm: &Entity<EditorViewModel>, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorView> {
        self.editors
            .entry(vm.entity_id())
            .or_insert_with(|| cx.new(|cx| EditorView::new(vm.clone(), window, cx)))
            .clone()
    }

    fn preview_view(&mut self, doc: &Entity<DocumentViewModel>, window: &mut Window, cx: &mut Context<Self>) -> Entity<PreviewView> {
        let id = doc.entity_id();
        self.document_subscriptions.entry(id).or_insert_with(|| {
            cx.subscribe_in(doc, window, |this, doc, ev: &DocumentEvent, window, cx| {
                if let DocumentEvent::JumpToLine(line) = ev {
                    let editor = doc.read(cx).editor.clone();
                    editor.update(cx, |e, cx| e.go_to_line(line - 1, cx));
                    let view = this.editor_view(&editor, window, cx);
                    window.focus(&view.focus_handle(cx));
                }
            })
        });
        self.previews.entry(id).or_insert_with(|| cx.new(|cx| PreviewView::new(doc.clone(), cx))).clone()
    }

    /// Libère les vues des documents fermés.
    fn forget_closed_documents(&mut self, cx: &App) {
        let ws = self.vm.read(cx);
        let mut alive: HashSet<EntityId> = HashSet::new();
        for d in &ws.documents {
            alive.insert(d.entity_id());
            alive.insert(d.read(cx).editor.entity_id());
        }
        alive.insert(ws.find.query.entity_id());
        alive.insert(ws.find.replacement.entity_id());
        if let Some((_, p)) = &ws.path_prompt {
            alive.insert(p.entity_id());
        }
        self.editors.retain(|id, _| alive.contains(id));
        self.previews.retain(|id, _| alive.contains(id));
        self.document_subscriptions.retain(|id, _| alive.contains(id));
    }

    // ------------------------------------------------------------ demandes du ViewModel

    fn on_workspace_event(&mut self, ev: &WorkspaceEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.clone() {
            WorkspaceEvent::Focus(editor) => {
                let view = self.editor_view(&editor, window, cx);
                window.focus(&view.focus_handle(cx));
            }
            WorkspaceEvent::RequestPath { purpose, directory, suggested } => {
                self.pick_path(purpose, directory, suggested, window, cx)
            }
            WorkspaceEvent::ConfirmClose { document, name } => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &format!("Fermer « {name} » sans enregistrer ?"),
                    Some("Les modifications seront perdues."),
                    &["Fermer sans enregistrer", "Annuler"],
                    cx,
                );
                let vm = self.vm.clone();
                cx.spawn(async move |_, cx| {
                    if let Ok(0) = answer.await {
                        vm.update(cx, |vm, cx| vm.close_document(&document, true, cx)).ok();
                    }
                })
                .detach();
            }
            WorkspaceEvent::ConfirmQuit { names } => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Des modifications ne sont pas enregistrées",
                    Some(&names.join(", ")),
                    &["Quitter sans enregistrer", "Annuler"],
                    cx,
                );
                cx.spawn(async move |_, cx| {
                    if let Ok(0) = answer.await {
                        cx.update(|cx| cx.quit()).ok();
                    }
                })
                .detach();
            }
            WorkspaceEvent::ConfirmSwitch { names } => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    "Changer d'interface sans enregistrer ?",
                    Some(&format!("Les modifications seront perdues : {}", names.join(", "))),
                    &["Changer sans enregistrer", "Annuler"],
                    cx,
                );
                let vm = self.vm.clone();
                cx.spawn(async move |_, cx| {
                    if let Ok(0) = answer.await {
                        vm.update(cx, |vm, cx| vm.launch_classic(cx)).ok();
                    }
                })
                .detach();
            }
            WorkspaceEvent::ConfirmOverwrite { document, path } => {
                let answer = window.prompt(
                    PromptLevel::Warning,
                    &format!("« {} » a changé sur le disque", path.display()),
                    Some("Un autre programme l'a modifié. L'enregistrer maintenant remplace ces modifications."),
                    &["Remplacer", "Annuler"],
                    cx,
                );
                let vm = self.vm.clone();
                cx.spawn(async move |_, cx| {
                    if let Ok(0) = answer.await {
                        vm.update(cx, |vm, cx| vm.overwrite(&document, path, cx)).ok();
                    }
                })
                .detach();
            }
            WorkspaceEvent::Quit => cx.quit(),
        }
    }

    /// Sélecteur natif ; s'il est indisponible (pas de portail XDG…), saisie dans l'application.
    fn pick_path(&mut self, purpose: PathPurpose, directory: std::path::PathBuf, suggested: String, window: &mut Window, cx: &mut Context<Self>) {
        let vm = self.vm.clone();
        let fallback = directory.join(&suggested).to_string_lossy().to_string();
        if purpose.existing() {
            let answer = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: purpose == PathPurpose::Open,
                prompt: None,
            });
            cx.spawn_in(window, async move |_, cx| {
                let result = answer.await;
                vm.update(cx, |vm, cx| match result {
                    Ok(Ok(Some(paths))) => {
                        for p in paths {
                            vm.complete_path(purpose, p, false, cx);
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => vm.show_path_prompt(purpose, fallback, cx),
                })
                .ok();
            })
            .detach();
        } else {
            let answer = cx.prompt_for_new_path(&directory, Some(&suggested));
            cx.spawn_in(window, async move |_, cx| {
                let result = answer.await;
                vm.update(cx, |vm, cx| match result {
                    // le sélecteur natif a déjà confirmé un éventuel remplacement
                    Ok(Ok(Some(p))) => vm.complete_path(purpose, p, true, cx),
                    Ok(Ok(None)) => {}
                    _ => vm.show_path_prompt(purpose, fallback, cx),
                })
                .ok();
            })
            .detach();
        }
    }

    // ------------------------------------------------------------ séparateur redimensionnable

    fn on_split_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging_split = true;
        cx.notify();
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging_split {
            return;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.dragging_split = false;
            cx.notify();
            return;
        }
        let split = f32::from(ev.position.x) / f32::from(window.viewport_size().width);
        self.vm.update(cx, |vm, cx| vm.set_split(split, cx));
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging_split {
            self.dragging_split = false;
            cx.notify();
        }
    }

    // ------------------------------------------------------------ sous-parties

    fn with_vm(&self, f: impl Fn(&mut WorkspaceViewModel, &mut Context<WorkspaceViewModel>) + 'static, cx: &mut Context<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let vm = self.vm.clone();
        let _ = cx;
        move |_, _, cx: &mut App| vm.update(cx, |vm, cx| f(vm, cx))
    }

    fn with_document(&self, f: impl Fn(&mut DocumentViewModel, &mut Context<DocumentViewModel>) + 'static, cx: &App) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let doc = self.vm.read(cx).active_document().clone();
        move |_, _, cx: &mut App| doc.update(cx, |d, cx| f(d, cx))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = (self.vm.read(cx).active_document().read(cx).preview.zoom * 100.0).round();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .child(button("tb-new", "Nouveau").on_click(self.with_vm(|vm, cx| vm.new_document(cx), cx)))
            .child(button("tb-templates", "Modèles ▾").on_click(self.with_vm(|vm, cx| vm.toggle_templates(cx), cx)))
            .child(button("tb-open", "Ouvrir").on_click(self.with_vm(|vm, cx| vm.request_path(PathPurpose::Open, cx), cx)))
            .child(button("tb-save", "Enregistrer").on_click(self.with_vm(|vm, cx| vm.save(cx), cx)))
            .child(button("tb-save-as", "Enregistrer sous").on_click(self.with_vm(|vm, cx| vm.request_path(PathPurpose::SaveAs, cx), cx)))
            .child(separator())
            .child(button("tb-svg", "Exporter SVG").on_click(self.with_vm(|vm, cx| vm.request_path(PathPurpose::ExportSvg, cx), cx)))
            .child(button("tb-import", "Importer SIMP").on_click(self.with_vm(|vm, cx| vm.request_path(PathPurpose::ImportSimp, cx), cx)))
            .child(button("tb-export", "Exporter SIMP").on_click(self.with_vm(|vm, cx| vm.request_path(PathPurpose::ExportSimp, cx), cx)))
            .child(separator())
            .child(button("tb-find", "Rechercher").on_click(self.with_vm(|vm, cx| vm.open_find(cx), cx)))
            .child(button("tb-diagnostics", "Diagnostics").on_click(self.with_vm(|vm, cx| vm.toggle_diagnostics(cx), cx)))
            .child(div().flex_1())
            .child(button("tb-zoom-out", "−").on_click(self.with_document(|d, cx| d.zoom_by(1.0 / 1.25, cx), cx)))
            .child(button("tb-zoom", format!("{zoom} %")).on_click(self.with_document(|d, cx| d.zoom_reset(cx), cx)))
            .child(button("tb-zoom-in", "+").on_click(self.with_document(|d, cx| d.zoom_by(1.25, cx), cx)))
            .child(button("tb-fit", "Ajuster").on_click(self.with_document(|d, cx| d.zoom_fit(cx), cx)))
            .child(separator())
            .child(
                button("tb-preferences", "⚙")
                    .tooltip(tooltip("Préférences (Ctrl+,)"))
                    .on_click(self.with_vm(|vm, cx| vm.toggle_preferences(cx), cx)),
            )
    }

    /// Barre des préférences : chaque choix s'applique et s'enregistre aussitôt.
    fn render_preferences(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.vm.read(cx).preferences.clone();
        let label = |t: &'static str| div().text_size(px(13.)).text_color(theme::text()).pl_2().child(t);
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(theme::secondary_bar())
            .border_b_1()
            .border_color(theme::border())
            .child(label("Interface :"))
            // cette fenêtre est la nouvelle interface : le choix courant est toujours celui-ci
            .child(choice("pref-interface-editor", "Nouvelle", true).on_click(self.with_vm(|vm, cx| vm.stay_on_editor(cx), cx)))
            .child(choice("pref-interface-classic", "Classique", false).on_click(self.with_vm(|vm, cx| vm.switch_to_classic(cx), cx)))
            .child(separator())
            .child(label("Thème :"))
            .children(ThemeChoice::ALL.iter().map(|&t| {
                choice(format!("pref-theme-{}", t.label()), t.label(), p.theme == t)
                    .on_click(self.with_vm(move |vm, cx| vm.set_theme(t, cx), cx))
            }))
            .child(separator())
            .child(label("Diagramme en thème sombre :"))
            .child(choice("pref-sheet", "Feuille claire", !p.adapt_diagram).on_click(self.with_vm(|vm, cx| vm.set_adapt_diagram(false, cx), cx)))
            .child(choice("pref-adapt", "Couleurs adaptées", p.adapt_diagram).on_click(self.with_vm(|vm, cx| vm.set_adapt_diagram(true, cx), cx)))
            .child(separator())
            .child(label("Autocomplétion :"))
            .child(choice("pref-ac-on", "Pendant la frappe", p.autocomplete).on_click(self.with_vm(|vm, cx| vm.set_autocomplete(true, cx), cx)))
            .child(choice("pref-ac-off", "Ctrl+Espace seulement", !p.autocomplete).on_click(self.with_vm(|vm, cx| vm.set_autocomplete(false, cx), cx)))
            .child(separator())
            .child(label("Taille du texte :"))
            .child(button("pref-size-minus", "−").on_click(self.with_vm(|vm, cx| vm.change_code_size(-1, cx), cx)))
            .child(div().text_size(px(13.)).text_color(theme::text()).child(p.code_size.to_string()))
            .child(button("pref-size-plus", "+").on_click(self.with_vm(|vm, cx| vm.change_code_size(1, cx), cx)))
            .child(div().flex_1())
            .child(button("pref-close", "×").tooltip(tooltip("Fermer les préférences")).on_click(self.with_vm(|vm, cx| vm.toggle_preferences(cx), cx)))
    }

    fn render_templates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(theme::secondary_bar())
            .border_b_1()
            .border_color(theme::border())
            .child(div().pr_2().child(hint("Nouveau diagramme :")))
            .children(TEMPLATES.iter().enumerate().map(|(i, t)| {
                button(format!("template-{i}"), t.name)
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::pressed())
                    .on_click(self.with_vm(move |vm, cx| vm.new_from_template(i, cx), cx))
            }))
    }

    fn render_path_prompt(&mut self, purpose: PathPurpose, input: Entity<EditorViewModel>, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.editor_view(&input, window, cx);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(theme::warning_bar())
            .border_b_1()
            .border_color(theme::warning_border())
            .child(div().text_size(px(13.)).child(purpose.label()))
            .child(field(view, None))
            .child(hint("Entrée pour valider, Échap pour annuler"))
            .child(button("path-cancel", "Annuler").on_click(self.with_vm(|vm, cx| vm.cancel_path_prompt(cx), cx)))
    }

    fn render_find(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (query, replacement) = {
            let ws = self.vm.read(cx);
            (ws.find.query.clone(), ws.find.replacement.clone())
        };
        let count = self.vm.read(cx).search_count(cx);
        let query_view = self.editor_view(&query, window, cx);
        let replacement_view = self.editor_view(&replacement, window, cx);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(theme::panel())
            .border_b_1()
            .border_color(theme::border())
            .child(field(query_view, Some(220.)))
            .child(button("find-prev", "↑").on_click(self.with_vm(|vm, cx| vm.find_step(true, cx), cx)))
            .child(button("find-next", "↓").on_click(self.with_vm(|vm, cx| vm.find_step(false, cx), cx)))
            .child(hint(format!("{count} résultat(s)")))
            .child(field(replacement_view, Some(220.)))
            .child(button("replace-next", "Remplacer").on_click(self.with_vm(|vm, cx| vm.replace_next(cx), cx)))
            .child(button("replace-all", "Tout remplacer").on_click(self.with_vm(|vm, cx| vm.replace_all(cx), cx)))
            .child(div().flex_1())
            .child(button("find-close", "×").on_click(self.with_vm(|vm, cx| vm.close_find(cx), cx)))
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.vm.read(cx);
        let tabs: Vec<_> = ws
            .documents
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let d = doc.read(cx);
                let dirty = d.editor.read(cx).buffer.is_dirty();
                (i, doc.clone(), d.display_name(), dirty, i == ws.active)
            })
            .collect();
        div()
            .flex()
            .flex_row()
            .bg(theme::tab_bar())
            .border_b_1()
            .border_color(theme::border())
            .children(tabs.into_iter().map(|(i, doc, name, dirty, active)| {
                let close_doc = doc.clone();
                div()
                    .id(gpui::ElementId::Name(format!("tab-{}", doc.entity_id()).into()))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .text_size(px(13.))
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(theme::border())
                    .when(active, |s| s.bg(theme::surface()).text_color(theme::text()))
                    .when(!active, |s| s.text_color(theme::muted()).hover(|s| s.bg(theme::current_line())))
                    .on_click(self.with_vm(move |vm, cx| vm.activate(i, cx), cx))
                    .child(format!("{}{name}", if dirty { "● " } else { "" }))
                    .child(
                        div()
                            .id(gpui::ElementId::Name(format!("close-{}", doc.entity_id()).into()))
                            .px_1()
                            .rounded_sm()
                            .text_color(theme::faint())
                            .hover(|s| s.bg(theme::border()).text_color(theme::text()))
                            .on_click({
                                let vm = self.vm.clone();
                                move |_, _, cx: &mut App| {
                                    cx.stop_propagation();
                                    vm.update(cx, |vm, cx| vm.close_document(&close_doc, false, cx));
                                }
                            })
                            .child("×"),
                    )
            }))
            .child(button("tab-new", "+").on_click(self.with_vm(|vm, cx| vm.new_document(cx), cx)))
    }

    /// Liste des diagnostics, virtualisée (seules les lignes visibles sont construites).
    fn render_diagnostics(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = self.vm.read(cx).active_document().clone();
        let count = doc.read(cx).output().map(|o| o.diagnostics.len()).unwrap_or(0);
        let panel = div().h(px(120.)).flex_none().bg(theme::diagnostics_bg()).border_t_1().border_color(theme::border()).text_size(px(12.5));
        if count == 0 {
            return panel.child(div().px_3().py_2().text_color(theme::muted()).child("Aucun diagnostic.")).into_any_element();
        }
        panel
            .child(
                uniform_list("diagnostics", count, move |range, _, cx: &mut App| {
                    let d = doc.read(cx);
                    let Some(out) = d.output() else { return Vec::new() };
                    range
                        .filter_map(|i| out.diagnostics.get(i).map(|g| (i, g.clone())))
                        .map(|(i, g)| {
                            let editor = d.editor.clone();
                            let line = g.line;
                            div()
                                .id(i)
                                .flex()
                                .flex_row()
                                .gap_2()
                                .px_3()
                                .py(px(3.))
                                .cursor_pointer()
                                .hover(|s| s.bg(theme::row_hover()))
                                .on_click(move |_, _, cx: &mut App| {
                                    if line > 0 {
                                        editor.update(cx, |e, cx| e.go_to_line(line - 1, cx));
                                    }
                                })
                                .child(div().text_color(theme::severity(g.severity)).child(theme::severity_icon(g.severity)))
                                .child(div().w(px(70.)).text_color(theme::muted()).child(format!("ligne {line}")))
                                .child(div().text_color(theme::text()).child(g.message))
                        })
                        .collect()
                })
                .size_full(),
            )
            .into_any_element()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.vm.read(cx);
        let doc_entity = ws.active_document().clone();
        let doc = doc_entity.read(cx);
        let (line, col) = doc.editor.read(cx).cursor_line_col();
        let (errors, warnings) = doc.diagnostic_counts();
        let (kind, blocks) = doc.output().map(|o| (o.kind.clone(), o.blocks)).unwrap_or_default();
        let state = if !doc.has_renderer() {
            "moteur absent"
        } else if doc.render_error.is_some() && !doc.render_pending {
            "erreur de rendu"
        } else if doc.is_up_to_date(cx) {
            "à jour"
        } else {
            "rendu…"
        };
        let path = doc.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "non enregistré".into());
        let block = doc.block;
        let status = ws.status.clone();
        let count_color = |n: usize, sev| if n > 0 { theme::severity(sev) } else { theme::muted() };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py(px(3.))
            .bg(theme::panel())
            .border_t_1()
            .border_color(theme::border())
            .text_size(px(12.))
            .text_color(theme::status_text())
            .child(format!("Ln {line}, Col {col}"))
            .child(format!("Type : {}", if kind.is_empty() { "—" } else { &kind }))
            .when(blocks > 1, |s| {
                s.child(button("block-prev", "◀").on_click(self.with_document(|d, cx| d.change_block(-1, cx), cx)))
                    .child(format!("bloc {}/{blocks}", block + 1))
                    .child(button("block-next", "▶").on_click(self.with_document(|d, cx| d.change_block(1, cx), cx)))
            })
            .child(div().text_color(count_color(errors, crate::model::diagram::Severity::Error)).child(format!("✖ {errors}")))
            .child(div().text_color(count_color(warnings, crate::model::diagram::Severity::Warning)).child(format!("▲ {warnings}")))
            .child(state)
            .child(
                div()
                    .id("status-message")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when_some(status, |s, st| {
                        // un message trop long pour la barre reste lisible en entier au survol
                        s.when(st.is_error, |s| s.text_color(theme::severity(crate::model::diagram::Severity::Error)))
                            .tooltip(tooltip(st.message.clone()))
                            .child(st.message)
                    }),
            )
            .child(div().max_w(px(420.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(SharedString::from(path)))
    }
}

/// Associe les actions de l'espace de travail aux commandes des ViewModels.
macro_rules! bind_workspace {
    ($el:expr, $cx:expr, { $( $action:ty => |$this:ident, $c:ident| $body:expr ),* $(,)? }) => {
        $el $(
            .on_action($cx.listener(|$this: &mut WorkspaceView, _: &$action, _: &mut Window, $c: &mut Context<WorkspaceView>| { $body; }))
        )*
    };
}

impl WorkspaceView {
    fn vm_do(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut WorkspaceViewModel, &mut Context<WorkspaceViewModel>)) {
        self.vm.update(cx, f);
    }

    fn doc_do(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut DocumentViewModel, &mut Context<DocumentViewModel>)) {
        let doc = self.vm.read(cx).active_document().clone();
        doc.update(cx, f);
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (doc, split, find_open, templates_open, diagnostics_open, prompt, preferences_open) = {
            let ws = self.vm.read(cx);
            // palette de ce rendu (les vues enfants sont dessinées après celle-ci)
            let system_dark = matches!(window.appearance(), gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark);
            theme::set_appearance(ws.preferences.theme.is_dark(system_dark), ws.preferences.adapt_diagram);
            (
                ws.active_document().clone(),
                ws.split,
                ws.find.open,
                ws.templates_open,
                ws.diagnostics_open,
                ws.path_prompt.clone(),
                ws.preferences_open,
            )
        };
        self.sync_title(window, cx);
        let editor_vm = doc.read(cx).editor.clone();
        let editor = self.editor_view(&editor_vm, window, cx);
        let preview = self.preview_view(&doc, window, cx);
        let prompt_bar = prompt.map(|(p, input)| self.render_path_prompt(p, input, window, cx).into_any_element());
        let find_bar = find_open.then(|| self.render_find(window, cx).into_any_element());

        let root = div()
            .id("workspace")
            // pas de `track_focus` ici : un clic dans une barre d'outils laisse le focus à l'éditeur
            .key_context("Workspace")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::surface())
            .font_family(theme::UI_FONT)
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up));
        let root = bind_workspace!(root, cx, {
            NewFile => |this, cx| this.vm_do(cx, |vm, cx| vm.new_document(cx)),
            OpenFile => |this, cx| this.vm_do(cx, |vm, cx| vm.request_path(PathPurpose::Open, cx)),
            Save => |this, cx| this.vm_do(cx, |vm, cx| vm.save(cx)),
            SaveAs => |this, cx| this.vm_do(cx, |vm, cx| vm.request_path(PathPurpose::SaveAs, cx)),
            CloseTab => |this, cx| this.vm_do(cx, |vm, cx| vm.close_active(cx)),
            NextTab => |this, cx| this.vm_do(cx, |vm, cx| vm.cycle_tab(1, cx)),
            PrevTab => |this, cx| this.vm_do(cx, |vm, cx| vm.cycle_tab(-1, cx)),
            ExportSvg => |this, cx| this.vm_do(cx, |vm, cx| vm.request_path(PathPurpose::ExportSvg, cx)),
            ImportSimp => |this, cx| this.vm_do(cx, |vm, cx| vm.request_path(PathPurpose::ImportSimp, cx)),
            ExportSimp => |this, cx| this.vm_do(cx, |vm, cx| vm.request_path(PathPurpose::ExportSimp, cx)),
            Find => |this, cx| this.vm_do(cx, |vm, cx| vm.open_find(cx)),
            FindNext => |this, cx| this.vm_do(cx, |vm, cx| vm.find_step(false, cx)),
            FindPrev => |this, cx| this.vm_do(cx, |vm, cx| vm.find_step(true, cx)),
            ReplaceAll => |this, cx| this.vm_do(cx, |vm, cx| vm.replace_all(cx)),
            ToggleDiagnostics => |this, cx| this.vm_do(cx, |vm, cx| vm.toggle_diagnostics(cx)),
            ToggleTemplates => |this, cx| this.vm_do(cx, |vm, cx| vm.toggle_templates(cx)),
            TogglePreferences => |this, cx| this.vm_do(cx, |vm, cx| vm.toggle_preferences(cx)),
            Quit => |this, cx| this.vm_do(cx, |vm, cx| vm.request_quit(cx)),
            ZoomIn => |this, cx| this.doc_do(cx, |d, cx| d.zoom_by(1.25, cx)),
            ZoomOut => |this, cx| this.doc_do(cx, |d, cx| d.zoom_by(1.0 / 1.25, cx)),
            ZoomReset => |this, cx| this.doc_do(cx, |d, cx| d.zoom_reset(cx)),
            ZoomFit => |this, cx| this.doc_do(cx, |d, cx| d.zoom_fit(cx)),
            NextBlock => |this, cx| this.doc_do(cx, |d, cx| d.change_block(1, cx)),
            PrevBlock => |this, cx| this.doc_do(cx, |d, cx| d.change_block(-1, cx)),
        });
        root.child(self.render_toolbar(cx))
            .when(preferences_open, |s| s.child(self.render_preferences(cx)))
            .when(templates_open, |s| s.child(self.render_templates(cx)))
            .children(prompt_bar)
            .children(find_bar)
            .child(self.render_tabs(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .h_full()
                            .w(gpui::relative(split))
                            .overflow_hidden()
                            .child(div().flex_1().min_h_0().pt_1().child(editor))
                            .when(diagnostics_open, |s| s.child(self.render_diagnostics(cx))),
                    )
                    .child(
                        div()
                            .id("splitter")
                            .w(px(5.))
                            .h_full()
                            .flex_none()
                            .bg(if self.dragging_split { theme::accent() } else { theme::border() })
                            .hover(|s| s.bg(theme::splitter_hover()))
                            .cursor_col_resize()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_split_down)),
                    )
                    .child(preview),
            )
            .child(self.render_status(cx))
    }
}
