//! Client du moteur de rendu natif `pumlrender` (Nim) : recherche du binaire,
//! exécution sans shell et lecture du protocole `--editor`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub line: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderOutput {
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

    /// SVG redimensionné : largeur/hauteur multipliées, viewBox conservée,
    /// pour une rastérisation nette quel que soit le zoom.
    pub fn scaled_svg(&self, zoom: f32) -> String {
        let w = (self.width * zoom).max(1.0);
        let h = (self.height * zoom).max(1.0);
        let svg = &self.svg;
        let Some(start) = svg.find("<svg ") else {
            return svg.clone();
        };
        let Some(end) = svg[start..].find('>').map(|e| start + e) else {
            return svg.clone();
        };
        let head = &svg[start..end];
        let mut new_head = String::new();
        let mut rest = head;
        for attr in ["width=\"", "height=\""] {
            if let Some(p) = rest.find(&format!(" {attr}")) {
                let vstart = p + 1 + attr.len();
                if let Some(vend) = rest[vstart..].find('"') {
                    new_head.push_str(&rest[..vstart]);
                    let v = if attr.starts_with('w') { w } else { h };
                    new_head.push_str(&format!("{v:.2}"));
                    rest = &rest[vstart + vend..];
                }
            }
        }
        new_head.push_str(rest);
        format!("{}{}{}", &svg[..start], new_head, &svg[end..])
    }
}

/// Analyse la sortie de `pumlrender --editor`.
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
        match parts[0] {
            "KIND" => r.kind = parts.get(1).unwrap_or(&"").to_string(),
            "BLOCKS" => r.blocks = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1),
            "SIZE" => {
                r.width = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                r.height = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
            }
            "DIAG" if parts.len() >= 4 => r.diagnostics.push(Diagnostic {
                line: parts[1].parse().unwrap_or(0),
                severity: match parts[2] {
                    "error" => Severity::Error,
                    "warning" => Severity::Warning,
                    _ => Severity::Info,
                },
                message: parts[3..].join(" "),
            }),
            "REGION" if parts.len() >= 6 => r.regions.push(Region {
                line: parts[1].parse().unwrap_or(0),
                x: parts[2].parse().unwrap_or(0.0),
                y: parts[3].parse().unwrap_or(0.0),
                w: parts[4].parse().unwrap_or(0.0),
                h: parts[5].parse().unwrap_or(0.0),
            }),
            "SVG" => {
                let len: usize = parts
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .ok_or("longueur SVG invalide")?;
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

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "pumlrender.exe"
    } else {
        "pumlrender"
    }
}

/// Emplacements essayés : $PUMLRENDER, à côté de l'exécutable, dépôt
/// (renderer/bin), puis le PATH.
pub fn locate_renderer() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("PUMLRENDER") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..5 {
            let Some(d) = dir else { break };
            for cand in [d.join(exe_name()), d.join("renderer").join("bin").join(exe_name())] {
                if cand.is_file() {
                    return Some(cand);
                }
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(exe_name()))
        .find(|p| p.is_file())
}

/// Lance le rendu (bloquant) ; à appeler depuis un exécuteur d'arrière-plan.
pub fn render(renderer: &Path, text: &str, block: usize) -> Result<RenderOutput, String> {
    let mut child = Command::new(renderer)
        .arg("--editor")
        .arg(format!("--block={}", block + 1))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("impossible de lancer {} : {e}", renderer.display()))?;
    {
        let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
        let data = text.as_bytes().to_vec();
        // écriture dans un fil séparé pour éviter un interblocage sur de gros SVG
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("échec du moteur de rendu : {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!(
            "le moteur de rendu a échoué : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    parse_protocol(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80.5\"></svg>";
        format!(
            "PUMLRENDER 1\nKIND\tclass\nBLOCKS\t2\nSIZE\t120\t80.5\nDIAG\t3\twarning\tligne ignorée\nREGION\t2\t10\t20\t30\t40\nSVG\t{}\n{}",
            svg.len(),
            svg
        )
    }

    #[test]
    fn parses_protocol() {
        let r = parse_protocol(sample().as_bytes()).unwrap();
        assert_eq!(r.kind, "class");
        assert_eq!(r.blocks, 2);
        assert_eq!(r.width, 120.0);
        assert_eq!(r.diagnostics[0].line, 3);
        assert_eq!(r.diagnostics[0].severity, Severity::Warning);
        assert_eq!(r.regions[0].w, 30.0);
        assert!(r.svg.starts_with("<svg"));
        assert!(r.svg.ends_with("</svg>"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_protocol(b"hello\n").is_err());
        assert!(parse_protocol(b"PUMLRENDER 1\nSVG\t999\n<svg/>").is_err());
    }

    #[test]
    fn scales_svg_dimensions() {
        let r = parse_protocol(sample().as_bytes()).unwrap();
        let s = r.scaled_svg(2.0);
        assert!(s.contains("width=\"240.00\""));
        assert!(s.contains("height=\"161.00\""));
    }

    #[test]
    fn renders_with_real_binary_when_available() {
        let Some(bin) = locate_renderer() else {
            eprintln!("pumlrender absent : test ignoré");
            return;
        };
        let r = render(&bin, "@startuml\nA -> B : x\n@enduml\n", 0).unwrap();
        assert_eq!(r.kind, "sequence");
        assert!(r.svg.contains("<svg"));
        assert!(!r.regions.is_empty());
    }
}
