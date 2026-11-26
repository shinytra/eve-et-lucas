//! Arrivées à la maison, stockées dans un document Automerge.
//!
//! Quand un téléphone entre dans la zone « maison » (géofencing Android), il
//! ajoute une arrivée. La synchro la transmet au PC et à l'autre téléphone,
//! qui sonnent. Racine : table `id -> Arrival` (UUID v4), comme les notes ;
//! les arrivées de plus de 30 jours sont supprimées pour garder un petit
//! document.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Durée de conservation d'une arrivée.
pub const KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// Une arrivée plus ancienne que ça n'a plus de raison de faire sonner
/// (téléphone resté hors ligne longtemps, appli rouverte le lendemain…).
pub const RING_WITHIN_MS: i64 = 15 * 60 * 1000;

/// Une arrivée à la maison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Arrival {
    pub id: String,
    /// Prénom de la personne qui arrive.
    pub who: String,
    /// Identifiant (clé iroh) de l'appareil qui l'a détectée.
    pub device: String,
    /// Moment de l'entrée dans la zone (millisecondes Unix).
    pub at: i64,
}

/// Le document des arrivées. `Clone` garde l'identifiant d'appareil :
/// utiliser [`ArrivalsDoc::fork`] pour simuler un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct ArrivalsDoc {
    doc: AutoCommit,
}

