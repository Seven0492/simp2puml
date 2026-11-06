//! Modèle : scène vectorielle construite à partir du SVG produit par
//! `pumlrender`.
//!
//! L'aperçu ne rastérise pas le SVG : il le lit une fois (en arrière-plan),
//! le transforme en primitives (rectangles, ellipses, chemins, textes) et la
//! vue les dessine directement avec GPUI. Le zoom n'est alors qu'une
//! transformation : rien n'est recalculé, et le tracé reste net à toute échelle.
//!
//! Seul le sous-ensemble SVG émis par `pumlrender` et par PlantUML est pris en
//! charge : `rect`, `line`, `circle`, `ellipse`, `polygon`, `path` (M L H V Q C A Z,
//! absolus ou relatifs), `text`, et `g` avec `translate(...)`. Les couleurs viennent
//! des attributs ou de `style="…"` ; un dégradé est réduit à sa première couleur.

use std::collections::HashMap;
use std::ops::Range;

use roxmltree::{Document, Node, ParsingOptions};

use crate::model::glyph_metrics;

/// Couleur RGBA (0xRRGGBBAA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub fn with_alpha(self, alpha: f32) -> Color {
        let a = ((self.0 & 0xff) as f32 * alpha.clamp(0.0, 1.0)).round() as u32;
        Color((self.0 & 0xffff_ff00) | a)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathCmd {
    Move(Point),
    Line(Point),
    Quad { ctrl: Point, to: Point },
    Cubic { c1: Point, c2: Point, to: Point },
    Arc { rx: f32, ry: f32, rotation: f32, large: bool, sweep: bool, to: Point },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    Rect { x: f32, y: f32, w: f32, h: f32, radius: f32 },
    Ellipse { cx: f32, cy: f32, rx: f32, ry: f32 },
    Path(Vec<PathCmd>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub dash: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    /// Position de la ligne de base.
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub size: f32,
    pub color: Color,
    pub anchor: Anchor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub monospace: bool,
}

/// Boîte englobante (coordonnées SVG), utilisée pour ne dessiner que le visible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Bounds {
    pub fn intersects(&self, o: &Bounds) -> bool {
        self.x <= o.x + o.w && o.x <= self.x + self.w && self.y <= o.y + o.h && o.y <= self.y + self.h
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Shape { geometry: Geometry, style: Style, bounds: Bounds },
    Text { text: Text, bounds: Bounds },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
    /// Formes de chaque flèche (trait et pointes), par numéro de groupe : le survol
    /// les recolore à leur place, en gardant pointillés, courbes et formes de pointe.
    pub link_items: HashMap<usize, Range<usize>>,
}

/// Marge de `geometry_bounds` au-delà du trait, retirée pour mesurer ce qui se voit.
const BOUNDS_MARGIN: f32 = 1.0;
/// Une forme sans contour qui couvre presque toute la zone est un fond, pas un contenu.
const BACKGROUND_SHARE: f32 = 0.6;

impl Bounds {
    pub fn contains(&self, o: &Bounds, margin: f32) -> bool {
        o.x >= self.x - margin && o.y >= self.y - margin && o.x + o.w <= self.x + self.w + margin && o.y + o.h <= self.y + self.h + margin
    }

    fn union(self, o: Bounds) -> Bounds {
        let (x, y) = (self.x.min(o.x), self.y.min(o.y));
        Bounds { x, y, w: (self.x + self.w).max(o.x + o.w) - x, h: (self.y + self.h).max(o.y + o.h) - y }
    }
}

/// Boîte de sélection : le contenu, avec le même écart de tous les côtés, sans sortir du
/// parent. Contre un bord du parent l'écart de ce côté se réduit ; les autres ne changent pas.
pub fn selection_box(content: Bounds, padding: f32, parent: Bounds) -> Bounds {
    let x0 = (content.x - padding).max(parent.x).min(content.x);
    let y0 = (content.y - padding).max(parent.y).min(content.y);
    let x1 = (content.x + content.w + padding).min(parent.x + parent.w).max(content.x + content.w);
    let y1 = (content.y + content.h + padding).min(parent.y + parent.h).max(content.y + content.h);
    Bounds { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

impl Scene {
    /// Ce qui se voit réellement dans une zone cliquable (souvent plus large que son contenu,
    /// pour que le clic soit facile). `text_width` : largeur mesurée avec la vraie police ;
    /// `baseline` : ordonnée où la ligne de base sera réellement peinte ;
    /// `pixels_per_unit` : pixels de l'écran par unité du SVG (le zoom, à l'échelle de l'écran).
    pub fn content_bounds(
        &self,
        zone: Bounds,
        text_width: &mut dyn FnMut(&Text) -> f32,
        baseline: &mut dyn FnMut(f32) -> f32,
        pixels_per_unit: f32,
    ) -> Option<Bounds> {
        let mut content: Option<Bounds> = None;
        let mut add = |b: Bounds| content = Some(content.map_or(b, |c: Bounds| c.union(b)));
        for item in &self.items {
            match item {
                Item::Shape { bounds, style, .. } => {
                    let visible = Bounds {
                        x: bounds.x + BOUNDS_MARGIN,
                        y: bounds.y + BOUNDS_MARGIN,
                        w: bounds.w - 2.0 * BOUNDS_MARGIN,
                        h: bounds.h - 2.0 * BOUNDS_MARGIN,
                    };
                    let background = style.stroke.is_none() && visible.w * visible.h >= BACKGROUND_SHARE * zone.w * zone.h;
                    if !background && zone.contains(&visible, 2.0) {
                        add(visible);
                    }
                }
                Item::Text { text, .. } => {
                    let width = text_width(text);
                    let x = match text.anchor {
                        Anchor::Start => text.x,
                        Anchor::Middle => text.x - width / 2.0,
                        Anchor::End => text.x - width,
                    };
                    let inside = x >= zone.x - 2.0 && x <= zone.x + zone.w && text.y >= zone.y && text.y <= zone.y + zone.h;
                    if inside {
                        let (top, bottom) = glyph_metrics::text_extent(text, pixels_per_unit);
                        add(Bounds { x, y: baseline(text.y) - top, w: width, h: top + bottom });
                    }
                }
            }
        }
        content
    }

    pub fn parse(svg: &str) -> Result<Scene, String> {
        let opts = ParsingOptions { allow_dtd: false, ..ParsingOptions::default() };
        let doc = Document::parse_with_options(svg, opts).map_err(|e| format!("SVG illisible : {e}"))?;
        let root = doc.root_element();
        if root.tag_name().name() != "svg" {
            return Err("racine SVG attendue".into());
        }
        let mut scene = Scene::default();
        let (vw, vh) = view_box(&root);
        scene.width = vw.or_else(|| num(&root, "width")).unwrap_or(0.0);
        scene.height = vh.or_else(|| num(&root, "height")).unwrap_or(0.0);
        // fond demandé par le thème : `style="background:#…"` sur la racine
        if let Some(color) = style_value(&root, "background").and_then(|v| paint(&root, v)) {
            let geometry = Geometry::Rect { x: 0.0, y: 0.0, w: scene.width, h: scene.height, radius: 0.0 };
            let bounds = geometry_bounds(&geometry, 0.0);
            let style = Style { fill: Some(color), stroke: None, stroke_width: 0.0, dash: Vec::new() };
            scene.items.push(Item::Shape { geometry, style, bounds });
        }
        let mut links = LinkGroups { items: &mut scene.link_items, next: 0 };
        walk(&root, 0.0, 0.0, &mut scene.items, &mut links);
        Ok(scene)
    }
}

// ------------------------------------------------------------------ lecture

fn view_box(node: &Node) -> (Option<f32>, Option<f32>) {
    let Some(vb) = node.attribute("viewBox") else { return (None, None) };
    let v: Vec<f32> = vb.split([' ', ',']).filter_map(|s| s.parse().ok()).collect();
    if v.len() == 4 {
        (Some(v[2]), Some(v[3]))
    } else {
        (None, None)
    }
}

fn num(node: &Node, name: &str) -> Option<f32> {
    node.attribute(name)?.trim().trim_end_matches("px").parse().ok()
}

fn num0(node: &Node, name: &str) -> f32 {
    num(node, name).unwrap_or(0.0)
}

fn translate(node: &Node) -> (f32, f32) {
    let Some(t) = node.attribute("transform") else { return (0.0, 0.0) };
    let Some(rest) = t.trim().strip_prefix("translate(") else { return (0.0, 0.0) };
    let inner = rest.trim_end_matches(')');
    let v: Vec<f32> = inner
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect();
    (v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
}

/// Valeur d'une propriété dans l'attribut `style="a:b;c:d"`.
fn style_value<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attribute("style")?.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        (k.trim() == name).then(|| v.trim())
    })
}

/// Attribut de présentation, ou la même propriété dans `style`, qui a priorité en SVG.
fn property<'a>(node: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    style_value(node, name).or_else(|| node.attribute(name))
}

