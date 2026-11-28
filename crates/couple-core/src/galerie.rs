//! Galerie photos : des photos en vrac ou rangées dans des albums.
//!
//! Le document `galerie` est partagé. Même principe de fusion que les notes :
//! une table `clé -> élément` à la racine, identifiants UUID v4, la clé est
//! `photo:<id>` ou `album:<id>`. Une photo n'est qu'une référence vers un
//! fichier partagé (document `files`) : son contenu voyage déjà par
//! iroh-blobs, le PC en garde une copie et un téléphone télécharge d'office
//! les images de 10 Mo au plus.
//!
//! Une photo n'a pas besoin d'album (`album_id` vide : « en vrac »). Si un
//! album est supprimé sur un appareil pendant qu'un autre y range une photo,
//! la photo n'est jamais perdue : un album inconnu vaut « en vrac ».

use std::collections::HashSet;

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{files::SharedFile, sync::Syncable, Error, Result};

/// Longueur maximale du nom d'un album, en caractères.
pub const MAX_ALBUM_NAME: usize = 60;
/// Nombre maximal d'albums.
pub const MAX_ALBUMS: usize = 100;
/// Taille maximale d'une photo (l'aperçu de l'interface est limité à 20 Mo).
pub const MAX_PHOTO_SIZE: u64 = 20 * 1024 * 1024;

const PHOTO_PREFIX: &str = "photo:";
const ALBUM_PREFIX: &str = "album:";

/// Une photo de la galerie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Photo {
    pub id: String,
    /// Fichier partagé montré (document `files`).
    pub file_id: String,
    /// Album où elle est rangée ; vide = en vrac.
    pub album_id: String,
    /// Qui l'a ajoutée.
    pub added_by: String,
    pub added_at: i64,
}

/// Un album.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub created_by: String,
    pub created_at: i64,
}

/// Une photo avec son fichier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhotoView {
    #[serde(flatten)]
    pub photo: Photo,
    pub file: SharedFile,
}

/// Un fichier que la galerie sait montrer.
pub fn is_usable(file: &SharedFile) -> bool {
    file.is_image() && file.size <= MAX_PHOTO_SIZE
}

/// Nettoie le nom d'un album (espaces superflus, caractères de contrôle).
pub fn clean_album_name(name: &str) -> Result<String> {
    let cleaned = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>();
    if cleaned.is_empty() {
        return Err(Error::InvalidGalerie(
            "un album doit avoir un nom".to_owned(),
        ));
    }
    if cleaned.chars().count() > MAX_ALBUM_NAME {
        return Err(Error::InvalidGalerie(format!(
            "le nom d'un album ne doit pas dépasser {MAX_ALBUM_NAME} caractères"
        )));
    }
    Ok(cleaned)
}

/// Le document de la galerie. `Clone` garde l'identifiant d'appareil :
/// utiliser [`GalerieDoc::fork`] pour simuler un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct GalerieDoc {
    doc: AutoCommit,
}

impl GalerieDoc {
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

