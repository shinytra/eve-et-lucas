//! Dates importantes : commandes Tauri et rappels.
//!
//! Le cœur calcule les rappels jour par jour ; ici on choisit l'heure (9 h,
//! heure locale de l'appareil) et on affiche les notifications :
//! - à 9 h, ou au lancement de l'appli si elle était fermée à 9 h, chaque
//!   rappel du jour s'affiche une seule fois (clés gardées dans
//!   `rappels-dates.json`) ;
//! - sur téléphone, les rappels des 60 prochains jours sont aussi confiés à
//!   Android, pour s'afficher même appli fermée.

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use couple_core::{DateInput, DateReminder, Day, ImportantDate, UpcomingDate};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_notification::NotificationExt;

use crate::{now_ms, reminders::notification_id, store::AppState};

/// Heure locale des rappels.
const REMIND_HOUR: u32 = 9;
/// Vérification régulière, en plus des réveils après chaque modification.
const CHECK_EVERY: Duration = Duration::from_secs(60);
/// Sur téléphone : rappels confiés à Android à l'avance, en jours.
#[cfg_attr(not(mobile), allow(dead_code))]
const SCHEDULE_DAYS: i64 = 60;
const FILE: &str = "rappels-dates.json";

/// La date du jour et l'heure, dans le fuseau de l'appareil.
pub(crate) fn local_today() -> (Day, u32) {
    let now = jiff::ZonedDateTime::now();
    let day = Day::new(
        i32::from(now.year()),
        u32::try_from(now.month()).unwrap_or(1),
        u32::try_from(now.day()).unwrap_or(1),
    )
    .unwrap_or(Day {
        year: 1970,
        month: 1,
        day: 1,
    });
    (day, u32::try_from(now.hour()).unwrap_or(0))
}

/// Le moment où le rappel d'un jour s'affiche (millisecondes Unix).
#[cfg_attr(not(mobile), allow(dead_code))]
fn remind_at(day: Day) -> Option<i64> {
    let date = jiff::civil::Date::new(
        i16::try_from(day.year).ok()?,
        i8::try_from(day.month).ok()?,
        i8::try_from(day.day).ok()?,
    )
    .ok()?;
    let hour = i8::try_from(REMIND_HOUR).ok()?;
    date.at(hour, 0, 0, 0)
        .to_zoned_datetime(jiff::tz::TimeZone::system())
        .ok()
        .map(|t| t.timestamp().as_millisecond())
}

fn notification_key(reminder: &DateReminder) -> i32 {
    notification_id(&format!("date:{}", reminder.key()))
}

/// Prévient la synchro, les rappels et le widget qu'une date a changé.
fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
    state.dates_wanted.notify_one();
    state.widget_wanted.notify_one();
}

/// Dernier nombre d'entrées illisibles signalé (pour ne pas le répéter).
static UNREADABLE: AtomicUsize = AtomicUsize::new(0);

#[tauri::command]
pub fn list_dates(state: State<'_, Arc<AppState>>) -> Vec<UpcomingDate> {
    let doc = state.dates.lock();
    let unreadable = doc.unreadable_count();
    if UNREADABLE.swap(unreadable, Ordering::Relaxed) != unreadable && unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les dates");
    }
    doc.upcoming(local_today().0)
}

#[tauri::command]
pub fn add_date(
    state: State<'_, Arc<AppState>>,
    author: String,
    input: DateInput,
) -> Result<ImportantDate, String> {
    let date = state.dates.change("d'ajouter la date", |doc| {
        doc.add(&author, &input, now_ms())
    })?;
    changed(&state);
    Ok(date)
}

#[tauri::command]
pub fn update_date(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: DateInput,
) -> Result<ImportantDate, String> {
    let date = state.dates.change("de modifier la date", |doc| {
        doc.update(&id, &input, now_ms())
    })?;
    changed(&state);
    Ok(date)
}

#[tauri::command]
pub fn delete_date(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .dates
        .change("de supprimer la date", |doc| doc.delete(&id))?;
    changed(&state);
    Ok(())
}

