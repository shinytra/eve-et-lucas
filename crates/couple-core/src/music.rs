//! Musiques que l'on se recommande, avec un lien YouTube Music.
//!
//! Une recommandation = un seul morceau (choix de Lucas : pas de playlist).
//! Même principe que les animés : la racine du document est une table
//! `id -> Music`, chaque recommandation a un identifiant UUID v4. Le lien est
//! gardé sous une forme courte et vérifiée (`titre:<id vidéo>`), une seule
//! valeur Automerge. L'adresse complète est reconstruite à partir de cette forme, jamais
//! reprise telle quelle d'un autre appareil.
//!
//! Le titre et l'artiste viennent de l'oEmbed de YouTube (sans clé, voir
//! [`oembed_url`] et [`parse_oembed`]) ou sont saisis à la main. Le cœur ne fait
//! pas de réseau : l'appli envoie la requête.

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Longueur maximale d'un titre ou d'un nom d'artiste, en caractères.
pub const MAX_TITLE: usize = 200;
/// Longueur maximale du petit mot qui accompagne la recommandation.
pub const MAX_MESSAGE: usize = 500;
/// Longueur maximale du texte partagé dans lequel on cherche un lien.
const MAX_SHARED_TEXT: usize = 2000;

/// Un morceau (vidéo YouTube ou titre YouTube Music).
pub const KIND_TRACK: &str = "titre";

/// Une recommandation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Music {
    pub id: String,
    /// Forme courte du lien : `titre:<id de la vidéo>`.
    pub link: String,
    /// Titre du morceau (peut être vide).
    pub title: String,
    /// Artiste ou chaîne (peut être vide).
    pub artist: String,
    /// Petit mot : « écoute le refrain ! ».
    pub message: String,
    /// Qui la recommande.
    pub added_by: String,
    pub created_at: i64,
    pub updated_at: i64,
    /// Écoutée par l'autre ?
    pub listened: bool,
    pub listened_by: String,
    pub listened_at: Option<i64>,
}

/// Un lien YouTube Music reconnu (un morceau).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// [`KIND_TRACK`] (le seul type pour l'instant).
    pub kind: &'static str,
    pub id: String,
}

impl Link {
    /// Forme courte gardée dans le document.
    pub fn code(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }

    /// Relit une forme courte (venue du document : donc vérifiée).
    pub fn from_code(code: &str) -> Option<Self> {
        let (kind, id) = code.split_once(':')?;
        (kind == KIND_TRACK && valid_video_id(id)).then(|| Self::track(id))
    }

    fn track(id: &str) -> Self {
        Self {
            kind: KIND_TRACK,
            id: id.to_owned(),
        }
    }

    /// Adresse à ouvrir (dans l'appli YouTube Music si elle est installée).
    pub fn url(&self) -> String {
        format!("https://music.youtube.com/watch?v={}", self.id)
    }

    /// Miniature du morceau (hébergée par YouTube).
    pub fn thumbnail(&self) -> String {
        format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", self.id)
    }
}

fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Valeur d'un paramètre de l'adresse (`?v=…&list=…`).
fn query_param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// Reconnaît une adresse YouTube Music ou YouTube.
fn parse_url(url: &str) -> Option<Link> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    // Fragment (`#…`) ignoré.
    let rest = rest.split('#').next().unwrap_or(rest);
    let (host_and_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = host_and_path.split_once('/').unwrap_or((host_and_path, ""));
    let host = host.to_ascii_lowercase();
    let path = path.trim_end_matches('/');
    if host == "youtu.be" {
        return valid_video_id(path).then(|| Link::track(path));
    }
    if ![
        "music.youtube.com",
        "www.youtube.com",
        "youtube.com",
        "m.youtube.com",
    ]
    .contains(&host.as_str())
    {
        return None;
    }
    match path.split_once('/').unwrap_or((path, "")) {
        // Un morceau lu depuis une playlist garde son lien de morceau
        // (`&list=…` ignoré) : c'est bien lui qui est recommandé.
        ("watch", "") => {
            let video = query_param(query, "v")?;
            valid_video_id(video).then(|| Link::track(video))
        }
        ("shorts", id) if valid_video_id(id) => Some(Link::track(id)),
        _ => None,
    }
}

