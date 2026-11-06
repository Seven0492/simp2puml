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

pub const MIN_CODE_SIZE: u32 = 10;
pub const MAX_CODE_SIZE: u32 = 28;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
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
        Self { theme: ThemeChoice::System, adapt_diagram: false, autocomplete: true, code_size: 14 }
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
            "# Préférences de l'éditeur simp2puml\n\
             # theme : system, light ou dark\n\
             theme = {}\n\
             # en thème sombre, adapter les couleurs du diagramme (sinon feuille claire)\n\
             adapt_diagram = {}\n\
             # ouvrir l'autocomplétion pendant la frappe (Ctrl+Espace reste disponible)\n\
             autocomplete = {}\n\
             # taille du texte de l'éditeur, de {MIN_CODE_SIZE} à {MAX_CODE_SIZE}\n\
             code_size = {}\n",
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
        let p = Preferences { theme: ThemeChoice::Dark, adapt_diagram: true, autocomplete: false, code_size: 18 };
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
