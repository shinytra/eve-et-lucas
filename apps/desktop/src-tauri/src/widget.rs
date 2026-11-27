//! Widget d'écran d'accueil (téléphone seulement).
//!
//! L'appli envoie au widget un résumé tout prêt (`couple_core::widget`) à
//! chaque changement des tâches, des arrivées, des présences ou des dates importantes. Les tâches cochées sur le
//! widget sont notées par Android, même appli fermée, puis appliquées ici
//! au nom de la personne qui porte le téléphone.

use std::sync::Arc;

use tauri::AppHandle;

use crate::store::AppState;

/// Lance la mise à jour du widget (sans effet sur PC).
#[cfg_attr(not(mobile), allow(unused_variables))]
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    #[cfg(mobile)]
    phone::start(app, state);
}

#[cfg(mobile)]
mod phone {
    use std::{sync::Arc, time::Duration};

    use couple_core::widget::{apply_action, snapshot, WidgetAction};
    use tauri::AppHandle;
    use tauri_plugin_home_widget::HomeWidgetExt;

    use crate::{now_ms, store::AppState};

    pub fn start(app: &AppHandle, state: Arc<AppState>) {
        {
            let state = state.clone();
            if let Err(e) = app
                .home_widget()
                .on_action(move |_| state.widget_wanted.notify_one())
            {
                eprintln!("Écoute du widget impossible : {e}");
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // Dernier contenu envoyé, sans la date de calcul.
            let mut sent: Option<String> = None;
            loop {
                let (a, s, last) = (app.clone(), state.clone(), sent.take());
                sent = tauri::async_runtime::spawn_blocking(move || {
                    ingest(&a, &s);
                    refresh(&a, &s, last)
                })
                .await
                .unwrap_or(None);
                // Toutes les minutes au moins : une tâche passe « en retard »,
                // une arrivée finit par disparaître, minuit change la prochaine date.
                tokio::select! {
                    () = state.widget_wanted.notified() => {}
                    () = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
            }
        });
    }

    /// Envoie le résumé au widget s'il a changé ; renvoie ce qui est affiché.
    fn refresh(app: &AppHandle, state: &AppState, last: Option<String>) -> Option<String> {
        let now = now_ms();
        // Un verrou à la fois.
        let tasks = state.tasks.lock().list();
        let arrivals = state.arrivals.lock().list();
        let dates = state.dates.lock().upcoming(crate::dates::local_today().0);
        let presence = state.presence.lock().current(now);
        let me = crate::arrivals::load_settings(state).who;
        let mut data = snapshot(&tasks, &arrivals, &dates, &presence, &me, now);
        let key = {
            data.updated_at = 0;
            serde_json::to_string(&data).ok()?
        };
        if last.as_ref() == Some(&key) {
            return last;
        }
        data.updated_at = now;
        let json = serde_json::to_string(&data).ok()?;
        match app.home_widget().update(json) {
            Ok(()) => Some(key),
            Err(e) => {
                eprintln!("Impossible de mettre à jour le widget : {e}");
                last
            }
        }
    }

    /// Applique les tâches cochées sur le widget, puis les confirme à Android.
    fn ingest(app: &AppHandle, state: &AppState) {
        let actions = match app.home_widget().peek_actions() {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Actions du widget indisponibles : {e}");
                return;
            }
        };
        if actions.is_empty() {
            return;
        }
        let who = match crate::arrivals::load_settings(state).who {
            who if !who.trim().is_empty() => who,
            _ => state.device_name(),
        };
        let mut done = Vec::new();
        let mut changed = false;
        for action in actions {
            let action = WidgetAction {
                id: action.id,
                kind: action.kind,
                task_id: action.task_id,
                at: action.at,
            };
            match state
                .tasks
                .change("de cocher la tâche", |d| apply_action(d, &action, &who))
            {
                Ok(c) => {
                    changed |= c;
                    // Cochée depuis le widget : l'autre est prévenu aussi.
                    let task = if c {
                        state.tasks.lock().get(&action.task_id).ok()
                    } else {
                        None
                    };
                    if let Some(task) = task {
                        crate::notify::announce(state, |me, at| {
                            couple_core::notify::Event::task_done(&task, &who, me, at)
                        });
                    }
                    done.push(action.id);
                }
                // Android la garde : nouvel essai au prochain passage.
                Err(e) => {
                    state.notice(e);
                    break;
                }
            }
        }
        if !done.is_empty() {
            if let Err(e) = app.home_widget().ack_actions(done) {
                eprintln!("Impossible de confirmer les actions du widget : {e}");
            }
        }
        if changed {
            state.sync_wanted.notify_one();
            couple_sync::Store::changed(state, &[crate::store::TASKS_DOC.to_owned()]);
        }
    }
}
