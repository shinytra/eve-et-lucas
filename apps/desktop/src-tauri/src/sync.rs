//! Synchro en tâche de fond et commandes d'appairage.

use std::{sync::Arc, time::Duration};

use couple_sync::{Network, SyncNode};
use serde::Serialize;
use tauri::{async_runtime, AppHandle, Emitter, Manager, State};
use tokio::sync::OnceCell;

use crate::{now_ms, store::AppState};

/// Intervalle entre deux synchros automatiques avec tous les appareils.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Durée de validité d'un code d'appairage.
const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);

/// Le nœud iroh, une fois démarré.
#[derive(Default)]
pub struct SyncHandle(pub OnceCell<SyncNode<AppState>>);

/// Démarre le nœud et la boucle de synchro (en arrière-plan, sans bloquer
/// l'ouverture de la fenêtre).
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let _ = state.app.set(app.clone());
    let app = app.clone();
    async_runtime::spawn(async move {
        let node = match SyncNode::spawn(state.secret.clone(), state.clone(), Network::Online).await
        {
            Ok(node) => node,
            Err(e) => {
                let message = format!("La synchro n'a pas pu démarrer : {e}");
                eprintln!("{message}");
                let _ = app.emit("sync-status", SyncStatus::error(message));
                return;
            }
        };
        let handle = app.state::<SyncHandle>();
        let _ = handle.0.set(node);
        let Some(node) = handle.0.get() else { return };
        loop {
            run_once(&app, node).await;
            // Prochaine synchro : après une modification locale, ou au plus tard
            // après l'intervalle.
            let _ = tokio::time::timeout(SYNC_INTERVAL, state.sync_wanted.notified()).await;
        }
    });
}

async fn run_once(app: &AppHandle, node: &SyncNode<AppState>) {
    let results = node.sync_all().await;
    let reached = results.iter().filter(|(_, r)| r.is_ok()).count();
    let status = SyncStatus {
        at: now_ms(),
        reached,
        total: results.len(),
        error: None,
    };
    let _ = app.emit("sync-status", status);
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
}

impl SyncStatus {
    fn error(message: String) -> Self {
        Self {
            at: now_ms(),
            reached: 0,
            total: 0,
            error: Some(message),
        }
    }
}

fn node<'a>(handle: &'a State<'_, SyncHandle>) -> Result<&'a SyncNode<AppState>, String> {
    handle
        .0
        .get()
        .ok_or_else(|| "La synchro démarre encore, réessaie dans quelques secondes.".to_owned())
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
        .change("de retirer l'appareil", |d| d.remove(&id))?;
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
) -> Result<(), String> {
    let node = node(&handle)?;
    node.join(&code)
        .await
        .map_err(|e| format!("Appairage impossible : {e}"))?;
    // Prévient aussi les autres appareils déjà connus.
    state.sync_wanted.notify_one();
    Ok(())
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
