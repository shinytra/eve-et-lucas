//! Commandes Tauri des tâches communes.

use std::sync::Arc;

use couple_core::{notify::Event, Task};
use tauri::State;

use crate::{now_ms, store::AppState};

/// Prévient la synchro, les rappels et le widget qu'une tâche a changé.
fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
    state.reminders_wanted.notify_one();
    state.widget_wanted.notify_one();
}

#[tauri::command]
pub fn list_tasks(state: State<'_, Arc<AppState>>) -> Vec<Task> {
    let doc = state.tasks.lock();
    let unreadable = doc.unreadable_count();
    if unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les tâches");
    }
    doc.list()
}

#[tauri::command]
pub fn add_task(
    state: State<'_, Arc<AppState>>,
    author: String,
    title: String,
    assignee: String,
    due_at: Option<i64>,
) -> Result<Task, String> {
    let task = state.tasks.change("d'ajouter la tâche", |doc| {
        doc.add(&author, &title, &assignee, due_at, now_ms())
    })?;
    changed(&state);
    crate::notify::announce(&state, |me, at| Event::task_added(&task, &author, me, at));
    Ok(task)
}

#[tauri::command]
pub fn update_task(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    assignee: String,
    due_at: Option<i64>,
) -> Result<Task, String> {
    let task = state.tasks.change("de modifier la tâche", |doc| {
        doc.update(&id, &title, &assignee, due_at, now_ms())
    })?;
    changed(&state);
    Ok(task)
}

#[tauri::command]
pub fn set_task_done(
    state: State<'_, Arc<AppState>>,
    id: String,
    done: bool,
    by: String,
) -> Result<Task, String> {
    // Lu dans la même modification : deux clics rapprochés n'annoncent qu'une fois.
    let (task, newly_done) = state.tasks.change("de cocher la tâche", |doc| {
        let was_done = doc.get(&id).is_ok_and(|t| t.done);
        doc.set_done(&id, done, &by, now_ms())
            .map(|t| (t, done && !was_done))
    })?;
    changed(&state);
    if newly_done {
        crate::notify::announce(&state, |me, at| Event::task_done(&task, &by, me, at));
    }
    Ok(task)
}

#[tauri::command]
pub fn delete_task(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .tasks
        .change("de supprimer la tâche", |doc| doc.delete(&id))?;
    changed(&state);
    Ok(())
}
