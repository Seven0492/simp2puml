//! Modèle : résultat d'un rendu (diagnostics, régions cliquables, SVG) et
//! lecture du protocole texte de `pumlrender`.

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
    pub svg: String,
}

impl RenderOutput {
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

    #[test]
    fn rejects_garbage() {
        assert!(parse_protocol(b"hello\n").is_err());
        assert!(parse_protocol(b"PUMLRENDER 1\nSVG\t999\n<svg/>").is_err());
        assert!(parse_protocol(b"PUMLRENDER 1\nERROR\tmauvais\n").is_err());
    }
}
