//! Vue de l'aperçu : dessine la scène vectorielle du document avec les
//! primitives GPUI (quads, chemins, texte), sans bitmap intermédiaire.
//!
//! - Zoom et défilement ne sont qu'une transformation : rien n'est recalculé.
//! - Seuls les éléments visibles sont dessinés (élagage par boîte englobante,
//!   comme la scène de Zed).
//! - Les rectangles et cercles sans pointillés passent par des quads GPU
//!   (moins coûteux que des chemins tessellés).

use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, Corners, Edges, ElementId, Entity, FocusHandle,
    Focusable, FontStyle, FontWeight, GlobalElementId, Hsla, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, ScrollWheelEvent,
    SharedString, Style, Subscription, TextRun, UnderlineStyle, Window, anchored, deferred, div,
    fill, font, point, prelude::*, px, quad, relative, size,
};

use std::ops::Range;

use super::components::{button, hint};
use super::theme;
use crate::model::svg_scene::{self, Anchor, Color, Geometry, Item, PathCmd, Scene, Style as Paint, Text};
use crate::viewmodel::document_vm::{DocumentViewModel, LinkMenu};

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

impl PreviewView {
    fn on_key_down(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "escape" {
            self.document.update(cx, |d, cx| d.dismiss_link_menu(cx));
        }
    }

    /// Plusieurs flèches sous le clic : l'utilisateur confirme laquelle il visait.
    fn link_menu(&self, menu: &LinkMenu) -> AnyElement {
        let origin = self.bounds.map(|b| b.origin).unwrap_or_default();
        let position = point(origin.x + px(menu.x), origin.y + px(menu.y));
        let rows = menu.choices.iter().map(|choice| {
            let (link, line) = (choice.link, choice.line);
            let (over, click) = (self.document.clone(), self.document.clone());
            button(format!("link-choice-{line}"), format!("ligne {line}  {}", choice.label))
                .on_hover(move |hovered, _, cx: &mut App| {
                    over.update(cx, |d, cx| d.highlight_link(hovered.then_some(link), cx));
                })
                .on_click(move |_, _, cx: &mut App| {
                    click.update(cx, |d, cx| d.choose_link(line, cx));
                })
        });
        deferred(
            anchored().position(position).snap_to_window_with_margin(px(8.)).child(
                div()
                    .id("link-menu")
                    .occlude()
                    .min_w(px(220.))
                    .max_w(px(560.))
                    .p_1()
                    .bg(theme::surface())
                    .border_1()
                    .border_color(theme::field_border())
                    .rounded_md()
                    .shadow_lg()
                    .font_family(theme::UI_FONT)
                    .child(div().px_2().py_1().child(hint("Plusieurs flèches ici :")))
                    .children(rows),
            ),
        )
        .with_priority(1)
        .into_any_element()
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
        // un rendu précédent reste affiché, mais l'échec du rendu courant est signalé
        let stale_error = doc.rendering.is_some().then(|| doc.render_error.clone()).flatten();
        let hovering = doc.preview.hovered_region.is_some() || !doc.hovered_links.is_empty();
        let menu = doc.link_menu.clone();
        let root = div()
            .debug_selector(|| "preview".to_string())
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
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .child(SceneElement { view: cx.entity() })
            .when_some(stale_error, |s, e| {
                s.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .px_3()
                        .py_1()
                        .bg(theme::error_banner())
                        .text_size(px(12.5))
                        .text_color(theme::severity(crate::model::diagram::Severity::Error))
                        .child(format!("Rendu impossible (aperçu précédent affiché) : {e}")),
                )
            })
            .children(menu.map(|m| self.link_menu(&m)))
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
    /// Ligne de base d'un texte, sur un pixel entier de l'écran. Sinon GPUI l'arrondit
    /// lui-même vers le haut, et le texte n'est plus là où la boîte de survol le croit.
    fn baseline(&self, y: f32, scale: f32) -> Pixels {
        px((f32::from(self.point(0.0, y).y) * scale).round() / scale)
    }
}

