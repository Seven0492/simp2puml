//! ViewModel de l'espace de travail : onglets, recherche, saisie de chemin,
//! messages d'état et commandes de fichiers.
//!
//! Les boîtes de dialogue et le focus relèvent de la vue : le ViewModel
//! émet une demande (`WorkspaceEvent`) et la vue y répond, puis rappelle une
//! commande (`complete_path`, `close_document`…).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription, WeakEntity};

use crate::model::preferences::{InterfaceChoice, MAX_CODE_SIZE, MIN_CODE_SIZE, Preferences, PreviewEngine, SecurityProfile, ThemeChoice};
use crate::model::templates::{self, TEMPLATES};
use crate::services::{file_service, interface_service, preferences_service};
use crate::services::render_service::{self, RenderService};
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

    /// Extension ajoutée à un nom de sortie saisi sans extension.
    fn default_extension(self) -> Option<&'static str> {
        match self {
            PathPurpose::SaveAs => Some("puml"),
            PathPurpose::ExportSvg => Some("svg"),
            PathPurpose::ExportSimp => Some("simp"),
            PathPurpose::Open | PathPurpose::ImportSimp => None,
        }
    }

    /// Chemin de sortie complété ; vrai si l'extension a été ajoutée.
    fn complete_extension(self, path: PathBuf) -> (PathBuf, bool) {
        match self.default_extension() {
            Some(ext) if path.extension().is_none() && path.file_name().is_some() => (path.with_extension(ext), true),
            _ => (path, false),
        }
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
    /// Changer d'interface avec des documents modifiés : confirmer.
    ConfirmSwitch { names: Vec<String> },
    /// Le fichier a changé sur le disque : confirmer son remplacement.
    ConfirmOverwrite { document: Entity<DocumentViewModel>, path: PathBuf },
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
    pub preferences_open: bool,
    pub preferences: Preferences,
    pub diagnostics_open: bool,
    /// Part de la largeur occupée par l'éditeur (0..1).
    pub split: f32,
    pub status: Option<Status>,
    /// Document visé par la demande de chemin en cours : l'onglet actif peut
    /// changer pendant que le sélecteur ou la saisie de chemin est ouvert.
    path_target: Option<WeakEntity<DocumentViewModel>>,
    service: Option<Arc<RenderService>>,
    /// Installation du moteur officiel en cours.
    pub installing: bool,
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
                EditorEvent::Submit => this.replace_next(cx),
                EditorEvent::Cancel => this.close_find(cx),
                EditorEvent::FocusPrev => cx.emit(WorkspaceEvent::Focus(this.find.query.clone())),
                _ => {}
            }),
        ];
        let (preferences, preferences_message) = preferences_service::load();
        let service = make_service(preferences.preview_engine);
        let mut ws = Self {
            documents: Vec::new(),
            active: 0,
            find: FindState { open: false, query, replacement },
            path_prompt: None,
            templates_open: false,
            preferences_open: false,
            preferences,
            diagnostics_open: true,
            split: 0.45,
            status: None,
            path_target: None,
            service,
            installing: false,
            untitled_count: 0,
            _subscriptions: subs,
        };
        if let Some(m) = preferences_message {
            ws.set_status(m, true, cx);
        }
        match (&ws.service, render_service::invalid_override()) {
            (None, _) => ws.set_status("Moteur « pumlrender » introuvable : définissez PUMLRENDER ou construisez renderer/", true, cx),
            (Some(found), Some(wrong)) => {
                let message = format!("PUMLRENDER ne désigne pas un fichier ({wrong}) : moteur utilisé : {}", found.exe.display());
                ws.set_status(message, true, cx);
            }
            _ => {}
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
        // même titre que l'interface d'origine de simp2puml : « fichier — simp2puml 0.1.0 »
        format!("{dirty}{} — simp2puml {}", doc.display_name(), env!("CARGO_PKG_VERSION"))
    }

    fn set_status(&mut self, message: impl Into<String>, is_error: bool, cx: &mut Context<Self>) {
        self.status = Some(Status { message: message.into(), is_error });
        cx.notify();
    }

    // ------------------------------------------------------------ onglets

    fn add_document(&mut self, path: Option<PathBuf>, title: String, text: &str, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let profile = self.preferences.security_profile;
        let doc = cx.new(|cx| {
            let mut doc = DocumentViewModel::new(text, path, title, service, cx);
            doc.security_profile = profile;
            doc
        });
        let (autocomplete, code_size) = (self.preferences.autocomplete, self.preferences.code_size);
        doc.read(cx).editor.clone().update(cx, |e, cx| {
            e.autocomplete = autocomplete;
            e.set_code_size(code_size as f32, cx);
        });
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
            // fermer un onglet placé avant l'onglet actif ne change pas l'onglet actif
            let active = if index < self.active { self.active - 1 } else { self.active };
            self.active = active.min(self.documents.len() - 1);
            self.activate(self.active, cx);
        }
        cx.notify();
    }

    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        let doc = self.active_document().clone();
        self.close_document(&doc, false, cx);
    }

    fn unsaved_names(&self, cx: &App) -> Vec<String> {
        self.documents
            .iter()
            .map(|d| d.read(cx))
            .filter(|d| d.editor.read(cx).buffer.is_dirty())
            .map(|d| d.display_name())
            .collect()
    }

    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        let names = self.unsaved_names(cx);
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

    /// Onglet déjà associé à ce fichier (chemins résolus : liens, `..`, relatifs).
    fn document_for(&self, path: &Path, cx: &App) -> Option<usize> {
        let key = same_file_key(path);
        self.documents.iter().position(|d| d.read(cx).path.as_deref().is_some_and(|p| same_file_key(p) == key))
    }

    pub fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(i) = self.document_for(&path, cx) {
            self.activate(i, cx);
            return;
        }
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("simp")) {
            self.import_simp(path, cx);
            return;
        }
        // empreinte relevée avant la lecture : une écriture entre les deux sera vue
        let stamp = file_service::disk_stamp(&path);
        match file_service::read_text(&path) {
            Ok(text) => {
                self.drop_pristine_tab(cx);
                let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.set_status(format!("Ouvert : {title}"), false, cx);
                self.add_document(Some(path.clone()), title, &text, cx);
                self.active_document().update(cx, |d, _| d.disk_stamp = stamp);
            }
            Err(e) => self.set_status(format!("Ouverture impossible : {e}"), true, cx),
        }
    }

    /// L'onglet de départ, jamais enregistré ni modifié, laisse sa place au
    /// premier fichier ouvert ou importé.
    fn drop_pristine_tab(&mut self, cx: &App) {
        if self.documents.len() == 1 {
            let d = self.documents[0].read(cx);
            if d.path.is_none() && !d.editor.read(cx).buffer.is_dirty() {
                self.documents.clear();
            }
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
                    this.drop_pristine_tab(cx);
                    this.add_document(None, format!("{stem}.puml (importé)"), &conv.text, cx);
                    // jamais enregistré : la fermeture demandera confirmation
                    this.active_editor(cx).update(cx, |ed, _| ed.buffer.mark_unsaved());
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
        let doc = self.active_document().clone();
        let d = doc.read(cx);
        match d.path.clone() {
            Some(path) => {
                // ne pas effacer sans le dire ce qu'un autre programme a écrit
                let current = file_service::disk_stamp(&path);
                if d.disk_stamp.is_some() && current.is_some() && current != d.disk_stamp {
                    cx.emit(WorkspaceEvent::ConfirmOverwrite { document: doc, path });
                } else {
                    self.save_to(&doc, path, true, cx);
                }
            }
            None => self.request_path(PathPurpose::SaveAs, cx),
        }
    }

    /// Réponse à `ConfirmOverwrite` : remplacer le fichier modifié sur le disque.
    pub fn overwrite(&mut self, doc: &Entity<DocumentViewModel>, path: PathBuf, cx: &mut Context<Self>) {
        if self.documents.contains(doc) {
            self.save_to(doc, path, true, cx);
        }
    }

    /// `overwrite` : le sélecteur natif a déjà confirmé le remplacement.
    fn save_to(&mut self, doc: &Entity<DocumentViewModel>, path: PathBuf, overwrite: bool, cx: &mut Context<Self>) {
        let doc = doc.clone();
        if self.document_for(&path, cx).is_some_and(|i| self.documents[i] != doc) {
            self.set_status(format!("« {} » est ouvert dans un autre onglet : fermez-le d'abord", path.display()), true, cx);
            return;
        }
        let text = doc.read(cx).editor.read(cx).text().to_string();
        let same = doc.read(cx).path.as_deref().is_some_and(|p| same_file_key(p) == same_file_key(&path));
        let result = if same || overwrite {
            file_service::save_document(&path, &text)
        } else {
            file_service::write_new(&path, &text)
        };
        match result {
            Ok(()) => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                doc.update(cx, |d, cx| {
                    d.disk_stamp = file_service::disk_stamp(&path);
                    d.path = Some(path.clone());
                    d.editor.update(cx, |ed, cx| {
                        ed.buffer.mark_saved();
                        cx.notify();
                    });
                    cx.notify();
                });
                self.set_status(format!("Enregistré : {name}"), false, cx);
            }
            Err(e) => self.set_status(format!("Enregistrement impossible : {e}"), true, cx),
        }
    }

    fn export_svg(&mut self, doc: &Entity<DocumentViewModel>, path: PathBuf, cx: &mut Context<Self>) {
        // n'exporter que le rendu du texte actuel, jamais un rendu périmé
        if !doc.read(cx).is_up_to_date(cx) || doc.read(cx).render_error.is_some() {
            self.set_status("Aperçu pas à jour (rendu en cours ou en erreur) : réessayez une fois l'aperçu à jour", true, cx);
            return;
        }
        let svg = doc.read(cx).output().map(|o| o.svg.clone());
        let Some(svg) = svg else {
            self.set_status("Aucun rendu disponible à exporter", true, cx);
            return;
        };
        match file_service::write_new(&path, &svg) {
            Ok(()) => self.set_status(format!("SVG exporté : {}", path.display()), false, cx),
            Err(e) => self.set_status(format!("Export SVG impossible : {e}"), true, cx),
        }
    }

    fn export_simp(&mut self, doc: &Entity<DocumentViewModel>, path: PathBuf, cx: &mut Context<Self>) {
        let text = doc.read(cx).editor.read(cx).text().to_string();
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
        self.path_target = Some(self.active_document().downgrade());
        cx.emit(WorkspaceEvent::RequestPath { purpose, directory, suggested });
    }

    /// Chemin fourni (sélecteur natif ou saisie) : exécuter l'action demandée.
    pub fn complete_path(&mut self, purpose: PathPurpose, path: PathBuf, confirmed: bool, cx: &mut Context<Self>) {
        self.path_prompt = None;
        let target = self
            .path_target
            .take()
            .and_then(|w| w.upgrade())
            .filter(|d| self.documents.contains(d));
        let needs_target = matches!(purpose, PathPurpose::SaveAs | PathPurpose::ExportSvg | PathPurpose::ExportSimp);
        if needs_target && target.is_none() {
            self.set_status("Le document visé a été fermé : action annulée", true, cx);
            return;
        }
        // une extension ajoutée donne un autre fichier que celui confirmé par le
        // sélecteur : il ne doit pas être remplacé sans confirmation
        let (path, completed) = purpose.complete_extension(path);
        let confirmed = confirmed && !completed;
        match purpose {
            PathPurpose::Open => self.open_path(path, cx),
            PathPurpose::ImportSimp => self.import_simp(path, cx),
            PathPurpose::SaveAs => self.save_to(&target.expect("vérifié"), path, confirmed, cx),
            PathPurpose::ExportSvg => self.export_svg(&target.expect("vérifié"), path, cx),
            PathPurpose::ExportSimp => self.export_simp(&target.expect("vérifié"), path, cx),
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
        self.path_target = None;
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
        // les occurrences sont calculées une fois par modification, pas à chaque image
        self.active_editor(cx).read(cx).highlights.len()
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

    /// Remplace l'occurrence sélectionnée, puis passe à la suivante.
    pub fn replace_next(&mut self, cx: &mut Context<Self>) {
        let with = self.find.replacement.read(cx).text().to_string();
        self.active_editor(cx).update(cx, |e, cx| {
            // la sélection n'est remplacée que si c'est bien une occurrence
            if e.highlights.contains(&e.buffer.selection) {
                e.replace_selection(&with, cx);
            }
        });
        self.find_step(false, cx);
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

    // ------------------------------------------------------------ préférences

    pub fn toggle_preferences(&mut self, cx: &mut Context<Self>) {
        self.preferences_open = !self.preferences_open;
        cx.notify();
    }

    /// Passe à l'interface classique, après confirmation si du travail serait perdu.
    pub fn switch_to_classic(&mut self, cx: &mut Context<Self>) {
        let names = self.unsaved_names(cx);
        if names.is_empty() {
            self.launch_classic(cx);
        } else {
            cx.emit(WorkspaceEvent::ConfirmSwitch { names });
        }
    }

    /// Ouvre l'interface classique sur le fichier actif, enregistre le choix et quitte.
    pub fn launch_classic(&mut self, cx: &mut Context<Self>) {
        let file = self.active_document().read(cx).path.clone();
        match interface_service::launch_classic(file.as_deref()) {
            Ok(()) => {
                self.update_preferences(|p| p.interface = InterfaceChoice::Classic, cx);
                cx.emit(WorkspaceEvent::Quit);
            }
            Err(e) => self.set_status(format!("Changement d'interface impossible : {e}"), true, cx),
        }
    }

    /// Lancement direct de `puml-editor` alors que l'interface classique était choisie.
    pub fn stay_on_editor(&mut self, cx: &mut Context<Self>) {
        self.update_preferences(|p| p.interface = InterfaceChoice::Editor, cx);
    }

    pub fn set_theme(&mut self, theme: ThemeChoice, cx: &mut Context<Self>) {
        self.update_preferences(|p| p.theme = theme, cx);
    }

    pub fn set_adapt_diagram(&mut self, adapt: bool, cx: &mut Context<Self>) {
        self.update_preferences(|p| p.adapt_diagram = adapt, cx);
    }

    pub fn set_autocomplete(&mut self, on: bool, cx: &mut Context<Self>) {
        self.update_preferences(|p| p.autocomplete = on, cx);
        for doc in &self.documents {
            doc.read(cx).editor.clone().update(cx, |e, _| {
                e.autocomplete = on;
                if !on {
                    e.completions = None;
                }
            });
        }
    }

    fn reconfigure_documents(&mut self, cx: &mut Context<Self>) {
        let (service, profile) = (self.service.clone(), self.preferences.security_profile);
        for doc in self.documents.clone() {
            doc.update(cx, |d, cx| d.reconfigure(service.clone(), profile, cx));
        }
    }

    pub fn set_preview_engine(&mut self, engine: PreviewEngine, cx: &mut Context<Self>) {
        if engine == self.preferences.preview_engine {
            return;
        }
        self.update_preferences(|p| p.preview_engine = engine, cx);
        self.service = make_service(engine);
        self.reconfigure_documents(cx);
    }

    pub fn set_security_profile(&mut self, profile: SecurityProfile, cx: &mut Context<Self>) {
        if profile == self.preferences.security_profile {
            return;
        }
        self.update_preferences(|p| p.security_profile = profile, cx);
        self.reconfigure_documents(cx);
    }

    /// Le moteur choisi manque : la vue propose de l'installer.
    pub fn official_engine_missing(&self, cx: &App) -> bool {
        self.preferences.preview_engine == PreviewEngine::Official
            && self.active_document().read(cx).render_error.as_deref().is_some_and(|e| e.contains("non installé"))
    }

    /// Télécharge et vérifie le moteur officiel, en arrière-plan, puis relance les rendus.
    pub fn install_official_engine(&mut self, cx: &mut Context<Self>) {
        if self.installing {
            return;
        }
        self.installing = true;
        self.set_status("Installation de PlantUML en cours…", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { render_service::install_official() }).await;
            this.update(cx, |this, cx| {
                this.installing = false;
                match result {
                    Ok(()) => {
                        this.set_status("PlantUML installé", false, cx);
                        for doc in this.documents.clone() {
                            doc.update(cx, |d, cx| d.rerender(cx));
                        }
                    }
                    Err(e) => this.set_status(format!("Installation de PlantUML impossible : {e}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    /// Taille du texte de l'éditeur, pour tous les onglets.
    pub fn change_code_size(&mut self, delta: i32, cx: &mut Context<Self>) {
        let size = self.preferences.code_size.saturating_add_signed(delta).clamp(MIN_CODE_SIZE, MAX_CODE_SIZE);
        self.set_code_size(size, cx);
    }

    pub fn reset_code_size(&mut self, cx: &mut Context<Self>) {
        self.set_code_size(Preferences::default().code_size, cx);
    }

    fn set_code_size(&mut self, size: u32, cx: &mut Context<Self>) {
        self.update_preferences(|p| p.code_size = size, cx);
        for doc in &self.documents {
            doc.read(cx).editor.clone().update(cx, |e, cx| e.set_code_size(size as f32, cx));
        }
    }

    /// Applique et enregistre aussitôt (pas de bouton « Appliquer »).
    fn update_preferences(&mut self, f: impl FnOnce(&mut Preferences), cx: &mut Context<Self>) {
        let mut p = self.preferences.clone();
        f(&mut p);
        if p == self.preferences {
            return;
        }
        self.preferences = p;
        if let Err(e) = preferences_service::save(&self.preferences) {
            self.set_status(format!("Préférences appliquées mais non enregistrées : {e}"), true, cx);
        }
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

fn make_service(engine: PreviewEngine) -> Option<Arc<RenderService>> {
    match engine {
        PreviewEngine::Native => RenderService::locate().map(Arc::new),
        PreviewEngine::Official => Some(Arc::new(RenderService::official())),
    }
}

/// Développe `~/` en début de chemin.
fn expand_home(s: &str) -> String {
    if let (Some(rest), Some(home)) = (s.strip_prefix("~/"), std::env::var_os("HOME")) {
        return Path::new(&home).join(rest).to_string_lossy().to_string();
    }
    s.to_string()
}

/// Clé de comparaison de deux chemins de fichier : chemin résolu s'il existe,
/// sinon dossier parent résolu + nom (fichier pas encore créé).
fn same_file_key(path: &Path) -> PathBuf {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p;
    }
    match (path.parent(), path.file_name()) {
        (Some(dir), Some(name)) => {
            let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
            std::fs::canonicalize(dir).map(|d| d.join(name)).unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le titre affiche cette version : elle suit celle du paquet simp2puml.
    #[test]
    fn version_matches_the_simp2puml_package() {
        let init = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/simp2puml/__init__.py")).unwrap();
        assert!(init.contains(&format!("__version__ = \"{}\"", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn missing_output_extension_is_completed_and_flagged() {
        let out = |purpose: PathPurpose, path: &str| purpose.complete_extension(PathBuf::from(path));
        assert_eq!(out(PathPurpose::SaveAs, "/d/a"), (PathBuf::from("/d/a.puml"), true));
        assert_eq!(out(PathPurpose::ExportSvg, "a"), (PathBuf::from("a.svg"), true));
        assert_eq!(out(PathPurpose::ExportSimp, "a"), (PathBuf::from("a.simp"), true));
        // une extension choisie n'est jamais changée
        assert_eq!(out(PathPurpose::SaveAs, "a.txt"), (PathBuf::from("a.txt"), false));
        assert_eq!(out(PathPurpose::ExportSimp, "a.txt"), (PathBuf::from("a.txt"), false));
        assert_eq!(out(PathPurpose::Open, "a"), (PathBuf::from("a"), false));
    }
}
