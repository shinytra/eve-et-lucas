//! Synchro P2P des documents du couple, avec iroh.
//!
//! - Chaque appareil a une clé iroh (son identité) et une copie complète des
//!   documents. Il n'y a pas de serveur : le PC, allumé presque tout le temps,
//!   est simplement un pair de plus qui garde tout.
//! - **Appairage** (ALPN [`PAIR_ALPN`]) : l'appareil hôte produit un code
//!   d'appairage à usage unique (affiché en QR code), valable quelques minutes.
//!   L'appareil qui le saisit se présente avec ce code ; chacun ajoute l'autre
//!   à son document des appareils, puis ils se synchronisent.
//! - **Synchro** (ALPN [`SYNC_ALPN`]) : n'accepte que les appareils présents
//!   dans le document des appareils. Pour chaque document, les deux côtés
//!   échangent des messages du protocole de synchro d'Automerge jusqu'à ce
//!   qu'aucun n'ait plus rien à envoyer.
//!
//! - **Fichiers** (ALPN d'iroh-blobs) : le contenu des fichiers partagés est
//!   rangé par empreinte BLAKE3 et transféré de façon vérifiée. Seuls les
//!   appareils appairés peuvent en demander. L'appli télécharge ceux qu'elle
//!   veut ([`Store::wanted_blobs`]) avec [`SyncNode::fetch_wanted_from`], à
//!   part, pour qu'un gros fichier ne retarde pas la synchro des documents.
//!
//! Le stockage (où sont les documents, comment ils sont enregistrés) est fourni
//! par l'appli via le trait [`Store`].

mod blobs;

use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};

use couple_core::SyncState;
use iroh::{
    endpoint::{Connection, RecvStream, SendStream},
    protocol::{AcceptError, ProtocolHandler, Router},
    Endpoint, EndpointAddr, EndpointId, SecretKey,
};
use serde::{Deserialize, Serialize};

pub use iroh;
pub use iroh_blobs;

/// Protocole de synchro des documents.
pub const SYNC_ALPN: &[u8] = b"eve-et-lucas/sync/1";
/// Protocole d'appairage d'un nouvel appareil.
pub const PAIR_ALPN: &[u8] = b"eve-et-lucas/pair/1";
/// Préfixe des codes d'appairage (permet de reconnaître un code et sa version).
pub const PAIRING_PREFIX: &str = "EL1:";

/// Taille maximale d'un message de synchro (entre appareils appairés).
const MAX_FRAME: usize = 64 * 1024 * 1024;
/// Taille maximale d'un message d'appairage : il arrive avant toute
/// authentification, donc d'un inconnu potentiel.
const MAX_PAIR_FRAME: usize = 4 * 1024;
/// Nombre maximal d'allers-retours par document (la synchro converge bien avant).
const MAX_ROUNDS: usize = 200;
/// Délai maximal pour une étape réseau.
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Délai maximal pour joindre un appareil (téléphone en veille, PC éteint…).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Attente avant de réessayer un appareil injoignable, doublée à chaque échec
/// (2, 4, 8 min…) jusqu'à [`RETRY_MAX`]. Chaque tentative réveille la radio
/// du téléphone pendant une vingtaine de secondes : inutile de le faire toutes
/// les minutes pour un PC éteint. Une modification locale, une connexion de
/// cet appareil ou [`SyncNode::retry_now`] fait réessayer aussitôt.
const RETRY_MIN: Duration = Duration::from_secs(2 * 60);
const RETRY_MAX: Duration = Duration::from_secs(15 * 60);

/// Erreurs de la synchro.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("réseau : {0}")]
    Network(String),
    #[error("code d'appairage invalide")]
    InvalidCode,
    #[error("code d'appairage refusé : {0}")]
    PairingRefused(String),
    #[error("appareil non appairé")]
    UnknownPeer,
    #[error("le pair ne répond pas")]
    Timeout,
    #[error("protocole : {0}")]
    Protocol(String),
    #[error("stockage : {0}")]
    Store(String),
    #[error("synchro en pause (appli en arrière-plan)")]
    Paused,
    #[error("appareil injoignable : {0}")]
    Unreachable(String),
}

impl SyncError {
    /// L'appareil n'a pas pu être joint ou a cessé de répondre (en veille,
    /// éteint, hors réseau) : normal, pas une panne.
    pub fn is_unreachable(&self) -> bool {
        matches!(
            self,
            Self::Unreachable(_) | Self::Timeout | Self::Network(_)
        )
    }
}

