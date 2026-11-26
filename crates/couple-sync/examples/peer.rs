//! Faux téléphone pour tester l'appli PC à la main ou par script :
//! rejoint l'appareil qui affiche le code, ajoute une note, synchronise, puis
//! affiche les notes reçues.
//!
//! Usage : `cargo run -p couple-sync --example peer -- "EL1:…" "Titre de la note"`

use std::sync::{Arc, Mutex};

use couple_core::{DevicesDoc, NotesDoc, SyncState, Syncable};
use couple_sync::{iroh::SecretKey, Network, Store, SyncNode};
use iroh::EndpointId;

struct Phone {
    id: EndpointId,
    notes: Mutex<NotesDoc>,
    devices: Mutex<DevicesDoc>,
}

impl Phone {
    fn with<T>(&self, doc: &str, f: impl FnOnce(&mut dyn Syncable) -> T) -> Result<T, String> {
        match doc {
            "notes" => Ok(f(&mut *self.notes.lock().unwrap())),
            "devices" => Ok(f(&mut *self.devices.lock().unwrap())),
            _ => Err(format!("document inconnu {doc}")),
        }
    }
}

impl Store for Phone {
    fn doc_names(&self) -> Vec<String> {
        vec!["devices".into(), "notes".into()]
    }
    fn generate(&self, doc: &str, state: &mut SyncState) -> Result<Option<Vec<u8>>, String> {
        self.with(doc, |d| d.generate_sync_message(state))
    }
    fn receive(&self, doc: &str, state: &mut SyncState, msg: &[u8]) -> Result<bool, String> {
        self.with(doc, |d| d.receive_sync_message(state, msg))?
            .map_err(|e| e.to_string())
    }
    fn peers(&self) -> Vec<EndpointId> {
        self.devices
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter_map(|d| d.id.parse().ok())
            .filter(|id| *id != self.id)
            .collect()
    }
    fn add_peer(&self, id: EndpointId, name: &str) -> Result<(), String> {
        self.devices
            .lock()
            .unwrap()
            .upsert(&id.to_string(), name, 0)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn device_name(&self) -> String {
        "Téléphone de test".into()
    }
    fn changed(&self, docs: &[String]) {
        println!("documents changés : {docs:?}");
    }
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let code = args.next().expect("code d'appairage attendu");
    let title = args
        .next()
        .unwrap_or_else(|| "Note du téléphone de test".into());
    let secret = SecretKey::generate();
    let phone = Arc::new(Phone {
        id: secret.public(),
        notes: Mutex::new(NotesDoc::new()),
        devices: Mutex::new(DevicesDoc::new()),
    });
    phone
        .devices
        .lock()
        .unwrap()
        .upsert(&phone.id.to_string(), "Téléphone de test", 0)
        .unwrap();
    phone
        .notes
        .lock()
        .unwrap()
        .add("Eve", &title, "envoyée par le pair de test", 0)
        .unwrap();

    let node = SyncNode::spawn(secret, phone.clone(), Network::Local)
        .await
        .expect("démarrage");
    let outcome = match node.join(&code).await {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("appairage réussi avec {}", outcome.host_name);
    println!("première synchro : {:?}", outcome.first_sync);
    for note in phone.notes.lock().unwrap().list().unwrap() {
        println!("note : {} ({})", note.title, note.author);
    }
    for device in phone.devices.lock().unwrap().list() {
        println!("appareil : {}", device.name);
    }
    node.shutdown().await;
}
