//! Modèle : retrouver dans le texte source l'endroit exact d'un élément du diagramme.
//!
//! Le moteur ne donne que des lignes ; Ctrl+clic veut le caractère. On cherche donc ce qui
//! est affiché dans la ligne source : le texte tel quel, puis le mot cliqué (l'affichage
//! peut ajouter une numérotation ou des guillemets), ou le jeton d'une flèche.

use std::ops::Range;

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Corrige `closest`, le résultat de `closest_index_for_x` : dans le dernier caractère d'un texte,
/// GPUI renvoie toujours sa fin. À plus d'un demi-caractère du bord proposé, le bord voisin est
/// plus proche et le remplace. `x_of` : position réelle à l'écran d'un indice du texte.
pub fn nearest_edge(text: &str, x: f32, closest: usize, x_of: &dyn Fn(usize) -> f32) -> usize {
    let (Some(head), Some(tail)) = (text.get(..closest), text.get(closest..)) else {
        return closest;
    };
    let before = head.char_indices().next_back().map(|(i, _)| i);
    let after = tail.chars().next().map(|c| closest + c.len_utf8());
    let distance = |i: usize| (x_of(i) - x).abs();
    [before, after].into_iter().flatten().fold(closest, |best, i| if distance(i) < distance(best) { i } else { best })
}

/// Position dans `line` du bord d'indice `index` (en octets) de `shown`.
pub fn locate_text(line: &str, shown: &str, index: usize) -> Option<usize> {
    if shown.trim().is_empty() {
        return None;
    }
    if let Some(start) = line.find(shown) {
        return Some(start + index);
    }
    flexible_match(line, shown, index)
}

/// Comme `find`, en acceptant autant d'espaces qu'on veut là où `shown` en a.
fn flexible_match(line: &str, shown: &str, index: usize) -> Option<usize> {
    let target: Vec<(usize, char)> = shown.char_indices().filter(|(_, c)| !c.is_whitespace()).collect();
    let first = target.first()?.1;
    for (start, c) in line.char_indices() {
        if c != first {
            continue;
        }
        let mut wanted = target.iter();
        let mut next = wanted.next();
        let mut found = None;
        let mut end = start;
        for (pos, ch) in line[start..].char_indices() {
            let Some(&(shown_at, expected)) = next else { break };
            if ch.is_whitespace() {
                continue;
            }
            if ch != expected {
                break;
            }
            if shown_at >= index && found.is_none() {
                found = Some(start + pos);
            }
            end = start + pos + ch.len_utf8();
            next = wanted.next();
        }
        if next.is_none() {
            // bord droit du dernier caractère : juste après lui
            return found.or(Some(end));
        }
    }
    None
}

/// Le mot cliqué, en entier, dans `line` : l'affichage et le source peuvent différer autour.
pub fn locate_word(line: &str, shown: &str, index: usize) -> Option<usize> {
    let index = index.min(shown.len());
    let start = shown[..index].char_indices().rev().find(|(_, c)| !is_word_char(*c)).map_or(0, |(i, c)| i + c.len_utf8());
    let end = shown[index..].char_indices().find(|(_, c)| !is_word_char(*c)).map_or(shown.len(), |(i, _)| index + i);
    let word = &shown[start..end];
    if word.is_empty() {
        return None;
    }
    let mut from = 0;
    while let Some(pos) = line[from..].find(word) {
        let at = from + pos;
        let before = line[..at].chars().next_back();
        let after = line[at + word.len()..].chars().next();
        if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
            return Some(at + index - start);
        }
        from = at + word.len();
    }
    None
}

/// Extrémités possibles d'une flèche PlantUML, de part et d'autre de son trait.
const HEAD_CHARS: &[char] = &['<', '>', '|', '*', '#', '}', '{', '+', '^', '~', '\\', '/', '?', '['];
const DIRECTIONS: [&str; 8] = ["up", "down", "left", "right", "u", "d", "l", "r"];

