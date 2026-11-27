//! Arrivées et présence dans les lieux (maison, université…).
//!
//! - Téléphone : le plugin `tauri-plugin-geofence` (Kotlin) note chaque
//!   entrée ou sortie de lieu, même appli fermée. L'appli les récupère au
//!   démarrage et à chaque évènement, et les ajoute aux documents partagés
//!   `presence` (où est chacun) et, pour une arrivée dans un lieu annoncé,
//!   `arrivals`, au nom de la personne choisie sur ce téléphone.
//! - Tous les appareils : une arrivée récente venue d'un autre appareil fait
//!   sonner (notification avec son), une seule fois.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use couple_core::{arrivals::RING_WITHIN_MS, Arrival, PresenceEvent};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_notification::NotificationExt;

use crate::{
    now_ms,
    store::{AppState, PRESENCE_DOC},
};

/// Réglages propres à cet appareil.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ArrivalSettings {
    /// Qui utilise cet appareil (« Eve » ou « Lucas »), choisi au premier
    /// lancement puis dans Paramètres (`ui/moi.js`). Sert aussi de prénom aux
    /// ajouts faits ici, aux cases cochées sur le widget et aux notifications.
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

/// Où est chacun : son dernier évènement (entrée dans un lieu, ou sortie).
#[tauri::command]
pub fn list_presence(state: State<'_, Arc<AppState>>) -> Vec<PresenceEvent> {
    state.presence.lock().current(now_ms())
}

/// Ajoute un évènement de présence (de ce téléphone, ou reçu par ntfy) ;
/// prévient la synchro et l'interface s'il est nouveau.
pub(crate) fn record_presence(state: &AppState, event: &PresenceEvent) -> Result<bool, String> {
    let added = state
        .presence
        .change("d'enregistrer où tu es", |d| d.record(event))?;
    if added {
        state.sync_wanted.notify_one();
        couple_sync::Store::changed(state, &[PRESENCE_DOC.to_owned()]);
    }
    Ok(added)
}

#[tauri::command]
pub fn arrival_person(state: State<'_, Arc<AppState>>) -> String {
    load_settings(&state).who
}

/// Choisit qui utilise cet appareil (nom donné aux arrivées détectées ici).
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
    // Musiques partagées avant le choix du prénom : elles peuvent être ajoutées.
    state.music_wanted.notify_one();
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
    // Même déjà connue (arrivée d'abord par la synchro), une arrivée qui a
    // sonné sur Android ne doit plus sonner ici.
    let present = added.is_some()
        || state
            .arrivals
            .lock()
            .list()
            .iter()
            .any(|a| a.id == arrival.id);
    if present && already_rung {
        let mut settings = load_settings(state);
        if !settings.rung.contains(&arrival.id) {
            settings.rung.push(arrival.id.clone());
            save_settings(state, &settings)?;
        }
    }
    Ok(added.is_some())
}

/// Texte de la notification pour une arrivée, avec le lieu s'il est connu
/// (même identifiant dans le document des présences).
fn message(arrival: &Arrival, place: Option<&PresenceEvent>) -> String {
    match place.filter(|p| !p.place.is_empty()) {
        Some(p) => format!("{} arrive · {} {}", arrival.who, p.emoji(), p.place),
        None => format!("{} arrive à la maison", arrival.who),
    }
}

/// Lance la surveillance des arrivées.
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    if let Err(e) = state
        .arrivals
        .change("de ranger les arrivées", |d| d.prune(now_ms()))
    {
        eprintln!("{e}");
    }
    if let Err(e) = state
        .presence
        .change("de ranger les présences", |d| d.prune(now_ms()))
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
    // D'abord ce qu'Android a déjà fait sonner (réveil ntfy), pour ne pas
    // sonner une deuxième fois.
    #[cfg(mobile)]
    crate::push::phone::ingest_received(app, state);
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
    // Avant de sonner : Android ne sonnera pas si le réveil ntfy de la même
    // arrivée arrive ensuite.
    #[cfg(mobile)]
    {
        use tauri_plugin_geofence::GeofenceExt;
        let ids: Vec<String> = to_ring.iter().map(|a| a.id.clone()).collect();
        if let Err(e) = app.geofence().mark_rung(ids) {
            eprintln!("Impossible de noter les arrivées signalées : {e}");
        }
    }
    let places = state.presence.lock().list();
    for arrival in &to_ring {
        let place = places.iter().find(|p| p.id == arrival.id);
        let result = app
            .notification()
            .builder()
            .title(message(arrival, place))
            .body("Détecté par son téléphone en approchant")
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

    use couple_core::PresenceEvent;

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
            let arrival = if pending.kind == "arrival" {
                state
                    .arrivals
                    .change("d'enregistrer l'arrivée", |d| {
                        d.add_with_id(&pending.id, &who, &device, pending.at)
                    })
                    .map(|_| ())
            } else {
                Ok(())
            };
            let presence = PresenceEvent {
                id: pending.id.clone(),
                who: who.clone(),
                device: device.clone(),
                at: pending.at,
                inside: pending.inside,
                place: pending.place.clone(),
                place_kind: pending.place_kind.clone(),
            };
            // Arrivée notée avant la mise à jour (sans lieu) : pas de présence,
            // aucune sortie ne viendrait la corriger.
            let legacy = pending.kind == "arrival" && pending.place.is_empty();
            match arrival.and_then(|()| {
                if legacy {
                    Ok(false)
                } else {
                    super::record_presence(state, &presence)
                }
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
    fn arrival_rung_by_android_never_rings_again() {
        let dir = crate::store::tests::temp_dir("arrivee");
        let state = AppState::load(&dir, "Téléphone").unwrap();
        let now = now_ms();
        let x = Arrival {
            id: "x".into(),
            who: "Lucas".into(),
            device: "tel-lucas".into(),
            at: now,
        };
        // L'arrivée est d'abord reçue par la synchro…
        state
            .arrivals
            .change("", |d| d.add_with_id(&x.id, &x.who, &x.device, x.at))
            .unwrap();
        // … puis Android signale qu'il a déjà sonné pour elle (réveil ntfy).
        assert!(!add_received(&state, &x, true).unwrap());
        let rung = load_settings(&state).rung;
        assert_eq!(rung, vec!["x".to_owned()]);
        assert!(state
            .arrivals
            .lock()
            .to_ring(&state.self_id.to_string(), &rung, now)
            .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn message_names_the_person_and_the_place() {
        let a = Arrival {
            id: "x".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 0,
        };
        assert_eq!(message(&a, None), "Eve arrive à la maison");
        let p = PresenceEvent {
            id: "x".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 0,
            inside: true,
            place: "Maison 2".into(),
            place_kind: "maison".into(),
        };
        assert_eq!(message(&a, Some(&p)), "Eve arrive · 🏠 Maison 2");
    }

    #[test]
    fn presence_is_recorded_once() {
        let dir = crate::store::tests::temp_dir("presence");
        let state = AppState::load(&dir, "Téléphone").unwrap();
        let p = PresenceEvent {
            id: "p1".into(),
            who: "Eve".into(),
            device: "tel-eve".into(),
            at: now_ms(),
            inside: true,
            place: "Université".into(),
            place_kind: "etudes".into(),
        };
        assert!(record_presence(&state, &p).unwrap());
        assert!(!record_presence(&state, &p).unwrap());
        assert_eq!(state.presence.lock().current(now_ms()), vec![p]);
        // Gardé sur disque.
        drop(state);
        let state = AppState::load(&dir, "Téléphone").unwrap();
        assert_eq!(state.presence.lock().list().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
