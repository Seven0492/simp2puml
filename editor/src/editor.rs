//! Vue d'édition de texte GPUI : rendu des lignes visibles avec coloration,
//! gouttière (numéros et diagnostics), sélection, saisie IME, souris et défilement.

use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollWheelEvent, ShapedLine, SharedString, Style, TextRun, UTF16Selection, Window, actions,
    div, fill, font, point, prelude::*, px, relative, rgb, size,
};

use crate::buffer::Buffer;
use crate::highlight::{LineState, Token, highlight_line, line_states};
use crate::renderer::Severity;

actions!(
    editor,
    [
        Backspace,
        BackspaceWord,
        Delete,
        DeleteWord,
        Left,
        Right,
        Up,
        Down,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        Home,
        End,
        SelectHome,
        SelectEnd,
        DocStart,
        DocEnd,
        SelectDocStart,
        SelectDocEnd,
        PageUp,
        PageDown,
        SelectAll,
        Newline,
        Tab,
        Outdent,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        ToggleComment,
        DuplicateLine,
        DeleteLine,
        MoveLineUp,
        MoveLineDown,
        Escape,
    ]
);

pub const FONT_FAMILY: &str = "DejaVu Sans Mono";
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;

#[derive(Clone, Debug)]
pub enum EditorEvent {
    Changed,
    CursorMoved,
    Submit,
    Cancel,
}

pub struct TextEditor {
    pub buffer: Buffer,
    focus_handle: FocusHandle,
    marked_range: Option<Range<usize>>,
    scroll_x: f32,
    scroll_y: f32,
    pub single_line: bool,
    placeholder: SharedString,
    /// Diagnostics par ligne (0-based) affichés dans la gouttière.
    pub line_marks: Vec<(usize, Severity, String)>,
    /// Plages surlignées (résultats de recherche).
    pub highlights: Vec<Range<usize>>,
    states_cache: (u64, Vec<LineState>),
    last_bounds: Option<Bounds<Pixels>>,
    last_lines: Vec<(usize, ShapedLine)>,
    text_x: f32,
    char_w: f32,
    is_selecting: bool,
    autoscroll: bool,
}

impl EventEmitter<EditorEvent> for TextEditor {}

impl Focusable for TextEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn token_color(t: Token) -> Hsla {
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

impl TextEditor {
    pub fn new(text: &str, single_line: bool, placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self {
            buffer: Buffer::new(text),
            focus_handle: cx.focus_handle(),
            marked_range: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            single_line,
            placeholder: SharedString::from(placeholder.to_string()),
            line_marks: Vec::new(),
            highlights: Vec::new(),
            states_cache: (u64::MAX, Vec::new()),
            last_bounds: None,
            last_lines: Vec::new(),
            text_x: 0.0,
            char_w: 8.4,
            is_selecting: false,
            autoscroll: true,
        }
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = if self.single_line { text.replace('\n', " ") } else { text.to_string() };
        self.buffer = Buffer::new(&text);
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        cx.emit(EditorEvent::Changed);
        cx.notify();
    }

    fn moved(&mut self, cx: &mut Context<Self>) {
        self.autoscroll = true;
        cx.emit(EditorEvent::CursorMoved);
        cx.notify();
    }

    /// Place le curseur au début de la ligne (0-based) et la rend visible.
    pub fn go_to_line(&mut self, line: usize, cx: &mut Context<Self>) {
        let l = line.min(self.buffer.line_count().saturating_sub(1));
        let r = self.buffer.line_range(l);
        let indent = self.buffer.line_text(l).len() - self.buffer.line_text(l).trim_start().len();
        self.buffer.set_cursor(r.start + indent);
        if let Some(b) = self.last_bounds {
            let h: f32 = b.size.height.into();
            self.scroll_y = (l as f32 * LINE_HEIGHT - h / 3.0).max(0.0);
        }
        self.moved(cx);
    }

    pub fn select(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        self.buffer.select_range(range);
        self.moved(cx);
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let c = self.buffer.cursor();
        (self.buffer.line_of(c) + 1, self.buffer.char_column(c) + 1)
    }

    fn ensure_states(&mut self) {
        let v = self.buffer.version();
        if self.states_cache.0 != v || self.states_cache.1.len() != self.buffer.line_count() {
            self.states_cache = (v, line_states(self.buffer.text()));
        }
    }

    // ------------------------------------------------------------ actions