/// Trouve un lien YouTube Music (ou YouTube) dans un texte : un lien collé, ou
/// le texte envoyé par le menu Partager d'Android, qui peut entourer le lien
/// d'autres mots.
pub fn parse_link(text: &str) -> Result<Link> {
    if text.chars().count() > MAX_SHARED_TEXT {
        return Err(Error::InvalidMusicLink("texte trop long".to_owned()));
    }
    text.split_whitespace()
        .filter_map(|word| {
            let start = word.find("http")?;
            let url = &word[start..];
            // Le lien s'arrête au premier caractère qu'une adresse ne
            // contient pas (parenthèse, guillemet…) ; un point final n'en
            // fait pas partie.
            let end = url
                .find(|c: char| !(c.is_ascii_alphanumeric() || "-._~:/?#&=%+".contains(c)))
                .unwrap_or(url.len());
            parse_url(url[..end].trim_end_matches(['.', ',']))
        })
        .next()
        .ok_or_else(|| {
            Error::InvalidMusicLink(
                "aucun lien de morceau YouTube Music ou YouTube trouvé (une musique à la fois, pas de playlist)"
                    .to_owned(),
            )
        })
}

/// Adresse oEmbed de YouTube (sans clé) qui donne le titre et l'artiste.
pub fn oembed_url(link: &Link) -> String {
    format!(
        "https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{}",
        link.id
    )
}

#[derive(Deserialize)]
struct OEmbed {
    #[serde(default)]
    title: String,
    #[serde(default)]
    author_name: String,
}

/// Lit la réponse oEmbed : (titre, artiste). Les chaînes automatiques de
/// YouTube Music s'appellent « Artiste - Topic » : on garde l'artiste.
pub fn parse_oembed(bytes: &[u8]) -> Result<(String, String)> {
    let o: OEmbed = serde_json::from_slice(bytes)
        .map_err(|e| Error::InvalidMusicLink(format!("réponse de YouTube illisible ({e})")))?;
    let artist = o.author_name.trim();
    let artist = artist.strip_suffix(" - Topic").unwrap_or(artist);
    Ok((truncate(&o.title, MAX_TITLE), truncate(artist, MAX_TITLE)))
}

/// Coupe après `max` caractères puis enlève les espaces autour : nettoyer
/// deux fois donne le même résultat.
fn truncate(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = flat.chars().take(max).collect();
    cut.trim().to_owned()
}

/// Titre, artiste et mot nettoyés (refusés s'ils sont trop longs).
fn clean_texts(title: &str, artist: &str, message: &str) -> Result<(String, String, String)> {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let artist = artist.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.chars().count() > MAX_TITLE || artist.chars().count() > MAX_TITLE {
        return Err(Error::MusicTextTooLong(MAX_TITLE));
    }
    let message = message.trim();
    if message.chars().count() > MAX_MESSAGE {
        return Err(Error::MusicTextTooLong(MAX_MESSAGE));
    }
    Ok((title, artist, message.to_owned()))
}

/// Le document des musiques. Comme pour [`crate::NotesDoc`], `Clone` garde
/// l'identifiant d'appareil : utiliser [`MusicDoc::fork`] pour simuler un
/// autre appareil.
#[derive(Debug, Default, Clone)]
pub struct MusicDoc {
    doc: AutoCommit,
}

impl MusicDoc {
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

    /// Toutes les entrées lisibles, doublons compris.
    fn all(&self) -> Vec<Music> {
        self.doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect()
    }

    /// Toutes les musiques lisibles : d'abord celles à écouter (la plus
    /// récente en premier), puis celles déjà écoutées (la dernière écoutée en
    /// premier). Un même morceau recommandé sur deux appareils avant leur
    /// synchro n'apparaît qu'une fois : la recommandation la plus ancienne
    /// est gardée, la même partout.
    pub fn list(&self) -> Vec<Music> {
        let mut all = self.all();
        all.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<Music> = all
            .into_iter()
            .filter(|m| seen.insert(m.link.clone()))
            .collect();
        list.sort_by(|a, b| {
            a.listened
                .cmp(&b.listened)
                .then_with(|| {
                    let at = |m: &Music| {
                        if m.listened {
                            m.listened_at.unwrap_or(m.updated_at)
                        } else {
                            m.created_at
                        }
                    };
                    at(b).cmp(&at(a))
                })
                .then_with(|| a.id.cmp(&b.id))
        });
        list
    }

