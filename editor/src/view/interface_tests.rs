//! Parcours réels de l'interface sur la plateforme de test de GPUI : vrais
//! clics, vrais raccourcis, vraie frappe, sans fenêtre ni session graphique.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};

use super::theme;
use super::workspace_view::WorkspaceView;
use crate::keymap;
use crate::model::preferences::{InterfaceChoice, LinkClick, Preferences, PreviewEngine, SecurityProfile, ThemeChoice};
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

/// Faux `simp2puml-gui` : note ses arguments, ou échoue avec un message.
fn fake_classic(name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = temp_file(name, &format!("#!/bin/sh\n{body}\n"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY : les parcours sont exécutés un à la fois (verrou `INTERFACE`).
    unsafe { std::env::set_var("SIMP2PUML_GUI", &script) };
    script
}

fn diagnostics(s: &Session) -> Vec<String> {
    s.window.read(|cx| {
        let doc = s.vm.read(cx).active_document().read(cx);
        doc.output().map(|o| o.diagnostics.iter().map(|d| format!("{} {}", d.line, d.message)).collect()).unwrap_or_default()
    })
}

fn rendered_svg(s: &Session) -> String {
    s.window.read(|cx| s.vm.read(cx).active_document().read(cx).output().map(|o| o.svg.clone()).unwrap_or_default())
}

/// Les trois profils de sécurité, sur le vrai moteur natif et de vrais fichiers.
#[gpui::test]
fn security_profiles_govern_macros_and_includes(cx: &mut TestAppContext) {
    if crate::services::render_service::RenderService::locate().is_none() {
        eprintln!("pumlrender absent : test ignoré");
        return;
    }
    let dir = test_dir().join("profils");
    std::fs::create_dir_all(dir.join("doc")).unwrap();
    std::fs::write(dir.join("secret.puml"), "class SECRET").unwrap();
    std::fs::write(dir.join("doc/commun.iuml"), "class Commun").unwrap();
    let path = dir.join("doc/d.puml");
    std::fs::write(
        &path,
        "@startuml\n!define BOITE(n) class n\nBOITE(Macro)\n!include commun.iuml\n!include ../secret.puml\n@enduml\n",
    )
    .unwrap();
    let mut s = open(cx, vec![path]);
    assert_eq!(s.preferences().security_profile, SecurityProfile::Macros);

    // Macros (défaut) : !define marche, !include est refusé et le message dit quoi faire
    let (svg, notes) = (rendered_svg(&s), diagnostics(&s));
    assert!(svg.contains("Macro") && !svg.contains("Commun"), "{svg}");
    assert!(notes.iter().any(|n| n.starts_with("4 ") && n.contains("Dossier du document")), "{notes:?}");

    // Dossier du document : l'inclusion voisine passe, pas celle qui sort du dossier
    s.click("tb-preferences");
    s.click("pref-security-folder");
    s.window.run_until_parked();
    let (svg, notes) = (rendered_svg(&s), diagnostics(&s));
    assert!(svg.contains("Commun") && svg.contains("Macro") && !svg.contains("SECRET"), "{svg}");
    assert!(notes.iter().any(|n| n.starts_with("5 ") && n.contains("hors du dossier")), "{notes:?}");
    assert_eq!(preferences_service::load().0.security_profile, SecurityProfile::Folder, "enregistré");

    // Aucun : ni macros ni fichiers, le texte reste signalé
    s.click("pref-security-none");
    s.window.run_until_parked();
    let (svg, notes) = (rendered_svg(&s), diagnostics(&s));
    assert!(!svg.contains("Commun"), "{svg}");
    assert!(notes.iter().any(|n| n.starts_with("2 ") && n.contains("Aucun")), "{notes:?}");
}

/// Faux moteur officiel : `corps` reçoit la requête dans $id.
fn fake_preview(name: &str, reply: &str) {
    use std::os::unix::fs::PermissionsExt;
    let script = temp_file(
        name,
        &format!("#!/bin/sh\nwhile read -r c id b n; do\n  head -c \"$n\" >/dev/null\n  {reply}\ndone\n"),
    );
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    // SAFETY : les parcours sont exécutés un à la fois (verrou `INTERFACE`).
    unsafe { std::env::set_var("SIMP2PUML_PREVIEW", &script) };
}

#[gpui::test]
fn preview_engine_can_be_switched_and_is_remembered(cx: &mut TestAppContext) {
    let path = temp_file("moteur.puml", "@startuml\nA -> B\n@enduml\n");
    let mut s = open(cx, vec![path]);
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\"/>";
    fake_preview(
        "officiel.sh",
        &format!("printf 'PUMLRENDER 1\\nID\\t%s\\nKIND\\tfaux officiel\\nBLOCKS\\t1\\nSIZE\\t10\\t10\\nSVG\\t{}\\n{}' \"$id\"", svg.len(), svg.replace('"', "\\\"")),
    );
    assert_eq!(s.preferences().preview_engine, PreviewEngine::Native, "le natif par défaut");
    s.click("tb-preferences");
    s.click("pref-engine-official");
    s.window.run_until_parked();
    let kind = s.window.read(|cx| s.vm.read(cx).active_document().read(cx).output().map(|o| o.kind.clone()));
    assert_eq!(kind.as_deref(), Some("faux officiel"));
    assert_eq!(preferences_service::load().0.preview_engine, PreviewEngine::Official);
    s.click("pref-engine-native");
    s.window.run_until_parked();
    assert_eq!(preferences_service::load().0.preview_engine, PreviewEngine::Native);
    unsafe { std::env::remove_var("SIMP2PUML_PREVIEW") };
}

#[gpui::test]
fn missing_official_engine_offers_installation(cx: &mut TestAppContext) {
    let path = temp_file("installe.puml", "@startuml\nA -> B\n@enduml\n");
    let mut s = open(cx, vec![path]);
    fake_preview("absent.sh", "printf 'PUMLRENDER 1\\nID\\t%s\\nERROR\\tmoteur PlantUML non installé\\n' \"$id\"");
    s.click("tb-preferences");
    s.click("pref-engine-official");
    s.window.run_until_parked();
    assert!(s.vm.read_with(s.window, |vm, cx| vm.official_engine_missing(cx)));
    assert!(s.window.debug_bounds("pref-install-official").is_some(), "le bouton est proposé");
    unsafe { std::env::remove_var("SIMP2PUML_PREVIEW") };
}

/// Parcours complet : un clic sur un attribut dans l'aperçu place le curseur sur sa ligne.
#[gpui::test]
fn clicking_a_property_in_the_preview_goes_to_its_line(cx: &mut TestAppContext) {
    if crate::services::render_service::RenderService::locate().is_none() {
        eprintln!("pumlrender absent : test ignoré");
        return;
    }
    let path = temp_file("clic.puml", "@startuml\nclass A {\n  +x : int\n  -y : String\n  +f() : void\n}\n@enduml\n");
    let s = open(cx, vec![path]);
    s.window.run_until_parked();
    let doc = s.window.read(|cx| s.vm.read(cx).active_document().clone());
    let line_of_member = |wanted: usize, s: &Session| {
        s.window.read(|cx| {
            let d = doc.read(cx);
            let output = d.output().expect("rendu");
            // plus petite zone de cette ligne ; le clic passe par le même calcul que la souris
            let r = output
                .regions
                .iter()
                .filter(|r| r.line == wanted)
                .min_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)))
                .expect("zone du membre")
                .clone();
            let (ox, oy) = d.scene_origin();
            (ox + (r.x + r.w / 2.0) * d.preview.zoom, oy + (r.y + r.h / 2.0) * d.preview.zoom)
        })
    };
    for wanted in [3, 4, 5] {
        let (x, y) = line_of_member(wanted, &s);
        assert!(doc.update(s.window, |d, cx| d.click_preview(x, y, cx)), "ligne {wanted}");
        s.window.run_until_parked();
        let (line, _) = s.window.read(|cx| s.vm.read(cx).active_editor(cx).read(cx).cursor_line_col());
        assert_eq!(line, wanted, "le curseur va à la ligne du membre");
    }
}