    pub fn merge(&mut self, other: &mut GalerieDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    /// Une entrée venue d'un autre appareil n'est gardée que si elle respecte
    /// les mêmes règles qu'une entrée créée ici.
    fn read_photo(&self, id: &str) -> Option<Photo> {
        let key = format!("{PHOTO_PREFIX}{id}");
        hydrate_prop::<_, Photo, _, _>(&self.doc, ROOT, key.as_str())
            .ok()
            .filter(|p| p.id == id && !p.file_id.is_empty() && !p.added_by.trim().is_empty())
    }

    fn read_album(&self, id: &str) -> Option<Album> {
        let key = format!("{ALBUM_PREFIX}{id}");
        hydrate_prop::<_, Album, _, _>(&self.doc, ROOT, key.as_str())
            .ok()
            .filter(|a| a.id == id && clean_album_name(&a.name).is_ok_and(|n| n == a.name))
    }

    /// Albums, du plus ancien au plus récent.
    pub fn albums(&self) -> Vec<Album> {
        let mut albums: Vec<Album> = self
            .doc
            .keys(ROOT)
            .filter_map(|key| {
                key.strip_prefix(ALBUM_PREFIX)
                    .and_then(|id| self.read_album(id))
            })
            .collect();
        albums.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        albums.truncate(MAX_ALBUMS);
        albums
    }

    /// Photos dont le fichier existe encore, la plus récente en premier.
    /// Une photo rangée dans un album qui n'existe plus est « en vrac ».
    pub fn photos(&self, files: &[SharedFile]) -> Vec<PhotoView> {
        let albums: HashSet<String> = self.albums().into_iter().map(|a| a.id).collect();
        let mut photos: Vec<PhotoView> = self
            .doc
            .keys(ROOT)
            .filter_map(|key| {
                let photo = self.read_photo(key.strip_prefix(PHOTO_PREFIX)?)?;
                let file = files.iter().find(|f| f.id == photo.file_id)?;
                is_usable(file).then(|| PhotoView {
                    photo: Photo {
                        album_id: if albums.contains(&photo.album_id) {
                            photo.album_id
                        } else {
                            String::new()
                        },
                        ..photo
                    },
                    file: file.clone(),
                })
            })
            .collect();
        photos.sort_by(|a, b| {
            b.photo
                .added_at
                .cmp(&a.photo.added_at)
                .then_with(|| a.photo.id.cmp(&b.photo.id))
        });
        photos
    }

    /// Ajoute une photo (un fichier partagé), en vrac ou dans un album.
    pub fn add_photo(
        &mut self,
        author: &str,
        file: &SharedFile,
        album_id: &str,
        now_ms: i64,
    ) -> Result<Photo> {
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        if !file.is_image() {
            return Err(Error::InvalidGalerie(
                "seules les images (PNG, JPEG, GIF, WebP) vont dans la galerie".to_owned(),
            ));
        }
        if file.size > MAX_PHOTO_SIZE {
            return Err(Error::InvalidGalerie(format!(
                "une photo ne doit pas dépasser {} Mo",
                MAX_PHOTO_SIZE / (1024 * 1024)
            )));
        }
        if !album_id.is_empty() && self.read_album(album_id).is_none() {
            return Err(Error::AlbumNotFound(album_id.to_owned()));
        }
        let photo = Photo {
            id: uuid::Uuid::new_v4().to_string(),
            file_id: file.id.clone(),
            album_id: album_id.to_owned(),
            added_by: author.to_owned(),
            added_at: now_ms,
        };
        let key = format!("{PHOTO_PREFIX}{}", photo.id);
        reconcile_prop(&mut self.doc, ROOT, key.as_str(), &photo)?;
        Ok(photo)
    }

    /// Range la photo dans un album, ou la remet en vrac (`album_id` vide).
    pub fn move_photo(&mut self, id: &str, album_id: &str) -> Result<Photo> {
        if !album_id.is_empty() && self.read_album(album_id).is_none() {
            return Err(Error::AlbumNotFound(album_id.to_owned()));
        }
        let mut photo = self
            .read_photo(id)
            .ok_or_else(|| Error::PhotoNotFound(id.to_owned()))?;
        if photo.album_id != album_id {
            photo.album_id = album_id.to_owned();
            let key = format!("{PHOTO_PREFIX}{id}");
            reconcile_prop(&mut self.doc, ROOT, key.as_str(), &photo)?;
        }
        Ok(photo)
    }

    /// Retire la photo de la galerie (le fichier partagé n'est pas touché ici).
    pub fn remove_photo(&mut self, id: &str) -> Result<()> {
        let key = format!("{PHOTO_PREFIX}{id}");
        if self.doc.get(ROOT, key.as_str())?.is_none() {
            return Err(Error::PhotoNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, key.as_str())?;
        Ok(())
    }

    pub fn add_album(&mut self, author: &str, name: &str, now_ms: i64) -> Result<Album> {
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        let name = clean_album_name(name)?;
        if self.albums().len() >= MAX_ALBUMS {
            return Err(Error::InvalidGalerie(format!(
                "{MAX_ALBUMS} albums au plus : supprimes-en un d'abord"
            )));
        }
        let album = Album {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            created_by: author.to_owned(),
            created_at: now_ms,
        };
        let key = format!("{ALBUM_PREFIX}{}", album.id);
        reconcile_prop(&mut self.doc, ROOT, key.as_str(), &album)?;
        Ok(album)
    }

    pub fn rename_album(&mut self, id: &str, name: &str) -> Result<Album> {
        let name = clean_album_name(name)?;
        let mut album = self
            .read_album(id)
            .ok_or_else(|| Error::AlbumNotFound(id.to_owned()))?;
        if album.name != name {
            album.name = name;
            let key = format!("{ALBUM_PREFIX}{id}");
            reconcile_prop(&mut self.doc, ROOT, key.as_str(), &album)?;
        }
        Ok(album)
    }

    /// Supprime l'album ; ses photos restent dans la galerie, en vrac.
    pub fn remove_album(&mut self, id: &str) -> Result<()> {
        let key = format!("{ALBUM_PREFIX}{id}");
        if self.doc.get(ROOT, key.as_str())?.is_none() {
            return Err(Error::AlbumNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, key.as_str())?;
        Ok(())
    }
}

impl Syncable for GalerieDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{files::FilesDoc, sync::tests::sync_pair};

    const HASH: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
    const HASH2: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn files() -> (SharedFile, SharedFile, SharedFile) {
        let mut doc = FilesDoc::new();
        let a = doc
            .add("Lucas", "a.jpg", 1000, HASH, "image/jpeg", 1)
            .unwrap();
        let b = doc
            .add("Eve", "b.png", 2000, HASH2, "image/png", 2)
            .unwrap();
        let text = doc
            .add("Eve", "liste.txt", 10, HASH2, "text/plain", 3)
            .unwrap();
        (a, b, text)
    }

    #[test]
    fn photos_loose_or_in_albums() {
        let (a, b, text) = files();
        let all = vec![a.clone(), b.clone(), text.clone()];
        let mut doc = GalerieDoc::new();
        let album = doc.add_album("Eve", "  Vacances   2026 ", 1).unwrap();
        assert_eq!(album.name, "Vacances 2026");
        let p1 = doc.add_photo("Lucas", &a, "", 10).unwrap();
        let p2 = doc.add_photo("Eve", &b, &album.id, 20).unwrap();

        let photos = doc.photos(&all);
        assert_eq!(photos.len(), 2);
        assert_eq!(photos[0].photo, p2, "la plus récente d'abord");
        assert_eq!(photos[0].photo.album_id, album.id);
        assert_eq!(photos[1].photo.album_id, "");

        doc.move_photo(&p1.id, &album.id).unwrap();
        assert!(doc
            .photos(&all)
            .iter()
            .all(|p| p.photo.album_id == album.id));
        doc.move_photo(&p1.id, "").unwrap();
        assert_eq!(doc.photos(&all)[1].photo.album_id, "");

        assert!(doc.add_photo("Eve", &text, "", 1).is_err(), "pas une image");
        assert!(doc.add_photo("", &a, "", 1).is_err(), "sans prénom");
        assert!(matches!(
            doc.add_photo("Eve", &a, "inconnu", 1),
            Err(Error::AlbumNotFound(_))
        ));
        assert!(matches!(
            doc.move_photo(&p1.id, "inconnu"),
            Err(Error::AlbumNotFound(_))
        ));
        let big = SharedFile {
            size: MAX_PHOTO_SIZE + 1,
            ..a.clone()
        };
        assert!(doc.add_photo("Eve", &big, "", 1).is_err());

        doc.remove_photo(&p2.id).unwrap();
        assert!(doc.remove_photo(&p2.id).is_err());
        assert_eq!(doc.photos(&all).len(), 1);
    }

    #[test]
    fn album_names_are_checked() {
        let mut doc = GalerieDoc::new();
        assert!(doc.add_album("Eve", "   ", 1).is_err());
        assert!(doc
            .add_album("Eve", &"x".repeat(MAX_ALBUM_NAME + 1), 1)
            .is_err());
        assert!(doc.add_album("", "Noël", 1).is_err());
        let album = doc.add_album("Eve", "Noël", 1).unwrap();
        assert_eq!(
            doc.rename_album(&album.id, "Noël 2026").unwrap().name,
            "Noël 2026"
        );
        assert!(doc.rename_album(&album.id, "").is_err());
        assert!(matches!(
            doc.rename_album("inconnu", "x"),
            Err(Error::AlbumNotFound(_))
        ));
    }

    #[test]
    fn deleting_an_album_keeps_its_photos_loose() {
        let (a, _, _) = files();
        let all = vec![a.clone()];
        let mut doc = GalerieDoc::new();
        let album = doc.add_album("Eve", "Plage", 1).unwrap();
        doc.add_photo("Eve", &a, &album.id, 2).unwrap();
        doc.remove_album(&album.id).unwrap();
        assert!(doc.albums().is_empty());
        let photos = doc.photos(&all);
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].photo.album_id, "");
        assert!(doc.remove_album(&album.id).is_err());
    }

