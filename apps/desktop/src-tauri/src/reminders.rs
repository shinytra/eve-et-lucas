//! Rappels des tâches : notifications à l'échéance.
//!
//! - Une tâche à faire dont l'échéance est passée affiche une notification.
//!   Sur téléphone, elle est permanente (impossible à balayer) jusqu'à ce que
//!   la tâche soit cochée ou supprimée, sur n'importe quel appareil. Sur PC,
//!   où une notification ne peut pas rester affichée, elle revient toutes les
//!   heures tant que la tâche n'est pas faite.
//! - Sur téléphone, les échéances à venir sont aussi confiées à Android : le
//!   rappel s'affiche même si l'appli a été fermée entre-temps.

use std::{collections::HashMap, sync::Arc, time::Duration};

use couple_core::Task;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::{now_ms, store::AppState};

/// Vérification régulière, en plus des réveils après chaque modification.
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// Sur PC : intervalle entre deux rappels d'une tâche toujours pas faite.
const REPEAT_MS: i64 = 60 * 60 * 1000;

/// Identifiant de notification d'une tâche, stable d'un lancement à l'autre.
fn notification_id(task_id: &str) -> i32 {
    // FNV-1a : simple et identique sur toutes les plateformes.
    let hash = task_id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    // Positif : Android refuse certains identifiants négatifs.
    i32::try_from(hash & 0x7fff_ffff).unwrap_or(0)
}

/// Ce qu'il faut faire à un instant donné.
#[derive(Debug, Default, PartialEq)]
struct Plan {
    /// Tâches dont il faut (ré)afficher le rappel.
    show: Vec<Task>,
    /// Tâches dont le rappel doit disparaître (faites, supprimées ou reportées).
    hide: Vec<String>,
}

/// `shown` : tâche -> date du dernier rappel affiché. `repeat` : `None` sur
/// téléphone (notification permanente), sinon l'intervalle entre deux rappels.
fn plan(due: &[Task], shown: &HashMap<String, i64>, now: i64, repeat: Option<i64>) -> Plan {
    let show = due
        .iter()
        .filter(|t| match shown.get(&t.id) {
            None => true,
            Some(last) => repeat.is_some_and(|r| now - last >= r),
        })
        .cloned()
        .collect();
    let mut hide: Vec<String> = shown
        .keys()
        .filter(|id| !due.iter().any(|t| &t.id == *id))
        .cloned()
        .collect();
    hide.sort();
    Plan { show, hide }
}

fn body(task: &Task) -> String {
    match task.assignee.as_str() {
        "" => "À faire ensemble".to_owned(),
        who => format!("Pour {who}"),
    }
}

/// Lance la boucle des rappels.
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(mobile)]
        mobile::prepare(&app);
        let mut shown = HashMap::new();
        #[cfg(mobile)]
        let mut scheduled = HashMap::new();
        loop {
            let now = now_ms();
            let (due, _upcoming) = {
                let tasks = state.tasks.lock();
                (tasks.due(now), tasks.upcoming(now))
            };
            let repeat = if cfg!(mobile) { None } else { Some(REPEAT_MS) };
            let plan = plan(&due, &shown, now, repeat);
            for task in &plan.show {
                if let Err(e) = show(&app, task) {
                    eprintln!("Impossible d'afficher le rappel « {} » : {e}", task.title);
                }
                shown.insert(task.id.clone(), now);
            }
            if !plan.hide.is_empty() {
                let ids = plan.hide.iter().map(|id| notification_id(id)).collect();
                if let Err(e) = app.notification().remove_active(ids) {
                    eprintln!("Impossible de retirer des rappels : {e}");
                }
                for id in &plan.hide {
                    shown.remove(id);
                }
            }
            #[cfg(mobile)]
            mobile::schedule(&app, &_upcoming, &mut scheduled);

            tokio::select! {
                () = state.reminders_wanted.notified() => {}
                () = tokio::time::sleep(CHECK_EVERY) => {}
            }
        }
    });
}

fn show(app: &AppHandle, task: &Task) -> tauri_plugin_notification::Result<()> {
    let builder = app
        .notification()
        .builder()
        .id(notification_id(&task.id))
        .title(format!("Rappel : {}", task.title))
        .body(body(task));
    #[cfg(mobile)]
    let builder = builder.ongoing();
    builder.show()
}

#[cfg(mobile)]
mod mobile {
    use std::collections::HashMap;

