//! Dates importantes du couple (anniversaire de couple, anniversaires,
//! fêtes…), stockées dans un document Automerge.
//!
//! Même principe que les tâches : la racine est une table `id -> ImportantDate`
//! avec des identifiants UUID v4. Une date se répète chaque année ou n'a lieu
//! qu'une fois. Le cœur ne connaît pas le fuseau horaire : l'appli lui donne
//! la date du jour ([`Day`]) dans l'heure locale de l'appareil, et c'est elle
//! qui choisit l'heure des rappels.

use std::fmt;

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Longueur maximale du nom d'une date, en caractères.
pub const MAX_TITLE: usize = 100;
/// Rappel au plus tôt 60 jours avant.
pub const MAX_REMIND_DAYS: u32 = 60;
/// Années acceptées.
pub const MIN_YEAR: i32 = 1900;
pub const MAX_YEAR: i32 = 2200;

/// Types de dates. Un type inconnu (autre version de l'appli) est affiché
/// comme [`KIND_OTHER`].
pub const KIND_COUPLE: &str = "couple";
pub const KIND_BIRTHDAY: &str = "anniversaire";
pub const KIND_HOLIDAY: &str = "fete";
pub const KIND_OTHER: &str = "autre";
const KINDS: [&str; 4] = [KIND_COUPLE, KIND_BIRTHDAY, KIND_HOLIDAY, KIND_OTHER];

/// Un jour du calendrier, sans heure ni fuseau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Day {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Day {
    /// Le jour, s'il existe (le 29 février seulement les années bissextiles).
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        ((1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month))
            .then_some(Self { year, month, day })
    }

    /// Nombre de jours depuis le 1er janvier 1970 (algorithme de H. Hinnant).
    pub fn to_days(self) -> i64 {
        let y = i64::from(self.year) - i64::from(self.month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(self.month);
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// Inverse de [`Day::to_days`].
    pub fn from_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        // Les valeurs sont bornées par construction (mois 1-12, jour 1-31).
        Self {
            year: i32::try_from(year).unwrap_or(i32::MAX),
            month: u32::try_from(month).unwrap_or(1),
            day: u32::try_from(day).unwrap_or(1),
        }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.to_days() + n)
    }
}

impl fmt::Display for Day {
    /// « 14 février 2027 ».
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.day, month_name(self.month), self.year)
    }
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ];
    usize::try_from(month)
        .ok()
        .and_then(|m| NAMES.get(m.wrapping_sub(1)))
        .copied()
        .unwrap_or("?")
}

/// Une date importante.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportantDate {
    pub id: String,
    pub title: String,
    /// [`KIND_COUPLE`], [`KIND_BIRTHDAY`], [`KIND_HOLIDAY`] ou [`KIND_OTHER`].
    pub kind: String,
    pub month: u32,
    pub day: u32,
    /// Date qui revient chaque année : année de départ, pour compter les
    /// années (« 5 ans »), ou aucune. Date unique : son année (obligatoire).
    pub year: Option<i32>,
    pub yearly: bool,
    /// Rappel ce nombre de jours avant (0 : seulement le jour même).
    pub remind_days: u32,
    /// Prénom de la personne qui l'a ajoutée.
    pub author: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Forme enregistrée dans le document. Le jour, le mois, l'année et la
/// répétition forment une seule valeur (`when`, par exemple
/// `annuel:2021-06-12`, `annuel:02-29` ou `unique:2027-09-05`) : deux
/// modifications simultanées ne peuvent pas les mélanger en une date
/// impossible, Automerge garde l'une ou l'autre en entier.
#[derive(Debug, Clone, PartialEq, Eq, Hydrate, Reconcile)]
struct Stored {
    id: String,
    title: String,
    kind: String,
    when: String,
    remind_days: u32,
    author: String,
    created_at: i64,
    updated_at: i64,
}

const YEARLY: &str = "annuel";
const ONCE: &str = "unique";

fn encode_when(date: &ImportantDate) -> String {
    let repeat = if date.yearly { YEARLY } else { ONCE };
    match date.year {
        Some(y) => format!("{repeat}:{y:04}-{:02}-{:02}", date.month, date.day),
        None => format!("{repeat}:{:02}-{:02}", date.month, date.day),
    }
}

