//! Service : lecture des fichiers inclus, bornée au dossier du document.
//!
//! Le chemin réel (liens et `..` résolus) doit rester dans ce dossier : un lien
//! symbolique ne permet pas d'en sortir. Seuls les fichiers ordinaires sont lus,
//! avec la même limite de taille et le même contrôle UTF-8 que les documents.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model::preflight::IncludeSource;
use crate::services::file_service;

pub struct DocumentFolder {
    root: PathBuf,
}

impl DocumentFolder {
    /// `None` si le dossier n'existe pas ou plus.
    pub fn new(document: &Path) -> Option<(Self, PathBuf)> {
        let parent = document.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let root = fs::canonicalize(parent).ok()?;
        Some((Self { root: root.clone() }, root))
    }
}

impl IncludeSource for DocumentFolder {
    fn read(&self, target: &str, from: &Path) -> Result<(PathBuf, String), String> {
        let wanted = Path::new(target);
        if wanted.is_absolute() {
            return Err("chemin absolu refusé : indiquer un chemin relatif au document".into());
        }
        let path = fs::canonicalize(from.join(wanted)).map_err(|_| "fichier introuvable".to_string())?;
        if !path.starts_with(&self.root) {
            return Err("hors du dossier du document : refusé".into());
        }
        let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err("pas un fichier ordinaire".into());
        }
        let text = file_service::read_text(&path)?;
        Ok((path, text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::preferences::SecurityProfile;
    use crate::model::preflight::prepare;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("puml-editor-include-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("doc/sous")).unwrap();
        dir
    }

    #[test]
    fn only_the_document_folder_is_readable() {
        let dir = scratch("borne");
        fs::write(dir.join("secret.txt"), "SECRET").unwrap();
        fs::write(dir.join("doc/a.puml"), "class Voisin").unwrap();
        fs::write(dir.join("doc/sous/b.puml"), "class Sous\n!include ../a.puml").unwrap();
        let document = dir.join("doc/diagramme.puml");
        let (source, root) = DocumentFolder::new(&document).unwrap();
        let run = |text: &str| prepare(text, SecurityProfile::Folder, Some(&root), &source);

        // relatif, y compris depuis un sous-dossier : chaque inclusion part de son propre fichier
        let ok = run("!include sous/b.puml");
        assert!(ok.text.contains("class Sous") && ok.text.contains("class Voisin"), "{:?}", ok);
        assert!(ok.diagnostics.is_empty());

        for attempt in ["!include ../secret.txt", "!include sous/../../secret.txt"] {
            let p = run(attempt);
            assert!(!p.text.contains("SECRET"), "{attempt}");
            assert!(p.diagnostics[0].message.contains("hors du dossier"), "{:?}", p.diagnostics);
        }
        let absolute = format!("!include {}", dir.join("secret.txt").display());
        assert!(run(&absolute).diagnostics[0].message.contains("absolu"));
        assert!(run("!include sous").diagnostics[0].message.contains("pas un fichier"));
        assert!(run("!include absent").diagnostics[0].message.contains("introuvable"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_leave_the_folder() {
        let dir = scratch("lien");
        fs::write(dir.join("secret.txt"), "SECRET").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.txt"), dir.join("doc/lien.puml")).unwrap();
        std::os::unix::fs::symlink(dir.join("doc"), dir.join("doc/boucle")).unwrap();
        let (source, root) = DocumentFolder::new(&dir.join("doc/d.puml")).unwrap();
        let p = prepare("!include lien.puml", SecurityProfile::Folder, Some(&root), &source);
        assert!(!p.text.contains("SECRET"));
        assert!(p.diagnostics[0].message.contains("hors du dossier"));
        // un lien qui reste dans le dossier est suivi normalement
        fs::write(dir.join("doc/v.puml"), "class V").unwrap();
        let inside = prepare("!include boucle/v.puml", SecurityProfile::Folder, Some(&root), &source);
        assert!(inside.text.contains("class V"), "{:?}", inside);
        let _ = fs::remove_dir_all(&dir);
    }
}
