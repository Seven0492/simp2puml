//! ViewModel d'un champ d'édition (éditeur multiligne ou champ simple).
//!
//! Il expose l'état présenté (texte, sélection, défilement, marques de
//! diagnostic, surlignages de recherche) et les commandes d'édition. Il ne
//! dessine rien : `view::editor_view` l'affiche et lui transmet les actions.

use std::ops::Range;

use gpui::{ClipboardItem, Context, EventEmitter, SharedString};

use crate::model::buffer::Buffer;
use crate::model::completion::{self, CompletionList};
use crate::model::diagram::Severity;
use crate::model::syntax::{SyntaxCache, Token};

pub const LINE_HEIGHT: f32 = 20.0;

#[derive(Clone, Debug)]
pub enum EditorEvent {
    /// Le texte a changé.
    Changed,
    CursorMoved,
    /// Entrée dans un champ simple.
    Submit,
    /// Échap.
    Cancel,
    /// Tabulation dans un champ simple : passer au champ suivant/précédent.
    FocusNext,
    FocusPrev,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    DocStart,
    DocEnd,
}

/// Menu d'autocomplétion ouvert.
#[derive(Clone, Debug)]
pub struct CompletionMenu {
    pub list: CompletionList,
    pub selected: usize,
}

/// Diagnostic affiché dans la gouttière (ligne 0-based).
#[derive(Clone, Debug)]
pub struct LineMark {
    pub line: usize,
    pub severity: Severity,
}

pub struct EditorViewModel {
    pub buffer: Buffer,
    syntax: SyntaxCache,
    /// Texte en cours de composition (méthode de saisie).
    pub marked_range: Option<Range<usize>>,
    pub single_line: bool,
    pub placeholder: SharedString,
    pub line_marks: Vec<LineMark>,
    /// Résultats de recherche à surligner.
    pub highlights: Vec<Range<usize>>,
    /// requête de recherche ouverte ; les surlignages suivent chaque modification
    search_query: String,
    pub scroll_x: f32,
    pub scroll_y: f32,
    viewport: (f32, f32),
    /// Le curseur doit être ramené dans la zone visible au prochain affichage.
    autoscroll: bool,
    pub completions: Option<CompletionMenu>,
    /// Préférence : ouvrir le menu pendant la frappe (sinon Ctrl+Espace seulement).
    pub autocomplete: bool,
}

impl EventEmitter<EditorEvent> for EditorViewModel {}

