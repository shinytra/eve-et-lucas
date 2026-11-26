//! Appli Eve & Lucas : couche Tauri.
//!
//! Ce fichier ne contient que de la « plomberie » : il expose les fonctions du
//! cœur (`couple-core`) à l'interface web via des commandes Tauri et enregistre
//! le document sur le disque. Aucune règle métier ici.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

use couple_core::{Note, NotesDoc};
use tauri::{Manager, State};

#[cfg(desktop)]
mod desktop;

const NOTES_FILE: &str = "notes.automerge";

struct AppState {
    notes: Mutex<NotesDoc>,
    notes_path: PathBuf,
    /// Message à montrer au démarrage (fichier de notes illisible, etc.).
    startup_warning: Option<String>,
}

impl AppState {
    /// Charge les notes. Un fichier illisible n'empêche pas l'appli de
    /// démarrer : il est mis de côté (renommé) et un avertissement est affiché.
    fn load(data_dir: &Path) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;
        let notes_path = data_dir.join(NOTES_FILE);
        let mut startup_warning = None;
        let notes = match fs::read(&notes_path) {
            Ok(bytes) => match NotesDoc::load(&bytes) {
                Ok(doc) => doc,
                Err(e) => {
                    let backup = notes_path.with_extension(format!("corrompu-{}", now_ms()));
                    fs::rename(&notes_path, &backup)?;
                    let message = format!(
                        "Le fichier des notes était illisible ({e}). Il a été mis de côté dans {} \
                         et l'appli repart d'une liste vide.",
                        backup.display()
                    );
                    eprintln!("{message}");
                    startup_warning = Some(message);
                    NotesDoc::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => NotesDoc::new(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            notes: Mutex::new(notes),
            notes_path,
            startup_warning,
        })
    }

    fn lock(&self) -> MutexGuard<'_, NotesDoc> {
        // Un panic pendant une commande ne doit pas bloquer toutes les suivantes :
        // le document reste cohérent car il n'est remplacé qu'après une écriture réussie.
        self.notes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Applique une modification sur une copie, l'enregistre, puis seulement
    /// remplace le document en mémoire. Si l'écriture échoue, rien ne change.
    fn change<T>(
        &self,
        action: &str,
        f: impl FnOnce(&mut NotesDoc) -> couple_core::Result<T>,
    ) -> Result<T, String> {
        let mut doc = self.lock();
        let mut next = doc.clone();
        let out = f(&mut next).map_err(|e| format!("Impossible {action} : {e}"))?;
        write_atomic(&self.notes_path, &next.save()).map_err(|e| {
            format!(
                "Impossible {action} : l'enregistrement dans {} a échoué ({e})",
                self.notes_path.display()
            )
        })?;
        *doc = next;
        Ok(out)
    }
}

/// Écrit dans un fichier temporaire, force l'écriture sur le disque, puis
/// renomme. Même après une coupure de courant, le fichier contient soit
/// l'ancienne, soit la nouvelle version complète.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    // Sous Unix, le renommage n'est durable qu'une fois le dossier synchronisé.
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or_else(|| {
            eprintln!("Horloge système invalide : date des notes mise à 0");
            0
        })
}

#[tauri::command]
fn list_notes(state: State<'_, AppState>) -> Result<Vec<Note>, String> {
    let doc = state.lock();
    let unreadable = doc.unreadable_count();
    if unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les notes");
    }
    doc.list()
        .map_err(|e| format!("Impossible de lire les notes : {e}"))
}

#[tauri::command]
fn startup_warning(state: State<'_, AppState>) -> Option<String> {
    state.startup_warning.clone()
}

#[tauri::command]
fn add_note(
    state: State<'_, AppState>,
    author: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    state.change("d'ajouter la note", |doc| {
        doc.add(&author, &title, &body, now_ms())
    })
}

#[tauri::command]
fn update_note(
    state: State<'_, AppState>,
    id: String,
    title: String,
    body: String,
) -> Result<Note, String> {
    state.change("de modifier la note", |doc| {
        doc.update(&id, &title, &body, now_ms())
    })
}

#[tauri::command]
fn delete_note(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.change("de supprimer la note", |doc| doc.delete(&id))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();

    #[cfg(desktop)]
    {
        builder = desktop::plugins(builder).on_window_event(desktop::on_window_event);
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
        .setup(|app| {
            #[cfg(feature = "mcp-bridge")]
            app.add_capability(include_str!("../capabilities-dev/mcp-bridge.json"))?;
            let data_dir = app.path().app_data_dir()?;
            app.manage(AppState::load(&data_dir)?);
            #[cfg(desktop)]
            desktop::setup(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_notes,
            startup_warning,
            add_note,
            update_note,
            delete_note
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("Impossible de lancer l'appli Eve & Lucas : {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire propre à chaque test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eve-et-lucas-test-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_starts_empty_and_changes_are_saved() {
        let dir = temp_dir("roundtrip");
        let state = AppState::load(&dir).unwrap();
        assert!(state.startup_warning.is_none());
        assert!(state.lock().list().unwrap().is_empty());

        let note = state
            .change("d'ajouter", |doc| doc.add("Lucas", "Titre", "", 1))
            .unwrap();

        let reloaded = AppState::load(&dir).unwrap();
        assert_eq!(reloaded.lock().list().unwrap(), vec![note]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside_with_a_warning() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(NOTES_FILE), b"pas un document").unwrap();

        let state = AppState::load(&dir).unwrap();
        assert!(state.startup_warning.is_some());
        assert!(state.lock().list().unwrap().is_empty());
        let backups = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("corrompu")
            })
            .count();
        assert_eq!(backups, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_write_leaves_memory_unchanged() {
        let dir = temp_dir("failed-write");
        let state = AppState::load(&dir).unwrap();
        // Rend l'écriture impossible : le dossier de données disparaît.
        fs::remove_dir_all(&dir).unwrap();

        let err = state
            .change("d'ajouter la note", |doc| doc.add("Eve", "Titre", "", 1))
            .unwrap_err();
        assert!(err.starts_with("Impossible d'ajouter la note"));
        assert!(state.lock().list().unwrap().is_empty());
    }

    #[test]
    fn core_errors_are_explained_in_french() {
        let dir = temp_dir("errors");
        let state = AppState::load(&dir).unwrap();
        let err = state
            .change("d'ajouter la note", |doc| doc.add("Eve", " ", " ", 1))
            .unwrap_err();
        assert_eq!(
            err,
            "Impossible d'ajouter la note : une note doit avoir un titre ou un contenu"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
