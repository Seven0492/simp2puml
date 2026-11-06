//! Modèle : résultat d'un rendu (diagnostics, régions cliquables, SVG) et
//! lecture du protocole texte de `pumlrender`.

use crate::model::preferences::LinkClick;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    /// Ligne 1-based dans le document.
    pub line: usize,
    pub severity: Severity,
    pub message: String,
}

/// Zone du diagramme (coordonnées SVG) rattachée à une ligne source.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub line: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Region {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
}

/// Tracé d'une flèche, cliquable selon `LinkClick`.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub line: usize,
    /// Les régions à partir de cet indice sont dessinées au-dessus de la flèche.
    pub after: usize,
    pub points: Vec<(f32, f32)>,
}

/// Longueur cliquable de chaque bout d'une flèche, en mode `LinkClick::Ends`.
pub const ARROW_END_LENGTH: f32 = 28.0;

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

impl Link {
    /// Morceaux cliquables : tout le tracé, ou seulement ses deux bouts.
    pub fn clickable(&self, mode: LinkClick) -> Vec<Vec<(f32, f32)>> {
        let total: f32 = self.points.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
        if mode == LinkClick::Along || total <= 2.0 * ARROW_END_LENGTH {
            return vec![self.points.clone()];
        }
        let reversed: Vec<(f32, f32)> = self.points.iter().rev().copied().collect();
        vec![head_of(&self.points), head_of(&reversed)]
    }

    pub fn distance(&self, x: f32, y: f32, mode: LinkClick) -> f32 {
        self.clickable(mode)
            .iter()
            .flat_map(|part| part.windows(2).map(|w| segment_distance((x, y), w[0], w[1])))
            .fold(f32::INFINITY, f32::min)
    }
}

/// Début d'un tracé, sur `ARROW_END_LENGTH`.
fn head_of(points: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let mut out = vec![points[0]];
    let mut left = ARROW_END_LENGTH;
    for w in points.windows(2) {
        let len = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        if len >= left {
            let t = if len > 0.0 { left / len } else { 0.0 };
            out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            return out;
        }
        out.push(w[1]);
        left -= len;
    }
    out
}

/// Ce qui se trouve sous le pointeur.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    Region(usize),
    /// Une ou plusieurs flèches, la plus proche d'abord.
    Links(Vec<usize>),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderOutput {
    /// Identifiant de requête renvoyé par le mode persistant.
    pub id: String,
    pub kind: String,
    pub blocks: usize,
    pub width: f32,
    pub height: f32,
    pub diagnostics: Vec<Diagnostic>,
    pub regions: Vec<Region>,
    pub links: Vec<Link>,
    pub svg: String,
}

impl RenderOutput {
    /// Élément sous le pointeur. `tolerance` : distance à une flèche, en unités du SVG.
    pub fn hit_at(&self, x: f32, y: f32, tolerance: f32, mode: LinkClick) -> Option<Hit> {
        let mut near: Vec<(usize, f32)> = self
            .links
            .iter()
            .enumerate()
            .map(|(i, l)| (i, l.distance(x, y, mode)))
            .filter(|(_, d)| *d <= tolerance)
            .collect();
        near.sort_by(|a, b| a.1.total_cmp(&b.1));
        let region = self.region_at(x, y);
        match (near.first(), region) {
            (None, None) => None,
            (None, Some(r)) => Some(Hit::Region(r)),
            // ce qui est dessiné au-dessus de la flèche la plus proche (boîte, libellé) l'emporte
            (Some((closest, _)), Some(r)) if r >= self.links[*closest].after => Some(Hit::Region(r)),
            _ => Some(Hit::Links(near.into_iter().map(|(i, _)| i).collect())),
        }
    }

    pub fn count(&self, sev: Severity) -> usize {
        self.diagnostics.iter().filter(|d| d.severity == sev).count()
    }

    /// Région la plus petite contenant le point (la plus précise).
    pub fn region_at(&self, x: f32, y: f32) -> Option<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, r)| r.contains(x, y))
            .min_by(|(_, a), (_, b)| (a.w * a.h).total_cmp(&(b.w * b.h)))
            .map(|(i, _)| i)
    }
}

