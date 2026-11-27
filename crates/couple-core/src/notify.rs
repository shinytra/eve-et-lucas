//! Notifications des nouveautés : « Eve a ajouté une note », « Lucas a fait
//! une tâche »…
//!
//! L'appareil où l'on fait le changement envoie un court message chiffré
//! (le même canal ntfy que les arrivées, voir [`crate::push`]) à tous les
//! autres. Chaque appareil choisit lui-même, type par type, ce qu'il affiche
//! ([`NotifySettings`], propre à l'appareil, jamais synchronisé). Sur
//! téléphone, c'est le code Kotlin qui affiche la notification, même appli
//! fermée ; il ne fait qu'appliquer les mêmes règles que [`Event::should_show`].
//!
//! Le titre et le texte de la notification sont préparés ici, par l'appareil
//! qui envoie : le téléphone qui reçoit n'a qu'à les afficher.

use serde::{Deserialize, Serialize};

use crate::{
    animes::Anime, dates::ImportantDate, files::SharedFile, music::Music, notes::Note,
    push::PushMessage, tasks::Task, Error, Result,
};

/// Nouvelle note.
pub const NOTE: &str = "note";
/// Nouvelle tâche.
pub const TASK: &str = "tache";
/// Tâche cochée.
pub const TASK_DONE: &str = "tache-faite";
/// Nouvelle date importante.
pub const DATE: &str = "date";
/// Nouvel animé, ou épisode noté.
pub const ANIME: &str = "anime";
/// Nouveau fichier partagé.
pub const FILE: &str = "fichier";
/// Musique recommandée.
pub const MUSIC: &str = "musique";

/// Un type de notification, pour l'écran des réglages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NotifyKind {
    pub code: &'static str,
    pub label: &'static str,
    pub description: &'static str,
}

/// Tous les types, dans l'ordre de l'écran des réglages.
pub const KINDS: [NotifyKind; 7] = [
    NotifyKind {
        code: NOTE,
        label: "Nouvelles notes",
        description: "Quand l'autre ajoute une note.",
    },
    NotifyKind {
        code: TASK,
        label: "Nouvelles tâches",
        description: "Quand l'autre ajoute une tâche.",
    },
    NotifyKind {
        code: TASK_DONE,
        label: "Tâches faites",
        description: "Quand l'autre coche une tâche.",
    },
    NotifyKind {
        code: DATE,
        label: "Dates importantes",
        description: "Quand l'autre ajoute une date (les rappels du jour J restent à part).",
    },
    NotifyKind {
        code: ANIME,
        label: "Animés",
        description: "Quand l'autre ajoute un animé ou note un épisode.",
    },
    NotifyKind {
        code: FILE,
        label: "Fichiers partagés",
        description: "Quand l'autre partage un fichier.",
    },
    NotifyKind {
        code: MUSIC,
        label: "Musiques",
        description: "Quand l'autre te recommande une musique.",
    },
];

/// Longueur maximale (en caractères) du titre et du texte d'une notification.
pub const MAX_TEXT: usize = 120;
/// Longueur maximale d'un prénom annoncé.
pub const MAX_WHO: usize = 64;
/// Longueur maximale d'un identifiant (évènement, élément, appareil).
const MAX_ID: usize = 128;
/// Une nouveauté plus ancienne n'est plus affichée (ntfy garde les messages
/// 12 h : un PC qui se reconnecte après une nuit ne ressort pas tout).
pub const SHOW_WITHIN_MS: i64 = 12 * 60 * 60 * 1000;
/// Tolérance pour une horloge un peu en avance.
const CLOCK_SKEW_MS: i64 = 60_000;

pub fn is_kind(code: &str) -> bool {
    KINDS.iter().any(|k| k.code == code)
}

/// Les réglages de cet appareil. On garde les types *désactivés* : un type
/// ajouté dans une version suivante est activé d'office.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifySettings {
    #[serde(default)]
    pub disabled: Vec<String>,
}

/// Un type et son état, pour l'interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotifyChoice {
    pub code: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub enabled: bool,
}

impl NotifySettings {
    pub fn enabled(&self, kind: &str) -> bool {
        is_kind(kind) && !self.disabled.iter().any(|d| d == kind)
    }

    /// Active ou coupe un type. Un type inconnu est refusé.
    pub fn set(&mut self, kind: &str, on: bool) -> Result<()> {
        if !is_kind(kind) {
            return Err(Error::UnknownNotification(kind.to_owned()));
        }
        self.disabled.retain(|d| d != kind);
        if !on {
            self.disabled.push(kind.to_owned());
        }
        self.clean();
        Ok(())
    }

    /// Oublie les types inconnus et les doublons (fichier modifié à la main,
    /// type retiré dans une version suivante).
    pub fn clean(&mut self) {
        self.disabled.retain(|d| is_kind(d));
        self.disabled.sort();
        self.disabled.dedup();
    }

