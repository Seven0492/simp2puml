//! Service : passage à l'interface classique (Tkinter, `simp2puml-gui`).
//!
//! Les deux interfaces partagent le convertisseur et le fichier de préférences :
//! seule la fenêtre change. Le choix n'est enregistré qu'une fois l'autre
//! interface démarrée, pour ne jamais laisser un choix qui ne s'ouvre pas.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::services::file_service;

/// Un échec de démarrage se voit vite ; au-delà, la fenêtre est en train de s'ouvrir.
const START_CHECK: Duration = Duration::from_millis(600);

/// $SIMP2PUML_GUI, à côté de l'exécutable, dans le PATH (entrées absolues), sinon le module Python.
fn classic_command() -> Command {
    let mut cmd = if let Some(p) = std::env::var_os("SIMP2PUML_GUI") {
        Command::new(p)
    } else {
        let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("simp2puml-gui")));
        let in_path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).map(|d| d.join("simp2puml-gui")).collect::<Vec<_>>())
            .unwrap_or_default();
        match beside.into_iter().chain(in_path).find(|c| c.is_file()) {
            Some(found) => Command::new(found),
            None => file_service::python_module("simp2puml.launcher"),
        }
    };
    cmd.arg("--interface").arg("classic");
    cmd
}

/// Démarre l'interface classique sur ce fichier ; elle survit à la fermeture de l'éditeur.
pub fn launch_classic(file: Option<&Path>) -> Result<(), String> {
    let mut cmd = classic_command();
    cmd.args(file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("interface classique introuvable ({e})"))?;
    let start = Instant::now();
    while start.elapsed() < START_CHECK {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut detail = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = stderr.read_to_string(&mut detail);
                }
                let detail = detail.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default().to_string();
                return Err(if detail.is_empty() { format!("l'interface classique s'est arrêtée ({status})") } else { detail });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("interface classique : {e}")),
        }
    }
    Ok(())
}
