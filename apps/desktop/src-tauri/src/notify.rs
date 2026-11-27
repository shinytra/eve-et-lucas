//! Notifications des nouveautés (voir `couple_core::notify`).
//!
//! - Envoi : après un ajout fait ici (note, tâche…), l'annonce chiffrée part
//!   vers l'adresse ntfy de chaque autre appareil appairé, en tâche de fond.
//!   Sans réseau, rien n'est perdu : la synchro apportera le changement, sans
//!   notification.
//! - Réception : sur PC, par l'écoute ntfy de `push.rs` ; sur téléphone, par
//!   le code Kotlin du plugin geofence, même appli fermée (il reçoit la liste
//!   des types activés avec la configuration du réveil).
//! - Réglages : `notifications.json`, propre à cet appareil.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

use couple_core::{
    notify::{Event, NotifyChoice, NotifySettings},
    push::seal,
    PushMessage,
};
use tauri::State;

use crate::{now_ms, store::AppState};

/// Une seule lecture-modification-écriture du fichier à la fois.
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

fn settings_path(state: &AppState) -> PathBuf {
    state.data_dir.join("notifications.json")
}

/// Lit les réglages. Un fichier illisible est mis de côté avec un
/// avertissement, et tout est activé.
pub(crate) fn load_settings(state: &AppState) -> NotifySettings {
    let path = settings_path(state);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return NotifySettings::default(),
        Err(e) => {
            state.notice(format!(
                "Réglages des notifications illisibles ({}) : {e}",
                path.display()
            ));
            return NotifySettings::default();
        }
    };
    match serde_json::from_slice::<NotifySettings>(&bytes) {
        Ok(mut settings) => {
            settings.clean();
            settings
        }
        Err(e) => {
            let backup = path.with_extension(format!("corrompu-{}", now_ms()));
            let moved = std::fs::rename(&path, &backup).is_ok();
            state.notice(format!(
                "Réglages des notifications illisibles ({e}) : {} Toutes les notifications sont réactivées.",
                if moved {
                    format!("mis de côté dans {}.", backup.display())
                } else {
                    "ignorés.".to_owned()
                }
            ));
            NotifySettings::default()
        }
    }
}

fn save_settings(state: &AppState, settings: &NotifySettings) -> Result<(), String> {
    let bytes = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&settings_path(state), &bytes)
        .map_err(|e| format!("Impossible d'enregistrer les réglages des notifications : {e}"))
}

#[tauri::command]
pub fn notification_settings(state: State<'_, Arc<AppState>>) -> Vec<NotifyChoice> {
    load_settings(&state).choices()
}

/// Active ou coupe un type de notification sur cet appareil.
#[tauri::command]
pub fn set_notification(
    state: State<'_, Arc<AppState>>,
    kind: String,
    enabled: bool,
) -> Result<Vec<NotifyChoice>, String> {
    let settings = {
        let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let mut settings = load_settings(&state);
        settings.set(&kind, enabled).map_err(|e| e.to_string())?;
        save_settings(&state, &settings)?;
        settings
    };
    // Le téléphone filtre lui-même, appli fermée : il doit connaître le choix.
    state.push_wanted.notify_one();
    Ok(settings.choices())
}

/// Annonce une nouveauté faite ici aux autres appareils (en tâche de fond,
/// sans jamais bloquer ni faire échouer la commande).
pub(crate) fn announce(state: &AppState, make: impl FnOnce(&str, i64) -> Event) {
    let me = state.self_id.to_string();
    let event = make(&me, now_ms());
    let allowed: Vec<String> = state
        .devices
        .lock()
        .list()
        .into_iter()
        .map(|d| d.id)
        .collect();
    let (key, targets) = {
        let push = state.push.lock();
        (push.key(), push.targets_except(&me, &allowed))
    };
    let Some(key) = key else {
        return;
    };
    if targets.is_empty() {
        return;
    }
    let body = match seal(&key, &PushMessage::event(&event)) {
        Ok(body) => body,
        Err(e) => {
            eprintln!("Annonce impossible à chiffrer : {e}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        let client = match crate::animes::client() {
            Ok(client) => client,
            Err(e) => {
                eprintln!("Annonce impossible : {e}");
                return;
            }
        };
        for target in targets {
            // Corps = message chiffré tel quel, aucun en-tête ntfy : le
            // serveur n'apprend rien. Pas l'adresse dans le journal (le sujet
            // sert de secret).
            let sent = client
                .post(&target)
                .header("Content-Type", "text/plain; charset=utf-8")
                .body(body.clone())
                .send()
                .await
                .and_then(reqwest::Response::error_for_status);
            if let Err(e) = sent {
                eprintln!(
                    "Annonce par ntfy impossible : {}",
                    e.status()
                        .map_or_else(|| "réseau".to_owned(), |s| s.to_string())
                );
            }
        }
    });
}

#[cfg(desktop)]
pub(crate) use desktop::show_received;