/// Couleur, ou première couleur du dégradé désigné par `url(#id)`.
fn paint(node: &Node, value: &str) -> Option<Color> {
    let Some(id) = value.strip_prefix("url(#").and_then(|v| v.strip_suffix(')')) else {
        return parse_color(value);
    };
    let gradient = node.document().descendants().find(|n| n.attribute("id") == Some(id))?;
    let stop = gradient.children().find(|n| n.has_tag_name("stop") || n.tag_name().name() == "stop")?;
    parse_color(stop.attribute("stop-color")?)
}

fn prop_num(node: &Node, name: &str) -> Option<f32> {
    property(node, name)?.trim().trim_end_matches("px").parse().ok()
}

fn style(node: &Node) -> Style {
    let opacity = prop_num(node, "opacity").unwrap_or(1.0);
    let fill = match property(node, "fill") {
        None => Some(Color(0x000000ff)),
        Some(v) => paint(node, v),
    }
    .map(|c| c.with_alpha(prop_num(node, "fill-opacity").unwrap_or(1.0) * opacity));
    let stroke = property(node, "stroke")
        .and_then(|v| paint(node, v))
        .map(|c| c.with_alpha(prop_num(node, "stroke-opacity").unwrap_or(1.0) * opacity));
    let dash = property(node, "stroke-dasharray")
        .map(|d| d.split([' ', ',']).filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_default();
    Style { fill, stroke, stroke_width: prop_num(node, "stroke-width").unwrap_or(1.0), dash }
}

struct LinkGroups<'a> {
    items: &'a mut HashMap<usize, Range<usize>>,
    /// Rang du prochain `<g class="link">` (sortie de PlantUML).
    next: usize,
}