    /// Nombre d'entrées du document qui ne sont pas des musiques valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    /// Une entrée venue d'un autre appareil n'est gardée que si elle respecte
    /// les mêmes règles qu'une entrée créée ici.
    fn read(&self, key: &str) -> Option<Music> {
        hydrate_prop::<_, Music, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|m| {
                m.id == key
                    && Link::from_code(&m.link).is_some()
                    && !m.added_by.trim().is_empty()
                    && clean_texts(&m.title, &m.artist, &m.message).ok()
                        == Some((m.title.clone(), m.artist.clone(), m.message.clone()))
            })
    }

    pub fn get(&self, id: &str) -> Result<Music> {
        self.read(id)
            .ok_or_else(|| Error::MusicNotFound(id.to_owned()))
    }

    /// Vérifie qu'un ajout serait accepté (même règles que [`MusicDoc::add`]),
    /// sans rien changer : l'appli évite ainsi d'interroger YouTube pour rien.
    pub fn check_new(
        &self,
        author: &str,
        link: &Link,
        title: &str,
        artist: &str,
        message: &str,
    ) -> Result<(String, String, String)> {
        if author.trim().is_empty() {
            return Err(Error::MissingAuthor);
        }
        let code = link.code();
        if Link::from_code(&code).as_ref() != Some(link) {
            return Err(Error::InvalidMusicLink(code));
        }
        let texts = clean_texts(title, artist, message)?;
        if let Some(existing) = self.list().into_iter().find(|m| m.link == code) {
            let name = if existing.title.is_empty() {
                "cette musique".to_owned()
            } else {
                format!("« {} »", existing.title)
            };
            return Err(Error::MusicAlreadyAdded(name, existing.added_by));
        }
        Ok(texts)
    }

    /// Ajoute une recommandation à partir d'un lien (ou d'un texte qui en
    /// contient un). Refusée si ce morceau est déjà dans la liste.
    pub fn add(
        &mut self,
        author: &str,
        link: &Link,
        title: &str,
        artist: &str,
        message: &str,
        now_ms: i64,
    ) -> Result<Music> {
        let (title, artist, message) = self.check_new(author, link, title, artist, message)?;
        let author = author.trim();
        let code = link.code();
        let music = Music {
            id: uuid::Uuid::new_v4().to_string(),
            link: code,
            title,
            artist,
            message,
            added_by: author.to_owned(),
            created_at: now_ms,
            updated_at: now_ms,
            listened: false,
            listened_by: String::new(),
            listened_at: None,
        };
        reconcile_prop(&mut self.doc, ROOT, music.id.as_str(), &music)?;
        Ok(music)
    }

    /// Modifie le titre, l'artiste et le petit mot.
    pub fn update(
        &mut self,
        id: &str,
        title: &str,
        artist: &str,
        message: &str,
        now_ms: i64,
    ) -> Result<Music> {
        let mut music = self.get(id)?;
        let (title, artist, message) = clean_texts(title, artist, message)?;
        music.title = title;
        music.artist = artist;
        music.message = message;
        music.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &music)?;
        Ok(music)
    }

    /// Note la musique écoutée (ou pas encore).
    pub fn set_listened(
        &mut self,
        id: &str,
        listened: bool,
        by: &str,
        now_ms: i64,
    ) -> Result<Music> {
        let mut music = self.get(id)?;
        let by = by.trim();
        if listened && by.is_empty() {
            return Err(Error::MissingAuthor);
        }
        music.listened = listened;
        music.listened_by = if listened {
            by.to_owned()
        } else {
            String::new()
        };
        music.listened_at = listened.then_some(now_ms);
        music.updated_at = now_ms;
        reconcile_prop(&mut self.doc, ROOT, id, &music)?;
        Ok(music)
    }

    /// Supprime la musique, et ses doublons venus d'un autre appareil
    /// (sinon l'un d'eux réapparaîtrait à sa place).
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::MusicNotFound(id.to_owned()));
        }
        let twins: Vec<String> = match self.read(id) {
            Some(music) => self
                .all()
                .into_iter()
                .filter(|m| m.link == music.link && m.id != id)
                .map(|m| m.id)
                .collect(),
            None => Vec::new(),
        };
        self.doc.delete(ROOT, id)?;
        for twin in twins {
            self.doc.delete(ROOT, twin.as_str())?;
        }
        Ok(())
    }

    pub fn merge(&mut self, other: &mut MusicDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for MusicDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    const VIDEO: &str = "dQw4w9WgXcQ";

    fn track() -> Link {
        parse_link(&format!("https://music.youtube.com/watch?v={VIDEO}&si=abc")).unwrap()
    }

    #[test]
    fn recognizes_youtube_music_links() {
        let cases = [
            (
                format!("https://music.youtube.com/watch?v={VIDEO}&si=x1"),
                "titre:dQw4w9WgXcQ",
            ),
            (
                format!("https://music.youtube.com/watch?v={VIDEO}&list=RDAMVM{VIDEO}"),
                "titre:dQw4w9WgXcQ",
            ),
            (
                format!("https://www.youtube.com/watch?v={VIDEO}"),
                "titre:dQw4w9WgXcQ",
            ),
            (
                format!("https://m.youtube.com/watch?feature=share&v={VIDEO}"),
                "titre:dQw4w9WgXcQ",
            ),
            (
                format!("https://youtu.be/{VIDEO}?si=xyz"),
                "titre:dQw4w9WgXcQ",
            ),
            (
                format!("https://youtube.com/shorts/{VIDEO}"),
                "titre:dQw4w9WgXcQ",
            ),
            // Texte autour du lien (menu Partager), majuscules dans l'hôte.
            (
                format!("Écoute ça : https://Music.YouTube.com/watch?v={VIDEO} 🎶"),
                "titre:dQw4w9WgXcQ",
            ),
            (format!("(https://youtu.be/{VIDEO})"), "titre:dQw4w9WgXcQ"),
            (
                format!("Écoute https://youtu.be/{VIDEO}."),
                "titre:dQw4w9WgXcQ",
            ),
            (format!("«https://youtu.be/{VIDEO}»"), "titre:dQw4w9WgXcQ"),
        ];
        for (text, code) in cases {
            let link = parse_link(&text).unwrap_or_else(|e| panic!("{text} : {e}"));
            assert_eq!(link.code(), code, "{text}");
            assert_eq!(Link::from_code(code), Some(link));
        }
    }

    #[test]
    fn refuses_other_links() {
        for bad in [
            "",
            "pas de lien",
            "https://open.spotify.com/track/abc",
            "https://music.youtube.com.evil.fr/watch?v=dQw4w9WgXcQ",
            "https://evil.fr/https://youtu.be",
            "https://music.youtube.com/watch?v=court",
            "https://music.youtube.com/watch?v=dQw4w9WgXc<",
            "https://music.youtube.com/watch",
            "https://music.youtube.com/playlist?list=PLabc_DEF-123",
            "https://music.youtube.com/browse/MPREb_AbCdEf123",
            "https://music.youtube.com/channel/UC123",
            "javascript:alert(1)",
            "ftp://youtu.be/dQw4w9WgXcQ",
        ] {
            assert!(parse_link(bad).is_err(), "{bad}");
        }
        assert!(parse_link(&"a".repeat(3000)).is_err());
        for code in [
            "titre:court",
            "video:dQw4w9WgXcQ",
            "playlist:PLabc",
            "titre:",
            "x",
        ] {
            assert!(Link::from_code(code).is_none(), "{code}");
        }
    }

    #[test]
    fn rebuilds_urls_from_the_code() {
        assert_eq!(
            track().url(),
            format!("https://music.youtube.com/watch?v={VIDEO}")
        );
        assert_eq!(
            track().thumbnail(),
            format!("https://i.ytimg.com/vi/{VIDEO}/mqdefault.jpg")
        );
        assert_eq!(
            oembed_url(&track()),
            format!("https://www.youtube.com/oembed?format=json&url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3D{VIDEO}")
        );
    }

    #[test]
    fn reads_oembed() {
        let body = br#"{"title":"Never Gonna  Give You Up","author_name":"Rick Astley - Topic","type":"video","thumbnail_url":"https://i.ytimg.com/vi/x/hqdefault.jpg"}"#;
        assert_eq!(
            parse_oembed(body).unwrap(),
            (
                "Never Gonna Give You Up".to_owned(),
                "Rick Astley".to_owned()
            )
        );
        assert_eq!(
            parse_oembed(br#"{"title":"Sans artiste"}"#).unwrap(),
            ("Sans artiste".to_owned(), String::new())
        );
        assert!(parse_oembed(b"Not Found").is_err());
        let long = format!(r#"{{"title":"{}"}}"#, "é".repeat(500));
        assert_eq!(
            parse_oembed(long.as_bytes()).unwrap().0.chars().count(),
            MAX_TITLE
        );
    }

    #[test]
    fn add_listen_update_delete() {
        let mut doc = MusicDoc::new();
        let music = doc
            .add(
                "Eve",
                &track(),
                " Never  Gonna ",
                "Rick Astley",
                " Le refrain ! ",
                1,
            )
            .unwrap();
        assert_eq!(music.title, "Never Gonna");
        assert_eq!(music.message, "Le refrain !");
        assert!(!music.listened);
        assert_eq!(doc.list(), vec![music.clone()]);

        let heard = doc.set_listened(&music.id, true, "Lucas", 2).unwrap();
        assert!(heard.listened);
        assert_eq!(heard.listened_by, "Lucas");
        assert_eq!(heard.listened_at, Some(2));
        let again = doc.set_listened(&music.id, false, "", 3).unwrap();
        assert!(!again.listened && again.listened_by.is_empty() && again.listened_at.is_none());

        let edited = doc.update(&music.id, "Titre", "", "", 4).unwrap();
        assert_eq!(
            (edited.title.as_str(), edited.artist.as_str()),
            ("Titre", "")
        );

        doc.delete(&music.id).unwrap();
        assert!(doc.list().is_empty());
        assert!(matches!(doc.get(&music.id), Err(Error::MusicNotFound(_))));
        assert!(matches!(
            doc.delete(&music.id),
            Err(Error::MusicNotFound(_))
        ));
    }

    #[test]
    fn rejects_invalid_input() {
        let mut doc = MusicDoc::new();
        assert!(matches!(
            doc.add(" ", &track(), "", "", "", 1),
            Err(Error::MissingAuthor)
        ));
        assert!(matches!(
            doc.add("Eve", &track(), &"a".repeat(MAX_TITLE + 1), "", "", 1),
            Err(Error::MusicTextTooLong(MAX_TITLE))
        ));
        assert!(matches!(
            doc.add("Eve", &track(), "", "", &"m".repeat(MAX_MESSAGE + 1), 1),
            Err(Error::MusicTextTooLong(MAX_MESSAGE))
        ));
        let forged = Link {
            kind: KIND_TRACK,
            id: "pas valide".to_owned(),
        };
        assert!(matches!(
            doc.add("Eve", &forged, "", "", "", 1),
            Err(Error::InvalidMusicLink(_))
        ));
        let music = doc.add("Eve", &track(), "Titre", "", "", 1).unwrap();
        // Même morceau, autre forme de lien : déjà là.
        let same = parse_link(&format!("https://youtu.be/{VIDEO}")).unwrap();
        let err = doc.add("Lucas", &same, "", "", "", 2).unwrap_err();
        assert_eq!(
            err.to_string(),
            "« Titre » est déjà dans la liste (recommandée par Eve)"
        );
        assert!(matches!(
            doc.set_listened(&music.id, true, " ", 2),
            Err(Error::MissingAuthor)
        ));
    }

    #[test]
    fn list_order() {
        let mut doc = MusicDoc::new();
        let link = |id: &str| Link::from_code(&format!("titre:{id}")).unwrap();
        let old = doc.add("Eve", &link("aaaaaaaaaaa"), "", "", "", 1).unwrap();
        let heard_first = doc.add("Eve", &link("bbbbbbbbbbb"), "", "", "", 2).unwrap();
        let recent = doc.add("Eve", &link("ccccccccccc"), "", "", "", 3).unwrap();
        let heard_last = doc.add("Eve", &link("ddddddddddd"), "", "", "", 4).unwrap();
        doc.set_listened(&heard_first.id, true, "Lucas", 10)
            .unwrap();
        doc.set_listened(&heard_last.id, true, "Lucas", 11).unwrap();
        // Modifier le mot ne fait pas remonter une musique à écouter.
        doc.update(&old.id, "", "", "mot", 12).unwrap();
        let ids: Vec<_> = doc.list().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, vec![recent.id, old.id, heard_last.id, heard_first.id]);
    }

    #[test]
    fn survives_save_and_load() {
        let mut doc = MusicDoc::new();
        let a = doc
            .add("Lucas", &track(), "Titre", "Artiste", "mot", 1)
            .unwrap();
        let reloaded = MusicDoc::load(&doc.save()).unwrap();
        assert_eq!(reloaded.get(&a.id).unwrap(), a);
    }

    #[test]
    fn concurrent_additions_are_kept() {
        let mut pc = MusicDoc::new();
        let mut phone = pc.fork();
        pc.add("Lucas", &track(), "", "", "", 1).unwrap();
        let other = Link::from_code("titre:aaaaaaaaaaa").unwrap();
        phone.add("Eve", &other, "", "", "", 1).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }

    #[test]
    fn concurrent_listen_and_edit_converge() {
        let mut pc = MusicDoc::new();
        let music = pc.add("Eve", &track(), "Titre", "", "", 1).unwrap();
        let mut phone = pc.fork();
        pc.update(&music.id, "Titre", "Artiste", "Écoute-la", 2)
            .unwrap();
        phone.set_listened(&music.id, true, "Lucas", 3).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        let merged = pc.get(&music.id).unwrap();
        assert!(merged.listened);
        assert_eq!(merged.listened_by, "Lucas");
        assert_eq!(merged.message, "Écoute-la");
        assert_eq!(merged.artist, "Artiste");
    }

    #[test]
    fn concurrent_delete_wins_over_listen() {
        let mut pc = MusicDoc::new();
        let music = pc.add("Eve", &track(), "", "", "", 1).unwrap();
        let mut phone = pc.fork();
        pc.delete(&music.id).unwrap();
        phone.set_listened(&music.id, true, "Lucas", 2).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert!(pc.list().is_empty() && phone.list().is_empty());
    }

    #[test]
    fn unreadable_or_invalid_entries_are_ignored() {
        let mut doc = MusicDoc::new();
        let good = doc.add("Eve", &track(), "", "", "", 1).unwrap();
        doc.doc.put(ROOT, "bizarre", "pas une musique").unwrap();
        // Bien formée, mais avec un lien qui ne vient pas de YouTube.
        let mut evil = good.clone();
        evil.id = "piege".to_owned();
        evil.link = "titre:https://evil".to_owned();
        reconcile_prop(&mut doc.doc, ROOT, "piege", &evil).unwrap();
        assert_eq!(doc.list(), vec![good]);
        assert_eq!(doc.unreadable_count(), 2);
    }

    #[test]
    fn same_track_added_on_two_devices_shows_once() {
        let mut pc = MusicDoc::new();
        let mut phone = pc.fork();
        let first = pc.add("Lucas", &track(), "", "", "", 1).unwrap();
        let other = phone.add("Eve", &track(), "", "", "mot", 2).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        // La plus ancienne, la même sur les deux appareils.
        assert_eq!(pc.list(), vec![first.clone()]);
        assert_eq!(phone.list(), vec![first.clone()]);
        // La supprimer ne fait pas réapparaître l'autre.
        pc.delete(&first.id).unwrap();
        assert!(pc.list().is_empty());
        assert!(pc.get(&other.id).is_err());
    }

    #[test]
    fn long_shared_text_counts_characters() {
        // 1500 caractères accentués (plus de 2000 octets) puis le lien.
        let text = format!("{} https://youtu.be/{VIDEO}", "é".repeat(1500));
        assert!(parse_link(&text).is_ok());
    }

    #[test]
    fn syncs_with_protocol() {
        let mut pc = MusicDoc::new();
        let mut phone = MusicDoc::new();
        pc.add("Lucas", &track(), "", "", "", 1).unwrap();
        let other = Link::from_code("titre:aaaaaaaaaaa").unwrap();
        phone.add("Eve", &other, "", "", "", 2).unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
    }
}
