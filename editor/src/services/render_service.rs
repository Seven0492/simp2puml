//! Service : moteur de rendu natif `pumlrender` (Nim).
//!
//! Comme Zed avec ses serveurs de langage, l'éditeur garde un seul processus
//! `pumlrender --serve` vivant et lui envoie une requête par rendu, au lieu
//! de lancer un processus à chaque frappe. Si le processus meurt, il est
//! relancé ; si le binaire ne connaît pas `--serve` (sondé au lancement), on
//! se replie sur un processus par rendu (`--editor`).
//!
//! Un rendu qui dépasse `RENDER_TIMEOUT` est abandonné et son processus tué :
//! un document qui bloque le moteur ne doit pas bloquer les autres onglets.
//! Sous Linux, le moteur est tué avec l'éditeur (PR_SET_PDEATHSIG).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use crate::model::diagram::{RenderOutput, parse_protocol};

pub const RENDER_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE: &str = "@startuml\n@enduml\n";

/// Échec d'une requête : `retry` si le processus est mort (le relancer peut
/// suffire), faux pour un dépassement de délai (le même texte bloquerait encore).
struct Failure {
    message: String,
    retry: bool,
}

struct Server {
    child: Child,
    stdin: ChildStdin,
    /// réponses brutes lues par un fil dédié (lecture interruptible par délai)
    responses: Receiver<Result<Vec<u8>, String>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Lit une réponse : en-têtes jusqu'à « SVG\t<n> » puis exactement n octets,
/// ou une ligne ERROR. `Ok(None)` en fin de flux.
fn read_response(stdout: &mut BufReader<ChildStdout>) -> Result<Option<Vec<u8>>, String> {
    let io = |e: std::io::Error| format!("échange avec le moteur de rendu : {e}");
    let mut raw = Vec::new();
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line).map_err(io)? == 0 {
            return Ok(None);
        }
        raw.extend_from_slice(line.as_bytes());
        if let Some(n) = line.strip_prefix("SVG\t") {
            let n: usize = n.trim().parse().map_err(|_| "longueur SVG invalide".to_string())?;
            let mut svg = vec![0; n];
            stdout.read_exact(&mut svg).map_err(io)?;
            raw.extend_from_slice(&svg);
            return Ok(Some(raw));
        }
        if line.starts_with("ERROR") {
            return Ok(Some(raw));
        }
    }
}

/// Le moteur ne survit pas à l'éditeur, même en cas d'arrêt brutal.
fn die_with_parent(cmd: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
        }
        const PR_SET_PDEATHSIG: i32 = 1;
        const SIGKILL: u64 = 9;
        // SAFETY : prctl est sûr entre fork et exec (aucune allocation).
        unsafe {
            cmd.pre_exec(|| {
                prctl(PR_SET_PDEATHSIG, SIGKILL, 0, 0, 0);
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = cmd;
}

pub struct RenderService {
    pub exe: PathBuf,
    server: Mutex<Option<Server>>,
    next_id: AtomicU64,
    serve_unsupported: AtomicBool,
    /// vrai dès qu'une requête --serve a abouti (sonde comprise)
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
                match self.spawn() {
                    Ok(s) => *guard = Some(s),
                    Err(e) if !self.served.load(Ordering::Relaxed) => {
                        // la sonde a échoué : binaire sans --serve
                        self.serve_unsupported.store(true, Ordering::Relaxed);
                        drop(guard);
                        return render_once(&self.exe, text, block).map_err(|e2| format!("{e} ; {e2}"));
                    }
                    Err(e) => return Err(e),
                }
            }
            let server = guard.as_mut().expect("processus démarré");
            match self.request(server, text, block) {
                Ok(out) => return Ok(out),
                Err(f) => {
                    // processus tué ou mort : relancé à la tentative suivante ou au prochain rendu
                    *guard = None;
                    if !f.retry || attempt == 1 {
                        return Err(f.message);
                    }
                }
            }
        }
        unreachable!()
    }

    /// Lance `pumlrender --serve` ; à la première fois, vérifie par une requête
    /// triviale que le binaire gère ce mode.
    fn spawn(&self) -> Result<Server, String> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--serve").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        die_with_parent(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("impossible de lancer {} : {e}", self.exe.display()))?;
        let stdin = child.stdin.take().ok_or("stdin indisponible")?;
        let mut stdout = BufReader::new(child.stdout.take().ok_or("stdout indisponible")?);
        let (tx, responses) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let r = read_response(&mut stdout);
                let end = !matches!(r, Ok(Some(_)));
                if tx.send(r.and_then(|o| o.ok_or_else(|| "le moteur de rendu s'est arrêté".to_string()))).is_err() || end {
                    break;
                }
            }
        });
        let mut server = Server { child, stdin, responses };
        if !self.served.load(Ordering::Relaxed) {
            self.request(&mut server, PROBE, 0).map_err(|f| f.message)?;
            self.served.store(true, Ordering::Relaxed);
        }
        Ok(server)
    }

    fn request(&self, server: &mut Server, text: &str, block: usize) -> Result<RenderOutput, Failure> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed).to_string();
        let died = |message: String| Failure { message, retry: true };
        let io = |e: std::io::Error| died(format!("échange avec le moteur de rendu : {e}"));
        writeln!(server.stdin, "RENDER {id} {block} {}", text.len()).map_err(io)?;
        server.stdin.write_all(text.as_bytes()).map_err(io)?;
        server.stdin.flush().map_err(io)?;
        let raw = match server.responses.recv_timeout(RENDER_TIMEOUT) {
            Ok(r) => r.map_err(died)?,
            Err(RecvTimeoutError::Timeout) => {
                return Err(Failure {
                    message: format!("rendu abandonné après {} s (le moteur ne répond plus)", RENDER_TIMEOUT.as_secs()),
                    retry: false,
                });
            }
            Err(RecvTimeoutError::Disconnected) => return Err(died("le moteur de rendu s'est arrêté".into())),
        };
        let out = parse_protocol(&raw).map_err(died)?;
        if out.id != id {
            return Err(died(format!("réponse désynchronisée ({} au lieu de {id})", out.id)));
        }
        Ok(out)
    }
}

