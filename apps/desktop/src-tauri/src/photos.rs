//! Widget photo : images et GIF choisis pour le widget « Photo » d'Android.
//!
//! La liste est le document partagé `cadres` (`couple_core::cadres`) : une
//! image mise sur le widget par Eve apparaît aussi sur celui de Lucas. Les
//! commandes marchent aussi sur PC (on choisit depuis n'importe quel
//! appareil) ; seul le téléphone prépare les images et dessine le widget.
//!
//! Sur téléphone, chaque image est préparée une seule fois (découpe des GIF,
//! réduction : `couple_core::cadres::prepare`) dans `widget-photo/<empreinte>/`,
//! puis confiée au widget. Rien ne tourne en continu : la préparation ne se
//! fait que quand la liste change ou qu'une image arrive d'un autre appareil.

use std::sync::Arc;

use couple_core::{Cadre, CadreView};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{
    now_ms,
    store::{AppState, CADRES_DOC},
    sync::SyncHandle,
};

/// Une image du widget, telle que l'interface la montre.
#[derive(Debug, Serialize)]
pub struct CadreItem {
    #[serde(flatten)]
    view: CadreView,
    /// Le contenu est présent sur cet appareil (aperçu possible).
    available: bool,
}

fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
    couple_sync::Store::changed(state, &[CADRES_DOC.to_owned()]);
}

#[tauri::command]
pub async fn list_cadres(
    state: State<'_, Arc<AppState>>,
    sync: State<'_, SyncHandle>,
) -> Result<Vec<CadreItem>, String> {
    let files = state.files.lock().list();
    let views = state.cadres.lock().list(&files);
    let node = sync.node().ok();
    let mut items = Vec::with_capacity(views.len());
    for view in views {
        let available = match node {
            Some(node) => node.has_blob(&view.file.hash).await,
            None => false,
        };
        items.push(CadreItem { view, available });
    }
    Ok(items)
}

#[tauri::command]
pub fn add_cadre(
    state: State<'_, Arc<AppState>>,
    author: String,
    file_id: String,
) -> Result<Cadre, String> {
    let files = state.files.lock().list();
    let file = files
        .iter()
        .find(|f| f.id == file_id)
        .ok_or("Ce fichier n'existe plus.")?
        .clone();
    let cadre = state
        .cadres
        .change("de mettre l'image sur le widget", |d| {
            d.add(&author, &file, &files, now_ms())
        })?;
    changed(&state);
    Ok(cadre)
}

#[tauri::command]
pub fn set_cadre_fit(
    state: State<'_, Arc<AppState>>,
    id: String,
    fit: String,
) -> Result<Cadre, String> {
    let cadre = state
        .cadres
        .change("de changer l'affichage de l'image", |d| {
            d.set_fit(&id, &fit)
        })?;
    changed(&state);
    Ok(cadre)
}

#[tauri::command]
pub fn remove_cadre(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .cadres
        .change("de retirer l'image du widget", |d| d.remove(&id))?;
    changed(&state);
    Ok(())
}

/// Lance la préparation des images du widget (sans effet sur PC).
#[cfg_attr(not(mobile), allow(unused_variables))]
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    #[cfg(mobile)]
    phone::start(app, state);
}

/// Préparation des images sur le disque (aussi testée sur PC).
#[cfg_attr(not(mobile), allow(dead_code))]
mod prepared {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use couple_core::cadres::Prepared;
    use serde::{Deserialize, Serialize};

    use crate::store::write_atomic;

    /// Description d'une image préparée, écrite en dernier : un dossier sans
    /// elle est incomplet (appli arrêtée en pleine préparation).
    const META: &str = "image.json";
    /// Image que le cœur n'a pas su lire : on ne réessaie pas à chaque fois.
    const UNREADABLE: &str = "illisible";

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Meta {
        pub width: u32,
        pub height: u32,
        pub frame_ms: u32,
        pub count: usize,
        pub sequence: Vec<usize>,
    }

    impl Meta {
        /// Chemins des images différentes, par indice (l'ordre d'affichage
        /// est donné par `sequence`).
        pub fn frames(&self, dir: &Path) -> Vec<String> {
            (0..self.count)
                .map(|i| frame_path(dir, i).display().to_string())
                .collect()
        }
    }

