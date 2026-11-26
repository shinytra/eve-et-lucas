//! Commandes Tauri des fichiers partagés. Le contenu passe par le nœud de
//! synchro (iroh-blobs), la liste par le document `files`.

use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use couple_core::{files::MAX_SIZE, SharedFile};
use serde::Serialize;
use tauri::{ipc::Response, AppHandle, Manager, State};

use crate::{now_ms, store::AppState, sync::SyncHandle};

/// Taille maximale d'un fichier envoyé à l'interface pour un aperçu.
const PREVIEW_MAX: u64 = 20 * 1024 * 1024;

/// Un fichier partagé, tel que l'interface le montre.
#[derive(Debug, Serialize)]
pub struct FileView {
    #[serde(flatten)]
    file: SharedFile,
    /// Le contenu est présent sur cet appareil.
    available: bool,
    is_image: bool,
}

fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
}

#[tauri::command]
pub async fn list_files(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
) -> Result<Vec<FileView>, String> {
    let files = state.files.lock().list();
    let node = sync.node().ok();
    let mut views = Vec::with_capacity(files.len());
    for file in files {
        let available = match node {
            Some(node) => node.has_blob(&file.hash).await,
            None => false,
        };
        let is_image = file.is_image();
        views.push(FileView {
            file,
            available,
            is_image,
        });
    }
    Ok(views)
}

/// Envois en cours : le contenu arrive par morceaux (en base64) dans un
/// fichier temporaire, puis est rangé d'un coup. Ça marche aussi sur Android,
/// où l'interface ne peut pas envoyer de binaire brut à Rust.
#[derive(Default)]
pub struct Uploads(Mutex<HashMap<String, Upload>>);

struct Upload {
    path: PathBuf,
    file: std::fs::File,
    name: String,
    mime: String,
    author: String,
    size: u64,
    written: u64,
}

/// Nombre maximal d'envois simultanés.
const MAX_UPLOADS: usize = 4;
/// Taille maximale d'un morceau (avant base64).
const MAX_CHUNK: usize = 4 * 1024 * 1024;

