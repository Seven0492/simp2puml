//! simp2puml (binaire `puml-editor`) : éditeur PlantUML GPUI avec aperçu vectoriel en direct.
//!
//! Architecture MVVM :
//! - `model`     : données et règles (tampon, coloration, scène SVG…), sans GPUI ;
//! - `services`  : processus et fichiers (moteur `pumlrender`, `simp2puml`) ;
//! - `viewmodel` : état présenté et commandes, observables par les vues ;
//! - `view`      : dessin GPUI et traduction des entrées en commandes.
//!
//! Usage : puml-editor [fichier.puml|fichier.simp]...

mod keymap;
mod model;
mod services;
mod view;
mod viewmodel;

use std::path::PathBuf;

use gpui::{App, AppContext, Application, Bounds, WindowBounds, WindowOptions, px, size};

use view::workspace_view::WorkspaceView;
use viewmodel::workspace_vm::WorkspaceViewModel;

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "simp2puml — éditeur PlantUML avec aperçu natif

Usage : puml-editor [FICHIER]...

Ouvre les fichiers .puml/.plantuml/.txt indiqués ; un fichier .simp est
importé via le convertisseur simp2puml dans un nouvel onglet non enregistré.

Variables d'environnement :
  PUMLRENDER   chemin du moteur de rendu natif (pumlrender)
  SIMP2PUML    chemin du convertisseur simp2puml";

fn main() {
    let mut paths: Vec<PathBuf> = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return;
            }
            "-V" | "--version" => {
                println!("simp2puml {VERSION}");
                return;
            }
            a if a.starts_with('-') => {
                eprintln!("Erreur : option inconnue {a}");
                std::process::exit(2);
            }
            a => paths.push(PathBuf::from(a)),
        }
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(keymap::bindings());
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    // associe la fenêtre à l'entrée de menu simp2puml.desktop
                    app_id: Some("simp2puml".into()),
                    ..Default::default()
                },
                |window, cx| {
                    let vm = cx.new(|cx| WorkspaceViewModel::new(paths.clone(), cx));
                    cx.new(|cx| WorkspaceView::new(vm, window, cx))
                },
            )
            .expect("ouverture de la fenêtre impossible");
        window
            .update(cx, |_, _, cx| cx.activate(true))
            .ok();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
