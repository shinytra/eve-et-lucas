//! Rappels des tâches : notifications à l'échéance.
//!
//! - Une tâche à faire dont l'échéance est passée affiche une notification,
//!   retirée dès que la tâche est cochée ou supprimée, sur n'importe quel
//!   appareil.
//! - Sur téléphone, la notification est « en cours » (Android ne la retire pas
//!   tout seul). Depuis Android 14 on peut quand même la balayer : elle
//!   revient alors au bout de 15 minutes tant que la tâche n'est pas faite.
//!   Sur PC, où une notification ne reste pas affichée, elle revient toutes
//!   les heures.
//! - Sur téléphone, les échéances à venir sont aussi confiées à Android : le
//!   rappel s'affiche même si l'appli a été fermée entre-temps. La liste de
//!   ces rappels programmés est gardée sur le disque pour pouvoir les annuler
//!   au lancement suivant (tâche faite ou supprimée entre-temps).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use couple_core::Task;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::{now_ms, store::AppState};

/// Vérification régulière, en plus des réveils après chaque modification.
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// Intervalle avant de remontrer le rappel d'une tâche toujours pas faite.
const REPEAT_MS: i64 = if cfg!(mobile) {
    15 * 60 * 1000
} else {
    60 * 60 * 1000
};

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
    /// Notifications à retirer (tâche faite, supprimée ou reportée).
    hide: Vec<i32>,
    /// Tâches à oublier dans `shown`.
    forget: Vec<String>,
}

/// - `shown` : tâche -> date du dernier rappel affiché par ce lancement ;
/// - `visible` : notifications réellement affichées, si le système sait le
///   dire (téléphone), sinon `None` (PC).
fn plan(
    due: &[Task],
    shown: &HashMap<String, i64>,
    visible: Option<&HashSet<i32>>,
    now: i64,
) -> Plan {
    let is_visible = |id: &str| visible.is_some_and(|v| v.contains(&notification_id(id)));
    let show = due
        .iter()
        .filter(|t| match shown.get(&t.id) {
            None => !is_visible(&t.id),
            Some(last) => !is_visible(&t.id) && now - last >= REPEAT_MS,
        })
        .cloned()
        .collect();
    let mut forget: Vec<String> = shown
        .keys()
        .filter(|id| !due.iter().any(|t| &t.id == *id))
        .cloned()
        .collect();
    forget.sort();
    let due_ids: HashSet<i32> = due.iter().map(|t| notification_id(&t.id)).collect();
    let mut hide: Vec<i32> = match visible {
        // Tout ce qui est affiché sans être échu, y compris ce qu'un lancement
        // précédent a laissé.
        Some(v) => v
            .iter()
            .filter(|id| !due_ids.contains(id))
            .copied()
            .collect(),
        None => forget.iter().map(|id| notification_id(id)).collect(),
    };
    hide.sort_unstable();
    Plan { show, hide, forget }
}

