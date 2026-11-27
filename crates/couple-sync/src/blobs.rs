//! Contenu des fichiers partagés (iroh-blobs) : stockage, accès réservé aux
//! appareils appairés, téléchargement.

use std::{path::Path, str::FromStr, sync::Arc, time::Duration};

use iroh::EndpointAddr;
use iroh_blobs::{
    provider::events::{
        AbortReason, ConnectMode, EventMask, EventSender, ProviderMessage, RequestMode,
    },
    store::{
        fs::{options::Options, FsStore},
        mem::MemStore,
        GcConfig,
    },
    Hash,
};

use crate::{net, Result, Shared, Store, SyncError, SyncNode};

/// Délai maximal pour télécharger un fichier (jusqu'à 500 Mo par un relais).
const FETCH_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Fréquence du ménage des contenus qui ne sont plus partagés.
const GC_EVERY: Duration = Duration::from_secs(10 * 60);
/// Préfixe des étiquettes qui protègent un contenu du ménage.
const TAG_PREFIX: &str = "fichier-";

/// Où est rangé le contenu des fichiers.
#[derive(Debug, Clone)]
pub(crate) enum Blobs {
    Memory(MemStore),
    Disk(FsStore),
}

impl Blobs {
    pub(crate) fn memory() -> Self {
        Self::Memory(MemStore::new())
    }

    pub(crate) async fn on_disk(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| SyncError::Store(e.to_string()))?;
        let mut options = Options::new(dir);
        options.gc = Some(GcConfig {
            interval: GC_EVERY,
            add_protected: None,
        });
        let store = FsStore::load_with_opts(dir.join("blobs.db"), options)
            .await
            .map_err(|e| SyncError::Store(format!("fichiers partagés illisibles : {e}")))?;
        Ok(Self::Disk(store))
    }

    pub(crate) fn api(&self) -> &iroh_blobs::api::Store {
        match self {
            Self::Memory(s) => s,
            Self::Disk(s) => s,
        }
    }
}

fn parse(hash: &str) -> Result<Hash> {
    Hash::from_str(hash).map_err(|_| SyncError::Protocol(format!("empreinte invalide : {hash}")))
}

fn store_err(e: impl std::fmt::Display) -> SyncError {
    SyncError::Store(e.to_string())
}

/// N'accepte les demandes de fichiers que des appareils appairés (la liste
/// est relue à chaque connexion : un appareil retiré perd l'accès aussitôt).
/// Personne ne peut écrire dans notre stockage (« push » désactivé).
pub(crate) fn only_peers<S: Store>(shared: Arc<Shared<S>>) -> EventSender {
    let mask = EventMask {
        connected: ConnectMode::Intercept,
        push: RequestMode::Disabled,
        ..EventMask::DEFAULT
    };
    let (tx, mut rx) = EventSender::channel(32, mask);
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let ProviderMessage::ClientConnected(msg) = msg {
                let allowed = msg
                    .endpoint_id
                    .is_some_and(|id| shared.store.peers().contains(&id));
                let answer = if allowed {
                    Ok(())
                } else {
                    Err(AbortReason::Permission)
                };
                msg.tx.send(answer).await.ok();
            }
        }
    });
    tx
}

/// Protocole des fichiers dont l'arrêt (pause du nœud) ne ferme pas le
/// stockage : il sert encore pendant la pause et après la reprise. Il est
/// fermé par [`SyncNode::shutdown`].
#[derive(Debug, Clone)]
pub(crate) struct KeepStore(pub(crate) iroh_blobs::BlobsProtocol);

impl iroh::protocol::ProtocolHandler for KeepStore {
    async fn accept(
        &self,
        conn: iroh::endpoint::Connection,
    ) -> std::result::Result<(), iroh::protocol::AcceptError> {
        self.0.accept(conn).await
    }
}

impl<S: Store> SyncNode<S> {
    /// Range un contenu et renvoie son empreinte (hexadécimal). Il est
    /// gardé jusqu'à [`SyncNode::forget_blob`] ou [`SyncNode::retain_blobs`].
    pub async fn add_blob(&self, bytes: Vec<u8>) -> Result<String> {
        let tag = self
            .blobs
            .api()
            .add_bytes(bytes)
            .temp_tag()
            .await
            .map_err(store_err)?;
        let hash = tag.hash();
        self.protect(hash).await?;
        Ok(hash.to_hex())
    }

    /// Comme [`SyncNode::add_blob`], à partir d'un fichier (chemin absolu),
    /// sans le charger en mémoire.
    pub async fn add_blob_from_path(&self, path: &Path) -> Result<String> {
        let tag = self
            .blobs
            .api()
            .add_path(path)
            .temp_tag()
            .await
            .map_err(store_err)?;
        let hash = tag.hash();
        self.protect(hash).await?;
        Ok(hash.to_hex())
    }

    /// Ne garde que les contenus de `keep` (empreintes encore partagées) : les
    /// autres seront effacés au prochain ménage. Sert quand un fichier a été
    /// supprimé sur un autre appareil.
    pub async fn retain_blobs(&self, keep: &[String]) -> Result<usize> {
        use iroh_blobs::api::tags::DeleteOptions;
        let mut stale = Vec::new();
        let mut tags = self
            .blobs
            .api()
            .tags()
            .list_prefix(TAG_PREFIX)
            .await
            .map_err(store_err)?;
        while let Some(tag) = n0_future::StreamExt::next(&mut tags).await {
            let tag = tag.map_err(store_err)?;
            if !keep.contains(&tag.hash.to_hex()) {
                stale.push(tag.name);
            }
        }
        for name in &stale {
            self.blobs
                .api()
                .tags()
                .delete_with_opts(DeleteOptions::single(name.as_ref()))
                .await
                .map_err(store_err)?;
        }
        Ok(stale.len())
    }