/// Ouvre une connexion ; un échec ici veut dire que l'appareil est absent
/// ([`SyncError::Unreachable`], qui seul déclenche l'attente avant de réessayer).
pub(crate) async fn connect(
    endpoint: &Endpoint,
    addr: EndpointAddr,
    alpn: &[u8],
) -> Result<Connection> {
    tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(addr, alpn))
        .await
        .map_err(|_| SyncError::Unreachable("pas de réponse".to_owned()))?
        .map_err(|e| SyncError::Unreachable(e.to_string()))
}

pub type Result<T> = std::result::Result<T, SyncError>;

fn net(e: impl std::fmt::Display) -> SyncError {
    SyncError::Network(e.to_string())
}

/// Ce que l'appli doit fournir pour être synchronisée.
pub trait Store: Send + Sync + 'static {
    /// Noms des documents à synchroniser (les mêmes sur tous les appareils).
    fn doc_names(&self) -> Vec<String>;
    /// Prochain message de synchro pour ce document.
    fn generate(
        &self,
        doc: &str,
        state: &mut SyncState,
    ) -> std::result::Result<Option<Vec<u8>>, String>;
    /// Applique un message reçu (et l'enregistre). `true` si le document a changé.
    fn receive(
        &self,
        doc: &str,
        state: &mut SyncState,
        message: &[u8],
    ) -> std::result::Result<bool, String>;
    /// Identifiants iroh des appareils appairés (sans celui-ci).
    fn peers(&self) -> Vec<EndpointId>;
    /// Ajoute (ou renomme) un appareil appairé.
    fn add_peer(&self, id: EndpointId, name: &str) -> std::result::Result<(), String>;
    /// Nom de cet appareil, donné à l'autre lors de l'appairage.
    fn device_name(&self) -> String;
    /// Appelé après qu'une synchro a modifié des documents (pour rafraîchir l'interface).
    fn changed(&self, docs: &[String]);
    /// Le code d'appairage affiché vient d'être utilisé (accepté ou non) :
    /// il n'est plus valable.
    fn pairing_used(&self, _accepted: bool) {}
    /// Empreintes (hexadécimal) des fichiers à télécharger automatiquement
    /// s'ils manquent sur cet appareil.
    fn wanted_blobs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Des fichiers viennent d'être téléchargés.
    fn blobs_received(&self, _hashes: &[String]) {}
}

/// Comment l'appareil se connecte au réseau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    /// Réseau public d'iroh : relais et annuaire DNS de n0, fonctionne derrière
    /// une box sans ouvrir de port. Les relais transmettent des données chiffrées
    /// et ne stockent rien.
    Online,
    /// Pas de relais ni d'annuaire : connexions directes seulement (tests).
    Local,
}

/// Contenu d'un code d'appairage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairingCode {
    /// Adresse de l'appareil hôte.
    pub host: EndpointAddr,
    /// Secret à usage unique.
    pub token: String,
}

impl PairingCode {
    pub fn encode(&self) -> String {
        // Sérialiser une adresse et un texte ne peut pas échouer.
        let json = serde_json::to_vec(self).expect("code d'appairage sérialisable");
        format!(
            "{PAIRING_PREFIX}{}",
            data_encoding::BASE64URL_NOPAD.encode(&json)
        )
    }