    fn frame_path(dir: &Path, i: usize) -> PathBuf {
        dir.join(format!("{i}.png"))
    }

    /// Image déjà préparée et complète, ou `None`.
    pub fn read(dir: &Path) -> Option<Meta> {
        let meta: Meta = serde_json::from_slice(&fs::read(dir.join(META)).ok()?).ok()?;
        let complete = meta.count > 0
            && meta.sequence.iter().all(|&i| i < meta.count)
            && (0..meta.count).all(|i| frame_path(dir, i).is_file());
        complete.then_some(meta)
    }

    pub fn is_unreadable(dir: &Path) -> bool {
        dir.join(UNREADABLE).exists()
    }

    pub fn mark_unreadable(dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dir)?;
        fs::write(dir.join(UNREADABLE), b"")
    }

    /// Écrit les images puis leur description.
    pub fn write(dir: &Path, prepared: &Prepared) -> std::io::Result<Meta> {
        // Restes d'une préparation interrompue.
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir)?;
        for (i, png) in prepared.frames.iter().enumerate() {
            write_atomic(&frame_path(dir, i), png)?;
        }
        let meta = Meta {
            width: prepared.width,
            height: prepared.height,
            frame_ms: prepared.frame_ms,
            count: prepared.frames.len(),
            sequence: prepared.sequence.clone(),
        };
        let json = serde_json::to_vec(&meta).map_err(std::io::Error::other)?;
        write_atomic(&dir.join(META), &json)?;
        Ok(meta)
    }

    /// Efface les dossiers d'images qui ne sont plus sur le widget.
    pub fn clean(root: &Path, keep: &[String]) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) {
                if let Err(e) = fs::remove_dir_all(entry.path()) {
                    eprintln!("Ancienne image du widget non effacée ({name}) : {e}");
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn written_images_are_read_back_and_cleaned() {
            let root = std::env::temp_dir().join(format!("eve-photos-{}", std::process::id()));
            let dir = root.join("abc");
            let prepared = Prepared {
                width: 2,
                height: 1,
                frame_ms: 100,
                frames: vec![vec![1], vec![2]],
                sequence: vec![0, 1, 1],
            };
            assert!(read(&dir).is_none());
            let meta = write(&dir, &prepared).unwrap();
            assert_eq!(read(&dir), Some(meta.clone()));
            assert_eq!(meta.frames(&dir).len(), 2);
            // Une image manquante : préparation à refaire.
            fs::remove_file(dir.join("1.png")).unwrap();
            assert!(read(&dir).is_none());

            let other = root.join("def");
            mark_unreadable(&other).unwrap();
            assert!(is_unreadable(&other));
            clean(&root, &["abc".to_owned()]);
            assert!(dir.exists());
            assert!(!other.exists());
            fs::remove_dir_all(root).unwrap();
        }
    }
}

#[cfg(mobile)]
mod phone {
    use std::{sync::Arc, time::Duration};

    use couple_core::cadres::prepare;
    use serde::Serialize;
    use tauri::{AppHandle, Manager};
    use tauri_plugin_home_widget::HomeWidgetExt;

    use super::prepared;
    use crate::{now_ms, store::AppState, sync::SyncHandle};

