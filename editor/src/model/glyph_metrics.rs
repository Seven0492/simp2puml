//! Modèle : hauteur réelle des textes du diagramme une fois dessinés.
//!
//! GPUI ne donne pas la boîte des glyphes, et celle de la police réserve toujours la place
//! d'un jambage : un texte sans jambage paraîtrait décentré dans sa boîte de survol. Le
//! lissage cale de plus les glyphes sur la grille de pixels : à 13 px, une parenthèse monte
//! à 11 rangées au lieu des 9,9 annoncées par la police.
//!
//! La table vient donc d'une mesure, avec le moteur de rastérisation de GPUI (swash, lissage
//! actif) sur DejaVu Sans, police du diagramme ; oblique et mono donnent les mêmes valeurs,
//! et le gras s'en écarte d'au plus une rangée.

use crate::model::svg_scene::Text;

const FIRST_SIZE: usize = 6;

/// Rangées de pixels, par taille entière à partir de `FIRST_SIZE` : au-dessus de la ligne de
/// base pour a, d, T, t, « : » ; en dessous pour « ( », « { », g, « | », « , », « / ».
const ROWS: [[i32; 11]; 43] = [
    [3, 5, 4, 4, 3, 0, 1, 1, 1, 0, 1],
    [4, 6, 5, 5, 4, 0, 1, 1, 2, 1, 1],
    [4, 7, 6, 5, 4, 0, 1, 2, 2, 1, 1],
    [5, 8, 7, 6, 5, 0, 1, 2, 2, 1, 0],
    [5, 8, 7, 7, 5, 1, 1, 2, 2, 1, 1],
    [6, 9, 8, 8, 6, 1, 2, 2, 3, 1, 1],
    [7, 10, 9, 9, 6, 1, 2, 3, 3, 1, 1],
    [7, 11, 9, 9, 7, 1, 2, 3, 3, 1, 2],
    [8, 11, 10, 10, 7, 1, 2, 3, 3, 1, 2],
    [8, 11, 11, 10, 8, 2, 3, 3, 4, 2, 1],
    [9, 12, 12, 11, 8, 2, 3, 3, 4, 2, 1],
    [9, 13, 12, 12, 9, 2, 3, 4, 4, 2, 2],
    [10, 14, 13, 13, 9, 2, 3, 4, 4, 2, 2],
    [10, 14, 14, 13, 10, 3, 4, 4, 5, 3, 2],
    [11, 15, 15, 14, 10, 3, 3, 4, 5, 3, 1],
    [11, 16, 15, 14, 11, 3, 3, 4, 5, 2, 2],
    [12, 17, 16, 15, 11, 3, 3, 5, 5, 2, 2],
    [13, 17, 17, 17, 12, 4, 4, 5, 6, 3, 2],
    [13, 18, 18, 17, 12, 3, 4, 5, 6, 3, 2],
    [14, 19, 18, 18, 13, 3, 4, 5, 6, 3, 3],
    [14, 20, 19, 18, 13, 3, 4, 5, 6, 3, 2],
    [15, 21, 20, 19, 14, 3, 4, 6, 6, 3, 2],
    [15, 21, 20, 19, 14, 4, 5, 6, 7, 4, 3],
    [16, 22, 21, 21, 15, 4, 5, 6, 7, 3, 3],
    [16, 23, 22, 21, 16, 4, 5, 6, 7, 3, 3],
    [17, 24, 23, 22, 16, 4, 5, 6, 7, 3, 2],
    [18, 24, 23, 23, 17, 5, 6, 7, 8, 4, 3],
    [18, 25, 24, 23, 17, 4, 5, 7, 8, 4, 3],
    [19, 26, 25, 24, 18, 4, 5, 7, 8, 4, 3],
    [19, 27, 26, 24, 18, 4, 5, 7, 8, 4, 3],
    [20, 27, 26, 26, 19, 5, 6, 7, 9, 5, 4],
    [20, 28, 27, 26, 19, 5, 6, 8, 9, 4, 3],
    [22, 29, 28, 27, 20, 5, 6, 8, 9, 4, 3],
    [22, 30, 28, 27, 20, 5, 6, 8, 9, 4, 4],
    [23, 30, 29, 28, 21, 6, 7, 8, 10, 5, 4],
    [23, 31, 30, 28, 21, 6, 7, 9, 10, 5, 4],
    [24, 32, 31, 30, 22, 5, 7, 9, 10, 5, 4],
    [25, 33, 31, 31, 22, 5, 7, 9, 10, 5, 4],
    [25, 33, 32, 31, 23, 6, 8, 9, 11, 6, 4],
    [26, 34, 33, 32, 23, 6, 8, 9, 11, 5, 4],
    [26, 35, 34, 32, 24, 6, 7, 10, 11, 5, 4],
    [27, 36, 34, 33, 24, 6, 7, 10, 11, 5, 5],
    [27, 36, 35, 33, 25, 7, 8, 10, 12, 6, 4],
];