impl ArrivalsDoc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            doc: AutoCommit::load(bytes)?,
        })
    }

    pub fn fork(&mut self) -> Self {
        Self {
            doc: self.doc.fork(),
        }
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Toutes les arrivées lisibles, la plus récente en premier.
    pub fn list(&self) -> Vec<Arrival> {
        let mut list: Vec<Arrival> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| {
                hydrate_prop::<_, Arrival, _, _>(&self.doc, ROOT, id.as_str())
                    .ok()
                    .filter(|a| a.id == id && !a.who.trim().is_empty())
            })
            .collect();
        list.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
        list
    }

    /// Enregistre une arrivée. Une deuxième arrivée du même appareil moins
    /// de 5 minutes après la précédente est ignorée (le GPS qui hésite au
    /// bord de la zone ne doit pas faire sonner deux fois) : renvoie `None`.
    pub fn add(&mut self, who: &str, device: &str, at: i64) -> Result<Option<Arrival>> {
        const DEBOUNCE_MS: i64 = 5 * 60 * 1000;
        let who = who.trim();
        if who.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let duplicate = self
            .list()
            .iter()
            .any(|a| a.device == device && (a.at - at).abs() < DEBOUNCE_MS);
        if duplicate {
            return Ok(None);
        }
        let arrival = Arrival {
            id: uuid::Uuid::new_v4().to_string(),
            who: who.to_owned(),
            device: device.to_owned(),
            at,
        };
        reconcile_prop(&mut self.doc, ROOT, arrival.id.as_str(), &arrival)?;
        Ok(Some(arrival))
    }

    /// Supprime les arrivées de plus de [`KEEP_MS`] ; renvoie leur nombre.
    pub fn prune(&mut self, now_ms: i64) -> Result<usize> {
        let old: Vec<String> = self
            .list()
            .into_iter()
            .filter(|a| now_ms - a.at > KEEP_MS)
            .map(|a| a.id)
            .collect();
        for id in &old {
            self.doc.delete(ROOT, id.as_str())?;
        }
        Ok(old.len())
    }

    /// Arrivées qui doivent faire sonner cet appareil : venues d'un autre
    /// appareil, récentes, et absentes de `rung` (identifiants déjà signalés
    /// ici), de la plus ancienne à la plus récente. On garde des identifiants
    /// plutôt qu'une date : une arrivée reçue en retard par la synchro, mais
    /// plus ancienne qu'une autre déjà signalée, sonne quand même.
    pub fn to_ring(&self, self_device: &str, rung: &[String], now_ms: i64) -> Vec<Arrival> {
        let mut list: Vec<Arrival> = self
            .list()
            .into_iter()
            .filter(|a| {
                a.device != self_device
                    && !rung.contains(&a.id)
                    && now_ms - a.at <= RING_WITHIN_MS
                    && a.at <= now_ms + 60_000
            })
            .collect();
        list.reverse();
        list
    }

    pub fn merge(&mut self, other: &mut ArrivalsDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for ArrivalsDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    const MIN: i64 = 60 * 1000;

    #[test]
    fn add_list_and_debounce() {
        let mut doc = ArrivalsDoc::new();
        let a = doc.add("Eve", "tel-eve", 10 * MIN).unwrap().unwrap();
        assert_eq!(a.who, "Eve");
        // Le GPS hésite : même appareil, 2 minutes plus tard.
        assert!(doc.add("Eve", "tel-eve", 12 * MIN).unwrap().is_none());
        // Autre appareil au même moment : gardé.
        let b = doc.add("Lucas", "tel-lucas", 12 * MIN).unwrap().unwrap();
        // Même appareil, bien plus tard : gardé.
        let c = doc.add("Eve", "tel-eve", 60 * MIN).unwrap().unwrap();
        assert_eq!(doc.list(), vec![c, b, a]);
        assert!(matches!(doc.add(" ", "x", 1), Err(Error::MissingAuthor)));
    }

    #[test]
    fn rings_only_for_recent_arrivals_from_others() {
        let mut doc = ArrivalsDoc::new();
        let now = 1_000 * MIN;
        let old = doc.add("Eve", "tel-eve", now - 20 * MIN).unwrap().unwrap();
        let recent = doc.add("Eve", "tel-eve", now - 2 * MIN).unwrap().unwrap();
        let mine = doc.add("Lucas", "pc", now - MIN).unwrap().unwrap();
        let ids = |v: Vec<Arrival>| v.into_iter().map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(ids(doc.to_ring("pc", &[], now)), vec![recent.id.clone()]);
        // Déjà signalée : plus rien.
        assert!(doc.to_ring("pc", std::slice::from_ref(&recent.id), now).is_empty());
        // L'autre téléphone, lui, sonne pour l'arrivée du PC… et pas pour la sienne.
        assert_eq!(ids(doc.to_ring("tel-eve", &[], now)), vec![mine.id]);
        assert!(!ids(doc.to_ring("pc", &[], now)).contains(&old.id));
    }

    #[test]
    fn late_synced_arrival_still_rings() {
        let now = 1_000 * MIN;
        let mut pc = ArrivalsDoc::new();
        let mut tel_eve = pc.fork();
        let mut tel_lucas = pc.fork();
        let eve = tel_eve
            .add("Eve", "tel-eve", now - 5 * MIN)
            .unwrap()
            .unwrap();
        let lucas = tel_lucas
            .add("Lucas", "tel-lucas", now - 2 * MIN)
            .unwrap()
            .unwrap();
        // Le PC reçoit d'abord l'arrivée de Lucas (la plus récente) et sonne.
        pc.merge(&mut tel_lucas).unwrap();
        let ids = |v: Vec<Arrival>| v.into_iter().map(|a| a.id).collect::<Vec<_>>();
        let rung = ids(pc.to_ring("pc", &[], now));
        assert_eq!(rung, vec![lucas.id.clone()]);
        // Celle d'Eve, plus ancienne, arrive ensuite : elle sonne aussi.
        pc.merge(&mut tel_eve).unwrap();
        assert_eq!(ids(pc.to_ring("pc", &rung, now)), vec![eve.id]);
    }

    #[test]
    fn prune_removes_old_arrivals() {
        let mut doc = ArrivalsDoc::new();
        doc.add("Eve", "tel-eve", 0).unwrap();
        let kept = doc.add("Eve", "tel-eve", KEEP_MS).unwrap().unwrap();
        assert_eq!(doc.prune(KEEP_MS + 1).unwrap(), 1);
        assert_eq!(doc.list(), vec![kept]);
    }

    #[test]
    fn arrivals_merge_and_sync() {
        let mut pc = ArrivalsDoc::new();
        let mut phone = pc.fork();
        phone.add("Eve", "tel-eve", 5 * MIN).unwrap();
        pc.add("Lucas", "tel-lucas", 6 * MIN).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);

        let mut other = ArrivalsDoc::new();
        other.add("Eve", "tel-eve", 50 * MIN).unwrap();
        sync_pair(&mut pc, &mut other);
        assert_eq!(pc.list(), other.list());
        assert_eq!(pc.list().len(), 3);
        let reloaded = ArrivalsDoc::load(&pc.save()).unwrap();
        assert_eq!(reloaded.list(), pc.list());
    }
}