    async fn protect(&self, hash: Hash) -> Result<()> {
        self.blobs
            .api()
            .tags()
            .set(format!("{TAG_PREFIX}{}", hash.to_hex()), hash)
            .await
            .map_err(store_err)
    }

    /// Le contenu est-il complet sur cet appareil ?
    pub async fn has_blob(&self, hash: &str) -> bool {
        match parse(hash) {
            Ok(h) => self.blobs.api().has(h).await.unwrap_or(false),
            Err(_) => false,
        }
    }

    /// Lit tout le contenu (pour un aperçu ou un enregistrement).
    pub async fn read_blob(&self, hash: &str) -> Result<Vec<u8>> {
        let bytes = self
            .blobs
            .api()
            .get_bytes(parse(hash)?)
            .await
            .map_err(store_err)?;
        Ok(bytes.to_vec())
    }

    /// Copie le contenu dans un fichier (chemin absolu).
    pub async fn export_blob(&self, hash: &str, target: &Path) -> Result<()> {
        self.blobs
            .api()
            .export(parse(hash)?, target)
            .finish()
            .await
            .map_err(store_err)?;
        Ok(())
    }

    /// Le contenu n'est plus partagé : il sera effacé au prochain ménage.
    pub async fn forget_blob(&self, hash: &str) -> Result<()> {
        let hash = parse(hash)?;
        self.blobs
            .api()
            .tags()
            .delete(format!("{TAG_PREFIX}{}", hash.to_hex()))
            .await
            .map_err(store_err)?;
        Ok(())
    }

    /// Télécharge un contenu depuis le premier appareil appairé qui l'a.
    pub async fn fetch_blob(&self, hash: &str) -> Result<()> {
        let parsed = parse(hash)?;
        if self.blobs.api().has(parsed).await.unwrap_or(false) {
            return self.protect(parsed).await;
        }
        let mut last = SyncError::Protocol("aucun appareil appairé".to_owned());
        for peer in self.shared.store.peers() {
            match self.fetch_from(peer, parsed).await {
                Ok(()) => {
                    self.shared.store.blobs_received(&[hash.to_owned()]);
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    async fn fetch_from(&self, peer: iroh::EndpointId, hash: Hash) -> Result<()> {
        let endpoint = self.endpoint()?;
        // Étiqueté avant de commencer : le ménage ne touche pas à un
        // téléchargement en cours, et une partie reçue sert à reprendre.
        self.protect(hash).await?;
        let addr = self
            .shared
            .addrs()
            .get(&peer)
            .cloned()
            .unwrap_or_else(|| EndpointAddr::new(peer));
        let conn = crate::connect(&endpoint, addr, iroh_blobs::ALPN).await?;
        let result = tokio::time::timeout(
            FETCH_TIMEOUT,
            self.blobs.api().remote().fetch(conn.clone(), hash),
        )
        .await
        .map_err(|_| SyncError::Timeout)?
        .map_err(net);
        conn.close(0u32.into(), b"fini");
        result?;
        if !self.blobs.api().has(hash).await.unwrap_or(false) {
            return Err(SyncError::Protocol("fichier incomplet".to_owned()));
        }
        Ok(())
    }

    /// Télécharge chez `peer` les fichiers voulus qui manquent (à appeler
    /// après une synchro). Un échec n'empêche pas les autres (réessayé plus
    /// tard). Renvoie les empreintes téléchargées.
    ///
    /// Un appareil qui n'a pas répondu à la dernière synchro est sauté : chaque
    /// fichier manquant coûterait sinon une tentative de connexion (et un
    /// réveil de la radio) à chaque passage.
    pub async fn fetch_wanted_from(&self, peer: iroh::EndpointId) -> Vec<String> {
        let mut downloaded = Vec::new();
        if !self.is_reachable(peer) || self.is_paused() {
            return downloaded;
        }
        for hash in self.shared.store.wanted_blobs() {
            let Ok(parsed) = parse(&hash) else { continue };
            if self.blobs.api().has(parsed).await.unwrap_or(false) {
                // Présent mais peut-être pas étiqueté (ménage) : on le protège.
                if let Err(e) = self.protect(parsed).await {
                    eprintln!("Fichier {} non protégé : {e}", parsed.fmt_short());
                }
                continue;
            }
            match self.fetch_from(peer, parsed).await {
                Ok(()) => downloaded.push(hash),
                Err(e) => {
                    eprintln!(
                        "Fichier {} non téléchargé depuis {peer} : {e}",
                        parsed.fmt_short()
                    );
                    // Appareil parti entre-temps : les autres fichiers attendront.
                    if matches!(e, SyncError::Unreachable(_) | SyncError::Paused) {
                        break;
                    }
                }
            }
        }
        if !downloaded.is_empty() {
            self.shared.store.blobs_received(&downloaded);
        }
        downloaded
    }
}