    fn backspace(&mut self, _: &Backspace, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.backspace(false);
        self.changed(cx);
    }
    fn backspace_word(&mut self, _: &BackspaceWord, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.backspace(true);
        self.changed(cx);
    }
    fn delete(&mut self, _: &Delete, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.delete_forward(false);
        self.changed(cx);
    }
    fn delete_word(&mut self, _: &DeleteWord, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.delete_forward(true);
        self.changed(cx);
    }
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(false, false);
        self.moved(cx);
    }
    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(false, false);
        self.moved(cx);
    }
    fn up(&mut self, _: &Up, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_vertical(-1, false);
        self.moved(cx);
    }
    fn down(&mut self, _: &Down, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_vertical(1, false);
        self.moved(cx);
    }
    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(true, false);
        self.moved(cx);
    }
    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(true, false);
        self.moved(cx);
    }
    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_vertical(-1, true);
        self.moved(cx);
    }
    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_vertical(1, true);
        self.moved(cx);
    }
    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(false, true);
        self.moved(cx);
    }
    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(false, true);
        self.moved(cx);
    }
    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left(true, true);
        self.moved(cx);
    }
    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right(true, true);
        self.moved(cx);
    }
    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_home(false);
        self.moved(cx);
    }
    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_end(false);
        self.moved(cx);
    }
    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_home(true);
        self.moved(cx);
    }
    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_end(true);
        self.moved(cx);
    }
    fn doc_start(&mut self, _: &DocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_doc(false, false);
        self.moved(cx);
    }
    fn doc_end(&mut self, _: &DocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_doc(true, false);
        self.moved(cx);
    }
    fn select_doc_start(&mut self, _: &SelectDocStart, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_doc(false, true);
        self.moved(cx);
    }
    fn select_doc_end(&mut self, _: &SelectDocEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_doc(true, true);
        self.moved(cx);
    }
    fn page_lines(&self) -> isize {
        self.last_bounds
            .map(|b| (f32::from(b.size.height) / LINE_HEIGHT) as isize - 2)
            .unwrap_or(20)
            .max(1)
    }
    fn page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.page_lines();
        self.buffer.move_vertical(-n, false);
        self.scroll_y = (self.scroll_y - n as f32 * LINE_HEIGHT).max(0.0);
        self.moved(cx);
    }
    fn page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        let n = self.page_lines();
        self.buffer.move_vertical(n, false);
        self.scroll_y += n as f32 * LINE_HEIGHT;
        self.moved(cx);
    }
    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.emit(EditorEvent::CursorMoved);
        cx.notify();
    }
    fn newline(&mut self, _: &Newline, _: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            cx.emit(EditorEvent::Submit);
            return;
        }
        self.buffer.newline();
        self.changed(cx);
    }
    fn tab(&mut self, _: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            window.focus_next();
            return;
        }
        self.buffer.indent();
        self.changed(cx);
    }
    fn outdent(&mut self, _: &Outdent, window: &mut Window, cx: &mut Context<Self>) {
        if self.single_line {
            window.focus_prev();
            return;
        }
        self.buffer.outdent();
        self.changed(cx);
    }
    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = if self.buffer.has_selection() {
            self.buffer.selected_text().to_string()
        } else if !self.single_line {
            let l = self.buffer.line_of(self.buffer.cursor());
            format!("{}\n", self.buffer.line_text(l))
        } else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.has_selection() {
            cx.write_to_clipboard(ClipboardItem::new_string(self.buffer.selected_text().to_string()));
            self.buffer.insert("");
        } else if !self.single_line {
            let l = self.buffer.line_of(self.buffer.cursor());
            cx.write_to_clipboard(ClipboardItem::new_string(format!("{}\n", self.buffer.line_text(l))));
            self.buffer.delete_lines();
        } else {
            return;
        }
        self.changed(cx);
    }
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
            let text = text.replace("\r\n", "\n");
            let text = if self.single_line { text.replace('\n', " ") } else { text };
            self.buffer.insert(&text);
            self.changed(cx);
        }
    }
    fn undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.undo() {
            self.changed(cx);
        }
    }
    fn redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.redo() {
            self.changed(cx);
        }
    }
    fn toggle_comment(&mut self, _: &ToggleComment, _: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.buffer.toggle_comment();
            self.changed(cx);
        }
    }
    fn duplicate_line(&mut self, _: &DuplicateLine, _: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.buffer.duplicate_lines();
            self.changed(cx);
        }
    }
    fn delete_line(&mut self, _: &DeleteLine, _: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.buffer.delete_lines();
            self.changed(cx);
        }
    }
    fn move_line_up(&mut self, _: &MoveLineUp, _: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.buffer.move_lines(false);
            self.changed(cx);
        }
    }
    fn move_line_down(&mut self, _: &MoveLineDown, _: &mut Window, cx: &mut Context<Self>) {
        if !self.single_line {
            self.buffer.move_lines(true);
            self.changed(cx);
        }
    }
    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        if self.buffer.has_selection() {
            let c = self.buffer.cursor();
            self.buffer.set_cursor(c);
            cx.notify();
        }
        cx.emit(EditorEvent::Cancel);
    }

    // ------------------------------------------------------------ souris

    fn index_for_position(&self, pos: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else { return 0 };
        let y = f32::from(pos.y - bounds.top()) + self.scroll_y;
        let line = if self.single_line { 0 } else { (y / LINE_HEIGHT).floor().max(0.0) as usize };
        if line >= self.buffer.line_count() {
            return self.buffer.len();
        }
        let x = f32::from(pos.x - bounds.left()) - self.text_x + self.scroll_x;
        let start = self.buffer.line_start(line);
        if let Some((_, shaped)) = self.last_lines.iter().find(|(l, _)| *l == line) {
            start + shaped.closest_index_for_x(px(x.max(0.0)))
        } else {
            start
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let idx = self.index_for_position(ev.position);
        if ev.click_count >= 3 {
            let l = self.buffer.line_of(idx);
            let r = self.buffer.line_range(l);
            let end = (r.end + 1).min(self.buffer.len());
            self.buffer.select_range(r.start..end);
        } else if ev.click_count == 2 {
            let r = self.buffer.word_at(idx);
            self.buffer.select_range(r);
        } else if ev.modifiers.shift {
            self.buffer.select_to(idx);
        } else {
            self.buffer.set_cursor(idx);
        }
        self.is_selecting = true;
        self.moved(cx);
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting && ev.pressed_button == Some(MouseButton::Left) {
            let idx = self.index_for_position(ev.position);
            self.buffer.select_to(idx);
            self.moved(cx);
        } else if self.is_selecting {
            self.is_selecting = false;
        }
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(LINE_HEIGHT));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let (dx, dy) = if ev.modifiers.shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
        self.scroll_y = (self.scroll_y - dy).max(0.0);
        self.scroll_x = (self.scroll_x - dx).max(0.0);
        self.clamp_scroll();
        self.autoscroll = false;
        cx.notify();
    }

    fn clamp_scroll(&mut self) {
        let max_y = if let Some(b) = self.last_bounds {
            (self.buffer.line_count() as f32 * LINE_HEIGHT - f32::from(b.size.height) / 2.0).max(0.0)
        } else {
            f32::MAX
        };
        self.scroll_y = self.scroll_y.min(max_y);
        if self.single_line {
            self.scroll_y = 0.0;
        }
    }
}

