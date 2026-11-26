//! Réveil des appareils par ntfy.sh (messages chiffrés, voir
//! `couple_core::push`).
//!
//! - PC : un sujet ntfy aléatoire, propre à ce PC, publié dans le document
//!   `push`. L'appli l'écoute en continu ; une arrivée reçue est ajoutée au
//!   document des arrivées, et la boucle habituelle fait sonner.
//! - Téléphone : l'appli ntfy (distributeur UnifiedPush) donne une adresse,
//!   publiée de la même façon. Le code Kotlin du plugin geofence envoie lui-même
//!   le message en arrivant (même appli fermée) et sonne à la réception ; l'appli
//!   lui donne la clé, le prénom et les adresses des autres, puis récupère les
//!   arrivées reçues.

use std::sync::Arc;

use couple_core::Arrival;
use couple_sync::Store;
use tauri::AppHandle;

use crate::store::{AppState, ARRIVALS_DOC, PUSH_DOC};

/// Prépare le réveil : crée la clé commune si besoin, puis lance l'écoute
/// (PC) ou la configuration du plugin (téléphone).
#[cfg_attr(desktop, allow(unused_variables))]
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let missing = state.push.lock().key().is_none();
    if missing {
        match state
            .push
            .change("de créer la clé de réveil", |d| d.ensure_key())
        {
            Ok(_) => {
                state.sync_wanted.notify_one();
                Store::changed(&*state, &[PUSH_DOC.to_owned()]);
            }
            Err(e) => state.notice(e),
        }
    }
    #[cfg(desktop)]
    desktop::start(state);
    #[cfg(mobile)]
    phone::start(app, state);
}

/// Ajoute une arrivée reçue par ntfy. `already_rung` : le téléphone a déjà
/// sonné de lui-même (code Kotlin), il ne faut pas sonner une deuxième fois.
pub(crate) fn accept(state: &AppState, arrival: Arrival, already_rung: bool) {
    if arrival.device == state.self_id.to_string() {
        return;
    }
    match crate::arrivals::add_received(state, &arrival, already_rung) {
        Ok(true) => {
            state.sync_wanted.notify_one();
            Store::changed(state, &[ARRIVALS_DOC.to_owned()]);
        }
        Ok(false) => {}
        Err(e) => state.notice(e),
    }
}

#[cfg(desktop)]
mod desktop {
    use std::{sync::Arc, time::Duration};

    use couple_core::push::open;
    use couple_sync::Store;
    use serde::Deserialize;

    use crate::{
        now_ms,
        store::{AppState, PUSH_DOC},
    };

    const SERVER: &str = "https://ntfy.sh";
    /// Au démarrage, on rattrape ce qui a été envoyé pendant que le PC était
    /// éteint, dans la limite où une arrivée fait encore sonner.
    const CATCH_UP: &str = "15m";

    /// Un sujet tel qu'on les crée : `el-` puis 32 chiffres hexadécimaux.
    fn valid_topic(topic: &str) -> bool {
        topic.len() == 35
            && topic.starts_with("el-")
            && topic[3..].chars().all(|c| c.is_ascii_hexdigit())
    }

