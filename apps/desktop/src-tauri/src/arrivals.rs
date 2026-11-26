//! Arrivée à la maison.
//!
//! - Téléphone : le plugin `tauri-plugin-geofence` (Kotlin) note chaque
//!   entrée dans la zone « maison », même appli fermée. L'appli les récupère
//!   au démarrage et à chaque évènement, et les ajoute au document partagé
//!   `arrivals` au nom de la personne choisie sur ce téléphone.
//! - Tous les appareils : une arrivée récente venue d'un autre appareil fait
//!   sonner (notification avec son), une seule fois.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use couple_core::{arrivals::RING_WITHIN_MS, Arrival};
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
    /// Identifiants des arrivées déjà signalées ici (seulement les récentes).
    #[serde(default)]
    pub rung: Vec<String>,
}

/// Une seule lecture-modification-écriture du fichier de réglages à la fois
/// (la boucle de sonnerie et la commande de l'interface tournent en parallèle).
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

fn settings_path(state: &AppState) -> PathBuf {
    state.data_dir.join("arrivee.json")
}

/// Lit les réglages. Un fichier illisible est mis de côté avec un
/// avertissement, et l'appli repart de réglages vides.
pub(crate) fn load_settings(state: &AppState) -> ArrivalSettings {
    let path = settings_path(state);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ArrivalSettings::default(),
        Err(e) => {
            state.notice(format!(
                "Réglages d'arrivée illisibles ({}) : {e}",
                path.display()
            ));
            return ArrivalSettings::default();
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(e) => {
            let backup = path.with_extension(format!("corrompu-{}", now_ms()));
            let moved = std::fs::rename(&path, &backup).is_ok();
            state.notice(format!(
                "Réglages d'arrivée illisibles ({e}) : {} Choisissez à nouveau qui porte ce téléphone.",
                if moved {
                    format!("mis de côté dans {}.", backup.display())
                } else {
                    "ignorés.".to_owned()
                }
            ));
            ArrivalSettings::default()
        }
    }
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
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut settings = load_settings(&state);
    settings.who = who.to_owned();
    save_settings(&state, &settings)?;
    // Le plugin doit envoyer le bon prénom dans les réveils.
    state.push_wanted.notify_one();
    Ok(())
}

/// Ajoute une arrivée reçue par ntfy ; `already_rung` la marque comme déjà
/// signalée (le téléphone a sonné tout seul en la recevant). Le verrou des
/// réglages est tenu tout du long : la boucle de sonnerie ne peut pas la voir
/// entre les deux. Renvoie `true` si elle était nouvelle.
pub(crate) fn add_received(
    state: &AppState,
    arrival: &Arrival,
    already_rung: bool,
) -> Result<bool, String> {
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let added = state
        .arrivals
        .change("d'enregistrer l'arrivée reçue", |d| {
            d.add_with_id(&arrival.id, &arrival.who, &arrival.device, arrival.at)
        })?;
    if added.is_some() && already_rung {
        let mut settings = load_settings(state);
        if !settings.rung.contains(&arrival.id) {
            settings.rung.push(arrival.id.clone());
            save_settings(state, &settings)?;
        }
    }
    Ok(added.is_some())
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
    let _guard = SETTINGS_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut settings = load_settings(state);
    let now = now_ms();
    let (to_ring, recent) = {
        let doc = state.arrivals.lock();
        let recent: Vec<String> = doc
            .list()
            .into_iter()
            .filter(|a| now - a.at <= RING_WITHIN_MS)
            .map(|a| a.id)
            .collect();
        (
            doc.to_ring(&state.self_id.to_string(), &settings.rung, now),
            recent,
        )
    };
    if to_ring.is_empty() {
        return;
    }
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
    settings.rung = remember(&settings.rung, &to_ring, &recent);
    if let Err(e) = save_settings(state, &settings) {
        eprintln!("{e}");
    }
    // Pour que le téléphone ne sonne pas une deuxième fois si le réveil
    // ntfy de la même arrivée arrive ensuite.
    #[cfg(mobile)]
    {
        use tauri_plugin_geofence::GeofenceExt;
        let ids: Vec<String> = to_ring.into_iter().map(|a| a.id).collect();
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(e) = app.geofence().mark_rung(ids) {
                eprintln!("Impossible de noter les arrivées signalées : {e}");
            }
        });
    }
}

/// Identifiants à retenir : ceux déjà signalés et ceux qu'on vient de
/// signaler, limités aux arrivées encore récentes (au-delà, elles ne sonnent
/// plus de toute façon).
fn remember(rung: &[String], new: &[Arrival], recent: &[String]) -> Vec<String> {
    rung.iter()
        .cloned()
        .chain(new.iter().map(|a| a.id.clone()))
        .filter(|id| recent.contains(id))
        .collect()
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
        let mut arrivals = match app.geofence().peek_arrivals() {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Arrivées indisponibles : {e}");
                return;
            }
        };
        if arrivals.is_empty() {
            return;
        }
        arrivals.sort_by_key(|a| a.at);
        let who = match load_settings(state).who {
            who if !who.trim().is_empty() => who,
            _ => state.device_name(),
        };
        let device = state.self_id.to_string();
        let mut saved = Vec::new();
        for pending in arrivals {
            // Même identifiant que dans le réveil envoyé par Android.
            match state.arrivals.change("d'enregistrer l'arrivée", |d| {
                d.add_with_id(&pending.id, &who, &device, pending.at)
            }) {
                Ok(_) => saved.push(pending.id),
                Err(e) => {
                    // Android les garde : nouvel essai au prochain évènement
                    // ou au prochain démarrage.
                    state.notice(e);
                    break;
                }
            }
        }
        // Android oublie seulement ce qui est bien enregistré.
        if !saved.is_empty() {
            if let Err(e) = app.geofence().ack_arrivals(saved) {
                eprintln!("Impossible de confirmer les arrivées : {e}");
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
        assert!(s.rung.is_empty());
        let s: ArrivalSettings = serde_json::from_str(r#"{"who":"Eve"}"#).unwrap();
        assert_eq!(s.who, "Eve");
    }

    #[test]
    fn remember_keeps_only_recent_ids() {
        let a = |id: &str| Arrival {
            id: id.into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 0,
        };
        let kept = remember(
            &["vieux".into(), "b".into()],
            &[a("c")],
            &["b".into(), "c".into()],
        );
        assert_eq!(kept, vec!["b".to_owned(), "c".to_owned()]);
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
