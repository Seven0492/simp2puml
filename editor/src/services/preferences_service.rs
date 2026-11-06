//! Service : lecture et enregistrement des préférences, communes aux deux
//! interfaces. Même emplacement que `src/simp2puml/preferences.py` :
//! `$XDG_CONFIG_HOME/simp2puml/preferences.conf`, sinon le dossier de
//! configuration habituel du système.

use std::path::PathBuf;

use crate::model::preferences::Preferences;
use crate::services::file_service;

pub fn path() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).map(PathBuf::from);
    let home = || var("HOME").or_else(|| var("USERPROFILE"));
    let base = var("XDG_CONFIG_HOME").filter(|p| p.is_absolute()).or_else(|| {
        if cfg!(windows) {
            var("APPDATA").or_else(|| home().map(|h| h.join("AppData").join("Roaming")))
        } else if cfg!(target_os = "macos") {
            home().map(|h| h.join("Library").join("Application Support"))
        } else {
            home().map(|h| h.join(".config"))
        }
    })?;
    Some(base.join("simp2puml").join("preferences.conf"))
}

/// Préférences enregistrées (valeurs par défaut si le fichier n'existe pas),
/// et un message si une partie n'a pas pu être lue.
pub fn load() -> (Preferences, Option<String>) {
    let Some(path) = path() else { return (Preferences::default(), None) };
    if !path.exists() {
        return (Preferences::default(), None);
    }
    match file_service::read_text(&path) {
        Ok(text) => {
            let (p, ignored) = Preferences::parse(&text);
            let msg = (!ignored.is_empty())
                .then(|| format!("Préférences : réglages ignorés dans {} ({})", path.display(), ignored.join(", ")));
            (p, msg)
        }
        Err(e) => (Preferences::default(), Some(format!("Préférences illisibles, valeurs par défaut utilisées : {e}"))),
    }
}

pub fn save(prefs: &Preferences) -> Result<(), String> {
    let path = path().ok_or("dossier de configuration introuvable (HOME non défini)")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{} : {e}", dir.display()))?;
    }
    file_service::save_document(&path, &prefs.to_text())
}