    pub fn decode(code: &str) -> Result<Self> {
        let body = code
            .trim()
            .strip_prefix(PAIRING_PREFIX)
            .ok_or(SyncError::InvalidCode)?;
        let json = data_encoding::BASE64URL_NOPAD
            .decode(body.as_bytes())
            .map_err(|_| SyncError::InvalidCode)?;
        serde_json::from_slice(&json).map_err(|_| SyncError::InvalidCode)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PairRequest {
    token: String,
    name: String,
    addr: EndpointAddr,
}

#[derive(Debug, Serialize, Deserialize)]
struct PairResponse {
    accepted: bool,
    name: String,
    /// Pourquoi l'appairage est refusé.
    #[serde(default)]
    reason: Option<String>,
}

/// Résultat d'un appairage réussi.
#[derive(Debug)]
pub struct JoinOutcome {
    /// Nom de l'appareil rejoint.
    pub host_name: String,
    /// Première synchro (l'appairage reste valable même si elle échoue :
    /// elle sera refaite à la synchro suivante).
    pub first_sync: Result<SyncReport>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DocHeader {
    doc: String,
}

/// Résultat d'une synchro avec un pair.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Documents modifiés de notre côté.
    pub changed: Vec<String>,
}

struct Pending {
    token: String,
    expires: Instant,
}

/// Un appareil qui n'a pas répondu : combien de fois, et quand réessayer.
#[derive(Debug, Clone, Copy)]
struct Absent {
    failures: u32,
    retry_at: Instant,
}

/// Attente après `failures` échecs de suite.
fn retry_delay(failures: u32) -> Duration {
    let doublings = failures.saturating_sub(1).min(8);
    RETRY_MIN.saturating_mul(1 << doublings).min(RETRY_MAX)
}

pub(crate) struct Shared<S: Store> {
    pub(crate) store: Arc<S>,
    pending: Mutex<Option<Pending>>,
    /// Adresses connues (apprises à l'appairage) pour joindre un pair sans annuaire.
    addrs: Mutex<HashMap<EndpointId, EndpointAddr>>,
    /// Appareils injoignables lors des dernières tentatives.
    absent: Mutex<HashMap<EndpointId, Absent>>,
}

impl<S: Store> Shared<S> {
    fn addrs(&self) -> MutexGuard<'_, HashMap<EndpointId, EndpointAddr>> {
        self.addrs.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn pending(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn absent(&self) -> MutexGuard<'_, HashMap<EndpointId, Absent>> {
        self.absent.lock().unwrap_or_else(|e| e.into_inner())
    }
    /// Note le résultat d'une tentative de connexion à `peer`.
    fn record(&self, peer: EndpointId, result: std::result::Result<(), &SyncError>) {
        let mut absent = self.absent();
        match result {
            Ok(()) => {
                absent.remove(&peer);
            }
            Err(SyncError::Unreachable(_)) => {
                let failures = absent.get(&peer).map_or(0, |a| a.failures) + 1;
                absent.insert(
                    peer,
                    Absent {
                        failures,
                        retry_at: Instant::now() + retry_delay(failures),
                    },
                );
            }
            // Connexion établie puis perdue, pause, erreur de protocole ou de
            // stockage : l'appareil était là, on réessaie normalement.
            Err(_) => {}
        }
    }
}

/// Un appareil en ligne, prêt à appairer et synchroniser.
///
/// Le nœud peut être mis en pause ([`SyncNode::pause`]) : la connexion au
/// relais (un ping toutes les 15 s) et les sondages réseau d'iroh (toutes les
/// 20 à 26 s) s'arrêtent, ce qui laisse dormir la radio du téléphone quand
/// l'appli est en arrière-plan. Les documents et les fichiers restent
/// disponibles ; [`SyncNode::resume`] reconnecte.
pub struct SyncNode<S: Store> {
    secret: SecretKey,
    network: Network,
    /// `None` pendant une pause.
    router: Mutex<Option<Router>>,
    /// Une pause ou une reprise à la fois.
    lifecycle: tokio::sync::Mutex<()>,
    shared: Arc<Shared<S>>,
    blobs: blobs::Blobs,
}

impl<S: Store> SyncNode<S> {
    /// Démarre le nœud avec la clé (identité) de l'appareil. Le contenu des
    /// fichiers est gardé en mémoire (tests) : voir [`SyncNode::spawn_with_files`].
    pub async fn spawn(secret: SecretKey, store: Arc<S>, network: Network) -> Result<Self> {
        Self::spawn_inner(secret, store, network, blobs::Blobs::memory()).await
    }

    /// Démarre le nœud en rangeant le contenu des fichiers dans `dir`.
    pub async fn spawn_with_files(
        secret: SecretKey,
        store: Arc<S>,
        network: Network,
        dir: &Path,
    ) -> Result<Self> {
        let blobs = blobs::Blobs::on_disk(dir).await?;
        Self::spawn_inner(secret, store, network, blobs).await
    }

    async fn spawn_inner(
        secret: SecretKey,
        store: Arc<S>,
        network: Network,
        blobs: blobs::Blobs,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            store,
            pending: Mutex::new(None),
            addrs: Mutex::new(HashMap::new()),
            absent: Mutex::new(HashMap::new()),
        });
        let router = start_router(&secret, network, &shared, &blobs).await?;
        Ok(Self {
            secret,
            network,
            router: Mutex::new(Some(router)),
            lifecycle: tokio::sync::Mutex::new(()),
            shared,
            blobs,
        })
    }

