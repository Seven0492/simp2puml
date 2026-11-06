//! ViewModel d'un document (un onglet) : texte édité, fichier associé,
//! rendu en direct et état de l'aperçu (zoom, défilement, survol).
//!
//! Rendu : chaque modification relance un rendu après un court délai ; seul
//! le plus récent est conservé (les résultats périmés sont ignorés). Le SVG
//! est converti en scène vectorielle dans l'exécuteur d'arrière-plan, comme
//! Zed le fait pour tout travail coûteux, afin de ne jamais bloquer l'interface.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui::{AppContext, Context, Entity, EventEmitter, Subscription, Task};

use crate::model::diagram::{Hit, RenderOutput, Severity};
use crate::model::preferences::{LinkClick, SecurityProfile};
use crate::model::preflight::{self, NoFiles};
use crate::model::svg_scene::Scene;
use crate::services::include_service::DocumentFolder;
use crate::services::render_service::RenderService;
use crate::viewmodel::editor_vm::{EditorEvent, EditorViewModel, LineMark};

/// Délai après la dernière frappe avant de relancer le rendu.
const RENDER_DEBOUNCE: Duration = Duration::from_millis(150);
const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.0;

#[derive(Clone, Debug)]
pub enum DocumentEvent {
    /// Un nouveau rendu est disponible.
    Rendered,
    /// L'utilisateur a cliqué un élément de l'aperçu : aller à cette ligne (1-based).
    JumpToLine(usize),
}

/// Résultat affichable d'un rendu.
pub struct Rendering {
    pub output: Arc<RenderOutput>,
    pub scene: Arc<Scene>,
    pub version: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewState {
    pub zoom: f32,
    /// Décalage de défilement, en pixels écran.
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub hovered_region: Option<usize>,
}

/// Distance au trait d'une flèche pour la toucher, en pixels à l'écran.
const LINK_TOLERANCE: f32 = 6.0;
const CHOICE_LABEL_CHARS: usize = 60;

/// Choix proposé quand plusieurs flèches se chevauchent sous le clic.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkChoice {
    pub link: usize,
    pub line: usize,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkMenu {
    /// Position du clic, relative à la zone d'aperçu.
    pub x: f32,
    pub y: f32,
    pub choices: Vec<LinkChoice>,
}

pub struct DocumentViewModel {
    pub editor: Entity<EditorViewModel>,
    pub path: Option<PathBuf>,
    /// Date et taille du fichier à l'ouverture ou au dernier enregistrement.
    pub disk_stamp: Option<(SystemTime, u64)>,
    title: String,
    pub rendering: Option<Rendering>,
    pub render_error: Option<String>,
    pub render_pending: bool,
    pub block: usize,
    pub preview: PreviewState,
    /// Ce que le diagramme a le droit de faire (préférences).
    pub security_profile: SecurityProfile,
    pub link_click: LinkClick,
    /// Flèches sous le pointeur, surlignées sur tout leur tracé.
    pub hovered_links: Vec<usize>,
    pub link_menu: Option<LinkMenu>,
    service: Option<Arc<RenderService>>,
    render_task: Option<Task<()>>,
    _editor_subscription: Subscription,
}

impl EventEmitter<DocumentEvent> for DocumentViewModel {}

impl DocumentViewModel {
    pub fn new(
        text: &str,
        path: Option<PathBuf>,
        title: String,
        service: Option<Arc<RenderService>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let editor = cx.new(|_| EditorViewModel::new(text, false, ""));
        let sub = cx.subscribe(&editor, |this, _, ev: &EditorEvent, cx| {
            if matches!(ev, EditorEvent::Changed) {
                this.schedule_render(RENDER_DEBOUNCE, cx);
            }
        });
        let mut doc = Self {
            editor,
            path,
            disk_stamp: None,
            title,
            rendering: None,
            render_error: None,
            render_pending: false,
            block: 0,
            preview: PreviewState {
                zoom: 1.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                viewport_w: 0.0,
                viewport_h: 0.0,
                hovered_region: None,
            },
            security_profile: SecurityProfile::Macros,
            link_click: LinkClick::Along,
            hovered_links: Vec::new(),
            link_menu: None,
            service,
            render_task: None,
            _editor_subscription: sub,
        };
        doc.schedule_render(Duration::ZERO, cx);
        doc
    }

