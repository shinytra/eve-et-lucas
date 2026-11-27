//! Synchro en tâche de fond et commandes d'appairage.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use couple_sync::{Network, SyncNode};
use serde::Serialize;
use tauri::{async_runtime, AppHandle, Emitter, Manager, State};
use tokio::sync::{Notify, OnceCell};

use crate::{now_ms, store::AppState};

/// Intervalle entre deux synchros automatiques avec tous les appareils.
const SYNC_INTERVAL: Duration = Duration::from_secs(60);
/// Durée de validité d'un code d'appairage.
const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
/// Appli en arrière-plan (téléphone) : délai avant de couper le réseau, pour
/// finir une synchro en cours et ne pas couper pour un simple sélecteur de
/// fichier ou une demande d'autorisation.
const BACKGROUND_GRACE: Duration = Duration::from_secs(60);
/// En arrière-plan, synchro de temps en temps pour recevoir les
/// modifications des autres (widget à jour), si Android laisse l'appli tourner.
const BACKGROUND_SYNC_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Le nœud iroh, une fois démarré, et le dernier état de la synchro.
#[derive(Default)]
pub struct SyncHandle {
    node: OnceCell<SyncNode<AppState>>,
    /// Si le démarrage a échoué, pourquoi.
    start_error: Mutex<Option<String>>,
    /// Dernier état, pour l'interface qui s'ouvre après coup.
    last_status: Mutex<Option<SyncStatus>>,
    /// L'appli est en arrière-plan (téléphone seulement).
    background: AtomicBool,
    /// Passage au premier plan ou en arrière-plan, ou synchro faite en
    /// arrière-plan (relance le délai avant la pause).
    lifecycle: Notify,
    /// Synchros et téléchargements en cours : pas de pause pendant ce temps.
    busy: AtomicUsize,
}

/// Tient le réseau allumé tant qu'il existe (voir [`SyncHandle::busy`]).
struct Busy<'a>(&'a SyncHandle);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, Ordering::SeqCst);
        // Le délai avant la pause repart de maintenant.
        self.0.lifecycle.notify_one();
    }
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

    fn busy(&self) -> Busy<'_> {
        self.busy.fetch_add(1, Ordering::SeqCst);
        self.lifecycle.notify_one();
        Busy(self)
    }

    fn in_background(&self) -> bool {
        self.background.load(Ordering::SeqCst)
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
        spawn_lifecycle(app.clone());
        // La première synchro essaie tout le monde.
        let mut wanted = true;
        let mut last_background_sync = Instant::now();
        loop {
            if handle.in_background() {
                // En arrière-plan : réseau rallumé le temps d'envoyer une
                // modification faite ici (arrivée, case cochée sur le
                // widget…) et, toutes les 15 min, de recevoir celles des
                // autres (widget à jour).
                if wanted || last_background_sync.elapsed() >= BACKGROUND_SYNC_INTERVAL {
                    last_background_sync = Instant::now();
                    let _busy = handle.busy();
                    if ensure_running(&app, &handle, node).await {
                        run_once(&app, &handle, node, &state, wanted).await;
                        state.downloads_wanted.notify_one();
                    }
                }
            } else if ensure_running(&app, &handle, node).await {
                run_once(&app, &handle, node, &state, wanted).await;
                state.downloads_wanted.notify_one();
            }
            // Prochaine synchro : après une modification locale (tous les
            // appareils), ou au plus tard après l'intervalle (sans ceux qui
            // n'ont pas répondu récemment, réessayés de moins en moins souvent).
            wanted = tokio::time::timeout(SYNC_INTERVAL, state.sync_wanted.notified())
                .await
                .is_ok();
        }
    });
}

/// Rallume le réseau s'il est en pause ; `false` (état signalé) s'il ne
/// redémarre pas.
async fn ensure_running(app: &AppHandle, handle: &SyncHandle, node: &SyncNode<AppState>) -> bool {
    if !node.is_paused() {
        return true;
    }
    match node.resume().await {
        Ok(()) => true,
        Err(e) => {
            let message = format!("La synchro n'a pas pu reprendre : {e}");
            eprintln!("{message}");
            handle.set_status(app, SyncStatus::error(message));
            false
        }
    }
}