fn title(task: &Task) -> String {
    format!("Rappel : {}", task.title)
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
        let mut scheduled = mobile::prepare(&app, &state);
        let mut shown = HashMap::new();
        let mut failed_once = false;
        loop {
            let now = now_ms();
            let (due, _upcoming) = {
                let tasks = state.tasks.lock();
                (tasks.due(now), tasks.upcoming(now))
            };
            #[cfg(mobile)]
            let visible = mobile::visible(&app);
            #[cfg(not(mobile))]
            let visible: Option<HashSet<i32>> = None;

            let plan = plan(&due, &shown, visible.as_ref(), now);
            for task in &plan.show {
                match show(&app, task) {
                    Ok(()) => {
                        shown.insert(task.id.clone(), now);
                    }
                    // Réessayé au passage suivant ; signalé une seule fois.
                    Err(e) if !failed_once => {
                        failed_once = true;
                        state.notice(format!(
                            "Impossible d'afficher le rappel « {} » : {e}",
                            task.title
                        ));
                    }
                    Err(e) => eprintln!("Rappel « {} » toujours impossible : {e}", task.title),
                }
            }
            if !plan.hide.is_empty() {
                if let Err(e) = app.notification().remove_active(plan.hide) {
                    eprintln!("Impossible de retirer des rappels : {e}");
                }
            }
            for id in &plan.forget {
                shown.remove(id);
            }
            #[cfg(mobile)]
            mobile::schedule(&app, &state, &due, &_upcoming, &mut scheduled);

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
        .title(title(task))
        .body(body(task));
    #[cfg(mobile)]
    let builder = builder.ongoing();
    builder.show()
}

/// Les rappels confiés à Android qui n'ont plus lieu d'être : tâche faite,
/// supprimée ou reportée. Ceux dont l'échéance vient de passer ne sont pas
/// annulés (annuler retirerait aussi la notification affichée).
#[cfg_attr(not(mobile), allow(dead_code))]
fn stale_schedules(
    scheduled: &HashMap<String, i64>,
    due: &[Task],
    upcoming: &[Task],
) -> (Vec<String>, Vec<String>) {
    let mut cancel = Vec::new();
    let mut fired = Vec::new();
    for (id, at) in scheduled {
        if upcoming
            .iter()
            .any(|t| &t.id == id && t.due_at == Some(*at))
        {
            continue;
        }
        if due.iter().any(|t| &t.id == id && t.due_at == Some(*at)) {
            fired.push(id.clone());
        } else {
            cancel.push(id.clone());
        }
    }
    cancel.sort();
    fired.sort();
    (cancel, fired)
}

#[cfg(mobile)]
mod mobile {
    use std::collections::{HashMap, HashSet};

    use couple_core::Task;
    use tauri::{plugin::PermissionState, AppHandle};
    use tauri_plugin_notification::{NotificationExt, Schedule};

    use super::{body, notification_id, stale_schedules, title};
    use crate::{now_ms, store::AppState};

    const FILE: &str = "rappels-programmes.json";

    /// Notifications de l'appli actuellement affichées par Android.
    pub fn visible(app: &AppHandle) -> Option<HashSet<i32>> {
        match app.notification().active() {
            Ok(list) => Some(list.iter().map(|n| n.id()).collect()),
            Err(e) => {
                eprintln!("Liste des notifications indisponible : {e}");
                None
            }
        }
    }

    fn save(state: &AppState, scheduled: &HashMap<String, i64>) {
        let result = serde_json::to_vec(scheduled)
            .map_err(std::io::Error::other)
            .and_then(|bytes| crate::store::write_atomic(&state.data_dir.join(FILE), &bytes));
        if let Err(e) = result {
            eprintln!("Impossible d'enregistrer les rappels programmés : {e}");
        }
    }

    /// Demande l'autorisation d'afficher des notifications (Android 13 et
    /// plus), puis annule les rappels programmés par un lancement précédent
    /// (sauf ceux déjà échus, dont la notification est peut-être affichée) :
    /// ils sont reprogrammés juste après à partir des tâches actuelles.
    pub fn prepare(app: &AppHandle, state: &AppState) -> HashMap<String, i64> {
        let notification = app.notification();
        let granted = match notification.permission_state() {
            Ok(PermissionState::Granted) => true,
            _ => matches!(
                notification.request_permission(),
                Ok(PermissionState::Granted)
            ),
        };
        if !granted {
            state.notice(
                "Les notifications sont désactivées pour Eve & Lucas : les rappels des tâches ne \
                 s'afficheront pas. Active-les dans les réglages du téléphone."
                    .to_owned(),
            );
        }

        let previous: HashMap<String, i64> = std::fs::read(state.data_dir.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let (tasks, due) = {
            let doc = state.tasks.lock();
            (doc.list(), doc.due(now_ms()))
        };
        let due_ids: HashSet<&str> = due.iter().map(|t| t.id.as_str()).collect();
        let ids: Vec<i32> = previous
            .keys()
            .map(String::as_str)
            .chain(tasks.iter().map(|t| t.id.as_str()))
            .filter(|id| !due_ids.contains(id))
            .map(notification_id)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        if !ids.is_empty() {
            if let Err(e) = notification.cancel(ids) {
                eprintln!("Impossible d'annuler les anciens rappels : {e}");
            }
        }
        let scheduled = HashMap::new();
        save(state, &scheduled);
        scheduled
    }

    /// Confie à Android les échéances à venir. `scheduled` : tâche ->
    /// échéance déjà programmée.
    pub fn schedule(
        app: &AppHandle,
        state: &AppState,
        due: &[Task],
        upcoming: &[Task],
        scheduled: &mut HashMap<String, i64>,
    ) {
        let notification = app.notification();
        let before = scheduled.clone();
        let (cancel, fired) = stale_schedules(scheduled, due, upcoming);
        if !cancel.is_empty() {
            let ids = cancel.iter().map(|id| notification_id(id)).collect();
            if let Err(e) = notification.cancel(ids) {
                eprintln!("Impossible d'annuler des rappels : {e}");
            }
        }
        for id in cancel.iter().chain(&fired) {
            scheduled.remove(id);
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
                .title(title(task))
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
        if *scheduled != before {
            save(state, scheduled);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, due_at: i64) -> Task {
        Task {
            id: id.to_owned(),
            title: id.to_owned(),
            author: "Eve".to_owned(),
            assignee: String::new(),
            due_at: Some(due_at),
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
    fn pc_shows_then_repeats() {
        let due = vec![task("a", 0)];
        assert_eq!(plan(&due, &HashMap::new(), None, 10).show, due);
        let shown = HashMap::from([("a".to_owned(), 10)]);
        assert!(plan(&due, &shown, None, 10 + REPEAT_MS - 1).show.is_empty());
        assert_eq!(plan(&due, &shown, None, 10 + REPEAT_MS).show, due);
    }

    #[test]
    fn phone_keeps_visible_notification_and_brings_back_a_swiped_one() {
        let due = vec![task("a", 0)];
        let id = notification_id("a");
        let shown = HashMap::from([("a".to_owned(), 10)]);
        let visible = HashSet::from([id]);
        // Toujours affichée : rien à faire, même longtemps après.
        assert_eq!(
            plan(&due, &shown, Some(&visible), 10 + 10 * REPEAT_MS),
            Plan::default()
        );
        // Balayée : elle revient, mais pas tout de suite.
        let none = HashSet::new();
        assert!(plan(&due, &shown, Some(&none), 10 + REPEAT_MS - 1)
            .show
            .is_empty());
        assert_eq!(plan(&due, &shown, Some(&none), 10 + REPEAT_MS).show, due);
        // Déjà affichée par Android (rappel programmé) : pas de doublon.
        assert!(plan(&due, &HashMap::new(), Some(&visible), 10)
            .show
            .is_empty());
    }

    #[test]
    fn done_or_deleted_task_is_hidden() {
        let shown = HashMap::from([("a".to_owned(), 10), ("b".to_owned(), 10)]);
        let p = plan(&[task("b", 0)], &shown, None, 20);
        assert!(p.show.is_empty());
        assert_eq!(p.hide, vec![notification_id("a")]);
        assert_eq!(p.forget, vec!["a".to_owned()]);
    }

    #[test]
    fn phone_hides_leftovers_from_a_previous_launch() {
        // Notification restée d'un lancement précédent pour une tâche faite
        // ailleurs entre-temps : inconnue de `shown`, mais visible.
        let leftover = notification_id("faite");
        let visible = HashSet::from([leftover, notification_id("b")]);
        let p = plan(&[task("b", 0)], &HashMap::new(), Some(&visible), 20);
        assert_eq!(p.hide, vec![leftover]);
        assert!(p.show.is_empty());
    }

    #[test]
    fn schedules_that_just_fired_are_not_cancelled() {
        let scheduled = HashMap::from([
            ("echue".to_owned(), 100),
            ("faite".to_owned(), 300),
            ("reportee".to_owned(), 300),
            ("a-venir".to_owned(), 400),
        ]);
        let due = vec![task("echue", 100)];
        let upcoming = vec![task("reportee", 500), task("a-venir", 400)];
        let (cancel, fired) = stale_schedules(&scheduled, &due, &upcoming);
        assert_eq!(cancel, vec!["faite".to_owned(), "reportee".to_owned()]);
        assert_eq!(fired, vec!["echue".to_owned()]);
    }

    #[test]
    fn body_names_the_assignee() {
        let mut t = task("a", 0);
        assert_eq!(body(&t), "À faire ensemble");
        t.assignee = "Lucas".to_owned();
        assert_eq!(body(&t), "Pour Lucas");
        assert_eq!(title(&t), "Rappel : a");
    }
}