    fn router(&self) -> Result<Router> {
        self.router
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(SyncError::Paused)
    }

    pub(crate) fn endpoint(&self) -> Result<Endpoint> {
        Ok(self.router()?.endpoint().clone())
    }

    /// Le nœud est-il en pause ?
    pub fn is_paused(&self) -> bool {
        self.router
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
    }

    /// Met le nœud en pause : plus aucun échange réseau, et les autres
    /// appareils ne peuvent plus le joindre. Sans effet s'il l'est déjà.
    pub async fn pause(&self) {
        self.pause_if(|| true).await;
    }

    /// Comme [`SyncNode::pause`], si `still_wanted()` le confirme une fois
    /// le tour venu (une reprise demandée entre-temps l'emporte). Renvoie
    /// `true` si le nœud a été mis en pause.
    pub async fn pause_if(&self, still_wanted: impl FnOnce() -> bool) -> bool {
        let _turn = self.lifecycle.lock().await;
        if !still_wanted() {
            return false;
        }
        let router = self.router.lock().unwrap_or_else(|e| e.into_inner()).take();
        let paused = router.is_some();
        if let Some(router) = router {
            let _ = router.shutdown().await;
        }
        paused
    }

    /// Reconnecte le nœud après une pause. Sans effet s'il tourne déjà.
    pub async fn resume(&self) -> Result<()> {
        let _turn = self.lifecycle.lock().await;
        if !self.is_paused() {
            return Ok(());
        }
        let router = start_router(&self.secret, self.network, &self.shared, &self.blobs).await?;
        *self.router.lock().unwrap_or_else(|e| e.into_inner()) = Some(router);
        Ok(())
    }

    /// Le réseau a peut-être changé (Wi-Fi ↔ données mobiles, retour de
    /// veille) : iroh refait ses sondages, et les appareils injoignables sont
    /// réessayés dès la prochaine synchro. Android ne prévient pas le code
    /// natif de ces changements : l'appli appelle ceci au retour au premier plan.
    pub async fn network_change(&self) {
        self.retry_now();
        if let Ok(endpoint) = self.endpoint() {
            endpoint.network_change().await;
        }
    }

    /// Oublie les échecs passés : tous les appareils seront réessayés.
    pub fn retry_now(&self) {
        self.shared.absent().clear();
    }

    /// Appareils appairés à essayer maintenant (ceux qui n'ont pas répondu
    /// récemment attendent leur tour, voir [`RETRY_MIN`]).
    pub fn peers_due(&self) -> Vec<EndpointId> {
        let now = Instant::now();
        let absent = self.shared.absent();
        self.shared
            .store
            .peers()
            .into_iter()
            .filter(|p| *p != self.id() && absent.get(p).is_none_or(|a| a.retry_at <= now))
            .collect()
    }

    /// L'appareil a-t-il répondu à la dernière tentative ?
    pub fn is_reachable(&self, peer: EndpointId) -> bool {
        !self.shared.absent().contains_key(&peer)
    }

    pub fn id(&self) -> EndpointId {
        self.secret.public()
    }

    /// Adresse de cet appareil (seulement l'identifiant pendant une pause).
    pub fn addr(&self) -> EndpointAddr {
        match self.endpoint() {
            Ok(endpoint) => endpoint.addr(),
            Err(_) => EndpointAddr::new(self.id()),
        }
    }

    /// Mémorise l'adresse directe d'un appareil (sinon il est cherché via
    /// l'annuaire d'iroh, ce qui n'existe pas en mode [`Network::Local`]).
    pub fn add_addr(&self, addr: EndpointAddr) {
        self.shared.addrs().insert(addr.id, addr);
    }

    /// Attend d'être joignable (relais contacté), au plus `timeout`.
    pub async fn wait_online(&self, timeout: Duration) -> bool {
        let Ok(endpoint) = self.endpoint() else {
            return false;
        };
        tokio::time::timeout(timeout, endpoint.online())
            .await
            .is_ok()
    }

