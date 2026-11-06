//! Modèle : préparation du texte avant le rendu, selon le profil de sécurité.
//!
//! C'est ici, et non dans les moteurs, que le profil s'applique : les deux moteurs
//! reçoivent le même texte et aucun n'a à lire de fichier. Les lignes ajoutées
//! par une inclusion sont rattachées à la ligne du `!include`, pour que
//! diagnostics et clics dans l'aperçu renvoient au document ouvert.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::model::diagram::{Diagnostic, RenderOutput, Severity};
use crate::model::preferences::SecurityProfile;

const MAX_DEPTH: usize = 16;
const MAX_FILES: usize = 256;
const MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;

/// Accès aux fichiers, fourni par `services::include_service` (bornage au dossier).
pub trait IncludeSource {
    /// Résout `target` depuis `from` ; renvoie le chemin réel (clé des inclusions déjà faites) et le texte.
    fn read(&self, target: &str, from: &Path) -> Result<(PathBuf, String), String>;
}

/// Pas de fichier accessible : profils sans inclusion et tests.
pub struct NoFiles;

impl IncludeSource for NoFiles {
    fn read(&self, _: &str, _: &Path) -> Result<(PathBuf, String), String> {
        Err("aucun fichier accessible".into())
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Prepared {
    pub text: String,
    /// Ligne du document (à partir de 1) de chaque ligne de `text`.
    lines: Vec<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Prepared {
    /// Ramène diagnostics et zones cliquables aux lignes du document, puis ajoute les messages de préparation.
    pub fn apply(&self, output: &mut RenderOutput) {
        let map = |line: usize| {
            if line == 0 { 0 } else { self.lines.get(line - 1).copied().unwrap_or(line) }
        };
        for d in &mut output.diagnostics {
            d.line = map(d.line);
        }
        for r in &mut output.regions {
            r.line = map(r.line);
        }
        let mut all = self.diagnostics.clone();
        all.append(&mut output.diagnostics);
        output.diagnostics = all;
    }
}

struct Run<'a> {
    profile: SecurityProfile,
    source: &'a dyn IncludeSource,
    base: Option<&'a Path>,
    out: Prepared,
    seen: HashSet<PathBuf>,
    bytes: usize,
    files: usize,
}

#[derive(Debug, PartialEq)]
struct Include {
    target: String,
    selector: Option<String>,
    many: bool,
    sub: bool,
}

/// `!include f`, `!include f!1`, `!include_once f`, `!include_many f`, `!includesub f!NOM`.
/// `Err` : forme refusée (URL, bibliothèque, chemin vide). `None` : pas une inclusion.
fn parse_include(line: &str) -> Option<Result<Include, String>> {
    let t = line.trim();
    let rest = t.strip_prefix('!')?;
    let word: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    let lower = word.to_ascii_lowercase();
    if lower == "includeurl" || lower == "import" {
        return Some(Err(format!("!{word} : jamais chargé")));
    }
    if !matches!(lower.as_str(), "include" | "include_once" | "include_many" | "includesub") {
        return None;
    }
    let arg = rest[word.len()..].trim().trim_matches('"');
    if arg.is_empty() {
        return Some(Err(format!("!{word} sans fichier")));
    }
    if arg.starts_with('<') {
        return Some(Err(format!("!{word} {arg} : bibliothèque standard non disponible")));
    }
    if arg.contains("://") {
        return Some(Err(format!("!{word} : adresse Internet refusée")));
    }
    let (target, selector) = match arg.rsplit_once('!') {
        Some((f, s)) if !f.is_empty() => (f.to_string(), Some(s.to_string())),
        _ => (arg.to_string(), None),
    };
    Some(Ok(Include { target, selector, many: lower == "include_many", sub: lower == "includesub" }))
}

fn is_include_family(word: &str) -> bool {
    matches!(word, "include" | "include_once" | "include_many" | "includesub" | "includeurl" | "import")
}

/// Le contenu utile d'un fichier inclus : un bloc `@start…@end`, un `!startsub`, ou tout le texte.
fn select(content: &str, inc: &Include) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();
    let lower = |l: &str| l.trim().to_ascii_lowercase();
    if let Some(name) = inc.selector.as_deref().filter(|s| s.parse::<usize>().is_err()) {
        // bloc nommé (`!startsub`, ou `@startuml(id=NOM)`)
        let mut inside = false;
        let mut found = false;
        let mut out = Vec::new();
        for l in &lines {
            let t = l.trim();
            if t.strip_prefix("!startsub").map(str::trim) == Some(name) {
                inside = true;
                found = true;
            } else if inside && lower(t).starts_with("!endsub") {
                inside = false;
            } else if inside {
                out.push(*l);
            }
        }
        if found {
            return Ok(out.join("\n"));
        }
        if inc.sub {
            return Err(format!("sous-partie « {name} » introuvable"));
        }
        let start = lines.iter().position(|l| lower(l).starts_with("@start") && (l.contains(&format!("(id={name})")) || l.split_whitespace().nth(1) == Some(name)));
        let Some(start) = start else { return Err(format!("bloc « {name} » introuvable")) };
        let end = lines[start..].iter().position(|l| lower(l).starts_with("@end")).map_or(lines.len(), |e| start + e);
        return Ok(lines[start + 1..end].join("\n"));
    }
    let index = inc.selector.as_deref().and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| lower(l).starts_with("@start")).map(|(i, _)| i).collect();
    if starts.is_empty() {
        return Ok(strip_sub_marks(&lines));
    }
    let Some(&start) = starts.get(index) else { return Err(format!("pas de bloc n° {index}")) };
    let end = lines[start..].iter().position(|l| lower(l).starts_with("@end")).map_or(lines.len(), |e| start + e);
    Ok(strip_sub_marks(&lines[start + 1..end]))
}