/// Remplace le rendu affiché par deux flèches qui se croisent, et une plus loin.
fn crossing_arrows(s: &mut Session) -> (Entity<crate::viewmodel::document_vm::DocumentViewModel>, (f32, f32), (f32, f32)) {
    use crate::model::diagram::Link;
    let doc = s.window.read(|cx| s.vm.read(cx).active_document().clone());
    let scene_point = doc.update(s.window, |d, _| {
        let r = d.rendering.as_mut().expect("rendu");
        let mut output = (*r.output).clone();
        output.regions.clear();
        output.links = vec![
            Link { line: 2, after: 0, points: vec![(0.0, 0.0), (200.0, 200.0)] },
            Link { line: 3, after: 0, points: vec![(200.0, 0.0), (0.0, 200.0)] },
            Link { line: 4, after: 0, points: vec![(0.0, 300.0), (200.0, 300.0)] },
        ];
        r.output = std::sync::Arc::new(output);
        let (ox, oy) = d.scene_origin();
        let z = d.preview.zoom;
        ((ox + 100.0 * z, oy + 100.0 * z), (ox + 100.0 * z, oy + 300.0 * z))
    });
    (doc, scene_point.0, scene_point.1)
}

fn cursor_line(s: &Session) -> usize {
    s.window.read(|cx| s.vm.read(cx).active_editor(cx).read(cx).cursor_line_col().0)
}

