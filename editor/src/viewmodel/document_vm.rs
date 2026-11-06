//! ViewModel d'un document (un onglet) : texte édité, fichier associé,
//! rendu en direct et état de l'aperçu (zoom, défilement, survol).
//!
//! Rendu : chaque modification relance un rendu après un court délai ; seul
//! le plus récent est conservé (les résultats périmés sont ignorés). Le SVG
//! est converti en scène vectorielle dans l'exécuteur d'arrière-plan, comme
//! Zed le fait pour tout travail coûteux, afin de ne jamais bloquer l'interface.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, Context, Entity, EventEmitter, Subscription, Task};

use crate::model::diagram::{RenderOutput, Severity};
use crate::model::svg_scene::Scene;
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

pub struct DocumentViewModel {
    pub editor: Entity<EditorViewModel>,
    pub path: Option<PathBuf>,
    title: String,
    pub rendering: Option<Rendering>,
    pub render_error: Option<String>,
    pub render_pending: bool,
    pub block: usize,
    pub preview: PreviewState,
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

    pub fn schedule_render(&mut self, delay: Duration, cx: &mut Context<Self>) {
        let Some(service) = self.service.clone() else { return };
        self.render_pending = true;
        // remplacer la tâche annule la précédente (dernier rendu gagnant)
        self.render_task = Some(cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }
            let Ok((text, version, block)) = this.update(cx, |this, cx| {
                let buffer = &this.editor.read(cx).buffer;
                (buffer.text().to_string(), buffer.version(), this.block)
            }) else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let output = service.render(&text, block)?;
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

    /// Région de l'aperçu sous un point écran (relatif au coin de la zone).
    pub fn region_at(&self, x: f32, y: f32) -> Option<usize> {
        let r = self.rendering.as_ref()?;
        let (sx, sy) = self.screen_to_scene(x, y);
        r.output.region_at(sx, sy)
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
        let region = self.region_at(x, y);
        if region != self.preview.hovered_region {
            self.preview.hovered_region = region;
            cx.notify();
        }
    }

    pub fn click_preview(&mut self, x: f32, y: f32, cx: &mut Context<Self>) -> bool {
        let Some(i) = self.region_at(x, y) else { return false };
        let line = self.output().map(|o| o.regions[i].line).unwrap_or(0);
        if line > 0 {
            cx.emit(DocumentEvent::JumpToLine(line));
        }
        true
    }
}
