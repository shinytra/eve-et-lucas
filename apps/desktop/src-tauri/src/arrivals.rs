//! Arrivée à la maison.
//!
//! - Téléphone : le plugin `tauri-plugin-geofence` (Kotlin) note chaque
//!   entrée dans la zone « maison », même appli fermée. L'appli les récupère
//!   au démarrage et à chaque évènement, et les ajoute au document partagé
//!   `arrivals` au nom de la personne choisie sur ce téléphone.
//! - Tous les appareils : une arrivée récente venue d'un autre appareil fait
//!   sonner (notification avec son), une seule fois.

use std::{path::PathBuf, sync::Arc, time::Duration};

use couple_core::Arrival;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_notification::NotificationExt;

use crate::{now_ms, store::AppState};

/// Réglages propres à cet appareil.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ArrivalSettings {
    /// Qui porte ce téléphone (« Eve » ou « Lucas »).
    #[serde(default)]
    pub who: String,
    /// Dernière arrivée déjà signalée ici (millisecondes Unix).
    #[serde(default)]
    pub seen_until: i64,
}

fn settings_path(state: &AppState) -> PathBuf {
    state.data_dir.join("arrivee.json")
}

fn load_settings(state: &AppState) -> ArrivalSettings {
    std::fs::read(settings_path(state))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_settings(state: &AppState, settings: &ArrivalSettings) -> Result<(), String> {
    let bytes = serde_json::to_vec(settings).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&settings_path(state), &bytes)
        .map_err(|e| format!("Impossible d'enregistrer les réglages d'arrivée : {e}"))
}

#[tauri::command]
pub fn list_arrivals(state: State<'_, Arc<AppState>>) -> Vec<Arrival> {
    state.arrivals.lock().list()
}

#[tauri::command]
pub fn arrival_person(state: State<'_, Arc<AppState>>) -> String {
    load_settings(&state).who
}

/// Choisit qui porte ce téléphone (nom donné aux arrivées détectées ici).
#[tauri::command]
pub fn set_arrival_person(state: State<'_, Arc<AppState>>, who: String) -> Result<(), String> {
    let who = who.trim();
    if who.is_empty() || who.chars().count() > 60 {
        return Err("Prénom invalide".to_owned());
    }
    let mut settings = load_settings(&state);
    settings.who = who.to_owned();
    save_settings(&state, &settings)
}

/// Texte de la notification pour une arrivée.
fn message(arrival: &Arrival) -> String {
    format!("{} arrive à la maison", arrival.who)
}

/// Lance la surveillance des arrivées.
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    if let Err(e) = state
        .arrivals
        .change("de ranger les arrivées", |d| d.prune(now_ms()))
    {
        eprintln!("{e}");
    }

    #[cfg(mobile)]
    phone::start(app, state.clone());

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            ring_new_arrivals(&app, &state);
            tokio::select! {
                () = state.arrivals_wanted.notified() => {}
                () = tokio::time::sleep(Duration::from_secs(60)) => {}
            }
        }
    });
}

/// Fait sonner pour les arrivées des autres pas encore signalées ici.
fn ring_new_arrivals(app: &AppHandle, state: &AppState) {
    let mut settings = load_settings(state);
    let now = now_ms();
    let to_ring =
        state
            .arrivals
            .lock()
            .to_ring(&state.self_id.to_string(), settings.seen_until, now);
    let Some(last) = to_ring.last() else {
        return;
    };
    for arrival in &to_ring {
        let result = app
            .notification()
            .builder()
            .title(message(arrival))
            .body("Détecté par son téléphone à l'approche de la maison")
            .sound("default")
            .show();
        if let Err(e) = result {
            state.notice(format!("Impossible de signaler l'arrivée : {e}"));
        }
    }
    settings.seen_until = last.at;
    if let Err(e) = save_settings(state, &settings) {
        eprintln!("{e}");
    }
}

#[cfg(mobile)]
mod phone {
    use std::sync::Arc;

    use tauri::AppHandle;
    use tauri_plugin_geofence::GeofenceExt;

    use super::load_settings;
    use crate::store::AppState;

    /// Récupère les arrivées notées par Android et les ajoute au document.
    /// Les appels au plugin bloquent : jamais sur le fil principal.
    fn ingest(app: &AppHandle, state: &AppState) {
        let arrivals = match app.geofence().take_arrivals() {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Arrivées indisponibles : {e}");
                return;
            }
        };
        if arrivals.is_empty() {
            return;
        }
        let who = match load_settings(state).who {
            who if !who.trim().is_empty() => who,
            _ => state.device_name(),
        };
        let device = state.self_id.to_string();
        for at in arrivals {
            if let Err(e) = state
                .arrivals
                .change("d'enregistrer l'arrivée", |d| d.add(&who, &device, at))
            {
                state.notice(e);
            }
        }
        // Les autres appareils l'apprennent tout de suite.
        state.sync_wanted.notify_one();
        couple_sync::Store::changed(state, &[crate::store::ARRIVALS_DOC.to_owned()]);
    }

    pub fn start(app: &AppHandle, state: Arc<AppState>) {
        // Arrivées survenues pendant que l'appli était fermée.
        {
            let (app, state) = (app.clone(), state.clone());
            tauri::async_runtime::spawn_blocking(move || ingest(&app, &state));
        }
        let handler_app = app.clone();
        let result = app.geofence().on_arrival(move |_| {
            let (app, state) = (handler_app.clone(), state.clone());
            tauri::async_runtime::spawn_blocking(move || ingest(&app, &state));
        });
        if let Err(e) = result {
            eprintln!("Écoute des arrivées impossible : {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_tolerate_missing_fields() {
        let s: ArrivalSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.who, "");
        assert_eq!(s.seen_until, 0);
        let s: ArrivalSettings = serde_json::from_str(r#"{"who":"Eve"}"#).unwrap();
        assert_eq!(s.who, "Eve");
    }

    #[test]
    fn message_names_the_person() {
        let a = Arrival {
            id: "x".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 0,
        };
        assert_eq!(message(&a), "Eve arrive à la maison");
    }
}