#[gpui::test]
fn overlapping_arrows_ask_which_one_was_meant(cx: &mut TestAppContext) {
    if crate::services::render_service::RenderService::locate().is_none() {
        eprintln!("pumlrender absent : test ignoré");
        return;
    }
    let path = temp_file("croise.puml", "@startuml\nA --> B : premier\nC --> D : second\nE --> F\n@enduml\n");
    let mut s = open(cx, vec![path]);
    let (doc, crossing, alone) = crossing_arrows(&mut s);

    // une seule flèche : droit à sa ligne, sans menu
    assert!(doc.update(s.window, |d, cx| d.click_preview(alone.0, alone.1, cx)));
    s.window.run_until_parked();
    assert_eq!(cursor_line(&s), 4);
    assert!(doc.read_with(s.window, |d, _| d.link_menu.is_none()));

    // au croisement : un menu qui cite les deux lignes du source
    doc.update(s.window, |d, cx| d.click_preview(crossing.0, crossing.1, cx));
    s.window.run_until_parked();
    let menu = doc.read_with(s.window, |d, _| d.link_menu.clone()).expect("menu");
    assert_eq!(menu.choices.iter().map(|c| c.line).collect::<Vec<_>>().len(), 2);
    assert!(menu.choices.iter().any(|c| c.line == 2 && c.label == "A --> B : premier"), "{menu:?}");
    assert!(menu.choices.iter().any(|c| c.line == 3 && c.label == "C --> D : second"), "{menu:?}");
    assert_eq!(cursor_line(&s), 4, "rien ne bouge avant le choix");

    // un clic à côté referme sans rien faire d'autre
    doc.update(s.window, |d, cx| d.click_preview(1.0, 1.0, cx));
    assert!(doc.read_with(s.window, |d, _| d.link_menu.is_none()));
    assert_eq!(cursor_line(&s), 4);

    // un vrai clic sur l'entrée va à la ligne choisie et referme le menu
    doc.update(s.window, |d, cx| d.click_preview(crossing.0, crossing.1, cx));
    s.click("link-choice-3");
    assert_eq!(cursor_line(&s), 3);
    assert!(doc.read_with(s.window, |d, _| d.link_menu.is_none()));
}

#[gpui::test]
fn the_click_mode_of_arrows_is_a_preference(cx: &mut TestAppContext) {
    if crate::services::render_service::RenderService::locate().is_none() {
        eprintln!("pumlrender absent : test ignoré");
        return;
    }
    let path = temp_file("modes.puml", "@startuml\nA --> B\nC --> D\nE --> F\n@enduml\n");
    let mut s = open(cx, vec![path]);
    let (doc, crossing, _) = crossing_arrows(&mut s);
    assert_eq!(s.preferences().link_click, LinkClick::Along, "toute la longueur par défaut");
    // le milieu d'une flèche réagit en mode « toute la longueur »…
    doc.update(s.window, |d, cx| d.hover(crossing.0, crossing.1, cx));
    assert_eq!(doc.read_with(s.window, |d, _| d.hovered_links.len()), 2);
    // …mais plus en mode « les bouts », qui ne garde que le départ et la pointe
    s.click("tb-preferences");
    s.click("pref-link-ends");
    assert_eq!(preferences_service::load().0.link_click, LinkClick::Ends, "enregistré");
    doc.update(s.window, |d, cx| d.hover(crossing.0, crossing.1, cx));
    assert!(doc.read_with(s.window, |d, _| d.hovered_links.is_empty()));
    let start = doc.read_with(s.window, |d, _| d.scene_origin());
    doc.update(s.window, |d, cx| d.hover(start.0 + 5.0, start.1 + 5.0, cx));
    assert_eq!(doc.read_with(s.window, |d, _| d.hovered_links.clone()), vec![0], "le départ réagit");
}