/// Repli : un processus par rendu (même délai maximal).
fn render_once(exe: &Path, text: &str, block: usize) -> Result<RenderOutput, String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--editor")
        .arg(format!("--block={}", block + 1))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    die_with_parent(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("impossible de lancer {} : {e}", exe.display()))?;
    let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
    let data = text.as_bytes().to_vec();
    // écriture et lectures dans des fils séparés : pas d'interblocage sur de gros SVG
    std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let collect = |mut r: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        rx
    };
    let stdout = collect(Box::new(child.stdout.take().ok_or("stdout indisponible")?));
    let stderr = collect(Box::new(child.stderr.take().ok_or("stderr indisponible")?));
    let out = match stdout.recv_timeout(RENDER_TIMEOUT) {
        Ok(out) => out,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("rendu abandonné après {} s (le moteur ne répond plus)", RENDER_TIMEOUT.as_secs()));
        }
    };
    let status = child.wait().map_err(|e| format!("échec du moteur de rendu : {e}"))?;
    if !status.success() && out.is_empty() {
        let err = stderr.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
        return Err(format!("le moteur de rendu a échoué : {}", String::from_utf8_lossy(&err).trim()));
    }
    parse_protocol(&out)
}

/// `$PUMLRENDER` défini mais qui ne désigne pas un fichier : à signaler, car un
/// autre moteur que celui demandé serait utilisé sans que rien ne l'indique.
pub fn invalid_override() -> Option<String> {
    std::env::var_os("PUMLRENDER").map(PathBuf::from).filter(|p| !p.is_file()).map(|p| p.display().to_string())
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
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(exe_name()))
        .find(|p| p.is_file())
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

    /// Faux moteur : répond à la sonde, puis ne répond plus.
    #[cfg(unix)]
    #[test]
    fn a_stuck_renderer_is_abandoned_after_the_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("puml-editor-bloque-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("pumlrender");
        std::fs::write(
            &script,
            "#!/bin/sh\nread h; read a; read b\n\
             printf 'PUMLRENDER 1\\nID\\t1\\nKIND\\tempty\\nBLOCKS\\t1\\nSIZE\\t1\\t1\\nSVG\\t0\\n'\n\
             exec sleep 60\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let service = RenderService::new(script);
        let start = std::time::Instant::now();
        let err = service.render("@startuml\nA -> B\n@enduml\n", 0).unwrap_err();
        assert!(err.contains("abandonné"), "{err}");
        assert!(start.elapsed() < RENDER_TIMEOUT + Duration::from_secs(3));
        // le mode persistant reste en place (le moteur gère --serve)
        assert!(!service.serve_unsupported.load(Ordering::Relaxed));
        let _ = std::fs::remove_dir_all(&dir);
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
