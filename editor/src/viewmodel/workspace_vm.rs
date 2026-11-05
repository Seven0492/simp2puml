//! ViewModel de l'espace de travail : onglets, recherche, saisie de chemin,
//! messages d'état et commandes de fichiers.
//!
//! Les boîtes de dialogue et le focus relèvent de la vue : le ViewModel
//! émet une demande (`WorkspaceEvent`) et la vue y répond, puis rappelle une
//! commande (`complete_path`, `close_document`…).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription};

use crate::model::templates::{self, TEMPLATES};
use crate::services::file_service;
use crate::services::render_service::RenderService;
use crate::viewmodel::document_vm::DocumentViewModel;
use crate::viewmodel::editor_vm::{EditorEvent, EditorViewModel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathPurpose {
    Open,
    SaveAs,
    ExportSvg,
    ImportSimp,
    ExportSimp,
}

impl PathPurpose {
    pub fn label(self) -> &'static str {
        match self {
            PathPurpose::Open => "Ouvrir le fichier :",
            PathPurpose::SaveAs => "Enregistrer sous :",
            PathPurpose::ExportSvg => "Exporter le SVG vers (nouveau fichier) :",
            PathPurpose::ImportSimp => "Importer le fichier SIMP :",
            PathPurpose::ExportSimp => "Exporter en SIMP vers (nouveau fichier, expérimental) :",
        }
    }

    /// Le chemin désigne-t-il un fichier existant (ouverture) ?
    pub fn existing(self) -> bool {
        matches!(self, PathPurpose::Open | PathPurpose::ImportSimp)
    }
}

#[derive(Clone)]
pub enum WorkspaceEvent {
    /// Demander un chemin à l'utilisateur (sélecteur natif).
    RequestPath { purpose: PathPurpose, directory: PathBuf, suggested: String },
    /// Confirmer la fermeture d'un document modifié.
    ConfirmClose { document: Entity<DocumentViewModel>, name: String },
    /// Confirmer la sortie avec des documents modifiés.
    ConfirmQuit { names: Vec<String> },
    Quit,
    /// Donner le focus à un champ (éditeur, recherche, chemin).
    Focus(Entity<EditorViewModel>),
}

#[derive(Clone, Debug)]
pub struct Status {
    pub message: String,
    pub is_error: bool,
}

pub struct FindState {
    pub open: bool,
    pub query: Entity<EditorViewModel>,
    pub replacement: Entity<EditorViewModel>,
}

pub struct WorkspaceViewModel {
    pub documents: Vec<Entity<DocumentViewModel>>,
    pub active: usize,
    pub find: FindState,
    pub path_prompt: Option<(PathPurpose, Entity<EditorViewModel>)>,
    pub templates_open: bool,
    pub diagnostics_open: bool,
    /// Part de la largeur occupée par l'éditeur (0..1).
    pub split: f32,
    pub status: Option<Status>,
    service: Option<Arc<RenderService>>,
    untitled_count: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<WorkspaceEvent> for WorkspaceViewModel {}

impl WorkspaceViewModel {
    pub fn new(paths: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|_| EditorViewModel::new("", true, "Rechercher…"));
        let replacement = cx.new(|_| EditorViewModel::new("", true, "Remplacer par…"));
        let subs = vec![
            cx.subscribe(&query, |this, _, ev: &EditorEvent, cx| match ev {
                EditorEvent::Changed => this.refresh_search(cx),
                EditorEvent::Submit => this.find_step(false, cx),
                EditorEvent::Cancel => this.close_find(cx),
                EditorEvent::FocusNext => cx.emit(WorkspaceEvent::Focus(this.find.replacement.clone())),
                _ => {}
            }),
            cx.subscribe(&replacement, |this, _, ev: &EditorEvent, cx| match ev {
                EditorEvent::Submit => this.replace_all(cx),
                EditorEvent::Cancel => this.close_find(cx),
                EditorEvent::FocusPrev => cx.emit(WorkspaceEvent::Focus(this.find.query.clone())),
                _ => {}
            }),
        ];
        let service = RenderService::locate().map(Arc::new);
        let mut ws = Self {
            documents: Vec::new(),
            active: 0,
            find: FindState { open: false, query, replacement },
            path_prompt: None,
            templates_open: false,
            diagnostics_open: true,
            split: 0.45,
            status: None,
            service,
            untitled_count: 0,
            _subscriptions: subs,
        };
        if ws.service.is_none() {
            ws.set_status("Moteur « pumlrender » introuvable : définissez PUMLRENDER ou construisez renderer/", true, cx);
        }
        for p in paths {
            ws.open_path(p, cx);
        }
        if ws.documents.is_empty() {
            ws.add_document(None, "sans titre.puml".into(), TEMPLATES[0].text, cx);
        }
        ws
    }

