//! Modèle : préférences de l'éditeur et leur format texte (`clé = valeur`).
//!
//! Le format est volontairement simple et tolérant : une clé inconnue ou une
//! valeur invalide est signalée puis ignorée, sans perdre les autres réglages.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    /// suivre l'apparence du système
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Système",
            ThemeChoice::Light => "Clair",
            ThemeChoice::Dark => "Sombre",
        }
    }

    fn key(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    /// Thème sombre effectif, d'après l'apparence du système.
    pub fn is_dark(self, system_dark: bool) -> bool {
        match self {
            ThemeChoice::System => system_dark,
            ThemeChoice::Light => false,
            ThemeChoice::Dark => true,
        }
    }
}

/// Interface graphique ouverte au lancement ; le choix est commun aux deux
/// interfaces (`src/simp2puml/preferences.py` lit et écrit la même clé).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterfaceChoice {
    /// la nouvelle interface (cet éditeur)
    Editor,
    /// l'interface classique, en Tkinter
    Classic,
}

impl InterfaceChoice {
    const ALL: [InterfaceChoice; 2] = [InterfaceChoice::Editor, InterfaceChoice::Classic];

    fn key(self) -> &'static str {
        match self {
            InterfaceChoice::Editor => "editor",
            InterfaceChoice::Classic => "classic",
        }
    }
}

/// Moteur de l'aperçu de cette interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewEngine {
    /// `pumlrender` : instantané, sans dépendance, rendu approché
    Native,
    /// PlantUML : fidèle, à installer (Java ou binaire natif)
    Official,
}

impl PreviewEngine {
    const ALL: [PreviewEngine; 2] = [PreviewEngine::Native, PreviewEngine::Official];

    fn key(self) -> &'static str {
        match self {
            PreviewEngine::Native => "native",
            PreviewEngine::Official => "official",
        }
    }
}

/// Où une flèche réagit au clic. Choix temporaire, le temps de comparer les deux.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkClick {
    /// Sur toute sa longueur.
    Along,
    /// Seulement au départ et à la pointe.
    Ends,
}

impl LinkClick {
    const ALL: [LinkClick; 2] = [LinkClick::Along, LinkClick::Ends];

    fn key(self) -> &'static str {
        match self {
            LinkClick::Along => "along",
            LinkClick::Ends => "ends",
        }
    }
}

/// Ce qu'un diagramme a le droit de faire ; appliqué avant le rendu (`model/preflight.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityProfile {
    /// Ni préprocesseur ni fichier.
    None,
    /// `!define`, variables, conditions et procédures, sans accès aux fichiers.
    Macros,
    /// En plus, `!include` dans le dossier du document et ses sous-dossiers.
    Folder,
}

impl SecurityProfile {
    pub const ALL: [SecurityProfile; 3] = [SecurityProfile::None, SecurityProfile::Macros, SecurityProfile::Folder];

    pub fn label(self) -> &'static str {
        match self {
            SecurityProfile::None => "Aucun",
            SecurityProfile::Macros => "Macros",
            SecurityProfile::Folder => "Dossier du document",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            SecurityProfile::None => "Préprocesseur désactivé : !define, variables et !include sont ignorés et signalés",
            SecurityProfile::Macros => "!define, variables, conditions et procédures ; aucun fichier n'est lu",
            SecurityProfile::Folder => "Macros, et !include limité au dossier du document et à ses sous-dossiers",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SecurityProfile::None => "none",
            SecurityProfile::Macros => "macros",
            SecurityProfile::Folder => "folder",
        }
    }
}

pub const MIN_CODE_SIZE: u32 = 10;
pub const MAX_CODE_SIZE: u32 = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub interface: InterfaceChoice,
    pub preview_engine: PreviewEngine,
    pub security_profile: SecurityProfile,
    pub link_click: LinkClick,
    pub theme: ThemeChoice,
    /// En thème sombre, adapter les couleurs du diagramme (sinon feuille claire).
    pub adapt_diagram: bool,
    /// Ouvrir l'autocomplétion pendant la frappe (Ctrl+Espace reste disponible).
    pub autocomplete: bool,
    /// Taille du texte de l'éditeur, en points.
    pub code_size: u32,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            interface: InterfaceChoice::Editor,
            preview_engine: PreviewEngine::Native,
            security_profile: SecurityProfile::Macros,
            link_click: LinkClick::Along,
            theme: ThemeChoice::System, adapt_diagram: false, autocomplete: true,
            code_size: 14,
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "oui" | "1" => Some(true),
        "false" | "non" | "0" => Some(false),
        _ => None,
    }
}

