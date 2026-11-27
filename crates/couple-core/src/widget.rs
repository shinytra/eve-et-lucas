//! Contenu du widget d'écran d'accueil (Android).
//!
//! Le widget ne lit pas les documents Automerge : l'appli lui envoie un
//! résumé tout prêt ([`WidgetData`]), recalculé à chaque changement. Les
//! actions faites sur le widget (cocher une tâche) sont notées par Android,
//! même appli fermée, puis appliquées par l'appli avec [`apply_action`].

use serde::{Deserialize, Serialize};

use crate::{Arrival, Day, Error, PresenceEvent, Result, Task, TasksDoc, UpcomingDate};

/// Nombre maximal de tâches affichées sur le widget.
pub const MAX_TASKS: usize = 8;
/// Nombre de dates à venir envoyées au widget : appli fermée, il passe
/// lui-même à la suivante quand une date est passée.
pub const MAX_DATES: usize = 5;
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

/// Où est une personne (voir [`crate::presence`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetPresence {
    pub who: String,
    /// Dans l'un de ses lieux ; sinon ailleurs.
    pub inside: bool,
    /// Nom du lieu (vide : la maison, pour une ancienne arrivée).
    pub place: String,
    pub place_kind: String,
    /// Moment de l'entrée ou de la sortie.
    pub at: i64,
}

/// Une date importante à venir. Le widget compte lui-même les jours restants
/// à partir de `day` : le compte reste juste après minuit, appli fermée.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDate {
    pub title: String,
    pub emoji: String,
    /// Jour de la prochaine occurrence.
    pub day: Day,
    /// Années fêtées ce jour-là (« 5 ans »).
    pub years: Option<i32>,
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
    /// Où sont les autres (pas la personne qui porte ce téléphone), triés
    /// par prénom.
    #[serde(default)]
    pub presence: Vec<WidgetPresence>,
    /// Prochaines dates importantes, la plus proche en premier (au plus
    /// [`MAX_DATES`]). Le widget affiche la première qui n'est pas passée.
    pub next_dates: Vec<WidgetDate>,
    /// Moment du calcul (millisecondes Unix).
    pub updated_at: i64,
}

