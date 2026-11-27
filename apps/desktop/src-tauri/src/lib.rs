//! Appli Eve & Lucas : couche Tauri.
//!
//! Ce crate ne contient que de la « plomberie » : il expose les fonctions du
//! cœur (`couple-core`) à l'interface web via des commandes Tauri, enregistre
//! les documents sur le disque (`store.rs`), les synchronise avec les autres
//! appareils (`sync.rs`), affiche les rappels des tâches (`reminders.rs`)
//! et des dates importantes (`dates.rs`), garde les affiches des animés
//! (`animes.rs`) et reçoit les musiques partagées (`music.rs`).
//! Aucune règle métier ici.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use couple_core::{notify::Event, Note};
use tauri::{Manager, State};

mod animes;
mod appearance;
mod arrivals;
mod dates;
#[cfg(desktop)]
mod desktop;
mod files;
mod music;
mod notify;
mod push;
mod reminders;
mod store;
mod sync;
mod tasks;
mod widget;

use store::AppState;

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or_else(|| {
            eprintln!("Horloge système invalide : date mise à 0");
            0
        })
}

/// Nom proposé pour cet appareil au premier lancement (modifiable ensuite).
fn default_device_name() -> String {
    if cfg!(mobile) {
        "Téléphone".to_owned()
    } else {
        "PC".to_owned()
    }
}

#[tauri::command]
fn list_notes(state: State<'_, Arc<AppState>>) -> Result<Vec<Note>, String> {
    let doc = state.notes.lock();
    let unreadable = doc.unreadable_count();
    if unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les notes");
    }
    doc.list()
        .map_err(|e| format!("Impossible de lire les notes : {e}"))
}

#[tauri::command]
fn startup_warning(state: State<'_, Arc<AppState>>) -> Option<String> {
    let notices = state
        .notices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let all: Vec<String> = state
        .startup_warnings
        .iter()
        .cloned()
        .chain(notices)
        .collect();
    (!all.is_empty()).then(|| all.join("\n"))
}

#[tauri::command]
fn add_note(
    state: State<'_, Arc<AppState>>,
    author: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    let note = state.notes.change("d'ajouter la note", |doc| {
        doc.add(&author, &title, &body, now_ms())
    })?;
    state.sync_wanted.notify_one();
    notify::announce(&state, |me, at| Event::note_added(&note, me, at));
    Ok(note)
}

#[tauri::command]
fn update_note(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    let note = state.notes.change("de modifier la note", |doc| {
        doc.update(&id, &title, &body, now_ms())
    })?;
    state.sync_wanted.notify_one();
    Ok(note)
}

#[tauri::command]
fn delete_note(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .notes
        .change("de supprimer la note", |doc| doc.delete(&id))?;
    state.sync_wanted.notify_one();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = desktop::plugins(builder).on_window_event(desktop::on_window_event);
    }

    #[cfg(mobile)]
    {
        builder = builder
            .plugin(tauri_plugin_geofence::init())
            .plugin(tauri_plugin_home_widget::init())
            .plugin(tauri_plugin_share_target::init())
            // Économie de batterie : réseau coupé peu après le passage en
            // arrière-plan, rallumé au retour (voir `sync::suspended`).
            .on_window_event(|window, event| match event {
                tauri::WindowEvent::Suspended => sync::suspended(window.app_handle()),
                tauri::WindowEvent::Resumed => {
                    sync::resumed(window.app_handle());
                    if let Some(state) = window.app_handle().try_state::<Arc<AppState>>() {
                        music::resumed(&state);
                    }
                }
                _ => {}
            });
    }

    // Uniquement avec `--features mcp-bridge` (jamais dans une version
    // distribuée) : laisse Claude Code, via le serveur MCP Tauri, faire des
    // captures, cliquer et lire les logs. Ce pont n'a pas d'authentification.
    #[cfg(feature = "mcp-bridge")]
    {
        builder = builder.plugin(
            tauri_plugin_mcp_bridge::Builder::new()
                .bind_address("127.0.0.1")
                .build(),
        );
    }

    let result = builder
        .plugin(tauri_plugin_notification::init())
        // Ouvre les musiques dans YouTube Music (seulement depuis Rust : aucune
        // permission n'est donnée à l'interface).
        .plugin(tauri_plugin_opener::init())
        .manage(sync::SyncHandle::default())
        .manage(files::Uploads::default())
        .setup(|app| {
            #[cfg(feature = "mcp-bridge")]
            app.add_capability(include_str!("../capabilities-dev/mcp-bridge.json"))?;
            // Permet de lancer une deuxième copie de test avec ses propres données.
            let data_dir = match std::env::var_os("EVE_ET_LUCAS_DATA_DIR") {
                Some(dir) => dir.into(),
                None => app.path().app_data_dir()?,
            };
            let state = Arc::new(AppState::load(&data_dir, &default_device_name())?);
            app.manage(state.clone());
            app.manage(appearance::AppearanceState::load(&data_dir));
            sync::start(app.handle(), state.clone());
            reminders::start(app.handle(), state.clone());
            dates::start(app.handle(), state.clone());
            arrivals::start(app.handle(), state.clone());
            widget::start(app.handle(), state.clone());
            animes::start(app.handle(), state.clone());
            music::start(app.handle(), state.clone());
            push::start(app.handle(), state);
            #[cfg(desktop)]
            desktop::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_notes,
            startup_warning,
            add_note,
            update_note,
            delete_note,
            tasks::list_tasks,
            tasks::add_task,
            tasks::update_task,
            tasks::set_task_done,
            tasks::delete_task,
            animes::list_animes,
            animes::search_animes,
            animes::add_anime,
            animes::set_anime_episode,
            animes::set_anime_status,
            animes::refresh_anime,
            animes::delete_anime,
            animes::anime_statuses,
            animes::anime_cover,
            music::list_musics,
            music::add_music,
            music::update_music,
            music::set_music_listened,
            music::delete_music,
            music::open_music,
            music::take_music_shares,
            dates::list_dates,
            dates::add_date,
            dates::update_date,
            dates::delete_date,
            arrivals::list_arrivals,
            arrivals::arrival_person,
            arrivals::set_arrival_person,
            files::list_files,
            files::upload_start,
            files::upload_chunk,
            files::upload_cancel,
            files::upload_finish,
            files::delete_file,
            files::download_file,
            files::file_content,
            files::save_file,
            sync::list_devices,
            sync::this_device,
            sync::rename_this_device,
            sync::remove_device,
            sync::start_pairing,
            sync::cancel_pairing,
            sync::join_pairing,
            sync::sync_now,
            sync::sync_status,
            appearance::get_appearance,
            appearance::set_appearance,
            appearance::apply_preset,
            appearance::save_theme,
            appearance::apply_saved_theme,
            appearance::delete_theme,
            appearance::export_theme,
            appearance::import_theme,
            appearance::get_wallpaper,
            appearance::set_wallpaper,
            appearance::adapt_to_image,
            notify::notification_settings,
            notify::set_notification,
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("Impossible de lancer l'appli Eve & Lucas : {e}");
        std::process::exit(1);
    }
}