/// Le jeton de la première flèche de la ligne (`-->`, `o--`, `..|>`, `-[#red]->`, `-up->`…),
/// hors guillemets et avant le libellé.
pub fn arrow_token(line: &str) -> Option<Range<usize>> {
    let bytes: Vec<(usize, char)> = line.char_indices().collect();
    let char_at = |k: usize| bytes.get(k).map(|(_, c)| *c);
    let mut quoted = false;
    let mut k = 0;
    while k < bytes.len() {
        let c = bytes[k].1;
        if c == '"' {
            quoted = !quoted;
        } else if !quoted {
            if c == ':' {
                return None; // le libellé commence : pas de flèche avant lui
            }
            let next = char_at(k + 1);
            let is_core = (c == '-' && matches!(next, Some('-' | '>' | '[' | '.')))
                || (c == '.' && matches!(next, Some('.' | '>' | '[')))
                || (c == '-' && next.is_some_and(|n| n.is_ascii_alphabetic()) && direction_at(&bytes, k + 1).is_some())
                || (c == '<' && matches!(next, Some('-' | '.')));
            if is_core {
                return Some(extend(&bytes, line, k));
            }
        }
        k += 1;
    }
    None
}

/// « up- », « left- » après un tiret : longueur de la direction.
fn direction_at(bytes: &[(usize, char)], start: usize) -> Option<usize> {
    let word: String = bytes[start..].iter().map(|(_, c)| *c).take_while(char::is_ascii_alphabetic).collect();
    let followed = bytes.get(start + word.chars().count()).map(|(_, c)| *c);
    (DIRECTIONS.contains(&word.to_ascii_lowercase().as_str()) && followed == Some('-')).then_some(word.chars().count())
}