    #[test]
    fn missing_file_hides_the_photo() {
        let (a, b, _) = files();
        let mut doc = GalerieDoc::new();
        doc.add_photo("Eve", &a, "", 1).unwrap();
        doc.add_photo("Eve", &b, "", 2).unwrap();
        assert_eq!(doc.photos(std::slice::from_ref(&b)).len(), 1);
    }

    #[test]
    fn two_devices_merge_without_losing_anything() {
        let (a, b, _) = files();
        let all = vec![a.clone(), b.clone()];
        let mut lucas = GalerieDoc::new();
        let album = lucas.add_album("Lucas", "Nous", 1).unwrap();
        let mut eve = lucas.fork();

        // Hors ligne : chacun ajoute une photo, Eve crée un album.
        lucas.add_photo("Lucas", &a, &album.id, 10).unwrap();
        eve.add_photo("Eve", &b, "", 11).unwrap();
        let eve_album = eve.add_album("Eve", "Chats", 12).unwrap();

        sync_pair(&mut lucas, &mut eve);
        assert_eq!(lucas.photos(&all), eve.photos(&all));
        assert_eq!(lucas.photos(&all).len(), 2);
        assert_eq!(lucas.albums().len(), 2);
        assert_eq!(lucas.albums(), eve.albums());

        // Eve supprime « Nous » pendant que Lucas y range l'autre photo :
        // aucune photo n'est perdue.
        let loose = eve
            .photos(&all)
            .into_iter()
            .find(|p| p.photo.album_id.is_empty())
            .unwrap();
        eve.remove_album(&album.id).unwrap();
        lucas.move_photo(&loose.photo.id, &album.id).unwrap();
        sync_pair(&mut lucas, &mut eve);
        assert_eq!(lucas.photos(&all), eve.photos(&all));
        assert_eq!(eve.photos(&all).len(), 2);
        assert!(eve.photos(&all).iter().all(|p| p.photo.album_id.is_empty()));
        assert_eq!(eve.albums().len(), 1);
        assert_eq!(eve.albums()[0].id, eve_album.id);
    }

    #[test]
    fn save_and_load_roundtrip_and_unreadable_entries_are_ignored() {
        let (a, _, _) = files();
        let all = vec![a.clone()];
        let mut doc = GalerieDoc::new();
        doc.add_photo("Eve", &a, "", 1).unwrap();
        // Entrée illisible venue d'ailleurs : ignorée, jamais bloquante.
        doc.doc.put(ROOT, "photo:cassee", "n'importe quoi").unwrap();
        doc.doc.put(ROOT, "album:vide", "").unwrap();
        let bytes = doc.save();
        let loaded = GalerieDoc::load(&bytes).unwrap();
        assert_eq!(loaded.photos(&all).len(), 1);
        assert!(loaded.albums().is_empty());
    }
}
