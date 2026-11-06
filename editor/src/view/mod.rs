//! Vues : dessin GPUI et traduction des entrées (clavier, souris) en
//! commandes de ViewModel. Aucune règle métier ici.

pub mod actions;
pub mod completion_menu;
pub mod components;
pub mod editor_view;
pub mod preview_view;
pub mod theme;
pub mod workspace_view;

#[cfg(test)]
mod interface_tests;