    pub fn display_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.title.clone())
    }

    pub fn has_renderer(&self) -> bool {
        self.service.is_some()
    }

    pub fn output(&self) -> Option<&RenderOutput> {
        self.rendering.as_ref().map(|r| r.output.as_ref())
    }

    /// Le rendu affiché correspond-il au texte actuel ?
    pub fn is_up_to_date(&self, cx: &gpui::App) -> bool {
        let version = self.editor.read(cx).buffer.version();
        !self.render_pending && self.rendering.as_ref().is_some_and(|r| r.version == version)
    }

    // ------------------------------------------------------------ rendu

    /// Change de moteur, ou de profil de sécurité : le texte est rendu à nouveau.
    pub fn reconfigure(&mut self, service: Option<Arc<RenderService>>, profile: SecurityProfile, cx: &mut Context<Self>) {
        self.service = service;
        self.security_profile = profile;
        self.render_error = None;
        self.schedule_render(Duration::ZERO, cx);
    }

    /// Rendu à nouveau, avec les mêmes réglages (moteur qui vient d'être installé).
    pub fn rerender(&mut self, cx: &mut Context<Self>) {
        self.render_error = None;
        self.schedule_render(Duration::ZERO, cx);
    }

    pub fn schedule_render(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(service) = self.service.clone() else { return };
        self.render_pending = true;
        // remplacer la tâche annule la précédente (dernier rendu gagnant)
        self.render_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok((text, version, block, profile, path)) = this.update(cx, |this, cx| {
                let buffer = &this.editor.read(cx).buffer;
                (buffer.text().to_string(), buffer.version(), this.block, this.security_profile, this.path.clone())
            }) else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    // le profil s'applique ici : les deux moteurs reçoivent le même texte, sans lire de fichier
                    let folder = (profile == SecurityProfile::Folder).then(|| path.as_deref().and_then(DocumentFolder::new)).flatten();
                    let prepared = match &folder {
                        Some((source, root)) => preflight::prepare(&text, profile, Some(root), source),
                        None => preflight::prepare(&text, profile, None, &NoFiles),
                    };
                    let mut output = service.render(&prepared.text, block)?;
                    prepared.apply(&mut output);
                    let scene = Scene::parse(&output.svg)?;
                    Ok::<_, String>((output, scene))
                })
                .await;
            this.update(cx, |this, cx| this.apply_render(result, version, cx)).ok();
        }));
    }

    fn apply_render(&mut self, result: Result<(RenderOutput, Scene), String>, version: u64, cx: &mut Context<Self>) {
        self.render_pending = false;
        match result {
            Ok((output, scene)) => {
                self.render_error = None;
                // même règle que le moteur : un bloc disparu ramène au dernier
                self.block = self.block.min(output.blocks.max(1) - 1);
                let marks = output
                    .diagnostics
                    .iter()
                    .filter(|d| d.line > 0)
                    .map(|d| LineMark { line: d.line - 1, severity: d.severity })
                    .collect();
                self.editor.update(cx, |ed, cx| {
                    ed.line_marks = marks;
                    cx.notify();
                });
                self.preview.hovered_region = None;
                self.hovered_links.clear();
                self.link_menu = None;
                self.rendering = Some(Rendering { output: Arc::new(output), scene: Arc::new(scene), version });
            }
            Err(e) => self.render_error = Some(e),
        }
        cx.emit(DocumentEvent::Rendered);
        cx.notify();
    }

    pub fn change_block(&mut self, delta: isize, cx: &mut Context<Self>) {
        let blocks = self.output().map(|o| o.blocks).unwrap_or(1).max(1) as isize;
        let block = (self.block as isize + delta).rem_euclid(blocks) as usize;
        if block != self.block {
            self.block = block;
            self.schedule_render(Duration::ZERO, cx);
        }
    }

    pub fn diagnostic_counts(&self) -> (usize, usize) {
        self.output()
            .map(|o| (o.count(Severity::Error), o.count(Severity::Warning)))
            .unwrap_or((0, 0))
    }

    // ------------------------------------------------------------ aperçu

    pub fn set_viewport(&mut self, w: f32, h: f32) {
        self.preview.viewport_w = w;
        self.preview.viewport_h = h;
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let Some(r) = &self.rendering else { return };
        let p = &mut self.preview;
        let max_x = (r.scene.width * p.zoom - p.viewport_w + 40.0).max(0.0);
        let max_y = (r.scene.height * p.zoom - p.viewport_h + 40.0).max(0.0);
        p.scroll_x = p.scroll_x.clamp(0.0, max_x);
        p.scroll_y = p.scroll_y.clamp(0.0, max_y);
    }

    /// Zoom autour d'un point de la zone d'aperçu (le point reste fixe).
    pub fn zoom_at(&mut self, factor: f32, anchor_x: f32, anchor_y: f32, cx: &mut Context<Self>) {
        let p = &mut self.preview;
        let new_zoom = (p.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let k = new_zoom / p.zoom;
        p.scroll_x = (p.scroll_x + anchor_x) * k - anchor_x;
        p.scroll_y = (p.scroll_y + anchor_y) * k - anchor_y;
        p.zoom = new_zoom;
        self.clamp_scroll();
        cx.notify();
    }

    /// Zoom centré sur la zone visible.
    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let (cx_, cy_) = (self.preview.viewport_w / 2.0, self.preview.viewport_h / 2.0);
        self.zoom_at(factor, cx_, cy_, cx);
    }

    pub fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        let factor = 1.0 / self.preview.zoom;
        self.zoom_by(factor, cx);
    }

    /// Ajuste le zoom pour que tout le diagramme soit visible.
    pub fn zoom_fit(&mut self, cx: &mut Context<Self>) {
        let Some(r) = &self.rendering else { return };
        let p = &mut self.preview;
        if r.scene.width <= 0.0 || r.scene.height <= 0.0 || p.viewport_w <= 0.0 {
            return;
        }
        let pad = 32.0;
        p.zoom = ((p.viewport_w - pad) / r.scene.width)
            .min((p.viewport_h - pad) / r.scene.height)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        p.scroll_x = 0.0;
        p.scroll_y = 0.0;
        cx.notify();
    }

    pub fn scroll_preview(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        self.preview.scroll_x -= dx;
        self.preview.scroll_y -= dy;
        self.clamp_scroll();
        cx.notify();
    }

    /// Élément de l'aperçu sous un point écran (relatif au coin de la zone).
    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let r = self.rendering.as_ref()?;
        let (sx, sy) = self.screen_to_scene(x, y);
        r.output.hit_at(sx, sy, LINK_TOLERANCE / self.preview.zoom, self.link_click)
    }

    pub fn screen_to_scene(&self, x: f32, y: f32) -> (f32, f32) {
        let p = &self.preview;
        let (ox, oy) = self.scene_origin();
        ((x - ox + p.scroll_x) / p.zoom, (y - oy + p.scroll_y) / p.zoom)
    }

    /// Marge autour du diagramme (centré s'il est plus petit que la zone).
    pub fn scene_origin(&self) -> (f32, f32) {
        let pad = 16.0;
        let Some(r) = &self.rendering else { return (pad, pad) };
        let p = &self.preview;
        let w = r.scene.width * p.zoom;
        let x = if w + 2.0 * pad < p.viewport_w { (p.viewport_w - w) / 2.0 } else { pad };
        (x, pad)
    }

    pub fn hover(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        if self.link_menu.is_some() {
            return; // le surlignage suit alors le menu
        }
        let (region, links) = match self.hit_at(x, y) {
            Some(Hit::Region(i)) => (Some(i), Vec::new()),
            Some(Hit::Links(links)) => (None, links),
            None => (None, Vec::new()),
        };
        if region != self.preview.hovered_region || links != self.hovered_links {
            self.preview.hovered_region = region;
            self.hovered_links = links;
            cx.notify();
        }
    }

    /// Clic dans l'aperçu ; vrai s'il a servi (sinon il peut commencer un glisser).
    pub fn click_preview(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> bool {
        // un menu ouvert se referme au premier clic à côté, qui ne fait rien d'autre
        if self.link_menu.take().is_some() {
            self.hovered_links.clear();
            cx.notify();
            return true;
        }
        let Some(output) = self.output() else { return false };
        let line = match self.hit_at(x, y) {
            None => return false,
            Some(Hit::Region(i)) => self.output().map(|o| o.regions[i].line).unwrap_or(0),
            Some(Hit::Links(links)) => {
                // une entrée par ligne de source : deux tracés d'une même ligne mènent au même endroit
                let mut lines: Vec<(usize, usize)> = Vec::new();
                for i in links {
                    let line = output.links[i].line;
                    if !lines.iter().any(|(_, l)| *l == line) {
                        lines.push((i, line));
                    }
                }
                if lines.len() > 1 {
                    let buffer = &self.editor.read(cx).buffer;
                    let choices = lines
                        .into_iter()
                        .map(|(link, line)| {
                            let text = buffer.line_text(line.saturating_sub(1)).trim();
                            let label: String = text.chars().take(CHOICE_LABEL_CHARS).collect();
                            LinkChoice { link, line, label }
                        })
                        .collect();
                    self.link_menu = Some(LinkMenu { x, y, choices });
                    cx.notify();
                    return true;
                }
                lines[0].1
            }
        };
        if line > 0 {
            cx.emit(DocumentEvent::JumpToLine(line));
        }
        true
    }

    /// Entrée du menu : aller à cette flèche.
    pub fn choose_link(&mut self, line: usize, cx: &mut Context<Self>) {
        self.dismiss_link_menu(cx);
        cx.emit(DocumentEvent::JumpToLine(line));
    }

    pub fn dismiss_link_menu(&mut self, cx: &mut Context<Self>) {
        if self.link_menu.take().is_some() {
            self.hovered_links.clear();
            cx.notify();
        }
    }

    /// Survol d'une entrée du menu : surligne la flèche correspondante.
    pub fn highlight_link(&mut self, link: Option<usize>, cx: &mut Context<Self>) {
        self.hovered_links = link.into_iter().collect();
        cx.notify();
    }

    pub fn set_link_click(&mut self, mode: LinkClick, cx: &mut Context<Self>) {
        self.link_click = mode;
        self.hovered_links.clear();
        self.link_menu = None;
        cx.notify();
    }
}
