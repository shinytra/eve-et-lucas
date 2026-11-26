//! Commandes Tauri des fichiers partagés. Le contenu passe par le nœud de
//! synchro (iroh-blobs), la liste par le document `files`.

use std::{path::PathBuf, sync::Arc};

use couple_core::{files::MAX_SIZE, SharedFile};
use serde::Serialize;
use tauri::{
    ipc::{InvokeBody, Request, Response},
    AppHandle, Manager, State,
};

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

/// Décode une valeur d'en-tête encodée par `encodeURIComponent`.
fn header(request: &Request<'_>, name: &str) -> Result<String, String> {
    let raw = request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    percent_decode(raw).ok_or_else(|| format!("en-tête {name} invalide"))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Partage un fichier. Le contenu arrive brut (corps de la requête), le nom,
/// le type et le prénom dans les en-têtes `x-name`, `x-mime` et `x-author`.
#[tauri::command]
pub async fn share_file(
    request: Request<'_>,
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
) -> Result<SharedFile, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("contenu du fichier manquant".to_owned());
    };
    let size = bytes.len() as u64;
    if size > MAX_SIZE {
        return Err(format!(
            "Impossible de partager le fichier : un fichier partagé ne doit pas dépasser {} Mo",
            MAX_SIZE / (1024 * 1024)
        ));
    }
    let name = header(&request, "x-name")?;
    let mime = header(&request, "x-mime")?;
    let author = header(&request, "x-author")?;
    // Vérifie le nom avant de ranger le contenu.
    couple_core::files::clean_name(&name)
        .map_err(|e| format!("Impossible de partager le fichier : {e}"))?;

    let node = sync.node()?;
    let hash = node
        .add_blob(bytes.clone())
        .await
        .map_err(|e| format!("Impossible d'enregistrer le fichier : {e}"))?;
    let added = state.files.change("de partager le fichier", |doc| {
        doc.add(&author, &name, size, &hash, &mime, now_ms())
    });
    match added {
        Ok(file) => {
            changed(&state);
            Ok(file)
        }
        Err(e) => {
            if !state.files.lock().hashes().contains(&hash) {
                let _ = node.forget_blob(&hash).await;
            }
            Err(e)
        }
    }
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
    // Le nom vient d'un autre appareil : on le nettoie encore (aucun dossier).
    let name = couple_core::files::clean_name(&file.name).map_err(|e| e.to_string())?;
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
    fn decodes_uri_components() {
        assert_eq!(
            percent_decode("photo%20d'%C3%A9t%C3%A9.jpg").unwrap(),
            "photo d'été.jpg"
        );
        assert_eq!(percent_decode("simple").unwrap(), "simple");
        assert!(percent_decode("%zz").is_none());
        assert!(percent_decode("%4").is_none());
        assert!(percent_decode("%FF").is_none());
    }

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
