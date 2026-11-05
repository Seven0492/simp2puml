//! Tampon de texte de l'éditeur : contenu, sélection, annulation et
//! opérations d'édition indépendantes de l'interface (testables sans GPUI).

use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
struct Edit {
    range: Range<usize>,
    old: String,
    new: String,
    sel_before: Range<usize>,
    sel_after: Range<usize>,
}

#[derive(Clone, Debug, Default)]
struct Transaction {
    edits: Vec<Edit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Insert,
    Delete,
    Other,
}

pub struct Buffer {
    text: String,
    line_starts: Vec<usize>,
    /// Sélection (octets UTF-8) ; `reversed` indique que le curseur est au début.
    pub selection: Range<usize>,
    pub reversed: bool,
    /// Colonne visée lors des déplacements verticaux.
    goal_column: Option<usize>,
    undo_stack: Vec<Transaction>,
    redo_stack: Vec<Transaction>,
    last_edit: Option<(EditKind, Instant)>,
    version: u64,
    saved_hash: u64,
    dirty_cache: Cell<(u64, bool)>,
}

fn text_hash(s: &str) -> u64 {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new("")
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut b = Self {
            text,
            line_starts: vec![0],
            selection: 0..0,
            reversed: false,
            goal_column: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            last_edit: None,
            version: 0,
            saved_hash: 0,
            dirty_cache: Cell::new((0, false)),
        };
        b.saved_hash = text_hash(&b.text);
        b.reindex();
        b
    }

    fn reindex(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (i, b) in self.text.bytes().enumerate() {
            if b == b'\n' {
                self.line_starts.push(i + 1);
            }
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Modifié par rapport au dernier enregistrement (une annulation qui
    /// ramène au texte enregistré rend le tampon propre).
    pub fn is_dirty(&self) -> bool {
        let (v, d) = self.dirty_cache.get();
        if v == self.version {
            return d;
        }
        let d = text_hash(&self.text) != self.saved_hash;
        self.dirty_cache.set((self.version, d));
        d
    }

    pub fn mark_saved(&mut self) {
        self.saved_hash = text_hash(&self.text);
        self.dirty_cache.set((self.version, false));
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[line.min(self.line_starts.len() - 1)]
    }

    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] - 1
        } else {
            self.text.len()
        }
    }

    pub fn line_range(&self, line: usize) -> Range<usize> {
        self.line_start(line)..self.line_end(line)
    }

    pub fn line_text(&self, line: usize) -> &str {
        &self.text[self.line_range(line)]
    }

    pub fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    /// (ligne, colonne en octets)
    pub fn point_of(&self, offset: usize) -> (usize, usize) {
        let line = self.line_of(offset);
        (line, offset - self.line_starts[line])
    }

    /// Colonne en caractères (pour l'affichage).
    pub fn char_column(&self, offset: usize) -> usize {
        let (line, _) = self.point_of(offset);
        self.text[self.line_starts[line]..offset].chars().count()
    }

    pub fn cursor(&self) -> usize {
        if self.reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }

    pub fn anchor(&self) -> usize {
        if self.reversed {
            self.selection.end
        } else {
            self.selection.start
        }
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty()
    }

    fn clamp(&self, o: usize) -> usize {
        let mut o = o.min(self.text.len());
        while !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    pub fn set_cursor(&mut self, offset: usize) {
        let o = self.clamp(offset);
        self.selection = o..o;
        self.reversed = false;
        self.goal_column = None;
        self.break_coalescing();
    }

    pub fn select_range(&mut self, range: Range<usize>) {
        let s = self.clamp(range.start);
        let e = self.clamp(range.end);
        self.selection = s.min(e)..s.max(e);
        self.reversed = e < s;
        self.goal_column = None;
        self.break_coalescing();
    }

    /// Étend la sélection en déplaçant le curseur.
    pub fn select_to(&mut self, offset: usize) {
        let anchor = self.anchor();
        let o = self.clamp(offset);
        if o < anchor {
            self.selection = o..anchor;
            self.reversed = true;
        } else {
            self.selection = anchor..o;
            self.reversed = false;
        }
        self.break_coalescing();
    }

    fn move_or_select(&mut self, offset: usize, select: bool) {
        if select {
            self.select_to(offset)
        } else {
            let o = self.clamp(offset);
            self.selection = o..o;
            self.reversed = false;
            self.break_coalescing();
        }
    }

    pub fn select_all(&mut self) {
        self.selection = 0..self.text.len();
        self.reversed = false;
    }

    pub fn prev_char(&self, offset: usize) -> usize {
        self.text[..offset]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    pub fn next_char(&self, offset: usize) -> usize {
        self.text[offset..]
            .chars()
            .next()
            .map(|c| offset + c.len_utf8())
            .unwrap_or(self.text.len())
    }

    pub fn prev_word(&self, offset: usize) -> usize {
        let mut o = offset;
        let mut it = self.text[..offset].char_indices().rev().peekable();
        while let Some(&(i, c)) = it.peek() {
            if is_word(c) {
                break;
            }
            o = i;
            it.next();
        }
        while let Some(&(i, c)) = it.peek() {
            if !is_word(c) {
                break;
            }
            o = i;
            it.next();
        }
        o
    }

    pub fn next_word(&self, offset: usize) -> usize {
        let mut o = offset;
        let mut it = self.text[offset..].char_indices().peekable();
        while let Some(&(_, c)) = it.peek() {
            if is_word(c) {
                break;
            }
            o += c.len_utf8();
            it.next();
        }
        while let Some(&(_, c)) = it.peek() {
            if !is_word(c) {
                break;
            }
            o += c.len_utf8();
            it.next();
        }
        o
    }

    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let mut s = offset;
        while s > 0 {
            let p = self.prev_char(s);
            if !self.text[p..s].chars().all(is_word) {
                break;
            }
            s = p;
        }
        let mut e = offset;
        while e < self.text.len() {
            let n = self.next_char(e);
            if !self.text[e..n].chars().all(is_word) {
                break;
            }
            e = n;
        }
        s..e
    }

    pub fn move_left(&mut self, select: bool, word: bool) {
        if !select && self.has_selection() {
            let s = self.selection.start;
            self.move_or_select(s, false);
            return;
        }
        let c = self.cursor();
        let t = if word { self.prev_word(c) } else { self.prev_char(c) };
        self.move_or_select(t, select);
        self.goal_column = None;
    }

    pub fn move_right(&mut self, select: bool, word: bool) {
        if !select && self.has_selection() {
            let e = self.selection.end;
            self.move_or_select(e, false);
            return;
        }
        let c = self.cursor();
        let t = if word { self.next_word(c) } else { self.next_char(c) };
        self.move_or_select(t, select);
        self.goal_column = None;
    }

    pub fn move_vertical(&mut self, delta: isize, select: bool) {
        let c = self.cursor();
        let (line, _) = self.point_of(c);
        let goal = self.goal_column.unwrap_or_else(|| self.char_column(c));
        let target = line as isize + delta;
        let t = if target < 0 {
            0
        } else if target as usize >= self.line_count() {
            self.text.len()
        } else {
            let l = target as usize;
            let r = self.line_range(l);
            let mut o = r.start;
            for (n, ch) in self.text[r.clone()].chars().enumerate() {
                if n >= goal {
                    break;
                }
                o += ch.len_utf8();
            }
            o
        };
        self.move_or_select(t, select);
        self.goal_column = Some(goal);
    }

    pub fn move_home(&mut self, select: bool) {
        let c = self.cursor();
        let line = self.line_of(c);
        let start = self.line_start(line);
        let indent = start
            + self.line_text(line).len()
            - self.line_text(line).trim_start().len();
        let t = if c == indent { start } else { indent };
        self.move_or_select(t, select);
    }

    pub fn move_end(&mut self, select: bool) {
        let line = self.line_of(self.cursor());
        let e = self.line_end(line);
        self.move_or_select(e, select);
    }

    pub fn move_doc(&mut self, end: bool, select: bool) {
        let t = if end { self.text.len() } else { 0 };
        self.move_or_select(t, select);
    }

    fn break_coalescing(&mut self) {
        self.last_edit = None;
    }

    /// Remplace `range` par `new` ; regroupe la frappe continue pour l'annulation.
    fn apply(&mut self, range: Range<usize>, new: &str, kind: EditKind) {
        let range = self.clamp(range.start)..self.clamp(range.end);
        let old = self.text[range.clone()].to_string();
        if old.is_empty() && new.is_empty() {
            return;
        }
        let sel_before = self.selection.clone();
        self.text.replace_range(range.clone(), new);
        self.reindex();
        let caret = range.start + new.len();
        self.selection = caret..caret;
        self.reversed = false;
        self.goal_column = None;
        self.version += 1;
        let edit = Edit {
            range: range.start..range.start + new.len(),
            old,
            new: new.to_string(),
            sel_before,
            sel_after: caret..caret,
        };
        let now = Instant::now();
        let coalesce = match self.last_edit {
            Some((k, t)) => {
                k == kind
                    && kind != EditKind::Other
                    && now.duration_since(t) < Duration::from_millis(1200)
                    && !new.contains('\n')
            }
            None => false,
        };
        if coalesce {
            if let Some(tx) = self.undo_stack.last_mut() {
                tx.edits.push(edit);
            } else {
                self.undo_stack.push(Transaction { edits: vec![edit] });
            }
        } else {
            self.undo_stack.push(Transaction { edits: vec![edit] });
        }
        self.redo_stack.clear();
        self.last_edit = Some((kind, now));
    }

    /// Plusieurs remplacements formant une seule étape d'annulation
    /// (plages triées, sans chevauchement, exprimées dans le texte d'origine).
    fn apply_many(&mut self, mut edits: Vec<(Range<usize>, String)>, selection: Range<usize>) {
        edits.sort_by_key(|(r, _)| r.start);
        let sel_before = self.selection.clone();
        let mut tx = Transaction::default();
        let mut delta: isize = 0;
        for (r, new) in edits {
            let start = (r.start as isize + delta) as usize;
            let end = (r.end as isize + delta) as usize;
            let old = self.text[start..end].to_string();
            self.text.replace_range(start..end, &new);
            delta += new.len() as isize - (end - start) as isize;
            tx.edits.push(Edit {
                range: start..start + new.len(),
                old,
                new,
                sel_before: sel_before.clone(),
                sel_after: selection.clone(),
            });
        }
        if tx.edits.is_empty() {
            return;
        }
        self.reindex();
        self.selection = self.clamp(selection.start)..self.clamp(selection.end);
        self.reversed = false;
        self.version += 1;
        self.undo_stack.push(tx);
        self.redo_stack.clear();
        self.last_edit = None;
    }

    pub fn replace_range(&mut self, range: Range<usize>, new: &str) {
        let kind = if new.is_empty() {
            EditKind::Delete
        } else if range.is_empty() && !new.contains(char::is_whitespace) {
            EditKind::Insert
        } else if range.is_empty() && new == " " {
            EditKind::Insert
        } else {
            EditKind::Other
        };
        self.apply(range, new, kind);
    }

    pub fn insert(&mut self, s: &str) {
        let r = self.selection.clone();
        self.replace_range(r, s);
    }

    pub fn backspace(&mut self, word: bool) {
        if self.has_selection() {
            self.insert("");
            return;
        }
        let c = self.cursor();
        if c == 0 {
            return;
        }
        // retrait d'indentation : efface jusqu'au taquet précédent
        let (line, col) = self.point_of(c);
        let before = &self.text[self.line_start(line)..c];
        let start = if word {
            self.prev_word(c)
        } else if !before.is_empty() && before.chars().all(|ch| ch == ' ') {
            c - (if col % 2 == 0 { 2.min(col) } else { 1 })
        } else {
            self.prev_char(c)
        };
        self.apply(start..c, "", EditKind::Delete);
    }

    pub fn delete_forward(&mut self, word: bool) {
        if self.has_selection() {
            self.insert("");
            return;
        }
        let c = self.cursor();
        let end = if word { self.next_word(c) } else { self.next_char(c) };
        self.apply(c..end, "", EditKind::Delete);
    }

    /// Retour à la ligne avec indentation automatique.
    pub fn newline(&mut self) {
        let c = self.selection.start;
        let line = self.line_of(c);
        let lt = &self.text[self.line_start(line)..c];
        let indent: String = lt.chars().take_while(|ch| *ch == ' ' || *ch == '\t').collect();
        let trimmed = lt.trim_end();
        let lower = trimmed.to_lowercase();
        let opens = trimmed.ends_with('{')
            || ["alt", "opt", "loop", "par", "group", "critical", "break", "box", "partition"]
                .iter()
                .any(|k| lower == *k || lower.starts_with(&format!("{k} ")))
            || lower.starts_with("if ")
            || lower.starts_with("if(")
            || lower.starts_with("while")
            || lower == "repeat"
            || lower == "fork"
            || lower == "fork again"
            || lower == "else"
            || lower.starts_with("else ")
            || lower.starts_with("elseif")
            || ((lower.starts_with("note ") || lower == "note") && !trimmed.contains(':'))
            || (lower.starts_with("legend") && !lower.contains(':'));
        let extra = if opens { "  " } else { "" };
        let s = format!("\n{indent}{extra}");
        self.apply(self.selection.clone(), &s, EditKind::Other);
    }

    fn selected_lines(&self) -> Range<usize> {
        let first = self.line_of(self.selection.start);
        let mut last = self.line_of(self.selection.end);
        if last > first && self.selection.end == self.line_start(last) {
            last -= 1;
        }
        first..last + 1
    }

    pub fn indent(&mut self) {
        if !self.has_selection() {
            let c = self.cursor();
            let col = self.char_column(c);
            let n = 2 - col % 2;
            self.apply(c..c, &" ".repeat(n), EditKind::Other);
            return;
        }
        let lines = self.selected_lines();
        let mut edits = Vec::new();
        for l in lines.clone() {
            if !self.line_text(l).is_empty() {
                let s = self.line_start(l);
                edits.push((s..s, "  ".to_string()));
            }
        }
        let added = edits.len() * 2;
        let sel = self.line_start(lines.start)..self.line_end(lines.end - 1) + added;
        self.apply_many(edits, sel);
    }

    pub fn outdent(&mut self) {
        let lines = self.selected_lines();
        let mut edits = Vec::new();
        let mut removed = 0;
        for l in lines.clone() {
            let t = self.line_text(l);
            let n = t.chars().take(2).take_while(|c| *c == ' ').count();
            let n = if n == 0 && t.starts_with('\t') { 1 } else { n };
            if n > 0 {
                let s = self.line_start(l);
                edits.push((s..s + n, String::new()));
                removed += n;
            }
        }
        if edits.is_empty() {
            return;
        }
        let sel = self.line_start(lines.start)..self.line_end(lines.end - 1) - removed;
        self.apply_many(edits, sel);
    }

    /// Commente ou décommente les lignes sélectionnées avec `'`.
    pub fn toggle_comment(&mut self) {
        let lines = self.selected_lines();
        let all_commented = lines
            .clone()
            .filter(|l| !self.line_text(*l).trim().is_empty())
            .all(|l| self.line_text(l).trim_start().starts_with('\''));
        let mut edits = Vec::new();
        for l in lines.clone() {
            let t = self.line_text(l);
            if t.trim().is_empty() {
                continue;
            }
            let ind = t.len() - t.trim_start().len();
            let s = self.line_start(l) + ind;
            if all_commented {
                let n = if t[ind..].starts_with("' ") { 2 } else { 1 };
                edits.push((s..s + n, String::new()));
            } else {
                edits.push((s..s, "' ".to_string()));
            }
        }
        let delta: isize = edits
            .iter()
            .map(|(r, s)| s.len() as isize - r.len() as isize)
            .sum();
        let end = (self.line_end(lines.end - 1) as isize + delta).max(0) as usize;
        self.apply_many(edits, self.line_start(lines.start)..end);
    }

    pub fn duplicate_lines(&mut self) {
        let lines = self.selected_lines();
        let s = self.line_start(lines.start);
        let e = self.line_end(lines.end - 1);
        let chunk = self.text[s..e].to_string();
        let caret = self.cursor() + chunk.len() + 1;
        self.apply_many(vec![(e..e, format!("\n{chunk}"))], caret..caret);
    }

    pub fn delete_lines(&mut self) {
        let lines = self.selected_lines();
        let s = self.line_start(lines.start);
        let mut e = self.line_end(lines.end - 1);
        let mut s2 = s;
        if e < self.text.len() {
            e += 1;
        } else if s > 0 {
            s2 = s - 1;
        }
        self.apply_many(vec![(s2..e, String::new())], s2..s2);
    }

    pub fn move_lines(&mut self, down: bool) {
        let lines = self.selected_lines();
        if (!down && lines.start == 0) || (down && lines.end >= self.line_count()) {
            return;
        }
        let s = self.line_start(lines.start);
        let e = self.line_end(lines.end - 1);
        let chunk = self.text[s..e].to_string();
        let (cur_start, cur_end) = (self.selection.start, self.selection.end);
        if down {
            let next = lines.end;
            let ns = self.line_start(next);
            let ne = self.line_end(next);
            let other = self.text[ns..ne].to_string();
            let shift = other.len() + 1;
            let replacement = format!("{other}\n{chunk}");
            self.apply_many(vec![(s..ne, replacement)], cur_start + shift..cur_end + shift);
        } else {
            let prev = lines.start - 1;
            let ps = self.line_start(prev);
            let pe = self.line_end(prev);
            let other = self.text[ps..pe].to_string();
            let shift = other.len() + 1;
            let replacement = format!("{chunk}\n{other}");
            self.apply_many(vec![(ps..e, replacement)], cur_start - shift..cur_end - shift);
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(tx) = self.undo_stack.pop() else {
            return false;
        };
        for e in tx.edits.iter().rev() {
            self.text.replace_range(e.range.clone(), &e.old);
        }
        self.reindex();
        let sel = tx.edits.first().map(|e| e.sel_before.clone()).unwrap_or(0..0);
        self.selection = self.clamp(sel.start)..self.clamp(sel.end);
        self.reversed = false;
        self.version += 1;
        self.redo_stack.push(tx);
        self.last_edit = None;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(tx) = self.redo_stack.pop() else {
            return false;
        };
        for e in tx.edits.iter() {
            let r = e.range.start..e.range.start + e.old.len();
            self.text.replace_range(r, &e.new);
        }
        self.reindex();
        let sel = tx.edits.last().map(|e| e.sel_after.clone()).unwrap_or(0..0);
        self.selection = self.clamp(sel.start)..self.clamp(sel.end);
        self.reversed = false;
        self.version += 1;
        self.undo_stack.push(tx);
        self.last_edit = None;
        true
    }

    /// Recherche (insensible à la casse) à partir de `from`, en bouclant.
    pub fn find(&self, query: &str, from: usize, backwards: bool) -> Option<Range<usize>> {
        if query.is_empty() {
            return None;
        }
        let hay = self.text.to_lowercase();
        let needle = query.to_lowercase();
        // to_lowercase peut changer les longueurs : on se replie sur une recherche exacte.
        let (hay, needle) = if hay.len() == self.text.len() {
            (hay, needle)
        } else {
            (self.text.clone(), query.to_string())
        };
        if backwards {
            let before = hay[..from.min(hay.len())].rfind(&needle);
            let found = before.or_else(|| hay.rfind(&needle))?;
            Some(found..found + needle.len())
        } else {
            let from = from.min(hay.len());
            let after = hay[from..].find(&needle).map(|i| i + from);
            let found = after.or_else(|| hay.find(&needle))?;
            Some(found..found + needle.len())
        }
    }

    pub fn count_matches(&self, query: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let hay = self.text.to_lowercase();
        let needle = query.to_lowercase();
        hay.matches(&needle).count()
    }

    /// Remplace toutes les occurrences (sensible à la casse si nécessaire).
    pub fn replace_all(&mut self, query: &str, with: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let lower = self.text.to_lowercase();
        let needle = query.to_lowercase();
        let use_lower = lower.len() == self.text.len();
        let hay = if use_lower { &lower } else { &self.text };
        let needle = if use_lower { needle } else { query.to_string() };
        let edits: Vec<(Range<usize>, String)> = hay
            .match_indices(&needle)
            .map(|(i, m)| (i..i + m.len(), with.to_string()))
            .collect();
        let n = edits.len();
        let c = self.cursor().min(self.text.len());
        self.apply_many(edits, c..c);
        let c = self.clamp(self.selection.start);
        self.selection = c..c;
        n
    }

    /// Convertit un décalage UTF-16 en décalage UTF-8.
    pub fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8 = 0;
        let mut utf16 = 0;
        for ch in self.text.chars() {
            if utf16 >= offset {
                break;
            }
            utf16 += ch.len_utf16();
            utf8 += ch.len_utf8();
        }
        utf8
    }

    pub fn offset_to_utf16(&self, offset: usize) -> usize {
        self.text[..offset.min(self.text.len())]
            .chars()
            .map(char::len_utf16)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_points() {
        let b = Buffer::new("a\nbé\n\nc");
        assert_eq!(b.line_count(), 4);
        assert_eq!(b.line_text(1), "bé");
        assert_eq!(b.point_of(3), (1, 1));
        assert_eq!(b.line_of(b.len()), 3);
        assert_eq!(b.char_column(5), 2);
    }

    #[test]
    fn typing_is_coalesced_for_undo() {
        let mut b = Buffer::new("");
        for c in ["c", "l", "a", "s", "s"] {
            b.insert(c);
        }
        assert_eq!(b.text(), "class");
        assert!(b.is_dirty());
        b.undo();
        assert_eq!(b.text(), "");
        assert!(!b.is_dirty());
        b.redo();
        assert_eq!(b.text(), "class");
        assert_eq!(b.cursor(), 5);
    }

    #[test]
    fn newline_keeps_and_increases_indent() {
        let mut b = Buffer::new("  class A {");
        b.set_cursor(b.len());
        b.newline();
        assert_eq!(b.text(), "  class A {\n    ");
        let mut b = Buffer::new("  A -> B");
        b.set_cursor(b.len());
        b.newline();
        assert_eq!(b.text(), "  A -> B\n  ");
    }

    #[test]
    fn toggle_comment_round_trip() {
        let mut b = Buffer::new("A -> B\n  B -> C");
        b.select_all();
        b.toggle_comment();
        assert_eq!(b.text(), "' A -> B\n  ' B -> C");
        b.toggle_comment();
        assert_eq!(b.text(), "A -> B\n  B -> C");
        b.undo();
        assert_eq!(b.text(), "' A -> B\n  ' B -> C");
    }

    #[test]
    fn indent_and_outdent_selection() {
        let mut b = Buffer::new("a\nb\n");
        b.select_range(0..3);
        b.indent();
        assert_eq!(b.text(), "  a\n  b\n");
        b.outdent();
        assert_eq!(b.text(), "a\nb\n");
    }

    #[test]
    fn vertical_moves_keep_goal_column() {
        let mut b = Buffer::new("abcdef\nab\nabcdef");
        b.set_cursor(5);
        b.move_vertical(1, false);
        assert_eq!(b.point_of(b.cursor()), (1, 2));
        b.move_vertical(1, false);
        assert_eq!(b.point_of(b.cursor()), (2, 5));
    }

    #[test]
    fn words_and_selection() {
        let mut b = Buffer::new("foo bar_baz");
        b.set_cursor(0);
        b.move_right(true, true);
        assert_eq!(b.selected_text(), "foo");
        assert_eq!(b.word_at(6), 4..11);
        b.set_cursor(11);
        b.move_left(false, true);
        assert_eq!(b.cursor(), 4);
    }

    #[test]
    fn line_operations() {
        let mut b = Buffer::new("one\ntwo\nthree");
        b.set_cursor(5);
        b.duplicate_lines();
        assert_eq!(b.text(), "one\ntwo\ntwo\nthree");
        b.delete_lines();
        assert_eq!(b.text(), "one\ntwo\nthree");
        b.set_cursor(0);
        b.move_lines(true);
        assert_eq!(b.text(), "two\none\nthree");
        assert_eq!(b.line_of(b.cursor()), 1);
        b.move_lines(false);
        assert_eq!(b.text(), "one\ntwo\nthree");
    }

    #[test]
    fn find_and_replace() {
        let mut b = Buffer::new("Alice -> Bob\nbob -> alice");
        assert_eq!(b.find("bob", 0, false), Some(9..12));
        assert_eq!(b.find("bob", 12, false), Some(13..16));
        assert_eq!(b.find("bob", 9, true), Some(13..16));
        assert_eq!(b.count_matches("alice"), 2);
        assert_eq!(b.replace_all("alice", "Carol"), 2);
        assert_eq!(b.text(), "Carol -> Bob\nbob -> Carol");
        b.undo();
        assert_eq!(b.text(), "Alice -> Bob\nbob -> alice");
    }

    #[test]
    fn utf16_conversions() {
        let b = Buffer::new("é😀a");
        assert_eq!(b.offset_to_utf16(2), 1);
        assert_eq!(b.offset_to_utf16(6), 3);
        assert_eq!(b.offset_from_utf16(3), 6);
    }

    #[test]
    fn backspace_removes_indent_unit() {
        let mut b = Buffer::new("    x");
        b.set_cursor(4);
        b.backspace(false);
        assert_eq!(b.text(), "  x");
    }
}
