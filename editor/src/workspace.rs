//! Espace de travail : onglets, barre d'outils, éditeur, aperçu SVG natif,
//! diagnostics, recherche/remplacement, fichiers et conversion SIMP.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, Focusable, Image, ImageFormat, RenderImage,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathPromptOptions, PromptLevel,
    ScrollWheelEvent, SharedString, Subscription, Task, Window, actions, div, img, prelude::*, px,
    rgb, rgba,
};

use smallvec::SmallVec;

use crate::editor::{EditorEvent, TextEditor};
use crate::files;
use crate::renderer::{self, RenderOutput, Severity};
use crate::templates::{self, TEMPLATES};

actions!(
    workspace,
    [
        NewFile,
        OpenFile,
        Save,
        SaveAs,
        CloseTab,
        NextTab,
        PrevTab,
        ExportSvg,
        ImportSimp,
        ExportSimp,
        Find,
        FindNext,
        FindPrev,
        ReplaceAll,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        ZoomFit,
        ToggleDiagnostics,
        ToggleTemplates,
        NextBlock,
        PrevBlock,
        Quit,
    ]
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathPurpose {
    Open,
    SaveAs,
    ExportSvg,
    ImportSimp,
    ExportSimp,
}

impl PathPurpose {
    fn label(self) -> &'static str {
        match self {
            PathPurpose::Open => "Ouvrir le fichier :",
            PathPurpose::SaveAs => "Enregistrer sous :",
            PathPurpose::ExportSvg => "Exporter le SVG vers (nouveau fichier) :",
            PathPurpose::ImportSimp => "Importer le fichier SIMP :",
            PathPurpose::ExportSimp => "Exporter en SIMP vers (nouveau fichier, expérimental) :",
        }
    }
}

struct Document {
    id: usize,
    editor: Entity<TextEditor>,
    path: Option<PathBuf>,
    title: String,
    output: Option<RenderOutput>,
    render_error: Option<String>,
    image: Option<(u32, Arc<RenderImage>)>,
    rendered_version: Option<u64>,
    rendering: bool,
    block: usize,
    zoom: f32,
    _subscription: Subscription,
}

impl Document {
    fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.title.clone())
    }
}

pub struct Workspace {
    docs: Vec<Document>,
    active: usize,
    next_id: usize,
    renderer: Option<PathBuf>,
    focus_handle: FocusHandle,
    find_open: bool,
    find_input: Entity<TextEditor>,
    replace_input: Entity<TextEditor>,
    path_prompt: Option<(PathPurpose, Entity<TextEditor>)>,
    templates_open: bool,
    show_diagnostics: bool,
    split: f32,
    dragging_split: bool,
    status: Option<(String, bool)>,
    render_tasks: HashMap<usize, Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn button(id: impl Into<SharedString>, label: impl Into<SharedString>) -> gpui::Stateful<gpui::Div> {
    let id: SharedString = id.into();
    div()
        .id(gpui::ElementId::Name(id))
        .px_2()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .text_color(rgb(0x1f2430))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0xe4e8f1)))
        .active(|s| s.bg(rgb(0xd5dbe8)))
        .child(label.into())
}

