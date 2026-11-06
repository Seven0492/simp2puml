//! Vue : couleurs, polices et dimensions de l'interface, en un seul endroit.
//!
//! Deux palettes (claire et sombre). La palette en vigueur est choisie par la
//! vue de l'espace de travail à chaque rendu, d'après les préférences et
//! l'apparence du système (`set_appearance`) ; les autres vues ne font que lire.

use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Hsla, Rgba, rgb, rgba};

use crate::model::diagram::Severity;
use crate::model::syntax::Token;

pub const UI_FONT: &str = ".SystemUIFont";
pub const CODE_FONT: &str = "DejaVu Sans Mono";
pub const DIAGRAM_FONT: &str = "DejaVu Sans";
pub const DIAGRAM_MONO_FONT: &str = "DejaVu Sans Mono";
pub const CODE_SIZE: f32 = 14.0;

static DARK: AtomicBool = AtomicBool::new(false);
static ADAPT_DIAGRAM: AtomicBool = AtomicBool::new(false);

/// Palette sombre ou claire ; `adapt_diagram` : couleurs du diagramme
/// adaptées au thème sombre (sinon le diagramme reste sur une feuille claire).
pub fn set_appearance(dark: bool, adapt_diagram: bool) {
    DARK.store(dark, Ordering::Relaxed);
    ADAPT_DIAGRAM.store(adapt_diagram, Ordering::Relaxed);
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

fn pick(light: u32, dark: u32) -> Rgba {
    rgb(if is_dark() { dark } else { light })
}

fn pick_a(light: u32, dark: u32) -> Rgba {
    rgba(if is_dark() { dark } else { light })
}

pub fn text() -> Rgba { pick(0x1f2430, 0xd6dae3) }
pub fn muted() -> Rgba { pick(0x6b7280, 0x9aa3b2) }
pub fn faint() -> Rgba { pick(0xa0a7b4, 0x6b7383) }
pub fn border() -> Rgba { pick(0xdde2ec, 0x353b48) }
pub fn surface() -> Rgba { pick(0xffffff, 0x1e2229) }
pub fn panel() -> Rgba { pick(0xf4f6fa, 0x252a33) }
pub fn tab_bar() -> Rgba { pick(0xe9edf4, 0x1a1d23) }
pub fn hover() -> Rgba { pick(0xe4e8f1, 0x323946) }
pub fn pressed() -> Rgba { pick(0xd5dbe8, 0x3d4555) }
pub fn accent() -> Rgba { pick(0x3a6ea5, 0x5b9bd5) }
pub fn on_accent() -> Rgba { pick(0xffffff, 0x10141a) }
pub fn canvas() -> Rgba { pick(0xeceff4, 0x16191e) }
pub fn gutter() -> Rgba { pick(0xf7f8fb, 0x1e2229) }
pub fn current_line() -> Rgba { pick(0xf3f5fa, 0x262b35) }
pub fn cursor() -> Rgba { pick(0x1d5fc4, 0x7aaef0) }
pub fn selection() -> Rgba { pick_a(0x3a6ea540, 0x5b9bd555) }
pub fn search_match() -> Rgba { pick_a(0xffcc0066, 0xd4a01a66) }
pub fn error_line() -> Rgba { pick_a(0xd32f2f14, 0xe0606026) }
pub fn region_hover() -> Rgba { pick_a(0x3a6ea524, 0x5b9bd533) }
pub fn warning_bar() -> Rgba { pick(0xfff8e1, 0x3a3220) }
pub fn warning_border() -> Rgba { pick(0xe8d9a8, 0x5c4d2a) }
pub fn error_banner() -> Rgba { pick(0xfdecea, 0x3d2224) }
pub fn field_border() -> Rgba { pick(0xc9d0dd, 0x434a59) }
pub fn completion_selected() -> Rgba { pick(0xdbe6f6, 0x2f4766) }
pub fn secondary_bar() -> Rgba { pick(0xeef2f8, 0x222731) }
pub fn diagnostics_bg() -> Rgba { pick(0xfbfcfe, 0x1b1f25) }
pub fn row_hover() -> Rgba { pick(0xeef2f8, 0x2a303b) }
pub fn status_text() -> Rgba { pick(0x4b5563, 0xa9b1bf) }
pub fn splitter_hover() -> Rgba { pick(0xb8c3d6, 0x4a5568) }
pub fn sheet_shadow() -> Rgba { pick_a(0x0000000d, 0x00000059) }

/// Fond de la feuille du diagramme.
pub fn sheet() -> Rgba {
    if is_dark() && ADAPT_DIAGRAM.load(Ordering::Relaxed) { rgb(0x23272f) } else { rgb(0xffffff) }
}

/// Couleur d'un élément du diagramme : telle quelle, ou, en thème sombre
/// adapté, avec la luminosité inversée (teinte et saturation conservées) :
/// le noir devient clair, les fonds pastel deviennent sombres.
pub fn diagram(c: Hsla) -> Hsla {
    if !(is_dark() && ADAPT_DIAGRAM.load(Ordering::Relaxed)) {
        return c;
    }
    Hsla { l: 0.12 + (1.0 - c.l) * 0.78, s: c.s * 0.8, ..c }
}

pub fn severity(sev: Severity) -> Rgba {
    match sev {
        Severity::Error => pick(0xd32f2f, 0xf07178),
        Severity::Warning => pick(0xb7791f, 0xe6b450),
        Severity::Info => pick(0x3a6ea5, 0x6cb6ff),
    }
}

pub fn severity_icon(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "✖",
        Severity::Warning => "▲",
        Severity::Info => "ℹ",
    }
}

pub fn token(t: Token) -> Hsla {
    let (light, dark) = match t {
        Token::Plain => (0x1f2430, 0xd6dae3),
        Token::Comment => (0x7b8494, 0x7f8899),
        Token::Directive => (0xb3261e, 0xf07178),
        Token::Preprocessor => (0x8e3fb5, 0xc792ea),
        Token::Keyword => (0x1d5fc4, 0x82aaff),
        Token::Arrow => (0xc0560c, 0xf78c6c),
        Token::Str => (0x1c7c3a, 0xc3e88d),
        Token::Stereotype => (0x0e7490, 0x89ddff),
        Token::Color => (0xa15c00, 0xffcb6b),
        Token::Label => (0x3b4252, 0xb8c0cc),
        Token::Bracket => (0x6b7280, 0x9aa3b2),
    };
    pick(light, dark).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapted_diagram_inverts_lightness_only_in_dark_mode() {
        let black: Hsla = rgb(0x000000).into();
        set_appearance(false, true);
        assert_eq!(diagram(black), black);
        set_appearance(true, true);
        assert!(diagram(black).l > 0.8);
        set_appearance(true, false);
        assert_eq!(diagram(black), black);
        set_appearance(false, false);
    }
}
