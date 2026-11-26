//! Appli Eve & Lucas : couche Tauri.
//!
//! Ce fichier ne contient que de la « plomberie » : il expose les fonctions du
//! cœur (`couple-core`) à l'interface web via des commandes Tauri et enregistre
//! le document sur le disque. Aucune règle métier ici.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use couple_core::{Note, NotesDoc};
use tauri::{Manager, State};

const NOTES_FILE: &str = "notes.automerge";

struct AppState {
    notes: Mutex<NotesDoc>,
    notes_path: PathBuf,
}

impl AppState {
    fn load(data_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        fs::create_dir_all(data_dir)?;
        let notes_path = data_dir.join(NOTES_FILE);
        let notes = match fs::read(&notes_path) {
            Ok(bytes) => NotesDoc::load(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => NotesDoc::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            notes: Mutex::new(notes),
            notes_path,
        })
    }

    /// Applique une modification au document puis l'enregistre.
    fn change<T>(
        &self,
        f: impl FnOnce(&mut NotesDoc) -> couple_core::Result<T>,
    ) -> Result<T, String> {
        let mut doc = self.notes.lock().map_err(|e| e.to_string())?;
        let out = f(&mut doc).map_err(|e| e.to_string())?;
        write_atomic(&self.notes_path, &doc.save()).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

/// Écrit dans un fichier temporaire puis renomme, pour ne jamais laisser un
/// document à moitié écrit si l'appli est coupée.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[tauri::command]
fn list_notes(state: State<'_, AppState>) -> Result<Vec<Note>, String> {
    let doc = state.notes.lock().map_err(|e| e.to_string())?;
    doc.list().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_note(
    state: State<'_, AppState>,
    author: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    state.change(|doc| doc.add(&author, &title, &body, now_ms()))
}

#[tauri::command]
fn update_note(
    state: State<'_, AppState>,
    id: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    state.change(|doc| doc.update(&id, &title, &body, now_ms()))
}

#[tauri::command]
fn delete_note(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.change(|doc| doc.delete(&id))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    // En debug uniquement : laisse Claude Code (serveur MCP Tauri) faire des
    // captures, cliquer et lire les logs de l'appli. Écoute en local seulement.
    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(
            tauri_plugin_mcp_bridge::Builder::new()
                .bind_address("127.0.0.1")
                .build(),
        );
    }

    builder
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState::load(&data_dir)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_notes,
            add_note,
            update_note,
            delete_note
        ])
        .run(tauri::generate_context!())
        .expect("impossible de lancer l'appli Eve & Lucas");
}
