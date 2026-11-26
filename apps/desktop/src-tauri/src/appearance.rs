//! Plomberie de l'apparence : fichier JSON local et commandes Tauri.
//!
//! Les règles (préréglages, bornes, validation) sont dans
//! `couple_core::appearance` ; ici on ne fait que lire et écrire les fichiers.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use couple_core::appearance::{self, Limits, Preset, SavedTheme, LIMITS};
use couple_core::{Appearance, AppearanceSettings};
use serde::Serialize;
use tauri::State;

use super::now_ms;
use crate::store::write_atomic;

const APPEARANCE_FILE: &str = "apparence.json";
/// L'image de fond est gardée à part (data URL), pour que le fichier des
/// réglages reste petit et lisible.
const WALLPAPER_FILE: &str = "fond-ecran.txt";

pub struct AppearanceState {
    settings: Mutex<AppearanceSettings>,
    path: PathBuf,
    wallpaper_path: PathBuf,
    /// Avertissement du démarrage, montré une seule fois.
    startup_warning: Mutex<Option<String>>,
}

/// Tout ce dont l'écran de réglages a besoin, en un seul appel.
#[derive(Serialize)]
pub struct AppearanceView {
    current: Appearance,
    saved: Vec<SavedTheme>,
    presets: Vec<Preset>,
    limits: Limits,
    warning: Option<String>,
}

impl AppearanceState {
    /// Charge les réglages. Ne bloque jamais le démarrage : un fichier
    /// illisible est mis de côté (ou ignoré si même cela échoue) et l'appli
    /// repart du thème par défaut, avec un avertissement.
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(APPEARANCE_FILE);
        let (settings, startup_warning) = match fs::read(&path) {
            Ok(bytes) => match AppearanceSettings::from_json(&bytes) {
                Ok(settings) => (settings, None),
                Err(e) => {
                    let backup = path.with_extension(format!("corrompu-{}", now_ms()));
                    let message = match fs::rename(&path, &backup) {
                        Ok(()) => format!(
                            "Les réglages d'apparence étaient illisibles ({e}). Ils ont été mis \
                             de côté dans {} et le thème par défaut est rétabli.",
                            backup.display()
                        ),
                        Err(rename) => format!(
                            "Les réglages d'apparence étaient illisibles ({e}) et n'ont pas pu \
                             être mis de côté ({rename}). Le thème par défaut est utilisé."
                        ),
                    };
                    eprintln!("{message}");
                    (AppearanceSettings::default(), Some(message))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (AppearanceSettings::default(), None)
            }
            Err(e) => {
                let message = format!(
                    "Impossible de lire les réglages d'apparence ({e}). Le thème par défaut est \
                     utilisé."
                );
                eprintln!("{message}");
                (AppearanceSettings::default(), Some(message))
            }
        };
        Self {
            settings: Mutex::new(settings),
            path,
            wallpaper_path: data_dir.join(WALLPAPER_FILE),
            startup_warning: Mutex::new(startup_warning),
        }
    }

    fn lock(&self) -> MutexGuard<'_, AppearanceSettings> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Modifie une copie, l'enregistre, puis remplace celle en mémoire.
    fn change<T>(
        &self,
        action: &str,
        f: impl FnOnce(&mut AppearanceSettings) -> couple_core::Result<T>,
    ) -> Result<T, String> {
        let mut settings = self.lock();
        let mut next = settings.clone();
        let out = f(&mut next).map_err(|e| format!("Impossible {action} : {e}"))?;
        let bytes = next
            .to_json()
            .map_err(|e| format!("Impossible {action} : {e}"))?;
        write_atomic(&self.path, &bytes).map_err(|e| {
            format!(
                "Impossible {action} : l'enregistrement dans {} a échoué ({e})",
                self.path.display()
            )
        })?;
        *settings = next;
        Ok(out)
    }

    fn view(&self) -> AppearanceView {
        let settings = self.lock();
        AppearanceView {
            current: settings.current.clone(),
            saved: settings.saved.clone(),
            presets: appearance::presets(),
            limits: LIMITS,
            warning: self
                .startup_warning
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take(),
        }
    }
}

#[tauri::command]
pub fn get_appearance(state: State<'_, AppearanceState>) -> AppearanceView {
    state.view()
}

#[tauri::command]
pub fn set_appearance(
    state: State<'_, AppearanceState>,
    appearance: Appearance,
) -> Result<Appearance, String> {
    state.change("d'enregistrer l'apparence", |s| {
        Ok(s.set_current(appearance))
    })
}

#[tauri::command]
pub fn apply_preset(state: State<'_, AppearanceState>, id: String) -> Result<Appearance, String> {
    state.change("d'appliquer le préréglage", |s| s.apply_preset(&id))
}