fn walk(node: &Node, ox: f32, oy: f32, out: &mut Vec<Item>, links: &mut LinkGroups) {
    for child in node.children().filter(|n| n.is_element()) {
        let tag = child.tag_name().name();
        let push = |geometry: Geometry, out: &mut Vec<Item>| {
            let st = style(&child);
            if st.fill.is_none() && st.stroke.is_none() {
                return;
            }
            let bounds = geometry_bounds(&geometry, st.stroke_width);
            out.push(Item::Shape { geometry, style: st, bounds });
        };
        match tag {
            "g" => {
                let (dx, dy) = translate(&child);
                // `data-link` : pumlrender ; `class="link"` : PlantUML, numérotés dans l'ordre
                let group = child.attribute("data-link").and_then(|v| v.parse().ok()).or_else(|| {
                    (child.attribute("class") == Some("link")).then(|| {
                        links.next += 1;
                        links.next - 1
                    })
                });
                let start = out.len();
                walk(&child, ox + dx, oy + dy, out, links);
                if let Some(group) = group {
                    links.items.insert(group, start..out.len());
                }
            }
            "rect" => push(
                Geometry::Rect {
                    x: ox + num0(&child, "x"),
                    y: oy + num0(&child, "y"),
                    w: num0(&child, "width"),
                    h: num0(&child, "height"),
                    radius: num(&child, "rx").or_else(|| num(&child, "ry")).unwrap_or(0.0),
                },
                out,
            ),
            "circle" => {
                let r = num0(&child, "r");
                push(Geometry::Ellipse { cx: ox + num0(&child, "cx"), cy: oy + num0(&child, "cy"), rx: r, ry: r }, out)
            }
            "ellipse" => push(
                Geometry::Ellipse {
                    cx: ox + num0(&child, "cx"),
                    cy: oy + num0(&child, "cy"),
                    rx: num0(&child, "rx"),
                    ry: num0(&child, "ry"),
                },
                out,
            ),
            "line" => {
                let p = |a: &str, b: &str| Point { x: ox + num0(&child, a), y: oy + num0(&child, b) };
                push(Geometry::Path(vec![PathCmd::Move(p("x1", "y1")), PathCmd::Line(p("x2", "y2"))]), out)
            }
            "polygon" | "polyline" => {
                let v: Vec<f32> = child
                    .attribute("points")
                    .unwrap_or("")
                    .split([' ', ','])
                    .filter_map(|s| s.parse().ok())
                    .collect();
                let mut cmds: Vec<PathCmd> = v
                    .chunks_exact(2)
                    .enumerate()
                    .map(|(i, c)| {
                        let p = Point { x: ox + c[0], y: oy + c[1] };
                        if i == 0 { PathCmd::Move(p) } else { PathCmd::Line(p) }
                    })
                    .collect();
                if tag == "polygon" && !cmds.is_empty() {
                    cmds.push(PathCmd::Close);
                }
                push(Geometry::Path(cmds), out)
            }
            "path" => {
                let cmds = parse_path(child.attribute("d").unwrap_or(""), ox, oy);
                if !cmds.is_empty() {
                    push(Geometry::Path(cmds), out)
                }
            }
            "text" => {
                let content: String = child.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
                if content.trim().is_empty() {
                    continue;
                }
                let size = num(&child, "font-size").unwrap_or(13.0);
                let anchor = match child.attribute("text-anchor") {
                    Some("middle") => Anchor::Middle,
                    Some("end") => Anchor::End,
                    _ => Anchor::Start,
                };
                let text = Text {
                    x: ox + num0(&child, "x"),
                    y: oy + num0(&child, "y"),
                    size,
                    color: child.attribute("fill").and_then(parse_color).unwrap_or(Color(0x000000ff)),
                    anchor,
                    bold: matches!(child.attribute("font-weight"), Some("bold") | Some("700")),
                    italic: child.attribute("font-style") == Some("italic"),
                    underline: child.attribute("text-decoration") == Some("underline"),
                    monospace: child.attribute("font-family").is_some_and(|f| f.contains("Mono")),
                    content,
                };
                let w = text.content.chars().count() as f32 * size * 0.62;
                let x0 = match anchor {
                    Anchor::Start => text.x,
                    Anchor::Middle => text.x - w / 2.0,
                    Anchor::End => text.x - w,
                };
                let bounds = Bounds { x: x0, y: text.y - size, w, h: size * 1.3 };
                out.push(Item::Text { text, bounds });
            }
            // defs, commentaires, éléments inconnus : ignorés
            _ => {}
        }
    }
}