    /// Crée un code d'appairage à usage unique, valable `ttl`. Un nouveau code
    /// remplace le précédent.
    pub fn start_pairing(&self, ttl: Duration) -> Result<PairingCode> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| SyncError::Protocol(e.to_string()))?;
        let token = data_encoding::HEXLOWER.encode(&bytes);
        *self.shared.pending() = Some(Pending {
            token: token.clone(),
            expires: Instant::now() + ttl,
        });
        Ok(PairingCode {
            host: self.addr(),
            token,
        })
    }

    /// Annule le code d'appairage en cours.
    pub fn cancel_pairing(&self) {
        *self.shared.pending() = None;
    }

    /// Rejoint l'appareil qui a affiché `code`, puis se synchronise avec lui.
    pub async fn join(&self, code: &str) -> Result<JoinOutcome> {
        let code = PairingCode::decode(code)?;
        let host_id = code.host.id;
        let conn = timeout(self.endpoint()?.connect(code.host.clone(), PAIR_ALPN))
            .await?
            .map_err(net)?;
        let (mut send, mut recv) = conn.open_bi().await.map_err(net)?;
        let request = PairRequest {
            token: code.token,
            name: self.shared.store.device_name(),
            addr: self.addr(),
        };
        write_json(&mut send, &request).await?;
        send.finish().map_err(net)?;
        let response: PairResponse = read_json(&mut recv).await?;
        conn.close(0u32.into(), b"ok");
        if !response.accepted {
            return Err(SyncError::PairingRefused(
                response
                    .reason
                    .unwrap_or_else(|| "code expiré ou déjà utilisé".to_owned()),
            ));
        }
        self.shared.addrs().insert(host_id, code.host);
        self.shared
            .store
            .add_peer(host_id, &response.name)
            .map_err(SyncError::Store)?;
        Ok(JoinOutcome {
            host_name: response.name,
            first_sync: self.sync_with(host_id).await,
        })
    }

    /// Se synchronise avec un appareil appairé.
    pub async fn sync_with(&self, peer: EndpointId) -> Result<SyncReport> {
        let result = self.sync_with_inner(peer).await;
        self.shared.record(peer, result.as_ref().map(|_| ()));
        result
    }

    async fn sync_with_inner(&self, peer: EndpointId) -> Result<SyncReport> {
        let endpoint = self.endpoint()?;
        let addr = self
            .shared
            .addrs()
            .get(&peer)
            .cloned()
            .unwrap_or_else(|| EndpointAddr::new(peer));
        let conn = connect(&endpoint, addr, SYNC_ALPN).await?;
        let mut report = SyncReport::default();
        for doc in self.shared.store.doc_names() {
            let (mut send, mut recv) = conn.open_bi().await.map_err(net)?;
            write_json(&mut send, &DocHeader { doc: doc.clone() }).await?;
            if run_sync(&*self.shared.store, &doc, &mut send, &mut recv, true).await? {
                // Prévenir tout de suite, même si le document suivant échoue.
                self.shared.store.changed(std::slice::from_ref(&doc));
                report.changed.push(doc);
            }
        }
        conn.close(0u32.into(), b"fini");
        Ok(report)
    }

    /// Se synchronise avec tous les appareils appairés. Un appareil injoignable
    /// (téléphone en veille, PC éteint) n'empêche pas les autres.
    pub async fn sync_all(&self) -> Vec<(EndpointId, Result<SyncReport>)> {
        let peers: Vec<_> = self
            .shared
            .store
            .peers()
            .into_iter()
            .filter(|p| *p != self.id())
            .collect();
        self.sync_peers(peers).await
    }

    /// Comme [`SyncNode::sync_all`], sans les appareils qui n'ont pas répondu
    /// récemment et dont le tour n'est pas venu ([`SyncNode::peers_due`]).
    pub async fn sync_due(&self) -> Vec<(EndpointId, Result<SyncReport>)> {
        self.sync_peers(self.peers_due()).await
    }

    async fn sync_peers(&self, peers: Vec<EndpointId>) -> Vec<(EndpointId, Result<SyncReport>)> {
        let mut results = Vec::new();
        for peer in peers {
            results.push((peer, self.sync_with(peer).await));
        }
        results
    }

    /// Arrête proprement le nœud.
    pub async fn shutdown(self) {
        self.pause().await;
        if let Err(e) = self.blobs.api().shutdown().await {
            eprintln!("Arrêt du stockage des fichiers : {e}");
        }
    }
}

