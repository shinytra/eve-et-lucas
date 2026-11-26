//! Tâches communes, stockées dans un document Automerge.
//!
//! Même principe que les notes : la racine est une table `id -> Task`, chaque
//! tâche a un identifiant unique (UUID v4) et deux appareils qui ajoutent des
//! tâches en même temps ne se gênent pas. Si les deux modifient le même champ
//! de la même tâche (par exemple l'un la coche pendant que l'autre la décoche),
//! Automerge garde une seule valeur, la même partout. Une suppression
//! l'emporte sur une modification faite en même temps.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Longueur maximale du titre d'une tâche, en caractères.
pub const MAX_TITLE: usize = 200;

/// Une tâche commune.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Task {
    pub id: String,
    pub title: String,
    /// Prénom de la personne qui a créé la tâche.
    pub author: String,
    /// Qui doit la faire : un prénom, ou vide pour « tous les deux ».
    pub assignee: String,
    /// Échéance du rappel (millisecondes Unix), ou aucune.
    pub due_at: Option<i64>,
    pub done: bool,
    /// Qui l'a cochée (vide si elle n'est pas faite).
    pub done_by: String,
    pub done_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Le document des tâches communes. Comme pour [`crate::NotesDoc`], `Clone`
/// garde l'identifiant d'appareil : utiliser [`TasksDoc::fork`] pour simuler
/// un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct TasksDoc {
    doc: AutoCommit,
}

impl TasksDoc {
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

    /// Toutes les tâches lisibles : d'abord celles à faire (échéance la plus
    /// proche en premier, puis sans échéance, de la plus récente à la plus
    /// ancienne), puis celles faites (la dernière cochée en premier).
    pub fn list(&self) -> Vec<Task> {
        let mut tasks: Vec<Task> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        tasks.sort_by(|a, b| {
            a.done
                .cmp(&b.done)
                .then_with(|| {
                    if a.done {
                        b.done_at.cmp(&a.done_at)
                    } else {
                        // `None` après toutes les échéances.
                        let key = |t: &Task| (t.due_at.is_none(), t.due_at);
                        key(a)
                            .cmp(&key(b))
                            .then_with(|| b.created_at.cmp(&a.created_at))
                    }
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        tasks
    }

    /// Nombre d'entrées du document qui ne sont pas des tâches valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    fn read(&self, key: &str) -> Option<Task> {
        hydrate_prop::<_, Task, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|task| task.id == key)
    }

    pub fn get(&self, id: &str) -> Result<Task> {
        self.read(id)
            .ok_or_else(|| Error::TaskNotFound(id.to_owned()))
    }

    /// Ajoute une tâche et la renvoie.
    pub fn add(
        &mut self,
        author: &str,
        title: &str,
        assignee: &str,
        due_at: Option<i64>,
        now_ms: i64,
    ) -> Result<Task> {
        let title = clean_title(title)?;
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let task = Task {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            author: author.to_owned(),
            assignee: assignee.trim().to_owned(),
            due_at,
            done: false,
            done_by: String::new(),
            done_at: None,
            created_at: now_ms,
            updated_at: now_ms,
        };
        reconcile_prop(&mut self.doc, ROOT, task.id.as_str(), &task)?;
        Ok(task)
    }

    /// Modifie le titre, la personne et l'échéance d'une tâche.
    pub fn update(
        &mut self,
        id: &str,
        title: &str,
        assignee: &str,
        due_at: Option<i64>,
        now_ms: i64,
    ) -> Result<Task> {
        let title = clean_title(title)?;
        let mut task = self.get(id)?;
        task.title = title;
        task.assignee = assignee.trim().to_owned();
        task.due_at = due_at;
        task.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &task)?;
        Ok(task)
    }

    /// Coche (ou décoche) une tâche.
    pub fn set_done(&mut self, id: &str, done: bool, by: &str, now_ms: i64) -> Result<Task> {
        let mut task = self.get(id)?;
        task.done = done;
        if done {
            let by = by.trim();
            if by.is_empty() {
                return Err(Error::MissingAuthor);
            }
            task.done_by = by.to_owned();
            task.done_at = Some(now_ms);
        } else {
            task.done_by = String::new();
            task.done_at = None;
        }
        task.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &task)?;
        Ok(task)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::TaskNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }

    /// Tâches à faire dont l'échéance est passée : ce sont celles pour
    /// lesquelles l'appli doit afficher un rappel.
    pub fn due(&self, now_ms: i64) -> Vec<Task> {
        self.list()
            .into_iter()
            .filter(|t| !t.done && t.due_at.is_some_and(|due| due <= now_ms))
            .collect()
    }

    /// Tâches à faire dont l'échéance est encore à venir.
    pub fn upcoming(&self, now_ms: i64) -> Vec<Task> {
        self.list()
            .into_iter()
            .filter(|t| !t.done && t.due_at.is_some_and(|due| due > now_ms))
            .collect()
    }

    pub fn merge(&mut self, other: &mut TasksDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for TasksDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

fn clean_title(title: &str) -> Result<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::EmptyTask);
    }
    if title.chars().count() > MAX_TITLE {
        return Err(Error::TaskTooLong(MAX_TITLE));
    }
    Ok(title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    #[test]
    fn add_update_done_delete() {
        let mut doc = TasksDoc::new();
        let task = doc
            .add("Lucas", " Sortir les poubelles ", "Eve", Some(100), 1)
            .unwrap();
        assert_eq!(task.title, "Sortir les poubelles");
        assert_eq!(task.assignee, "Eve");
        assert!(!task.done);
        assert_eq!(doc.list(), vec![task.clone()]);

        let updated = doc.update(&task.id, "Poubelles", "", None, 2).unwrap();
        assert_eq!(updated.assignee, "");
        assert_eq!(updated.due_at, None);
        assert_eq!(updated.created_at, 1);
        assert_eq!(doc.get(&task.id).unwrap(), updated);

        let done = doc.set_done(&task.id, true, "Eve", 3).unwrap();
        assert!(done.done);
        assert_eq!(done.done_by, "Eve");
        assert_eq!(done.done_at, Some(3));

        let undone = doc.set_done(&task.id, false, "Lucas", 4).unwrap();
        assert!(!undone.done);
        assert_eq!(undone.done_by, "");
        assert_eq!(undone.done_at, None);

        doc.delete(&task.id).unwrap();
        assert!(doc.list().is_empty());
        assert!(matches!(doc.get(&task.id), Err(Error::TaskNotFound(_))));
        assert!(matches!(doc.delete(&task.id), Err(Error::TaskNotFound(_))));
    }

    #[test]
    fn rejects_invalid_tasks() {
        let mut doc = TasksDoc::new();
        assert!(matches!(
            doc.add("Eve", "  ", "", None, 1),
            Err(Error::EmptyTask)
        ));
        assert!(matches!(
            doc.add(" ", "Courses", "", None, 1),
            Err(Error::MissingAuthor)
        ));
        let long = "a".repeat(MAX_TITLE + 1);
        assert!(matches!(
            doc.add("Eve", &long, "", None, 1),
            Err(Error::TaskTooLong(_))
        ));
        let task = doc.add("Eve", "Courses", "", None, 1).unwrap();
        assert!(matches!(
            doc.set_done(&task.id, true, "", 2),
            Err(Error::MissingAuthor)
        ));
    }

    #[test]
    fn list_order() {
        let mut doc = TasksDoc::new();
        let late = doc.add("Eve", "Plus tard", "", Some(500), 1).unwrap();
        let soon = doc.add("Eve", "Bientôt", "", Some(100), 2).unwrap();
        let old = doc.add("Eve", "Sans date, ancienne", "", None, 3).unwrap();
        let new = doc.add("Eve", "Sans date, récente", "", None, 4).unwrap();
        let done_first = doc.add("Eve", "Faite en premier", "", None, 5).unwrap();
        let done_last = doc.add("Eve", "Faite en dernier", "", None, 6).unwrap();
        doc.set_done(&done_first.id, true, "Eve", 7).unwrap();
        doc.set_done(&done_last.id, true, "Lucas", 8).unwrap();

        let ids: Vec<_> = doc.list().into_iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            vec![
                soon.id,
                late.id,
                new.id,
                old.id,
                done_last.id,
                done_first.id
            ]
        );
    }

    #[test]
    fn due_and_upcoming() {
        let mut doc = TasksDoc::new();
        let past = doc.add("Eve", "Passée", "", Some(100), 1).unwrap();
        let now = doc.add("Eve", "Maintenant", "", Some(200), 1).unwrap();
        let future = doc.add("Eve", "Future", "", Some(300), 1).unwrap();
        doc.add("Eve", "Sans date", "", None, 1).unwrap();
        let done = doc.add("Eve", "Faite", "", Some(50), 1).unwrap();
        doc.set_done(&done.id, true, "Eve", 2).unwrap();

        let ids = |tasks: Vec<Task>| tasks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(doc.due(200)), vec![past.id, now.id]);
        assert_eq!(ids(doc.upcoming(200)), vec![future.id]);
    }

    #[test]
    fn survives_save_and_load() {
        let mut doc = TasksDoc::new();
        let task = doc.add("Lucas", "Pain", "Lucas", Some(42), 1).unwrap();
        let mut reloaded = TasksDoc::load(&doc.save()).unwrap();
        assert_eq!(reloaded.list(), vec![task]);
        // Une tâche sans échéance se relit aussi (champ `None`).
        let other = reloaded.add("Eve", "Lait", "", None, 2).unwrap();
        let again = TasksDoc::load(&reloaded.save()).unwrap();
        assert_eq!(again.get(&other.id).unwrap(), other);
    }

    #[test]
    fn concurrent_additions_are_kept() {
        let mut pc = TasksDoc::new();
        let mut phone = pc.fork();
        let a = pc.add("Lucas", "Réserver le resto", "", None, 1).unwrap();
        let b = phone
            .add("Eve", "Acheter des fleurs", "Lucas", None, 1)
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
        assert!(pc.get(&a.id).is_ok() && pc.get(&b.id).is_ok());
    }

    #[test]
    fn concurrent_done_and_edit_converge() {
        let mut pc = TasksDoc::new();
        let task = pc.add("Lucas", "Ménage", "", Some(10), 1).unwrap();
        let mut phone = pc.fork();
        pc.update(&task.id, "Ménage du salon", "", Some(10), 2)
            .unwrap();
        phone.set_done(&task.id, true, "Eve", 3).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        // Champs différents : les deux modifications sont gardées.
        let merged = pc.get(&task.id).unwrap();
        assert_eq!(merged.title, "Ménage du salon");
        assert!(merged.done);
        assert_eq!(merged.done_by, "Eve");
    }

    #[test]
    fn concurrent_delete_wins_over_done() {
        let mut pc = TasksDoc::new();
        let task = pc.add("Lucas", "Vaisselle", "", None, 1).unwrap();
        let mut phone = pc.fork();
        pc.delete(&task.id).unwrap();
        phone.set_done(&task.id, true, "Eve", 2).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert!(pc.list().is_empty());
        assert!(phone.list().is_empty());
    }

    #[test]
    fn unreadable_entry_is_ignored() {
        let mut doc = TasksDoc::new();
        let task = doc.add("Eve", "Courses", "", None, 1).unwrap();
        doc.doc.put(ROOT, "bizarre", "pas une tâche").unwrap();
        assert_eq!(doc.list(), vec![task]);
        assert_eq!(doc.unreadable_count(), 1);
    }

    #[test]
    fn syncs_with_protocol() {
        let mut pc = TasksDoc::new();
        let mut phone = TasksDoc::new();
        pc.add("Lucas", "Pain", "", Some(1_000), 1).unwrap();
        phone.add("Eve", "Lait", "Eve", None, 2).unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }
}