fn extend(bytes: &[(usize, char)], line: &str, core: usize) -> Range<usize> {
    // à gauche : une pointe collée au trait ; « o » et « x » seulement détachés d'un nom
    let mut left = core;
    while left > 0 {
        let prev = bytes[left - 1].1;
        let letter_head = matches!(prev, 'o' | 'x') && (left < 2 || bytes[left - 2].1.is_whitespace());
        if HEAD_CHARS.contains(&prev) && prev != '[' || letter_head {
            left -= 1;
        } else {
            break;
        }
    }
    // à droite : trait, pointes, couleur entre crochets, direction
    let mut right = core;
    while right < bytes.len() {
        let c = bytes[right].1;
        if c == '-' || c == '.' || c == '=' {
            right += 1;
        } else if c == '[' {
            match bytes[right..].iter().position(|(_, ch)| *ch == ']') {
                Some(close) => right += close + 1,
                None => break,
            }
        } else if let Some(len) = direction_at(bytes, right).filter(|_| right > core) {
            right += len;
        } else if HEAD_CHARS.contains(&c) || (matches!(c, 'o' | 'x') && bytes.get(right + 1).is_none_or(|(_, n)| n.is_whitespace())) {
            right += 1;
        } else {
            break;
        }
    }
    let end = bytes.get(right).map_or(line.len(), |(i, _)| *i);
    bytes[left].0..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(line: &str) -> Option<&str> {
        arrow_token(line).map(|r| &line[r])
    }

    #[test]
    fn the_nearest_edge_of_a_character_is_chosen() {
        // trois caractères de 10 pixels : bords à 0, 10, 20, 30
        let x_of = |i: usize| i as f32 * 10.0;
        // ce que renvoie GPUI, y compris sa fin de texte pour tout le dernier caractère
        let closest = |x: f32| if x > 20.0 { 3 } else { ((x + 5.0) / 10.0).floor() as usize };
        for (x, expected) in [(0.0, 0), (4.0, 0), (6.0, 1), (14.0, 1), (16.0, 2), (24.0, 2), (26.0, 3), (99.0, 3)] {
            assert_eq!(nearest_edge("abc", x, closest(x), &x_of), expected, "x = {x}");
        }
        assert_eq!(nearest_edge("abc", 15.0, 1, &x_of), 1, "à égale distance : le bord proposé");
        // caractères de plusieurs octets : jamais au milieu d'un caractère
        let bytes = |i: usize| match i { 0 => 0.0, 2 => 10.0, _ => 20.0 };
        assert_eq!(nearest_edge("éa", 8.0, 0, &bytes), 2);
        assert_eq!(nearest_edge("éa", 12.0, 3, &bytes), 2);
        assert_eq!(nearest_edge("", 8.0, 0, &|_| 0.0), 0);
        assert_eq!(nearest_edge("é", 8.0, 1, &|_| 0.0), 1, "indice hors d'un bord : laissé tel quel");
    }

    #[test]
    fn every_kind_of_arrow_is_found() {
        for (line, expected) in [
            ("A --> B", "-->"),
            ("A -> B", "->"),
            ("A ->> B : x", "->>"),
            ("A <-- B", "<--"),
            ("A <|-- B", "<|--"),
            ("A --|> B", "--|>"),
            ("A ..|> B", "..|>"),
            ("A ..> B : y", "..>"),
            ("A *-- B", "*--"),
            ("A o-- B", "o--"),
            ("A --o B", "--o"),
            ("A -- B", "--"),
            ("A .. B", ".."),
            ("A -[#red]-> B", "-[#red]->"),
            ("A -[hidden]down- B", "-[hidden]down-"),
            ("A -up-> B", "-up->"),
            ("A -left- B", "-left-"),
            ("A<|--B", "<|--"),
            ("  Client --> S : GET /panier", "-->"),
        ] {
            assert_eq!(token(line), Some(expected), "{line}");
        }
    }

    #[test]
    fn quotes_names_and_labels_are_not_arrows() {
        assert_eq!(token(r#"Membre "1" o-- "0..*" Livre : emprunte >"#), Some("o--"), "« 0..* » est entre guillemets");
        assert_eq!(token(r#"A "x --> y" --> B"#), Some("-->"));
        assert_eq!(token("class nom-compose"), None, "un tiret dans un nom");
        assert_eq!(token("A : x --> y"), None, "après le libellé");
        assert_eq!(token(":action;"), None);
        assert_eq!(token("** enfant"), None);
        assert_eq!(token("a.b"), None);
    }

    #[test]
    fn text_is_found_exactly_then_with_flexible_spaces() {
        assert_eq!(locate_text("    - titre : String", "titre : String", 8), Some(14));
        assert_eq!(locate_text("A -> B : un  message", "un message", 3), Some(13), "le source a deux espaces");
        assert_eq!(locate_text("rien ici", "autre", 0), None);
        assert_eq!(locate_text("x", "  ", 0), None);
        assert_eq!(locate_text("é : à", "à", 0), Some(5), "positions en octets");
        // bord droit du dernier caractère, avec un espacement différent dans le source
        assert_eq!(locate_text("A : un  mot suite", "un mot", 6), Some(11));
        assert_eq!(locate_text("A : un mot suite", "un mot", 6), Some(10));
    }

    #[test]
    fn the_clicked_word_is_found_when_the_display_differs() {
        // affichage « 1 GET /panier » pour un source sans le numéro
        let shown = "1 GET /panier";
        assert_eq!(locate_word("Client -> S : GET /panier", shown, 3), Some(15));
        // stéréotype affiché entre guillemets français
        assert_eq!(locate_word("class M <<entity>> {", "«entity»", 4), Some(12));
        // le mot entier, pas un morceau d'un autre mot
        assert_eq!(locate_word("classe class", "class", 0), Some(7));
        assert_eq!(locate_word("sans rapport", "autre", 1), None);
        assert_eq!(locate_word("x", " : ", 1), None);
    }
}