/// `(mois, jour, année, chaque année)`, ou rien si la valeur est illisible.
fn decode_when(when: &str) -> Option<(u32, u32, Option<i32>, bool)> {
    let (repeat, rest) = when.split_once(':')?;
    let yearly = match repeat {
        YEARLY => true,
        ONCE => false,
        _ => return None,
    };
    let parts: Vec<&str> = rest.split('-').collect();
    let (year, month, day) = match parts.as_slice() {
        [y, m, d] => (Some(y.parse().ok()?), m, d),
        [m, d] => (None, m, d),
        _ => return None,
    };
    Some((month.parse().ok()?, day.parse().ok()?, year, yearly))
}

impl From<&ImportantDate> for Stored {
    fn from(d: &ImportantDate) -> Self {
        Self {
            id: d.id.clone(),
            title: d.title.clone(),
            kind: d.kind.clone(),
            when: encode_when(d),
            remind_days: d.remind_days,
            author: d.author.clone(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

impl Stored {
    /// La date, si elle est valide. Un rappel trop lointain (autre version de
    /// l'appli) est ramené à [`MAX_REMIND_DAYS`].
    fn into_date(self) -> Option<ImportantDate> {
        let (month, day, year, yearly) = decode_when(&self.when)?;
        validate_day(month, day, year, yearly).ok()?;
        Some(ImportantDate {
            id: self.id,
            title: self.title,
            kind: self.kind,
            month,
            day,
            year,
            yearly,
            remind_days: self.remind_days.min(MAX_REMIND_DAYS),
            author: self.author,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Ce que la personne saisit pour une date.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateInput {
    pub title: String,
    pub kind: String,
    pub month: u32,
    pub day: u32,
    pub year: Option<i32>,
    pub yearly: bool,
    pub remind_days: u32,
}

impl ImportantDate {
    /// Type affichable : un type inconnu devient [`KIND_OTHER`].
    pub fn kind(&self) -> &str {
        if KINDS.contains(&self.kind.as_str()) {
            &self.kind
        } else {
            KIND_OTHER
        }
    }

    /// Le jour où la date tombe l'année `year` (le 29 février devient le 28
    /// les années non bissextiles), ou aucun si elle n'a pas lieu cette année.
    fn in_year(&self, year: i32) -> Option<Day> {
        if !self.yearly && self.year != Some(year) {
            return None;
        }
        if self.yearly && self.year.is_some_and(|start| year < start) {
            return None;
        }
        Day::new(year, self.month, self.day)
            .or_else(|| (self.month == 2 && self.day == 29).then(|| Day::new(year, 2, 28))?)
    }

    /// Prochaine fois que la date tombe, aujourd'hui compris.
    pub fn next_on_or_after(&self, today: Day) -> Option<Day> {
        if self.yearly {
            // Année de départ dans le futur : la première fois est cette année-là.
            let from = self.year.map_or(today.year, |y| y.max(today.year));
            (from..=from.saturating_add(1))
                .filter_map(|y| self.in_year(y))
                .find(|d| *d >= today)
        } else {
            self.year
                .and_then(|y| self.in_year(y))
                .filter(|d| *d >= today)
        }
    }

    /// Nombre d'années fêtées le jour `on` (« 5 ans »), si l'année de départ
    /// est connue et qu'il y en a au moins une.
    pub fn years_on(&self, on: Day) -> Option<i32> {
        if !self.yearly {
            return None;
        }
        self.year.map(|y| on.year - y).filter(|n| *n > 0)
    }

    /// Petit dessin du type de date (« 🎂 » pour un anniversaire…).
    pub fn emoji(&self) -> &'static str {
        match self.kind() {
            KIND_COUPLE => "💞",
            KIND_BIRTHDAY => "🎂",
            KIND_HOLIDAY => "🎉",
            _ => "📅",
        }
    }
}

/// Une date et sa prochaine occurrence, pour l'affichage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpcomingDate {
    #[serde(flatten)]
    pub date: ImportantDate,
    /// Prochaine fois (aujourd'hui compris) ; aucune pour une date unique passée.
    pub next: Option<Day>,
    /// Jours restants (0 : aujourd'hui).
    pub days_left: Option<i64>,
    /// Années fêtées ce jour-là.
    pub years: Option<i32>,
}

/// Un rappel à afficher un jour donné.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateReminder {
    pub date_id: String,
    /// Jour où le rappel doit s'afficher.
    pub on: Day,
    /// Jour de l'événement.
    pub event: Day,
    /// Jours entre le rappel et l'événement (0 : le jour même).
    pub days_before: u32,
    pub title: String,
    pub body: String,
}

impl DateReminder {
    /// Clé unique du rappel (pour ne l'afficher qu'une fois) : la date, le
    /// jour du rappel et le nombre de jours avant l'événement.
    pub fn key(&self) -> String {
        format!(
            "{}@{:04}-{:02}-{:02}-j{}",
            self.date_id, self.on.year, self.on.month, self.on.day, self.days_before
        )
    }
}

/// Le document des dates importantes. Comme pour [`crate::TasksDoc`], `Clone`
/// garde l'identifiant d'appareil : utiliser [`DatesDoc::fork`] pour simuler
/// un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct DatesDoc {
    doc: AutoCommit,
}

impl DatesDoc {
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

    /// Toutes les dates lisibles, sans ordre particulier (voir [`Self::upcoming`]).
    pub fn list(&self) -> Vec<ImportantDate> {
        let mut dates: Vec<ImportantDate> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        dates.sort_by(|a, b| a.id.cmp(&b.id));
        dates
    }

    /// Nombre d'entrées du document qui ne sont pas des dates valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    /// Une entrée venue d'un autre appareil n'est gardée que si elle est
    /// valide : sinon elle est ignorée, sans bloquer les autres.
    fn read(&self, key: &str) -> Option<ImportantDate> {
        hydrate_prop::<_, Stored, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|d| d.id == key)
            .and_then(Stored::into_date)
    }

    pub fn get(&self, id: &str) -> Result<ImportantDate> {
        self.read(id)
            .ok_or_else(|| Error::DateNotFound(id.to_owned()))
    }

    /// Ajoute une date et la renvoie.
    pub fn add(&mut self, author: &str, input: &DateInput, now_ms: i64) -> Result<ImportantDate> {
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let mut date = ImportantDate {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            kind: String::new(),
            month: 0,
            day: 0,
            year: None,
            yearly: true,
            remind_days: 0,
            author: author.to_owned(),
            created_at: now_ms,
            updated_at: now_ms,
        };
        apply(&mut date, input)?;
        reconcile_prop(&mut self.doc, ROOT, date.id.as_str(), Stored::from(&date))?;
        Ok(date)
    }

    /// Modifie une date.
    pub fn update(&mut self, id: &str, input: &DateInput, now_ms: i64) -> Result<ImportantDate> {
        let mut date = self.get(id)?;
        apply(&mut date, input)?;
        date.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, Stored::from(&date))?;
        Ok(date)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::DateNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }

    /// Toutes les dates avec leur prochaine occurrence : la plus proche en
    /// premier, puis les dates uniques passées (la plus récente en premier).
    pub fn upcoming(&self, today: Day) -> Vec<UpcomingDate> {
        let mut out: Vec<UpcomingDate> = self
            .list()
            .into_iter()
            .map(|date| {
                let next = date.next_on_or_after(today);
                UpcomingDate {
                    days_left: next.map(|d| d.to_days() - today.to_days()),
                    years: next.and_then(|d| date.years_on(d)),
                    next,
                    date,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            let key = |u: &UpcomingDate| (u.days_left.is_none(), u.days_left);
            key(a)
                .cmp(&key(b))
                .then_with(|| {
                    // Dates passées : la plus récente d'abord.
                    let past = |u: &UpcomingDate| (u.date.year, u.date.month, u.date.day);
                    past(b).cmp(&past(a))
                })
                .then_with(|| a.date.title.cmp(&b.date.title))
                .then_with(|| a.date.id.cmp(&b.date.id))
        });
        out
    }

    /// Rappels à afficher entre `from` et `to` (inclus) : `remind_days` jours
    /// avant chaque occurrence, puis le jour même.
    pub fn reminders(&self, from: Day, to: Day) -> Vec<DateReminder> {
        let mut out = Vec::new();
        for date in self.list() {
            let mut cursor = from;
            // Occurrences dont un rappel peut tomber dans l'intervalle.
            while let Some(event) = date.next_on_or_after(cursor) {
                if event.to_days() - i64::from(date.remind_days) > to.to_days() {
                    break;
                }
                let mut before = vec![0];
                if date.remind_days > 0 {
                    before.push(date.remind_days);
                }
                for days_before in before {
                    let on = event.add_days(-i64::from(days_before));
                    if on >= from && on <= to {
                        out.push(reminder(&date, on, event, days_before));
                    }
                }
                cursor = event.add_days(1);
            }
        }
        out.sort_by(|a, b| a.on.cmp(&b.on).then_with(|| a.key().cmp(&b.key())));
        out
    }

    pub fn merge(&mut self, other: &mut DatesDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for DatesDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

fn reminder(date: &ImportantDate, on: Day, event: Day, days_before: u32) -> DateReminder {
    let title = match days_before {
        0 => format!("{} Aujourd'hui : {}", date.emoji(), date.title),
        1 => format!("{} Demain : {}", date.emoji(), date.title),
        n => format!("{} Dans {n} jours : {}", date.emoji(), date.title),
    };
    let when = format!("{} {}", event.day, month_name(event.month));
    let body = match date.years_on(event) {
        Some(1) => format!("1 an le {when}"),
        Some(n) => format!("{n} ans le {when}"),
        None => format!("Le {when}"),
    };
    DateReminder {
        date_id: date.id.clone(),
        on,
        event,
        days_before,
        title,
        body,
    }
}

fn validate_day(month: u32, day: u32, year: Option<i32>, yearly: bool) -> Result<()> {
    if let Some(y) = year {
        if !(MIN_YEAR..=MAX_YEAR).contains(&y) {
            return Err(Error::InvalidDate(format!(
                "l'année doit être entre {MIN_YEAR} et {MAX_YEAR}"
            )));
        }
    }
    let valid = match (yearly, year) {
        (false, None) => {
            return Err(Error::InvalidDate(
                "une date qui ne revient pas chaque année doit avoir une année".to_owned(),
            ))
        }
        (false, Some(y)) => Day::new(y, month, day).is_some(),
        // Date annuelle : le 29 février est accepté (fêté le 28 sinon), mais
        // l'année de départ doit être bissextile dans ce cas.
        (true, Some(y)) => Day::new(y, month, day).is_some(),
        (true, None) => Day::new(2000, month, day).is_some(),
    };
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidDate("ce jour n'existe pas".to_owned()))
    }
}

fn apply(date: &mut ImportantDate, input: &DateInput) -> Result<()> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(Error::InvalidDate("il manque le nom".to_owned()));
    }
    if title.chars().count() > MAX_TITLE {
        return Err(Error::InvalidDate(format!(
            "le nom ne doit pas dépasser {MAX_TITLE} caractères"
        )));
    }
    // Un type inconnu (venu d'une version plus récente) est gardé tel quel
    // s'il ne change pas.
    if !KINDS.contains(&input.kind.as_str()) && input.kind != date.kind {
        return Err(Error::InvalidDate(format!("type inconnu : {}", input.kind)));
    }
    if input.remind_days > MAX_REMIND_DAYS {
        return Err(Error::InvalidDate(format!(
            "le rappel doit être au plus {MAX_REMIND_DAYS} jours avant"
        )));
    }
    validate_day(input.month, input.day, input.year, input.yearly)?;
    date.title = title.to_owned();
    date.kind = input.kind.clone();
    date.month = input.month;
    date.day = input.day;
    date.year = input.year;
    date.yearly = input.yearly;
    date.remind_days = input.remind_days;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    fn day(y: i32, m: u32, d: u32) -> Day {
        Day::new(y, m, d).unwrap()
    }

    fn input(title: &str, kind: &str, m: u32, d: u32, year: Option<i32>) -> DateInput {
        DateInput {
            title: title.to_owned(),
            kind: kind.to_owned(),
            month: m,
            day: d,
            year,
            yearly: true,
            remind_days: 0,
        }
    }

    #[test]
    fn calendar_round_trip() {
        assert_eq!(day(1970, 1, 1).to_days(), 0);
        assert_eq!(day(2000, 3, 1).to_days(), 11_017);
        assert_eq!(day(1969, 12, 31).to_days(), -1);
        for n in -800_000..800_000 {
            if n % 997 == 0 {
                assert_eq!(Day::from_days(n).to_days(), n);
            }
        }
        assert_eq!(day(2024, 2, 28).add_days(1), day(2024, 2, 29));
        assert_eq!(day(2025, 2, 28).add_days(1), day(2025, 3, 1));
        assert_eq!(day(2026, 12, 31).add_days(1), day(2027, 1, 1));
        assert!(Day::new(2025, 2, 29).is_none());
        assert!(Day::new(2000, 2, 29).is_some());
        assert!(Day::new(1900, 2, 29).is_none());
        assert!(Day::new(2025, 13, 1).is_none());
        assert!(Day::new(2025, 4, 31).is_none());
        assert_eq!(day(2027, 2, 14).to_string(), "14 février 2027");
    }

    #[test]
    fn add_update_delete() {
        let mut doc = DatesDoc::new();
        let date = doc
            .add(
                "Lucas",
                &input(" Notre anniversaire ", KIND_COUPLE, 6, 12, Some(2021)),
                1,
            )
            .unwrap();
        assert_eq!(date.title, "Notre anniversaire");
        assert_eq!(date.author, "Lucas");
        assert_eq!(doc.list(), vec![date.clone()]);

        let mut changed = input("Anniversaire d'Eve", KIND_BIRTHDAY, 3, 8, None);
        changed.remind_days = 7;
        let updated = doc.update(&date.id, &changed, 2).unwrap();
        assert_eq!(updated.kind, KIND_BIRTHDAY);
        assert_eq!(updated.year, None);
        assert_eq!(updated.remind_days, 7);
        assert_eq!(updated.created_at, 1);
        assert_eq!(updated.updated_at, 2);
        assert_eq!(doc.get(&date.id).unwrap(), updated);

        doc.delete(&date.id).unwrap();
        assert!(doc.list().is_empty());
        assert!(matches!(doc.get(&date.id), Err(Error::DateNotFound(_))));
        assert!(matches!(doc.delete(&date.id), Err(Error::DateNotFound(_))));
    }

    #[test]
    fn rejects_invalid_dates() {
        let mut doc = DatesDoc::new();
        let bad = |doc: &mut DatesDoc, i: DateInput| {
            assert!(
                matches!(doc.add("Eve", &i, 1), Err(Error::InvalidDate(_))),
                "{i:?}"
            )
        };
        bad(&mut doc, input(" ", KIND_COUPLE, 1, 1, None));
        bad(
            &mut doc,
            input(&"a".repeat(MAX_TITLE + 1), KIND_COUPLE, 1, 1, None),
        );
        bad(&mut doc, input("x", "inconnu", 1, 1, None));
        bad(&mut doc, input("x", KIND_OTHER, 2, 30, None));
        bad(&mut doc, input("x", KIND_OTHER, 13, 1, None));
        bad(&mut doc, input("x", KIND_OTHER, 0, 1, None));
        bad(&mut doc, input("x", KIND_OTHER, 2, 29, Some(2025)));
        bad(&mut doc, input("x", KIND_OTHER, 1, 1, Some(1800)));
        let mut once = input("x", KIND_OTHER, 1, 1, None);
        once.yearly = false;
        bad(&mut doc, once);
        let mut far = input("x", KIND_OTHER, 1, 1, None);
        far.remind_days = MAX_REMIND_DAYS + 1;
        bad(&mut doc, far);
        assert!(matches!(
            doc.add(" ", &input("x", KIND_OTHER, 1, 1, None), 1),
            Err(Error::MissingAuthor)
        ));
        // Le 29 février sans année, ou d'une année bissextile, est accepté.
        doc.add("Eve", &input("x", KIND_OTHER, 2, 29, None), 1)
            .unwrap();
        doc.add("Eve", &input("x", KIND_OTHER, 2, 29, Some(2024)), 1)
            .unwrap();
    }

    #[test]
    fn next_occurrence_and_years() {
        let mut doc = DatesDoc::new();
        let couple = doc
            .add("Lucas", &input("Nous", KIND_COUPLE, 6, 12, Some(2021)), 1)
            .unwrap();
        // Pas encore passée cette année.
        assert_eq!(
            couple.next_on_or_after(day(2026, 6, 1)),
            Some(day(2026, 6, 12))
        );
        assert_eq!(couple.years_on(day(2026, 6, 12)), Some(5));
        // Aujourd'hui compte.
        assert_eq!(
            couple.next_on_or_after(day(2026, 6, 12)),
            Some(day(2026, 6, 12))
        );
        // Déjà passée : l'année suivante.
        assert_eq!(
            couple.next_on_or_after(day(2026, 6, 13)),
            Some(day(2027, 6, 12))
        );
        // Avant l'année de départ : la première fois.
        assert_eq!(
            couple.next_on_or_after(day(2019, 1, 1)),
            Some(day(2021, 6, 12))
        );
        assert_eq!(couple.years_on(day(2021, 6, 12)), None);
        assert_eq!(couple.years_on(day(2022, 6, 12)), Some(1));

        // 29 février : le 28 les années non bissextiles.
        let leap = doc
            .add("Eve", &input("Bissextile", KIND_BIRTHDAY, 2, 29, None), 1)
            .unwrap();
        assert_eq!(
            leap.next_on_or_after(day(2026, 1, 1)),
            Some(day(2026, 2, 28))
        );
        assert_eq!(
            leap.next_on_or_after(day(2027, 3, 1)),
            Some(day(2028, 2, 29))
        );

        // Date unique.
        let mut once = input("Mariage de Léa", KIND_OTHER, 9, 5, Some(2027));
        once.yearly = false;
        let once = doc.add("Eve", &once, 1).unwrap();
        assert_eq!(
            once.next_on_or_after(day(2026, 11, 26)),
            Some(day(2027, 9, 5))
        );
        assert_eq!(once.next_on_or_after(day(2027, 9, 6)), None);
        assert_eq!(once.years_on(day(2027, 9, 5)), None);
    }

    #[test]
    fn upcoming_order_and_countdown() {
        let mut doc = DatesDoc::new();
        let today = day(2026, 11, 26);
        let noel = doc
            .add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 1)
            .unwrap();
        let couple = doc
            .add("Lucas", &input("Nous", KIND_COUPLE, 11, 26, Some(2021)), 1)
            .unwrap();
        let st_valentin = doc
            .add(
                "Eve",
                &input("Saint-Valentin", KIND_HOLIDAY, 2, 14, None),
                1,
            )
            .unwrap();
        let mut old = input("Déménagement", KIND_OTHER, 3, 1, Some(2025));
        old.yearly = false;
        let old = doc.add("Eve", &old, 1).unwrap();
        let mut older = input("Concert", KIND_OTHER, 3, 1, Some(2024));
        older.yearly = false;
        let older = doc.add("Eve", &older, 1).unwrap();

        let list = doc.upcoming(today);
        let ids: Vec<_> = list.iter().map(|u| u.date.id.clone()).collect();
        assert_eq!(
            ids,
            vec![couple.id, noel.id, st_valentin.id, old.id, older.id]
        );
        assert_eq!(list[0].days_left, Some(0));
        assert_eq!(list[0].years, Some(5));
        assert_eq!(list[1].days_left, Some(29));
        assert_eq!(list[1].next, Some(day(2026, 12, 25)));
        assert_eq!(list[2].next, Some(day(2027, 2, 14)));
        assert_eq!(list[2].years, None);
        assert_eq!(list[3].next, None);
        assert_eq!(list[3].days_left, None);
    }

