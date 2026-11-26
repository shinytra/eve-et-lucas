//! Synchro en tâche de fond et commandes d'appairage.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use couple_sync::{Network, SyncNode};
use serde::Serialize;
use tauri::{async_runtime, AppHandle, Emitter, Manager, State};
use tokio::sync::OnceCell;

use crate::{now_ms, store::AppState};

/// Intervalle entre deux synchros automatiques avec tous les appareils.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Durée de validité d'un code d'appairage.
const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);

/// Le nœud iroh, une fois démarré, et le dernier état de la synchro.
#[derive(Default)]
pub struct SyncHandle {
    node: OnceCell<SyncNode<AppState>>,
    /// Si le démarrage a échoué, pourquoi.
    start_error: Mutex<Option<String>>,
    /// Dernier état, pour l'interface qui s'ouvre après coup.
    last_status: Mutex<Option<SyncStatus>>,
}

impl SyncHandle {
    /// Le nœud, s'il a démarré.
    pub fn node(&self) -> Result<&SyncNode<AppState>, String> {
        self.node.get().ok_or_else(|| {
            self.start_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
                .unwrap_or_else(|| {
                    "La synchro démarre encore, réessaie dans un instant.".to_owned()
                })
        })
    }

    fn set_status(&self, app: &AppHandle, status: SyncStatus) {
        *self.last_status.lock().unwrap_or_else(|e| e.into_inner()) = Some(status.clone());
        let _ = app.emit("sync-status", status);
    }
}

/// Démarre le nœud et la boucle de synchro (en arrière-plan, sans bloquer
/// l'ouverture de la fenêtre).
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let _ = state.app.set(app.clone());
    let app = app.clone();
    async_runtime::spawn(async move {
        let files_dir = state.data_dir.join("fichiers");
        let spawned = SyncNode::spawn_with_files(
            state.secret.clone(),
            state.clone(),
            Network::Online,
            &files_dir,
        )
        .await;
        let node = match spawned {
            Ok(node) => node,
            Err(e) => {
                let message = format!("La synchro n'a pas pu démarrer : {e}");
                eprintln!("{message}");
                let handle = app.state::<SyncHandle>();
                *handle.start_error.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(message.clone());
                handle.set_status(&app, SyncStatus::error(message));
                return;
            }
        };
        let handle = app.state::<SyncHandle>();
        let _ = handle.node.set(node);
        let Some(node) = handle.node.get() else {
            return;
        };
        spawn_downloads(app.clone(), state.clone());
        loop {
            run_once(&app, &handle, node, &state).await;
            state.downloads_wanted.notify_one();
            // Prochaine synchro : après une modification locale, ou au plus tard
            // après l'intervalle.
            let _ = tokio::time::timeout(SYNC_INTERVAL, state.sync_wanted.notified()).await;
        }
    });
}

/// Téléchargements de fichiers, à part de la synchro des documents : un gros
/// fichier ne retarde pas les notes et les tâches. Après chaque passage, les
/// contenus des fichiers supprimés (ici ou ailleurs) sont libérés.
fn spawn_downloads(app: AppHandle, state: Arc<AppState>) {
    async_runtime::spawn(async move {
        let handle = app.state::<SyncHandle>();
        let Some(node) = handle.node.get() else {
            return;
        };
        loop {
            state.downloads_wanted.notified().await;
            for peer in couple_sync::Store::peers(&*state) {
                node.fetch_wanted_from(peer).await;
            }
            let _guard = state.blob_guard.lock().await;
            let keep = state.files.lock().hashes();
            if let Err(e) = node.retain_blobs(&keep).await {
                eprintln!("Ménage des fichiers impossible : {e}");
            }
        }
    });
}