fn hsla(c: Color) -> Hsla {
    theme::diagram(gpui::rgba(c.0).into())
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
        let hovered_links = doc.hovered_links.clone();
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
            window.paint_quad(fill(Bounds::new(sheet.origin + point(px(0.), px(2.)), sheet.size), theme::sheet_shadow()));
            window.paint_quad(fill(sheet, theme::sheet()));
            // formes des flèches survolées ; une flèche sans formes connues garde un simple trait
            let groups: Vec<&Range<usize>> = hovered_links
                .iter()
                .filter_map(|i| output.links.get(*i))
                .filter_map(|l| scene.link_items.get(&l.group))
                .collect();
            paint_scene(&scene, &visible, &groups, t, window, cx);
            if let Some((index, r)) = p.hovered_region.and_then(|i| output.regions.get(i).map(|r| (i, r))) {
                // la zone de clic peut être large ; la boîte, elle, épouse ce qui se voit
                let zone = svg_scene::Bounds { x: r.x, y: r.y, w: r.w, h: r.h };
                let scale = window.scale_factor();
                let content = scene
                    .content_bounds(zone, &mut |text| measured_width(text, t, window), &mut |y| f32::from(t.baseline(y, scale) - t.origin.y) / t.zoom, t.zoom * scale)
                    .unwrap_or(zone);
                let limit = output
                    .parent_of(index)
                    .map(|o| svg_scene::Bounds { x: o.x, y: o.y, w: o.w, h: o.h })
                    .unwrap_or(svg_scene::Bounds { x: 0.0, y: 0.0, w: scene.width, h: scene.height });
                let area = svg_scene::selection_box(content, SELECTION_PADDING / p.zoom, limit);
                // bords sur des pixels entiers : le trait reste net, et l'écart égal à l'œil
                let snap = |v: Pixels| px((f32::from(v) * scale).round() / scale);
                let (top_left, bottom_right) = (t.point(area.x, area.y), t.point(area.x + area.w, area.y + area.h));
                let b = Bounds::from_corners(
                    point(snap(top_left.x), snap(top_left.y)),
                    point(snap(bottom_right.x), snap(bottom_right.y)),
                );
                window.paint_quad(
                    quad(b, Corners::all(px(3.)), theme::region_hover(), Edges::all(px(1.)), theme::accent(), Default::default()),
                );
            }
            for link in hovered_links.iter().filter_map(|i| output.links.get(*i)).filter(|l| !scene.link_items.contains_key(&l.group)) {
                let mut builder = PathBuilder::stroke(px(3.0));
                for (k, (x, y)) in link.points.iter().enumerate() {
                    if k == 0 { builder.move_to(t.point(*x, *y)) } else { builder.line_to(t.point(*x, *y)) }
                }
                if let Ok(path) = builder.build() {
                    let color: Hsla = theme::accent().into();
                    window.paint_path(path, color);
                }
            }
        });
    }
}

/// Colorie une forme de flèche survolée, à sa place dans le dessin : les pointillés et la
/// courbe restent, et ce qui passe au-dessus (boîtes, libellés) reste au-dessus. Une pointe
/// pleine (remplie de sa propre couleur de trait) devient pleine d'accent ; une pointe
/// creuse garde son fond, qui continue de masquer le trait qui arrive.
fn highlight_colors(style: &Paint, accent: Hsla) -> (Option<Hsla>, Option<Hsla>) {
    let solid = style.fill.is_some() && style.fill == style.stroke;
    let fill = if solid { Some(accent) } else { style.fill.map(hsla) };
    (fill, style.stroke.map(|_| accent))
}

/// Le trait survolé s'épaissit un peu : le bleu seul se lirait mal sur un trait fin.
const HIGHLIGHT_EXTRA_WIDTH: f32 = 0.8;

fn paint_scene(scene: &Scene, visible: &crate::model::svg_scene::Bounds, highlighted: &[&Range<usize>], t: Transform, window: &mut Window, cx: &mut App) {
    let accent: Hsla = theme::accent().into();
    for (index, item) in scene.items.iter().enumerate() {
        match item {
            Item::Shape { bounds, .. } | Item::Text { bounds, .. } if !bounds.intersects(visible) => {}
            Item::Shape { geometry, style, .. } if highlighted.iter().any(|r| r.contains(&index)) => {
                let (fill, stroke) = highlight_colors(style, accent);
                paint_colored(geometry, style.stroke_width + HIGHLIGHT_EXTRA_WIDTH, &style.dash, fill, stroke, t, window);
            }
            Item::Shape { geometry, style, .. } => paint_shape(geometry, style, t, window),
            Item::Text { text, .. } => paint_text(text, t, window, cx),
        }
    }
}

fn paint_shape(geometry: &Geometry, style: &Paint, t: Transform, window: &mut Window) {
    paint_colored(geometry, style.stroke_width, &style.dash, style.fill.map(hsla), style.stroke.map(hsla), t, window);
}

