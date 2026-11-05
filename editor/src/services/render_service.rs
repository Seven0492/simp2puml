//! Service : moteur de rendu natif `pumlrender` (Nim).
//!
//! Comme Zed avec ses serveurs de langage, l'éditeur garde un seul processus
//! `pumlrender --serve` vivant et lui envoie une requête par rendu, au lieu
//! de lancer un processus à chaque frappe. Si le processus meurt, il est
//! relancé ; si le binaire ne connaît pas `--serve`, on se replie sur un
//! processus par rendu (`--editor`).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::model::diagram::{RenderOutput, parse_protocol};

struct Server {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct RenderService {
    exe: PathBuf,
    server: Mutex<Option<Server>>,
    next_id: AtomicU64,
    serve_unsupported: AtomicBool,
    /// vrai dès qu'une requête --serve a abouti
    served: AtomicBool,
}

impl RenderService {
    /// Cherche `pumlrender` : $PUMLRENDER, à côté de l'exécutable, dans
    /// `renderer/bin` du dépôt, puis dans le PATH.
    pub fn locate() -> Option<RenderService> {
        locate_renderer().map(Self::new)
    }

    pub fn new(exe: PathBuf) -> Self {
        Self {
            exe,
            server: Mutex::new(None),
            next_id: AtomicU64::new(1),
            serve_unsupported: AtomicBool::new(false),
            served: AtomicBool::new(false),
        }
    }

    /// Rendu bloquant : à appeler depuis l'exécuteur d'arrière-plan.
    pub fn render(&self, text: &str, block: usize) -> Result<RenderOutput, String> {
        if self.serve_unsupported.load(Ordering::Relaxed) {
            return render_once(&self.exe, text, block);
        }
        let mut guard = self.server.lock().map_err(|_| "service de rendu indisponible")?;
        // deux tentatives : la seconde après relance d'un processus mort
        for attempt in 0..2 {
            if guard.is_none() {
                *guard = Some(self.spawn()?);
            }
            let server = guard.as_mut().expect("processus démarré");
            match self.request(server, text, block) {
                Ok(out) => {
                    self.served.store(true, Ordering::Relaxed);
                    return Ok(out);
                }
                Err(e) => {
                    *guard = None;
                    if attempt == 1 {
                        // Un moteur qui a déjà répondu gère --serve : l'échec vient
                        // de ce document (le processus sera relancé au prochain rendu).
                        // Sinon, le binaire ne connaît probablement pas --serve.
                        if !self.served.load(Ordering::Relaxed) {
                            self.serve_unsupported.store(true, Ordering::Relaxed);
                        }
                        return render_once(&self.exe, text, block).map_err(|e2| format!("{e} ; {e2}"));
                    }
                }
            }
        }
        unreachable!()
    }

    fn spawn(&self) -> Result<Server, String> {
        let mut child = Command::new(&self.exe)
            .arg("--serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("impossible de lancer {} : {e}", self.exe.display()))?;
        let stdin = child.stdin.take().ok_or("stdin indisponible")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("stdout indisponible")?);
        Ok(Server { child, stdin, stdout })
    }

    fn request(&self, server: &mut Server, text: &str, block: usize) -> Result<RenderOutput, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let io = |e: std::io::Error| format!("échange avec le moteur de rendu : {e}");
        write!(server.stdin, "RENDER {id} {block} {}\n", text.len()).map_err(io)?;
        server.stdin.write_all(text.as_bytes()).map_err(io)?;
        server.stdin.flush().map_err(io)?;
        // en-têtes jusqu'à « SVG\t<n> », puis exactement n octets
        let mut raw = Vec::new();
        loop {
            let mut line = String::new();
            if server.stdout.read_line(&mut line).map_err(io)? == 0 {
                return Err("le moteur de rendu s'est arrêté".into());
            }
            raw.extend_from_slice(line.as_bytes());
            if let Some(n) = line.strip_prefix("SVG\t") {
                let n: usize = n.trim().parse().map_err(|_| "longueur SVG invalide")?;
                let mut svg = vec![0; n];
                server.stdout.read_exact(&mut svg).map_err(io)?;
                raw.extend_from_slice(&svg);
                break;
            }
            if line.starts_with("ERROR") {
                break;
            }
        }
        let out = parse_protocol(&raw)?;
        if out.id != id {
            return Err(format!("réponse désynchronisée ({} au lieu de {id})", out.id));
        }
        Ok(out)
    }
}

/// Repli : un processus par rendu.
fn render_once(exe: &Path, text: &str, block: usize) -> Result<RenderOutput, String> {
    let mut child = Command::new(exe)
        .arg("--editor")
        .arg(format!("--block={}", block + 1))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("impossible de lancer {} : {e}", exe.display()))?;
    let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
    let data = text.as_bytes().to_vec();
    // écriture dans un fil séparé pour éviter un interblocage sur de gros SVG
    std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output().map_err(|e| format!("échec du moteur de rendu : {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(format!("le moteur de rendu a échoué : {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_protocol(&out.stdout)
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "pumlrender.exe" } else { "pumlrender" }
}

fn locate_renderer() -> Option<PathBuf> {
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
    std::env::split_paths(&path).map(|d| d.join(exe_name())).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_process_serves_several_requests() {
        let Some(service) = RenderService::locate() else {
            eprintln!("pumlrender absent : test ignoré");
            return;
        };
        let a = service.render("@startuml\nA -> B : x\n@enduml\n", 0).unwrap();
        assert_eq!(a.kind, "sequence");
        assert!(!a.regions.is_empty());
        let b = service.render("@startmindmap\n* racine\n@endmindmap\n", 0).unwrap();
        assert_eq!(b.kind, "mindmap");
        assert!(!service.serve_unsupported.load(Ordering::Relaxed));
    }

    #[test]
    fn every_sample_becomes_a_vector_scene() {
        let Some(service) = RenderService::locate() else {
            eprintln!("pumlrender absent : test ignoré");
            return;
        };
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../renderer/tests/samples");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            let out = service.render(&text, 0).unwrap();
            let scene = crate::model::svg_scene::Scene::parse(&out.svg)
                .unwrap_or_else(|e| panic!("{} : {e}", path.display()));
            assert!(scene.items.len() > 3, "{}", path.display());
            assert_eq!((scene.width, scene.height), (out.width, out.height));
            checked += 1;
        }
        assert!(checked >= 10);
    }
}
