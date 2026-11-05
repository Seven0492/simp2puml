//! Raccourcis clavier : `secondary` vaut Cmd sous macOS et Ctrl ailleurs.
//! Contextes : « TextEditor » (éditeur), « TextInput » (champs d'une ligne),
//! sans contexte pour les commandes de l'espace de travail.

use gpui::KeyBinding;

pub fn bindings() -> Vec<KeyBinding> {
    use crate::view::actions as e;
    use crate::view::actions as w;
    let ed = Some("TextEditor");
    let field = Some("TextInput");
    let mut b = Vec::new();
    // commandes communes aux deux types de champs
    for ctx in [ed, field] {
        b.extend([
            KeyBinding::new("backspace", e::Backspace, ctx),
            KeyBinding::new("shift-backspace", e::Backspace, ctx),
            KeyBinding::new("ctrl-backspace", e::BackspaceWord, ctx),
            KeyBinding::new("alt-backspace", e::BackspaceWord, ctx),
            KeyBinding::new("delete", e::Delete, ctx),
            KeyBinding::new("ctrl-delete", e::DeleteWord, ctx),
            KeyBinding::new("left", e::Left, ctx),
            KeyBinding::new("right", e::Right, ctx),
            KeyBinding::new("shift-left", e::SelectLeft, ctx),
            KeyBinding::new("shift-right", e::SelectRight, ctx),
            KeyBinding::new("ctrl-left", e::WordLeft, ctx),
            KeyBinding::new("ctrl-right", e::WordRight, ctx),
            KeyBinding::new("alt-left", e::WordLeft, ctx),
            KeyBinding::new("alt-right", e::WordRight, ctx),
            KeyBinding::new("ctrl-shift-left", e::SelectWordLeft, ctx),
            KeyBinding::new("ctrl-shift-right", e::SelectWordRight, ctx),
            KeyBinding::new("home", e::Home, ctx),
            KeyBinding::new("end", e::End, ctx),
            KeyBinding::new("shift-home", e::SelectHome, ctx),
            KeyBinding::new("shift-end", e::SelectEnd, ctx),
            KeyBinding::new("secondary-a", e::SelectAll, ctx),
            KeyBinding::new("secondary-c", e::Copy, ctx),
            KeyBinding::new("secondary-x", e::Cut, ctx),
            KeyBinding::new("secondary-v", e::Paste, ctx),
            KeyBinding::new("shift-insert", e::Paste, ctx),
            KeyBinding::new("secondary-z", e::Undo, ctx),
            KeyBinding::new("secondary-shift-z", e::Redo, ctx),
            KeyBinding::new("secondary-y", e::Redo, ctx),
            KeyBinding::new("enter", e::Newline, ctx),
            KeyBinding::new("tab", e::Tab, ctx),
            KeyBinding::new("shift-tab", e::Outdent, ctx),
            KeyBinding::new("escape", e::Escape, ctx),
        ]);
    }
    b.extend([
        KeyBinding::new("up", e::Up, ed),
        KeyBinding::new("down", e::Down, ed),
        KeyBinding::new("shift-up", e::SelectUp, ed),
        KeyBinding::new("shift-down", e::SelectDown, ed),
        KeyBinding::new("ctrl-home", e::DocStart, ed),
        KeyBinding::new("ctrl-end", e::DocEnd, ed),
        KeyBinding::new("cmd-up", e::DocStart, ed),
        KeyBinding::new("cmd-down", e::DocEnd, ed),
        KeyBinding::new("ctrl-shift-home", e::SelectDocStart, ed),
        KeyBinding::new("ctrl-shift-end", e::SelectDocEnd, ed),
        KeyBinding::new("pageup", e::PageUp, ed),
        KeyBinding::new("pagedown", e::PageDown, ed),
        KeyBinding::new("secondary-/", e::ToggleComment, ed),
        KeyBinding::new("secondary-d", e::DuplicateLine, ed),
        KeyBinding::new("secondary-shift-k", e::DeleteLine, ed),
        KeyBinding::new("alt-up", e::MoveLineUp, ed),
        KeyBinding::new("alt-down", e::MoveLineDown, ed),
        KeyBinding::new("shift-enter", w::FindPrev, field),
        // espace de travail (actifs partout)
        KeyBinding::new("secondary-n", w::NewFile, None),
        KeyBinding::new("secondary-o", w::OpenFile, None),
        KeyBinding::new("secondary-s", w::Save, None),
        KeyBinding::new("secondary-shift-s", w::SaveAs, None),
        KeyBinding::new("secondary-w", w::CloseTab, None),
        KeyBinding::new("ctrl-tab", w::NextTab, None),
        KeyBinding::new("ctrl-shift-tab", w::PrevTab, None),
        KeyBinding::new("ctrl-pagedown", w::NextTab, None),
        KeyBinding::new("ctrl-pageup", w::PrevTab, None),
        KeyBinding::new("secondary-e", w::ExportSvg, None),
        KeyBinding::new("secondary-i", w::ImportSimp, None),
        KeyBinding::new("secondary-f", w::Find, None),
        KeyBinding::new("secondary-h", w::Find, None),
        KeyBinding::new("f3", w::FindNext, None),
        KeyBinding::new("shift-f3", w::FindPrev, None),
        KeyBinding::new("secondary-g", w::FindNext, None),
        KeyBinding::new("secondary-=", w::ZoomIn, None),
        KeyBinding::new("secondary-+", w::ZoomIn, None),
        KeyBinding::new("secondary--", w::ZoomOut, None),
        KeyBinding::new("secondary-0", w::ZoomReset, None),
        KeyBinding::new("secondary-9", w::ZoomFit, None),
        KeyBinding::new("secondary-j", w::ToggleDiagnostics, None),
        KeyBinding::new("secondary-t", w::ToggleTemplates, None),
        KeyBinding::new("alt-pagedown", w::NextBlock, None),
        KeyBinding::new("alt-pageup", w::PrevBlock, None),
        KeyBinding::new("secondary-q", w::Quit, None),
    ]);
    b
}

