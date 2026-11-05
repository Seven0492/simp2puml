//! Vue de l'aperçu : dessine la scène vectorielle du document avec les
//! primitives GPUI (quads, chemins, texte), sans bitmap intermédiaire.
//!
//! - Zoom et défilement ne sont qu'une transformation : rien n'est recalculé.
//! - Seuls les éléments visibles sont dessinés (élagage par boîte englobante,
//!   comme la scène de Zed).
//! - Les rectangles et cercles sans pointillés passent par des quads GPU
//!   (moins coûteux que des chemins tessellés).

use gpui::{
    App, Bounds, ContentMask, Context, Corners, Edges, ElementId, Entity, FocusHandle, Focusable,
    FontStyle, FontWeight, GlobalElementId, Hsla, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, ScrollWheelEvent, SharedString,
    Style, Subscription, TextRun, UnderlineStyle, Window, div, fill, font, point, prelude::*, px,
    quad, relative, size,
};

use super::theme;
use crate::model::svg_scene::{Anchor, Color, Geometry, Item, PathCmd, Scene, Style as Paint, Text};
use crate::viewmodel::document_vm::DocumentViewModel;

pub struct PreviewView {
    document: Entity<DocumentViewModel>,
    focus_handle: FocusHandle,
    bounds: Option<Bounds<Pixels>>,
    /// Dernière position pendant un glisser pour faire défiler.
    drag_from: Option<Point<Pixels>>,
    _subscription: Subscription,
}

impl Focusable for PreviewView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PreviewView {
    pub fn new(document: Entity<DocumentViewModel>, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&document, |_, _, cx| cx.notify());
        Self { document, focus_handle: cx.focus_handle(), bounds: None, drag_from: None, _subscription: sub }
    }

    fn local(&self, p: Point<Pixels>) -> (f32, f32) {
        let o = self.bounds.map(|b| b.origin).unwrap_or_default();
        (f32::from(p.x - o.x), f32::from(p.y - o.y))
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(20.));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        if ev.modifiers.secondary() {
            // Ctrl + molette : zoom autour du pointeur
            let (x, y) = self.local(ev.position);
            let factor = if dy > 0.0 { 1.1 } else { 1.0 / 1.1 };
            self.document.update(cx, |d, cx| d.zoom_at(factor, x, y, cx));
        } else {
            let (dx, dy) = if ev.modifiers.shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
            self.document.update(cx, |d, cx| d.scroll_preview(dx, dy, cx));
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let (x, y) = self.local(ev.position);
        let on_element = self.document.update(cx, |d, cx| d.click_preview(x, y, cx));
        if !on_element {
            self.drag_from = Some(ev.position);
        }
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(from) = self.drag_from {
            if ev.pressed_button == Some(MouseButton::Left) {
                let (dx, dy) = (f32::from(ev.position.x - from.x), f32::from(ev.position.y - from.y));
                self.drag_from = Some(ev.position);
                self.document.update(cx, |d, cx| d.scroll_preview(dx, dy, cx));
                return;
            }
            self.drag_from = None;
        }
        let (x, y) = self.local(ev.position);
        self.document.update(cx, |d, cx| d.hover(x, y, cx));
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.drag_from = None;
    }
}

impl Render for PreviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = self.document.read(cx);
        let message: Option<(String, bool)> = if !doc.has_renderer() {
            Some(("Aperçu indisponible : moteur « pumlrender » introuvable. Construisez-le dans renderer/ ou définissez PUMLRENDER.".into(), false))
        } else if doc.rendering.is_none() {
            Some(match &doc.render_error {
                Some(e) => (e.clone(), true),
                None => ("Rendu en cours…".into(), false),
            })
        } else {
            None
        };
        let hovering = doc.preview.hovered_region.is_some();
        let root = div()
            .id("preview")
            .track_focus(&self.focus_handle)
            .flex_1()
            .h_full()
            .bg(theme::canvas())
            .overflow_hidden();
        if let Some((text, is_error)) = message {
            return root
                .p_4()
                .child(div().text_size(px(13.)).text_color(if is_error { theme::severity(crate::model::diagram::Severity::Error) } else { theme::muted() }).child(text))
                .into_any_element();
        }
        root.cursor(if hovering { gpui::CursorStyle::PointingHand } else { gpui::CursorStyle::Arrow })
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(SceneElement { view: cx.entity() })
            .into_any_element()
    }
}

// ---------------------------------------------------------------- élément de dessin

struct SceneElement {
    view: Entity<PreviewView>,
}

impl IntoElement for SceneElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// Transformation coordonnées SVG → écran.
#[derive(Clone, Copy)]
struct Transform {
    origin: Point<Pixels>,
    zoom: f32,
}