    /// Les types activés (pour le code Kotlin).
    pub fn enabled_kinds(&self) -> Vec<String> {
        KINDS
            .iter()
            .filter(|k| self.enabled(k.code))
            .map(|k| k.code.to_owned())
            .collect()
    }

    pub fn choices(&self) -> Vec<NotifyChoice> {
        KINDS
            .iter()
            .map(|k| NotifyChoice {
                code: k.code,
                label: k.label,
                description: k.description,
                enabled: self.enabled(k.code),
            })
            .collect()
    }
}

/// Une nouveauté à annoncer aux autres appareils.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Un des types de [`KINDS`].
    pub kind: String,
    /// Identifiant unique de l'annonce (UUID v4) : jamais affichée deux fois.
    pub id: String,
    /// Qui a fait le changement.
    pub who: String,
    /// Identifiant iroh de l'appareil qui l'annonce.
    pub device: String,
    pub at: i64,
    /// Élément concerné : une deuxième annonce pour le même élément (épisode
    /// suivant…) remplace la notification au lieu de s'empiler.
    pub item: String,
    pub title: String,
    pub body: String,
}

/// Coupe un texte à `max` caractères (avec « … »), sans retour à la ligne.
fn short(text: &str, max: usize) -> String {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

impl Event {
    fn new(
        kind: &str,
        who: &str,
        device: &str,
        at: i64,
        item: &str,
        title: String,
        body: &str,
    ) -> Self {
        Self {
            kind: kind.to_owned(),
            id: uuid::Uuid::new_v4().to_string(),
            who: short(who, MAX_WHO),
            device: device.to_owned(),
            at,
            item: item.to_owned(),
            title: short(&title, MAX_TEXT),
            body: short(body, MAX_TEXT),
        }
    }

    pub fn note_added(note: &Note, device: &str, at: i64) -> Self {
        let body = if note.title.trim().is_empty() {
            note.body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
        } else {
            &note.title
        };
        Self::new(
            NOTE,
            &note.author,
            device,
            at,
            &note.id,
            format!("{} a ajouté une note", note.author.trim()),
            body,
        )
    }

    pub fn task_added(task: &Task, who: &str, device: &str, at: i64) -> Self {
        let body = match task.assignee.trim() {
            "" => task.title.clone(),
            assignee => format!("{} · pour {assignee}", task.title),
        };
        Self::new(
            TASK,
            who,
            device,
            at,
            &task.id,
            format!("{} a ajouté une tâche", who.trim()),
            &body,
        )
    }

    pub fn task_done(task: &Task, who: &str, device: &str, at: i64) -> Self {
        Self::new(
            TASK_DONE,
            who,
            device,
            at,
            &task.id,
            format!("{} a fait une tâche", who.trim()),
            &task.title,
        )
    }

    pub fn date_added(date: &ImportantDate, device: &str, at: i64) -> Self {
        Self::new(
            DATE,
            &date.author,
            device,
            at,
            &date.id,
            format!("{} a ajouté une date importante", date.author.trim()),
            &format!("{} · {:02}/{:02}", date.title, date.day, date.month),
        )
    }

    pub fn anime_added(anime: &Anime, device: &str, at: i64) -> Self {
        Self::new(
            ANIME,
            &anime.added_by,
            device,
            at,
            &anime.id,
            format!("{} a ajouté un animé", anime.added_by.trim()),
            &anime.info.title,
        )
    }

    pub fn anime_episode(anime: &Anime, who: &str, device: &str, at: i64) -> Self {
        Self::new(
            ANIME,
            who,
            device,
            at,
            &anime.id,
            format!("{} a noté l'épisode {}", who.trim(), anime.episode),
            &anime.info.title,
        )
    }

    pub fn file_shared(file: &SharedFile, device: &str, at: i64) -> Self {
        Self::new(
            FILE,
            &file.author,
            device,
            at,
            &file.id,
            format!("{} a partagé un fichier", file.author.trim()),
            &file.name,
        )
    }

    pub fn music_added(music: &Music, device: &str, at: i64) -> Self {
        let mut body = match (music.title.as_str(), music.artist.as_str()) {
            ("", _) => "Un morceau YouTube Music".to_owned(),
            (title, "") => title.to_owned(),
            (title, artist) => format!("{title} · {artist}"),
        };
        if !music.message.is_empty() {
            body = format!("{body} : « {} »", music.message);
        }
        Self::new(
            MUSIC,
            &music.added_by,
            device,
            at,
            &music.id,
            format!("{} te recommande une musique", music.added_by.trim()),
            &body,
        )
    }

    /// Faut-il l'afficher ici ? Pas si ce type est coupé, si elle vient de
    /// cet appareil ou de la personne qui le porte (`owner`, téléphone
    /// seulement), ni si elle est trop vieille ou datée dans le futur.
    pub fn should_show(
        &self,
        settings: &NotifySettings,
        self_device: &str,
        owner: Option<&str>,
        now_ms: i64,
    ) -> bool {
        let own = owner.is_some_and(|o| {
            !o.trim().is_empty() && o.trim().eq_ignore_ascii_case(self.who.trim())
        });
        settings.enabled(&self.kind)
            && self.device != self_device
            && !own
            && self.at <= now_ms + CLOCK_SKEW_MS
            && now_ms - self.at <= SHOW_WITHIN_MS
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && id.chars().all(|c| c.is_ascii_graphic())
}

impl PushMessage {
    pub fn event(event: &Event) -> Self {
        Self {
            v: 1,
            kind: event.kind.clone(),
            id: event.id.clone(),
            who: event.who.clone(),
            device: event.device.clone(),
            at: event.at,
            item: event.item.clone(),
            title: event.title.clone(),
            body: event.body.clone(),
            place: String::new(),
            place_kind: String::new(),
            inside: None,
        }
    }

    /// La nouveauté annoncée, si le message en est une (version 1, type
    /// connu). Les textes trop longs sont coupés.
    pub fn into_event(self) -> Option<Event> {
        let who = self.who.trim();
        if self.v != 1
            || !is_kind(&self.kind)
            || who.is_empty()
            || who.chars().count() > MAX_WHO
            || !valid_id(&self.id)
            || !valid_id(&self.device)
            || !(self.item.is_empty() || valid_id(&self.item))
            || self.title.trim().is_empty()
        {
            return None;
        }
        Some(Event {
            who: who.to_owned(),
            title: short(&self.title, MAX_TEXT),
            body: short(&self.body, MAX_TEXT),
            kind: self.kind,
            id: self.id,
            device: self.device,
            at: self.at,
            item: self.item,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::{open, seal};
    use crate::NotesDoc;

    fn note() -> Note {
        let mut doc = NotesDoc::new();
        doc.add("Eve", "Courses", "lait\npain", 1).unwrap()
    }

    #[test]
    fn settings_default_to_everything() {
        let mut s = NotifySettings::default();
        assert!(KINDS.iter().all(|k| s.enabled(k.code)));
        assert!(!s.enabled("inconnu"));
        s.set(ANIME, false).unwrap();
        s.set(ANIME, false).unwrap();
        assert_eq!(s.disabled, vec![ANIME.to_owned()]);
        assert!(!s.enabled(ANIME));
        assert!(!s.enabled_kinds().contains(&ANIME.to_owned()));
        assert_eq!(s.enabled_kinds().len(), KINDS.len() - 1);
        s.set(ANIME, true).unwrap();
        assert!(s.disabled.is_empty());
        assert!(s.set("inconnu", false).is_err());
        assert_eq!(s.choices().len(), KINDS.len());
    }

    #[test]
    fn unknown_saved_kinds_are_dropped() {
        // Un type retiré plus tard, un doublon : le fichier reste lisible.
        let mut s: NotifySettings =
            serde_json::from_str(r#"{"disabled":["note","vieux","note"],"plus":1}"#).unwrap();
        s.clean();
        assert_eq!(s.disabled, vec![NOTE.to_owned()]);
        let empty: NotifySettings = serde_json::from_str("{}").unwrap();
        assert!(empty.enabled(FILE));
    }

    #[test]
    fn texts_are_ready_to_show() {
        let event = Event::note_added(&note(), "tel-eve", 10);
        assert_eq!(event.kind, NOTE);
        assert_eq!(event.title, "Eve a ajouté une note");
        assert_eq!(event.body, "Courses");
        assert_eq!(event.who, "Eve");

        let mut doc = NotesDoc::new();
        let untitled = doc.add("Eve", "", "\n  première ligne\nsuite", 1).unwrap();
        assert_eq!(Event::note_added(&untitled, "d", 1).body, "première ligne");

        let long = "x".repeat(500);
        let mut doc = NotesDoc::new();
        let big = doc.add("Eve", &long, "", 1).unwrap();
        let body = Event::note_added(&big, "d", 1).body;
        assert_eq!(body.chars().count(), MAX_TEXT);
        assert!(body.ends_with('…'));
        assert_eq!(short("a\nb\t\u{7}c", 10), "a b c");
    }

    #[test]
    fn goes_through_ntfy_and_back() {
        let key = [7u8; 32];
        let event = Event::note_added(&note(), "tel-eve", 10);
        let sealed = seal(&key, &PushMessage::event(&event)).unwrap();
        assert!(!sealed.contains("Courses"));
        let opened = open(&key, &sealed).unwrap();
        // Une nouveauté n'est pas une arrivée (anciennes versions : ignorée).
        assert!(opened.clone().into_arrival().is_none());
        assert_eq!(opened.into_event().unwrap(), event);
        // Et une arrivée n'est pas une nouveauté.
        let arrival = crate::Arrival {
            id: "a".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 1,
        };
        assert!(PushMessage::arrival(&arrival).into_event().is_none());
    }

    #[test]
    fn plaintext_format_for_kotlin() {
        let event = Event {
            kind: TASK_DONE.into(),
            id: "00000000-0000-4000-8000-000000000000".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 1,
            item: "t1".into(),
            title: "Eve a fait une tâche".into(),
            body: "Courses".into(),
        };
        assert_eq!(
            serde_json::to_string(&PushMessage::event(&event)).unwrap(),
            r#"{"v":1,"kind":"tache-faite","id":"00000000-0000-4000-8000-000000000000","who":"Eve","device":"d","at":1,"item":"t1","title":"Eve a fait une tâche","body":"Courses"}"#
        );
    }

    #[test]
    fn refuses_strange_messages() {
        let good = PushMessage::event(&Event::note_added(&note(), "tel-eve", 10));
        let cases: Vec<fn(&mut PushMessage)> = vec![
            |m| m.v = 2,
            |m| m.kind = "inconnu".into(),
            |m| m.kind = "arrival".into(),
            |m| m.who = "  ".into(),
            |m| m.who = "x".repeat(65),
            |m| m.id = String::new(),
            |m| m.id = "a b".into(),
            |m| m.device = "x".repeat(200),
            |m| m.item = "a\nb".into(),
            |m| m.title = " ".into(),
        ];
        for (i, change) in cases.iter().enumerate() {
            let mut m = good.clone();
            change(&mut m);
            assert!(m.into_event().is_none(), "cas {i}");
        }
        // Texte trop long envoyé par un autre : coupé, pas refusé.
        let mut m = good.clone();
        m.body = "y".repeat(1000);
        assert_eq!(m.into_event().unwrap().body.chars().count(), MAX_TEXT);
    }

    #[test]
    fn who_sees_what() {
        let now = 1_000_000_000;
        let event = Event::note_added(&note(), "tel-eve", now);
        let all = NotifySettings::default();
        assert!(event.should_show(&all, "pc", None, now));
        assert!(event.should_show(&all, "tel-lucas", Some("Lucas"), now + 1000));
        // Envoyée par cet appareil, ou par la personne qui porte ce téléphone.
        assert!(!event.should_show(&all, "tel-eve", None, now));
        assert!(!event.should_show(&all, "tablette", Some(" eve "), now));
        assert!(event.should_show(&all, "tablette", Some(""), now));
        // Type coupé ici.
        let mut no_notes = NotifySettings::default();
        no_notes.set(NOTE, false).unwrap();
        assert!(!event.should_show(&no_notes, "pc", None, now));
        // Trop vieille, ou datée dans le futur.
        assert!(!event.should_show(&all, "pc", None, now + SHOW_WITHIN_MS + 1));
        assert!(!event.should_show(&all, "pc", None, now - 120_000));
    }

    #[test]
    fn every_kind_has_texts() {
        let mut tasks = crate::TasksDoc::new();
        let task = tasks.add("Lucas", "Poubelles", "Eve", None, 1).unwrap();
        let added = Event::task_added(&task, "Lucas", "pc", 1);
        assert_eq!(added.title, "Lucas a ajouté une tâche");
        assert_eq!(added.body, "Poubelles · pour Eve");
        assert_eq!(
            Event::task_done(&task, "Eve", "d", 1).title,
            "Eve a fait une tâche"
        );
        for e in [added, Event::task_done(&task, "Eve", "d", 1)] {
            assert!(is_kind(&e.kind));
            assert_eq!(e.item, task.id);
        }
        let mut music = crate::MusicDoc::new();
        let link = crate::music::parse_link("https://youtu.be/dQw4w9WgXcQ").unwrap();
        let m = music.add("Eve", &link, "Titre", "Artiste", "", 1).unwrap();
        let e = Event::music_added(&m, "d", 1);
        assert_eq!(e.kind, MUSIC);
        assert_eq!(e.title, "Eve te recommande une musique");
        assert_eq!(e.body, "Titre · Artiste");
        let m = music.update(&m.id, "", "", "écoute le refrain", 2).unwrap();
        assert_eq!(
            Event::music_added(&m, "d", 1).body,
            "Un morceau YouTube Music : « écoute le refrain »"
        );
        // Deux annonces : deux identifiants, même élément.
        let a = Event::task_done(&task, "Eve", "d", 1);
        let b = Event::task_done(&task, "Eve", "d", 1);
        assert_ne!(a.id, b.id);
        assert_eq!(a.item, b.item);
    }
}