fn strip_sub_marks(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|l| {
            let t = l.trim().to_ascii_lowercase();
            !t.starts_with("!startsub") && !t.starts_with("!endsub")
        })
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

impl Run<'_> {
    fn warn(&mut self, line: usize, message: String) {
        self.out.diagnostics.push(Diagnostic { line, severity: Severity::Warning, message });
    }

    fn emit(&mut self, text: &str, line: usize) {
        self.out.text.push_str(text);
        self.out.text.push('\n');
        self.out.lines.push(line);
    }

    /// `doc_line` : ligne du document à laquelle rattacher ce texte (0 : ses propres lignes).
    fn walk(&mut self, text: &str, dir: Option<&Path>, doc_line: usize, depth: usize) {
        let mut comment = false;
        let mut literal = false; // blocs JSON et YAML : pas de préprocesseur
        for (i, raw) in text.split('\n').enumerate() {
            let line = if doc_line > 0 { doc_line } else { i + 1 };
            let t = raw.trim();
            let lower = t.to_ascii_lowercase();
            if lower.starts_with("@start") {
                literal = lower.starts_with("@startjson") || lower.starts_with("@startyaml");
            } else if lower.starts_with("@end") {
                literal = false;
            }
            let was_comment = comment;
            if t.contains("/'") {
                comment = !t.contains("'/");
            } else if comment && t.contains("'/") {
                comment = false;
            }
            if literal || was_comment || !t.starts_with('!') {
                self.emit(raw, line);
                continue;
            }
            let word: String = t[1..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            let word = word.to_ascii_lowercase();
            if is_include_family(&word) {
                self.include(t, dir, line, depth);
                self.blank_if_root(doc_line, line);
            } else if self.profile == SecurityProfile::None {
                self.warn(line, format!("!{word} désactivé par le profil de sécurité « Aucun » (Préférences)"));
                self.blank_if_root(doc_line, line);
            } else if matches!(word.as_str(), "startsub" | "endsub") {
                self.blank_if_root(doc_line, line);
            } else {
                self.emit(raw, line);
            }
        }
    }

    /// Une ligne retirée du document reste une ligne vide : ses voisines gardent leur numéro.
    fn blank_if_root(&mut self, doc_line: usize, line: usize) {
        if doc_line == 0 {
            self.emit("", line);
        }
    }

    fn include(&mut self, directive: &str, dir: Option<&Path>, line: usize, depth: usize) {
        if self.profile != SecurityProfile::Folder {
            self.warn(line, "!include refusé par le profil de sécurité : choisir « Dossier du document » dans les Préférences".into());
            return;
        }
        let inc = match parse_include(directive) {
            Some(Ok(inc)) => inc,
            Some(Err(message)) => return self.warn(line, message),
            None => return,
        };
        let (Some(dir), Some(_)) = (dir, self.base) else {
            return self.warn(line, "!include : enregistrez d'abord le document, son dossier sert de référence".into());
        };
        if depth >= MAX_DEPTH {
            return self.warn(line, format!("!include : plus de {MAX_DEPTH} niveaux d'inclusion"));
        }
        if self.files >= MAX_FILES {
            return self.warn(line, format!("!include : plus de {MAX_FILES} fichiers inclus"));
        }
        let (path, content) = match self.source.read(&inc.target, dir) {
            Ok(found) => found,
            Err(e) => return self.warn(line, format!("!include {} : {e}", inc.target)),
        };
        // une boucle (A inclut B qui inclut A) s'arrête ici : chaque fichier une fois, sauf !include_many
        let key = match &inc.selector {
            Some(s) => path.join(format!("!{s}")),
            None => path.clone(),
        };
        if !self.seen.insert(key) && !inc.many {
            return;
        }
        self.files += 1;
        self.bytes += content.len();
        if self.bytes > MAX_TOTAL_BYTES {
            return self.warn(line, "!include : plus de 20 MiB inclus".into());
        }
        match select(&content, &inc) {
            Ok(selected) => {
                let parent = path.parent().map(Path::to_path_buf);
                self.walk(&selected, parent.as_deref(), line, depth + 1);
            }
            Err(e) => self.warn(line, format!("!include {} : {e}", inc.target)),
        }
    }
}

