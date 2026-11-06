//! Vue d'un `EditorViewModel` : dessine les lignes visibles (coloration,
//! gouttière, sélection, curseur, diagnostics) et traduit clavier, souris et
//! méthode de saisie en commandes du ViewModel.
//!
//! Optimisations reprises de Zed : seules les lignes visibles sont mises en
//! forme (virtualisation), la mise en forme du texte est mise en cache par
//! GPUI d'une image à l'autre, et la coloration est incrémentale.

use std::ops::Range;

use gpui::{
    App, Bounds, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollWheelEvent, ShapedLine,
    SharedString, Style, Subscription, TextRun, UTF16Selection, Window, div, fill, font, point,
    prelude::*, px, relative, size,
};

use super::actions::*;
use super::completion_menu::completion_menu;
use super::theme;
use crate::model::diagram::Severity;
use crate::model::syntax::Token;
use crate::viewmodel::editor_vm::{EditorEvent, EditorViewModel, Motion};

/// Mesures du dernier affichage, nécessaires pour convertir une position
/// de souris en position dans le texte.
#[derive(Default)]
struct LayoutCache {
    bounds: Option<Bounds<Pixels>>,
    lines: Vec<(usize, ShapedLine)>,
    text_x: f32,
    /// Coin inférieur gauche du curseur (fenêtre), pour placer le menu.
    cursor_anchor: Option<Point<Pixels>>,
}

pub struct EditorView {
    vm: Entity<EditorViewModel>,
    focus_handle: FocusHandle,
    layout: LayoutCache,
    selecting: bool,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for EditorView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EditorView {
    pub fn new(vm: Entity<EditorViewModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subs = vec![
            cx.observe(&vm, |_, _, cx| cx.notify()),
            cx.subscribe_in(&vm, window, |_, _, ev: &EditorEvent, window, _| match ev {
                EditorEvent::FocusNext => window.focus_next(),
                EditorEvent::FocusPrev => window.focus_prev(),
                _ => {}
            }),
        ];
        Self { vm, focus_handle: cx.focus_handle(), layout: LayoutCache::default(), selecting: false, _subscriptions: subs }
    }

    // ------------------------------------------------------------ souris

    fn offset_at(&self, pos: Point<Pixels>, cx: &App) -> usize {
        let Some(bounds) = self.layout.bounds else { return 0 };
        let vm = self.vm.read(cx);
        let buffer = &vm.buffer;
        let y = f32::from(pos.y - bounds.top()) + vm.scroll_y;
        let line = if vm.single_line { 0 } else { (y / vm.line_height()).floor().max(0.0) as usize };
        if line >= buffer.line_count() {
            return buffer.len();
        }
        let x = f32::from(pos.x - bounds.left()) - self.layout.text_x + vm.scroll_x;
        let start = buffer.line_start(line);
        match self.layout.lines.iter().find(|(l, _)| *l == line) {
            Some((_, shaped)) => start + shaped.closest_index_for_x(px(x.max(0.0))),
            None => start,
        }
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus_handle);
        let offset = self.offset_at(ev.position, cx);
        self.selecting = true;
        self.vm.update(cx, |vm, cx| vm.click(offset, ev.click_count, ev.modifiers.shift, cx));
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
            return;
        }
        let offset = self.offset_at(ev.position, cx);
        self.vm.update(cx, |vm, cx| vm.drag_to(offset, cx));
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.selecting = false;
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let d = ev.delta.pixel_delta(px(self.vm.read(cx).line_height()));
        let (dx, dy) = (f32::from(d.x), f32::from(d.y));
        let (dx, dy) = if ev.modifiers.shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
        self.vm.update(cx, |vm, cx| vm.scroll_by(dx, dy, cx));
    }
}

// ---------------------------------------------------------------- saisie (IME)