impl EntityInputHandler for TextEditor {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.buffer.offset_from_utf16(range_utf16.start)..self.buffer.offset_from_utf16(range_utf16.end);
        actual_range.replace(self.buffer.offset_to_utf16(r.start)..self.buffer.offset_to_utf16(r.end));
        Some(self.buffer.text()[r].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let s = &self.buffer.selection;
        Some(UTF16Selection {
            range: self.buffer.offset_to_utf16(s.start)..self.buffer.offset_to_utf16(s.end),
            reversed: self.buffer.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|r| self.buffer.offset_to_utf16(r.start)..self.buffer.offset_to_utf16(r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.buffer.offset_from_utf16(r.start)..self.buffer.offset_from_utf16(r.end))
            .or(self.marked_range.clone())
            .unwrap_or(self.buffer.selection.clone());
        let text = if self.single_line { new_text.replace('\n', " ") } else { new_text.to_string() };
        self.buffer.replace_range(range, &text);
        self.marked_range = None;
        self.changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .map(|r| self.buffer.offset_from_utf16(r.start)..self.buffer.offset_from_utf16(r.end))
            .or(self.marked_range.clone())
            .unwrap_or(self.buffer.selection.clone());
        self.buffer.replace_range(range.clone(), new_text);
        self.marked_range = if new_text.is_empty() {
            None
        } else {
            Some(range.start..range.start + new_text.len())
        };
        if let Some(sel) = new_selected_range_utf16 {
            // décalages relatifs au texte marqué
            let local: String = new_text.to_string();
            let to8 = |u: usize| -> usize {
                let mut n16 = 0;
                let mut n8 = 0;
                for ch in local.chars() {
                    if n16 >= u {
                        break;
                    }
                    n16 += ch.len_utf16();
                    n8 += ch.len_utf8();
                }
                n8
            };
            self.buffer.select_range(range.start + to8(sel.start)..range.start + to8(sel.end));
        }
        self.changed(cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let start = self.buffer.offset_from_utf16(range_utf16.start);
        let end = self.buffer.offset_from_utf16(range_utf16.end);
        let line = self.buffer.line_of(start);
        let (_, shaped) = self.last_lines.iter().find(|(l, _)| *l == line)?;
        let ls = self.buffer.line_start(line);
        let le = self.buffer.line_end(line);
        let y = bounds.top() + px(line as f32 * LINE_HEIGHT - self.scroll_y);
        let x0 = bounds.left() + px(self.text_x - self.scroll_x) + shaped.x_for_index(start - ls);
        let x1 = bounds.left() + px(self.text_x - self.scroll_x) + shaped.x_for_index(end.min(le) - ls);
        Some(Bounds::from_corners(point(x0, y), point(x1, y + px(LINE_HEIGHT))))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let idx = self.index_for_position(point);
        Some(self.buffer.offset_to_utf16(idx))
    }
}

// ---------------------------------------------------------------- élément

struct EditorElement {
    editor: Entity<TextEditor>,
}

struct LinePaint {
    line: usize,
    y: f32,
    shaped: ShapedLine,
    number: Option<ShapedLine>,
}

struct Prepaint {
    lines: Vec<LinePaint>,
    gutter_w: f32,
    selections: Vec<Bounds<Pixels>>,
    highlights: Vec<Bounds<Pixels>>,
    cursor: Option<Bounds<Pixels>>,
    current_line: Option<Bounds<Pixels>>,
    marks: Vec<(f32, Severity)>,
    error_lines: Vec<Bounds<Pixels>>,
    placeholder: Option<ShapedLine>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

fn run(len: usize, color: Hsla, italic: bool) -> TextRun {
    let mut f = font(FONT_FAMILY);
    if italic {
        f.style = gpui::FontStyle::Italic;
    }
    TextRun { len, font: f, color, background_color: None, underline: None, strikethrough: None }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let single = self.editor.read(cx).single_line;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = if single { px(LINE_HEIGHT).into() } else { relative(1.).into() };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let text_system = window.text_system().clone();
        let fsize = px(FONT_SIZE);
        // largeur d'un caractère
        let probe = text_system.shape_line("0000000000".into(), fsize, &[run(10, rgb(0).into(), false)], None);
        let char_w = f32::from(probe.width) / 10.0;
        let height: f32 = bounds.size.height.into();
        let width: f32 = bounds.size.width.into();

        self.editor.update(cx, |ed, _| {
            ed.ensure_states();
            ed.char_w = char_w;
            ed.last_bounds = Some(bounds);
        });
        let ed = self.editor.read(cx);
        let single = ed.single_line;
        let line_count = ed.buffer.line_count();
        let digits = line_count.to_string().len().max(3);
        let gutter_w = if single { 4.0 } else { digits as f32 * char_w + 30.0 };

        // défilement automatique vers le curseur
        let cursor = ed.buffer.cursor();
        let (cl, _) = ed.buffer.point_of(cursor);
        let mut scroll_y = ed.scroll_y;
        let mut scroll_x = ed.scroll_x;
        let autoscroll = ed.autoscroll;
        if autoscroll {
            let cy = cl as f32 * LINE_HEIGHT;
            if cy < scroll_y {
                scroll_y = cy;
            } else if cy + LINE_HEIGHT > scroll_y + height {
                scroll_y = cy + LINE_HEIGHT - height;
            }
        }
        if single {
            scroll_y = 0.0;
        }
        let first = (scroll_y / LINE_HEIGHT).floor().max(0.0) as usize;
        let last = ((scroll_y + height) / LINE_HEIGHT).ceil() as usize + 1;
        let mut lines = Vec::new();
        let marks_src = ed.line_marks.clone();
        for l in first..last.min(line_count) {
            let range = ed.buffer.line_range(l);
            let text = ed.buffer.text()[range.clone()].to_string();
            let mut st = ed.states_cache.1.get(l).copied().unwrap_or_default();
            let segs = highlight_line(&text, &mut st);
            let mut runs: Vec<TextRun> = segs
                .iter()
                .map(|(r, t)| run(r.len(), token_color(*t), *t == Token::Comment))
                .collect();
            if runs.is_empty() {
                runs.push(run(0, token_color(Token::Plain), false));
            }
            // texte en cours de composition (IME) souligné
            if let Some(m) = ed.marked_range.as_ref() {
                if m.start >= range.start && m.end <= range.end {
                    let _ = m;
                }
            }
            let shaped = text_system.shape_line(SharedString::from(text), fsize, &runs, None);
            let number = if single {
                None
            } else {
                let s = format!("{}", l + 1);
                let col: Hsla = if l == cl { rgb(0x3b4252).into() } else { rgb(0xa0a7b4).into() };
                Some(text_system.shape_line(SharedString::from(s.clone()), px(12.5), &[run(s.len(), col, false)], None))
            };
            lines.push(LinePaint { line: l, y: l as f32 * LINE_HEIGHT - scroll_y, shaped, number });
        }

        // défilement horizontal vers le curseur
        if autoscroll {
            if let Some(lp) = lines.iter().find(|lp| lp.line == cl) {
                let cx_ = f32::from(lp.shaped.x_for_index(cursor - ed.buffer.line_start(cl)));
                let avail = width - gutter_w - 12.0;
                if cx_ < scroll_x {
                    scroll_x = (cx_ - 20.0).max(0.0);
                } else if cx_ > scroll_x + avail {
                    scroll_x = cx_ - avail + 20.0;
                }
            }
        }

        let origin_x = bounds.left() + px(gutter_w + 6.0 - scroll_x);
        let top = bounds.top();
        let x_at = |lp: &LinePaint, local: usize| origin_x + lp.shaped.x_for_index(local);
        let mut selections = Vec::new();
        let sel = ed.buffer.selection.clone();
        let mut highlights = Vec::new();
        for lp in &lines {
            let ls = ed.buffer.line_start(lp.line);
            let le = ed.buffer.line_end(lp.line);
            let y0 = top + px(lp.y);
            let y1 = y0 + px(LINE_HEIGHT);
            if !sel.is_empty() && sel.start <= le && sel.end >= ls {
                let a = sel.start.max(ls) - ls;
                let b = sel.end.min(le) - ls;
                let mut x1 = x_at(lp, b);
                if sel.end > le {
                    x1 += px(char_w * 0.6);
                }
                selections.push(Bounds::from_corners(point(x_at(lp, a), y0), point(x1, y1)));
            }
            for h in &ed.highlights {
                if h.start >= ls && h.end <= le {
                    highlights.push(Bounds::from_corners(
                        point(x_at(lp, h.start - ls), y0),
                        point(x_at(lp, h.end - ls), y1),
                    ));
                }
            }
        }
        let cursor_b = lines.iter().find(|lp| lp.line == cl).map(|lp| {
            let x = x_at(lp, cursor - ed.buffer.line_start(cl));
            Bounds::new(point(x, top + px(lp.y + 2.0)), size(px(2.0), px(LINE_HEIGHT - 4.0)))
        });
        let current_line = if single || ed.buffer.has_selection() {
            None
        } else {
            Some(Bounds::new(
                point(bounds.left(), top + px(cl as f32 * LINE_HEIGHT - scroll_y)),
                size(bounds.size.width, px(LINE_HEIGHT)),
            ))
        };
        let mut marks = Vec::new();
        let mut error_lines = Vec::new();
        for (l, sev, _) in &marks_src {
            if *l >= first && *l < last {
                let y = *l as f32 * LINE_HEIGHT - scroll_y;
                marks.push((y, *sev));
                if *sev == Severity::Error {
                    error_lines.push(Bounds::new(
                        point(bounds.left() + px(gutter_w), top + px(y)),
                        size(bounds.size.width - px(gutter_w), px(LINE_HEIGHT)),
                    ));
                }
            }
        }
        let placeholder = if ed.buffer.is_empty() && !ed.placeholder.is_empty() {
            let p = ed.placeholder.clone();
            let n = p.len();
            Some(text_system.shape_line(p, fsize, &[run(n, rgb(0xa0a7b4).into(), true)], None))
        } else {
            None
        };
        let _ = ed;
        self.editor.update(cx, |ed, _| {
            ed.scroll_y = scroll_y;
            ed.scroll_x = scroll_x;
            ed.autoscroll = false;
            ed.text_x = gutter_w + 6.0;
            ed.last_lines = lines.iter().map(|lp| (lp.line, lp.shaped.clone())).collect();
        });
        Prepaint {
            lines,
            gutter_w,
            selections,
            highlights,
            cursor: cursor_b,
            current_line,
            marks,
            error_lines,
            placeholder,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.editor.read(cx).focus_handle.clone();
        let single = self.editor.read(cx).single_line;
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(b) = pre.current_line {
                window.paint_quad(fill(b, rgb(0xf3f5fa)));
            }
            for b in &pre.error_lines {
                window.paint_quad(fill(*b, gpui::rgba(0xd32f2f14)));
            }
            for b in &pre.highlights {
                window.paint_quad(fill(*b, gpui::rgba(0xffcc0066)));
            }
            for b in &pre.selections {
                window.paint_quad(fill(*b, gpui::rgba(0x3a6ea540)));
            }
            let text_mask = Bounds::from_corners(
                point(bounds.left() + px(pre.gutter_w), bounds.top()),
                bounds.bottom_right(),
            );
            window.with_content_mask(Some(ContentMask { bounds: text_mask }), |window| {
                let ox = self.editor.read(cx).text_x - self.editor.read(cx).scroll_x;
                if let Some(p) = pre.placeholder.as_ref() {
                    let _ = p.paint(point(bounds.left() + px(ox), bounds.top()), px(LINE_HEIGHT), window, cx);
                }
                for lp in &pre.lines {
                    let _ = lp.shaped.paint(
                        point(bounds.left() + px(ox), bounds.top() + px(lp.y)),
                        px(LINE_HEIGHT),
                        window,
                        cx,
                    );
                }
                if focus.is_focused(window) {
                    if let Some(c) = pre.cursor {
                        window.paint_quad(fill(c, rgb(0x1d5fc4)));
                    }
                }
            });
            if !single {
                let gutter = Bounds::new(bounds.origin, size(px(pre.gutter_w), bounds.size.height));
                window.paint_quad(fill(gutter, rgb(0xf7f8fb)));
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.left() + px(pre.gutter_w - 1.0), bounds.top()),
                        size(px(1.0), bounds.size.height),
                    ),
                    rgb(0xe3e6ee),
                ));
                for lp in &pre.lines {
                    if let Some(n) = lp.number.as_ref() {
                        let x = bounds.left() + px(pre.gutter_w - 10.0) - n.width;
                        let _ = n.paint(point(x, bounds.top() + px(lp.y + 1.5)), px(LINE_HEIGHT - 3.0), window, cx);
                    }
                }
                for (y, sev) in &pre.marks {
                    let color = match sev {
                        Severity::Error => rgb(0xd32f2f),
                        Severity::Warning => rgb(0xe0a100),
                        Severity::Info => rgb(0x3a6ea5),
                    };
                    let b = Bounds::new(
                        point(bounds.left() + px(5.0), bounds.top() + px(y + 6.0)),
                        size(px(8.0), px(8.0)),
                    );
                    window.paint_quad(fill(b, color).corner_radii(px(4.0)));
                }
            }
        });
    }
}

impl Render for TextEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("text-editor")
            .key_context(if self.single_line { "TextInput" } else { "TextEditor" })
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .size_full()
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::backspace_word))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::delete_word))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::doc_start))
            .on_action(cx.listener(Self::doc_end))
            .on_action(cx.listener(Self::select_doc_start))
            .on_action(cx.listener(Self::select_doc_end))
            .on_action(cx.listener(Self::page_up))
            .on_action(cx.listener(Self::page_down))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::tab))
            .on_action(cx.listener(Self::outdent))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(Self::toggle_comment))
            .on_action(cx.listener(Self::duplicate_line))
            .on_action(cx.listener(Self::delete_line))
            .on_action(cx.listener(Self::move_line_up))
            .on_action(cx.listener(Self::move_line_down))
            .on_action(cx.listener(Self::escape))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .child(EditorElement { editor: cx.entity() })
    }
}
