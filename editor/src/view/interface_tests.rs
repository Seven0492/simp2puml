//! Parcours réels de l'interface sur la plateforme de test de GPUI : vrais
//! clics, vrais raccourcis, vraie frappe, sans fenêtre ni session graphique.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};

use super::theme;
use super::workspace_view::WorkspaceView;
use crate::keymap;
use crate::model::preferences::{Preferences, ThemeChoice};
use crate::services::preferences_service;
use crate::viewmodel::workspace_vm::{PathPurpose, WorkspaceViewModel};

/// Palette et fichier de préférences sont partagés : un parcours à la fois.
pub static INTERFACE: Mutex<()> = Mutex::new(());

/// Dossier de configuration des tests, jamais celui de l'utilisateur.
fn test_dir() -> &'static PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("puml-editor-interface-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY : posé une seule fois, avant toute lecture des préférences de ce processus.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        dir
    })
}

struct Session<'a> {
    _guard: MutexGuard<'static, ()>,
    vm: Entity<WorkspaceViewModel>,
    window: &'a mut VisualTestContext,
}

/// Ouvre l'éditeur sur des préférences par défaut.
fn open<'a>(cx: &'a mut TestAppContext, paths: Vec<PathBuf>) -> Session<'a> {
    let guard = INTERFACE.lock().unwrap_or_else(|e| e.into_inner());
    test_dir();
    let _ = std::fs::remove_file(preferences_service::path().unwrap());
    cx.update(|cx| cx.bind_keys(keymap::bindings()));
    let (view, window) = cx.add_window_view(|window, cx| {
        let vm = cx.new(|cx| WorkspaceViewModel::new(paths, cx));
        WorkspaceView::new(vm, window, cx)
    });
    let vm = view.read_with(window, |v, _| v.view_model());
    window.run_until_parked();
    Session { _guard: guard, vm, window }
}