/// Prépare `text` selon le profil ; `base` : dossier du document, s'il a été enregistré.
pub fn prepare(text: &str, profile: SecurityProfile, base: Option<&Path>, source: &dyn IncludeSource) -> Prepared {
    let mut run = Run {
        profile,
        source,
        base,
        out: Prepared::default(),
        seen: HashSet::new(),
        bytes: text.len(),
        files: 0,
    };
    run.walk(text, base, 0, 0);
    run.out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::model::diagram::Region;

    /// Faux dossier : nom → contenu.
    struct Files(HashMap<&'static str, &'static str>);

    impl IncludeSource for Files {
        fn read(&self, target: &str, _: &Path) -> Result<(PathBuf, String), String> {
            match self.0.get(target) {
                Some(content) => Ok((PathBuf::from("/doc").join(target), content.to_string())),
                None => Err("fichier introuvable".into()),
            }
        }
    }

    fn files(list: &[(&'static str, &'static str)]) -> Files {
        Files(list.iter().copied().collect())
    }

    fn folder(text: &str, source: &Files) -> Prepared {
        prepare(text, SecurityProfile::Folder, Some(Path::new("/doc")), source)
    }

    #[test]
    fn profile_macros_keeps_defines_and_refuses_includes() {
        let p = prepare("@startuml\n!define A 1\n!include secret.puml\nA\n@enduml\n", SecurityProfile::Macros, Some(Path::new("/doc")), &NoFiles);
        assert!(p.text.contains("!define A 1"));
        assert!(!p.text.contains("secret"));
        assert_eq!(p.diagnostics.len(), 1);
        assert_eq!(p.diagnostics[0].line, 3);
        assert!(p.diagnostics[0].message.contains("Dossier du document"));
        assert_eq!(p.text.lines().nth(3), Some("A"), "les lignes gardent leur numéro");
    }

    #[test]
    fn profile_none_disables_the_preprocessor_except_in_data_blocks() {
        let p = prepare("@startuml\n!define A 1\n!theme plain\nclass B\n@enduml\n@startjson\n!x\n@endjson\n", SecurityProfile::None, None, &NoFiles);
        assert!(!p.text.contains("!define") && !p.text.contains("!theme"));
        assert_eq!(p.diagnostics.len(), 2);
        assert!(p.text.contains("class B") && p.text.contains("!x"));
    }

    #[test]
    fn include_inserts_the_file_and_maps_lines_back() {
        let src = files(&[("styles.iuml", "class Lib\nclass Lib2")]);
        let p = folder("@startuml\n!include styles.iuml\nclass Doc\n@enduml", &src);
        assert_eq!(p.text, "@startuml\nclass Lib\nclass Lib2\n\nclass Doc\n@enduml\n");
        assert!(p.diagnostics.is_empty());
        let mut out = RenderOutput {
            diagnostics: vec![Diagnostic { line: 3, severity: Severity::Warning, message: "x".into() }],
            regions: vec![Region { line: 5, x: 0.0, y: 0.0, w: 1.0, h: 1.0 }],
            ..RenderOutput::default()
        };
        p.apply(&mut out);
        assert_eq!(out.diagnostics[0].line, 2, "une ligne incluse renvoie au !include");
        assert_eq!(out.regions[0].line, 3, "une ligne du document garde son numéro");
    }

    #[test]
    fn included_blocks_and_subparts_are_selected() {
        let src = files(&[
            ("deux.puml", "@startuml\nclass Un\n@enduml\n@startuml\nclass Deux\n@enduml"),
            ("sub.puml", "class Hors\n!startsub PART\nclass Dans\n!endsub"),
        ]);
        assert!(folder("!include deux.puml", &src).text.contains("class Un"));
        assert!(!folder("!include deux.puml", &src).text.contains("class Deux"));
        assert!(folder("!include deux.puml!1", &src).text.contains("class Deux"));
        let sub = folder("!includesub sub.puml!PART", &src);
        assert!(sub.text.contains("class Dans") && !sub.text.contains("class Hors"));
        let whole = folder("!include sub.puml", &src);
        assert!(whole.text.contains("class Hors") && whole.text.contains("class Dans") && !whole.text.contains("!startsub"));
        assert_eq!(folder("!include deux.puml!9", &src).diagnostics.len(), 1);
        assert_eq!(folder("!includesub sub.puml!NON", &src).diagnostics.len(), 1);
    }

    #[test]
    fn cycles_and_repeats_stop() {
        let src = files(&[("a.puml", "class A\n!include b.puml"), ("b.puml", "class B\n!include a.puml")]);
        let p = folder("!include a.puml\n!include a.puml", &src);
        assert_eq!(p.text.matches("class A").count(), 1);
        assert_eq!(p.text.matches("class B").count(), 1);
        let many = folder("!include_many a.puml\n!include_many a.puml", &files(&[("a.puml", "class A")]));
        assert_eq!(many.text.matches("class A").count(), 2);
    }

    #[test]
    fn urls_library_and_missing_files_are_refused_with_a_message() {
        let src = files(&[]);
        for line in ["!includeurl http://x.example/a", "!include https://x.example/a", "!include <aws/common>", "!import x", "!include absent.puml", "!include"] {
            let p = folder(line, &src);
            assert_eq!(p.diagnostics.len(), 1, "{line}");
            assert_eq!(p.diagnostics[0].severity, Severity::Warning);
            assert_eq!(p.text.trim(), "", "{line}");
        }
    }

    #[test]
    fn unsaved_document_has_no_folder() {
        let p = prepare("!include a.puml", SecurityProfile::Folder, None, &files(&[("a.puml", "class A")]));
        assert!(p.diagnostics[0].message.contains("enregistrez"));
        assert!(!p.text.contains("class A"));
    }

    #[test]
    fn comments_are_not_preprocessed() {
        let p = prepare("/'\n!include a.puml\n'/\nclass A", SecurityProfile::Macros, None, &NoFiles);
        assert!(p.diagnostics.is_empty());
    }
}
