//! Appareils appairés du couple (PC, téléphone d'Eve, téléphone de Lucas).
//!
//! Ce document est synchronisé comme les autres : quand Eve appaire son
//! téléphone avec le PC, le téléphone de Lucas l'apprend à la synchro suivante
//! et peut ensuite se synchroniser directement avec elle, même PC éteint.
//! La clé de chaque entrée est l'identifiant iroh (clé publique) de l'appareil.
//!
//! Retirer un appareil laisse une marque `revoked:<id>` qui l'emporte toujours :
//! même si l'appareil retiré réécrit son entrée en même temps (par exemple un
//! téléphone volé qui se synchronise avec un appareil pas encore au courant),
//! il reste exclu partout dès que la marque arrive.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

const REVOKED_PREFIX: &str = "revoked:";
/// Longueur maximale d'un nom d'appareil (en caractères).
pub const MAX_NAME_CHARS: usize = 60;

/// Un appareil autorisé à se synchroniser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Device {
    /// Identifiant iroh de l'appareil (clé publique en texte).
    pub id: String,
    /// Nom choisi, par exemple « PC de Lucas » ou « Téléphone d'Eve ».
    pub name: String,
    /// Millisecondes depuis l'époque Unix.
    pub added_at: i64,
}

/// Le document des appareils appairés.
#[derive(Debug, Default, Clone)]
pub struct DevicesDoc {
    doc: AutoCommit,
}

impl DevicesDoc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            doc: AutoCommit::load(bytes)?,
        })
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Tous les appareils lisibles, triés par date d'ajout.
    pub fn list(&self) -> Vec<Device> {
        let mut devices: Vec<Device> = self
            .doc
            .keys(ROOT)
            .filter(|key| !key.starts_with(REVOKED_PREFIX) && !self.is_revoked(key))
            .filter_map(|key| {
                hydrate_prop::<_, Device, _, _>(&self.doc, ROOT, key.as_str())
                    .ok()
                    .filter(|d| d.id == key)
            })
            .collect();
        devices.sort_by(|a, b| a.added_at.cmp(&b.added_at).then(a.id.cmp(&b.id)));
        devices
    }

    pub fn contains(&self, id: &str) -> bool {
        self.list().iter().any(|d| d.id == id)
    }

    /// L'appareil a été retiré : il ne peut plus revenir, ni par la synchro ni
    /// par un nouvel appairage.
    pub fn is_revoked(&self, id: &str) -> bool {
        matches!(
            self.doc.get(ROOT, format!("{REVOKED_PREFIX}{id}")),
            Ok(Some(_))
        )
    }

    /// Ajoute un appareil, ou met à jour son nom s'il est déjà connu
    /// (la date d'ajout d'origine est gardée).
    pub fn upsert(&mut self, id: &str, name: &str, now_ms: i64) -> Result<Device> {
        let id = id.trim();
        let name = name.trim();
        if id.is_empty() {
            return Err(Error::InvalidDevice("identifiant vide".into()));
        }
        if name.is_empty() {
            return Err(Error::InvalidDevice("nom vide".into()));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(Error::InvalidDevice(format!(
                "nom trop long ({MAX_NAME_CHARS} caractères au plus)"
            )));
        }
        if id.starts_with(REVOKED_PREFIX) || self.is_revoked(id) {
            return Err(Error::InvalidDevice("cet appareil a été retiré".into()));
        }
        let added_at = self
            .list()
            .into_iter()
            .find(|d| d.id == id)
            .map_or(now_ms, |d| d.added_at);
        let device = Device {
            id: id.to_owned(),
            name: name.to_owned(),
            added_at,
        };
        reconcile_prop(&mut self.doc, ROOT, id, &device)?;
        Ok(device)
    }

    /// Retire un appareil, définitivement (il ne pourra plus se synchroniser).
    pub fn remove(&mut self, id: &str, now_ms: i64) -> Result<()> {
        if !self.contains(id) {
            return Err(Error::InvalidDevice(format!("appareil {id} inconnu")));
        }
        self.doc.delete(ROOT, id)?;
        self.doc
            .put(ROOT, format!("{REVOKED_PREFIX}{id}"), now_ms)?;
        Ok(())
    }

    pub fn fork(&mut self) -> Self {
        Self {
            doc: self.doc.fork(),
        }
    }
}

impl Syncable for DevicesDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    #[test]
    fn upsert_keeps_first_date_and_rejects_empty() {
        let mut doc = DevicesDoc::new();
        doc.upsert("pc", "PC de Lucas", 1).unwrap();
        let renamed = doc.upsert("pc", "PC du salon", 5).unwrap();
        assert_eq!(renamed.added_at, 1);
        assert_eq!(doc.list(), vec![renamed]);
        assert!(doc.upsert("", "x", 1).is_err());
        assert!(doc.upsert("x", " ", 1).is_err());
        assert!(doc.contains("pc"));
        assert!(doc.upsert("x", &"a".repeat(61), 1).is_err());
        doc.remove("pc", 9).unwrap();
        assert!(!doc.contains("pc"));
        assert!(doc.remove("pc", 9).is_err());
        assert!(
            doc.upsert("pc", "PC", 10).is_err(),
            "un appareil retiré ne revient pas"
        );
        assert!(doc.list().is_empty());
    }

    #[test]
    fn removed_device_cannot_come_back_through_a_concurrent_write() {
        let mut pc = DevicesDoc::new();
        pc.upsert("vole", "Téléphone volé", 1).unwrap();
        pc.upsert("eve", "Eve", 1).unwrap();
        let mut eve = pc.fork();
        let mut stolen = pc.fork();

        // Le PC retire le téléphone volé ; en même temps, celui-ci réécrit son entrée
        // et la fait passer à Eve, qui n'a pas encore reçu le retrait.
        pc.remove("vole", 2).unwrap();
        stolen.upsert("vole", "Je suis toujours là", 3).unwrap();
        sync_pair(&mut stolen, &mut eve);
        assert!(eve.contains("vole"));

        sync_pair(&mut pc, &mut eve);
        assert!(!eve.contains("vole"));
        assert!(!pc.contains("vole"));
        assert!(eve.is_revoked("vole"));
    }

    #[test]
    fn third_device_is_learned_through_the_pc() {
        // Le PC appaire Lucas puis Eve ; Lucas apprend Eve via le PC.
        let mut pc = DevicesDoc::new();
        pc.upsert("pc", "PC", 1).unwrap();
        let mut lucas = DevicesDoc::new();
        lucas.upsert("lucas", "Téléphone de Lucas", 1).unwrap();
        pc.upsert("lucas", "Téléphone de Lucas", 2).unwrap();
        sync_pair(&mut pc, &mut lucas);

        let mut eve = DevicesDoc::new();
        eve.upsert("eve", "Téléphone d'Eve", 3).unwrap();
        pc.upsert("eve", "Téléphone d'Eve", 3).unwrap();
        sync_pair(&mut pc, &mut eve);
        sync_pair(&mut pc, &mut lucas);

        assert!(lucas.contains("eve"));
        assert_eq!(lucas.list(), eve.list());
        assert_eq!(lucas.list().len(), 3);
    }
}
