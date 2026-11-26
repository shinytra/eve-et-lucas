//! Notes partagées, stockées dans un document Automerge.
//!
//! La racine du document est une table `id de note -> Note`. Chaque note est
//! créée par un seul appareil sous un identifiant unique (UUID v4) : deux
//! appareils qui ajoutent des notes en même temps n'entrent donc jamais en
//! conflit, et la fusion garde les deux. Si les deux modifient le même champ
//! de la même note, Automerge garde une seule valeur (choix déterministe,
//! identique sur tous les appareils).

use std::collections::HashMap;

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate, hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Une note partagée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Prénom de la personne qui a créé la note (« Eve » ou « Lucas »).
    pub author: String,
    /// Millisecondes depuis l'époque Unix.
    pub created_at: i64,
    /// Millisecondes depuis l'époque Unix.
    pub updated_at: i64,
}

/// Le document des notes partagées.
#[derive(Debug, Default, Clone)]
pub struct NotesDoc {
    doc: AutoCommit,
}

impl NotesDoc {
    /// Crée un document vide.
    pub fn new() -> Self {
        Self::default()
    }

    /// Recharge un document à partir des octets produits par [`NotesDoc::save`].
    pub fn load(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            doc: AutoCommit::load(bytes)?,
        })
    }

    /// Sérialise tout le document (historique compris).
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Toutes les notes, la plus récemment modifiée en premier.
    pub fn list(&self) -> Result<Vec<Note>> {
        let notes: HashMap<String, Note> = hydrate(&self.doc)?;
        let mut notes: Vec<Note> = notes.into_values().collect();
        notes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(notes)
    }

    /// Une note par son identifiant.
    pub fn get(&self, id: &str) -> Result<Note> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::NoteNotFound(id.to_owned()));
        }
        Ok(hydrate_prop(&self.doc, ROOT, id)?)
    }

    /// Ajoute une note et la renvoie.
    pub fn add(&mut self, author: &str, title: &str, body: &str, now_ms: i64) -> Result<Note> {
        let (title, body) = clean(title, body)?;
        let note = Note {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            body,
            author: author.trim().to_owned(),
            created_at: now_ms,
            updated_at: now_ms,
        };
        reconcile_prop(&mut self.doc, ROOT, note.id.as_str(), &note)?;
        Ok(note)
    }

    /// Modifie le titre et le contenu d'une note existante.
    pub fn update(&mut self, id: &str, title: &str, body: &str, now_ms: i64) -> Result<Note> {
        let (title, body) = clean(title, body)?;
        let mut note = self.get(id)?;
        note.title = title;
        note.body = body;
        note.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &note)?;
        Ok(note)
    }

    /// Supprime une note.
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::NoteNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }

    /// Intègre les changements d'un autre appareil. C'est ce que fera la
    /// synchro iroh ; l'opération est sans conflit et peut être répétée.
    pub fn merge(&mut self, other: &mut NotesDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

fn clean(title: &str, body: &str) -> Result<(String, String)> {
    let title = title.trim();
    let body = body.trim_end();
    if title.is_empty() && body.trim().is_empty() {
        return Err(Error::EmptyNote);
    }
    Ok((title.to_owned(), body.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simule un deuxième appareil qui part du même état.
    fn copy(doc: &mut NotesDoc) -> NotesDoc {
        NotesDoc::load(&doc.save()).unwrap()
    }

    #[test]
    fn add_update_delete() {
        let mut doc = NotesDoc::new();
        let note = doc.add("Lucas", " Courses ", "lait\npain\n", 1).unwrap();
        assert_eq!(note.title, "Courses");
        assert_eq!(note.body, "lait\npain");
        assert_eq!(doc.list().unwrap(), vec![note.clone()]);

        let updated = doc.update(&note.id, "Courses", "lait", 2).unwrap();
        assert_eq!(updated.created_at, 1);
        assert_eq!(updated.updated_at, 2);
        assert_eq!(doc.get(&note.id).unwrap(), updated);

        doc.delete(&note.id).unwrap();
        assert!(doc.list().unwrap().is_empty());
        assert!(matches!(doc.get(&note.id), Err(Error::NoteNotFound(_))));
        assert!(matches!(doc.delete(&note.id), Err(Error::NoteNotFound(_))));
    }

    #[test]
    fn rejects_empty_note() {
        let mut doc = NotesDoc::new();
        assert!(matches!(
            doc.add("Eve", "  ", "\n ", 1),
            Err(Error::EmptyNote)
        ));
    }

    #[test]
    fn list_is_sorted_by_last_update() {
        let mut doc = NotesDoc::new();
        let a = doc.add("Eve", "A", "", 1).unwrap();
        let b = doc.add("Eve", "B", "", 2).unwrap();
        let titles = |d: &NotesDoc| {
            d.list()
                .unwrap()
                .into_iter()
                .map(|n| n.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(&doc), ["B", "A"]);
        doc.update(&a.id, "A", "modifiée", 3).unwrap();
        assert_eq!(titles(&doc), ["A", "B"]);
        let _ = b;
    }

    #[test]
    fn save_and_load_roundtrip() {
        let mut doc = NotesDoc::new();
        doc.add("Lucas", "Film", "à voir ce soir", 1).unwrap();
        let mut reloaded = copy(&mut doc);
        assert_eq!(reloaded.list().unwrap(), doc.list().unwrap());
        assert!(NotesDoc::load(b"pas un document").is_err());
        // Le document rechargé reste modifiable.
        reloaded.add("Eve", "Resto", "", 2).unwrap();
        assert_eq!(reloaded.list().unwrap().len(), 2);
    }

    #[test]
    fn concurrent_adds_on_two_fresh_devices_are_both_kept() {
        // Les deux téléphones n'ont jamais communiqué avant.
        let mut eve = NotesDoc::new();
        let mut lucas = NotesDoc::new();
        eve.add("Eve", "De Eve", "", 1).unwrap();
        lucas.add("Lucas", "De Lucas", "", 1).unwrap();

        eve.merge(&mut lucas).unwrap();
        lucas.merge(&mut eve).unwrap();

        assert_eq!(eve.list().unwrap().len(), 2);
        assert_eq!(eve.list().unwrap(), lucas.list().unwrap());
    }

    #[test]
    fn concurrent_edit_and_delete_converge() {
        let mut eve = NotesDoc::new();
        let keep = eve.add("Eve", "Garder", "", 1).unwrap();
        let drop = eve.add("Eve", "Supprimer", "", 1).unwrap();
        let mut lucas = copy(&mut eve);

        eve.update(&keep.id, "Garder", "modifiée par Eve", 2)
            .unwrap();
        lucas.delete(&drop.id).unwrap();

        eve.merge(&mut lucas).unwrap();
        lucas.merge(&mut eve).unwrap();

        let notes = eve.list().unwrap();
        assert_eq!(notes, lucas.list().unwrap());
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "modifiée par Eve");
    }

    #[test]
    fn same_field_edited_on_both_devices_converges() {
        let mut eve = NotesDoc::new();
        let note = eve.add("Eve", "Titre", "", 1).unwrap();
        let mut lucas = copy(&mut eve);

        eve.update(&note.id, "Titre", "version Eve", 2).unwrap();
        lucas.update(&note.id, "Titre", "version Lucas", 2).unwrap();

        eve.merge(&mut lucas).unwrap();
        lucas.merge(&mut eve).unwrap();

        // Une seule version gagne, mais c'est la même des deux côtés.
        assert_eq!(eve.list().unwrap(), lucas.list().unwrap());
        assert_eq!(eve.list().unwrap().len(), 1);
    }
}