    // ------------------------------------------------------------ accès

    pub fn active_document(&self) -> &Entity<DocumentViewModel> {
        &self.documents[self.active]
    }

    pub fn active_editor(&self, cx: &App) -> Entity<EditorViewModel> {
        self.active_document().read(cx).editor.clone()
    }

    pub fn window_title(&self, cx: &App) -> String {
        let doc = self.active_document().read(cx);
        let dirty = if doc.editor.read(cx).buffer.is_dirty() { "• " } else { "" };
        format!("{dirty}{} — Éditeur PlantUML", doc.display_name())
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        self.status = Some(Status { message: message.into(), is_error });
        cx.notify();
    }

    // ------------------------------------------------------------ onglets

    fn add_document(&mut self, path: Option<PathBuf>, title: String, text: &str, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let doc = cx.new(|cx| DocumentViewModel::new(text, path, title, service, cx));
        self.documents.push(doc);
        self.activate(self.documents.len() - 1, cx);
    }

    fn next_untitled(&mut self, stem: &str) -> String {
        self.untitled_count += 1;
        format!("{stem} {}.puml", self.untitled_count)
    }

    pub fn activate(&mut self, index: usize, cx: &mut Context<Self>) {
        let index = index.min(self.documents.len().saturating_sub(1));
        if index != self.active {
            self.status = None;
        }
        self.active = index;
        if self.find.open {
            self.refresh_search(cx);
        }
        cx.emit(WorkspaceEvent::Focus(self.active_editor(cx)));
        cx.notify();
    }