impl EntityInputHandler for EditorView {
    fn text_for_range(&mut self, r16: Range<usize>, actual: &mut Option<Range<usize>>, _: &mut Window, cx: &mut Context<Self>) -> Option<String> {
        let b = &self.vm.read(cx).buffer;
        let r = b.offset_from_utf16(r16.start)..b.offset_from_utf16(r16.end);
        actual.replace(b.offset_to_utf16(r.start)..b.offset_to_utf16(r.end));
        Some(b.text()[r].to_string())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, cx: &mut Context<Self>) -> Option<UTF16Selection> {
        let b = &self.vm.read(cx).buffer;
        Some(UTF16Selection {
            range: b.offset_to_utf16(b.selection.start)..b.offset_to_utf16(b.selection.end),
            reversed: b.reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let vm = self.vm.read(cx);
        vm.marked_range.as_ref().map(|r| vm.buffer.offset_to_utf16(r.start)..vm.buffer.offset_to_utf16(r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.vm.update(cx, |vm, _| vm.marked_range = None);
    }

    fn replace_text_in_range(&mut self, r16: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.vm.update(cx, |vm, cx| {
            let r = r16.map(|r| vm.buffer.offset_from_utf16(r.start)..vm.buffer.offset_from_utf16(r.end));
            vm.replace_text(r, text, false, cx);
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        r16: Option<Range<usize>>,
        text: &str,
        _selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.vm.update(cx, |vm, cx| {
            let r = r16.map(|r| vm.buffer.offset_from_utf16(r.start)..vm.buffer.offset_from_utf16(r.end));
            vm.replace_text(r, text, true, cx);
        });
    }

    fn bounds_for_range(&mut self, r16: Range<usize>, bounds: Bounds<Pixels>, _: &mut Window, cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let vm = self.vm.read(cx);
        let b = &vm.buffer;
        let (start, end) = (b.offset_from_utf16(r16.start), b.offset_from_utf16(r16.end));
        let line = b.line_of(start);
        let (_, shaped) = self.layout.lines.iter().find(|(l, _)| *l == line)?;
        let ls = b.line_start(line);
        let x = bounds.left() + px(self.layout.text_x - vm.scroll_x);
        let line_height = vm.line_height();
        let y = bounds.top() + px(line as f32 * line_height - vm.scroll_y);
        Some(Bounds::from_corners(
            point(x + shaped.x_for_index(start - ls), y),
            point(x + shaped.x_for_index(end.min(b.line_end(line)) - ls), y + px(line_height)),
        ))
    }

    fn character_index_for_point(&mut self, p: Point<Pixels>, _: &mut Window, cx: &mut Context<Self>) -> Option<usize> {
        let offset = self.offset_at(p, cx);
        Some(self.vm.read(cx).buffer.offset_to_utf16(offset))
    }
}

// ---------------------------------------------------------------- élément de dessin

struct EditorElement {
    view: Entity<EditorView>,
}

struct VisibleLine {
    y: f32,
    shaped: ShapedLine,
    number: Option<ShapedLine>,
}

/// Tout ce qui est calculé avant de peindre (une image).
struct Frame {
    line_height: f32,
    lines: Vec<VisibleLine>,
    gutter_w: f32,
    text_x: f32,
    selections: Vec<Bounds<Pixels>>,
    matches: Vec<Bounds<Pixels>>,
    cursor: Option<Bounds<Pixels>>,
    current_line: Option<Bounds<Pixels>>,
    marks: Vec<(f32, Severity)>,
    placeholder: Option<ShapedLine>,
}

fn code_run(len: usize, color: Hsla, italic: bool) -> TextRun {
    let mut f = font(theme::CODE_FONT);
    if italic {
        f.style = gpui::FontStyle::Italic;
    }
    TextRun { len, font: f, color, background_color: None, underline: None, strikethrough: None }
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Frame;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let vm = self.view.read(cx).vm.read(cx);
        let (single, line_height) = (vm.single_line, vm.line_height());
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = if single { px(line_height).into() } else { relative(1.).into() };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), window: &mut Window, cx: &mut App) -> Frame {
        let text_system = window.text_system().clone();
        let vm = self.view.read(cx).vm.clone();
        let (font_size, line_height) = {
            let v = vm.read(cx);
            (px(v.code_size()), v.line_height())
        };
        let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));

        // gouttière : largeur selon le nombre de chiffres
        let probe = text_system.shape_line("0000000000".into(), font_size, &[code_run(10, theme::text().into(), false)], None);
        let char_w = f32::from(probe.width) / 10.0;
        let (single, line_count) = {
            let v = vm.read(cx);
            (v.single_line, v.buffer.line_count())
        };
        let gutter_w = if single { 0.0 } else { line_count.to_string().len().max(3) as f32 * char_w + 30.0 };
        let text_x = gutter_w + if single { 2.0 } else { 6.0 };

        // défilement : d'abord vertical (pour connaître les lignes visibles)
        vm.update(cx, |vm, _| {
            vm.set_viewport(width - text_x, height);
            vm.reveal_cursor_vertically();
        });
        let (scroll_y, cursor_line) = {
            let v = vm.read(cx);
            (v.scroll_y, v.buffer.line_of(v.buffer.cursor()))
        };
        let first = (scroll_y / line_height).floor().max(0.0) as usize;
        let last = (((scroll_y + height) / line_height).ceil() as usize + 1).min(line_count);

        // lignes visibles uniquement (virtualisation)
        let mut lines = Vec::with_capacity(last.saturating_sub(first));
        for l in first..last {
            let (text, segments) = vm.update(cx, |vm, _| (vm.buffer.line_text(l).to_string(), vm.highlighted_line(l)));
            let mut runs: Vec<TextRun> = segments.iter().map(|(r, t)| code_run(r.len(), theme::token(*t), *t == Token::Comment)).collect();
            if runs.is_empty() {
                runs.push(code_run(0, theme::token(Token::Plain), false));
            }
            let shaped = text_system.shape_line(SharedString::from(text), font_size, &runs, None);
            let number = (!single).then(|| {
                let s = (l + 1).to_string();
                let color = if l == cursor_line { theme::text() } else { theme::faint() };
                text_system.shape_line(SharedString::from(s.clone()), font_size - px(1.5), &[code_run(s.len(), color.into(), false)], None)
            });
            lines.push(VisibleLine { y: l as f32 * line_height - scroll_y, shaped, number });
        }

        // défilement horizontal vers le curseur
        let cursor_x = lines.get(cursor_line.wrapping_sub(first)).map(|vl| {
            let v = vm.read(cx);
            f32::from(vl.shaped.x_for_index(v.buffer.cursor() - v.buffer.line_start(cursor_line)))
        });
        vm.update(cx, |vm, _| vm.reveal_cursor_horizontally(cursor_x));

        let v = vm.read(cx);
        let b = &v.buffer;
        let origin_x = bounds.left() + px(text_x - v.scroll_x);
        let top = bounds.top();
        let x_at = |vl: &VisibleLine, local: usize| origin_x + vl.shaped.x_for_index(local);
        let mut selections = Vec::new();
        let mut matches = Vec::new();
        let sel = b.selection.clone();
        for (i, vl) in lines.iter().enumerate() {
            let l = first + i;
            let (ls, le) = (b.line_start(l), b.line_end(l));
            let (y0, y1) = (top + px(vl.y), top + px(vl.y + line_height));
            if !sel.is_empty() && sel.start <= le && sel.end >= ls {
                let mut x1 = x_at(vl, sel.end.min(le) - ls);
                if sel.end > le {
                    x1 += px(char_w * 0.6); // le retour à la ligne est sélectionné
                }
                selections.push(Bounds::from_corners(point(x_at(vl, sel.start.max(ls) - ls), y0), point(x1, y1)));
            }
            for h in v.highlights.iter().filter(|h| h.start >= ls && h.end <= le) {
                matches.push(Bounds::from_corners(point(x_at(vl, h.start - ls), y0), point(x_at(vl, h.end - ls), y1)));
            }
        }
        let cursor = lines.get(cursor_line.wrapping_sub(first)).map(|vl| {
            let x = x_at(vl, b.cursor() - b.line_start(cursor_line));
            Bounds::new(point(x, top + px(vl.y + 2.0)), size(px(2.0), px(line_height - 4.0)))
        });
        let current_line = (!single && !b.has_selection()).then(|| {
            Bounds::new(point(bounds.left(), top + px(cursor_line as f32 * line_height - v.scroll_y)), size(bounds.size.width, px(line_height)))
        });
        let marks = v
            .line_marks
            .iter()
            .filter(|m| m.line >= first && m.line < last)
            .map(|m| (m.line as f32 * line_height - v.scroll_y, m.severity))
            .collect();
        let placeholder = (b.is_empty() && !v.placeholder.is_empty()).then(|| {
            let p = v.placeholder.clone();
            let n = p.len();
            text_system.shape_line(p, font_size, &[code_run(n, theme::faint().into(), true)], None)
        });

        let view_lines = lines.iter().enumerate().map(|(i, vl)| (first + i, vl.shaped.clone())).collect();
        self.view.update(cx, |view, _| {
            let cursor_anchor = cursor.map(|c| point(c.left(), c.bottom() + px(2.0)));
            view.layout = LayoutCache { bounds: Some(bounds), lines: view_lines, text_x, cursor_anchor };
        });
        Frame { line_height, lines, gutter_w, text_x, selections, matches, cursor, current_line, marks, placeholder }
    }

    fn paint(&mut self, _: Option<&GlobalElementId>, _: Option<&gpui::InspectorElementId>, bounds: Bounds<Pixels>, _: &mut (), frame: &mut Frame, window: &mut Window, cx: &mut App) {
        let focus = self.view.read(cx).focus_handle.clone();
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.view.clone()), cx);
        let scroll_x = self.view.read(cx).vm.read(cx).scroll_x;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(b) = frame.current_line {
                window.paint_quad(fill(b, theme::current_line()));
            }
            for (y, sev) in &frame.marks {
                if *sev == Severity::Error {
                    let b = Bounds::new(point(bounds.left() + px(frame.gutter_w), bounds.top() + px(*y)), size(bounds.size.width, px(frame.line_height)));
                    window.paint_quad(fill(b, theme::error_line()));
                }
            }
            for b in &frame.matches {
                window.paint_quad(fill(*b, theme::search_match()));
            }
            for b in &frame.selections {
                window.paint_quad(fill(*b, theme::selection()));
            }
            // le texte ne déborde jamais sur la gouttière
            let text_area = Bounds::from_corners(point(bounds.left() + px(frame.gutter_w), bounds.top()), bounds.bottom_right());
            window.with_content_mask(Some(ContentMask { bounds: text_area }), |window| {
                let x = bounds.left() + px(frame.text_x - scroll_x);
                if let Some(p) = &frame.placeholder {
                    let _ = p.paint(point(x, bounds.top()), px(frame.line_height), window, cx);
                }
                for vl in &frame.lines {
                    let _ = vl.shaped.paint(point(x, bounds.top() + px(vl.y)), px(frame.line_height), window, cx);
                }
                if focus.is_focused(window) {
                    if let Some(c) = frame.cursor {
                        window.paint_quad(fill(c, theme::cursor()));
                    }
                }
            });
            if frame.gutter_w > 0.0 {
                paint_gutter(frame, bounds, window, cx);
            }
        });
    }
}

