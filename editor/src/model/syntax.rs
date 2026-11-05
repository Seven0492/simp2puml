//! Modèle : coloration syntaxique PlantUML ligne par ligne.
//!
//! Le seul état transmis d'une ligne à la suivante est « dans un commentaire
//! bloc ». `SyntaxCache` mémorise l'état de début de chaque ligne et ne le
//! recalcule qu'à partir de la première ligne modifiée, et seulement jusqu'à
//! la dernière ligne affichée (calcul paresseux, à la manière de Zed).

use std::ops::Range;

use super::buffer::Buffer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Plain,
    Comment,
    Directive,
    Preprocessor,
    Keyword,
    Arrow,
    Str,
    Stereotype,
    Color,
    Label,
    Bracket,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineState {
    pub in_block_comment: bool,
}

const KEYWORDS: &[&str] = &[
    "abstract", "actor", "agent", "alt", "annotation", "artifact", "as", "autonumber",
    "activate", "backward", "box", "boundary", "break", "card", "case", "caption", "circle",
    "class", "cloud", "collections", "component", "control", "create", "critical", "database",
    "deactivate", "destroy", "detach", "diamond", "else", "elseif", "end", "endif", "endwhile",
    "endswitch", "endlegend", "endnote", "endheader", "endfooter", "entity", "enum", "exception", "extends",
    "file", "folder", "footer", "fork", "frame", "group", "header", "hexagon", "hide", "if",
    "implements", "interface", "is", "kill", "label", "left", "legend", "loop", "map", "namespace",
    "newpage", "node", "not", "note", "object", "of", "on", "opt", "over", "package", "par",
    "participant", "partition", "person", "port", "portin", "portout", "protocol", "queue",
    "rectangle", "ref", "repeat", "return", "right", "scale", "show", "skinparam", "split",
    "stack", "start", "state", "stereotype", "stop", "storage", "struct", "switch", "then",
    "title", "together", "top", "bottom", "usecase", "while", "direction", "to", "again",
    "merge", "lasts", "starts", "happens", "project", "and", "then", "at", "days", "day", "hnote",
    "rnote", "across", "link", "metaclass", "record", "dataclass", "json", "up", "down",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Découpe une ligne en segments colorés (plages d'octets contiguës couvrant la ligne).
pub fn highlight_line(line: &str, state: &mut LineState) -> Vec<(Range<usize>, Token)> {
    let mut out: Vec<(Range<usize>, Token)> = Vec::new();
    let push = |r: Range<usize>, t: Token, out: &mut Vec<(Range<usize>, Token)>| {
        if r.is_empty() {
            return;
        }
        if let Some(last) = out.last_mut() {
            if last.1 == t && last.0.end == r.start {
                last.0.end = r.end;
                return;
            }
        }
        out.push((r, t));
    };
    let bytes = line.as_bytes();
    let n = line.len();
    let mut i = 0;
    if state.in_block_comment {
        match line.find("'/") {
            Some(p) => {
                push(0..p + 2, Token::Comment, &mut out);
                i = p + 2;
                state.in_block_comment = false;
            }
            None => {
                push(0..n, Token::Comment, &mut out);
                return out;
            }
        }
    }
    let trimmed = line[i..].trim_start();
    let lead = n - trimmed.len();
    let lead = if i == 0 { lead } else { usize::MAX };
    if i == 0 && lead > 0 && lead != usize::MAX {
        push(0..lead, Token::Plain, &mut out);
        i = lead;
    }
    if trimmed.starts_with('\'') {
        push(i..n, Token::Comment, &mut out);
        return out;
    }
    if trimmed.starts_with('@') {
        push(i..n, Token::Directive, &mut out);
        return out;
    }
    if trimmed.starts_with('!') {
        push(i..n, Token::Preprocessor, &mut out);
        return out;
    }
    let mut after_colon = false;
    while i < n {
        let c = line[i..].chars().next().unwrap();
        let cl = c.len_utf8();
        if after_colon {
            push(i..n, Token::Label, &mut out);
            break;
        }
        if line[i..].starts_with("/'") {
            match line[i + 2..].find("'/") {
                Some(p) => {
                    push(i..i + 2 + p + 2, Token::Comment, &mut out);
                    i += p + 4;
                }
                None => {
                    push(i..n, Token::Comment, &mut out);
                    state.in_block_comment = true;
                    return out;
                }
            }
            continue;
        }
        match c {
            '"' => {
                let end = line[i + 1..].find('"').map(|p| i + 1 + p + 1).unwrap_or(n);
                push(i..end, Token::Str, &mut out);
                i = end;
            }
            '<' if line[i..].starts_with("<<") => {
                let end = line[i + 2..].find(">>").map(|p| i + 2 + p + 2).unwrap_or(n);
                push(i..end, Token::Stereotype, &mut out);
                i = end;
            }
            // en début de ligne, # est une visibilité (membre protégé), pas une couleur
            '#' if i != lead && i + 1 < n && (bytes[i + 1] as char).is_ascii_alphanumeric() => {
                let mut e = i + 1;
                while e < n && (bytes[e].is_ascii_alphanumeric() || bytes[e] == b'/' || bytes[e] == b';') {
                    e += 1;
                }
                push(i..e, Token::Color, &mut out);
                i = e;
            }
            ':' => {
                push(i..i + 1, Token::Arrow, &mut out);
                i += 1;
                after_colon = true;
            }
            '{' | '}' | '[' | ']' | '(' | ')' => {
                push(i..i + 1, Token::Bracket, &mut out);
                i += 1;
            }
            '-' | '.' | '=' | '<' | '>' | '|' | '*' | '\\' | '/' | '^' => {
                let mut e = i;
                while e < n && matches!(bytes[e], b'-' | b'.' | b'=' | b'<' | b'>' | b'|' | b'*' | b'\\' | b'/' | b'^' | b'o' | b'x' | b'#' | b'+') {
                    // 'o' et 'x' ne font partie de la flèche que s'ils la terminent ou la commencent
                    if matches!(bytes[e], b'o' | b'x') && e + 1 < n && (bytes[e + 1] as char).is_alphanumeric() {
                        break;
                    }
                    if bytes[e] == b'#' {
                        break;
                    }
                    e += 1;
                }
                let tok = &line[i..e];
                let arrow = tok.contains('-') || tok.contains("..") || tok.contains('>') || tok.contains('<');
                push(i..e.max(i + 1), if arrow { Token::Arrow } else { Token::Plain }, &mut out);
                i = e.max(i + 1);
            }
            c if is_ident(c) => {
                let mut e = i;
                for ch in line[i..].chars() {
                    if !is_ident(ch) {
                        break;
                    }
                    e += ch.len_utf8();
                }
                let w = line[i..e].to_lowercase();
                let t = if KEYWORDS.binary_search(&w.as_str()).is_ok() || KEYWORDS.contains(&w.as_str()) {
                    Token::Keyword
                } else {
                    Token::Plain
                };
                push(i..e, t, &mut out);
                i = e;
            }
            _ => {
                push(i..i + cl, Token::Plain, &mut out);
                i += cl;
            }
        }
    }
    out
}

/// États de début de ligne, valides sur un préfixe du document.
#[derive(Default)]
pub struct SyntaxCache {
    states: Vec<LineState>,
}

impl SyntaxCache {
    /// Les états des lignes `0..=line` restent valides : une modification à
    /// partir du début de `line` ne change pas l'état d'entrée de cette ligne.
    pub fn invalidate_from(&mut self, line: usize) {
        self.states.truncate(line + 1);
    }

    /// État d'entrée de `line`, calculé à la demande depuis le dernier connu.
    pub fn state_at(&mut self, buffer: &Buffer, line: usize) -> LineState {
        if self.states.is_empty() {
            self.states.push(LineState::default());
        }
        while self.states.len() <= line && self.states.len() <= buffer.line_count() {
            let l = self.states.len() - 1;
            let mut st = self.states[l];
            highlight_line(buffer.line_text(l), &mut st);
            self.states.push(st);
        }
        self.states[line.min(self.states.len() - 1)]
    }

    /// Segments colorés d'une ligne du tampon.
    pub fn highlight(&mut self, buffer: &Buffer, line: usize) -> Vec<(Range<usize>, Token)> {
        let mut st = self.state_at(buffer, line);
        highlight_line(buffer.line_text(line), &mut st)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, Token)> {
        let mut st = LineState::default();
        highlight_line(line, &mut st)
            .into_iter()
            .map(|(r, t)| (line[r].to_string(), t))
            .collect()
    }

    #[test]
    fn relation_line() {
        let k = kinds("Alice -> Bob : bonjour");
        assert!(k.contains(&("->".to_string(), Token::Arrow)));
        assert!(k.contains(&(" bonjour".to_string(), Token::Label)));
    }

    #[test]
    fn keywords_strings_stereotypes() {
        let k = kinds("class \"Long\" as L <<entity>> #pink {");
        assert_eq!(k[0], ("class".to_string(), Token::Keyword));
        assert!(k.contains(&("\"Long\"".to_string(), Token::Str)));
        assert!(k.contains(&("<<entity>>".to_string(), Token::Stereotype)));
        assert!(k.contains(&("#pink".to_string(), Token::Color)));
    }

    #[test]
    fn hash_at_line_start_is_visibility() {
        let k = kinds("  #historique : List");
        assert!(!k.iter().any(|(_, t)| *t == Token::Color));
        assert!(kinds("class A #red").contains(&("#red".to_string(), Token::Color)));
    }

    #[test]
    fn comments_and_directives() {
        assert_eq!(kinds("' commentaire")[0].1, Token::Comment);
        assert_eq!(kinds("@startuml")[0].1, Token::Directive);
        assert_eq!(kinds("!include x")[0].1, Token::Preprocessor);
        let mut st = LineState::default();
        highlight_line("A /' début", &mut st);
        assert!(st.in_block_comment);
        let segs = highlight_line("fin '/ B", &mut st);
        assert_eq!(segs[0].1, Token::Comment);
        assert!(!st.in_block_comment);
    }

    #[test]
    fn cache_recomputes_after_invalidation() {
        let mut b = Buffer::new("a\n/' début\nb\nfin '/\nc");
        let mut cache = SyntaxCache::default();
        assert!(cache.state_at(&b, 2).in_block_comment);
        assert!(!cache.state_at(&b, 4).in_block_comment);
        b.replace_range(2..4, "");
        cache.invalidate_from(b.take_changed_from().unwrap());
        assert!(!cache.state_at(&b, 2).in_block_comment);
    }

    #[test]
    fn segments_cover_line() {
        let line = "Foo \"1\" *-- \"many\" Bar : contient >";
        let mut st = LineState::default();
        let segs = highlight_line(line, &mut st);
        let mut pos = 0;
        for (r, _) in segs {
            assert_eq!(r.start, pos);
            pos = r.end;
        }
        assert_eq!(pos, line.len());
    }
}
