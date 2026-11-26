//! Contenu du widget d'écran d'accueil (Android).
//!
//! Le widget ne lit pas les documents Automerge : l'appli lui envoie un
//! résumé tout prêt ([`WidgetData`]), recalculé à chaque changement. Les
//! actions faites sur le widget (cocher une tâche) sont notées par Android,
//! même appli fermée, puis appliquées par l'appli avec [`apply_action`].

use serde::{Deserialize, Serialize};

use crate::{Arrival, Error, Result, Task, TasksDoc};

/// Nombre maximal de tâches affichées sur le widget.
pub const MAX_TASKS: usize = 8;
/// Une arrivée reste affichée sur le widget pendant 12 heures.
pub const ARRIVAL_SHOWN_MS: i64 = 12 * 60 * 60 * 1000;

/// Une tâche telle qu'affichée sur le widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetTask {
    pub id: String,
    pub title: String,
    /// Prénom, ou vide pour « tous les deux ».
    pub assignee: String,
    pub due_at: Option<i64>,
    /// L'échéance est passée.
    pub late: bool,
}

/// Dernière arrivée à la maison, si elle est récente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetArrival {
    pub who: String,
    pub at: i64,
}

/// Ce que le widget affiche.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetData {
    /// Tâches à faire, la plus urgente en premier (au plus [`MAX_TASKS`]).
    pub tasks: Vec<WidgetTask>,
    /// Nombre total de tâches à faire (pour « et 3 autres »).
    pub open_count: usize,
    pub last_arrival: Option<WidgetArrival>,
    /// Moment du calcul (millisecondes Unix).
    pub updated_at: i64,
}

/// Calcule le contenu du widget. `tasks` dans l'ordre de [`TasksDoc::list`],
/// `arrivals` dans l'ordre de [`crate::ArrivalsDoc::list`].
pub fn snapshot(tasks: &[Task], arrivals: &[Arrival], now_ms: i64) -> WidgetData {
    let open: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();
    WidgetData {
        tasks: open
            .iter()
            .take(MAX_TASKS)
            .map(|t| WidgetTask {
                id: t.id.clone(),
                title: t.title.clone(),
                assignee: t.assignee.clone(),
                due_at: t.due_at,
                late: t.due_at.is_some_and(|due| due <= now_ms),
            })
            .collect(),
        open_count: open.len(),
        last_arrival: arrivals
            .iter()
            .max_by_key(|a| a.at)
            .filter(|a| now_ms - a.at <= ARRIVAL_SHOWN_MS && a.at <= now_ms + 60_000)
            .map(|a| WidgetArrival {
                who: a.who.clone(),
                at: a.at,
            }),
        updated_at: now_ms,
    }
}

/// Une action faite sur le widget, notée par Android.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetAction {
    /// Identifiant unique de l'action (pour la confirmer une fois appliquée).
    pub id: String,
    /// Pour l'instant, seulement `"done"` : cocher la tâche.
    pub kind: String,
    pub task_id: String,
    /// Moment du clic (millisecondes Unix).
    pub at: i64,
}

/// Applique une action du widget. Renvoie `Ok(true)` si le document a
/// changé, `Ok(false)` si l'action est sans objet (tâche supprimée ou déjà
/// faite entre-temps, type d'action inconnu) : elle peut être oubliée.
pub fn apply_action(doc: &mut TasksDoc, action: &WidgetAction, who: &str) -> Result<bool> {
    if action.kind != "done" {
        return Ok(false);
    }
    match doc.get(&action.task_id) {
        Ok(task) if task.done => Ok(false),
        Ok(_) => doc
            .set_done(&action.task_id, true, who, action.at)
            .map(|_| true),
        Err(Error::TaskNotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArrivalsDoc;

    const MIN: i64 = 60 * 1000;

    #[test]
    fn snapshot_lists_open_tasks_and_recent_arrival() {
        let now = 10_000 * MIN;
        let mut tasks = TasksDoc::new();
        let late = tasks
            .add("Eve", "Poubelles", "", Some(now - MIN), now - 60 * MIN)
            .unwrap();
        let later = tasks
            .add("Lucas", "Courses", "Lucas", Some(now + 60 * MIN), now)
            .unwrap();
        let done = tasks.add("Eve", "Fait", "", None, now).unwrap();
        tasks.set_done(&done.id, true, "Eve", now).unwrap();
        for i in 0..10 {
            tasks.add("Eve", &format!("t{i}"), "", None, now).unwrap();
        }
        let mut arrivals = ArrivalsDoc::new();
        arrivals.add("Eve", "tel-eve", now - 13 * 60 * MIN).unwrap();
        arrivals.add("Lucas", "tel-lucas", now - 30 * MIN).unwrap();

        let data = snapshot(&tasks.list(), &arrivals.list(), now);
        assert_eq!(data.open_count, 12);
        assert_eq!(data.tasks.len(), MAX_TASKS);
        assert_eq!(data.tasks[0].id, late.id);
        assert!(data.tasks[0].late);
        assert_eq!(data.tasks[1].id, later.id);
        assert!(!data.tasks[1].late);
        assert!(data.tasks.iter().all(|t| t.id != done.id));
        assert_eq!(
            data.last_arrival,
            Some(WidgetArrival {
                who: "Lucas".into(),
                at: now - 30 * MIN
            })
        );

        // Arrivée trop ancienne : plus affichée.
        let data = snapshot(&[], &arrivals.list(), now + 13 * 60 * MIN);
        assert_eq!(data.last_arrival, None);
    }

    #[test]
    fn widget_action_marks_task_done() {
        let mut tasks = TasksDoc::new();
        let t = tasks.add("Eve", "Pain", "", None, 0).unwrap();
        let action = WidgetAction {
            id: "a1".into(),
            kind: "done".into(),
            task_id: t.id.clone(),
            at: 5 * MIN,
        };
        assert!(apply_action(&mut tasks, &action, "Lucas").unwrap());
        let task = tasks.get(&t.id).unwrap();
        assert!(task.done);
        assert_eq!(task.done_by, "Lucas");
        assert_eq!(task.done_at, Some(5 * MIN));
        // Deuxième fois, ou tâche supprimée, ou action inconnue : sans objet.
        assert!(!apply_action(&mut tasks, &action, "Lucas").unwrap());
        tasks.delete(&t.id).unwrap();
        assert!(!apply_action(&mut tasks, &action, "Lucas").unwrap());
        let unknown = WidgetAction {
            kind: "autre".into(),
            ..action
        };
        assert!(!apply_action(&mut tasks, &unknown, "Lucas").unwrap());
    }

    #[test]
    fn widget_action_merges_with_other_device() {
        // Le téléphone coche depuis le widget pendant que le PC renomme la tâche.
        let mut pc = TasksDoc::new();
        let t = pc.add("Eve", "Pain", "", None, 0).unwrap();
        let mut phone = pc.fork();
        let action = WidgetAction {
            id: "a1".into(),
            kind: "done".into(),
            task_id: t.id.clone(),
            at: MIN,
        };
        assert!(apply_action(&mut phone, &action, "Eve").unwrap());
        pc.update(&t.id, "Pain complet", "", None, 2 * MIN).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 1);
    }

    #[test]
    fn widget_data_uses_camel_case() {
        let json = serde_json::to_string(&WidgetData::default()).unwrap();
        assert!(json.contains("openCount") && json.contains("lastArrival"));
        let action: WidgetAction =
            serde_json::from_str(r#"{"id":"a","kind":"done","taskId":"t","at":1}"#).unwrap();
        assert_eq!(action.task_id, "t");
    }
}