fn paint_colored(geometry: &Geometry, stroke_width: f32, dash: &[f32], fill_color: Option<Hsla>, stroke: Option<Hsla>, t: Transform, window: &mut Window) {
    let stroke_w = stroke_width * t.zoom;
    // chemin rapide : quads (rectangles et cercles sans pointillés)
    let as_quad = match *geometry {
        Geometry::Rect { x, y, w, h, radius } if dash.is_empty() => Some((x, y, w, h, radius)),
        Geometry::Ellipse { cx: ex, cy: ey, rx, ry } if rx == ry && dash.is_empty() => Some((ex - rx, ey - ry, 2.0 * rx, 2.0 * ry, rx)),
        _ => None,
    };
    if let Some((x, y, w, h, radius)) = as_quad {
        // le trait SVG est centré sur le contour ; celui d'un quad est intérieur
        let half = if stroke.is_some() { stroke_width / 2.0 } else { 0.0 };
        let b = Bounds::new(t.point(x - half, y - half), size(t.len(w + 2.0 * half), t.len(h + 2.0 * half)));
        let bg: Hsla = fill_color.unwrap_or(gpui::transparent_black());
        let (border_w, border_c) = match stroke {
            Some(c) => (px(stroke_w), c),
            None => (px(0.), gpui::transparent_black()),
        };
        window.paint_quad(quad(b, Corners::all(t.len(radius + half)), bg, Edges::all(border_w), border_c, Default::default()));
        return;
    }
    let cmds = geometry_commands(geometry);
    if let Some(c) = fill_color {
        let mut builder = PathBuilder::fill();
        append_commands(&mut builder, &cmds, t);
        if let Ok(path) = builder.build() {
            window.paint_path(path, c);
        }
    }
    if let Some(c) = stroke {
        let mut builder = PathBuilder::stroke(px(stroke_w.max(0.5)));
        if !dash.is_empty() {
            let dash: Vec<Pixels> = dash.iter().map(|d| t.len(*d)).collect();
            builder = builder.dash_array(&dash);
        }
        append_commands(&mut builder, &cmds, t);
        if let Ok(path) = builder.build() {
            window.paint_path(path, c);
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

/// Écart entre un élément survolé et sa boîte, en pixels à l'écran : le même à tout zoom.
const SELECTION_PADDING: f32 = 3.0;

/// En dessous, le texte est illisible : inutile de le mettre en forme.
const MIN_TEXT_PIXELS: f32 = 2.5;

/// Largeur d'un texte telle qu'elle sera peinte, en unités du SVG.
fn measured_width(text: &Text, t: Transform, window: &mut Window) -> f32 {
    if text.size * t.zoom < MIN_TEXT_PIXELS {
        return text.content.chars().count() as f32 * text.size * 0.6;
    }
    f32::from(shape_text(text, t, window).width) / t.zoom
}

fn paint_text(text: &Text, t: Transform, window: &mut Window, cx: &mut App) {
    if text.size * t.zoom < MIN_TEXT_PIXELS {
        return;
    }
    let shaped = shape_text(text, t, window);
    let x = t.point(text.x, text.y).x
        - match text.anchor {
            Anchor::Start => px(0.),
            Anchor::Middle => shaped.width / 2.0,
            Anchor::End => shaped.width,
        };
    // y SVG = ligne de base ; GPUI peint depuis le haut de la boîte de ligne. La petite marge
    // évite qu'une erreur d'arrondi fasse retomber le texte un pixel plus haut.
    let line_height = shaped.ascent + shaped.descent;
    let top = t.baseline(text.y, window.scale_factor()) - shaped.ascent + px(0.01);
    let _ = shaped.paint(point(x, top), line_height, window, cx);
}

fn shape_text(text: &Text, t: Transform, window: &mut Window) -> gpui::ShapedLine {
    let size_px = text.size * t.zoom;
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
    window.text_system().shape_line(SharedString::from(text.content.clone()), px(size_px), &[run], None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(fill: Option<u32>, stroke: Option<u32>) -> Paint {
        Paint { fill: fill.map(Color), stroke: stroke.map(Color), stroke_width: 1.2, dash: vec![6.0, 4.0] }
    }

    #[test]
    fn a_hollow_head_stays_hollow_and_a_solid_one_turns_accent() {
        let accent: Hsla = gpui::rgb(0x3a6ea5).into();
        let line = highlight_colors(&style(None, Some(0x3b4252ff)), accent);
        assert_eq!(line, (None, Some(accent)), "le trait seul");
        let hollow = highlight_colors(&style(Some(0xffffffff), Some(0x3b4252ff)), accent);
        assert_eq!(hollow.1, Some(accent), "le contour change");
        assert_ne!(hollow.0, Some(accent), "le fond reste : il masque le trait qui arrive");
        assert!(hollow.0.is_some());
        let solid = highlight_colors(&style(Some(0x3b4252ff), Some(0x3b4252ff)), accent);
        assert_eq!(solid, (Some(accent), Some(accent)));
    }
}
