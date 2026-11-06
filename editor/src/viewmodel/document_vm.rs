//! ViewModel d'un document (un onglet) : texte édité, fichier associé,
//! rendu en direct et état de l'aperçu (zoom, défilement, survol).
//!
//! Rendu : chaque modification relance un rendu après un court délai ; seul
//! le plus récent est conservé (les résultats périmés sont ignorés). Le SVG
//! est converti en scène vectorielle dans l'exécuteur d'arrière-plan, comme
//! Zed le fait pour tout travail coûteux, afin de ne jamais bloquer l'interface.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use gpui::{AppContext, Context, Entity, EventEmitter, Subscription, Task};

use crate::model::diagram::{Hit, RenderOutput, Severity};
use crate::model::preferences::SecurityProfile;
use crate::model::preflight::{self, NoFiles};
use crate::model::source_location;
use crate::model::svg_scene::Scene;
use crate::services::include_service::DocumentFolder;
use crate::services::render_service::RenderService;
use crate::viewmodel::editor_vm::{EditorEvent, EditorViewModel, LineMark};

/// Délai avant de relancer le moteur pendant une rafale de frappes : deux fois le dernier rendu,
/// entre ces deux bornes.
const MIN_RENDER_DELAY: Duration = Duration::from_millis(16);
const MAX_RENDER_DELAY: Duration = Duration::from_millis(150);