impl Session<'_> {
    fn click(&mut self, selector: &'static str) {
        self.window.run_until_parked();
        let bounds = self.window.debug_bounds(selector).unwrap_or_else(|| panic!("élément absent : {selector}"));
        self.window.simulate_click(bounds.center(), Modifiers::none());
        self.window.run_until_parked();
    }

    fn preferences(&self) -> Preferences {
        self.vm.read_with(self.window, |vm, _| vm.preferences.clone())
    }

    fn text(&self) -> String {
        self.window.read(|cx| self.vm.read(cx).active_editor(cx).read(cx).text().to_string())
    }

    fn tabs(&self) -> usize {
        self.vm.read_with(self.window, |vm, _| vm.documents.len())
    }

    fn status(&self) -> String {
        self.vm.read_with(self.window, |vm, _| vm.status.as_ref().map(|s| s.message.clone()).unwrap_or_default())
    }
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    let dir = test_dir().join(format!("fichiers-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// Force une empreinte différente, sans dépendre de la résolution des dates.
fn modify_outside(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
}

#[gpui::test]
fn preferences_menu_applies_and_saves_each_click(cx: &mut TestAppContext) {
    let mut s = open(cx, vec![]);
    let saved = preferences_service::path().unwrap();
    assert!(!saved.exists(), "rien n'est écrit tant qu'aucun réglage ne change");
    assert_eq!(s.preferences(), Preferences::default());

    // le raccourci ouvre et ferme la barre
    s.window.simulate_keystrokes("ctrl-,");
    assert!(s.vm.read_with(s.window, |vm, _| vm.preferences_open));
    s.window.simulate_keystrokes("ctrl-,");
    assert!(!s.vm.read_with(s.window, |vm, _| vm.preferences_open));

    // l'engrenage aussi
    s.click("tb-preferences");
    assert!(s.vm.read_with(s.window, |vm, _| vm.preferences_open));

    // « Système » suit la fenêtre (claire sur la plateforme de test)
    assert!(!theme::is_dark());
    s.click("pref-theme-Sombre");
    assert_eq!(s.preferences().theme, ThemeChoice::Dark);
    assert!(theme::is_dark(), "la palette change au clic");
    assert!(std::fs::read_to_string(&saved).unwrap().contains("theme = dark"), "le premier clic crée le fichier");

    s.click("pref-adapt");
    assert!(s.preferences().adapt_diagram);
    s.click("pref-ac-off");
    assert!(!s.preferences().autocomplete);
    let editor = s.window.read(|cx| s.vm.read(cx).active_editor(cx));
    assert!(!editor.read_with(s.window, |e, _| e.autocomplete), "l'éditeur ouvert suit le réglage");

    // taille du texte : appliquée à l'éditeur ouvert et bornée
    let height = editor.read_with(s.window, |e, _| e.line_height());
    s.click("pref-size-plus");
    s.click("pref-size-plus");
    assert_eq!(s.preferences().code_size, 16);
    assert!(editor.read_with(s.window, |e, _| e.line_height()) > height);
    s.click("pref-size-minus");
    assert_eq!(s.preferences().code_size, 15);

    // tout est relu à l'identique au prochain lancement
    let (reloaded, problem) = preferences_service::load();
    assert_eq!(problem, None);
    assert_eq!(reloaded, s.preferences());

    s.click("pref-theme-Clair");
    assert!(!theme::is_dark());
    s.click("pref-close");
    assert!(!s.vm.read_with(s.window, |vm, _| vm.preferences_open));
    theme::set_appearance(false, false);
}

#[gpui::test]
fn saved_preferences_are_applied_at_startup(cx: &mut TestAppContext) {
    let mut s = open(cx, vec![]);
    s.click("tb-preferences");
    s.click("pref-theme-Sombre");
    s.click("pref-ac-off");
    s.click("pref-size-plus");
    let saved = s.preferences();
    let _guard = s._guard;
    // nouvelle fenêtre : lit le fichier écrit par la précédente
    let (view, window) = cx.add_window_view(|window, cx| {
        let vm = cx.new(|cx| WorkspaceViewModel::new(vec![], cx));
        WorkspaceView::new(vm, window, cx)
    });
    window.run_until_parked();
    let vm = view.read_with(window, |v, _| v.view_model());
    let editor = window.read(|cx| vm.read(cx).active_editor(cx));
    assert_eq!(vm.read_with(window, |vm, _| vm.preferences.clone()), saved);
    assert!(theme::is_dark());
    assert!(!editor.read_with(window, |e, _| e.autocomplete));
    assert_eq!(editor.read_with(window, |e, _| e.code_size()), saved.code_size as f32);
    theme::set_appearance(false, false);
}

#[gpui::test]
fn window_title_follows_typing_and_saving(cx: &mut TestAppContext) {
    let path = temp_file("titre.puml", "@startuml\nA -> B\n@enduml\n");
    let s = open(cx, vec![path.clone()]);
    assert_eq!(s.tabs(), 1, "l'onglet de départ est remplacé par le fichier");
    assert_eq!(s.window.window_title().as_deref(), Some("titre.puml — Éditeur PlantUML"));
    s.window.simulate_input("x");
    s.window.run_until_parked();
    assert_eq!(s.window.window_title().as_deref(), Some("• titre.puml — Éditeur PlantUML"), "le point suit la frappe");
    s.window.simulate_keystrokes("ctrl-s");
    s.window.run_until_parked();
    assert_eq!(s.window.window_title().as_deref(), Some("titre.puml — Éditeur PlantUML"));
    assert!(std::fs::read_to_string(&path).unwrap().starts_with('x'));
    assert_eq!(s.status(), "Enregistré : titre.puml");
}

#[gpui::test]
fn saving_over_an_external_change_asks_first(cx: &mut TestAppContext) {
    let path = temp_file("externe.puml", "@startuml\nA -> B\n@enduml\n");
    let s = open(cx, vec![path.clone()]);
    s.window.simulate_input("x");
    modify_outside(&path, "modifié ailleurs, plus long que l'original\n");

    // « Annuler » : le fichier reste tel que l'autre programme l'a écrit
    s.window.simulate_keystrokes("ctrl-s");
    s.window.run_until_parked();
    assert!(s.window.has_pending_prompt(), "une confirmation est demandée");
    s.window.simulate_prompt_answer("Annuler");
    s.window.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "modifié ailleurs, plus long que l'original\n");
    assert!(s.window.read(|cx| s.vm.read(cx).has_unsaved(cx)));

    // « Remplacer » : le texte de l'éditeur est écrit et plus rien n'est demandé
    s.window.simulate_keystrokes("ctrl-s");
    s.window.run_until_parked();
    s.window.simulate_prompt_answer("Remplacer");
    s.window.run_until_parked();
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("x@startuml"));
    assert!(!s.window.read(|cx| s.vm.read(cx).has_unsaved(cx)));
    s.window.simulate_input("y");
    s.window.simulate_keystrokes("ctrl-s");
    s.window.run_until_parked();
    assert!(!s.window.has_pending_prompt());
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("xy@startuml") || std::fs::read_to_string(&path).unwrap().starts_with("yx@startuml"));
}