impl Workspace {
    pub fn new(paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let find_input = cx.new(|cx| TextEditor::new("", true, "Rechercher…", cx));
        let replace_input = cx.new(|cx| TextEditor::new("", true, "Remplacer par…", cx));
        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&find_input, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Changed => this.update_find_highlights(cx),
            EditorEvent::Submit => this.find_step(false, window, cx),
            EditorEvent::Cancel => this.close_find(window, cx),
            _ => {}
        }));
        subs.push(cx.subscribe_in(&replace_input, window, |this, _, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Submit => this.replace_all(&ReplaceAll, window, cx),
            EditorEvent::Cancel => this.close_find(window, cx),
            _ => {}
        }));
        let renderer = renderer::locate_renderer();
        let mut ws = Self {
            docs: Vec::new(),
            active: 0,
            next_id: 1,
            renderer,
            focus_handle: cx.focus_handle(),
            find_open: false,
            find_input,
            replace_input,
            path_prompt: None,
            templates_open: false,
            show_diagnostics: true,
            split: 0.45,
            dragging_split: false,
            status: None,
            render_tasks: HashMap::new(),
            _subscriptions: subs,
        };
        if ws.renderer.is_none() {
            ws.status = Some((
                "Moteur de rendu « pumlrender » introuvable : définissez PUMLRENDER ou construisez renderer/".into(),
                true,
            ));
        }
        for p in paths {
            ws.open_path(p, window, cx);
        }
        if ws.docs.is_empty() {
            ws.add_document(None, "sans titre.puml".into(), TEMPLATES[0].text, window, cx);
        }
        window.on_window_should_close(cx, {
            let this = cx.entity().downgrade();
            move |window, cx| {
                let Some(this) = this.upgrade() else { return true };
                let dirty: Vec<String> = this
                    .read(cx)
                    .docs
                    .iter()
                    .filter(|d| d.editor.read(cx).buffer.is_dirty())
                    .map(|d| d.display_name())
                    .collect();
                if dirty.is_empty() {
                    return true;
                }
                let rx = window.prompt(
                    PromptLevel::Warning,
                    "Des modifications ne sont pas enregistrées",
                    Some(&dirty.join(", ")),
                    &["Quitter sans enregistrer", "Annuler"],
                    cx,
                );
                cx.spawn(async move |cx| {
                    if let Ok(0) = rx.await {
                        cx.update(|cx| cx.quit()).ok();
                    }
                })
                .detach();
                false
            }
        });
        ws
    }

    fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_index(&self, id: usize) -> Option<usize> {
        self.docs.iter().position(|d| d.id == id)
    }

    fn set_status(&mut self, msg: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.status = Some((msg.into(), error));
        cx.notify();
    }

    fn add_document(
        &mut self,
        path: Option<PathBuf>,
        title: String,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = cx.new(|cx| TextEditor::new(text, false, "", cx));
        let id = self.next_id;
        self.next_id += 1;
        let sub = cx.subscribe_in(&editor, window, move |this, _, ev: &EditorEvent, _window, cx| match ev {
            EditorEvent::Changed => {
                this.schedule_render(id, 220, cx);
                cx.notify();
            }
            EditorEvent::CursorMoved => cx.notify(),
            _ => {}
        });
        self.docs.push(Document {
            id,
            editor: editor.clone(),
            path,
            title,
            output: None,
            render_error: None,
            image: None,
            rendered_version: None,
            rendering: false,
            block: 0,
            zoom: 1.0,
            _subscription: sub,
        });
        self.active = self.docs.len() - 1;
        self.status = None;
        self.schedule_render(id, 0, cx);
        window.focus(&editor.focus_handle(cx));
        self.update_title(window, cx);
        cx.notify();
    }

    fn update_title(&self, window: &mut Window, cx: &App) {
        if self.docs.is_empty() {
            return;
        }
        let d = self.doc();
        let dirty = if d.editor.read(cx).buffer.is_dirty() { "• " } else { "" };
        window.set_window_title(&format!("{dirty}{} — Éditeur PlantUML", d.display_name()));
    }

    // ------------------------------------------------------------ rendu

    fn schedule_render(&mut self, id: usize, delay_ms: u64, cx: &mut Context<Self>) {
        let Some(renderer) = self.renderer.clone() else { return };
        let task = cx.spawn(async move |this, cx| {
            if delay_ms > 0 {
                cx.background_executor().timer(Duration::from_millis(delay_ms)).await;
            }
            let Some((text, version, block)) = this
                .update(cx, |this, cx| {
                    let idx = this.doc_index(id)?;
                    let d = &mut this.docs[idx];
                    d.rendering = true;
                    let b = &d.editor.read(cx).buffer;
                    Some((b.text().to_string(), b.version(), d.block))
                })
                .ok()
                .flatten()
            else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { renderer::render(&renderer, &text, block) })
                .await;
            this.update(cx, |this, cx| {
                let Some(idx) = this.doc_index(id) else { return };
                let d = &mut this.docs[idx];
                d.rendering = false;
                d.rendered_version = Some(version);
                match result {
                    Ok(out) => {
                        d.render_error = None;
                        if d.block >= out.blocks {
                            d.block = 0;
                        }
                        d.image = None;
                        let marks: Vec<(usize, Severity, String)> = out
                            .diagnostics
                            .iter()
                            .filter(|g| g.line > 0)
                            .map(|g| (g.line - 1, g.severity, g.message.clone()))
                            .collect();
                        d.editor.update(cx, |ed, cx| {
                            ed.line_marks = marks;
                            cx.notify();
                        });
                        d.output = Some(out);
                    }
                    Err(e) => d.render_error = Some(e),
                }
                cx.notify();
            })
            .ok();
        });
        self.render_tasks.insert(id, task);
    }

    /// Rastérise le SVG au zoom courant. GPUI 0.2.2 n'échange pas les canaux
    /// des SVG fournis en mémoire (`Image`) : on produit donc nous-mêmes une
    /// image BGRA non prémultipliée, comme le fait son chargeur de fichiers.
    fn preview_image(&mut self, window: &Window, cx: &App) -> Option<Arc<RenderImage>> {
        let scale = window.scale_factor();
        let d = &mut self.docs[self.active];
        let out = d.output.as_ref()?;
        let key = (d.zoom * scale * 1000.0) as u32;
        if let Some((k, img)) = d.image.as_ref() {
            if *k == key {
                return Some(img.clone());
            }
        }
        let svg = out.scaled_svg(d.zoom * scale);
        let raw = Image::from_bytes(ImageFormat::Svg, svg.into_bytes())
            .to_image_data(cx.svg_renderer())
            .ok()?;
        let size = raw.size(0);
        let (w, h) = (size.width.0 as u32, size.height.0 as u32);
        let mut buf: image::RgbaImage = image::ImageBuffer::from_raw(w, h, raw.as_bytes(0)?.to_vec())?;
        // resvg produit du RGBA prémultiplié ; GPUI attend du BGRA non prémultiplié
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            if px[3] > 0 && px[3] < 255 {
                let a = px[3] as f32 / 255.0;
                for c in &mut px[..3] {
                    *c = (*c as f32 / a).min(255.0) as u8;
                }
            }
        }
        let image = Arc::new(RenderImage::new(SmallVec::from_elem(image::Frame::new(buf), 1)));
        d.image = Some((key, image.clone()));
        Some(image)
    }

    // ------------------------------------------------------------ fichiers

    fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(path.as_path())) {
            self.active = i;
            cx.notify();
            return;
        }
        let is_simp = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("simp"))
            .unwrap_or(false);
        if is_simp {
            self.import_simp_path(path, window, cx);
            return;
        }
        match files::read_text(&path) {
            Ok(text) => {
                // remplace l'onglet vide initial s'il n'a pas été modifié
                if self.docs.len() == 1
                    && self.docs[0].path.is_none()
                    && !self.docs[0].editor.read(cx).buffer.is_dirty()
                {
                    self.docs.clear();
                }
                let title = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                self.add_document(Some(path.clone()), title, &text, window, cx);
                self.set_status(format!("Ouvert : {}", path.display()), false, cx);
            }
            Err(e) => self.set_status(format!("Ouverture impossible : {e}"), true, cx),
        }
    }

    fn import_simp_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.set_status(format!("Import SIMP en cours : {}", path.display()), false, cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let p = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { files::import_simp(&p, None) })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(conv) => {
                    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    this.add_document(None, format!("{stem}.puml (importé)"), &conv.text, window, cx);
                    let n = conv.messages.iter().filter(|m| m.starts_with("Avertissement :")).count();
                    let msg = if n > 0 {
                        format!("SIMP importé avec {n} avertissement(s) : {}", conv.messages.join(" | "))
                    } else {
                        "SIMP importé (enregistrez le résultat sous un nouveau nom)".to_string()
                    };
                    this.set_status(msg, false, cx);
                }
                Err(e) => this.set_status(format!("Import SIMP impossible : {e}"), true, cx),
            })
            .ok();
        });
        task.detach();
    }

    fn save_to(&mut self, path: PathBuf, allow_overwrite: bool, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.doc().editor.read(cx).buffer.text().to_string();
        let same = self.doc().path.as_deref() == Some(path.as_path());
        let result = if same || allow_overwrite {
            files::save_document(&path, &text)
        } else {
            files::write_new(&path, &text)
        };
        match result {
            Ok(()) => {
                let d = &mut self.docs[self.active];
                d.path = Some(path.clone());
                d.editor.update(cx, |ed, cx| {
                    ed.buffer.mark_saved();
                    cx.notify();
                });
                self.update_title(window, cx);
                self.set_status(format!("Enregistré : {}", path.display()), false, cx);
            }
            Err(e) => self.set_status(format!("Enregistrement impossible : {e}"), true, cx),
        }
    }

    fn default_dir(&self) -> PathBuf {
        self.docs
            .get(self.active)
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn suggested_name(&self, ext: &str) -> String {
        let name = self.doc().display_name();
        let stem = name.split(" (").next().unwrap_or(&name);
        let stem = Path::new(stem).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("diagramme".into());
        format!("{stem}.{ext}")
    }

    /// Ouvre le sélecteur natif ; en cas d'échec (pas de portail XDG…),
    /// propose une saisie de chemin dans l'application.
    fn ask_path(&mut self, purpose: PathPurpose, window: &mut Window, cx: &mut Context<Self>) {
        let existing = matches!(purpose, PathPurpose::Open | PathPurpose::ImportSimp);
        let dir = self.default_dir();
        let suggested = match purpose {
            PathPurpose::SaveAs => self.suggested_name("puml"),
            PathPurpose::ExportSvg => self.suggested_name("svg"),
            PathPurpose::ExportSimp => self.suggested_name("simp"),
            _ => String::new(),
        };
        let fallback = dir.join(&suggested).to_string_lossy().to_string();
        if existing {
            let rx = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: purpose == PathPurpose::Open,
                prompt: None,
            });
            cx.spawn_in(window, async move |this, cx| {
                let res = rx.await;
                this.update_in(cx, |this, window, cx| match res {
                    Ok(Ok(Some(paths))) => {
                        for p in paths {
                            if purpose == PathPurpose::ImportSimp {
                                this.import_simp_path(p, window, cx);
                            } else {
                                this.open_path(p, window, cx);
                            }
                        }
                    }
                    Ok(Ok(None)) => {}
                    _ => this.show_path_prompt(purpose, fallback, window, cx),
                })
                .ok();
            })
            .detach();
        } else {
            let rx = cx.prompt_for_new_path(&dir, Some(&suggested));
            cx.spawn_in(window, async move |this, cx| {
                let res = rx.await;
                this.update_in(cx, |this, window, cx| match res {
                    // le sélecteur natif a déjà confirmé un éventuel remplacement
                    Ok(Ok(Some(p))) => this.complete_path(purpose, p, true, window, cx),
                    Ok(Ok(None)) => {}
                    _ => this.show_path_prompt(purpose, fallback, window, cx),
                })
                .ok();
            })
            .detach();
        }
    }

    fn show_path_prompt(&mut self, purpose: PathPurpose, initial: String, window: &mut Window, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            let mut e = TextEditor::new(&initial, true, "chemin du fichier", cx);
            let len = e.buffer.len();
            e.buffer.set_cursor(len);
            e
        });
        let sub = cx.subscribe_in(&input, window, move |this, input, ev: &EditorEvent, window, cx| match ev {
            EditorEvent::Submit => {
                let text = input.read(cx).text().trim().to_string();
                this.path_prompt = None;
                if !text.is_empty() {
                    let p = PathBuf::from(shellexpand(&text));
                    this.complete_path(purpose, p, false, window, cx);
                }
                cx.notify();
            }
            EditorEvent::Cancel => {
                this.path_prompt = None;
                this.focus_active(window, cx);
                cx.notify();
            }
            _ => {}
        });
        self._subscriptions.push(sub);
        window.focus(&input.focus_handle(cx));
        self.path_prompt = Some((purpose, input));
        cx.notify();
    }

    fn complete_path(&mut self, purpose: PathPurpose, p: PathBuf, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        match purpose {
            PathPurpose::Open => self.open_path(p, window, cx),
            PathPurpose::ImportSimp => self.import_simp_path(p, window, cx),
            PathPurpose::SaveAs => self.save_to(p, confirmed, window, cx),
            PathPurpose::ExportSvg => self.export_svg_to(p, cx),
            PathPurpose::ExportSimp => self.export_simp_to(p, cx),
        }
        self.focus_active(window, cx);
    }

    fn export_svg_to(&mut self, p: PathBuf, cx: &mut Context<Self>) {
        let Some(out) = self.doc().output.as_ref() else {
            self.set_status("Aucun rendu disponible à exporter", true, cx);
            return;
        };
        // un export ne remplace jamais un fichier existant
        match files::write_new(&p, &out.svg) {
            Ok(()) => self.set_status(format!("SVG exporté : {}", p.display()), false, cx),
            Err(e) => self.set_status(format!("Export SVG impossible : {e}"), true, cx),
        }
    }

    fn export_simp_to(&mut self, p: PathBuf, cx: &mut Context<Self>) {
        let text = self.doc().editor.read(cx).buffer.text().to_string();
        cx.spawn(async move |this, cx| {
            let target = p.clone();
            let res = cx
                .background_executor()
                .spawn(async move { files::export_simp(&text, &target) })
                .await;
            this.update(cx, |this, cx| match res {
                Ok(msgs) => this.set_status(
                    format!("SIMP exporté (expérimental, compatibilité non validée) : {} {}", p.display(), msgs.join(" | ")),
                    false,
                    cx,
                ),
                Err(e) => this.set_status(format!("Export SIMP impossible : {e}"), true, cx),
            })
            .ok();
        })
        .detach();
    }

    fn focus_active(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(d) = self.docs.get(self.active) {
            window.focus(&d.editor.focus_handle(cx));
        }
    }

    // ------------------------------------------------------------ actions

    fn new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.docs.iter().filter(|d| d.path.is_none()).count() + 1;
        self.add_document(None, format!("sans titre {n}.puml"), templates::blank(), window, cx);
        self.docs[self.active].editor.update(cx, |ed, cx| {
            ed.buffer.set_cursor(11);
            cx.notify();
        });
    }

    fn new_from_template(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let t = &TEMPLATES[idx];
        let n = self.docs.iter().filter(|d| d.path.is_none()).count() + 1;
        self.add_document(None, format!("{} {n}.puml", t.name.to_lowercase()), t.text, window, cx);
        self.templates_open = false;
    }

    fn open_file(&mut self, _: &OpenFile, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_path(PathPurpose::Open, window, cx);
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        match self.doc().path.clone() {
            Some(p) => self.save_to(p, true, window, cx),
            None => self.ask_path(PathPurpose::SaveAs, window, cx),
        }
    }

    fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_path(PathPurpose::SaveAs, window, cx);
    }

    fn export_svg(&mut self, _: &ExportSvg, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_path(PathPurpose::ExportSvg, window, cx);
    }

    fn import_simp(&mut self, _: &ImportSimp, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_path(PathPurpose::ImportSimp, window, cx);
    }

    fn export_simp(&mut self, _: &ExportSimp, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_path(PathPurpose::ExportSimp, window, cx);
    }

    fn close_tab_at(&mut self, idx: usize, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        if idx >= self.docs.len() {
            return;
        }
        if !force && self.docs[idx].editor.read(cx).buffer.is_dirty() {
            let name = self.docs[idx].display_name();
            let id = self.docs[idx].id;
            let rx = window.prompt(
                PromptLevel::Warning,
                &format!("Fermer « {name} » sans enregistrer ?"),
                Some("Les modifications seront perdues."),
                &["Fermer sans enregistrer", "Annuler"],
                cx,
            );
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(0) = rx.await {
                    this.update_in(cx, |this, window, cx| {
                        if let Some(i) = this.doc_index(id) {
                            this.close_tab_at(i, true, window, cx);
                        }
                    })
                    .ok();
                }
            })
            .detach();
            return;
        }
        let id = self.docs[idx].id;
        self.render_tasks.remove(&id);
        self.docs.remove(idx);
        if self.docs.is_empty() {
            self.add_document(None, "sans titre.puml".into(), templates::blank(), window, cx);
        }
        self.active = self.active.min(self.docs.len() - 1);
        self.focus_active(window, cx);
        self.update_title(window, cx);
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab_at(self.active, false, window, cx);
    }

    fn activate(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        if idx != self.active {
            self.status = None;
        }
        self.active = idx.min(self.docs.len() - 1);
        self.focus_active(window, cx);
        self.update_title(window, cx);
        if self.find_open {
            self.update_find_highlights(cx);
        }
        cx.notify();
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.docs.len();
        self.activate((self.active + 1) % n, window, cx);
    }

    fn prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.docs.len();
        self.activate((self.active + n - 1) % n, window, cx);
    }

    fn find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = true;
        let sel = self.doc().editor.read(cx).buffer.selected_text().to_string();
        if !sel.is_empty() && !sel.contains('\n') {
            self.find_input.update(cx, |e, cx| e.set_text(&sel, cx));
        }
        self.find_input.update(cx, |e, cx| {
            e.buffer.select_all();
            cx.notify();
        });
        window.focus(&self.find_input.focus_handle(cx));
        self.update_find_highlights(cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find_open = false;
        for d in &self.docs {
            d.editor.update(cx, |e, cx| {
                e.highlights.clear();
                cx.notify();
            });
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    fn update_find_highlights(&mut self, cx: &mut Context<Self>) {
        let q = self.find_input.read(cx).text().to_string();
        let ed = self.doc().editor.clone();
        ed.update(cx, |e, cx| {
            e.highlights.clear();
            if !q.is_empty() {
                let mut from = 0;
                let mut guard = 0;
                while let Some(r) = e.buffer.find(&q, from, false) {
                    if r.start < from || guard > 5000 {
                        break;
                    }
                    from = r.end.max(r.start + 1);
                    e.highlights.push(r);
                    guard += 1;
                }
            }
            cx.notify();
        });
        cx.notify();
    }

    fn find_step(&mut self, backwards: bool, window: &mut Window, cx: &mut Context<Self>) {
        let q = self.find_input.read(cx).text().to_string();
        if q.is_empty() {
            return;
        }
        let ed = self.doc().editor.clone();
        let found = ed.update(cx, |e, cx| {
            let b = &e.buffer;
            let from = if backwards { b.selection.start } else { b.selection.end };
            match b.find(&q, from, backwards) {
                Some(r) => {
                    e.select(r, cx);
                    true
                }
                None => false,
            }
        });
        if !found {
            self.set_status(format!("« {q} » introuvable"), true, cx);
        }
        let _ = window;
        cx.notify();
    }

    fn find_next(&mut self, _: &FindNext, window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(false, window, cx);
    }

    fn find_prev(&mut self, _: &FindPrev, window: &mut Window, cx: &mut Context<Self>) {
        self.find_step(true, window, cx);
    }

    fn replace_all(&mut self, _: &ReplaceAll, _: &mut Window, cx: &mut Context<Self>) {
        let q = self.find_input.read(cx).text().to_string();
        let r = self.replace_input.read(cx).text().to_string();
        if q.is_empty() {
            return;
        }
        let ed = self.doc().editor.clone();
        let n = ed.update(cx, |e, cx| {
            let n = e.buffer.replace_all(&q, &r);
            if n > 0 {
                cx.emit(EditorEvent::Changed);
            }
            cx.notify();
            n
        });
        self.update_find_highlights(cx);
        self.set_status(format!("{n} remplacement(s)"), false, cx);
    }

    fn set_zoom(&mut self, z: f32, cx: &mut Context<Self>) {
        let d = &mut self.docs[self.active];
        d.zoom = z.clamp(0.1, 6.0);
        cx.notify();
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        let z = self.doc().zoom * 1.25;
        self.set_zoom(z, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        let z = self.doc().zoom / 1.25;
        self.set_zoom(z, cx);
    }

    fn zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(1.0, cx);
    }

    fn zoom_fit(&mut self, _: &ZoomFit, window: &mut Window, cx: &mut Context<Self>) {
        let Some(out) = self.doc().output.as_ref() else { return };
        let vp = window.viewport_size();
        let w = f32::from(vp.width) * (1.0 - self.split) - 40.0;
        let h = f32::from(vp.height) - 180.0;
        if out.width > 0.0 && out.height > 0.0 {
            let z = (w / out.width).min(h / out.height);
            self.set_zoom(z, cx);
        }
    }

    fn toggle_diagnostics(&mut self, _: &ToggleDiagnostics, _: &mut Window, cx: &mut Context<Self>) {
        self.show_diagnostics = !self.show_diagnostics;
        cx.notify();
    }

    fn toggle_templates(&mut self, _: &ToggleTemplates, _: &mut Window, cx: &mut Context<Self>) {
        self.templates_open = !self.templates_open;
        cx.notify();
    }

    fn change_block(&mut self, delta: isize, cx: &mut Context<Self>) {
        let d = &mut self.docs[self.active];
        let blocks = d.output.as_ref().map(|o| o.blocks).unwrap_or(1).max(1) as isize;
        let nb = (d.block as isize + delta).rem_euclid(blocks) as usize;
        if nb != d.block {
            d.block = nb;
            let id = d.id;
            self.schedule_render(id, 0, cx);
        }
    }

    fn next_block(&mut self, _: &NextBlock, _: &mut Window, cx: &mut Context<Self>) {
        self.change_block(1, cx);
    }

    fn prev_block(&mut self, _: &PrevBlock, _: &mut Window, cx: &mut Context<Self>) {
        self.change_block(-1, cx);
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self.docs.iter().any(|d| d.editor.read(cx).buffer.is_dirty());
        if !dirty {
            cx.quit();
            return;
        }
        let rx = window.prompt(
            PromptLevel::Warning,
            "Des modifications ne sont pas enregistrées",
            Some("Quitter quand même ?"),
            &["Quitter sans enregistrer", "Annuler"],
            cx,
        );
        cx.spawn(async move |_, cx| {
            if let Ok(0) = rx.await {
                cx.update(|cx| cx.quit()).ok();
            }
        })
        .detach();
    }

    fn jump_to_line(&mut self, line: usize, window: &mut Window, cx: &mut Context<Self>) {
        if line == 0 {
            return;
        }
        let ed = self.doc().editor.clone();
        ed.update(cx, |e, cx| e.go_to_line(line - 1, cx));
        window.focus(&ed.focus_handle(cx));
    }

    // ------------------------------------------------------------ séparateur

    fn on_split_down(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.dragging_split = true;
        cx.notify();
    }

    fn on_root_move(&mut self, ev: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.dragging_split {
            if ev.pressed_button != Some(MouseButton::Left) {
                self.dragging_split = false;
            } else {
                let w = f32::from(window.viewport_size().width);
                self.split = (f32::from(ev.position.x) / w).clamp(0.15, 0.85);
            }
            cx.notify();
        }
    }

    fn on_root_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging_split {
            self.dragging_split = false;
            cx.notify();
        }
    }

    fn on_preview_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.modifiers.secondary() {
            let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
            let z = self.doc().zoom * if dy > 0.0 { 1.1 } else { 1.0 / 1.1 };
            self.set_zoom(z, cx);
            cx.stop_propagation();
        }
    }

    // ------------------------------------------------------------ rendu UI

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = (self.doc().zoom * 100.0).round();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .bg(rgb(0xf4f6fa))
            .border_b_1()
            .border_color(rgb(0xdde2ec))
            .child(button("tb-new", "Nouveau").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.new_file(&NewFile, w, cx))))
            .child(button("tb-tpl", "Modèles ▾").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.toggle_templates(&ToggleTemplates, w, cx))))
            .child(button("tb-open", "Ouvrir").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.open_file(&OpenFile, w, cx))))
            .child(button("tb-save", "Enregistrer").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.save(&Save, w, cx))))
            .child(button("tb-saveas", "Enregistrer sous").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.save_as(&SaveAs, w, cx))))
            .child(div().w(px(1.)).h(px(18.)).bg(rgb(0xd5dbe8)).mx_1())
            .child(button("tb-svg", "Exporter SVG").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.export_svg(&ExportSvg, w, cx))))
            .child(button("tb-isimp", "Importer SIMP").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.import_simp(&ImportSimp, w, cx))))
            .child(button("tb-esimp", "Exporter SIMP").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.export_simp(&ExportSimp, w, cx))))
            .child(div().w(px(1.)).h(px(18.)).bg(rgb(0xd5dbe8)).mx_1())
            .child(button("tb-find", "Rechercher").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.find(&Find, w, cx))))
            .child(button("tb-diag", "Diagnostics").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.toggle_diagnostics(&ToggleDiagnostics, w, cx))))
            .child(div().flex_1())
            .child(button("tb-zout", "−").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.zoom_out(&ZoomOut, w, cx))))
            .child(button("tb-z100", format!("{zoom} %")).on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.zoom_reset(&ZoomReset, w, cx))))
            .child(button("tb-zin", "+").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.zoom_in(&ZoomIn, w, cx))))
            .child(button("tb-zfit", "Ajuster").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.zoom_fit(&ZoomFit, w, cx))))
    }

    fn render_templates(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .px_2()
            .py_1()
            .bg(rgb(0xeef2f8))
            .border_b_1()
            .border_color(rgb(0xdde2ec))
            .child(div().text_size(px(12.)).text_color(rgb(0x6b7280)).py_1().pr_2().child("Nouveau diagramme :"))
            .children(TEMPLATES.iter().enumerate().map(|(i, t)| {
                button(SharedString::from(format!("tpl-{i}")), t.name)
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xd5dbe8))
                    .on_click(cx.listener(move |this, _: &ClickEvent, w, cx| this.new_from_template(i, w, cx)))
            }))
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .bg(rgb(0xe9edf4))
            .border_b_1()
            .border_color(rgb(0xdde2ec))
            .children(self.docs.iter().enumerate().map(|(i, d)| {
                let active = i == self.active;
                let dirty = d.editor.read(cx).buffer.is_dirty();
                div()
                    .id(gpui::ElementId::Integer(d.id as u64))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .text_size(px(13.))
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(rgb(0xdde2ec))
                    .when(active, |s| s.bg(rgb(0xffffff)).text_color(rgb(0x1f2430)))
                    .when(!active, |s| s.text_color(rgb(0x6b7280)).hover(|s| s.bg(rgb(0xf3f5fa))))
                    .on_click(cx.listener(move |this, _: &ClickEvent, w, cx| this.activate(i, w, cx)))
                    .child(format!("{}{}", if dirty { "● " } else { "" }, d.display_name()))
                    .child(
                        div()
                            .id(gpui::ElementId::Name(format!("close-{}", d.id).into()))
                            .px_1()
                            .rounded_sm()
                            .text_color(rgb(0x9aa1ad))
                            .hover(|s| s.bg(rgb(0xdde2ec)).text_color(rgb(0x1f2430)))
                            .on_click(cx.listener(move |this, _: &ClickEvent, w, cx| {
                                cx.stop_propagation();
                                this.close_tab_at(i, false, w, cx)
                            }))
                            .child("×"),
                    )
            }))
            .child(
                button("tab-new", "+")
                    .on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.new_file(&NewFile, w, cx))),
            )
    }

    fn render_find(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.find_input.read(cx).text().to_string();
        let count = self.doc().editor.read(cx).buffer.count_matches(&q);
        let field = |e: Entity<TextEditor>| {
            div()
                .w(px(220.))
                .h(px(26.))
                .px_2()
                .py(px(3.))
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(rgb(0xc9d0dd))
                .rounded_md()
                .overflow_hidden()
                .child(e)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(rgb(0xf4f6fa))
            .border_b_1()
            .border_color(rgb(0xdde2ec))
            .child(field(self.find_input.clone()))
            .child(button("find-prev", "↑").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.find_step(true, w, cx))))
            .child(button("find-next", "↓").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.find_step(false, w, cx))))
            .child(div().text_size(px(12.)).text_color(rgb(0x6b7280)).child(format!("{count} résultat(s)")))
            .child(field(self.replace_input.clone()))
            .child(button("replace-all", "Tout remplacer").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.replace_all(&ReplaceAll, w, cx))))
            .child(div().flex_1())
            .child(button("find-close", "×").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.close_find(w, cx))))
    }

    fn render_path_prompt(&self, purpose: PathPurpose, input: Entity<TextEditor>, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(rgb(0xfff8e1))
            .border_b_1()
            .border_color(rgb(0xe8d9a8))
            .child(div().text_size(px(13.)).child(purpose.label()))
            .child(
                div()
                    .flex_1()
                    .h(px(26.))
                    .px_2()
                    .py(px(3.))
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0xc9d0dd))
                    .rounded_md()
                    .overflow_hidden()
                    .child(input),
            )
            .child(div().text_size(px(12.)).text_color(rgb(0x6b7280)).child("Entrée pour valider, Échap pour annuler"))
            .child(button("path-cancel", "Annuler").on_click(cx.listener(|this, _: &ClickEvent, w, cx| {
                this.path_prompt = None;
                this.focus_active(w, cx);
                cx.notify();
            })))
    }

    fn render_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let image = self.preview_image(window, cx);
        let d = self.doc();
        let zoom = d.zoom;
        let mut root = div()
            .id("preview")
            .flex_1()
            .h_full()
            .overflow_scroll()
            .bg(rgb(0xeceff4))
            .on_scroll_wheel(cx.listener(Self::on_preview_scroll));
        if let Some(err) = d.render_error.clone() {
            return root
                .p_4()
                .child(div().text_color(rgb(0xb3261e)).text_size(px(13.)).child(err))
                .into_any_element();
        }
        if self.renderer.is_none() {
            return root
                .p_4()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(0x6b7280))
                        .child("Aperçu indisponible : moteur « pumlrender » introuvable. Construisez-le avec `nimble build` dans renderer/ ou définissez PUMLRENDER."),
                )
                .into_any_element();
        }
        let Some(out) = d.output.as_ref() else {
            return root
                .p_4()
                .child(div().text_size(px(13.)).text_color(rgb(0x6b7280)).child("Rendu en cours…"))
                .into_any_element();
        };
        let w = out.width * zoom;
        let h = out.height * zoom;
        let regions: Vec<_> = out.regions.clone();
        let mut canvas = div()
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .bg(rgb(0xffffff))
            .shadow_sm();
        if let Some(image) = image {
            canvas = canvas.child(img(image).absolute().top_0().left_0().w(px(w)).h(px(h)));
        }
        canvas = canvas.children(regions.into_iter().enumerate().map(|(i, r)| {
            let line = r.line;
            div()
                .id(gpui::ElementId::Name(format!("rg-{i}").into()))
                .absolute()
                .left(px(r.x * zoom))
                .top(px(r.y * zoom))
                .w(px((r.w * zoom).max(4.0)))
                .h(px((r.h * zoom).max(4.0)))
                .rounded_sm()
                .cursor_pointer()
                .hover(|s| s.bg(rgba(0x3a6ea51f)).border_1().border_color(rgba(0x3a6ea5aa)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, w, cx| this.jump_to_line(line, w, cx)),
                )
        }));
        root = root.p_4().child(canvas);
        root.into_any_element()
    }

    fn render_diagnostics(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.doc();
        let diags = d.output.as_ref().map(|o| o.diagnostics.clone()).unwrap_or_default();
        div()
            .id("diagnostics")
            .h(px(120.))
            .flex_none()
            .overflow_y_scroll()
            .bg(rgb(0xfbfcfe))
            .border_t_1()
            .border_color(rgb(0xdde2ec))
            .text_size(px(12.5))
            .when(diags.is_empty(), |s| {
                s.child(div().px_3().py_2().text_color(rgb(0x6b7280)).child("Aucun diagnostic."))
            })
            .children(diags.into_iter().enumerate().map(|(i, g)| {
                let (icon, color) = match g.severity {
                    Severity::Error => ("✖", rgb(0xd32f2f)),
                    Severity::Warning => ("▲", rgb(0xb7791f)),
                    Severity::Info => ("ℹ", rgb(0x3a6ea5)),
                };
                let line = g.line;
                div()
                    .id(gpui::ElementId::Name(format!("dg-{i}").into()))
                    .flex()
                    .flex_row()
                    .gap_2()
                    .px_3()
                    .py(px(3.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0xeef2f8)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, w, cx| this.jump_to_line(line, w, cx)))
                    .child(div().text_color(color).child(icon))
                    .child(div().text_color(rgb(0x6b7280)).w(px(70.)).child(format!("ligne {line}")))
                    .child(div().text_color(rgb(0x1f2430)).child(g.message))
            }))
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.doc();
        let ed = d.editor.read(cx);
        let (l, c) = ed.cursor_line_col();
        let (errs, warns, kind, blocks) = d
            .output
            .as_ref()
            .map(|o| (o.count(Severity::Error), o.count(Severity::Warning), o.kind.clone(), o.blocks))
            .unwrap_or((0, 0, String::new(), 1));
        let stale = d.rendered_version != Some(ed.buffer.version());
        let state = if d.rendering || stale { "rendu…" } else { "à jour" };
        let path = d.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "non enregistré".into());
        let msg = self.status.clone();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py(px(3.))
            .bg(rgb(0xf4f6fa))
            .border_t_1()
            .border_color(rgb(0xdde2ec))
            .text_size(px(12.))
            .text_color(rgb(0x4b5563))
            .child(format!("Ln {l}, Col {c}"))
            .child(format!("Type : {}", if kind.is_empty() { "—" } else { &kind }))
            .when(blocks > 1, |s| {
                s.child(button("blk-prev", "◀").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.prev_block(&PrevBlock, w, cx))))
                    .child(format!("bloc {}/{}", d.block + 1, blocks))
                    .child(button("blk-next", "▶").on_click(cx.listener(|this, _: &ClickEvent, w, cx| this.next_block(&NextBlock, w, cx))))
            })
            .child(div().text_color(if errs > 0 { rgb(0xd32f2f) } else { rgb(0x4b5563) }).child(format!("✖ {errs}")))
            .child(div().text_color(if warns > 0 { rgb(0xb7791f) } else { rgb(0x4b5563) }).child(format!("▲ {warns}")))
            .child(state)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .when_some(msg, |s, (m, err)| s.when(err, |s| s.text_color(rgb(0xd32f2f))).child(m)),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(path),
            )
    }
}