    pub fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.documents.len() as isize;
        self.activate((self.active as isize + delta).rem_euclid(n) as usize, cx);
    }

    pub fn new_document(&mut self, cx: &mut Context<Self>) {
        let title = self.next_untitled("sans titre");
        self.add_document(None, title, templates::blank(), cx);
        // curseur sur la ligne vide entre @startuml et @enduml
        self.active_editor(cx).update(cx, |ed, cx| ed.go_to_line(1, cx));
    }

    pub fn new_from_template(&mut self, index: usize, cx: &mut Context<Self>) {
        let t = &TEMPLATES[index];
        let title = self.next_untitled(&t.name.to_lowercase());
        self.add_document(None, title, t.text, cx);
        self.templates_open = false;
    }

    /// Ferme un document ; s'il est modifié, demande d'abord confirmation.
    pub fn close_document(&mut self, document: &Entity<DocumentViewModel>, force: bool, cx: &mut Context<Self>) {
        let Some(index) = self.documents.iter().position(|d| d == document) else { return };
        let doc = document.read(cx);
        if !force && doc.editor.read(cx).buffer.is_dirty() {
            let name = doc.display_name();
            cx.emit(WorkspaceEvent::ConfirmClose { document: document.clone(), name });
            return;
        }
        self.documents.remove(index);
        if self.documents.is_empty() {
            let title = self.next_untitled("sans titre");
            self.add_document(None, title, templates::blank(), cx);
        } else {
            self.activate(self.active.min(self.documents.len() - 1), cx);
        }
        cx.notify();
    }

    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        let doc = self.active_document().clone();
        self.close_document(&doc, false, cx);
    }

    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        let names: Vec<String> = self
            .documents
            .iter()
            .map(|d| d.read(cx))
            .filter(|d| d.editor.read(cx).buffer.is_dirty())
            .map(|d| d.display_name())
            .collect();
        if names.is_empty() {
            cx.emit(WorkspaceEvent::Quit);
        } else {
            cx.emit(WorkspaceEvent::ConfirmQuit { names });
        }
    }

    pub fn has_unsaved(&self, cx: &App) -> bool {
        self.documents.iter().any(|d| d.read(cx).editor.read(cx).buffer.is_dirty())
    }

    // ------------------------------------------------------------ fichiers

    pub fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(i) = self.documents.iter().position(|d| d.read(cx).path.as_deref() == Some(path.as_path())) {
            self.activate(i, cx);
            return;
        }
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("simp")) {
            self.import_simp(path, cx);
            return;
        }
        match file_service::read_text(&path) {
            Ok(text) => {
                // l'onglet vide initial, jamais modifié, est remplacé
                if self.documents.len() == 1 {
                    let d = self.documents[0].read(cx);
                    if d.path.is_none() && !d.editor.read(cx).buffer.is_dirty() {
                        self.documents.clear();
                    }
                }
                let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.add_document(Some(path.clone()), title, &text, cx);
                self.set_status(format!("Ouvert : {}", path.display()), false, cx);
            }
            Err(e) => self.set_status(format!("Ouverture impossible : {e}"), true, cx),
        }
    }

    /// Convertit un .simp dans un nouvel onglet non enregistré (rien n'est écrit près de la source).
    pub fn import_simp(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.set_status(format!("Import SIMP en cours : {}", path.display()), false, cx);
        cx.spawn(async move |this, cx| {
            let source = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { file_service::import_simp(&source, None) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(conv) => {
                    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    this.add_document(None, format!("{stem}.puml (importé)"), &conv.text, cx);
                    let warnings: Vec<&String> = conv.messages.iter().filter(|m| m.starts_with("Avertissement :")).collect();
                    let msg = if warnings.is_empty() {
                        "SIMP importé (enregistrez le résultat sous un nouveau nom)".to_string()
                    } else {
                        let list: Vec<&str> = warnings.iter().map(|s| s.as_str()).collect();
                        format!("SIMP importé avec {} avertissement(s) : {}", warnings.len(), list.join(" | "))
                    };
                    this.set_status(msg, false, cx);
                }
                Err(e) => this.set_status(format!("Import SIMP impossible : {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        match self.active_document().read(cx).path.clone() {
            Some(p) => self.save_to(p, true, cx),
            None => self.request_path(PathPurpose::SaveAs, cx),
        }
    }

    /// `overwrite` : le sélecteur natif a déjà confirmé le remplacement.
    fn save_to(&mut self, path: PathBuf, overwrite: bool, cx: &mut Context<Self>) {
        let doc = self.active_document().clone();
        let text = doc.read(cx).editor.read(cx).text().to_string();
        let same = doc.read(cx).path.as_deref() == Some(path.as_path());
        let result = if same || overwrite {
            file_service::save_document(&path, &text)
        } else {
            file_service::write_new(&path, &text)
        };
        match result {
            Ok(()) => {
                doc.update(cx, |d, cx| {
                    d.path = Some(path.clone());
                    d.editor.update(cx, |ed, cx| {
                        ed.buffer.mark_saved();
                        cx.notify();
                    });
                    cx.notify();
                });
                self.set_status(format!("Enregistré : {}", path.display()), false, cx);
            }
            Err(e) => self.set_status(format!("Enregistrement impossible : {e}"), true, cx),
        }
    }

    fn export_svg(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let svg = self.active_document().read(cx).output().map(|o| o.svg.clone());
        let Some(svg) = svg else {
            self.set_status("Aucun rendu disponible à exporter", true, cx);
            return;
        };
        match file_service::write_new(&path, &svg) {
            Ok(()) => self.set_status(format!("SVG exporté : {}", path.display()), false, cx),
            Err(e) => self.set_status(format!("Export SVG impossible : {e}"), true, cx),
        }
    }

    fn export_simp(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let text = self.active_editor(cx).read(cx).text().to_string();
        cx.spawn(async move |this, cx| {
            let target = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { file_service::export_simp(&text, &target) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(msgs) => this.set_status(
                    format!("SIMP exporté (expérimental, compatibilité non validée) : {} {}", path.display(), msgs.join(" | ")),
                    false,
                    cx,
                ),
                Err(e) => this.set_status(format!("Export SIMP impossible : {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn request_path(&mut self, purpose: PathPurpose, cx: &mut Context<Self>) {
        let directory = self.default_directory(cx);
        let suggested = match purpose {
            PathPurpose::SaveAs => self.suggested_name("puml", cx),
            PathPurpose::ExportSvg => self.suggested_name("svg", cx),
            PathPurpose::ExportSimp => self.suggested_name("simp", cx),
            _ => String::new(),
        };
        cx.emit(WorkspaceEvent::RequestPath { purpose, directory, suggested });
    }

    /// Chemin fourni (sélecteur natif ou saisie) : exécuter l'action demandée.
    pub fn complete_path(&mut self, purpose: PathPurpose, path: PathBuf, confirmed: bool, cx: &mut Context<Self>) {
        self.path_prompt = None;
        match purpose {
            PathPurpose::Open => self.open_path(path, cx),
            PathPurpose::ImportSimp => self.import_simp(path, cx),
            PathPurpose::SaveAs => self.save_to(path, confirmed, cx),
            PathPurpose::ExportSvg => self.export_svg(path, cx),
            PathPurpose::ExportSimp => self.export_simp(path, cx),
        }
        cx.emit(WorkspaceEvent::Focus(self.active_editor(cx)));
        cx.notify();
    }

    /// Repli quand le sélecteur natif est indisponible : saisie dans l'application.
    pub fn show_path_prompt(&mut self, purpose: PathPurpose, initial: String, cx: &mut Context<Self>) {
        let input = cx.new(|_| {
            let mut e = EditorViewModel::new(&initial, true, "chemin du fichier");
            let end = e.buffer.len();
            e.buffer.set_cursor(end);
            e
        });
        let sub = cx.subscribe(&input, move |this, input, ev: &EditorEvent, cx| match ev {
            EditorEvent::Submit => {
                let text = input.read(cx).text().trim().to_string();
                if text.is_empty() {
                    return;
                }
                this.complete_path(purpose, PathBuf::from(expand_home(&text)), false, cx);
            }
            EditorEvent::Cancel => this.cancel_path_prompt(cx),
            _ => {}
        });
        self._subscriptions.push(sub);
        cx.emit(WorkspaceEvent::Focus(input.clone()));
        self.path_prompt = Some((purpose, input));
        cx.notify();
    }

    pub fn cancel_path_prompt(&mut self, cx: &mut Context<Self>) {
        self.path_prompt = None;
        cx.emit(WorkspaceEvent::Focus(self.active_editor(cx)));
        cx.notify();
    }

    fn default_directory(&self, cx: &App) -> PathBuf {
        self.active_document()
            .read(cx)
            .path
            .as_ref()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn suggested_name(&self, ext: &str, cx: &App) -> String {
        let name = self.active_document().read(cx).display_name();
        let base = name.split(" (").next().unwrap_or(&name).to_string();
        let stem = Path::new(&base).file_stem().map(|s| s.to_string_lossy().to_string());
        format!("{}.{ext}", stem.unwrap_or_else(|| "diagramme".into()))
    }

    // ------------------------------------------------------------ recherche

    pub fn open_find(&mut self, cx: &mut Context<Self>) {
        self.find.open = true;
        let selected = self.active_editor(cx).read(cx).buffer.selected_text().to_string();
        let query = self.find.query.clone();
        query.update(cx, |q, cx| {
            if !selected.is_empty() && !selected.contains('\n') {
                q.set_text(&selected, cx);
            }
            q.select_all(cx);
        });
        self.refresh_search(cx);
        cx.emit(WorkspaceEvent::Focus(query));
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.find.open = false;
        for doc in &self.documents {
            let ed = doc.read(cx).editor.clone();
            ed.update(cx, |e, cx| e.set_search("", cx));
        }
        cx.emit(WorkspaceEvent::Focus(self.active_editor(cx)));
        cx.notify();
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let query = self.find.query.read(cx).text().to_string();
        self.active_editor(cx).update(cx, |e, cx| e.set_search(&query, cx));
        cx.notify();
    }

    pub fn search_count(&self, cx: &App) -> usize {
        let query = self.find.query.read(cx).text();
        self.active_editor(cx).read(cx).buffer.count_matches(query)
    }

    pub fn find_step(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let query = self.find.query.read(cx).text().to_string();
        if query.is_empty() {
            return;
        }
        let found = self.active_editor(cx).update(cx, |e, cx| e.find_step(&query, backwards, cx));
        if !found {
            self.set_status(format!("« {query} » introuvable"), true, cx);
        }
    }

    pub fn replace_all(&mut self, cx: &mut Context<Self>) {
        let query = self.find.query.read(cx).text().to_string();
        let with = self.find.replacement.read(cx).text().to_string();
        if query.is_empty() {
            return;
        }
        let n = self.active_editor(cx).update(cx, |e, cx| e.replace_all(&query, &with, cx));
        self.refresh_search(cx);
        self.set_status(format!("{n} remplacement(s)"), false, cx);
    }

    // ------------------------------------------------------------ panneaux

    pub fn toggle_templates(&mut self, cx: &mut Context<Self>) {
        self.templates_open = !self.templates_open;
        cx.notify();
    }

    pub fn toggle_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diagnostics_open = !self.diagnostics_open;
        cx.notify();
    }

    pub fn set_split(&mut self, split: f32, cx: &mut Context<Self>) {
        self.split = split.clamp(0.15, 0.85);
        cx.notify();
    }
}

/// Développe `~/` en début de chemin.
fn expand_home(s: &str) -> String {
    if let (Some(rest), Some(home)) = (s.strip_prefix("~/"), std::env::var_os("HOME")) {
        return Path::new(&home).join(rest).to_string_lossy().to_string();
    }
    s.to_string()
}
