//! Animés regardés ensemble, stockés dans un document Automerge.
//!
//! Même principe que les tâches : la racine est une table `id -> Anime`,
//! chaque animé a un identifiant unique (UUID v4). Pour chacun, on garde ses
//! informations (titre, affiche, nombre d'épisodes, venues d'AniList, voir
//! [`crate::anilist`]), où vous en êtes (« épisode où l'on s'est arrêté ») et
//! son statut (à voir, en cours, en pause, fini, abandonné).
//!
//! Si deux appareils changent l'épisode en même temps, Automerge garde une
//! seule des deux valeurs, la même partout. Une suppression l'emporte sur une
//! modification faite en même temps.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Longueur maximale d'un titre, en caractères.
pub const MAX_TITLE: usize = 200;
/// Nombre maximal d'épisodes (garde-fou contre une valeur absurde).
pub const MAX_EPISODES: u32 = 10_000;
/// Nombre maximal de genres gardés pour un animé.
pub const MAX_GENRES: usize = 8;
/// Longueur maximale d'un genre ou d'un format, en caractères.
const MAX_LABEL: usize = 40;
/// Longueur maximale de l'adresse d'une affiche, en octets.
const MAX_COVER_URL: usize = 300;
/// Seules les affiches hébergées par AniList sont acceptées : l'appli ne
/// télécharge jamais une adresse quelconque reçue d'un autre appareil.
const COVER_HOSTS: [&str; 1] = ["https://s4.anilist.co/"];

/// Où en est un animé.
pub const STATUSES: [&str; 5] = [
    STATUS_TO_WATCH,
    STATUS_WATCHING,
    STATUS_PAUSED,
    STATUS_DONE,
    STATUS_DROPPED,
];
pub const STATUS_TO_WATCH: &str = "a-voir";
pub const STATUS_WATCHING: &str = "en-cours";
pub const STATUS_PAUSED: &str = "en-pause";
pub const STATUS_DONE: &str = "fini";
pub const STATUS_DROPPED: &str = "abandonne";

/// Informations d'un animé, telles qu'AniList les donne (ou saisies à la main
/// quand il n'est pas sur AniList ou que le réseau manque).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct AnimeInfo {
    /// Identifiant AniList, ou aucun pour un animé saisi à la main.
    pub anilist_id: Option<i64>,
    pub title: String,
    /// Titre original (japonais), s'il est connu.
    pub title_native: String,
    /// Adresse de l'affiche (hébergée par AniList), ou vide.
    pub cover_url: String,
    /// Couleur dominante de l'affiche (`#rrggbb`), ou vide.
    pub color: String,
    /// « TV », « MOVIE », « OVA »…
    pub format: String,
    pub year: Option<i32>,
    pub genres: Vec<String>,
    /// Nombre total d'épisodes, s'il est connu (inconnu pour un animé en cours
    /// de diffusion).
    pub episodes: Option<u32>,
}

impl AnimeInfo {
    /// Nettoie les informations : texte coupé, adresse d'affiche refusée si
    /// elle n'est pas sur AniList, couleur refusée si elle n'est pas `#rrggbb`.
    pub fn cleaned(self) -> Result<Self> {
        let title = self.title.trim();
        if title.is_empty() {
            return Err(Error::EmptyAnime);
        }
        if title.chars().count() > MAX_TITLE {
            return Err(Error::AnimeTitleTooLong(MAX_TITLE));
        }
        Ok(Self {
            anilist_id: self.anilist_id.filter(|id| *id > 0),
            title: title.to_owned(),
            title_native: truncate(&self.title_native, MAX_TITLE),
            cover_url: if valid_cover_url(&self.cover_url) {
                self.cover_url
            } else {
                String::new()
            },
            color: if valid_color(&self.color) {
                self.color.to_ascii_lowercase()
            } else {
                String::new()
            },
            format: truncate(&self.format, MAX_LABEL),
            year: self.year.filter(|y| (1900..=2200).contains(y)),
            genres: self
                .genres
                .iter()
                .map(|g| truncate(g, MAX_LABEL))
                .filter(|g| !g.is_empty())
                .take(MAX_GENRES)
                .collect(),
            episodes: self.episodes.filter(|n| (1..=MAX_EPISODES).contains(n)),
        })
    }
}

/// Un animé de votre liste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Anime {
    pub id: String,
    pub info: AnimeInfo,
    /// Dernier épisode vu ensemble (0 : pas encore commencé).
    pub episode: u32,
    /// Une des valeurs de [`STATUSES`].
    pub status: String,
    /// Qui a changé l'épisode en dernier, et quand.
    pub episode_by: String,
    pub episode_at: Option<i64>,
    /// Prénom de la personne qui l'a ajouté.
    pub added_by: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// L'adresse d'une affiche acceptée par l'appli (https, hébergée par AniList).