fn geometry_bounds(g: &Geometry, stroke: f32) -> Bounds {
    let pad = stroke / 2.0 + 1.0;
    let (x0, y0, x1, y1) = match g {
        Geometry::Rect { x, y, w, h, .. } => (*x, *y, x + w, y + h),
        Geometry::Ellipse { cx, cy, rx, ry } => (cx - rx, cy - ry, cx + rx, cy + ry),
        Geometry::Path(cmds) => {
            let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            let mut add = |p: &Point| {
                b = (b.0.min(p.x), b.1.min(p.y), b.2.max(p.x), b.3.max(p.y));
            };
            for c in cmds {
                match c {
                    PathCmd::Move(p) | PathCmd::Line(p) => add(p),
                    PathCmd::Quad { ctrl, to } => {
                        add(ctrl);
                        add(to)
                    }
                    PathCmd::Cubic { c1, c2, to } => {
                        add(c1);
                        add(c2);
                        add(to)
                    }
                    // borne grossière : l'arc reste dans un rayon autour de son extrémité
                    PathCmd::Arc { rx, ry, to, .. } => {
                        let r = rx.max(*ry) * 2.0;
                        add(&Point { x: to.x - r, y: to.y - r });
                        add(&Point { x: to.x + r, y: to.y + r })
                    }
                    PathCmd::Close => {}
                }
            }
            b
        }
    };
    Bounds { x: x0 - pad, y: y0 - pad, w: x1 - x0 + 2.0 * pad, h: y1 - y0 + 2.0 * pad }
}