/// Analyse une réponse complète (`PUMLRENDER 1` … `SVG\t<n>` + n octets).
pub fn parse_protocol(out: &[u8]) -> Result<RenderOutput, String> {
    let mut r = RenderOutput::default();
    let mut pos = 0;
    let mut first = true;
    loop {
        let Some(nl) = out[pos..].iter().position(|b| *b == b'\n') else {
            return Err("sortie du moteur de rendu tronquée".into());
        };
        let line = String::from_utf8_lossy(&out[pos..pos + nl]).to_string();
        pos += nl + 1;
        if first {
            if line != "PUMLRENDER 1" {
                return Err(format!("protocole inconnu : {line}"));
            }
            first = false;
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let field = |i: usize| parts.get(i).copied().unwrap_or("");
        match parts[0] {
            "ID" => r.id = field(1).to_string(),
            "ERROR" => return Err(field(1).to_string()),
            "KIND" => r.kind = field(1).to_string(),
            "BLOCKS" => r.blocks = field(1).parse().unwrap_or(1),
            "SIZE" => {
                r.width = field(1).parse().unwrap_or(0.0);
                r.height = field(2).parse().unwrap_or(0.0);
            }
            "DIAG" if parts.len() >= 4 => r.diagnostics.push(Diagnostic {
                line: field(1).parse().unwrap_or(0),
                severity: match field(2) {
                    "error" => Severity::Error,
                    "warning" => Severity::Warning,
                    _ => Severity::Info,
                },
                message: parts[3..].join(" "),
            }),
            "REGION" if parts.len() >= 6 => r.regions.push(Region {
                line: field(1).parse().unwrap_or(0),
                x: field(2).parse().unwrap_or(0.0),
                y: field(3).parse().unwrap_or(0.0),
                w: field(4).parse().unwrap_or(0.0),
                h: field(5).parse().unwrap_or(0.0),
            }),
            "LINK" if parts.len() >= 4 => {
                let v: Vec<f32> = field(3).split(' ').filter_map(|s| s.parse().ok()).collect();
                let points: Vec<(f32, f32)> = v.chunks_exact(2).map(|c| (c[0], c[1])).collect();
                if points.len() >= 2 {
                    r.links.push(Link { line: field(1).parse().unwrap_or(0), after: field(2).parse().unwrap_or(0), points });
                }
            }
            "SVG" => {
                let len: usize = field(1).parse().map_err(|_| "longueur SVG invalide")?;
                if pos + len > out.len() {
                    return Err("SVG tronqué".into());
                }
                r.svg = String::from_utf8_lossy(&out[pos..pos + len]).to_string();
                return Ok(r);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80.5\"></svg>";
        format!(
            "PUMLRENDER 1\nID\t7\nKIND\tclass\nBLOCKS\t2\nSIZE\t120\t80.5\nDIAG\t3\twarning\tligne ignorée\nREGION\t2\t10\t20\t30\t40\nREGION\t5\t0\t0\t100\t100\nSVG\t{}\n{}",
            svg.len(),
            svg
        )
    }

    #[test]
    fn parses_protocol() {
        let r = parse_protocol(sample().as_bytes()).unwrap();
        assert_eq!(r.id, "7");
        assert_eq!(r.kind, "class");
        assert_eq!(r.blocks, 2);
        assert_eq!(r.width, 120.0);
        assert_eq!(r.diagnostics[0].line, 3);
        assert_eq!(r.diagnostics[0].severity, Severity::Warning);
        assert!(r.svg.ends_with("</svg>"));
    }

    #[test]
    fn smallest_region_wins() {
        let r = parse_protocol(sample().as_bytes()).unwrap();
        assert_eq!(r.region_at(15.0, 25.0), Some(0));
        assert_eq!(r.region_at(90.0, 90.0), Some(1));
        assert_eq!(r.region_at(500.0, 0.0), None);
    }

    fn arrow(line: usize, after: usize, points: &[(f32, f32)]) -> Link {
        Link { line, after, points: points.to_vec() }
    }

    #[test]
    fn links_are_read_and_a_bad_one_is_ignored() {
        let text = "PUMLRENDER 1\nID\t1\nLINK\t4\t2\t0 0 10 0 10 10\nLINK\t5\t2\t1\nSVG\t0\n";
        let r = parse_protocol(text.as_bytes()).unwrap();
        assert_eq!(r.links, vec![arrow(4, 2, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)])]);
    }

    #[test]
    fn an_arrow_is_clickable_along_its_whole_length() {
        let l = arrow(1, 0, &[(0.0, 0.0), (200.0, 0.0), (200.0, 200.0)]);
        assert_eq!(l.distance(100.0, 3.0, LinkClick::Along), 3.0, "au milieu d'un segment");
        assert_eq!(l.distance(203.0, 100.0, LinkClick::Along), 3.0, "après le coude");
        assert!(l.distance(100.0, 100.0, LinkClick::Along) > 50.0, "pas dans le vide de la diagonale");
    }

    #[test]
    fn in_ends_mode_only_the_start_and_the_tip_are_clickable() {
        let l = arrow(1, 0, &[(0.0, 0.0), (200.0, 0.0)]);
        assert_eq!(l.distance(10.0, 2.0, LinkClick::Ends), 2.0);
        assert_eq!(l.distance(190.0, 2.0, LinkClick::Ends), 2.0);
        assert!(l.distance(100.0, 2.0, LinkClick::Ends) > 50.0, "le milieu ne réagit pas");
        // une flèche trop courte pour avoir un milieu reste entière
        let short = arrow(1, 0, &[(0.0, 0.0), (40.0, 0.0)]);
        assert_eq!(short.distance(20.0, 1.0, LinkClick::Ends), 1.0);
        // un bout peut passer un coude
        let bent = arrow(1, 0, &[(0.0, 0.0), (10.0, 0.0), (10.0, 200.0)]);
        assert_eq!(bent.distance(10.0, 15.0, LinkClick::Ends), 0.0);
        assert!(bent.distance(10.0, 100.0, LinkClick::Ends) > 50.0);
    }

    #[test]
    fn overlapping_arrows_are_all_reported_closest_first() {
        let r = RenderOutput {
            links: vec![arrow(1, 0, &[(0.0, 0.0), (100.0, 0.0)]), arrow(2, 0, &[(0.0, 3.0), (100.0, 3.0)]), arrow(3, 0, &[(0.0, 90.0), (100.0, 90.0)])],
            ..RenderOutput::default()
        };
        assert_eq!(r.hit_at(50.0, 2.0, 6.0, LinkClick::Along), Some(Hit::Links(vec![1, 0])));
        assert_eq!(r.hit_at(50.0, 90.0, 6.0, LinkClick::Along), Some(Hit::Links(vec![2])));
        assert_eq!(r.hit_at(50.0, 50.0, 6.0, LinkClick::Along), None);
    }

    #[test]
    fn what_is_drawn_above_the_arrow_wins() {
        let zone = |x: f32| Region { line: 9, x, y: 0.0, w: 10.0, h: 10.0 };
        let r = RenderOutput { regions: vec![zone(0.0), zone(50.0)], links: vec![arrow(1, 1, &[(0.0, 5.0), (100.0, 5.0)])], ..RenderOutput::default() };
        // conteneur dessiné avant la flèche : la flèche ; boîte dessinée après : la boîte
        assert_eq!(r.hit_at(5.0, 5.0, 6.0, LinkClick::Along), Some(Hit::Links(vec![0])));
        assert_eq!(r.hit_at(55.0, 5.0, 6.0, LinkClick::Along), Some(Hit::Region(1)));
    }

    #[test]
    fn hit_testing_stays_fast_on_a_large_diagram() {
        let links: Vec<Link> = (0..3000).map(|i| arrow(i, 0, &[(0.0, i as f32), (500.0, i as f32 + 3.0), (900.0, i as f32)])).collect();
        let r = RenderOutput { links, ..RenderOutput::default() };
        let start = std::time::Instant::now();
        for _ in 0..10 {
            assert!(r.hit_at(250.0, 1500.0, 6.0, LinkClick::Along).is_some());
        }
        let each = start.elapsed() / 10;
        eprintln!("détection, 3000 flèches : {each:?} par mouvement de souris");
        assert!(each < std::time::Duration::from_millis(50), "{each:?}");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_protocol(b"hello\n").is_err());
        assert!(parse_protocol(b"PUMLRENDER 1\nSVG\t999\n<svg/>").is_err());
        assert!(parse_protocol(b"PUMLRENDER 1\nERROR\tmauvais\n").is_err());
    }
}