pub fn valid_cover_url(url: &str) -> bool {
    url.len() <= MAX_COVER_URL
        && COVER_HOSTS.iter().any(|host| {
            url.strip_prefix(host).is_some_and(|path| {
                !path.is_empty()
                    && path
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
                    && !path.contains("..")
            })
        })
}

fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Coupe après `max` caractères puis enlève les espaces autour : nettoyer
/// deux fois donne le même résultat (voir `AnimesDoc::read`).
fn truncate(text: &str, max: usize) -> String {
    let cut: String = text.trim().chars().take(max).collect();
    cut.trim().to_owned()
}

/// Le document des animés. Comme pour [`crate::NotesDoc`], `Clone` garde
/// l'identifiant d'appareil : utiliser [`AnimesDoc::fork`] pour simuler un
/// autre appareil.
#[derive(Debug, Default, Clone)]
pub struct AnimesDoc {
    doc: AutoCommit,
}

impl AnimesDoc {
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

    /// Tous les animés lisibles : d'abord ceux en cours, puis en pause, à
    /// voir, finis et abandonnés ; dans chaque groupe, le dernier touché en
    /// premier.
    pub fn list(&self) -> Vec<Anime> {
        let rank = |a: &Anime| {
            STATUSES
                .iter()
                .position(|s| *s == a.status)
                .map_or(usize::MAX, |i| [2, 0, 1, 3, 4][i])
        };
        let mut animes: Vec<Anime> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        animes.sort_by(|a, b| {
            rank(a)
                .cmp(&rank(b))
                .then_with(|| b.updated_at.cmp(&a.updated_at))
                .then_with(|| a.id.cmp(&b.id))
        });
        animes
    }