    #[test]
    fn reminders_before_and_on_the_day() {
        let mut doc = DatesDoc::new();
        let mut eve = input("Anniversaire d'Eve", KIND_BIRTHDAY, 12, 3, Some(1998));
        eve.remind_days = 7;
        let eve = doc.add("Lucas", &eve, 1).unwrap();
        doc.add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 1)
            .unwrap();

        // Une seule journée : seulement le rappel 7 jours avant.
        let today = day(2026, 11, 26);
        let r = doc.reminders(today, today);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].date_id, eve.id);
        assert_eq!(r[0].on, today);
        assert_eq!(r[0].event, day(2026, 12, 3));
        assert_eq!(r[0].days_before, 7);
        assert_eq!(r[0].title, "🎂 Dans 7 jours : Anniversaire d'Eve");
        assert_eq!(r[0].body, "28 ans le 3 décembre");
        assert_eq!(r[0].key(), format!("{}@2026-11-26-j7", eve.id));

        // Sur un mois : 7 jours avant, le jour même, puis Noël.
        let r = doc.reminders(today, day(2026, 12, 31));
        let days: Vec<_> = r.iter().map(|r| (r.on, r.days_before)).collect();
        assert_eq!(
            days,
            vec![
                (day(2026, 11, 26), 7),
                (day(2026, 12, 3), 0),
                (day(2026, 12, 25), 0)
            ]
        );
        assert_eq!(r[1].title, "🎂 Aujourd'hui : Anniversaire d'Eve");
        assert_eq!(r[2].body, "Le 25 décembre");

        // Le rappel « avant » de l'occurrence de l'année suivante est trouvé.
        let r = doc.reminders(day(2027, 11, 20), day(2027, 11, 30));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].event, day(2027, 12, 3));
        // Rappel passé : rien.
        assert!(doc
            .reminders(day(2026, 11, 27), day(2026, 12, 2))
            .is_empty());
    }

    #[test]
    fn reminder_titles() {
        let mut doc = DatesDoc::new();
        let mut i = input("Nous", KIND_COUPLE, 1, 2, Some(2025));
        i.remind_days = 1;
        doc.add("Eve", &i, 1).unwrap();
        let r = doc.reminders(day(2026, 1, 1), day(2026, 1, 2));
        assert_eq!(r[0].title, "💞 Demain : Nous");
        assert_eq!(r[0].body, "1 an le 2 janvier");
    }

    #[test]
    fn survives_save_and_load() {
        let mut doc = DatesDoc::new();
        let a = doc
            .add("Lucas", &input("Nous", KIND_COUPLE, 6, 12, Some(2021)), 1)
            .unwrap();
        let b = doc
            .add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 2)
            .unwrap();
        let reloaded = DatesDoc::load(&doc.save()).unwrap();
        assert_eq!(reloaded.get(&a.id).unwrap(), a);
        assert_eq!(reloaded.get(&b.id).unwrap(), b);
    }

    #[test]
    fn concurrent_additions_are_kept() {
        let mut pc = DatesDoc::new();
        let mut phone = pc.fork();
        pc.add("Lucas", &input("Nous", KIND_COUPLE, 6, 12, None), 1)
            .unwrap();
        phone
            .add(
                "Eve",
                &input("Anniversaire de Lucas", KIND_BIRTHDAY, 4, 2, None),
                1,
            )
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }

    #[test]
    fn concurrent_edit_and_delete() {
        let mut pc = DatesDoc::new();
        let d = pc
            .add("Lucas", &input("Nous", KIND_COUPLE, 6, 12, None), 1)
            .unwrap();
        let mut phone = pc.fork();
        // Modifications concurrentes : une seule valeur gardée, la même partout.
        let mut a = input("Notre anniversaire", KIND_COUPLE, 6, 12, None);
        a.remind_days = 7;
        pc.update(&d.id, &a, 2).unwrap();
        phone
            .update(&d.id, &input("Nous deux", KIND_COUPLE, 6, 13, None), 3)
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 1);

        // Une suppression l'emporte sur une modification simultanée.
        let mut phone = pc.fork();
        pc.delete(&d.id).unwrap();
        phone
            .update(&d.id, &input("Encore", KIND_COUPLE, 6, 12, None), 4)
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert!(pc.list().is_empty() && phone.list().is_empty());
    }

    #[test]
    fn unreadable_or_invalid_entries_are_ignored() {
        let mut doc = DatesDoc::new();
        let ok = doc
            .add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 1)
            .unwrap();
        doc.doc.put(ROOT, "bizarre", "pas une date").unwrap();
        // Une date impossible (31 février) ou illisible venue d'ailleurs est ignorée.
        for (id, when) in [("faux", "annuel:02-31"), ("bizarre2", "chaque:12-25")] {
            let mut bad = Stored::from(&ok);
            bad.id = id.into();
            bad.when = when.into();
            reconcile_prop(&mut doc.doc, ROOT, id, &bad).unwrap();
        }
        // Un type inconnu est lu comme « autre », et gardé s'il ne change pas.
        let mut future = Stored::from(&ok);
        future.id = "futur".into();
        future.kind = "nouveau-type".into();
        // Rappel trop lointain : ramené au maximum (pas de boucle sans fin).
        future.remind_days = u32::MAX;
        reconcile_prop(&mut doc.doc, ROOT, "futur", &future).unwrap();

        assert_eq!(doc.list().len(), 2);
        assert_eq!(doc.unreadable_count(), 3);
        let read = doc.get("futur").unwrap();
        assert_eq!(read.kind(), KIND_OTHER);
        assert_eq!(read.remind_days, MAX_REMIND_DAYS);
        assert_eq!(doc.reminders(day(2026, 1, 1), day(2026, 12, 31)).len(), 3);
        let same = input("Noël 2", "nouveau-type", 12, 25, None);
        assert_eq!(doc.update("futur", &same, 2).unwrap().kind, "nouveau-type");
        assert!(matches!(
            doc.update(&ok.id, &same, 2),
            Err(Error::InvalidDate(_))
        ));
    }

    #[test]
    fn concurrent_calendar_edits_never_hide_the_date() {
        // Eve en fait une date unique pendant que Lucas ne compte plus les
        // années : champ par champ, cela donnerait « unique sans année ».
        let mut pc = DatesDoc::new();
        let d = pc
            .add("Lucas", &input("Nous", KIND_COUPLE, 2, 29, Some(2024)), 1)
            .unwrap();
        let mut phone = pc.fork();
        let mut once = input("Nous", KIND_COUPLE, 2, 29, Some(2024));
        once.yearly = false;
        pc.update(&d.id, &once, 2).unwrap();
        phone
            .update(&d.id, &input("Nous", KIND_COUPLE, 3, 1, None), 3)
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 1);
        assert_eq!(pc.unreadable_count(), 0);
    }

    #[test]
    fn when_round_trip() {
        for (m, d, y, yearly, text) in [
            (6, 12, Some(2021), true, "annuel:2021-06-12"),
            (2, 29, None, true, "annuel:02-29"),
            (9, 5, Some(2027), false, "unique:2027-09-05"),
        ] {
            let mut doc = DatesDoc::new();
            let mut i = input("x", KIND_OTHER, m, d, y);
            i.yearly = yearly;
            let date = doc.add("Eve", &i, 1).unwrap();
            assert_eq!(encode_when(&date), text);
            assert_eq!(decode_when(text), Some((m, d, y, yearly)));
        }
        for bad in [
            "",
            "annuel",
            "annuel:",
            "annuel:1-2-3-4",
            "annuel:aa-01",
            "x:01-01",
        ] {
            assert_eq!(decode_when(bad), None, "{bad}");
        }
    }

    #[test]
    fn syncs_with_protocol() {
        let mut pc = DatesDoc::new();
        let mut phone = DatesDoc::new();
        pc.add("Lucas", &input("Nous", KIND_COUPLE, 6, 12, Some(2021)), 1)
            .unwrap();
        phone
            .add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 2)
            .unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }

    #[test]
    fn upcoming_serializes_flat() {
        let mut doc = DatesDoc::new();
        doc.add("Eve", &input("Noël", KIND_HOLIDAY, 12, 25, None), 1)
            .unwrap();
        let json = serde_json::to_value(doc.upcoming(day(2026, 11, 26))).unwrap();
        assert_eq!(json[0]["title"], "Noël");
        assert_eq!(json[0]["days_left"], 29);
        assert_eq!(json[0]["next"]["month"], 12);
    }
}
