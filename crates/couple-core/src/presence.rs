//! Où est chacun : dans l'un de ses lieux enregistrés (maison, université,
//! travail…) ou ailleurs.
//!
//! Les lieux eux-mêmes restent sur le téléphone de la personne (zones de
//! géorepérage d'Android, avec un nom). À chaque entrée ou sortie, le
//! téléphone ajoute ici un évènement ; la synchro (et le réveil ntfy) le
//! transmet aux autres. Racine : table `id -> PresenceEvent` (UUID v4) :
//! deux appareils qui ajoutent chacun un évènement ne se gênent jamais, et
//! l'état d'une personne est simplement son évènement le plus récent.

use std::collections::BTreeMap;

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Types de lieux, avec leur émoji.
pub const PLACE_KINDS: &[(&str, &str)] = &[
    ("maison", "🏠"),
    ("etudes", "🎓"),
    ("travail", "💼"),
    ("autre", "📍"),
];
/// Longueur maximale du nom d'un lieu.
pub const MAX_PLACE_NAME: usize = 40;
/// Les évènements plus anciens sont oubliés, sauf le dernier de chacun.
pub const KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;
/// Un évènement daté plus loin que ça dans le futur (horloge déréglée) est
/// ignoré : il bloquerait les suivants.
pub const FUTURE_TOLERANCE_MS: i64 = 10 * 60 * 1000;

/// Une entrée dans un lieu ou une sortie de tous ses lieux.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct PresenceEvent {
    pub id: String,
    /// Prénom de la personne.
    pub who: String,
    /// Identifiant (clé iroh) du téléphone qui l'a détecté.
    pub device: String,
    /// Moment de l'entrée ou de la sortie (millisecondes Unix).
    pub at: i64,
    /// Dans l'un de ses lieux (`place`) ; sinon ailleurs, et `place` est
    /// le lieu qu'elle vient de quitter.
    pub inside: bool,
    /// Nom du lieu, choisi par la personne (« Maison », « Université »…).
    /// Vide pour une arrivée d'avant les lieux nommés.
    pub place: String,
    /// Type du lieu (voir [`PLACE_KINDS`]).
    pub place_kind: String,
}

impl PresenceEvent {
    /// Émoji du type de lieu.
    pub fn emoji(&self) -> &'static str {
        place_emoji(&self.place_kind)
    }
}

/// Émoji d'un type de lieu (📍 pour un type inconnu).
pub fn place_emoji(kind: &str) -> &'static str {
    PLACE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("📍", |(_, e)| e)
}

/// Type connu, sinon `maison` s'il est vide (anciennes arrivées) ou `autre`.
pub fn clean_kind(kind: &str) -> &'static str {
    let kind = kind.trim();
    if kind.is_empty() {
        return "maison";
    }
    PLACE_KINDS
        .iter()
        .find(|(k, _)| *k == kind)
        .map_or("autre", |(k, _)| k)
}

/// Nom de lieu sur une ligne, sans caractère de contrôle, coupé à
/// [`MAX_PLACE_NAME`] caractères.
pub fn clean_place_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_PLACE_NAME)
        .collect()
}

/// Même personne, quelle que soit la casse ou les espaces.
fn person_key(who: &str) -> String {
    who.trim().to_lowercase()
}

/// Le document des présences. `Clone` garde l'identifiant d'appareil :
/// utiliser [`PresenceDoc::fork`] pour simuler un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct PresenceDoc {
    doc: AutoCommit,
}

impl PresenceDoc {
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