    /// Nombre d'entrées du document qui ne sont pas des animés valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    /// Une entrée venue d'un autre appareil n'est gardée que si elle respecte
    /// les mêmes règles qu'une entrée créée ici.
    fn read(&self, key: &str) -> Option<Anime> {
        hydrate_prop::<_, Anime, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|a| {
                a.id == key
                    && STATUSES.contains(&a.status.as_str())
                    && a.episode <= MAX_EPISODES
                    && a.info.clone().cleaned().ok().as_ref() == Some(&a.info)
            })
    }

    pub fn get(&self, id: &str) -> Result<Anime> {
        self.read(id)
            .ok_or_else(|| Error::AnimeNotFound(id.to_owned()))
    }

    /// Les identifiants AniList des animés de la liste (pour le cache des
    /// affiches) avec l'adresse de leur affiche.
    pub fn covers(&self) -> Vec<(i64, String)> {
        let mut covers: Vec<(i64, String)> = self
            .list()
            .into_iter()
            .filter_map(|a| {
                let id = a.info.anilist_id?;
                (!a.info.cover_url.is_empty()).then_some((id, a.info.cover_url))
            })
            .collect();
        covers.sort();
        covers.dedup_by_key(|(id, _)| *id);
        covers
    }

    /// Ajoute un animé (« à voir ») et le renvoie. Refusé s'il est déjà dans
    /// la liste (même identifiant AniList, ou même titre pour un animé saisi
    /// à la main).
    pub fn add(&mut self, author: &str, info: AnimeInfo, now_ms: i64) -> Result<Anime> {
        let info = info.cleaned()?;
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let duplicate = self.list().into_iter().any(|a| match info.anilist_id {
            Some(id) => a.info.anilist_id == Some(id),
            None => {
                a.info.anilist_id.is_none()
                    && a.info.title.to_lowercase() == info.title.to_lowercase()
            }
        });
        if duplicate {
            return Err(Error::AnimeAlreadyAdded(info.title));
        }
        let anime = Anime {
            id: uuid::Uuid::new_v4().to_string(),
            info,
            episode: 0,
            status: STATUS_TO_WATCH.to_owned(),
            episode_by: String::new(),
            episode_at: None,
            added_by: author.to_owned(),
            created_at: now_ms,
            updated_at: now_ms,
        };
        reconcile_prop(&mut self.doc, ROOT, anime.id.as_str(), &anime)?;
        Ok(anime)
    }

    /// Note l'épisode où vous vous êtes arrêtés. Commencer un animé « à
    /// voir » le passe « en cours » ; atteindre le dernier épisode le passe
    /// « fini ».
    pub fn set_episode(&mut self, id: &str, episode: u32, by: &str, now_ms: i64) -> Result<Anime> {
        let mut anime = self.get(id)?;
        let by = by.trim();
        if by.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let max = anime.info.episodes.unwrap_or(MAX_EPISODES);
        if episode > max {
            return Err(Error::EpisodeOutOfRange(max));
        }
        anime.episode = episode;
        anime.episode_by = by.to_owned();
        anime.episode_at = Some(now_ms);
        if anime.info.episodes == Some(episode) {
            anime.status = STATUS_DONE.to_owned();
        } else if episode > 0 && (anime.status == STATUS_TO_WATCH || anime.status == STATUS_DONE) {
            anime.status = STATUS_WATCHING.to_owned();
        }
        anime.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &anime)?;
        Ok(anime)
    }

    pub fn set_status(&mut self, id: &str, status: &str, now_ms: i64) -> Result<Anime> {
        if !STATUSES.contains(&status) {
            return Err(Error::InvalidAnimeStatus(status.to_owned()));
        }
        let mut anime = self.get(id)?;
        anime.status = status.to_owned();
        anime.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &anime)?;
        Ok(anime)
    }

    /// Remplace les informations (par exemple le nombre d'épisodes d'un animé
    /// encore en diffusion). L'épisode noté n'est jamais perdu : s'il dépasse
    /// le nouveau total, le total est ignoré. Un animé en cours dont le
    /// dernier épisode est déjà vu passe « fini ».
    pub fn update_info(&mut self, id: &str, info: AnimeInfo, now_ms: i64) -> Result<Anime> {
        let mut info = info.cleaned()?;
        let mut anime = self.get(id)?;
        if info.episodes.is_some_and(|n| n < anime.episode) {
            info.episodes = None;
        }
        if info.episodes == Some(anime.episode) && anime.status == STATUS_WATCHING {
            anime.status = STATUS_DONE.to_owned();
        }
        anime.info = info;
        anime.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &anime)?;
        Ok(anime)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::AnimeNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }

    pub fn merge(&mut self, other: &mut AnimesDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for AnimesDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    fn frieren() -> AnimeInfo {
        AnimeInfo {
            anilist_id: Some(154_587),
            title: "Frieren: Beyond Journey's End".to_owned(),
            title_native: "葬送のフリーレン".to_owned(),
            cover_url: "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx154587-n1fmjRv4JQUd.jpg".to_owned(),
            color: "#D6F1A1".to_owned(),
            format: "TV".to_owned(),
            year: Some(2023),
            genres: vec!["Adventure".to_owned(), "Drama".to_owned()],
            episodes: Some(28),
        }
    }

    fn manual(title: &str) -> AnimeInfo {
        AnimeInfo {
            title: title.to_owned(),
            ..AnimeInfo::default()
        }
    }

    #[test]
    fn add_progress_status_delete() {
        let mut doc = AnimesDoc::new();
        let anime = doc.add("Lucas", frieren(), 1).unwrap();
        assert_eq!(anime.status, STATUS_TO_WATCH);
        assert_eq!(anime.episode, 0);
        assert_eq!(anime.info.color, "#d6f1a1");
        assert_eq!(doc.list(), vec![anime.clone()]);

        let started = doc.set_episode(&anime.id, 3, "Eve", 2).unwrap();
        assert_eq!(started.status, STATUS_WATCHING);
        assert_eq!(started.episode_by, "Eve");
        assert_eq!(started.episode_at, Some(2));

        let paused = doc.set_status(&anime.id, STATUS_PAUSED, 3).unwrap();
        assert_eq!(paused.status, STATUS_PAUSED);
        // Reprendre un animé en pause ne change pas son statut tout seul.
        assert_eq!(
            doc.set_episode(&anime.id, 4, "Lucas", 4).unwrap().status,
            STATUS_PAUSED
        );

        let done = doc.set_episode(&anime.id, 28, "Lucas", 5).unwrap();
        assert_eq!(done.status, STATUS_DONE);
        // Revenir en arrière sur un animé fini le remet en cours.
        assert_eq!(
            doc.set_episode(&anime.id, 27, "Eve", 6).unwrap().status,
            STATUS_WATCHING
        );
        assert!(matches!(
            doc.set_episode(&anime.id, 29, "Eve", 7),
            Err(Error::EpisodeOutOfRange(28))
        ));

        doc.delete(&anime.id).unwrap();
        assert!(doc.list().is_empty());
        assert!(matches!(doc.get(&anime.id), Err(Error::AnimeNotFound(_))));
        assert!(matches!(
            doc.delete(&anime.id),
            Err(Error::AnimeNotFound(_))
        ));
    }

    #[test]
    fn rejects_invalid_input() {
        let mut doc = AnimesDoc::new();
        assert!(matches!(
            doc.add("Eve", manual("  "), 1),
            Err(Error::EmptyAnime)
        ));
        assert!(matches!(
            doc.add(" ", manual("Naruto"), 1),
            Err(Error::MissingAuthor)
        ));
        assert!(matches!(
            doc.add("Eve", manual(&"a".repeat(MAX_TITLE + 1)), 1),
            Err(Error::AnimeTitleTooLong(_))
        ));
        let anime = doc.add("Eve", manual("Naruto"), 1).unwrap();
        assert!(matches!(
            doc.add("Lucas", manual(" naruto "), 2),
            Err(Error::AnimeAlreadyAdded(_))
        ));
        doc.add("Lucas", frieren(), 2).unwrap();
        assert!(matches!(
            doc.add("Eve", frieren(), 3),
            Err(Error::AnimeAlreadyAdded(_))
        ));
        assert!(matches!(
            doc.set_status(&anime.id, "regarde", 2),
            Err(Error::InvalidAnimeStatus(_))
        ));
        assert!(matches!(
            doc.set_episode(&anime.id, 1, "", 2),
            Err(Error::MissingAuthor)
        ));
        // Sans nombre d'épisodes connu : seulement le garde-fou.
        assert!(doc.set_episode(&anime.id, 700, "Eve", 2).is_ok());
        assert!(doc
            .set_episode(&anime.id, MAX_EPISODES + 1, "Eve", 2)
            .is_err());
    }

    #[test]
    fn cover_urls_must_come_from_anilist() {
        assert!(valid_cover_url(&frieren().cover_url));
        assert!(!valid_cover_url("https://img.anili.st/media/154587"));
        for bad in [
            "http://s4.anilist.co/file/a.jpg",
            "https://s4.anilist.co.evil.fr/a.jpg",
            "https://evil.fr/https://s4.anilist.co/a.jpg",
            "https://s4.anilist.co/",
            "https://s4.anilist.co/../a.jpg",
            "https://s4.anilist.co/a.jpg?x=1",
            "https://s4.anilist.co/a b.jpg",
            "file:///etc/passwd",
        ] {
            assert!(!valid_cover_url(bad), "{bad}");
        }
        let info = AnimeInfo {
            cover_url: "https://evil.fr/a.jpg".to_owned(),
            color: "red".to_owned(),
            year: Some(42),
            episodes: Some(0),
            genres: (0..20).map(|i| i.to_string()).collect(),
            ..frieren()
        }
        .cleaned()
        .unwrap();
        assert_eq!(info.cover_url, "");
        assert_eq!(info.color, "");
        assert_eq!(info.year, None);
        assert_eq!(info.episodes, None);
        assert_eq!(info.genres.len(), MAX_GENRES);
    }

    #[test]
    fn list_order_and_covers() {
        let mut doc = AnimesDoc::new();
        let done = doc.add("Eve", manual("Fini"), 1).unwrap();
        doc.set_status(&done.id, STATUS_DONE, 10).unwrap();
        let to_watch = doc.add("Eve", manual("À voir"), 2).unwrap();
        let paused = doc.add("Eve", manual("Pause"), 3).unwrap();
        doc.set_status(&paused.id, STATUS_PAUSED, 4).unwrap();
        let old = doc.add("Eve", frieren(), 5).unwrap();
        doc.set_episode(&old.id, 1, "Eve", 6).unwrap();
        let recent = doc.add("Eve", manual("Récent"), 7).unwrap();
        doc.set_episode(&recent.id, 1, "Eve", 8).unwrap();

        let ids: Vec<_> = doc.list().into_iter().map(|a| a.id).collect();
        assert_eq!(
            ids,
            vec![recent.id, old.id, paused.id, to_watch.id, done.id]
        );
        assert_eq!(doc.covers(), vec![(154_587, frieren().cover_url)]);
    }

    #[test]
    fn cleaning_twice_changes_nothing() {
        let info = AnimeInfo {
            title_native: format!("{} fin", "a".repeat(MAX_TITLE - 1)),
            format: format!("  {}  x", "b".repeat(MAX_LABEL - 1)),
            genres: vec![format!("{} z", "g".repeat(MAX_LABEL - 1)), "  ".to_owned()],
            ..frieren()
        };
        let once = info.cleaned().unwrap();
        assert!(!once.title_native.ends_with(' '));
        assert_eq!(once.genres.len(), 1);
        assert_eq!(once.clone().cleaned().unwrap(), once);
        // Et l'animé reste lisible après l'ajout.
        let mut doc = AnimesDoc::new();
        let anime = doc.add("Eve", once, 1).unwrap();
        assert_eq!(doc.list(), vec![anime]);
    }

    #[test]
    fn update_info_finishes_when_last_episode_seen() {
        let mut doc = AnimesDoc::new();
        let airing = AnimeInfo {
            episodes: None,
            ..frieren()
        };
        let anime = doc.add("Lucas", airing, 1).unwrap();
        doc.set_episode(&anime.id, 28, "Lucas", 2).unwrap();
        assert_eq!(doc.get(&anime.id).unwrap().status, STATUS_WATCHING);
        assert_eq!(
            doc.update_info(&anime.id, frieren(), 3).unwrap().status,
            STATUS_DONE
        );
    }

    #[test]
    fn update_info_keeps_progress() {
        let mut doc = AnimesDoc::new();
        let airing = AnimeInfo {
            episodes: None,
            ..frieren()
        };
        let anime = doc.add("Lucas", airing, 1).unwrap();
        doc.set_episode(&anime.id, 12, "Lucas", 2).unwrap();
        let updated = doc.update_info(&anime.id, frieren(), 3).unwrap();
        assert_eq!(updated.info.episodes, Some(28));
        assert_eq!(updated.episode, 12);
        // Un total plus petit que l'épisode noté est ignoré.
        let short = AnimeInfo {
            episodes: Some(10),
            ..frieren()
        };
        assert_eq!(
            doc.update_info(&anime.id, short, 4).unwrap().info.episodes,
            None
        );
    }

    #[test]
    fn survives_save_and_load() {
        let mut doc = AnimesDoc::new();
        let a = doc.add("Lucas", frieren(), 1).unwrap();
        let b = doc.add("Eve", manual("Film du dimanche"), 2).unwrap();
        let reloaded = AnimesDoc::load(&doc.save()).unwrap();
        assert_eq!(reloaded.get(&a.id).unwrap(), a);
        assert_eq!(reloaded.get(&b.id).unwrap(), b);
    }

    #[test]
    fn concurrent_additions_are_kept() {
        let mut pc = AnimesDoc::new();
        let mut phone = pc.fork();
        let a = pc.add("Lucas", frieren(), 1).unwrap();
        let b = phone.add("Eve", manual("Ghibli"), 1).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
        assert!(pc.get(&a.id).is_ok() && pc.get(&b.id).is_ok());
    }

    #[test]
    fn concurrent_episode_and_status_converge() {
        let mut pc = AnimesDoc::new();
        let anime = pc.add("Lucas", frieren(), 1).unwrap();
        pc.set_episode(&anime.id, 2, "Lucas", 2).unwrap();
        let mut phone = pc.fork();
        pc.set_status(&anime.id, STATUS_PAUSED, 3).unwrap();
        phone.set_episode(&anime.id, 5, "Eve", 4).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        // Champs différents : les deux modifications sont gardées.
        let merged = pc.get(&anime.id).unwrap();
        assert_eq!(merged.status, STATUS_PAUSED);
        assert_eq!(merged.episode, 5);
        assert_eq!(merged.episode_by, "Eve");
    }

    #[test]
    fn concurrent_delete_wins_over_progress() {
        let mut pc = AnimesDoc::new();
        let anime = pc.add("Lucas", frieren(), 1).unwrap();
        let mut phone = pc.fork();
        pc.delete(&anime.id).unwrap();
        phone.set_episode(&anime.id, 3, "Eve", 2).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert!(pc.list().is_empty());
        assert!(phone.list().is_empty());
    }

    #[test]
    fn unreadable_or_invalid_entries_are_ignored() {
        let mut doc = AnimesDoc::new();
        let anime = doc.add("Eve", frieren(), 1).unwrap();
        doc.doc.put(ROOT, "bizarre", "pas un animé").unwrap();
        // Une entrée bien formée mais qui enfreint les règles (affiche hors
        // d'AniList) est ignorée aussi.
        let mut evil = doc.add("Eve", manual("Piège"), 2).unwrap();
        evil.info.cover_url = "https://evil.fr/a.jpg".to_owned();
        reconcile_prop(&mut doc.doc, ROOT, evil.id.as_str(), &evil).unwrap();
        assert_eq!(doc.list(), vec![anime]);
        assert_eq!(doc.unreadable_count(), 2);
    }

    #[test]
    fn syncs_with_protocol() {
        let mut pc = AnimesDoc::new();
        let mut phone = AnimesDoc::new();
        pc.add("Lucas", frieren(), 1).unwrap();
        phone.add("Eve", manual("Ghibli"), 2).unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }
}