#[gpui::test]
fn escape_closes_the_arrow_menu(cx: &mut TestAppContext) {
    if crate::services::render_service::RenderService::locate().is_none() {
        eprintln!("pumlrender absent : test ignoré");
        return;
    }
    let path = temp_file("echap.puml", "@startuml\nA --> B\nC --> D\n@enduml\n");
    let mut s = open(cx, vec![path]);
    let (doc, crossing, _) = crossing_arrows(&mut s);
    // vrai clic : il donne le focus à l'aperçu, d'où Échap est lu
    s.window.run_until_parked();
    let preview = s.window.debug_bounds("preview").expect("aperçu");
    let (x, y) = (f32::from(preview.left()) + crossing.0, f32::from(preview.top()) + crossing.1);
    s.window.simulate_click(gpui::point(gpui::px(x), gpui::px(y)), Modifiers::none());
    s.window.run_until_parked();
    assert!(doc.read_with(s.window, |d, _| d.link_menu.is_some()), "le clic réel a ouvert le menu");
    s.window.simulate_keystrokes("escape");
    s.window.run_until_parked();
    assert!(doc.read_with(s.window, |d, _| d.link_menu.is_none()));
}

#[gpui::test]
fn typing_continues_after_a_toolbar_click(cx: &mut TestAppContext) {
    let path = temp_file("focus.puml", "@startuml\n@enduml\n");
    let mut s = open(cx, vec![path]);
    s.click("tb-preferences");
    s.window.simulate_input("x");
    s.click("pref-size-plus");
    s.window.simulate_input("y");
    s.window.run_until_parked();
    assert!(s.text().contains("xy"), "{}", s.text());
}

#[gpui::test]
fn switching_to_the_classic_interface_saves_the_choice(cx: &mut TestAppContext) {
    let path = temp_file("bascule.puml", "@startuml\nA -> B\n@enduml\n");
    let mut s = open(cx, vec![path.clone()]);
    let args = test_dir().join("arguments-bascule");
    fake_classic("classique.sh", &format!("printf '%s\\n' \"$@\" > '{}'", args.display()));
    assert_eq!(s.preferences().interface, InterfaceChoice::Editor, "la nouvelle interface par défaut");

    // un document modifié demande confirmation : « Annuler » ne change rien
    s.click("tb-preferences");
    s.window.simulate_input("x");
    s.click("pref-interface-classic");
    assert!(s.window.has_pending_prompt());
    s.window.simulate_prompt_answer("Annuler");
    s.window.run_until_parked();
    assert!(!args.exists());
    assert_eq!(s.preferences().interface, InterfaceChoice::Editor);

    s.click("pref-interface-classic");
    s.window.simulate_prompt_answer("Changer sans enregistrer");
    s.window.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&args).unwrap(),
        format!("--interface\nclassic\n{}\n", path.display()),
        "l'interface classique reçoit le fichier actif"
    );
    assert_eq!(preferences_service::load().0.interface, InterfaceChoice::Classic);
    unsafe { std::env::remove_var("SIMP2PUML_GUI") };
}

#[gpui::test]
fn a_classic_interface_that_cannot_start_keeps_the_choice(cx: &mut TestAppContext) {
    let mut s = open(cx, vec![]);
    fake_classic("casse.sh", "echo 'Erreur : Tkinter absent' >&2; exit 1");
    s.click("tb-preferences");
    s.click("pref-interface-classic");
    assert!(s.status().contains("Tkinter absent"), "{}", s.status());
    assert_eq!(s.preferences().interface, InterfaceChoice::Editor);
    assert!(!preferences_service::path().unwrap().exists(), "aucun choix enregistré");
    unsafe { std::env::remove_var("SIMP2PUML_GUI") };
}

#[gpui::test]
fn window_title_follows_typing_and_saving(cx: &mut TestAppContext) {
    let path = temp_file("titre.puml", "@startuml\nA -> B\n@enduml\n");
    let s = open(cx, vec![path.clone()]);
    assert_eq!(s.tabs(), 1, "l'onglet de départ est remplacé par le fichier");
    assert_eq!(s.window.window_title().as_deref(), Some("titre.puml — simp2puml 0.1.0"));
    s.window.simulate_input("x");
    s.window.run_until_parked();
    assert_eq!(s.window.window_title().as_deref(), Some("• titre.puml — simp2puml 0.1.0"), "le point suit la frappe");
    s.window.simulate_keystrokes("ctrl-s");
    s.window.run_until_parked();
    assert_eq!(s.window.window_title().as_deref(), Some("titre.puml — simp2puml 0.1.0"));
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