async fn timeout<F: std::future::Future>(fut: F) -> Result<F::Output> {
    tokio::time::timeout(IO_TIMEOUT, fut)
        .await
        .map_err(|_| SyncError::Timeout)
}

/// Échange de messages Automerge sur un flux, jusqu'à ce que les deux côtés
/// n'aient plus rien à envoyer (un message vide = « rien de neuf »).
/// Renvoie `true` si le document local a changé.
async fn run_sync<S: Store>(
    store: &S,
    doc: &str,
    send: &mut SendStream,
    recv: &mut RecvStream,
    initiator: bool,
) -> Result<bool> {
    let mut state = SyncState::new();
    let mut changed = false;
    let mut last_sent_empty = false;
    if initiator {
        let msg = store.generate(doc, &mut state).map_err(SyncError::Store)?;
        last_sent_empty = msg.is_none();
        write_frame(send, msg.as_deref().unwrap_or_default()).await?;
    }
    for _ in 0..MAX_ROUNDS {
        let Some(frame) = read_frame(recv).await? else {
            // L'autre côté a terminé.
            finish_and_drain(send, recv).await;
            return Ok(changed);
        };
        if frame.is_empty() {
            if last_sent_empty {
                finish_and_drain(send, recv).await;
                return Ok(changed);
            }
        } else {
            changed |= store
                .receive(doc, &mut state, &frame)
                .map_err(SyncError::Store)?;
        }
        let msg = store.generate(doc, &mut state).map_err(SyncError::Store)?;
        last_sent_empty = msg.is_none();
        write_frame(send, msg.as_deref().unwrap_or_default()).await?;
    }
    Err(SyncError::Protocol(format!(
        "la synchro de « {doc} » ne converge pas"
    )))
}

/// Termine notre côté du flux puis attend que l'autre côté termine le sien :
/// ainsi aucun des deux ne ferme la connexion pendant que l'autre enregistre
/// encore le dernier message.
async fn finish_and_drain(send: &mut SendStream, recv: &mut RecvStream) {
    let _ = send.finish();
    for _ in 0..MAX_ROUNDS {
        match read_frame(recv).await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => return,
        }
    }
}

async fn write_frame(send: &mut SendStream, bytes: &[u8]) -> Result<()> {
    let len =
        u32::try_from(bytes.len()).map_err(|_| SyncError::Protocol("message trop gros".into()))?;
    timeout(send.write_all(&len.to_be_bytes()))
        .await?
        .map_err(net)?;
    timeout(send.write_all(bytes)).await?.map_err(net)?;
    Ok(())
}

/// Lit un message ; `None` si le flux est terminé proprement.
async fn read_frame(recv: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    read_frame_max(recv, MAX_FRAME).await
}

async fn read_frame_max(recv: &mut RecvStream, max: usize) -> Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match timeout(recv.read_exact(&mut len)).await? {
        Ok(()) => {}
        Err(iroh::endpoint::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(e) => return Err(net(e)),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(SyncError::Protocol("message trop gros".into()));
    }
    let mut buf = vec![0u8; len];
    timeout(recv.read_exact(&mut buf)).await?.map_err(net)?;
    Ok(Some(buf))
}

async fn write_json<T: Serialize>(send: &mut SendStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|e| SyncError::Protocol(e.to_string()))?;
    write_frame(send, &bytes).await
}

async fn read_json<T: for<'de> Deserialize<'de>>(recv: &mut RecvStream) -> Result<T> {
    // Les messages JSON (en-têtes, appairage) sont toujours petits.
    let frame = read_frame_max(recv, MAX_PAIR_FRAME)
        .await?
        .ok_or_else(|| SyncError::Protocol("flux fermé trop tôt".into()))?;
    serde_json::from_slice(&frame).map_err(|e| SyncError::Protocol(e.to_string()))
}

struct SyncHandler<S: Store>(Arc<Shared<S>>);

impl<S: Store> std::fmt::Debug for SyncHandler<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyncHandler")
    }
}