async fn run_once(
    app: &AppHandle,
    handle: &SyncHandle,
    node: &SyncNode<AppState>,
    state: &AppState,
) {
    let results = node.sync_all().await;
    let names: std::collections::HashMap<String, String> = state
        .devices
        .lock()
        .list()
        .into_iter()
        .map(|d| (d.id, d.name))
        .collect();
    let mut failures = Vec::new();
    for (peer, result) in &results {
        if let Err(e) = result {
            let name = names
                .get(&peer.to_string())
                .cloned()
                .unwrap_or_else(|| peer.fmt_short().to_string());
            eprintln!("Synchro avec {name} impossible : {e}");
            failures.push(SyncFailure {
                device: name,
                // Un appareil injoignable est normal (téléphone en veille, PC
                // éteint) ; les autres erreurs méritent d'être signalées.
                unreachable: matches!(
                    e,
                    couple_sync::SyncError::Timeout | couple_sync::SyncError::Network(_)
                ),
                message: e.to_string(),
            });
        }
    }
    let status = SyncStatus {
        at: now_ms(),
        reached: results.len() - failures.len(),
        total: results.len(),
        error: None,
        failures,
    };
    handle.set_status(app, status);
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncFailure {
    pub device: String,
    pub unreachable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    /// Heure de la dernière synchro (millisecondes Unix).
    pub at: i64,
    /// Appareils joints lors de cette synchro.
    pub reached: usize,
    /// Appareils appairés.
    pub total: usize,
    pub error: Option<String>,
    /// Appareils non joints, et pourquoi.
    pub failures: Vec<SyncFailure>,
}

impl SyncStatus {
    fn error(message: String) -> Self {
        Self {
            at: now_ms(),
            reached: 0,
            total: 0,
            error: Some(message),
            failures: Vec::new(),
        }
    }
}

fn node<'a>(handle: &'a State<'_, SyncHandle>) -> Result<&'a SyncNode<AppState>, String> {
    if let Some(node) = handle.node.get() {
        return Ok(node);
    }
    let start_error = handle
        .start_error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Err(start_error.unwrap_or_else(|| {
        "La synchro démarre encore, réessaie dans quelques secondes.".to_owned()
    }))
}

/// Dernier état connu de la synchro (l'interface le demande à l'ouverture).
#[tauri::command]
pub fn sync_status(handle: State<'_, SyncHandle>) -> Option<SyncStatus> {
    handle
        .last_status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[derive(Debug, Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub added_at: i64,
    pub this_device: bool,
}

#[tauri::command]
pub fn list_devices(state: State<'_, Arc<AppState>>) -> Vec<DeviceView> {
    let me = state.self_id.to_string();
    state
        .devices
        .lock()
        .list()
        .into_iter()
        .map(|d| DeviceView {
            this_device: d.id == me,
            id: d.id,
            name: d.name,
            added_at: d.added_at,
        })
        .collect()
}

#[tauri::command]
pub fn rename_this_device(state: State<'_, Arc<AppState>>, name: String) -> Result<(), String> {
    let id = state.self_id.to_string();
    state
        .devices
        .change("de renommer l'appareil", |d| d.upsert(&id, &name, now_ms()))?;
    state.sync_wanted.notify_one();
    Ok(())
}

#[tauri::command]
pub fn remove_device(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    if id == state.self_id.to_string() {
        return Err("Impossible de retirer cet appareil lui-même.".into());
    }
    state
        .devices
        .change("de retirer l'appareil", |d| d.remove(&id, now_ms()))?;
    state.sync_wanted.notify_one();
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PairingView {
    /// Code à saisir (ou scanner) sur l'autre appareil.
    pub code: String,
    /// QR code du même code, en SVG.
    pub qr_svg: String,
    pub expires_in_s: u64,
}

#[tauri::command]
pub fn start_pairing(handle: State<'_, SyncHandle>) -> Result<PairingView, String> {
    let node = node(&handle)?;
    let code = node
        .start_pairing(PAIRING_TTL)
        .map_err(|e| format!("Impossible de créer le code : {e}"))?
        .encode();
    let qr = qrcode::QrCode::new(code.as_bytes())
        .map_err(|e| format!("Impossible de créer le QR code : {e}"))?;
    let qr_svg = qr
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .build();
    Ok(PairingView {
        code,
        qr_svg,
        expires_in_s: PAIRING_TTL.as_secs(),
    })
}

#[tauri::command]
pub fn cancel_pairing(handle: State<'_, SyncHandle>) -> Result<(), String> {
    node(&handle)?.cancel_pairing();
    Ok(())
}

#[tauri::command]
pub async fn join_pairing(
    handle: State<'_, SyncHandle>,
    state: State<'_, Arc<AppState>>,
    code: String,
) -> Result<String, String> {
    let node = node(&handle)?;
    let outcome = node
        .join(&code)
        .await
        .map_err(|e| format!("Appairage impossible : {e}"))?;
    // Prévient aussi les autres appareils déjà connus.
    state.sync_wanted.notify_one();
    Ok(match outcome.first_sync {
        Ok(_) => format!("Appairé avec {}, synchro faite.", outcome.host_name),
        Err(e) => format!(
            "Appairé avec {}. La première synchro a échoué ({e}), elle sera refaite automatiquement.",
            outcome.host_name
        ),
    })
}

#[tauri::command]
pub fn sync_now(state: State<'_, Arc<AppState>>) {
    state.sync_wanted.notify_one();
}

#[tauri::command]
pub fn this_device(state: State<'_, Arc<AppState>>) -> DeviceView {
    DeviceView {
        id: state.self_id.to_string(),
        name: state.device_name(),
        added_at: 0,
        this_device: true,
    }
}