/// Lit l'attribut `d` d'un chemin ; les coordonnées sont rendues absolues.
pub fn parse_path(d: &str, ox: f32, oy: f32) -> Vec<PathCmd> {
    // découpage en commandes et nombres (gère « 10-5 », « .5.5 », « 1e-3 »)
    let mut tokens: Vec<Result<char, f32>> = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' {
            tokens.push(Ok(c));
            i += 1;
        } else if c == '-' || c == '+' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut seen_dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (ch == 'e' || ch == 'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse::<f32>() {
                tokens.push(Err(v));
            }
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    let mut cur = Point { x: 0.0, y: 0.0 };
    let mut start = cur;
    let mut cmd = 'M';
    let mut t = 0;
    let next = |t: &mut usize| -> Option<f32> {
        match tokens.get(*t) {
            Some(Err(v)) => {
                *t += 1;
                Some(*v)
            }
            _ => None,
        }
    };
    while t < tokens.len() {
        if let Ok(c) = tokens[t] {
            cmd = c;
            t += 1;
            if cmd == 'Z' || cmd == 'z' {
                out.push(PathCmd::Close);
                cur = start;
                continue;
            }
        }
        let rel = cmd.is_ascii_lowercase();
        // relatif : par rapport au point courant ; absolu : décalé par les groupes englobants
        let pt = |x: f32, y: f32| {
            if rel { Point { x: cur.x + x, y: cur.y + y } } else { Point { x: x + ox, y: y + oy } }
        };
        let before = t;
        match cmd.to_ascii_uppercase() {
            'M' | 'L' => {
                let (Some(x), Some(y)) = (next(&mut t), next(&mut t)) else { break };
                let p = pt(x, y);
                if cmd.eq_ignore_ascii_case(&'M') {
                    out.push(PathCmd::Move(p));
                    start = p;
                    // les paires suivantes d'un M sont des L
                    cmd = if rel { 'l' } else { 'L' };
                } else {
                    out.push(PathCmd::Line(p));
                }
                cur = p;
            }
            'H' => {
                let Some(x) = next(&mut t) else { break };
                cur = Point { x: if rel { cur.x + x } else { x + ox }, y: cur.y };
                out.push(PathCmd::Line(cur));
            }
            'V' => {
                let Some(y) = next(&mut t) else { break };
                cur = Point { x: cur.x, y: if rel { cur.y + y } else { y + oy } };
                out.push(PathCmd::Line(cur));
            }
            'Q' => {
                let v: Vec<f32> = (0..4).filter_map(|_| next(&mut t)).collect();
                if v.len() < 4 {
                    break;
                }
                let (ctrl, to) = (pt(v[0], v[1]), pt(v[2], v[3]));
                out.push(PathCmd::Quad { ctrl, to });
                cur = to;
            }
            'C' => {
                let v: Vec<f32> = (0..6).filter_map(|_| next(&mut t)).collect();
                if v.len() < 6 {
                    break;
                }
                let (c1, c2, to) = (pt(v[0], v[1]), pt(v[2], v[3]), pt(v[4], v[5]));
                out.push(PathCmd::Cubic { c1, c2, to });
                cur = to;
            }
            'A' => {
                let v: Vec<f32> = (0..7).filter_map(|_| next(&mut t)).collect();
                if v.len() < 7 {
                    break;
                }
                let to = pt(v[5], v[6]);
                out.push(PathCmd::Arc { rx: v[0], ry: v[1], rotation: v[2], large: v[3] != 0.0, sweep: v[4] != 0.0, to });
                cur = to;
            }
            _ => t += 1,
        }
        if t == before {
            t += 1;
        }
    }
    out
}

