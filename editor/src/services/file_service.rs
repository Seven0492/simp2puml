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

/// Synchronise le dossier après un renommage (best effort).
fn sync_parent(path: &Path) {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
}

/// Enregistre le document édité (remplacement atomique du fichier ouvert).
/// Un lien symbolique est suivi : c'est sa cible qui est remplacée, et le lien
/// reste un lien. Les droits du fichier existant sont conservés ; un fichier en
/// lecture seule est refusé plutôt que remplacé.
pub fn save_document(path: &Path, content: &str) -> Result<(), String> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = fs::metadata(&target).ok();
    if let Some(meta) = &existing {
        if meta.permissions().readonly() {
            return Err(format!("{} : fichier en lecture seule", path.display()));
        }
    }
    let tmp = write_temp(&target, content)?;
    let cleanup = |e: String| {
        let _ = fs::remove_file(&tmp);
        e
    };
    if let Some(meta) = &existing {
        fs::set_permissions(&tmp, meta.permissions()).map_err(|e| cleanup(format!("{} : {e}", tmp.display())))?;
    }
    fs::rename(&tmp, &target).map_err(|e| cleanup(format!("{} : {e}", path.display())))?;
    sync_parent(&target);
    Ok(())
}

/// Empreinte d'un fichier sur le disque (date et taille) : permet de voir qu'un
/// autre programme l'a modifié depuis l'ouverture ou le dernier enregistrement.
pub fn disk_stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Écrit une nouvelle sortie ; refuse d'écraser un fichier existant.
pub fn write_new(path: &Path, content: &str) -> Result<(), String> {
    if path.exists() || path.is_symlink() {
        return Err(format!("la sortie existe déjà : {}", path.display()));
    }
    let tmp = write_temp(path, content)?;
    // hard_link échoue si la cible est apparue entre-temps : jamais d'écrasement
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => {
            sync_parent(path);
            Ok(())
        }
        // FAT, exFAT, certains montages réseau : pas de liens durs. Création
        // exclusive directe (O_EXCL) : toujours sans écrasement.
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported || matches!(e.raw_os_error(), Some(1 | 95)) => {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("{} : {e}", path.display()))?;
            f.write_all(content.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| {
                let _ = fs::remove_file(path);
                format!("{} : {e}", path.display())
            })
        }
        Err(e) => Err(format!("{} : {e}", path.display())),
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!("puml-editor-{}-{nanos}", std::process::id()));
        // dossier privé : le texte du diagramme n'est pas lisible par les autres comptes
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&p).map_err(|e| format!("{} : {e}", p.display()))?;
        Ok(Self(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Commande du convertisseur `simp2puml`.
fn converter() -> Command {
    package_tool("SIMP2PUML", "simp2puml", "simp2puml")
}

/// Commande du paquet Python : variable d'environnement, à côté de l'exécutable,
/// dans le PATH (entrées absolues seulement), sinon le module Python.
pub fn package_tool(env: &str, script: &str, module: &str) -> Command {
    if let Some(p) = std::env::var_os(env) {
        return Command::new(p);
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(script)));
    let in_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).map(|d| d.join(script)).collect::<Vec<_>>())
        .unwrap_or_default();
    match beside.into_iter().chain(in_path).find(|c| c.is_file()) {
        Some(found) => Command::new(found),
        None => python_module(module),
    }
}

/// `python -P -m module` : le Python de l'interface classique s'il a été transmis
/// ($SIMP2PUML_PYTHON), sinon `python3`, avec src/ du dépôt si présent (`-P` évite
/// d'importer le dossier courant).
pub fn python_module(module: &str) -> Command {
    let python = std::env::var_os("SIMP2PUML_PYTHON").unwrap_or_else(|| "python3".into());
    let mut cmd = Command::new(python);
    cmd.arg("-P").arg("-m").arg(module);
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..5 {
            let Some(d) = dir else { break };
            let src = d.join("src").join("simp2puml");
            if src.join("__init__.py").is_file() {
                // l'aiguilleur peut déjà avoir désigné le paquet en cours d'exécution
                if std::env::var_os("PYTHONPATH").is_none() {
                    cmd.env("PYTHONPATH", d.join("src"));
                }
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

/// Message d'échec du convertisseur, sur une ligne : l'erreur de la ligne de
/// commande sans l'usage complet qui la précède.
fn failure_message(stderr: &str) -> Option<String> {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if let Some(message) = lines.iter().find_map(|l| l.split_once(": error: ").map(|(_, m)| m)) {
        return Some(message.to_string());
    }
    (!lines.is_empty()).then(|| lines.join(" | "))
}

/// Lance le convertisseur ; ses messages (dont les avertissements) sont sur stderr.
fn run(mut cmd: Command) -> Result<Vec<String>, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("convertisseur simp2puml introuvable ({e}) ; installez-le ou définissez SIMP2PUML"))?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(failure_message(&stderr).unwrap_or_else(|| format!("échec de la conversion ({})", out.status)));
    }
    Ok(stderr.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect())
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
    fn converter_failure_keeps_only_the_error_line() {
        let usage = "usage: simp2puml [-h]\n                 source\nsimp2puml: error: l'extension de sortie doit être .simp\n";
        assert_eq!(failure_message(usage).as_deref(), Some("l'extension de sortie doit être .simp"));
        assert_eq!(failure_message("Erreur : Ligne 5\n\nautre\n").as_deref(), Some("Erreur : Ligne 5 | autre"));
        assert_eq!(failure_message(" \n"), None);
    }

    /// Premier enregistrement des préférences : le fichier n'existe pas encore.
    #[test]
    fn save_creates_a_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.0.join("preferences.conf");
        save_document(&path, "theme = dark\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "theme = dark\n");
        save_document(&path, "theme = light\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "theme = light\n");
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1, "aucun fichier temporaire ne doit rester");
    }

    #[cfg(unix)]
    #[test]
    fn save_keeps_links_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = TempDir::new().unwrap();
        assert_eq!(fs::metadata(&dir.0).unwrap().permissions().mode() & 0o777, 0o700);
        let target = dir.0.join("cible.puml");
        let link = dir.0.join("lien.puml");
        fs::write(&target, "a").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        symlink(&target, &link).unwrap();
        save_document(&link, "b").unwrap();
        assert!(link.is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "b");
        assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
        fs::set_permissions(&target, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(save_document(&target, "c").is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "b");
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        // un lien pendant n'est jamais remplacé par une nouvelle sortie
        let dangling = dir.0.join("pendant.svg");
        symlink(dir.0.join("absent"), &dangling).unwrap();
        assert!(write_new(&dangling, "x").is_err());
        assert!(!dir.0.join("absent").exists());
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