/// Calcule le contenu du widget. `tasks` dans l'ordre de [`TasksDoc::list`],
/// `arrivals` dans l'ordre de [`crate::ArrivalsDoc::list`], `dates` dans
/// celui de [`crate::DatesDoc::upcoming`] (la plus proche en premier),
/// `presence` celui de [`crate::PresenceDoc::current`]. `me` : prénom de la
/// personne qui porte ce téléphone (vide s'il n'est pas choisi), dont la
/// présence n'est pas affichée.
pub fn snapshot(
    tasks: &[Task],
    arrivals: &[Arrival],
    dates: &[UpcomingDate],
    presence: &[PresenceEvent],
    me: &str,
    now_ms: i64,
) -> WidgetData {
    let me = me.trim();
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
        presence: presence
            .iter()
            .filter(|p| me.is_empty() || !p.who.trim().eq_ignore_ascii_case(me))
            .map(|p| WidgetPresence {
                who: p.who.clone(),
                inside: p.inside,
                place: p.place.clone(),
                place_kind: p.place_kind.clone(),
                at: p.at,
            })
            .collect(),
        next_dates: dates
            .iter()
            .filter_map(|u| {
                Some(WidgetDate {
                    title: u.date.title.clone(),
                    emoji: u.date.emoji().to_owned(),
                    day: u.next?,
                    years: u.years,
                })
            })
            .take(MAX_DATES)
            .collect(),
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
    use crate::dates::{KIND_BIRTHDAY, KIND_COUPLE, KIND_OTHER};
    use crate::{ArrivalsDoc, DateInput, DatesDoc};

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

        let data = snapshot(&tasks.list(), &arrivals.list(), &[], &[], "", now);
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
        let data = snapshot(&[], &arrivals.list(), &[], &[], "", now + 13 * 60 * MIN);
        assert_eq!(data.last_arrival, None);
        assert!(data.next_dates.is_empty());
    }

    fn date_input(
        title: &str,
        kind: &str,
        month: u32,
        day: u32,
        year: i32,
        yearly: bool,
    ) -> DateInput {
        DateInput {
            title: title.into(),
            kind: kind.into(),
            month,
            day,
            year: Some(year),
            yearly,
            remind_days: 0,
        }
    }

    #[test]
    fn snapshot_lists_next_important_dates() {
        let mut dates = DatesDoc::new();
        dates
            .add(
                "Eve",
                &date_input("Vacances", KIND_OTHER, 3, 1, 2026, false),
                0,
            )
            .unwrap();
        dates
            .add(
                "Lucas",
                &date_input("Nous", KIND_COUPLE, 6, 12, 2021, true),
                0,
            )
            .unwrap();
        dates
            .add(
                "Eve",
                &date_input("Mariage", KIND_OTHER, 9, 5, 2027, false),
                0,
            )
            .unwrap();
        for i in 0..6 {
            dates
                .add(
                    "Eve",
                    &date_input(&format!("d{i}"), KIND_OTHER, 12, 1, 2030, false),
                    0,
                )
                .unwrap();
        }
        let today = Day::new(2026, 11, 26).unwrap();
        let data = snapshot(&[], &[], &dates.upcoming(today), &[], "", 0);
        // La date unique passée (vacances) n'est jamais envoyée.
        assert_eq!(data.next_dates.len(), MAX_DATES);
        assert_eq!(
            data.next_dates[0],
            WidgetDate {
                title: "Nous".into(),
                emoji: "💞".into(),
                day: Day::new(2027, 6, 12).unwrap(),
                years: Some(6),
            }
        );
        assert_eq!(data.next_dates[1].title, "Mariage");
        assert_eq!(data.next_dates[1].years, None);

        // Le jour même, elle reste la première.
        let today = Day::new(2027, 6, 12).unwrap();
        let data = snapshot(&[], &[], &dates.upcoming(today), &[], "", 0);
        assert_eq!(data.next_dates[0].title, "Nous");

        // Plus aucune date à venir.
        let mut past = DatesDoc::new();
        past.add(
            "Eve",
            &date_input("Vacances", KIND_OTHER, 3, 1, 2026, false),
            0,
        )
        .unwrap();
        let data = snapshot(&[], &[], &past.upcoming(today), &[], "", 0);
        assert!(data.next_dates.is_empty());
    }

    #[test]
    fn next_dates_merge_from_other_device() {
        // Le PC et le téléphone ajoutent chacun une date avant de se synchroniser.
        let mut pc = DatesDoc::new();
        let mut phone = pc.fork();
        pc.add(
            "Lucas",
            &date_input("Eve", KIND_BIRTHDAY, 12, 3, 2000, true),
            0,
        )
        .unwrap();
        phone
            .add(
                "Eve",
                &date_input("Lucas", KIND_BIRTHDAY, 1, 20, 1997, true),
                0,
            )
            .unwrap();
        phone.merge(&mut pc).unwrap();
        pc.merge(&mut phone).unwrap();
        let today = Day::new(2026, 11, 26).unwrap();
        let data = snapshot(&[], &[], &phone.upcoming(today), &[], "", 0);
        assert_eq!(data, snapshot(&[], &[], &pc.upcoming(today), &[], "", 0));
        let next = &data.next_dates[0];
        assert_eq!((next.title.as_str(), next.emoji.as_str()), ("Eve", "🎂"));
        assert_eq!(next.day, Day::new(2026, 12, 3).unwrap());
        assert_eq!(next.years, Some(26));
        assert_eq!(data.next_dates[1].day, Day::new(2027, 1, 20).unwrap());
    }

    #[test]
    fn snapshot_shows_where_the_others_are() {
        let mut presence = crate::PresenceDoc::new();
        let mut tel_eve = presence.fork();
        let ev = |id: &str, who: &str, at, inside, place: &str, kind: &str| PresenceEvent {
            id: id.into(),
            who: who.into(),
            device: "d".into(),
            at,
            inside,
            place: place.into(),
            place_kind: kind.into(),
        };
        tel_eve
            .record(&ev("1", "Eve", 10 * MIN, true, "Université", "etudes"))
            .unwrap();
        presence
            .record(&ev("2", "Lucas", 20 * MIN, false, "Maison", "maison"))
            .unwrap();
        presence.merge(&mut tel_eve).unwrap();
        let current = presence.current(30 * MIN);
        // Sur le téléphone de Lucas : seulement Eve.
        let data = snapshot(&[], &[], &[], &current, "lucas", 30 * MIN);
        assert_eq!(
            data.presence,
            vec![WidgetPresence {
                who: "Eve".into(),
                inside: true,
                place: "Université".into(),
                place_kind: "etudes".into(),
                at: 10 * MIN,
            }]
        );
        // Prénom pas encore choisi : tout le monde.
        assert_eq!(snapshot(&[], &[], &[], &current, "", 0).presence.len(), 2);
        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains(r#""placeKind":"etudes""#), "{json}");
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
        assert!(json.contains("nextDates"));
        let action: WidgetAction =
            serde_json::from_str(r#"{"id":"a","kind":"done","taskId":"t","at":1}"#).unwrap();
        assert_eq!(action.task_id, "t");
    }
}
