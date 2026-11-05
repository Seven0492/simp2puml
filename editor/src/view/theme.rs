//! Vue : couleurs, polices et dimensions de l'interface, en un seul endroit.

use gpui::{Hsla, Rgba, rgb};

use crate::model::diagram::Severity;
use crate::model::syntax::Token;

pub const UI_FONT: &str = ".SystemUIFont";
pub const CODE_FONT: &str = "DejaVu Sans Mono";
pub const DIAGRAM_FONT: &str = "DejaVu Sans";
pub const DIAGRAM_MONO_FONT: &str = "DejaVu Sans Mono";
pub const CODE_SIZE: f32 = 14.0;

pub fn text() -> Rgba { rgb(0x1f2430) }
pub fn muted() -> Rgba { rgb(0x6b7280) }
pub fn faint() -> Rgba { rgb(0xa0a7b4) }
pub fn border() -> Rgba { rgb(0xdde2ec) }
pub fn surface() -> Rgba { rgb(0xffffff) }
pub fn panel() -> Rgba { rgb(0xf4f6fa) }
pub fn tab_bar() -> Rgba { rgb(0xe9edf4) }
pub fn hover() -> Rgba { rgb(0xe4e8f1) }
pub fn pressed() -> Rgba { rgb(0xd5dbe8) }
pub fn accent() -> Rgba { rgb(0x3a6ea5) }
pub fn canvas() -> Rgba { rgb(0xeceff4) }
pub fn gutter() -> Rgba { rgb(0xf7f8fb) }
pub fn current_line() -> Rgba { rgb(0xf3f5fa) }
pub fn cursor() -> Rgba { rgb(0x1d5fc4) }
pub fn selection() -> Rgba { gpui::rgba(0x3a6ea540) }
pub fn search_match() -> Rgba { gpui::rgba(0xffcc0066) }
pub fn error_line() -> Rgba { gpui::rgba(0xd32f2f14) }
pub fn region_hover() -> Rgba { gpui::rgba(0x3a6ea524) }
pub fn warning_bar() -> Rgba { rgb(0xfff8e1) }

pub fn severity(sev: Severity) -> Rgba {
    match sev {
        Severity::Error => rgb(0xd32f2f),
        Severity::Warning => rgb(0xb7791f),
        Severity::Info => rgb(0x3a6ea5),
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
    rgb(match t {
        Token::Plain => 0x1f2430,
        Token::Comment => 0x7b8494,
        Token::Directive => 0xb3261e,
        Token::Preprocessor => 0x8e3fb5,
        Token::Keyword => 0x1d5fc4,
        Token::Arrow => 0xc0560c,
        Token::Str => 0x1c7c3a,
        Token::Stereotype => 0x0e7490,
        Token::Color => 0xa15c00,
        Token::Label => 0x3b4252,
        Token::Bracket => 0x6b7280,
    })
    .into()
}