#[gpui::test]
fn replace_next_only_touches_the_selected_occurrence(cx: &mut TestAppContext) {
    let path = temp_file("remplacer.puml", "Alice -> Bob\nBob -> Alice\nBob -> Carol\n");
    let mut s = open(cx, vec![path]);
    s.window.simulate_keystrokes("ctrl-f");
    s.window.simulate_input("bob");
    s.window.run_until_parked();
    let replacement = s.vm.read_with(s.window, |vm, _| vm.find.replacement.clone());
    replacement.update(s.window, |r, cx| r.set_text("Zoé", cx));

    // rien de sélectionné au départ : le premier appui sélectionne, ne remplace pas
    s.click("replace-next");
    assert_eq!(s.text(), "Alice -> Bob\nBob -> Alice\nBob -> Carol\n");
    s.click("replace-next");
    assert_eq!(s.text(), "Alice -> Zoé\nBob -> Alice\nBob -> Carol\n");
    s.click("replace-next");
    assert_eq!(s.text(), "Alice -> Zoé\nZoé -> Alice\nBob -> Carol\n");
    assert_eq!(s.vm.read_with(s.window, |vm, cx| vm.search_count(cx)), 1, "les surlignages suivent le texte");
}

#[gpui::test]
fn import_replaces_the_pristine_tab(cx: &mut TestAppContext) {
    let dir = test_dir().join("import");
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("projet.simp");
    std::fs::write(
        &source,
        r#"<sim-project version="12.75" multi-file="no"><items/><models><model><abstract-items/><diagram id="v1" name="v1" type="uml-component"><abstract-items><item id="A" type="component" name="A"/></abstract-items><layer/></diagram></model></models></sim-project>"#,
    )
    .unwrap();
    let s = open(cx, vec![source]);
    s.window.run_until_parked();
    let status = s.status();
    if status.contains("introuvable") || status.contains("No module named") {
        eprintln!("convertisseur simp2puml absent : test ignoré ({status})");
        return;
    }
    assert!(status.starts_with("SIMP importé"), "{status}");
    assert_eq!(s.tabs(), 1, "l'onglet vide de départ a laissé sa place à l'import");
    assert!(s.text().contains("@startuml"));
    assert!(s.window.read(|cx| s.vm.read(cx).has_unsaved(cx)), "un import reste à enregistrer");
}

#[gpui::test]
fn exporting_without_extension_adds_it_and_never_overwrites(cx: &mut TestAppContext) {
    let dir = test_dir().join("export");
    std::fs::create_dir_all(&dir).unwrap();
    let s = open(cx, vec![]);
    s.window.run_until_parked();
    let existing = dir.join("deja.svg");
    std::fs::write(&existing, "à garder").unwrap();
    // le sélecteur a confirmé « deja » : ce n'est pas une confirmation pour « deja.svg »
    s.vm.update(s.window, |vm, cx| {
        vm.request_path(PathPurpose::ExportSvg, cx);
        vm.complete_path(PathPurpose::ExportSvg, dir.join("deja"), true, cx);
    });
    s.window.run_until_parked();
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "à garder");
    assert!(s.status().contains("existe déjà") || s.status().contains("Aperçu pas à jour"), "{}", s.status());
}