#[cfg(desktop)]
mod desktop {
    use std::path::PathBuf;

    use couple_core::notify::{Event, SHOW_WITHIN_MS};
    use serde::{Deserialize, Serialize};
    use tauri::AppHandle;
    use tauri_plugin_notification::NotificationExt;

    use crate::{now_ms, store::AppState};

    /// Annonces déjà affichées : le PC rattrape les 15 dernières minutes à
    /// chaque démarrage, il ne faut pas les montrer deux fois.
    #[derive(Debug, Default, Serialize, Deserialize)]
    struct Seen {
        #[serde(default)]
        ids: Vec<(String, i64)>,
    }

    /// Au-delà, les plus anciennes sont oubliées.
    const MAX_SEEN: usize = 200;

    fn seen_path(state: &AppState) -> PathBuf {
        state.data_dir.join("notifications-vues.json")
    }

    /// Lit les annonces déjà affichées. Un fichier illisible est mis de côté
    /// avec un avertissement (au pire, une annonce affichée deux fois).
    fn load_seen(state: &AppState, path: &std::path::Path) -> Seen {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Seen::default(),
            Err(e) => {
                eprintln!("Notifications déjà affichées illisibles : {e}");
                return Seen::default();
            }
        };
        serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            let backup = path.with_extension(format!("corrompu-{}", now_ms()));
            let moved = std::fs::rename(path, &backup).is_ok();
            state.notice(format!(
                "Liste des notifications déjà affichées illisible ({e}) : {}",
                if moved {
                    format!("mise de côté dans {}.", backup.display())
                } else {
                    "ignorée.".to_owned()
                }
            ));
            Seen::default()
        })
    }

    /// Note l'annonce comme affichée ; `false` si elle l'était déjà.
    fn claim(state: &AppState, id: &str, now: i64) -> bool {
        let path = seen_path(state);
        let mut seen = load_seen(state, &path);
        if seen.ids.iter().any(|(i, _)| i == id) {
            return false;
        }
        seen.ids.retain(|(_, at)| now - at <= SHOW_WITHIN_MS);
        seen.ids.push((id.to_owned(), now));
        let extra = seen.ids.len().saturating_sub(MAX_SEEN);
        seen.ids.drain(..extra);
        match serde_json::to_vec(&seen) {
            Ok(bytes) => {
                if let Err(e) = crate::store::write_atomic(&path, &bytes) {
                    eprintln!("Impossible de noter la notification affichée : {e}");
                }
            }
            Err(e) => eprintln!("{e}"),
        }
        true
    }

    /// Faut-il afficher cette annonce reçue ? (réglages, appareil, âge, déjà vue)
    pub(super) fn wanted(state: &AppState, event: &Event, now: i64) -> bool {
        let settings = super::load_settings(state);
        // Le PC est commun : pas de « propriétaire » à exclure.
        event.should_show(&settings, &state.self_id.to_string(), None, now)
            && claim(state, &event.id, now)
    }

    pub(crate) fn show_received(app: &AppHandle, state: &AppState, event: &Event) {
        if !wanted(state, event, now_ms()) {
            return;
        }
        if let Err(e) = app
            .notification()
            .builder()
            .title(&event.title)
            .body(&event.body)
            .show()
        {
            eprintln!("Notification impossible : {e}");
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use couple_core::{notify::NOTE, NotesDoc};

        #[test]
        fn shown_once_and_filtered() {
            let dir = crate::store::tests::temp_dir("notify");
            let state = AppState::load(&dir, "PC").unwrap();
            let now = now_ms();
            let note = NotesDoc::new().add("Eve", "Courses", "", 1).unwrap();
            let event = Event::note_added(&note, "tel-eve", now);
            assert!(wanted(&state, &event, now));
            // Rattrapée après un redémarrage : pas une deuxième fois.
            assert!(!wanted(&state, &event, now + 1000));
            // Envoyée par ce PC lui-même.
            let own = Event::note_added(&note, &state.self_id.to_string(), now);
            assert!(!wanted(&state, &own, now));
            // Type coupé sur ce PC.
            let mut settings = super::super::load_settings(&state);
            settings.set(NOTE, false).unwrap();
            super::super::save_settings(&state, &settings).unwrap();
            let other = Event::note_added(&note, "tel-eve", now);
            assert!(!wanted(&state, &other, now));
            // Liste des annonces vues abîmée : mise de côté, sans bloquer.
            std::fs::write(seen_path(&state), b"[[").unwrap();
            let fresh = Event::note_added(&note, "tel-eve", now);
            assert!(claim(&state, &fresh.id, now));
            assert!(!claim(&state, &fresh.id, now));
            // Réglages abîmés : mis de côté, tout est réactivé.
            std::fs::write(super::super::settings_path(&state), b"{pas du json").unwrap();
            assert!(super::super::load_settings(&state).enabled(NOTE));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