impl Uploads {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Upload>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn uploads_dir(state: &AppState) -> PathBuf {
    state.data_dir.join("fichiers").join("envois")
}

/// Commence le partage d'un fichier de `size` octets ; renvoie l'identifiant
/// de l'envoi.
#[tauri::command]
pub fn upload_start(
    state: State<'_, Arc<AppState>>,
    uploads: State<'_, Uploads>,
    name: String,
    mime: String,
    author: String,
    size: u64,
) -> Result<String, String> {
    let fail = |e: String| format!("Impossible de partager le fichier : {e}");
    if size > MAX_SIZE {
        return Err(fail(format!(
            "un fichier partagé ne doit pas dépasser {} Mo",
            MAX_SIZE / (1024 * 1024)
        )));
    }
    let name = couple_core::files::clean_name(&name).map_err(|e| fail(e.to_string()))?;
    if author.trim().is_empty() {
        return Err(fail("il manque le prénom".to_owned()));
    }
    let mut uploads = uploads.lock();
    if uploads.len() >= MAX_UPLOADS {
        return Err(fail("trop d'envois en cours".to_owned()));
    }
    let dir = uploads_dir(&state);
    if uploads.is_empty() {
        // Restes d'un envoi interrompu (appli fermée en plein envoi).
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir).map_err(|e| fail(e.to_string()))?;
    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join(format!("{id}.part"));
    let file = std::fs::File::create(&path).map_err(|e| fail(e.to_string()))?;
    uploads.insert(
        id.clone(),
        Upload {
            path,
            file,
            name,
            mime,
            author,
            size,
            written: 0,
        },
    );
    Ok(id)
}

/// Ajoute un morceau (base64) à un envoi.
#[tauri::command]
pub fn upload_chunk(uploads: State<'_, Uploads>, id: String, data: String) -> Result<(), String> {
    let bytes = data_encoding::BASE64
        .decode(data.as_bytes())
        .map_err(|_| "morceau de fichier illisible".to_owned())?;
    if bytes.len() > MAX_CHUNK {
        return Err("morceau de fichier trop gros".to_owned());
    }
    let mut uploads = uploads.lock();
    let upload = uploads.get_mut(&id).ok_or("envoi inconnu ou annulé")?;
    if upload.written + bytes.len() as u64 > upload.size {
        return Err("le fichier est plus gros qu'annoncé".to_owned());
    }
    upload
        .file
        .write_all(&bytes)
        .map_err(|e| format!("Impossible d'écrire le fichier : {e}"))?;
    upload.written += bytes.len() as u64;
    Ok(())
}

/// Abandonne un envoi.
#[tauri::command]
pub fn upload_cancel(uploads: State<'_, Uploads>, id: String) {
    if let Some(upload) = uploads.lock().remove(&id) {
        drop(upload.file);
        let _ = std::fs::remove_file(upload.path);
    }
}

/// Termine un envoi : le contenu est rangé et le fichier apparaît chez tous.
#[tauri::command]
pub async fn upload_finish(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
    uploads: State<'_, Uploads>,
    id: String,
) -> Result<SharedFile, String> {
    let upload = uploads
        .lock()
        .remove(&id)
        .ok_or("envoi inconnu ou annulé")?;
    let Upload {
        path,
        file,
        name,
        mime,
        author,
        size,
        written,
    } = upload;
    let result = async {
        if written != size {
            return Err("le fichier est incomplet".to_owned());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let node = sync.node()?;
        // Pas de ménage entre le rangement et l'ajout à la liste.
        let _guard = state.blob_guard.lock().await;
        let hash = node
            .add_blob_from_path(&path)
            .await
            .map_err(|e| format!("Impossible d'enregistrer le fichier : {e}"))?;
        let added = state.files.change("de partager le fichier", |doc| {
            doc.add(&author, &name, size, &hash, &mime, now_ms())
        });
        if added.is_err() && !state.files.lock().hashes().contains(&hash) {
            let _ = node.forget_blob(&hash).await;
        }
        added
    }
    .await;
    let _ = std::fs::remove_file(&path);
    let file = result?;
    changed(&state);
    Ok(file)
}

#[tauri::command]
pub async fn delete_file(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
    id: String,
) -> Result<(), String> {
    let hash = state
        .files
        .lock()
        .get(&id)
        .map_err(|e| format!("Impossible de supprimer le fichier : {e}"))?
        .hash;
    let _guard = state.blob_guard.lock().await;
    state
        .files
        .change("de supprimer le fichier", |doc| doc.delete(&id))?;
    changed(&state);
    // Le même contenu peut être partagé deux fois : on ne l'oublie que s'il
    // n'est plus utilisé.
    if !state.files.lock().hashes().contains(&hash) {
        if let Ok(node) = sync.node() {
            if let Err(e) = node.forget_blob(&hash).await {
                eprintln!("Contenu {hash} non libéré : {e}");
            }
        }
    }
    Ok(())
}

/// Télécharge le contenu depuis un autre appareil.
#[tauri::command]
pub async fn download_file(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
    id: String,
) -> Result<(), String> {
    let hash = state.files.lock().get(&id).map_err(|e| e.to_string())?.hash;
    sync.node()?
        .fetch_blob(&hash)
        .await
        .map_err(|e| format!("Téléchargement impossible (l'autre appareil est-il allumé ?) : {e}"))
}

/// Contenu brut d'un fichier présent sur cet appareil, pour un aperçu.
#[tauri::command]
pub async fn file_content(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
    id: String,
) -> Result<Response, String> {
    let file = state.files.lock().get(&id).map_err(|e| e.to_string())?;
    if file.size > PREVIEW_MAX {
        return Err("fichier trop gros pour un aperçu".to_owned());
    }
    let bytes = sync
        .node()?
        .read_blob(&file.hash)
        .await
        .map_err(|e| format!("Fichier indisponible sur cet appareil : {e}"))?;
    Ok(Response::new(bytes))
}

/// Premier chemin libre : « nom.ext », puis « nom (2).ext », etc.
fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, format!(".{ext}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap_or(candidate)
}

/// Enregistre le fichier dans le dossier Téléchargements et renvoie son chemin.
///
/// Sur Android, ce dossier serait celui, privé, de l'appli (invisible dans
/// « Fichiers ») : l'enregistrement y viendra avec un plugin MediaStore.
#[tauri::command]
pub async fn save_file(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
    id: String,
) -> Result<String, String> {
    let file = state.files.lock().get(&id).map_err(|e| e.to_string())?;
    // Sous Linux, le dossier n'est pas toujours déclaré (xdg-user-dirs) :
    // on se rabat alors sur ~/Downloads, créé au besoin.
    let dir = match app.path().download_dir() {
        Ok(dir) => dir,
        Err(_) => app
            .path()
            .home_dir()
            .map(|home| home.join("Downloads"))
            .map_err(|e| format!("Dossier Téléchargements introuvable : {e}"))?,
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Dossier Téléchargements inaccessible : {e}"))?;
    // Le nom vient d'un autre appareil : on le nettoie encore (aucun dossier,
    // aucun nom réservé, longueur raisonnable).
    let name = couple_core::files::disk_name(&file.name).map_err(|e| e.to_string())?;
    let target = free_path(&dir, &name);
    sync.node()?
        .export_blob(&file.hash, &target)
        .await
        .map_err(|e| format!("Impossible d'enregistrer le fichier : {e}"))?;
    Ok(target.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_path_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("eve-free-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::write(dir.join("a (2).txt"), b"").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (3).txt"));
        std::fs::write(dir.join("README"), b"").unwrap();
        assert_eq!(free_path(&dir, "README"), dir.join("README (2)"));
        std::fs::write(dir.join(".env"), b"").unwrap();
        assert_eq!(free_path(&dir, ".env"), dir.join(".env (2)"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