impl<S: Store> ProtocolHandler for SyncHandler<S> {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        let peer = conn.remote_id();
        if !self.0.store.peers().contains(&peer) {
            conn.close(1u32.into(), b"appareil non appaire");
            return Ok(());
        }
        // Il vient de nous joindre : inutile d'attendre pour le réessayer.
        self.0.absent().remove(&peer);
        let known_docs = self.0.store.doc_names();
        // Un flux par document, ouvert par l'appareil qui a lancé la synchro.
        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
            let header: DocHeader = match read_json(&mut recv).await {
                Ok(h) => h,
                Err(_) => break,
            };
            if !known_docs.contains(&header.doc) {
                // Document inconnu (autre version de l'appli) : on l'ignore.
                let _ = send.finish();
                let _ = recv.stop(0u32.into());
                continue;
            }
            match run_sync(&*self.0.store, &header.doc, &mut send, &mut recv, false).await {
                // Prévenir tout de suite : l'interface se rafraîchit sans attendre
                // la fin de la connexion.
                Ok(true) => self.0.store.changed(std::slice::from_ref(&header.doc)),
                Ok(false) => {}
                Err(e) => {
                    eprintln!(
                        "Synchro de « {} » avec {} interrompue : {e}",
                        header.doc,
                        peer.fmt_short()
                    );
                    break;
                }
            }
        }
        Ok(())
    }
}

struct PairHandler<S: Store>(Arc<Shared<S>>);

impl<S: Store> std::fmt::Debug for PairHandler<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairHandler")
    }
}

impl<S: Store> ProtocolHandler for PairHandler<S> {
    async fn accept(&self, conn: Connection) -> std::result::Result<(), AcceptError> {
        let Ok((mut send, mut recv)) = conn.accept_bi().await else {
            return Ok(());
        };
        let Ok(request) = read_json::<PairRequest>(&mut recv).await else {
            return Ok(());
        };
        let peer = conn.remote_id();
        // Le code est consommé dès la première tentative, bonne ou mauvaise :
        // impossible de deviner le secret par essais successifs.
        let valid = {
            let mut pending = self.0.pending();
            let ok = pending.as_ref().is_some_and(|p| {
                p.expires > Instant::now()
                    && constant_time_eq(p.token.as_bytes(), request.token.as_bytes())
            });
            *pending = None;
            ok
        };
        let refusal = if !valid {
            Some("code expiré ou déjà utilisé".to_owned())
        } else if request.addr.id != peer {
            Some("adresse de l'appareil incohérente".to_owned())
        } else {
            self.0.store.add_peer(peer, &request.name).err()
        };
        if let Some(reason) = &refusal {
            eprintln!("Appairage refusé pour {} : {reason}", peer.fmt_short());
        } else {
            self.0.addrs().insert(peer, request.addr);
        }
        self.0.store.pairing_used(refusal.is_none());
        let response = PairResponse {
            accepted: refusal.is_none(),
            name: self.0.store.device_name(),
            reason: refusal,
        };
        let _ = write_json(&mut send, &response).await;
        let _ = send.finish();
        // Laisse au pair le temps de lire la réponse, sans le laisser occuper la connexion.
        let _ = tokio::time::timeout(Duration::from_secs(5), conn.closed()).await;
        Ok(())
    }
}

/// Démarre la connexion au réseau et les protocoles acceptés.
async fn start_router<S: Store>(
    secret: &SecretKey,
    network: Network,
    shared: &Arc<Shared<S>>,
    blobs: &blobs::Blobs,
) -> Result<Router> {
    let builder = match network {
        Network::Online => Endpoint::builder(iroh::endpoint::presets::N0),
        Network::Local => Endpoint::builder(iroh::endpoint::presets::Minimal)
            .relay_mode(iroh::RelayMode::Disabled),
    };
    let endpoint = builder
        .secret_key(secret.clone())
        .bind()
        .await
        .map_err(net)?;
    let only_peers = blobs::only_peers(shared.clone());
    Ok(Router::builder(endpoint)
        .accept(SYNC_ALPN, SyncHandler(shared.clone()))
        .accept(PAIR_ALPN, PairHandler(shared.clone()))
        .accept(
            iroh_blobs::ALPN,
            blobs::KeepStore(iroh_blobs::BlobsProtocol::new(
                blobs.api(),
                Some(only_peers),
            )),
        )
        .spawn())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_doubles_up_to_the_cap() {
        let minutes = |n| retry_delay(n).as_secs() / 60;
        assert_eq!([1, 2, 3, 4, 5, 100].map(minutes), [2, 4, 8, 15, 15, 15]);
        assert_eq!(retry_delay(0), RETRY_MIN);
    }
}
