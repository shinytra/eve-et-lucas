//! Comportement propre au PC : l'appli tourne en tâche de fond.
//!
//! Le PC est le pair « boîte aux lettres » toujours allumé (voir CLAUDE.md) :
//! fermer la fenêtre la cache sans arrêter l'appli, une icône dans la barre
//! des tâches permet de la rouvrir ou de la quitter, et l'appli peut se lancer
//! toute seule au démarrage de l'ordinateur (fenêtre cachée).

use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Builder, Manager, Runtime, Window, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Argument passé par le lancement automatique : démarrer sans fenêtre.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Plugins propres au PC. `single-instance` doit être le premier : un second
/// lancement (double-clic sur l'icône du bureau) réaffiche la fenêtre
/// existante au lieu d'ouvrir une deuxième copie de l'appli.
pub fn plugins<R: Runtime>(builder: Builder<R>) -> Builder<R> {
    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        // Adresse des versions donnée par `update.rs` (dépôt de `couple_core::update`).
        .plugin(tauri_plugin_updater::Builder::new().build())
}

/// Icône de la barre des tâches et fenêtre de départ.
pub fn setup<R: Runtime>(app: &mut App<R>) -> tauri::Result<()> {
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or_else(|e| {
        eprintln!("Impossible de savoir si le lancement au démarrage est actif : {e}");
        false
    });

    let open = MenuItem::with_id(app, "open", "Ouvrir Eve & Lucas", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Lancer au démarrage de l'ordinateur",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &autostart, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Eve & Lucas")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "autostart" => set_autostart(app, &autostart),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    if starts_minimized(std::env::args()) {
        if let Some(window) = app.get_webview_window("main") {
            window.hide()?;
        }
    }
    Ok(())
}

/// Fermer la fenêtre la cache seulement : l'appli continue à tourner pour
/// garder les notes à jour. On quitte vraiment par le menu de l'icône.
pub fn on_window_event<R: Runtime>(window: &Window<R>, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
            api.prevent_close();
            if let Err(e) = window.hide() {
                eprintln!("Impossible de cacher la fenêtre : {e}");
            }
        }
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("Fenêtre principale introuvable");
        return;
    };
    let result = window
        .unminimize()
        .and_then(|_| window.show())
        .and_then(|_| window.set_focus());
    if let Err(e) = result {
        eprintln!("Impossible d'afficher la fenêtre : {e}");
    }
}

/// Le menu a déjà coché ou décoché la case : on applique ce choix, et on
/// remet la case dans l'état réel si le système refuse.
fn set_autostart<R: Runtime>(app: &AppHandle<R>, item: &CheckMenuItem<R>) {
    let wanted = item.is_checked().unwrap_or(false);
    let launcher = app.autolaunch();
    let result = if wanted {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        eprintln!("Impossible de modifier le lancement au démarrage : {e}");
        let actual = launcher.is_enabled().unwrap_or(!wanted);
        let _ = item.set_checked(actual);
    }
}

fn starts_minimized(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|arg| arg == MINIMIZED_ARG)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn minimized_only_with_the_autostart_argument() {
        assert!(starts_minimized(args(&["eve-et-lucas", "--minimized"])));
        assert!(!starts_minimized(args(&["eve-et-lucas"])));
        assert!(!starts_minimized(args(&["eve-et-lucas", "--minimised"])));
    }
}