/// Ce dont l'appareil se souvient entre deux lancements.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Memory {
    /// Rappels déjà affichés : clé -> jour du rappel.
    shown: BTreeMap<String, Day>,
    /// Téléphone : rappels confiés à Android.
    scheduled: BTreeMap<String, Scheduled>,
}

/// Un rappel confié à Android.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Scheduled {
    /// Moment prévu (millisecondes Unix).
    at: i64,
    /// Titre et texte programmés : s'ils changent (date renommée…), le rappel
    /// est reprogrammé.
    text: String,
}

fn text(reminder: &DateReminder) -> String {
    format!("{}\n{}", reminder.title, reminder.body)
}

impl Memory {
    /// Un fichier illisible est mis de côté avec un avertissement ; au pire,
    /// les rappels du jour s'affichent une deuxième fois.
    fn load(state: &AppState) -> Self {
        let path = state.data_dir.join(FILE);
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice(&bytes) {
            Ok(memory) => memory,
            Err(e) => {
                let backup = path.with_extension(format!("corrompu-{}", now_ms()));
                let moved = std::fs::rename(&path, &backup).is_ok();
                state.notice(format!(
                    "Liste des rappels de dates déjà affichés illisible ({e}) : {} \
                     Les rappels du jour peuvent s'afficher une deuxième fois.",
                    if moved {
                        format!("mise de côté dans {}.", backup.display())
                    } else {
                        "ignorée.".to_owned()
                    }
                ));
                Self::default()
            }
        }
    }

    fn save(&self, state: &AppState) {
        let result = serde_json::to_vec(self)
            .map_err(std::io::Error::other)
            .and_then(|bytes| crate::store::write_atomic(&state.data_dir.join(FILE), &bytes));
        if let Err(e) = result {
            eprintln!("Impossible d'enregistrer les rappels des dates : {e}");
        }
    }

    /// Oublie ce qui date d'avant hier (il n'y a plus rien à éviter).
    fn prune(&mut self, today: Day, now: i64) {
        let limit = today.add_days(-1);
        self.shown.retain(|_, on| *on >= limit);
        self.scheduled
            .retain(|_, s| s.at > now - 48 * 60 * 60 * 1000);
    }

    /// Le rappel a-t-il déjà été affiché (par l'appli ou par Android) ?
    fn handled(&self, key: &str, now: i64) -> bool {
        self.shown.contains_key(key) || self.scheduled.get(key).is_some_and(|s| s.at <= now)
    }
}

/// Rappels du jour à afficher maintenant.
fn to_show<'a>(
    today_reminders: &'a [DateReminder],
    memory: &Memory,
    hour: u32,
    now: i64,
) -> Vec<&'a DateReminder> {
    if hour < REMIND_HOUR {
        return Vec::new();
    }
    today_reminders
        .iter()
        .filter(|r| !memory.handled(&r.key(), now))
        .collect()
}

/// Lance la boucle des rappels des dates.
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut memory = Memory::load(&state);
        let mut failed_once = false;
        loop {
            let before_pass = serde_json::to_vec(&memory).ok();
            let now = now_ms();
            let (today, hour) = local_today();
            memory.prune(today, now);
            let today_reminders = state.dates.lock().reminders(today, today);
            for reminder in to_show(&today_reminders, &memory, hour, now) {
                match show(&app, reminder) {
                    Ok(()) => {
                        memory.shown.insert(reminder.key(), reminder.on);
                    }
                    Err(e) if !failed_once => {
                        failed_once = true;
                        state.notice(format!(
                            "Impossible d'afficher le rappel « {} » : {e}",
                            reminder.title
                        ));
                    }
                    Err(e) => eprintln!("Rappel « {} » toujours impossible : {e}", reminder.title),
                }
            }
            #[cfg(mobile)]
            {
                let horizon = state
                    .dates
                    .lock()
                    .reminders(today, today.add_days(SCHEDULE_DAYS));
                mobile::schedule(&app, &horizon, &mut memory, now);
            }
            if serde_json::to_vec(&memory).ok() != before_pass {
                memory.save(&state);
            }
            tokio::select! {
                () = state.dates_wanted.notified() => {}
                () = tokio::time::sleep(CHECK_EVERY) => {}
            }
        }
    });
}

