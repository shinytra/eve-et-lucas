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
}

impl MemStore {
    fn new(name: &str) -> Arc<Self> {
        Arc::new(Self {
            name: name.into(),
            id: Mutex::new(None),
            notes: Mutex::new(NotesDoc::new()),
            devices: Mutex::new(DevicesDoc::new()),
            changes: Mutex::new(0),
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