/// Une frappe isolée se rend tout de suite ; seule une frappe qui en suit une autre de près attend.
/// `since_last_edit` : `None` à la toute première modification.
fn render_delay(last_render: Duration, since_last_edit: Option<Duration>) -> Duration {
    let delay = (last_render * 2).clamp(MIN_RENDER_DELAY, MAX_RENDER_DELAY);
    if since_last_edit.is_none_or(|since| since >= delay) {
        Duration::ZERO
    } else {
        delay
    }
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 8.0;

/// Zoom sur une grille : un va-et-vient repasse par les mêmes valeurs. L'atlas de GPUI range les
/// glyphes par taille exacte et ne les libère jamais : sans grille, chaque pas en ajouterait.
fn snap_zoom(zoom: f32) -> f32 {
    ((zoom * 1024.0).round() / 1024.0).clamp(MIN_ZOOM, MAX_ZOOM)
}

#[derive(Clone, Debug)]
pub enum DocumentEvent {
    /// Un nouveau rendu est disponible.
    Rendered,
    /// L'utilisateur a cliqué un élément de l'aperçu : aller à cette ligne (1-based).
    JumpToLine(usize),
    /// Ctrl+clic : aller à cet endroit précis du texte (une sélection pour une flèche).
    JumpToRange(std::ops::Range<usize>),
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
    /// Ouvert par un Ctrl+clic : le choix mène au jeton de la flèche, pas à la ligne.
    pub precise: bool,
}

/// Lignes parcourues après celle de l'élément pour retrouver un texte : le corps d'une note
/// suit sa déclaration, sans que le moteur note sa ligne.
const TEXT_SEARCH_LINES: usize = 60;

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
    /// Flèches sous le pointeur, surlignées sur tout leur tracé.
    pub hovered_links: Vec<usize>,
    pub link_menu: Option<LinkMenu>,
    service: Option<Arc<RenderService>>,
    render_task: Option<Task<()>>,
    /// Durée du dernier rendu, de la préparation du texte à la lecture du SVG.
    last_render: Duration,
    last_edit: Option<Instant>,
    /// Rendu le plus récent demandé, et le tour de calcul de ce document : un rendu en attente
    /// de son tour et devenu périmé n'est pas calculé, les frappes ne s'accumulent pas en file.
    render_generation: Arc<AtomicU64>,
    render_turn: Arc<Mutex<()>>,
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
                let since_last_edit = this.last_edit.replace(Instant::now()).map(|at| at.elapsed());
                this.schedule_render(render_delay(this.last_render, since_last_edit), cx);
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
            hovered_links: Vec::new(),
            link_menu: None,
            service,
            render_task: None,
            last_render: Duration::ZERO,
            last_edit: None,
            render_generation: Arc::default(),
            render_turn: Arc::default(),
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
        let generation = self.render_generation.fetch_add(1, Ordering::Relaxed) + 1;
        let latest = self.render_generation.clone();
        let turn = self.render_turn.clone();
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
                    let _turn = turn.lock().unwrap_or_else(|e| e.into_inner());
                    if latest.load(Ordering::Relaxed) != generation {
                        return None;
                    }
                    let start = Instant::now();
                    // le profil s'applique ici : les deux moteurs reçoivent le même texte, sans lire de fichier
                    let folder = (profile == SecurityProfile::Folder).then(|| path.as_deref().and_then(DocumentFolder::new)).flatten();
                    let prepared = match &folder {
                        Some((source, root)) => preflight::prepare(&text, profile, Some(root), source),
                        None => preflight::prepare(&text, profile, None, &NoFiles),
                    };
                    let result = service.render(&prepared.text, block).and_then(|mut output| {
                        prepared.apply(&mut output);
                        let scene = Scene::parse(&output.svg)?;
                        Ok((output, scene))
                    });
                    Some((result, start.elapsed()))
                })
                .await;
            let Some((result, elapsed)) = result else { return };
            this.update(cx, |this, cx| {
                this.last_render = elapsed;
                this.apply_render(result, version, cx)
            })
            .ok();
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
        let new_zoom = snap_zoom(p.zoom * factor);
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
        p.zoom = snap_zoom(((p.viewport_w - pad) / r.scene.width).min((p.viewport_h - pad) / r.scene.height));
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
        r.output.hit_at(sx, sy, LINK_TOLERANCE / self.preview.zoom)
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
        self.click(x, y, false, None, cx)
    }

    /// Ctrl+clic : l'endroit exact. `text` : le texte affiché sous le pointeur, avec l'indice
    /// (en octets) du caractère visé.
    pub fn click_precise(&mut self, x: f32, y: f32, text: Option<(&str, usize)>, cx: &mut Context<Self>) -> bool {
        self.click(x, y, true, text, cx)
    }

    /// Texte affiché dans la ligne de l'élément ou les suivantes : le texte entier, puis le mot visé.
    fn locate_shown(&self, line: usize, shown: &str, index: usize, cx: &Context<Self>) -> Option<usize> {
        let buffer = &self.editor.read(cx).buffer;
        let first = line.saturating_sub(1);
        let lines = first..(first + TEXT_SEARCH_LINES).min(buffer.line_count());
        let search = |find: fn(&str, &str, usize) -> Option<usize>| {
            lines.clone().find_map(|l| find(buffer.line_text(l), shown, index).map(|col| buffer.line_start(l) + col))
        };
        search(source_location::locate_text).or_else(|| search(source_location::locate_word))
    }

    /// Le jeton de la flèche de cette ligne ; sa ligne seule si elle n'en a pas (activité, carte).
    fn jump_to_arrow(&self, line: usize, cx: &mut Context<Self>) {
        let target = {
            let buffer = &self.editor.read(cx).buffer;
            let start = buffer.line_start(line.saturating_sub(1));
            source_location::arrow_token(buffer.line_text(line.saturating_sub(1))).map(|r| start + r.start..start + r.end)
        };
        cx.emit(target.map_or(DocumentEvent::JumpToLine(line), DocumentEvent::JumpToRange));
    }

    fn click(&mut self, x: f32, y: f32, precise: bool, text: Option<(&str, usize)>, cx: &mut Context<Self>) -> bool {
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
                    self.link_menu = Some(LinkMenu { x, y, choices, precise });
                    cx.notify();
                    return true;
                }
                if precise {
                    self.jump_to_arrow(lines[0].1, cx);
                    return true;
                }
                lines[0].1
            }
        };
        if line == 0 {
            return true;
        }
        let exact = text.filter(|_| precise).and_then(|(shown, index)| self.locate_shown(line, shown, index, cx));
        match exact {
            Some(offset) => cx.emit(DocumentEvent::JumpToRange(offset..offset)),
            None => cx.emit(DocumentEvent::JumpToLine(line)),
        }
        true
    }

    /// Entrée du menu : aller à cette flèche.
    pub fn choose_link(&mut self, line: usize, cx: &mut Context<Self>) {
        let precise = self.link_menu.as_ref().is_some_and(|m| m.precise);
        self.dismiss_link_menu(cx);
        if precise {
            self.jump_to_arrow(line, cx);
        } else {
            cx.emit(DocumentEvent::JumpToLine(line));
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zooming_back_and_forth_only_ever_uses_a_few_values() {
        for start in [1.0_f32, 0.2, 3.0] {
            for factor in [1.1_f32, 1.25, 1.0 / 1.1] {
                let mut zoom = start;
                let mut seen = std::collections::HashSet::new();
                let mut after_two_cycles = 0;
                for cycle in 0..40 {
                    for _ in 0..12 {
                        zoom = snap_zoom(zoom * factor);
                        seen.insert(zoom.to_bits());
                    }
                    for _ in 0..12 {
                        zoom = snap_zoom(zoom / factor);
                        seen.insert(zoom.to_bits());
                    }
                    if cycle == 1 {
                        after_two_cycles = seen.len();
                    }
                }
                assert_eq!(seen.len(), after_two_cycles, "départ {start}, facteur {factor}");
                assert!(seen.len() <= 3 * 13, "{} valeurs", seen.len());
            }
        }
        assert_eq!(snap_zoom(1.0), 1.0);
        assert_eq!(snap_zoom(snap_zoom(1.1) / 1.1), 1.0, "aller-retour exact depuis 100 %");
        assert_eq!(snap_zoom(100.0), MAX_ZOOM);
        assert_eq!(snap_zoom(0.0), MIN_ZOOM);
    }

    #[test]
    fn the_render_delay_follows_the_last_render() {
        let soon = Some(Duration::from_millis(1));
        assert_eq!(render_delay(Duration::ZERO, soon), MIN_RENDER_DELAY);
        assert_eq!(render_delay(Duration::from_millis(40), soon), Duration::from_millis(80));
        assert_eq!(render_delay(Duration::from_secs(3), soon), MAX_RENDER_DELAY);
        // frappe isolée : tout de suite, même sur un gros document
        assert_eq!(render_delay(Duration::from_secs(3), None), Duration::ZERO);
        assert_eq!(render_delay(Duration::from_secs(3), Some(MAX_RENDER_DELAY)), Duration::ZERO);
        assert_eq!(render_delay(Duration::from_millis(40), Some(Duration::from_millis(79))), Duration::from_millis(80));
    }
}
