//! Tests de bout en bout : de vrais nœuds iroh qui se parlent en local.

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use couple_core::{DevicesDoc, NotesDoc, SyncState, Syncable};
use couple_sync::{iroh::SecretKey, Network, Store, SyncError, SyncNode};
use iroh::EndpointId;

/// Stockage en mémoire, comme l'appli mais sans disque.
struct MemStore {
    name: String,
    id: Mutex<Option<EndpointId>>,
    notes: Mutex<NotesDoc>,
    devices: Mutex<DevicesDoc>,
    changes: Mutex<usize>,
    wanted: Mutex<Vec<String>>,
    received: Mutex<Vec<String>>,
}

impl MemStore {
    fn new(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            id: Mutex::new(None),
            notes: Mutex::new(NotesDoc::new()),
            devices: Mutex::new(DevicesDoc::new()),
            changes: Mutex::new(0),
            wanted: Mutex::new(Vec::new()),
            received: Mutex::new(Vec::new()),
        })
    }
    fn titles(&self) -> Vec<String> {
        let mut t: Vec<String> = self
            .notes
            .lock()
            .unwrap()
            .list()
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        t.sort();
        t
    }
    fn add(&self, title: &str) {
        self.notes
            .lock()
            .unwrap()
            .add(&self.name, title, "", 1)
            .unwrap();
    }
    fn with_doc<T>(&self, doc: &str, f: impl FnOnce(&mut dyn Syncable) -> T) -> T {
        match doc {
            "notes" => f(&mut *self.notes.lock().unwrap()),
            "devices" => f(&mut *self.devices.lock().unwrap()),
            _ => panic!("document inconnu {doc}"),
        }
    }
}