/// Développe ~ en début de chemin.
fn shellexpand(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest).to_string_lossy().to_string();
        }
    }
    s.to_string()
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.update_title(window, cx);
        let editor = self.doc().editor.clone();
        let split = self.split;
        let preview = self.render_preview(window, cx);
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .font_family(".SystemUIFont")
            .on_action(cx.listener(Self::new_file))
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::prev_tab))
            .on_action(cx.listener(Self::export_svg))
            .on_action(cx.listener(Self::import_simp))
            .on_action(cx.listener(Self::export_simp))
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::find_next))
            .on_action(cx.listener(Self::find_prev))
            .on_action(cx.listener(Self::replace_all))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::zoom_fit))
            .on_action(cx.listener(Self::toggle_diagnostics))
            .on_action(cx.listener(Self::toggle_templates))
            .on_action(cx.listener(Self::next_block))
            .on_action(cx.listener(Self::prev_block))
            .on_action(cx.listener(Self::quit))
            .on_mouse_move(cx.listener(Self::on_root_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_root_up))
            .child(self.render_toolbar(cx))
            .when(self.templates_open, |s| s.child(self.render_templates(cx)))
            .when_some(self.path_prompt.clone(), |s, (p, input)| s.child(self.render_path_prompt(p, input, cx)))
            .when(self.find_open, |s| s.child(self.render_find(cx)))
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
                            .when(self.show_diagnostics, |s| s.child(self.render_diagnostics(cx))),
                    )
                    .child(
                        div()
                            .id("splitter")
                            .w(px(5.))
                            .h_full()
                            .flex_none()
                            .bg(if self.dragging_split { rgb(0x3a6ea5) } else { rgb(0xdde2ec) })
                            .hover(|s| s.bg(rgb(0xb8c3d6)))
                            .cursor_col_resize()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_split_down)),
                    )
                    .child(preview),
            )
            .child(self.render_status(cx))
    }
}