const X_HEIGHT: usize = 0;
const ASCENDER: usize = 1;
const CAPITAL: usize = 2;
const LETTER_T: usize = 3;
const COLON: usize = 4;
const BELOW_PAREN: usize = 5;
const BELOW_BRACE: usize = 6;
const BELOW_DESCENDER: usize = 7;
const BELOW_PIPE: usize = 8;
const BELOW_COMMA: usize = 9;
const BELOW_SLASH: usize = 10;

/// Colonnes du haut et du bas de ce caractère ; `None` : rien sous la ligne de base.
fn columns(c: char) -> (usize, Option<usize>) {
    match c {
        'a' | 'c' | 'e' | 'm' | 'n' | 'o' | 'r' | 's' | 'u' | 'v' | 'w' | 'x' | 'z' => (X_HEIGHT, None),
        'g' | 'p' | 'q' | 'y' | 'ç' => (X_HEIGHT, Some(BELOW_DESCENDER)),
        'j' => (ASCENDER, Some(BELOW_DESCENDER)),
        't' => (LETTER_T, None),
        '(' | ')' | '[' | ']' => (ASCENDER, Some(BELOW_PAREN)),
        '{' | '}' => (ASCENDER, Some(BELOW_BRACE)),
        '|' => (ASCENDER, Some(BELOW_PIPE)),
        '/' | '\\' => (CAPITAL, Some(BELOW_SLASH)),
        ':' | '.' | '-' | '=' | '+' | '~' | '<' | '>' | '*' => (COLON, None),
        ';' | ',' => (COLON, Some(BELOW_COMMA)),
        'Q' => (CAPITAL, Some(BELOW_PAREN)),
        '$' | '@' => (ASCENDER, Some(BELOW_PAREN)),
        '_' => (COLON, Some(BELOW_PIPE)),
        c if c.is_ascii_uppercase() || c.is_ascii_digit() => (CAPITAL, None),
        _ => (ASCENDER, None),
    }
}

/// Étendue verticale d'un texte (au-dessus, en dessous de la ligne de base), en unités du SVG.
pub fn text_extent(text: &Text, pixels_per_unit: f32) -> (f32, f32) {
    let pixels = text.size * pixels_per_unit;
    // au-delà de la table, le lissage ne déplace plus qu'une fraction de pixel : proportion
    let last = FIRST_SIZE + ROWS.len() - 1;
    let size = (pixels.round() as usize).clamp(FIRST_SIZE, last);
    let scale = if pixels > last as f32 { pixels / last as f32 } else { 1.0 };
    let row = &ROWS[size - FIRST_SIZE];
    let mut top = None;
    let mut bottom = 0;
    for c in text.content.chars().filter(|c| !c.is_whitespace()) {
        let (above, below) = columns(c);
        top = Some(top.map_or(row[above], |t: i32| t.max(row[above])));
        bottom = bottom.max(below.map_or(0, |b| row[b]));
    }
    let Some(top) = top else { return (0.0, 0.0) };
    let mut bottom = bottom as f32 * scale;
    // le soulignement est peint sous la ligne de base : un pixel d'épaisseur, à 15 % de la taille
    if text.underline {
        bottom = bottom.max((0.15 * pixels).ceil() + 1.0);
    }
    (top as f32 * scale / pixels_per_unit, bottom / pixels_per_unit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::svg_scene::{Anchor, Color};

    fn text(content: &str, size: f32, underline: bool) -> Text {
        Text {
            x: 0.0,
            y: 0.0,
            content: content.to_string(),
            size,
            color: Color(0),
            anchor: Anchor::Start,
            bold: false,
            italic: false,
            underline,
            monospace: false,
        }
    }

    #[test]
    fn rows_measured_on_the_screen_at_13_px() {
        let at_13 = |content: &str| text_extent(&text(content, 13.0, false), 1.0);
        assert_eq!(at_13("aire() : double"), (11.0, 1.0), "relevé sur une capture réelle");
        assert_eq!(at_13("nom : String"), (11.0, 3.0), "le point du i monte comme un d");
        assert_eq!(at_13("aaa"), (7.0, 0.0));
        assert_eq!(at_13("Test"), (9.0, 0.0));
        assert_eq!(at_13("  "), (0.0, 0.0));
    }

    #[test]
    fn the_size_on_screen_decides_with_the_zoom() {
        // 13 unités au zoom 2 : 26 pixels, soit 20 rangées et 3, rapportées aux unités du SVG
        assert_eq!(text_extent(&text("(", 13.0, false), 2.0), (10.0, 1.5));
        // très petit ou très grand : hors de la table, sans panique
        assert!(text_extent(&text("(", 13.0, false), 0.1).0 > 0.0);
        let huge = text_extent(&text("(", 13.0, false), 8.0);
        assert!(huge.0 > 8.0 && huge.0 < 11.0, "{huge:?}");
    }

    #[test]
    fn an_underline_counts_below_the_text() {
        let (_, plain) = text_extent(&text("aaa", 13.0, false), 1.0);
        let (_, underlined) = text_extent(&text("aaa", 13.0, true), 1.0);
        assert_eq!((plain, underlined), (0.0, 3.0));
    }
}