/// L'appli passe en arrière-plan (téléphone).
#[cfg(mobile)]
pub fn suspended(app: &AppHandle) {
    let handle = app.state::<SyncHandle>();
    handle.background.store(true, Ordering::SeqCst);
    handle.lifecycle.notify_one();
}

/// L'appli revient au premier plan (téléphone) : reconnexion, puis synchro
/// avec tout le monde.
#[cfg(mobile)]
pub fn resumed(app: &AppHandle) {
    let handle = app.state::<SyncHandle>();
    handle.background.store(false, Ordering::SeqCst);
    handle.lifecycle.notify_one();
    let app = app.clone();
    async_runtime::spawn(async move {
        let handle = app.state::<SyncHandle>();
        if let Some(node) = handle.node.get() {
            if ensure_running(&app, &handle, node).await {
                // Le réseau a pu changer pendant l'absence (Wi-Fi ↔ données).
                node.network_change().await;
            }
        }
        app.state::<Arc<AppState>>().sync_wanted.notify_one();
    });
}

/// Coupe le réseau quand l'appli est restée [`BACKGROUND_GRACE`] en
/// arrière-plan sans rien synchroniser : plus de ping au relais toutes les
/// 15 s ni de sondage réseau toutes les 20 à 26 s, la radio du téléphone peut
/// dormir.
fn spawn_lifecycle(app: AppHandle) {
    async_runtime::spawn(async move {
        let handle = app.state::<SyncHandle>();
        let Some(node) = handle.node.get() else {
            return;
        };
        loop {
            if !handle.in_background() || node.is_paused() {
                handle.lifecycle.notified().await;
                continue;
            }
            // Retour au premier plan, ou synchro qui commence ou finit : on
            // recompte le délai.
            if tokio::time::timeout(BACKGROUND_GRACE, handle.lifecycle.notified())
                .await
                .is_ok()
            {
                continue;
            }
            // Décidé au dernier moment : un retour au premier plan pendant
            // l'attente du verrou l'emporte.
            node.pause_if(|| handle.in_background() && handle.busy.load(Ordering::SeqCst) == 0)
                .await;
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
            // Le réseau reste allumé jusqu'à la fin des téléchargements.
            let _busy = handle.busy();
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
    everyone: bool,
) {
    let results = if everyone {
        node.sync_all().await
    } else {
        node.sync_due().await
    };
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
                unreachable: e.is_unreachable(),
                message: e.to_string(),
            });
        }
    }
    // Appareils non essayés cette fois (pas de réponse aux dernières tentatives).
    let mut total = results.len();
    for peer in couple_sync::Store::peers(state) {
        if results.iter().all(|(id, _)| *id != peer) {
            total += 1;
            failures.push(SyncFailure {
                device: names
                    .get(&peer.to_string())
                    .cloned()
                    .unwrap_or_else(|| peer.fmt_short().to_string()),
                unreachable: true,
                message: "injoignable aux dernières tentatives, nouvel essai bientôt".to_owned(),
            });
        }
    }
    let status = SyncStatus {
        at: now_ms(),
        reached: total - failures.len(),
        total,
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
    couple_sync::Store::changed(&**state, &[crate::store::DEVICES_DOC.to_owned()]);
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
    // Nouvelle clé de réveil : l'appareil retiré garde l'ancienne, mais ne
    // peut plus ni lire ni fabriquer de réveils acceptés.
    let rotated = state
        .push
        .change("de changer la clé de réveil", |d| d.rotate_key(&id));
    state.sync_wanted.notify_one();
    couple_sync::Store::changed(
        &**state,
        &[
            crate::store::DEVICES_DOC.to_owned(),
            crate::store::PUSH_DOC.to_owned(),
        ],
    );
    rotated
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