fn show(app: &AppHandle, reminder: &DateReminder) -> tauri_plugin_notification::Result<()> {
    app.notification()
        .builder()
        .id(notification_key(reminder))
        .title(&reminder.title)
        .body(&reminder.body)
        .show()
}

/// Rappels confiés à Android à annuler (date supprimée, déplacée ou
/// renommée) et à (re)programmer. Ceux dont le moment est passé ne sont pas
/// annulés : cela retirerait la notification affichée.
#[cfg_attr(not(mobile), allow(dead_code))]
fn schedule_plan<'a>(
    wanted: &'a [DateReminder],
    memory: &Memory,
    now: i64,
) -> (Vec<String>, Vec<(&'a DateReminder, i64)>) {
    let wanted: Vec<(&DateReminder, Scheduled)> = wanted
        .iter()
        .filter_map(|r| remind_at(r.on).map(|at| (r, Scheduled { at, text: text(r) })))
        .filter(|(_, s)| s.at > now)
        .collect();
    let cancel = memory
        .scheduled
        .iter()
        .filter(|(key, s)| s.at > now && !wanted.iter().any(|(r, w)| &r.key() == *key && w == *s))
        .map(|(key, _)| key.clone())
        .collect();
    let add = wanted
        .into_iter()
        .filter(|(r, s)| {
            memory.scheduled.get(&r.key()) != Some(s) && !memory.handled(&r.key(), now)
        })
        .map(|(r, s)| (r, s.at))
        .collect();
    (cancel, add)
}

#[cfg(mobile)]
mod mobile {
    use couple_core::DateReminder;
    use tauri::AppHandle;
    use tauri_plugin_notification::{NotificationExt, Schedule};

    use super::{notification_id, notification_key, schedule_plan, Memory};

    pub fn schedule(app: &AppHandle, wanted: &[DateReminder], memory: &mut Memory, now: i64) {
        let notification = app.notification();
        let (cancel, add) = schedule_plan(wanted, memory, now);
        if !cancel.is_empty() {
            let ids = cancel
                .iter()
                .map(|key| notification_id(&format!("date:{key}")))
                .collect();
            if let Err(e) = notification.cancel(ids) {
                eprintln!("Impossible d'annuler des rappels de dates : {e}");
            }
            for key in &cancel {
                memory.scheduled.remove(key);
            }
        }
        for (reminder, at) in add {
            let Ok(date) =
                time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(at) * 1_000_000)
            else {
                continue;
            };
            let result = notification
                .builder()
                .id(notification_key(reminder))
                .title(&reminder.title)
                .body(&reminder.body)
                .schedule(Schedule::At {
                    date,
                    repeating: false,
                    allow_while_idle: true,
                })
                .show();
            match result {
                Ok(()) => {
                    memory.scheduled.insert(
                        reminder.key(),
                        super::Scheduled {
                            at,
                            text: super::text(reminder),
                        },
                    );
                }
                Err(e) => eprintln!(
                    "Impossible de programmer le rappel « {} » : {e}",
                    reminder.title
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use couple_core::{dates::KIND_BIRTHDAY, DatesDoc};

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::new(y, m, d).unwrap()
    }

    fn doc_with_birthday() -> DatesDoc {
        let mut doc = DatesDoc::new();
        doc.add(
            "Lucas",
            &DateInput {
                title: "Anniversaire d'Eve".into(),
                kind: KIND_BIRTHDAY.into(),
                month: 12,
                day: 3,
                year: None,
                yearly: true,
                remind_days: 7,
            },
            1,
        )
        .unwrap();
        doc
    }

    #[test]
    fn shows_once_from_nine_oclock() {
        let today = day(2026, 11, 26);
        let reminders = doc_with_birthday().reminders(today, today);
        assert_eq!(reminders.len(), 1);
        let mut memory = Memory::default();
        assert!(to_show(&reminders, &memory, REMIND_HOUR - 1, 0).is_empty());
        assert_eq!(to_show(&reminders, &memory, REMIND_HOUR, 0).len(), 1);
        memory.shown.insert(reminders[0].key(), today);
        assert!(to_show(&reminders, &memory, 23, 0).is_empty());
    }

    #[test]
    fn reminder_fired_by_android_is_not_shown_again() {
        let today = day(2026, 11, 26);
        let reminders = doc_with_birthday().reminders(today, today);
        let mut memory = Memory::default();
        memory.scheduled.insert(
            reminders[0].key(),
            Scheduled {
                at: 1_000,
                text: String::new(),
            },
        );
        // Moment prévu passé : Android l'a affiché, l'appli ne le refait pas.
        assert!(to_show(&reminders, &memory, 10, 2_000).is_empty());
        // Pas encore passé (horloge changée entre-temps) : l'appli l'affiche.
        assert_eq!(to_show(&reminders, &memory, 10, 500).len(), 1);
    }

    #[test]
    fn prune_forgets_old_entries() {
        let today = day(2026, 11, 26);
        let mut memory = Memory::default();
        memory.shown.insert("vieux".into(), day(2026, 11, 20));
        memory.shown.insert("hier".into(), day(2026, 11, 25));
        let at = |at| Scheduled {
            at,
            text: String::new(),
        };
        memory.scheduled.insert("passe".into(), at(0));
        memory.scheduled.insert("futur".into(), at(10_000_000_000));
        memory.prune(today, 1_000_000_000);
        assert_eq!(memory.shown.keys().collect::<Vec<_>>(), vec!["hier"]);
        assert_eq!(memory.scheduled.keys().collect::<Vec<_>>(), vec!["futur"]);
    }

    #[test]
    fn schedule_plan_adds_and_cancels() {
        let today = day(2026, 11, 26);
        let doc = doc_with_birthday();
        let wanted = doc.reminders(today, today.add_days(SCHEDULE_DAYS));
        // Le rappel d'aujourd'hui (7 jours avant) et celui du jour J.
        assert_eq!(wanted.len(), 2);
        let now = remind_at(today).unwrap() - 1;
        let mut memory = Memory::default();
        let (cancel, add) = schedule_plan(&wanted, &memory, now);
        assert!(cancel.is_empty());
        assert_eq!(add.len(), 2);
        for (r, at) in add {
            memory
                .scheduled
                .insert(r.key(), Scheduled { at, text: text(r) });
        }
        // Déjà programmés : rien à faire.
        let (cancel, add) = schedule_plan(&wanted, &memory, now);
        assert!(cancel.is_empty() && add.is_empty());
        // Date renommée : les deux rappels sont annulés puis reprogrammés.
        let mut renamed = wanted.clone();
        for r in &mut renamed {
            r.title.push_str(" (renommée)");
        }
        let (cancel, add) = schedule_plan(&renamed, &memory, now);
        assert_eq!((cancel.len(), add.len()), (2, 2));
        // Date supprimée : les rappels à venir sont annulés.
        let (cancel, add) = schedule_plan(&[], &memory, now);
        assert_eq!(cancel.len(), 2);
        assert!(add.is_empty());
        // Après 9 h, celui du jour est passé : il n'est plus annulé.
        let (cancel, _) = schedule_plan(&[], &memory, now + 2);
        assert_eq!(cancel.len(), 1);
    }

    #[test]
    fn memory_round_trip() {
        let mut memory = Memory::default();
        memory
            .shown
            .insert("a@2026-11-26".into(), day(2026, 11, 26));
        memory.scheduled.insert(
            "b@2026-12-03-j0".into(),
            Scheduled {
                at: 42,
                text: "Titre\nTexte".into(),
            },
        );
        let json = serde_json::to_vec(&memory).unwrap();
        assert_eq!(serde_json::from_slice::<Memory>(&json).unwrap(), memory);
    }
}
