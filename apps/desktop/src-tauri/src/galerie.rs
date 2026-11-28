//! Galerie photos : photos en vrac ou rangées dans des albums.
//!
//! La liste est le document partagé `galerie` (`couple_core::galerie`) ; le
//! contenu des photos est celui des fichiers partagés (document `files`,
//! envoyé par `upload_*` de `files.rs`) : mêmes transferts, même copie sur le
//! PC, et les téléphones téléchargent d'office les images de 10 Mo au plus.

use std::sync::Arc;

use couple_core::{Album, Photo, PhotoView};
use serde::Serialize;
use tauri::State;

use crate::{
    now_ms,
    store::{AppState, GALERIE_DOC},
    sync::SyncHandle,
};

/// Une photo, telle que l'interface la montre.
#[derive(Debug, Serialize)]
pub struct PhotoItem {
    #[serde(flatten)]
    view: PhotoView,
    /// Le contenu est présent sur cet appareil (aperçu possible).
    available: bool,
}

/// Toute la galerie.
#[derive(Debug, Serialize)]
pub struct Galerie {
    albums: Vec<Album>,
    photos: Vec<PhotoItem>,
}

fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
    couple_sync::Store::changed(state, &[GALERIE_DOC.to_owned()]);
}

#[tauri::command]
pub async fn list_galerie(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
) -> Result<Galerie, String> {
    let files = state.files.lock().list();
    let (albums, views) = {
        let doc = state.galerie.lock();
        (doc.albums(), doc.photos(&files))
    };
    let node = sync.node().ok();
    let mut photos = Vec::with_capacity(views.len());
    for view in views {
        let available = match node {
            Some(node) => node.has_blob(&view.file.hash).await,
            None => false,
        };
        photos.push(PhotoItem { view, available });
    }
    Ok(Galerie { albums, photos })
}

/// Met un fichier déjà partagé dans la galerie (`album_id` vide : en vrac).
#[tauri::command]
pub fn add_photo(
    state: State<'_, Arc<AppState>>,
    author: String,
    file_id: String,
    album_id: String,
) -> Result<Photo, String> {
    let file = state
        .files
        .lock()
        .get(&file_id)
        .map_err(|_| "Ce fichier n'existe plus.".to_owned())?;
    let photo = state.galerie.change("d'ajouter la photo", |d| {
        d.add_photo(&author, &file, &album_id, now_ms())
    })?;
    changed(&state);
    Ok(photo)
}

/// Range la photo dans un album, ou la remet en vrac (`album_id` vide).
#[tauri::command]
pub fn move_photo(
    state: State<'_, Arc<AppState>>,
    id: String,
    album_id: String,
) -> Result<Photo, String> {
    let photo = state
        .galerie
        .change("de ranger la photo", |d| d.move_photo(&id, &album_id))?;
    changed(&state);
    Ok(photo)
}

/// Retire la photo de la galerie. L'interface supprime ensuite le fichier
/// partagé (`delete_file`) pour libérer la place.
#[tauri::command]
pub fn remove_photo(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .galerie
        .change("de retirer la photo", |d| d.remove_photo(&id))?;
    changed(&state);
    Ok(())
}

#[tauri::command]
pub fn add_album(
    state: State<'_, Arc<AppState>>,
    author: String,
    name: String,
) -> Result<Album, String> {
    let album = state.galerie.change("de créer l'album", |d| {
        d.add_album(&author, &name, now_ms())
    })?;
    changed(&state);
    Ok(album)
}

#[tauri::command]
pub fn rename_album(
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
) -> Result<Album, String> {
    let album = state
        .galerie
        .change("de renommer l'album", |d| d.rename_album(&id, &name))?;
    changed(&state);
    Ok(album)
}

/// Supprime l'album ; ses photos restent dans la galerie, en vrac.
#[tauri::command]
pub fn remove_album(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .galerie
        .change("de supprimer l'album", |d| d.remove_album(&id))?;
    changed(&state);
    Ok(())
}