impl EditorViewModel {
    pub fn new(text: &str, single_line: bool, placeholder: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            syntax: SyntaxCache::default(),
            marked_range: None,
            single_line,
            placeholder: SharedString::from(placeholder.to_string()),
            line_marks: Vec::new(),
            highlights: Vec::new(),
            search_query: String::new(),
            scroll_x: 0.0,
            scroll_y: 0.0,
            viewport: (0.0, 0.0),
            autoscroll: true,
            completions: None,
            autocomplete: true,
        }
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    /// (ligne, colonne), 1-based, pour la barre d'état.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let c = self.buffer.cursor();
        (self.buffer.line_of(c) + 1, self.buffer.char_column(c) + 1)
    }

    // ------------------------------------------------------------ notifications

    fn changed(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.buffer.take_changed_from() {
            self.syntax.invalidate_from(line);
        }
        if !self.search_query.is_empty() {
            self.highlights = self.buffer.find_all(&self.search_query);
        }
        self.autoscroll = true;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        self.autoscroll = true;
        cx.emit(EditorEvent::CursorMoved);
        cx.notify();
    }

    // ------------------------------------------------------------ données pour la vue

    /// Segments colorés d'une ligne (cache incrémental).
    pub fn highlighted_line(&mut self, line: usize) -> Vec<(Range<usize>, Token)> {
        self.syntax.highlight(&self.buffer, line)
    }

    /// La vue communique la taille de la zone de texte.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width, height);
    }

    /// Étape 1 du suivi du curseur : défilement vertical (avant la mise en
    /// forme, pour savoir quelles lignes afficher).
    pub fn reveal_cursor_vertically(&mut self) {
        if !self.autoscroll {
            return;
        }
        let y = self.buffer.line_of(self.buffer.cursor()) as f32 * LINE_HEIGHT;
        let height = self.viewport.1;
        if y < self.scroll_y {
            self.scroll_y = y;
        } else if y + LINE_HEIGHT > self.scroll_y + height {
            self.scroll_y = y + LINE_HEIGHT - height;
        }
        if self.single_line {
            self.scroll_y = 0.0;
        }
    }

    /// Étape 2 : défilement horizontal, avec l'abscisse du curseur mesurée
    /// par la vue dans sa ligne.
    pub fn reveal_cursor_horizontally(&mut self, cursor_x: Option<f32>) {
        if !std::mem::take(&mut self.autoscroll) {
            return;
        }
        let Some(x) = cursor_x else { return };
        let width = (self.viewport.0 - 12.0).max(20.0);
        if x < self.scroll_x {
            self.scroll_x = (x - 20.0).max(0.0);
        } else if x > self.scroll_x + width {
            self.scroll_x = x - width + 20.0;
        }
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let max_y = (self.buffer.line_count() as f32 * LINE_HEIGHT - self.viewport.1 / 2.0).max(0.0);
        self.scroll_y = (self.scroll_y - dy).clamp(0.0, max_y);
        self.scroll_x = (self.scroll_x - dx).max(0.0);
        if self.single_line {
            self.scroll_y = 0.0;
        }
        self.autoscroll = false;
        cx.notify();
    }

    // ------------------------------------------------------------ commandes : texte

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.completions = None;
        let text = if self.single_line { text.replace('\n', " ") } else { text.to_string() };
        self.buffer = Buffer::new(&text);
        self.syntax = SyntaxCache::default();
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        self.changed(cx);
    }

    /// Remplace une plage (ou la sélection) ; `mark` indique une composition IME.
    pub fn replace_text(&mut self, range: Option<Range<usize>>, text: &str, mark: bool, cx: &mut Context<Self>) {
        let range = range.or(self.marked_range.clone()).unwrap_or(self.buffer.selection.clone());
        let text = if self.single_line { text.replace('\n', " ") } else { text.to_string() };
        self.buffer.replace_range(range.clone(), &text);
        self.marked_range = (mark && !text.is_empty()).then(|| range.start..range.start + text.len());
        // frappe d'un caractère de mot, d'un « @ » ou d'une flèche : (ré)ouvrir le menu
        let typing = !mark && !text.is_empty() && text.chars().all(|c| c.is_alphanumeric() || "_@-.<>|*".contains(c));
        if typing && !self.single_line && (self.autocomplete || self.completions.is_some()) {
            self.refresh_completions(false);
        } else {
            self.completions = None;
        }
        self.changed(cx);
    }

    pub fn delete(&mut self, backward: bool, word: bool, cx: &mut Context<Self>) {
        if backward {
            self.buffer.backspace(word);
        } else {
            self.buffer.delete_forward(word);
        }
        // en effaçant, le menu ouvert suit le préfixe
        if self.completions.is_some() {
            self.refresh_completions(false);
        }
        self.changed(cx);
    }

    pub fn newline(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        if self.single_line {
            cx.emit(EditorEvent::Submit);
        } else {
            self.buffer.newline();
            self.changed(cx);
        }
    }

    pub fn indent(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        if self.single_line {
            cx.emit(EditorEvent::FocusNext);
        } else {
            self.buffer.indent();
            self.changed(cx);
        }
    }

    pub fn outdent(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        if self.single_line {
            cx.emit(EditorEvent::FocusPrev);
        } else {
            self.buffer.outdent();
            self.changed(cx);
        }
    }

    /// Opérations sur des lignes entières (sans effet dans un champ simple).
    pub fn line_op(&mut self, op: fn(&mut Buffer), cx: &mut Context<Self>) {
        self.completions = None;
        if !self.single_line {
            op(&mut self.buffer);
            self.changed(cx);
        }
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        if self.buffer.undo() {
            self.changed(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        if self.buffer.redo() {
            self.changed(cx);
        }
    }

    pub fn replace_all(&mut self, query: &str, with: &str, cx: &mut Context<Self>) -> usize {
        self.completions = None;
        let n = self.buffer.replace_all(query, with);
        if n > 0 {
            self.changed(cx);
        }
        n
    }

    // ------------------------------------------------------------ commandes : presse-papiers

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.clipboard_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn cut(&mut self, cx: &mut Context<Self>) {
        self.completions = None;
        let Some(text) = self.clipboard_text() else { return };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if self.buffer.has_selection() {
            self.buffer.insert("");
        } else {
            self.buffer.delete_lines();
        }
        self.changed(cx);
    }

    /// Sélection, ou ligne entière s'il n'y en a pas (comme dans Zed).
    fn clipboard_text(&self) -> Option<String> {
        if self.buffer.has_selection() {
            Some(self.buffer.selected_text().to_string())
        } else if !self.single_line {
            let l = self.buffer.line_of(self.buffer.cursor());
            Some(format!("{}\n", self.buffer.line_text(l)))
        } else {
            None
        }
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            self.replace_text(None, &text.replace("\r\n", "\n").replace('\r', "\n"), false, cx);
        }
    }

    // ------------------------------------------------------------ commandes : curseur

    pub fn move_cursor(&mut self, motion: Motion, select: bool, cx: &mut Context<Self>) {
        let page = ((self.viewport.1 / LINE_HEIGHT) as isize - 2).max(1);
        let b = &mut self.buffer;
        match motion {
            Motion::Left => b.move_left(select, false),
            Motion::Right => b.move_right(select, false),
            Motion::WordLeft => b.move_left(select, true),
            Motion::WordRight => b.move_right(select, true),
            Motion::Up if !self.single_line => b.move_vertical(-1, select),
            Motion::Down if !self.single_line => b.move_vertical(1, select),
            Motion::PageUp => b.move_vertical(-page, select),
            Motion::PageDown => b.move_vertical(page, select),
            Motion::Home => b.move_home(select),
            Motion::End => b.move_end(select),
            Motion::DocStart => b.move_doc(false, select),
            Motion::DocEnd => b.move_doc(true, select),
            Motion::Up | Motion::Down => {}
        }
        self.moved(cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.emit(EditorEvent::CursorMoved);
        cx.notify();
    }

    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.buffer.select_range(range);
        self.moved(cx);
    }

    /// Clic : 1 = placer, 2 = mot, 3 = ligne ; Maj étend la sélection.
    pub fn click(&mut self, offset: usize, count: usize, extend: bool, cx: &mut Context<Self>) {
        match count {
            n if n >= 3 => {
                let l = self.buffer.line_of(offset);
                let r = self.buffer.line_range(l);
                let end = (r.end + 1).min(self.buffer.len());
                self.buffer.select_range(r.start..end);
            }
            2 => {
                let r = self.buffer.word_at(offset);
                self.buffer.select_range(r);
            }
            _ if extend => self.buffer.select_to(offset),
            _ => self.buffer.set_cursor(offset),
        }
        self.moved(cx);
    }

    pub fn drag_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buffer.select_to(offset);
        self.moved(cx);
    }

    pub fn escape(&mut self, cx: &mut Context<Self>) {
        if self.buffer.has_selection() {
            let c = self.buffer.cursor();
            self.buffer.set_cursor(c);
            cx.notify();
        }
        cx.emit(EditorEvent::Cancel);
    }

    /// Curseur au premier caractère non blanc de la ligne (0-based), centrée à l'écran.
    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let l = line.min(self.buffer.line_count().saturating_sub(1));
        let text = self.buffer.line_text(l);
        let indent = text.len() - text.trim_start().len();
        let start = self.buffer.line_start(l);
        self.buffer.set_cursor(start + indent);
        // centrer la ligne seulement si elle est hors de la zone visible
        let y = l as f32 * LINE_HEIGHT;
        let height = self.viewport.1;
        if height > 0.0 && (y < self.scroll_y || y + LINE_HEIGHT > self.scroll_y + height) {
            self.scroll_y = (y - height / 3.0).max(0.0);
        }
        self.moved(cx);
    }

    // ------------------------------------------------------------ commandes : autocomplétion

    fn refresh_completions(&mut self, explicit: bool) {
        let cursor = self.buffer.cursor();
        let selected_label = self
            .completions
            .as_ref()
            .and_then(|m| m.list.items.get(m.selected))
            .map(|c| c.label.clone());
        self.completions = completion::complete(self.buffer.text(), cursor, explicit).map(|list| {
            // conserver l'élément sélectionné s'il est toujours proposé
            let selected = selected_label
                .and_then(|l| list.items.iter().position(|c| c.label == l))
                .unwrap_or(0);
            CompletionMenu { list, selected }
        });
    }

    /// Ctrl+Espace : ouvre le menu même sans préfixe.
    pub fn show_completions(&mut self, cx: &mut Context<Self>) {
        if !self.single_line {
            self.refresh_completions(true);
            cx.notify();
        }
    }

    pub fn select_completion(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(menu) = &mut self.completions {
            let n = menu.list.items.len() as isize;
            menu.selected = (menu.selected as isize + delta).rem_euclid(n) as usize;
            cx.notify();
        }
    }

    pub fn dismiss_completions(&mut self, cx: &mut Context<Self>) {
        if self.completions.take().is_some() {
            cx.notify();
        }
    }

    /// Insère la proposition (celle sélectionnée par défaut).
    pub fn confirm_completion(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        let Some(menu) = self.completions.take() else { return };
        let Some(item) = menu.list.items.get(index.unwrap_or(menu.selected)) else { return };
        let range = menu.list.range.clone();
        let line = self.buffer.line_of(range.start);
        let line_text = self.buffer.line_text(line);
        let indent: String = line_text.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let (text, cursor) = completion::expand(&item.insert, &indent);
        self.buffer.replace_range(range.clone(), &text);
        self.buffer.set_cursor(range.start + cursor);
        self.changed(cx);
    }

    /// Plages de toutes les occurrences (pour le surlignage de recherche).
    pub fn set_search(&mut self, query: &str, cx: &mut Context<Self>) {
        self.search_query = query.to_string();
        self.highlights = self.buffer.find_all(query);
        cx.notify();
    }

    /// Sélectionne l'occurrence suivante (ou précédente) ; `false` si aucune.
    pub fn find_step(&mut self, query: &str, backwards: bool, cx: &mut Context<Self>) -> bool {
        let from = if backwards { self.buffer.selection.start } else { self.buffer.selection.end };
        match self.buffer.find(query, from, backwards) {
            Some(r) => {
                self.select(r, cx);
                true
            }
            None => false,
        }
    }
}
