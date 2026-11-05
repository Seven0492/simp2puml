//! Service : fichiers — lecture bornée, enregistrement atomique, export sans
//! écrasement et passerelle vers le convertisseur Python `simp2puml`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_INPUT: u64 = 20 * 1024 * 1024;

pub fn read_text(path: &Path) -> Result<String, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{} : {e}", path.display()))?;
    if meta.len() > MAX_INPUT {
        return Err("fichier trop volumineux (maximum : 20 MiB)".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("{} : {e}", path.display()))?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|_| format!("{} : contenu non UTF-8", path.display()))
}

fn temp_sibling(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("fichier");
    path.with_file_name(format!(".{name}.{}.{nanos}.tmp", std::process::id()))
}

fn write_temp(path: &Path, content: &str) -> Result<PathBuf, String> {
    let tmp = temp_sibling(path);
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| format!("{} : {e}", tmp.display()))?;
    f.write_all(content.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("{} : {e}", tmp.display())
        })?;
    Ok(tmp)
}

/// Enregistre le document édité (remplacement atomique du fichier ouvert).
pub fn save_document(path: &Path, content: &str) -> Result<(), String> {
    let tmp = write_temp(path, content)?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{} : {e}", path.display())
    })
}

/// Écrit une nouvelle sortie ; refuse d'écraser un fichier existant.
pub fn write_new(path: &Path, content: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!("la sortie existe déjà : {}", path.display()));
    }
    let tmp = write_temp(path, content)?;
    // hard_link échoue si la cible est apparue entre-temps : jamais d'écrasement
    let r = fs::hard_link(&tmp, path).map_err(|e| format!("{} : {e}", path.display()));
    let _ = fs::remove_file(&tmp);
    r
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("puml-editor-{}-{nanos}", std::process::id()));
        fs::create_dir(&p).map_err(|e| format!("{} : {e}", p.display()))?;
        Ok(Self(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Commande du convertisseur : $SIMP2PUML, `simp2puml` du PATH, sinon
/// `python3 -m simp2puml` (avec src/ du dépôt si présent).
fn converter() -> Command {
    if let Some(p) = std::env::var_os("SIMP2PUML") {
        return Command::new(p);
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let c = d.join("simp2puml");
            if c.is_file() {
                return Command::new(c);
            }
        }
    }
    let mut cmd = Command::new("python3");
    cmd.arg("-m").arg("simp2puml");
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..5 {
            let Some(d) = dir else { break };
            let src = d.join("src").join("simp2puml");
            if src.join("__init__.py").is_file() {
                cmd.env("PYTHONPATH", d.join("src"));
                break;
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }
    cmd
}

pub struct Conversion {
    pub text: String,
    pub messages: Vec<String>,
}

fn run(mut cmd: Command) -> Result<Vec<String>, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("convertisseur simp2puml introuvable ({e}) ; installez-le ou définissez SIMP2PUML"))?;
    let mut messages: Vec<String> = String::from_utf8_lossy(&out.stderr)
        .lines()
        .map(str::to_string)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !out.status.success() {
        if messages.is_empty() {
            messages.push(format!("échec de la conversion ({})", out.status));
        }
        return Err(messages.join("\n"));
    }
    messages.extend(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with("Avertissement :"))
            .map(str::to_string),
    );
    Ok(messages)
}

/// Importe un fichier .simp en texte PlantUML (sans rien écrire à côté de la source).
/// Sans diagramme désigné, tous les diagrammes du projet sont importés, un bloc
/// `@start…@end` chacun : le convertisseur refuserait sinon un projet qui en
/// contient plusieurs.
pub fn import_simp(source: &Path, diagram: Option<&str>) -> Result<Conversion, String> {
    let dir = TempDir::new()?;
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("diagramme");
    let out = dir.0.join(format!("{stem}.puml"));
    let mut cmd = converter();
    cmd.arg(source).arg("-o").arg(&out);
    match diagram {
        Some(d) => cmd.arg("--diagram").arg(d),
        None => cmd.arg("--all"),
    };
    let messages = run(cmd)?;
    let text = read_text(&out)?;
    Ok(Conversion { text, messages })
}

/// Exporte le texte courant vers un nouveau fichier .simp (expérimental).
pub fn export_simp(text: &str, target: &Path) -> Result<Vec<String>, String> {
    if target.exists() {
        return Err(format!("la sortie existe déjà : {}", target.display()));
    }
    let dir = TempDir::new()?;
    let src = dir.0.join("export.puml");
    fs::write(&src, text).map_err(|e| format!("{} : {e}", src.display()))?;
    let mut cmd = converter();
    cmd.arg(&src).arg("-o").arg(target);
    run(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_new_never_overwrites() {
        let dir = TempDir::new().unwrap();
        let p = dir.0.join("a.svg");
        write_new(&p, "1").unwrap();
        assert!(write_new(&p, "2").is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), "1");
        save_document(&p, "3").unwrap();
        assert_eq!(read_text(&p).unwrap(), "3");
        let leftovers = fs::read_dir(&dir.0).unwrap().count();
        assert_eq!(leftovers, 1);
    }

    #[test]
    fn import_simp_keeps_every_diagram() {
        // projet synthétique à deux diagrammes (pas un fichier enregistré par l'application)
        let dir = TempDir::new().unwrap();
        let source = dir.0.join("deux.simp");
        let view = |id: &str, class: &str| {
            format!(
                r#"<diagram id="{id}" name="{id}" type="uml-component"><abstract-items><item id="{class}" type="component" name="{class}"/></abstract-items><layer/></diagram>"#
            )
        };
        let project = format!(
            r#"<sim-project version="12.75" multi-file="no"><items/><models><model><abstract-items/>{}{}</model></models></sim-project>"#,
            view("v1", "A"),
            view("v2", "B")
        );
        fs::write(&source, project).unwrap();
        match import_simp(&source, None) {
            Ok(conv) => assert_eq!(conv.text.matches("@startuml").count(), 2, "{}", conv.text),
            Err(e) if e.contains("introuvable") || e.contains("No module named") => {
                eprintln!("convertisseur simp2puml absent : test ignoré ({e})");
            }
            Err(e) => panic!("{e}"),
        }
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1, "rien n'est écrit près de la source");
    }

    #[test]
    fn read_strips_bom() {
        let dir = TempDir::new().unwrap();
        let p = dir.0.join("b.puml");
        fs::write(&p, b"\xEF\xBB\xBF@startuml").unwrap();
        assert_eq!(read_text(&p).unwrap(), "@startuml");
    }
}
