//! Synchro entre appareils, indépendante du réseau.
//!
//! Chaque document partagé sait produire et appliquer des messages du
//! protocole de synchro d'Automerge. Le transport (iroh) est dans le crate
//! `couple-sync` : ici on ne manipule que des octets.

use automerge::{
    sync::{Message, SyncDoc},
    AutoCommit,
};

use crate::{Error, Result};

/// État de synchro avec un pair donné (à garder pendant une session de synchro).
pub use automerge::sync::State as SyncState;

/// Un document qui peut se synchroniser avec la même copie sur un autre appareil.
pub trait Syncable {
    #[doc(hidden)]
    fn automerge(&mut self) -> &mut AutoCommit;

    /// Prochain message à envoyer à ce pair, ou `None` s'il n'y a plus rien à dire.
    fn generate_sync_message(&mut self, state: &mut SyncState) -> Option<Vec<u8>> {
        self.automerge()
            .sync()
            .generate_sync_message(state)
            .map(Message::encode)
    }

    /// Applique un message reçu. Renvoie `true` si le document a changé.
    fn receive_sync_message(&mut self, state: &mut SyncState, message: &[u8]) -> Result<bool> {
        let message = Message::decode(message).map_err(|e| Error::SyncMessage(e.to_string()))?;
        let doc = self.automerge();
        let before = doc.get_heads();
        doc.sync().receive_sync_message(state, message)?;
        Ok(doc.get_heads() != before)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Fait converger deux documents en mémoire, comme le ferait le réseau.
    pub(crate) fn sync_pair<D: Syncable>(a: &mut D, b: &mut D) {
        let (mut sa, mut sb) = (SyncState::new(), SyncState::new());
        for _ in 0..20 {
            let ma = a.generate_sync_message(&mut sa);
            let mb = b.generate_sync_message(&mut sb);
            if ma.is_none() && mb.is_none() {
                return;
            }
            if let Some(m) = ma {
                b.receive_sync_message(&mut sb, &m).unwrap();
            }
            if let Some(m) = mb {
                a.receive_sync_message(&mut sa, &m).unwrap();
            }
        }
        panic!("la synchro ne converge pas");
    }
}