impl Transform {
    fn point(&self, x: f32, y: f32) -> Point<Pixels> {
        point(self.origin.x + px(x * self.zoom), self.origin.y + px(y * self.zoom))
    }
    fn len(&self, v: f32) -> Pixels {
        px(v * self.zoom)
    }
}

fn hsla(c: Color) -> Hsla {
    gpui::rgba(c.0).into()
}

impl Element for SceneElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), _: &mut Window, cx: &mut App) {
        let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
        let doc = self.view.update(cx, |view, _| {
            view.bounds = Some(bounds);
            view.document.clone()
        });
        doc.update(cx, |d, _| d.set_viewport(w, h));
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), _: &mut (), window: &mut Window, cx: &mut App) {
        let doc = self.view.read(cx).document.read(cx);
        let Some(rendering) = doc.rendering.as_ref() else { return };
        let scene = rendering.scene.clone();
        let output = rendering.output.clone();
        let p = doc.preview;
        let (ox, oy) = doc.scene_origin();
        let t = Transform {
            origin: point(bounds.left() + px(ox - p.scroll_x), bounds.top() + px(oy - p.scroll_y)),
            zoom: p.zoom,
        };
        // zone visible en coordonnées de la scène (élagage)
        let visible = crate::model::svg_scene::Bounds {
            x: (p.scroll_x - ox) / p.zoom,
            y: (p.scroll_y - oy) / p.zoom,
            w: f32::from(bounds.size.width) / p.zoom,
            h: f32::from(bounds.size.height) / p.zoom,
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // feuille : ombre légère et fond
            let sheet = Bounds::new(t.point(0.0, 0.0), size(t.len(scene.width), t.len(scene.height)));
            window.paint_quad(fill(Bounds::new(sheet.origin + point(px(0.), px(2.)), sheet.size), gpui::rgba(0x0000000d)));
            window.paint_quad(fill(sheet, theme::surface()));
            paint_scene(&scene, &visible, t, window, cx);
            if let Some(r) = p.hovered_region.and_then(|i| output.regions.get(i)) {
                let b = Bounds::new(t.point(r.x - 2.0, r.y - 2.0), size(t.len(r.w + 4.0), t.len(r.h + 4.0)));
                window.paint_quad(
                    quad(b, Corners::all(px(3.)), theme::region_hover(), Edges::all(px(1.)), theme::accent(), Default::default()),
                );
            }
        });
    }
}

fn paint_scene(scene: &Scene, visible: &crate::model::svg_scene::Bounds, t: Transform, window: &mut Window, cx: &mut App) {
    for item in &scene.items {
        match item {
            Item::Shape { bounds, .. } | Item::Text { bounds, .. } if !bounds.intersects(visible) => {}
            Item::Shape { geometry, style, .. } => paint_shape(geometry, style, t, window),
            Item::Text { text, .. } => paint_text(text, t, window, cx),
        }
    }
}

fn paint_shape(geometry: &Geometry, style: &Paint, t: Transform, window: &mut Window) {
    let stroke_w = style.stroke_width * t.zoom;
    // chemin rapide : quads (rectangles et cercles sans pointillés)
    let as_quad = match *geometry {
        Geometry::Rect { x, y, w, h, radius } if style.dash.is_empty() => Some((x, y, w, h, radius)),
        Geometry::Ellipse { cx: ex, cy: ey, rx, ry } if rx == ry && style.dash.is_empty() => Some((ex - rx, ey - ry, 2.0 * rx, 2.0 * ry, rx)),
        _ => None,
    };
    if let Some((x, y, w, h, radius)) = as_quad {
        // le trait SVG est centré sur le contour ; celui d'un quad est intérieur
        let half = if style.stroke.is_some() { style.stroke_width / 2.0 } else { 0.0 };
        let b = Bounds::new(t.point(x - half, y - half), size(t.len(w + 2.0 * half), t.len(h + 2.0 * half)));
        let bg: Hsla = style.fill.map(hsla).unwrap_or(gpui::transparent_black());
        let (border_w, border_c) = match style.stroke {
            Some(c) => (px(stroke_w), hsla(c)),
            None => (px(0.), gpui::transparent_black()),
        };
        window.paint_quad(quad(b, Corners::all(t.len(radius + half)), bg, Edges::all(border_w), border_c, Default::default()));
        return;
    }
    let cmds = geometry_commands(geometry);
    if let Some(c) = style.fill {
        let mut builder = PathBuilder::fill();
        append_commands(&mut builder, &cmds, t);
        if let Ok(path) = builder.build() {
            window.paint_path(path, hsla(c));
        }
    }
    if let Some(c) = style.stroke {
        let mut builder = PathBuilder::stroke(px(stroke_w.max(0.5)));
        if !style.dash.is_empty() {
            let dash: Vec<Pixels> = style.dash.iter().map(|d| t.len(*d)).collect();
            builder = builder.dash_array(&dash);
        }
        append_commands(&mut builder, &cmds, t);
        if let Ok(path) = builder.build() {
            window.paint_path(path, hsla(c));
        }
    }
}