    /// Ce que le widget photo affiche (lu par `PhotoWidgetUpdater.kt`).
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PhotoData {
        images: Vec<PhotoImage>,
        /// Images pas encore arrivées sur ce téléphone.
        pending: usize,
        updated_at: i64,
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct PhotoImage {
        id: String,
        name: String,
        fit: String,
        width: u32,
        height: u32,
        /// 0 pour une image fixe.
        frame_ms: u32,
        /// Chemins des images différentes.
        frames: Vec<String>,
        /// Ordre d'affichage (indices dans `frames`).
        sequence: Vec<usize>,
    }

    pub fn start(app: &AppHandle, state: Arc<AppState>) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut sent: Option<String> = None;
            // Délai avant un nouvel essai après un échec (synchro pas encore
            // démarrée…) : 10 s, puis de plus en plus long, 15 min au plus.
            let mut backoff = Duration::from_secs(10);
            loop {
                let retry = match refresh(&app, &state, sent.as_deref()).await {
                    Ok(json) => {
                        sent = Some(json);
                        backoff = Duration::from_secs(10);
                        None
                    }
                    Err(e) => {
                        eprintln!("Widget photo pas encore à jour : {e}");
                        let delay = backoff;
                        backoff = (backoff * 2).min(Duration::from_secs(15 * 60));
                        Some(delay)
                    }
                };
                // Pas de minuterie sinon : rien ne change tant que la liste
                // ou les fichiers ne changent pas (batterie).
                match retry {
                    Some(delay) => {
                        tokio::select! {
                            () = state.photos_wanted.notified() => {}
                            () = tokio::time::sleep(delay) => {}
                        }
                    }
                    None => state.photos_wanted.notified().await,
                }
            }
        });
    }

    /// Prépare les images manquantes et envoie la liste au widget si elle a
    /// changé ; renvoie ce qui est affiché (sans la date d'envoi).
    async fn refresh(
        app: &AppHandle,
        state: &AppState,
        last: Option<&str>,
    ) -> Result<String, String> {
        let files = state.files.lock().list();
        let views = state.cadres.lock().list(&files);
        let sync = app.state::<SyncHandle>();
        let node = sync.node()?;
        let root = state.data_dir.join("widget-photo");
        let mut images = Vec::new();
        let mut keep = Vec::new();
        let mut pending = 0;
        for view in views {
            let hash = view.file.hash.clone();
            // Empreinte vérifiée par le cœur (64 caractères hexadécimaux) :
            // un nom de dossier sûr.
            let dir = root.join(&hash);
            keep.push(hash.clone());
            let meta = match prepared::read(&dir) {
                Some(meta) => meta,
                None if prepared::is_unreadable(&dir) => continue,
                None => {
                    if !node.has_blob(&hash).await {
                        pending += 1;
                        continue;
                    }
                    // Un échec (lecture, disque plein…) ne bloque que cette
                    // image : elle est réessayée au prochain passage.
                    let bytes = match node.read_blob(&hash).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            eprintln!("Image {} du widget indisponible : {e}", view.file.name);
                            pending += 1;
                            continue;
                        }
                    };
                    let target = dir.clone();
                    let done =
                        tauri::async_runtime::spawn_blocking(move || match prepare(&bytes) {
                            Ok(p) => prepared::write(&target, &p).map(Some),
                            Err(e) => {
                                eprintln!("Image du widget illisible : {e}");
                                prepared::mark_unreadable(&target).map(|()| None)
                            }
                        })
                        .await;
                    match done {
                        Ok(Ok(Some(meta))) => meta,
                        Ok(Ok(None)) => continue,
                        Ok(Err(e)) => {
                            eprintln!("Préparation de l'image {} impossible : {e}", view.file.name);
                            pending += 1;
                            continue;
                        }
                        Err(e) => {
                            eprintln!(
                                "Préparation de l'image {} interrompue : {e}",
                                view.file.name
                            );
                            pending += 1;
                            continue;
                        }
                    }
                }
            };
            images.push(PhotoImage {
                id: view.cadre.id,
                name: view.file.name,
                fit: view.cadre.fit,
                width: meta.width,
                height: meta.height,
                frame_ms: meta.frame_ms,
                frames: meta.frames(&dir),
                sequence: meta.sequence,
            });
        }
        let mut data = PhotoData {
            images,
            pending,
            updated_at: 0,
        };
        let key = serde_json::to_string(&data).map_err(|e| e.to_string())?;
        if last == Some(key.as_str()) {
            return Ok(key);
        }
        data.updated_at = now_ms();
        let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
        let handle = app.clone();
        tauri::async_runtime::spawn_blocking(move || handle.home_widget().update_photos(json))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("widget photo non mis à jour : {e}"))?;
        // Le widget a reçu la nouvelle liste : les anciennes images peuvent partir.
        prepared::clean(&root, &keep);
        Ok(key)
    }
}