fn paint_gutter(frame: &Frame, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let w = px(frame.gutter_w);
    window.paint_quad(fill(Bounds::new(bounds.origin, size(w, bounds.size.height)), theme::gutter()));
    window.paint_quad(fill(Bounds::new(point(bounds.left() + w - px(1.0), bounds.top()), size(px(1.0), bounds.size.height)), theme::border()));
    for vl in &frame.lines {
        if let Some(n) = &vl.number {
            let x = bounds.left() + w - px(10.0) - n.width;
            let _ = n.paint(point(x, bounds.top() + px(vl.y + 1.5)), px(frame.line_height - 3.0), window, cx);
        }
    }
    for (y, sev) in &frame.marks {
        let dot = Bounds::new(point(bounds.left() + px(5.0), bounds.top() + px(y + (frame.line_height - 8.0) / 2.0)), size(px(8.0), px(8.0)));
        window.paint_quad(fill(dot, theme::severity(*sev)).corner_radii(px(4.0)));
    }
}

// ---------------------------------------------------------------- liaison des actions

/// Associe chaque action clavier à une commande du ViewModel, en une table lisible.
macro_rules! bind_commands {
    ($el:expr, $cx:expr, { $( $action:ty => |$vm:ident, $c:ident| $body:expr ),* $(,)? }) => {
        $el $(
            .on_action($cx.listener(|this: &mut EditorView, _: &$action, _: &mut Window, cx: &mut Context<EditorView>| {
                this.vm.update(cx, |$vm, $c| { $body; });
            }))
        )*
    };
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (single, showing_completions) = {
            let vm = self.vm.read(cx);
            (vm.single_line, vm.completions.is_some())
        };
        let el = div()
            .id("editor")
            .key_context({
                let mut context = gpui::KeyContext::new_with_defaults();
                context.add(if single { "TextInput" } else { "TextEditor" });
                if showing_completions {
                    context.add("showing_completions");
                }
                context
            })
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll));
        let el = bind_commands!(el, cx, {
            Backspace => |vm, cx| vm.delete(true, false, cx),
            BackspaceWord => |vm, cx| vm.delete(true, true, cx),
            Delete => |vm, cx| vm.delete(false, false, cx),
            DeleteWord => |vm, cx| vm.delete(false, true, cx),
            Left => |vm, cx| vm.move_cursor(Motion::Left, false, cx),
            Right => |vm, cx| vm.move_cursor(Motion::Right, false, cx),
            Up => |vm, cx| vm.move_cursor(Motion::Up, false, cx),
            Down => |vm, cx| vm.move_cursor(Motion::Down, false, cx),
            WordLeft => |vm, cx| vm.move_cursor(Motion::WordLeft, false, cx),
            WordRight => |vm, cx| vm.move_cursor(Motion::WordRight, false, cx),
            SelectLeft => |vm, cx| vm.move_cursor(Motion::Left, true, cx),
            SelectRight => |vm, cx| vm.move_cursor(Motion::Right, true, cx),
            SelectUp => |vm, cx| vm.move_cursor(Motion::Up, true, cx),
            SelectDown => |vm, cx| vm.move_cursor(Motion::Down, true, cx),
            SelectWordLeft => |vm, cx| vm.move_cursor(Motion::WordLeft, true, cx),
            SelectWordRight => |vm, cx| vm.move_cursor(Motion::WordRight, true, cx),
            Home => |vm, cx| vm.move_cursor(Motion::Home, false, cx),
            End => |vm, cx| vm.move_cursor(Motion::End, false, cx),
            SelectHome => |vm, cx| vm.move_cursor(Motion::Home, true, cx),
            SelectEnd => |vm, cx| vm.move_cursor(Motion::End, true, cx),
            DocStart => |vm, cx| vm.move_cursor(Motion::DocStart, false, cx),
            DocEnd => |vm, cx| vm.move_cursor(Motion::DocEnd, false, cx),
            SelectDocStart => |vm, cx| vm.move_cursor(Motion::DocStart, true, cx),
            SelectDocEnd => |vm, cx| vm.move_cursor(Motion::DocEnd, true, cx),
            PageUp => |vm, cx| vm.move_cursor(Motion::PageUp, false, cx),
            PageDown => |vm, cx| vm.move_cursor(Motion::PageDown, false, cx),
            SelectAll => |vm, cx| vm.select_all(cx),
            Newline => |vm, cx| vm.newline(cx),
            Tab => |vm, cx| vm.indent(cx),
            Outdent => |vm, cx| vm.outdent(cx),
            Escape => |vm, cx| vm.escape(cx),
            Copy => |vm, cx| vm.copy(cx),
            Cut => |vm, cx| vm.cut(cx),
            Paste => |vm, cx| vm.paste(cx),
            Undo => |vm, cx| vm.undo(cx),
            Redo => |vm, cx| vm.redo(cx),
            ToggleComment => |vm, cx| vm.line_op(|b| b.toggle_comment(), cx),
            DuplicateLine => |vm, cx| vm.line_op(|b| b.duplicate_lines(), cx),
            DeleteLine => |vm, cx| vm.line_op(|b| b.delete_lines(), cx),
            MoveLineUp => |vm, cx| vm.line_op(|b| b.move_lines(false), cx),
            MoveLineDown => |vm, cx| vm.line_op(|b| b.move_lines(true), cx),
            ShowCompletions => |vm, cx| vm.show_completions(cx),
            ConfirmCompletion => |vm, cx| vm.confirm_completion(None, cx),
            SelectNextCompletion => |vm, cx| vm.select_completion(1, cx),
            SelectPrevCompletion => |vm, cx| vm.select_completion(-1, cx),
            DismissCompletions => |vm, cx| vm.dismiss_completions(cx),
        });
        let menu = match (self.vm.read(cx).completions.clone(), self.layout.cursor_anchor) {
            (Some(menu), Some(anchor)) if self.focus_handle.is_focused(window) => Some(completion_menu(&menu, anchor, self.vm.clone())),
            _ => None,
        };
        el.child(EditorElement { view: cx.entity() }).children(menu)
    }
}