impl Store for MemStore {
    fn doc_names(&self) -> Vec<String> {
        vec!["devices".into(), "notes".into()]
    }
    fn generate(&self, doc: &str, state: &mut SyncState) -> Result<Option<Vec<u8>>, String> {
        Ok(self.with_doc(doc, |d| d.generate_sync_message(state)))
    }
    fn receive(&self, doc: &str, state: &mut SyncState, msg: &[u8]) -> Result<bool, String> {
        self.with_doc(doc, |d| d.receive_sync_message(state, msg))
            .map_err(|e| e.to_string())
    }
    fn peers(&self) -> Vec<EndpointId> {
        let me = *self.id.lock().unwrap();
        self.devices
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter_map(|d| d.id.parse().ok())
            .filter(|id| Some(*id) != me)
            .collect()
    }
    fn add_peer(&self, id: EndpointId, name: &str) -> Result<(), String> {
        self.devices
            .lock()
            .unwrap()
            .upsert(&id.to_string(), name, 2)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn device_name(&self) -> String {
        self.name.clone()
    }
    fn changed(&self, _docs: &[String]) {
        *self.changes.lock().unwrap() += 1;
    }
    fn wanted_blobs(&self) -> Vec<String> {
        self.wanted.lock().unwrap().clone()
    }
    fn blobs_received(&self, hashes: &[String]) {
        self.received.lock().unwrap().extend_from_slice(hashes);
    }
}

async fn node(name: &str) -> (Arc<MemStore>, SyncNode<MemStore>) {
    let store = MemStore::new(name);
    let node = SyncNode::spawn(SecretKey::generate(), store.clone(), Network::Local)
        .await
        .unwrap();
    *store.id.lock().unwrap() = Some(node.id());
    // Chaque appareil s'inscrit lui-même dans son document des appareils.
    store
        .devices
        .lock()
        .unwrap()
        .upsert(&node.id().to_string(), name, 1)
        .unwrap();
    (store, node)
}

const TTL: Duration = Duration::from_secs(60);

#[tokio::test]
async fn pairing_then_notes_sync_both_ways() {
    let (pc_store, pc) = node("PC").await;
    let (lucas_store, lucas) = node("Lucas").await;
    pc_store.add("Note du PC");
    lucas_store.add("Note de Lucas");

    let code = pc.start_pairing(TTL).unwrap().encode();
    let outcome = lucas.join(&code).await.unwrap();
    assert_eq!(outcome.host_name, "PC");
    let report = outcome.first_sync.unwrap();
    assert!(report.changed.contains(&"notes".to_string()));

    assert_eq!(pc_store.titles(), ["Note de Lucas", "Note du PC"]);
    assert_eq!(lucas_store.titles(), pc_store.titles());
    assert!(pc_store.peers().contains(&lucas.id()));
    assert!(lucas_store.peers().contains(&pc.id()));
    // L'hôte est prévenu du changement (son côté de la synchro finit juste après).
    for _ in 0..100 {
        if *pc_store.changes.lock().unwrap() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        *pc_store.changes.lock().unwrap() > 0,
        "l'hôte doit être prévenu du changement"
    );

    // Une nouvelle note sur le PC arrive chez Lucas, synchro lancée par le PC.
    pc_store.add("Après appairage");
    pc.sync_with(lucas.id()).await.unwrap();
    assert!(lucas_store
        .titles()
        .contains(&"Après appairage".to_string()));

    // Rien de neuf : la synchro ne change rien.
    assert!(pc.sync_with(lucas.id()).await.unwrap().changed.is_empty());
    pc.shutdown().await;
    lucas.shutdown().await;
}

#[tokio::test]
async fn phones_learn_each_other_and_sync_without_the_pc() {
    let (pc_store, pc) = node("PC").await;
    let (lucas_store, lucas) = node("Lucas").await;
    let (eve_store, eve) = node("Eve").await;

    lucas
        .join(&pc.start_pairing(TTL).unwrap().encode())
        .await
        .unwrap();
    eve.join(&pc.start_pairing(TTL).unwrap().encode())
        .await
        .unwrap();
    // Lucas apprend l'existence d'Eve par le PC.
    lucas.sync_with(pc.id()).await.unwrap();
    assert!(lucas_store.peers().contains(&eve.id()));
    assert!(eve_store.peers().contains(&lucas.id()));
    assert_eq!(pc_store.peers().len(), 2);

    // PC éteint : les deux téléphones se synchronisent directement.
    pc.shutdown().await;
    lucas.add_addr(eve.addr());
    eve_store.add("Écrite pendant que le PC dort");
    let results = lucas.sync_all().await;
    let ok: Vec<_> = results
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(ok, vec![eve.id()], "seule Eve est joignable : {results:?}");
    assert!(lucas_store
        .titles()
        .contains(&"Écrite pendant que le PC dort".to_string()));
    lucas.shutdown().await;
    eve.shutdown().await;
}

#[tokio::test]
async fn unpaired_device_cannot_sync() {
    let (pc_store, pc) = node("PC").await;
    let (_, intrus) = node("Intrus").await;
    pc_store.add("Secret");
    intrus.add_addr(pc.addr());
    let result = intrus.sync_with(pc.id()).await;
    assert!(result.is_err() || result.unwrap().changed.is_empty());
    assert!(pc_store.peers().is_empty());
    pc.shutdown().await;
    intrus.shutdown().await;
}

#[tokio::test]
async fn pairing_code_is_single_use_and_checked() {
    let (_, pc) = node("PC").await;
    let (_, lucas) = node("Lucas").await;
    let (_, intrus) = node("Intrus").await;

    // Code mal formé.
    assert!(matches!(
        lucas.join("n'importe quoi").await,
        Err(SyncError::InvalidCode)
    ));

    // Mauvais secret : refusé, et le vrai code est consommé.
    let mut code = pc.start_pairing(TTL).unwrap();
    let good = code.encode();
    code.token = "0".repeat(32);
    assert!(matches!(
        intrus.join(&code.encode()).await,
        Err(SyncError::PairingRefused(_))
    ));
    assert!(matches!(
        lucas.join(&good).await,
        Err(SyncError::PairingRefused(_))
    ));

    // Bon code : accepté une seule fois.
    let good = pc.start_pairing(TTL).unwrap().encode();
    lucas.join(&good).await.unwrap();
    assert!(matches!(
        intrus.join(&good).await,
        Err(SyncError::PairingRefused(_))
    ));

    // Code expiré.
    let expired = pc.start_pairing(Duration::from_millis(1)).unwrap().encode();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(matches!(
        intrus.join(&expired).await,
        Err(SyncError::PairingRefused(_))
    ));
    pc.shutdown().await;
    lucas.shutdown().await;
    intrus.shutdown().await;
}

#[tokio::test]
async fn wanted_files_are_downloaded_after_sync() {
    let (pc_store, pc) = node("PC").await;
    let (lucas_store, lucas) = node("Lucas").await;
    let code = pc.start_pairing(TTL).unwrap().encode();
    lucas.join(&code).await.unwrap().first_sync.unwrap();

    let content = b"photo de vacances".repeat(10_000);
    let hash = pc.add_blob(content.clone()).await.unwrap();
    assert_eq!(hash.len(), 64);
    assert!(pc.has_blob(&hash).await);
    assert!(!lucas.has_blob(&hash).await);

    // Sans demande, rien n'est téléchargé.
    assert!(lucas.fetch_wanted_from(pc.id()).await.is_empty());

    lucas_store.wanted.lock().unwrap().push(hash.clone());
    assert_eq!(lucas.fetch_wanted_from(pc.id()).await, vec![hash.clone()]);
    assert_eq!(*lucas_store.received.lock().unwrap(), vec![hash.clone()]);
    assert_eq!(lucas.read_blob(&hash).await.unwrap(), content);
    // Déjà là : pas de nouveau téléchargement.
    assert!(lucas.fetch_wanted_from(pc.id()).await.is_empty());

    // Fichier supprimé ailleurs : son contenu n'est plus gardé.
    let other = lucas.add_blob(b"autre".to_vec()).await.unwrap();
    assert_eq!(
        lucas
            .retain_blobs(std::slice::from_ref(&hash))
            .await
            .unwrap(),
        1
    );
    assert_eq!(lucas.retain_blobs(&[hash.clone(), other]).await.unwrap(), 0);
    drop(pc_store);
}

#[tokio::test]
async fn files_can_be_added_from_disk() {
    let (_store, pc) = node("PC").await;
    let dir = std::env::temp_dir().join(format!("couple-sync-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("envoi.bin");
    std::fs::write(&path, b"contenu sur le disque").unwrap();
    let from_disk = pc.add_blob_from_path(&path).await.unwrap();
    let from_memory = pc
        .add_blob(b"contenu sur le disque".to_vec())
        .await
        .unwrap();
    assert_eq!(from_disk, from_memory);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn files_can_be_fetched_on_demand_and_exported() {
    let (_pc_store, pc) = node("PC").await;
    let (_eve_store, eve) = node("Eve").await;
    let code = pc.start_pairing(TTL).unwrap().encode();
    eve.join(&code).await.unwrap().first_sync.unwrap();

    let hash = pc.add_blob(b"liste de courses".to_vec()).await.unwrap();
    eve.fetch_blob(&hash).await.unwrap();
    let dir = std::env::temp_dir().join(format!("couple-sync-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("courses.txt");
    eve.export_blob(&hash, &target).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"liste de courses");
    std::fs::remove_dir_all(dir).unwrap();

    assert!(eve.fetch_blob(&"0".repeat(64)).await.is_err());
    assert!(eve.fetch_blob("pas une empreinte").await.is_err());
}

#[tokio::test]
async fn unpaired_device_cannot_fetch_files() {
    let (_pc_store, pc) = node("PC").await;
    let (stranger_store, stranger) = node("Inconnu").await;
    let hash = pc.add_blob(b"secret".to_vec()).await.unwrap();
    // L'inconnu connaît le PC, mais le PC ne le connaît pas.
    stranger.add_addr(pc.addr());
    stranger_store
        .devices
        .lock()
        .unwrap()
        .upsert(&pc.id().to_string(), "PC", 1)
        .unwrap();
    assert!(stranger.fetch_blob(&hash).await.is_err());
    assert!(!stranger.has_blob(&hash).await);
}