    pub fn start(state: Arc<AppState>) {
        // reqwest est compilé sans fournisseur de chiffrement : on prend celui
        // d'iroh (ring). Déjà installé : rien à faire.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let Some(topic) = own_topic(&state) else {
            return;
        };
        tauri::async_runtime::spawn(listen(state, topic));
    }

    /// Le sujet de ce PC, créé et publié au premier lancement.
    fn own_topic(state: &AppState) -> Option<String> {
        let me = state.self_id.to_string();
        let prefix = format!("{SERVER}/");
        let known = state
            .push
            .lock()
            .target(&me)
            .and_then(|t| t.endpoint.strip_prefix(&prefix).map(str::to_owned))
            .filter(|topic| valid_topic(topic));
        if known.is_some() {
            return known;
        }
        let topic = format!("el-{}", uuid::Uuid::new_v4().simple());
        let endpoint = format!("{prefix}{topic}");
        if let Err(e) = state.push.change("d'enregistrer l'adresse de réveil", |d| {
            d.set_target(&me, &endpoint, now_ms())
        }) {
            state.notice(e);
            return None;
        }
        state.sync_wanted.notify_one();
        Store::changed(state, &[PUSH_DOC.to_owned()]);
        Some(topic)
    }

    async fn listen(state: Arc<AppState>, topic: String) {
        // Pas de délai global (la connexion reste ouverte), mais ntfy envoie
        // un signe de vie toutes les 45 s : sans rien pendant 2 min, on
        // se reconnecte.
        let client = match reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(120))
            .build()
        {
            Ok(client) => client,
            Err(e) => {
                state.notice(format!("Réveil par ntfy indisponible : {e}"));
                return;
            }
        };
        let mut since = CATCH_UP.to_owned();
        let mut delay = Duration::from_secs(5);
        loop {
            match listen_once(&client, &state, &topic, &mut since).await {
                Ok(()) => delay = Duration::from_secs(5),
                Err(e) => eprintln!("Écoute ntfy interrompue : {e}"),
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(300));
        }
    }

    /// Une connexion : lit les évènements ligne par ligne jusqu'à la coupure.
    async fn listen_once(
        client: &reqwest::Client,
        state: &AppState,
        topic: &str,
        since: &mut String,
    ) -> Result<(), String> {
        let url = format!("{SERVER}/{topic}/json?since={since}");
        let mut response = client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            buffer.extend_from_slice(&chunk);
            while let Some(end) = buffer.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buffer.drain(..=end).collect();
                handle_line(state, &line, since);
            }
            if buffer.len() > 64 * 1024 {
                return Err("évènement ntfy trop long".to_owned());
            }
        }
        Ok(())
    }

    /// Un évènement ntfy. Tout ce qui n'est pas un message chiffré avec notre
    /// clé est ignoré : le sujet est secret, mais rien n'empêche un inconnu
    /// qui le devinerait d'y écrire.
    fn handle_line(state: &AppState, line: &[u8], since: &mut String) {
        #[derive(Deserialize)]
        struct Event {
            id: Option<String>,
            event: String,
            message: Option<String>,
        }
        let Ok(event) = serde_json::from_slice::<Event>(line) else {
            return;
        };
        if event.event != "message" {
            return;
        }
        if let Some(id) = event.id {
            if !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
                *since = id;
            }
        }
        let key = state.push.lock().key();
        let (Some(text), Some(key)) = (event.message, key) else {
            return;
        };
        match open(&key, &text).map(couple_core::PushMessage::into_arrival) {
            Ok(Some(arrival)) => super::accept(state, arrival, false),
            Ok(None) => {}
            Err(e) => eprintln!("Message de réveil ignoré : {e}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use couple_core::{push::seal, Arrival, PushMessage};

        #[test]
        fn received_message_becomes_an_arrival() {
            let dir = crate::store::tests::temp_dir("push");
            let state = AppState::load(&dir, "PC").unwrap();
            state.push.change("", |d| d.ensure_key()).unwrap();
            let key = state.push.lock().key().unwrap();
            let arrival = Arrival {
                id: "id-1".into(),
                who: "Eve".into(),
                device: "tel-eve".into(),
                at: now_ms(),
            };
            let text = seal(&key, &PushMessage::arrival(&arrival)).unwrap();
            let line = serde_json::json!({"id": "abc123", "event": "message", "message": text});
            let mut since = CATCH_UP.to_owned();

            // Messages étrangers : ignorés, sans rien casser.
            handle_line(&state, br#"{"event":"keepalive","id":"k1"}"#, &mut since);
            handle_line(&state, b"pas du json", &mut since);
            let forged = serde_json::json!({"id": "x1", "event": "message", "message": "EL1.AAAA"});
            handle_line(&state, forged.to_string().as_bytes(), &mut since);
            assert!(state.arrivals.lock().list().is_empty());
            assert_eq!(since, "x1");

            handle_line(&state, line.to_string().as_bytes(), &mut since);
            assert_eq!(state.arrivals.lock().list(), vec![arrival.clone()]);
            assert_eq!(since, "abc123");
            // Reçu deux fois (rattrapage après une reconnexion) : une seule arrivée.
            handle_line(&state, line.to_string().as_bytes(), &mut since);
            assert_eq!(state.arrivals.lock().list(), vec![arrival]);
            std::fs::remove_dir_all(dir).unwrap();
        }

        #[test]
        fn topics() {
            let topic = format!("el-{}", uuid::Uuid::new_v4().simple());
            assert!(valid_topic(&topic));
            assert!(!valid_topic("el-abc"));
            assert!(!valid_topic("../../etc/passwd"));
            assert!(!valid_topic(&format!(
                "xx-{}",
                uuid::Uuid::new_v4().simple()
            )));
        }
    }
}

#[cfg(mobile)]
mod phone {
    use std::{sync::Arc, time::Duration};

    use couple_core::Arrival;
    use couple_sync::Store;
    use tauri::AppHandle;
    use tauri_plugin_geofence::{GeofenceExt, PushConfig};

    use crate::{
        now_ms,
        store::{AppState, PUSH_DOC},
    };

    pub fn start(app: &AppHandle, state: Arc<AppState>) {
        {
            let state = state.clone();
            if let Err(e) = app
                .geofence()
                .on_push_endpoint(move |endpoint| save_endpoint(&state, &endpoint))
            {
                eprintln!("Écoute de l'adresse ntfy impossible : {e}");
            }
        }
        {
            let (handler_app, state) = (app.clone(), state.clone());
            if let Err(e) = app.geofence().on_received(move |_| {
                let (app, state) = (handler_app.clone(), state.clone());
                tauri::async_runtime::spawn_blocking(move || ingest_received(&app, &state));
            }) {
                eprintln!("Écoute des réveils impossible : {e}");
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            {
                let (app, state) = (app.clone(), state.clone());
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    match app.geofence().register_push() {
                        Ok(status) => {
                            if let Some(endpoint) = status.endpoint {
                                save_endpoint(&state, &endpoint);
                            } else if !status.available {
                                eprintln!(
                                    "Aucune appli ntfy : l'arrivée ne réveillera pas les autres appareils."
                                );
                            }
                        }
                        Err(e) => eprintln!("Enregistrement auprès de ntfy impossible : {e}"),
                    }
                })
                .await;
            }
            loop {
                let (a, s) = (app.clone(), state.clone());
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    configure(&a, &s);
                    ingest_received(&a, &s);
                })
                .await;
                tokio::select! {
                    () = state.push_wanted.notified() => {}
                    () = tokio::time::sleep(Duration::from_secs(600)) => {}
                }
            }
        });
    }

    /// Publie l'adresse donnée par ntfy dans le document `push`.
    fn save_endpoint(state: &AppState, endpoint: &str) {
        let me = state.self_id.to_string();
        let same = state
            .push
            .lock()
            .target(&me)
            .is_some_and(|t| t.endpoint == endpoint.trim());
        if same {
            return;
        }
        match state.push.change("d'enregistrer l'adresse de réveil", |d| {
            d.set_target(&me, endpoint, now_ms())
        }) {
            Ok(()) => {
                state.sync_wanted.notify_one();
                Store::changed(state, &[PUSH_DOC.to_owned()]);
            }
            Err(e) => eprintln!("{e}"),
        }
    }

    /// Donne au plugin ce qu'il faut pour envoyer un réveil en arrivant.
    fn configure(app: &AppHandle, state: &AppState) {
        let me = state.self_id.to_string();
        let allowed: Vec<String> = state
            .devices
            .lock()
            .list()
            .into_iter()
            .map(|d| d.id)
            .collect();
        let (key, targets) = {
            let push = state.push.lock();
            (
                push.key_base64().unwrap_or_default(),
                push.targets_except(&me, &allowed),
            )
        };
        let who = crate::arrivals::load_settings(state).who;
        let config = PushConfig {
            key,
            who,
            device: me,
            targets,
        };
        if let Err(e) = app.geofence().set_push_config(config) {
            eprintln!("Impossible de configurer le réveil : {e}");
        }
    }

    /// Arrivées des autres reçues par ntfy pendant que l'appli dormait.
    fn ingest_received(app: &AppHandle, state: &AppState) {
        let received = match app.geofence().peek_received() {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Réveils reçus indisponibles : {e}");
                return;
            }
        };
        if received.is_empty() {
            return;
        }
        let mut ids = Vec::new();
        for r in received {
            ids.push(r.id.clone());
            let arrival = Arrival {
                id: r.id,
                who: r.who,
                device: r.device,
                at: r.at,
            };
            // Le téléphone a déjà sonné en recevant le message.
            super::accept(state, arrival, true);
        }
        if let Err(e) = app.geofence().ack_received(ids) {
            eprintln!("Impossible de confirmer les réveils reçus : {e}");
        }
    }
}