#[tauri::command]
pub fn save_theme(
    state: State<'_, AppearanceState>,
    name: String,
) -> Result<AppearanceView, String> {
    state.change("d'enregistrer le thème", |s| s.save_current_as(&name))?;
    Ok(state.view())
}

#[tauri::command]
pub fn apply_saved_theme(
    state: State<'_, AppearanceState>,
    id: String,
) -> Result<Appearance, String> {
    state.change("d'appliquer le thème", |s| s.apply_saved(&id))
}

#[tauri::command]
pub fn delete_theme(
    state: State<'_, AppearanceState>,
    id: String,
) -> Result<AppearanceView, String> {
    state.change("de supprimer le thème", |s| s.delete_saved(&id))?;
    Ok(state.view())
}

#[tauri::command]
pub fn export_theme(state: State<'_, AppearanceState>) -> Result<String, String> {
    state
        .lock()
        .current
        .to_code()
        .map_err(|e| format!("Impossible d'exporter le thème : {e}"))
}

#[tauri::command]
pub fn import_theme(state: State<'_, AppearanceState>, code: String) -> Result<Appearance, String> {
    state.change("d'importer le thème", |s| {
        Ok(s.set_current(Appearance::from_code(&code)?))
    })
}

/// Image de fond enregistrée, s'il y en a une (illisible : ignorée).
/// Commande asynchrone : lire plusieurs Mo ne doit pas figer l'interface.
#[tauri::command]
pub async fn get_wallpaper(state: State<'_, AppearanceState>) -> Result<Option<String>, String> {
    Ok(read_wallpaper(&state.wallpaper_path))
}

fn read_wallpaper(path: &Path) -> Option<String> {
    let error = match fs::read_to_string(path) {
        Ok(data) => match appearance::validate_wallpaper(&data) {
            Ok(()) => return Some(data),
            Err(e) => e.to_string(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => e.to_string(),
    };
    // Fichier illisible : mis de côté pour ne pas le relire à chaque démarrage.
    let backup = path.with_extension(format!("corrompu-{}", now_ms()));
    match fs::rename(path, &backup) {
        Ok(()) => eprintln!(
            "Image de fond illisible ({error}), mise de côté dans {}",
            backup.display()
        ),
        Err(e) => eprintln!("Image de fond illisible ({error}), ignorée ({e})"),
    }
    None
}

/// Enregistre l'image de fond (`None` la supprime). Asynchrone, comme
/// `get_wallpaper`.
#[tauri::command]
pub async fn set_wallpaper(
    state: State<'_, AppearanceState>,
    data_url: Option<String>,
) -> Result<(), String> {
    let _guard = state.lock();
    match data_url {
        Some(data) => {
            appearance::validate_wallpaper(&data).map_err(|e| e.to_string())?;
            write_atomic(&state.wallpaper_path, data.as_bytes())
                .map_err(|e| format!("Impossible d'enregistrer l'image de fond : {e}"))
        }
        None => match fs::remove_file(&state.wallpaper_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Impossible de retirer l'image de fond : {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eve-et-lucas-apparence-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn appearance_is_saved_and_reloaded() {
        let dir = temp_dir("roundtrip");
        let state = AppearanceState::load(&dir);
        assert_eq!(state.lock().current, Appearance::default());

        state.change("", |s| s.apply_preset("neon")).unwrap();
        state.change("", |s| s.save_current_as("Le mien")).unwrap();

        let reloaded = AppearanceState::load(&dir);
        assert!(reloaded.view().warning.is_none());
        assert_eq!(reloaded.lock().current.preset, "neon");
        assert_eq!(reloaded.lock().saved.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(APPEARANCE_FILE), b"{ pas du json").unwrap();

        let state = AppearanceState::load(&dir);
        assert!(state.view().warning.is_some());
        // Montré une seule fois, pas à chaque rechargement de l'interface.
        assert!(state.view().warning.is_none());
        assert_eq!(state.lock().current, Appearance::default());
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
    fn unreadable_file_never_blocks_startup() {
        // `apparence.json` est un dossier : ni lisible, ni renommable en fichier.
        let dir = temp_dir("unreadable");
        fs::create_dir_all(dir.join(APPEARANCE_FILE)).unwrap();
        let state = AppearanceState::load(&dir);
        assert!(state.view().warning.is_some());
        assert_eq!(state.lock().current, Appearance::default());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_write_leaves_memory_unchanged() {
        let dir = temp_dir("failed-write");
        let state = AppearanceState::load(&dir);
        fs::remove_dir_all(&dir).unwrap();
        let err = state
            .change("d'appliquer le préréglage", |s| s.apply_preset("nuit"))
            .unwrap_err();
        assert!(err.starts_with("Impossible d'appliquer le préréglage"));
        assert_eq!(state.lock().current, Appearance::default());
    }
}