    /// Tous les évènements lisibles, le plus récent en premier.
    pub fn list(&self) -> Vec<PresenceEvent> {
        let mut list: Vec<PresenceEvent> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| {
                hydrate_prop::<_, PresenceEvent, _, _>(&self.doc, ROOT, id.as_str())
                    .ok()
                    .filter(|e| e.id == id && !e.who.trim().is_empty())
            })
            .collect();
        list.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.id.cmp(&a.id)));
        list
    }

    /// Ajoute un évènement (identifiant choisi par Android, le même dans le
    /// réveil ntfy et dans la synchro). Déjà connu : renvoie `Ok(false)`.
    pub fn record(&mut self, event: &PresenceEvent) -> Result<bool> {
        let who = event.who.trim();
        if who.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let id = event.id.as_str();
        if id.is_empty() || id.chars().count() > 64 || self.doc.get(ROOT, id)?.is_some() {
            return Ok(false);
        }
        let clean = PresenceEvent {
            id: id.to_owned(),
            who: who.to_owned(),
            device: event.device.clone(),
            at: event.at,
            inside: event.inside,
            place: clean_place_name(&event.place),
            place_kind: clean_kind(&event.place_kind).to_owned(),
        };
        reconcile_prop(&mut self.doc, ROOT, id, &clean)?;
        Ok(true)
    }

    /// L'état de chacun : son évènement le plus récent (hors évènements
    /// datés dans le futur), trié par prénom.
    pub fn current(&self, now_ms: i64) -> Vec<PresenceEvent> {
        let mut latest: BTreeMap<String, PresenceEvent> = BTreeMap::new();
        // `list` est du plus récent au plus ancien : le premier vu gagne.
        for e in self.list() {
            if e.at > now_ms + FUTURE_TOLERANCE_MS {
                continue;
            }
            latest.entry(person_key(&e.who)).or_insert(e);
        }
        latest.into_values().collect()
    }

    /// Oublie les évènements de plus de [`KEEP_MS`], sauf le dernier de
    /// chaque personne ; renvoie leur nombre.
    pub fn prune(&mut self, now_ms: i64) -> Result<usize> {
        let keep: Vec<String> = self.current(now_ms).into_iter().map(|e| e.id).collect();
        let old: Vec<String> = self
            .list()
            .into_iter()
            .filter(|e| now_ms - e.at > KEEP_MS && !keep.contains(&e.id))
            .map(|e| e.id)
            .collect();
        for id in &old {
            self.doc.delete(ROOT, id.as_str())?;
        }
        Ok(old.len())
    }

    pub fn merge(&mut self, other: &mut PresenceDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for PresenceDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    const MIN: i64 = 60 * 1000;

    fn ev(id: &str, who: &str, at: i64, inside: bool, place: &str, kind: &str) -> PresenceEvent {
        PresenceEvent {
            id: id.into(),
            who: who.into(),
            device: format!("tel-{}", who.to_lowercase()),
            at,
            inside,
            place: place.into(),
            place_kind: kind.into(),
        }
    }

    #[test]
    fn latest_event_of_each_person_wins() {
        let mut doc = PresenceDoc::new();
        let now = 1_000 * MIN;
        doc.record(&ev("1", "Eve", now - 60 * MIN, true, "Maison", "maison"))
            .unwrap();
        doc.record(&ev("2", "Eve", now - 30 * MIN, false, "Maison", "maison"))
            .unwrap();
        doc.record(&ev("3", "Eve", now - 5 * MIN, true, "Université", "etudes"))
            .unwrap();
        doc.record(&ev("4", "Lucas", now - 10 * MIN, false, "Maison", "maison"))
            .unwrap();
        // Horloge du téléphone en avance d'une heure : ignoré.
        doc.record(&ev(
            "5",
            "Lucas",
            now + 60 * MIN,
            true,
            "Travail",
            "travail",
        ))
        .unwrap();
        let current = doc.current(now);
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].id, "3");
        assert!(current[0].inside);
        assert_eq!(current[0].emoji(), "🎓");
        assert_eq!(current[1].id, "4");
        assert!(!current[1].inside);
        // Déjà connu ou sans prénom.
        assert!(!doc.record(&ev("1", "Eve", 0, true, "", "")).unwrap());
        assert!(matches!(
            doc.record(&ev("x", " ", 0, true, "", "")),
            Err(Error::MissingAuthor)
        ));
    }

    #[test]
    fn names_and_kinds_are_cleaned() {
        let mut doc = PresenceDoc::new();
        let long = "Chez\nmamie  ".to_owned() + &"é".repeat(60);
        doc.record(&ev("1", " Eve ", 1, true, &long, "piscine"))
            .unwrap();
        doc.record(&ev("2", "Lucas", 1, true, "", "")).unwrap();
        let current = doc.current(10);
        assert_eq!(current[0].who, "Eve");
        assert!(current[0].place.starts_with("Chez mamie é"));
        assert_eq!(current[0].place.chars().count(), MAX_PLACE_NAME);
        assert_eq!(current[0].place_kind, "autre");
        assert_eq!(current[0].emoji(), "📍");
        // Arrivée d'avant les lieux nommés : la maison.
        assert_eq!(current[1].place_kind, "maison");
        assert_eq!(place_emoji("maison"), "🏠");
    }

    #[test]
    fn prune_keeps_last_event_of_each_person() {
        let mut doc = PresenceDoc::new();
        doc.record(&ev("1", "Eve", 0, true, "Maison", "maison"))
            .unwrap();
        doc.record(&ev("2", "Eve", MIN, false, "Maison", "maison"))
            .unwrap();
        doc.record(&ev("3", "Lucas", 2 * MIN, true, "Maison", "maison"))
            .unwrap();
        assert_eq!(doc.prune(KEEP_MS + 10 * MIN).unwrap(), 1);
        let ids: Vec<String> = doc.list().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["3".to_owned(), "2".to_owned()]);
    }

    #[test]
    fn presence_merges_between_devices() {
        // Le téléphone d'Eve note sa sortie pendant que le PC enregistre,
        // reçue en retard par ntfy, son entrée plus ancienne.
        let now = 1_000 * MIN;
        let mut pc = PresenceDoc::new();
        let mut tel_eve = pc.fork();
        tel_eve
            .record(&ev(
                "sortie",
                "Eve",
                now - MIN,
                false,
                "Université",
                "etudes",
            ))
            .unwrap();
        pc.record(&ev(
            "entree",
            "Eve",
            now - 90 * MIN,
            true,
            "Université",
            "etudes",
        ))
        .unwrap();
        pc.merge(&mut tel_eve).unwrap();
        tel_eve.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), tel_eve.list());
        assert_eq!(pc.current(now)[0].id, "sortie");
        assert!(!pc.current(now)[0].inside);

        let mut other = PresenceDoc::new();
        other
            .record(&ev("l1", "Lucas", now, true, "Travail", "travail"))
            .unwrap();
        sync_pair(&mut pc, &mut other);
        assert_eq!(pc.current(now), other.current(now));
        assert_eq!(pc.current(now).len(), 2);
        let reloaded = PresenceDoc::load(&pc.save()).unwrap();
        assert_eq!(reloaded.list(), pc.list());
    }
}
