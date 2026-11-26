//! Notes partagées, stockées dans un document Automerge.
//!
//! La racine du document est une table `id de note -> Note`. Chaque note est
//! créée par un seul appareil sous un identifiant unique (UUID v4) : deux
//! appareils qui ajoutent des notes en même temps n'entrent donc jamais en
//! conflit, et la fusion garde les deux. Si les deux modifient le même champ
//! de la même note, Automerge garde une seule valeur (choix déterministe,
//! identique sur tous les appareils). Si l'un modifie une note pendant que
//! l'autre la supprime, la suppression l'emporte : la modification est perdue.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Une note partagée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Prénom de la personne qui a créé la note (en principe « Eve » ou
    /// « Lucas » ; seul un prénom vide est refusé).
    pub author: String,
    /// Millisecondes depuis l'époque Unix.
    pub created_at: i64,
    /// Millisecondes depuis l'époque Unix.
    pub updated_at: i64,
}

/// Le document des notes partagées.
///
/// `Clone` garde le même identifiant d'appareil Automerge : c'est fait pour
/// remplacer un document par sa copie (voir l'appli PC), jamais pour simuler
/// un deuxième appareil. Pour cela, utiliser [`NotesDoc::fork`].
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

    /// Copie du document pour un autre appareil (nouvel identifiant d'appareil).
    pub fn fork(&mut self) -> Self {
        Self {
            doc: self.doc.fork(),
        }
    }

    /// Sérialise tout le document (historique compris).
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Toutes les notes lisibles, la plus récemment modifiée en premier.
    ///
    /// Une entrée mal formée (par exemple venue d'un appareil avec une autre
    /// version de l'appli) est ignorée plutôt que de masquer toutes les autres ;
    /// [`NotesDoc::unreadable_count`] permet de le signaler.
    pub fn list(&self) -> Result<Vec<Note>> {
        let mut notes: Vec<Note> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        notes.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(notes)
    }

    /// Nombre d'entrées du document qui ne sont pas des notes valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    /// Lit une entrée ; `None` si elle n'est pas une note valide ou si son
    /// identifiant ne correspond pas à sa clé.
    fn read(&self, key: &str) -> Option<Note> {
        hydrate_prop::<_, Note, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|note| note.id == key)
    }

    /// Une note par son identifiant.
    pub fn get(&self, id: &str) -> Result<Note> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::NoteNotFound(id.to_owned()));
        }
        let note: Note = hydrate_prop(&self.doc, ROOT, id)?;
        if note.id != id {
            return Err(Error::NoteNotFound(id.to_owned()));
        }
        Ok(note)
    }

    /// Ajoute une note et la renvoie.
    pub fn add(&mut self, author: &str, title: &str, body: &str, now_ms: i64) -> Result<Note> {
        let (title, body) = clean(title, body)?;
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let note = Note {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            body,
            author: author.to_owned(),
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

    /// Simule un deuxième appareil qui part du même état (nouvel identifiant
    /// d'appareil, contrairement à `clone`).
    fn copy(doc: &mut NotesDoc) -> NotesDoc {
        doc.fork()
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
        doc.add("Eve", "B", "", 2).unwrap();
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
    fn edit_and_delete_of_different_notes_converge() {
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

        // Une seule version gagne, entière, et c'est la même des deux côtés.
        let notes = eve.list().unwrap();
        assert_eq!(notes, lucas.list().unwrap());
        assert_eq!(notes.len(), 1);
        assert!(["version Eve", "version Lucas"].contains(&notes[0].body.as_str()));
    }

    #[test]
    fn rejects_missing_author() {
        let mut doc = NotesDoc::new();
        assert!(matches!(
            doc.add(" ", "Titre", "", 1),
            Err(Error::MissingAuthor)
        ));
        assert!(doc.list().unwrap().is_empty());
    }

    #[test]
    fn title_or_body_alone_is_enough() {
        let mut doc = NotesDoc::new();
        let only_body = doc.add(" Eve ", "  ", "contenu seul", 1).unwrap();
        assert_eq!(only_body.author, "Eve");
        assert_eq!(only_body.title, "");
        assert_eq!(doc.add("Eve", "Titre seul", "", 1).unwrap().body, "");
    }

    #[test]
    fn update_rejects_empty_and_unknown_notes() {
        let mut doc = NotesDoc::new();
        let note = doc.add("Eve", "Titre", "contenu", 1).unwrap();
        assert!(matches!(
            doc.update(&note.id, " ", "", 2),
            Err(Error::EmptyNote)
        ));
        assert_eq!(doc.get(&note.id).unwrap(), note);
        assert!(matches!(
            doc.update("inconnue", "Titre", "", 2),
            Err(Error::NoteNotFound(_))
        ));
    }

    #[test]
    fn malformed_entries_do_not_hide_other_notes() {
        let mut doc = NotesDoc::new();
        let note = doc.add("Lucas", "Valide", "", 1).unwrap();
        // Entrée écrite par un futur appareil dans un autre format.
        doc.doc.put(ROOT, "pas-une-note", 42_i64).unwrap();
        // Note rangée sous une clé qui n'est pas son identifiant.
        let mut wrong_key = note.clone();
        wrong_key.title = "Mauvaise clé".into();
        reconcile_prop(&mut doc.doc, ROOT, "autre-cle", &wrong_key).unwrap();

        assert_eq!(doc.list().unwrap(), vec![note]);
        assert_eq!(doc.unreadable_count(), 2);
        assert!(matches!(doc.get("autre-cle"), Err(Error::NoteNotFound(_))));
    }

    #[test]
    fn concurrent_update_and_delete_of_same_note_converge() {
        let mut eve = NotesDoc::new();
        let note = eve.add("Eve", "Titre", "", 1).unwrap();
        let mut lucas = copy(&mut eve);

        eve.update(&note.id, "Titre", "modifiée", 2).unwrap();
        lucas.delete(&note.id).unwrap();

        eve.merge(&mut lucas).unwrap();
        lucas.merge(&mut eve).unwrap();

        // La suppression l'emporte, des deux côtés.
        assert!(eve.list().unwrap().is_empty());
        assert!(lucas.list().unwrap().is_empty());
    }

    #[test]
    fn merge_can_be_repeated() {
        let mut eve = NotesDoc::new();
        eve.add("Eve", "Une", "", 1).unwrap();
        let mut lucas = NotesDoc::new();
        lucas.add("Lucas", "Deux", "", 1).unwrap();
        eve.merge(&mut lucas).unwrap();
        let once = eve.list().unwrap();
        eve.merge(&mut lucas).unwrap();
        assert_eq!(eve.list().unwrap(), once);
    }
}