/// `#rgb`, `#rrggbb`, `#rrggbbaa`, noms CSS ; `none` → pas de peinture.
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") || s.is_empty() {
        return None;
    }
    if let Some(hex) = s.strip_prefix('#') {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            3 => {
                let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
                Some(Color((r * 17) << 24 | (g * 17) << 16 | (b * 17) << 8 | 0xff))
            }
            6 => Some(Color(v << 8 | 0xff)),
            8 => Some(Color(v)),
            _ => None,
        };
    }
    let name = s.to_ascii_lowercase();
    NAMED_COLORS
        .binary_search_by(|(n, _)| n.cmp(&name.as_str()))
        .ok()
        .map(|i| Color(NAMED_COLORS[i].1 << 8 | 0xff))
}

/// Couleurs nommées CSS (triées pour la recherche dichotomique).
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff), ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000),
    ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00), ("chocolate", 0xd2691e),
    ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700),
    ("goldenrod", 0xdaa520), ("gray", 0x808080), ("green", 0x008000), ("greenyellow", 0xadff2f),
    ("grey", 0x808080), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1), ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3), ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6), ("olive", 0x808000),
    ("olivedrab", 0x6b8e23), ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f), ("pink", 0xffc0cb),
    ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072), ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d), ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa), ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4), ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347), ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shapes_of_each_arrow_are_found() {
        let native = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 9 9"><rect width="9" height="9" fill="#fff"/><g transform="translate(1 1)"><g data-link="1"><path d="M0 0 L5 5" fill="none" stroke="#000" stroke-dasharray="6 4"/><polygon points="1,1 2,2 3,1" fill="#fff" stroke="#000"/></g><text x="1" y="1">a</text></g></svg>"##;
        let scene = Scene::parse(native).unwrap();
        assert_eq!(scene.link_items.get(&1), Some(&(1..3)));
        let official = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 9 9"><g class="entity"><rect width="1" height="1" fill="#fff"/></g><g class="link"><path d="M0,0 L5,5" fill="none" style="stroke:#000;"/></g><g class="link"><path d="M1,1 L5,5" fill="none" style="stroke:#000;"/><polygon points="1,1 2,2 3,1" fill="#000"/></g></svg>"##;
        let scene = Scene::parse(official).unwrap();
        assert_eq!(scene.link_items.get(&0), Some(&(1..2)));
        assert_eq!(scene.link_items.get(&1), Some(&(2..4)));
    }

    /// Une ligne d'attribut : icône et texte, dans une classe dont la zone est plus large.
    fn member_scene() -> Scene {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 100"><rect x="10" y="10" width="180" height="80" fill="#fff" stroke="#000"/><line x1="10" y1="30" x2="190" y2="30" stroke="#000"/><circle cx="24" cy="40" r="3" fill="#0a0" stroke="#0a0"/><text x="32" y="44" font-size="13">nom</text><text x="32" y="60" font-size="13">autre</text></svg>"##;
        Scene::parse(svg).unwrap()
    }

    #[test]
    fn the_selection_hugs_what_is_visible_not_the_clickable_row() {
        let row = Bounds { x: 11.0, y: 32.0, w: 178.0, h: 16.0 };
        let content = member_scene().content_bounds(row, &mut |t| t.content.len() as f32 * 8.0, &mut |y| y, 1.0).unwrap();
        assert_eq!(content.x, 20.5, "bord gauche de l'icône, trait compris");
        assert_eq!(content.x + content.w, 32.0 + 24.0, "fin du texte mesuré, pas de la ligne");
        // « nom » n'a que des minuscules sans jambage ; c'est l'icône qui fixe le haut et le bas
        assert_eq!(content.y, 36.5, "le texte voisin n'entre pas");
        assert_eq!(content.y + content.h, 44.0, "rien sous la ligne de base : ni g ni parenthèse");
    }

    #[test]
    fn the_box_follows_where_the_text_is_really_painted() {
        let row = Bounds { x: 30.0, y: 34.0, w: 100.0, h: 14.0 };
        let exact = member_scene().content_bounds(row, &mut |_| 10.0, &mut |y| y, 1.0).unwrap();
        let raised = member_scene().content_bounds(row, &mut |_| 10.0, &mut |y| y - 0.5, 1.0).unwrap();
        assert_eq!(raised.y, exact.y - 0.5);
    }

    #[test]
    fn the_background_of_a_label_is_not_its_content() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 40"><rect x="10" y="10" width="60" height="18" fill="#fff" stroke="none" fill-opacity="0.85"/><text x="20" y="24" font-size="12">lien</text></svg>"##;
        let zone = Bounds { x: 10.0, y: 10.0, w: 60.0, h: 18.0 };
        let content = Scene::parse(svg).unwrap().content_bounds(zone, &mut |_| 24.0, &mut |y| y, 1.0).unwrap();
        assert_eq!((content.x, content.w), (20.0, 24.0));
    }

    #[test]
    fn an_element_with_a_border_is_its_own_content() {
        let zone = Bounds { x: 10.0, y: 10.0, w: 180.0, h: 80.0 };
        let content = member_scene().content_bounds(zone, &mut |_| 10.0, &mut |y| y, 1.0).unwrap();
        assert_eq!((content.x, content.y), (9.5, 9.5));
        assert_eq!((content.w, content.h), (181.0, 81.0));
    }

    #[test]
    fn an_empty_zone_has_no_content() {
        let zone = Bounds { x: 195.0, y: 95.0, w: 4.0, h: 4.0 };
        assert_eq!(member_scene().content_bounds(zone, &mut |_| 10.0, &mut |y| y, 1.0), None);
    }

    #[test]
    fn padding_is_equal_on_all_sides_until_the_parent_stops_it() {
        let content = Bounds { x: 50.0, y: 50.0, w: 20.0, h: 10.0 };
        let wide = Bounds { x: 0.0, y: 0.0, w: 200.0, h: 200.0 };
        assert_eq!(selection_box(content, 3.0, wide), Bounds { x: 47.0, y: 47.0, w: 26.0, h: 16.0 });
        // contre le bord gauche et le bord bas du parent : ces côtés se réduisent, pas les autres
        let tight = Bounds { x: 49.0, y: 0.0, w: 100.0, h: 61.0 };
        assert_eq!(selection_box(content, 3.0, tight), Bounds { x: 49.0, y: 47.0, w: 24.0, h: 14.0 });
        // un parent plus petit que le contenu ne le coupe jamais
        let inner = Bounds { x: 55.0, y: 55.0, w: 2.0, h: 2.0 };
        assert_eq!(selection_box(content, 3.0, inner), content);
    }

    /// Extrait de ce que produit PlantUML : couleurs dans `style`, dégradé, fond de page.
    #[test]
    fn plantuml_output_is_understood() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" style="width:50px;background:#102030;" viewBox="0 0 50 40"><defs><linearGradient id="g1"><stop stop-color="#59B6EC" offset="0%"/><stop stop-color="#2FA4E7" offset="100%"/></linearGradient></defs><g font-family="sans-serif" data-source-line="2"><rect x="1" y="2" width="3" height="4" fill="url(#g1)" style="stroke:#181818;stroke-width:0.5;"/><path d="M1,2 L3,4 Q5,6 7,8" fill="none" style="stroke:url(#g1);stroke-width:1;"/><line x1="0" y1="0" x2="5" y2="5" style="stroke:#FF0000;stroke-width:2;stroke-dasharray:2,3;"/><text x="5" y="6" fill="#F8F9FA" font-size="11" textLength="20" font-family="'Verdana'">ab</text></g></svg>"##;
        let scene = Scene::parse(svg).unwrap();
        assert_eq!((scene.width, scene.height), (50.0, 40.0));
        assert_eq!(scene.items.len(), 5, "fond + 4 éléments");
        let Item::Shape { style: background, .. } = &scene.items[0] else { panic!() };
        assert_eq!(background.fill, Some(Color(0x102030ff)));
        let Item::Shape { style: rect, .. } = &scene.items[1] else { panic!() };
        assert_eq!(rect.fill, Some(Color(0x59b6ecff)), "première couleur du dégradé");
        assert_eq!(rect.stroke, Some(Color(0x181818ff)));
        assert_eq!(rect.stroke_width, 0.5);
        let Item::Shape { style: path, geometry: Geometry::Path(cmds), .. } = &scene.items[2] else { panic!() };
        assert_eq!(path.fill, None);
        assert_eq!(path.stroke, Some(Color(0x59b6ecff)));
        assert_eq!(cmds.len(), 3);
        let Item::Shape { style: line, .. } = &scene.items[3] else { panic!() };
        assert_eq!(line.dash, vec![2.0, 3.0]);
    }


    #[test]
    fn named_colors_are_sorted() {
        assert!(NAMED_COLORS.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(parse_color("LightBlue"), Some(Color(0xadd8e6ff)));
        assert_eq!(parse_color("#FFF6BF"), Some(Color(0xfff6bfff)));
        assert_eq!(parse_color("#abc"), Some(Color(0xaabbccff)));
        assert_eq!(parse_color("none"), None);
    }

    #[test]
    fn parses_paths_with_relative_and_compact_numbers() {
        let cmds = parse_path("M10 20 H30 V40 l5-5 Q1 2 3 4 C1 2 3 4 5 6 A8 8 0 0 1 9 9 Z", 0.0, 0.0);
        assert_eq!(cmds[0], PathCmd::Move(Point { x: 10.0, y: 20.0 }));
        assert_eq!(cmds[1], PathCmd::Line(Point { x: 30.0, y: 20.0 }));
        assert_eq!(cmds[2], PathCmd::Line(Point { x: 30.0, y: 40.0 }));
        assert_eq!(cmds[3], PathCmd::Line(Point { x: 35.0, y: 35.0 }));
        assert!(matches!(cmds[6], PathCmd::Arc { sweep: true, large: false, .. }));
        assert_eq!(cmds.last(), Some(&PathCmd::Close));
    }

    #[test]
    fn builds_scene_with_groups_and_text() {
        let svg = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 200 100">
            <rect x="0" y="0" width="200" height="100" fill="#FFFFFF"/>
            <g transform="translate(10 5)">
              <rect x="1" y="2" width="30" height="20" rx="3" fill="#F7F8FC" stroke="#3B4252" stroke-width="1.2" stroke-dasharray="6 4"/>
              <text x="15" y="16" font-size="13" fill="#1F2430" text-anchor="middle" font-weight="bold">A &amp; B</text>
              <path d="M0 0 L10 10" fill="none" stroke="#000000"/>
              <rect x="0" y="0" width="5" height="5" fill="none" stroke="none"/>
            </g></svg>"##;
        let s = Scene::parse(svg).unwrap();
        assert_eq!((s.width, s.height), (200.0, 100.0));
        assert_eq!(s.items.len(), 4);
        let Item::Shape { geometry: Geometry::Rect { x, y, radius, .. }, style, .. } = &s.items[1] else { panic!() };
        assert_eq!((*x, *y, *radius), (11.0, 7.0, 3.0));
        assert_eq!(style.dash, vec![6.0, 4.0]);
        let Item::Text { text, .. } = &s.items[2] else { panic!() };
        assert_eq!(text.content, "A & B");
        assert_eq!((text.x, text.y), (25.0, 21.0));
        assert!(text.bold);
        assert_eq!(text.anchor, Anchor::Middle);
    }

    #[test]
    fn rejects_dtd() {
        let svg = r#"<!DOCTYPE svg [<!ENTITY x "y">]><svg xmlns="http://www.w3.org/2000/svg"/>"#;
        assert!(Scene::parse(svg).is_err());
    }
}