impl Preferences {
    /// Lit le format texte ; renvoie aussi les lignes ignorées (avec leur numéro).
    pub fn parse(text: &str) -> (Preferences, Vec<String>) {
        let mut p = Preferences::default();
        let mut ignored = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                ignored.push(format!("ligne {} : « {line} »", i + 1));
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let ok = match k {
                "interface" => InterfaceChoice::ALL.iter().find(|c| c.key() == v).map(|c| p.interface = *c).is_some(),
                "preview_engine" => PreviewEngine::ALL.iter().find(|c| c.key() == v).map(|c| p.preview_engine = *c).is_some(),
                "link_click" => LinkClick::ALL.iter().find(|c| c.key() == v).map(|c| p.link_click = *c).is_some(),
                "security_profile" => SecurityProfile::ALL.iter().find(|c| c.key() == v).map(|c| p.security_profile = *c).is_some(),
                "theme" => ThemeChoice::ALL.iter().find(|t| t.key() == v).map(|t| p.theme = *t).is_some(),
                "adapt_diagram" => parse_bool(v).map(|b| p.adapt_diagram = b).is_some(),
                "autocomplete" => parse_bool(v).map(|b| p.autocomplete = b).is_some(),
                "code_size" => v
                    .parse()
                    .ok()
                    .filter(|n| (MIN_CODE_SIZE..=MAX_CODE_SIZE).contains(n))
                    .map(|n| p.code_size = n)
                    .is_some(),
                _ => false,
            };
            if !ok {
                ignored.push(format!("ligne {} : « {line} »", i + 1));
            }
        }
        (p, ignored)
    }

    pub fn to_text(&self) -> String {
        format!(
            "# Préférences de simp2puml, communes aux deux interfaces\n\
             # interface : editor (nouvelle) ou classic\n\
             interface = {}\n\
             # moteur d'aperçu de la nouvelle interface : native ou official\n\
             preview_engine = {}\n\
             # none : pas de préprocesseur ; macros : !define et variables ;\n\
             # folder : en plus, !include limité au dossier du document\n\
             security_profile = {}\n\
             # clic sur une flèche : along (toute la longueur) ou ends (les bouts)\n\
             link_click = {}\n\
             # theme : system, light ou dark\n\
             theme = {}\n\
             # en thème sombre, adapter les couleurs du diagramme (sinon feuille claire)\n\
             adapt_diagram = {}\n\
             # ouvrir l'autocomplétion pendant la frappe (Ctrl+Espace reste disponible)\n\
             autocomplete = {}\n\
             # taille du texte de l'éditeur, de {MIN_CODE_SIZE} à {MAX_CODE_SIZE}\n\
             code_size = {}\n",
            self.interface.key(),
            self.preview_engine.key(),
            self.security_profile.key(),
            self.link_click.key(),
            self.theme.key(),
            self.adapt_diagram,
            self.autocomplete,
            self.code_size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let p = Preferences { interface: InterfaceChoice::Classic, preview_engine: PreviewEngine::Official, security_profile: SecurityProfile::Folder, link_click: LinkClick::Ends, theme: ThemeChoice::Dark, adapt_diagram: true, autocomplete: false, code_size: 18 };
        assert_eq!(Preferences::parse(&p.to_text()), (p, vec![]));
        assert_eq!(Preferences::parse("").0, Preferences::default());
    }

    #[test]
    fn invalid_lines_are_reported_and_ignored() {
        let (p, ignored) = Preferences::parse("theme = violet\nautocomplete = non\nbizarre\nzoom = 2\n");
        assert_eq!(p.theme, ThemeChoice::System);
        assert!(!p.autocomplete);
        assert_eq!(ignored.len(), 3);
        assert!(ignored[0].starts_with("ligne 1"));
    }

    /// Fichier lu aussi par les tests Python : les deux interfaces s'accordent sur le format.
    #[test]
    fn shared_fixture_is_read_like_the_python_interface() {
        let text = include_str!("../../../tests/data/preferences.conf");
        let (p, ignored) = Preferences::parse(text);
        assert_eq!(ignored, Vec::<String>::new());
        assert_eq!(p.interface, InterfaceChoice::Classic);
        assert_eq!(p.preview_engine, PreviewEngine::Official);
        assert_eq!(p.security_profile, SecurityProfile::Folder);
        assert_eq!(p.theme, ThemeChoice::Dark);
        assert_eq!(p.code_size, 18);
    }

    #[test]
    fn safe_defaults() {
        let p = Preferences::default();
        assert_eq!(p.preview_engine, PreviewEngine::Native);
        assert_eq!(p.security_profile, SecurityProfile::Macros, "aucun fichier lu sans le demander");
        assert_eq!(Preferences::parse("security_profile = tout\n").0.security_profile, SecurityProfile::Macros);
    }

    #[test]
    fn new_interface_is_the_default() {
        assert_eq!(Preferences::default().interface, InterfaceChoice::Editor);
        assert_eq!(Preferences::parse("interface = violette\n").0.interface, InterfaceChoice::Editor);
    }

    #[test]
    fn code_size_out_of_range_is_ignored() {
        let (p, ignored) = Preferences::parse("code_size = 16\ncode_size = 99\ncode_size = x\n");
        assert_eq!(p.code_size, 16);
        assert_eq!(ignored.len(), 2);
    }

    #[test]
    fn system_theme_follows_the_system() {
        assert!(ThemeChoice::System.is_dark(true));
        assert!(!ThemeChoice::System.is_dark(false));
        assert!(ThemeChoice::Dark.is_dark(false));
        assert!(!ThemeChoice::Light.is_dark(true));
    }
}
