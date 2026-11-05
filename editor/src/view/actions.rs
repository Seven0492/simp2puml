//! Vue : actions clavier. Les vues les traduisent en commandes de ViewModel.

use gpui::actions;

actions!(
    editor,
    [
        Backspace, BackspaceWord, Delete, DeleteWord,
        Left, Right, Up, Down, WordLeft, WordRight,
        SelectLeft, SelectRight, SelectUp, SelectDown, SelectWordLeft, SelectWordRight,
        Home, End, SelectHome, SelectEnd, DocStart, DocEnd, SelectDocStart, SelectDocEnd,
        PageUp, PageDown, SelectAll,
        Newline, Tab, Outdent, Escape,
        Copy, Cut, Paste, Undo, Redo,
        ToggleComment, DuplicateLine, DeleteLine, MoveLineUp, MoveLineDown,
    ]
);

actions!(
    workspace,
    [
        NewFile, OpenFile, Save, SaveAs, CloseTab, NextTab, PrevTab,
        ExportSvg, ImportSimp, ExportSimp,
        Find, FindNext, FindPrev, ReplaceAll,
        ZoomIn, ZoomOut, ZoomReset, ZoomFit,
        ToggleDiagnostics, ToggleTemplates, NextBlock, PrevBlock, Quit,
    ]
);