/// Toute géométrie exprimée en commandes de chemin.
fn geometry_commands(g: &Geometry) -> Vec<PathCmd> {
    use crate::model::svg_scene::Point as P;
    match *g {
        Geometry::Path(ref cmds) => cmds.clone(),
        Geometry::Ellipse { cx, cy, rx, ry } => vec![
            PathCmd::Move(P { x: cx + rx, y: cy }),
            PathCmd::Arc { rx, ry, rotation: 0.0, large: false, sweep: true, to: P { x: cx - rx, y: cy } },
            PathCmd::Arc { rx, ry, rotation: 0.0, large: false, sweep: true, to: P { x: cx + rx, y: cy } },
            PathCmd::Close,
        ],
        Geometry::Rect { x, y, w, h, radius } => {
            let r = radius.min(w / 2.0).min(h / 2.0);
            if r <= 0.0 {
                return vec![
                    PathCmd::Move(P { x, y }),
                    PathCmd::Line(P { x: x + w, y }),
                    PathCmd::Line(P { x: x + w, y: y + h }),
                    PathCmd::Line(P { x, y: y + h }),
                    PathCmd::Close,
                ];
            }
            let arc = |to: P| PathCmd::Arc { rx: r, ry: r, rotation: 0.0, large: false, sweep: true, to };
            vec![
                PathCmd::Move(P { x: x + r, y }),
                PathCmd::Line(P { x: x + w - r, y }),
                arc(P { x: x + w, y: y + r }),
                PathCmd::Line(P { x: x + w, y: y + h - r }),
                arc(P { x: x + w - r, y: y + h }),
                PathCmd::Line(P { x: x + r, y: y + h }),
                arc(P { x, y: y + h - r }),
                PathCmd::Line(P { x, y: y + r }),
                arc(P { x: x + r, y }),
                PathCmd::Close,
            ]
        }
    }
}

fn append_commands(b: &mut PathBuilder, cmds: &[PathCmd], t: Transform) {
    for c in cmds {
        match *c {
            PathCmd::Move(p) => b.move_to(t.point(p.x, p.y)),
            PathCmd::Line(p) => b.line_to(t.point(p.x, p.y)),
            PathCmd::Quad { ctrl, to } => b.curve_to(t.point(to.x, to.y), t.point(ctrl.x, ctrl.y)),
            PathCmd::Cubic { c1, c2, to } => b.cubic_bezier_to(t.point(to.x, to.y), t.point(c1.x, c1.y), t.point(c2.x, c2.y)),
            PathCmd::Arc { rx, ry, rotation, large, sweep, to } => {
                b.arc_to(point(t.len(rx), t.len(ry)), px(rotation), large, sweep, t.point(to.x, to.y))
            }
            PathCmd::Close => b.close(),
        }
    }
}

fn paint_text(text: &Text, t: Transform, window: &mut Window, cx: &mut App) {
    let size_px = text.size * t.zoom;
    if size_px < 2.5 {
        return; // illisible : inutile de mettre en forme
    }
    let mut f = font(if text.monospace { theme::DIAGRAM_MONO_FONT } else { theme::DIAGRAM_FONT });
    if text.bold {
        f.weight = FontWeight::BOLD;
    }
    if text.italic {
        f.style = FontStyle::Italic;
    }
    let color = hsla(text.color);
    let run = TextRun {
        len: text.content.len(),
        font: f,
        color,
        background_color: None,
        underline: text.underline.then(|| UnderlineStyle { thickness: px(1.0), color: Some(color), wavy: false }),
        strikethrough: None,
    };
    let shaped = window.text_system().shape_line(SharedString::from(text.content.clone()), px(size_px), &[run], None);
    let x = t.point(text.x, text.y).x
        - match text.anchor {
            Anchor::Start => px(0.),
            Anchor::Middle => shaped.width / 2.0,
            Anchor::End => shaped.width,
        };
    // y SVG = ligne de base ; GPUI peint depuis le haut de la boîte de ligne
    let line_height = shaped.ascent + shaped.descent;
    let top = t.point(text.x, text.y).y - shaped.ascent;
    let _ = shaped.paint(point(x, top), line_height, window, cx);
}