    use couple_core::Task;
    use tauri::{plugin::PermissionState, AppHandle};
    use tauri_plugin_notification::{NotificationExt, Schedule};

    use super::{body, notification_id};

    /// Demande l'autorisation d'afficher des notifications (Android 13 et
    /// plus) et oublie les rappels programmés lors d'un lancement précédent :
    /// ils sont reprogrammés juste après à partir des tâches actuelles.
    pub fn prepare(app: &AppHandle) {
        let notification = app.notification();
        match notification.permission_state() {
            Ok(PermissionState::Granted) => {}
            _ => {
                if let Err(e) = notification.request_permission() {
                    eprintln!("Autorisation des notifications impossible : {e}");
                }
            }
        }
        if let Err(e) = notification.cancel_all() {
            eprintln!("Impossible d'annuler les anciens rappels : {e}");
        }
    }

    /// Confie à Android les échéances à venir. `scheduled` : tâche -> échéance
    /// déjà programmée.
    pub fn schedule(app: &AppHandle, upcoming: &[Task], scheduled: &mut HashMap<String, i64>) {
        let notification = app.notification();
        let stale: Vec<String> = scheduled
            .iter()
            .filter(|(id, due)| {
                !upcoming
                    .iter()
                    .any(|t| &t.id == *id && t.due_at == Some(**due))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if !stale.is_empty() {
            let ids = stale.iter().map(|id| notification_id(id)).collect();
            if let Err(e) = notification.cancel(ids) {
                eprintln!("Impossible d'annuler des rappels : {e}");
            }
            for id in &stale {
                scheduled.remove(id);
            }
        }
        for task in upcoming {
            let Some(due) = task.due_at else { continue };
            if scheduled.get(&task.id) == Some(&due) {
                continue;
            }
            let Ok(date) =
                time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(due) * 1_000_000)
            else {
                continue;
            };
            let result = notification
                .builder()
                .id(notification_id(&task.id))
                .title(format!("Rappel : {}", task.title))
                .body(body(task))
                .ongoing()
                .schedule(Schedule::At {
                    date,
                    repeating: false,
                    allow_while_idle: true,
                })
                .show();
            match result {
                Ok(()) => {
                    scheduled.insert(task.id.clone(), due);
                }
                Err(e) => eprintln!(
                    "Impossible de programmer le rappel « {} » : {e}",
                    task.title
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            author: "Eve".to_owned(),
            assignee: String::new(),
            due_at: Some(0),
            done: false,
            done_by: String::new(),
            done_at: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn notification_ids_are_stable_and_positive() {
        let id = "7f0c1c9e-3c0b-4f7e-9a51-2b9d3f8a1e11";
        assert_eq!(notification_id(id), notification_id(id));
        assert!(notification_id(id) >= 0);
        assert_ne!(notification_id(id), notification_id("autre"));
    }

    #[test]
    fn new_due_task_is_shown_once_on_phone() {
        let due = vec![task("a")];
        let first = plan(&due, &HashMap::new(), 10, None);
        assert_eq!(first.show, due);
        let shown = HashMap::from([("a".to_owned(), 10)]);
        // Notification permanente : rien à refaire, même longtemps après.
        assert_eq!(
            plan(&due, &shown, 10 + 10 * REPEAT_MS, None),
            Plan::default()
        );
    }

    #[test]
    fn pc_repeats_every_hour() {
        let due = vec![task("a")];
        let shown = HashMap::from([("a".to_owned(), 10)]);
        assert!(plan(&due, &shown, 10 + REPEAT_MS - 1, Some(REPEAT_MS))
            .show
            .is_empty());
        assert_eq!(
            plan(&due, &shown, 10 + REPEAT_MS, Some(REPEAT_MS)).show,
            due
        );
    }

    #[test]
    fn done_or_deleted_task_is_hidden() {
        let shown = HashMap::from([("a".to_owned(), 10), ("b".to_owned(), 10)]);
        let p = plan(&[task("b")], &shown, 20, None);
        assert!(p.show.is_empty());
        assert_eq!(p.hide, vec!["a".to_owned()]);
    }

    #[test]
    fn body_names_the_assignee() {
        let mut t = task("a");
        assert_eq!(body(&t), "À faire ensemble");
        t.assignee = "Lucas".to_owned();
        assert_eq!(body(&t), "Pour Lucas");
    }
}
