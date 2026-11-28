//! Mise à jour de l'appli depuis l'appli (écran Paramètres).
//!
//! Les versions sont publiées sur le dépôt (public) de `couple_core::update`,
//! avec un fichier `latest.json`.
//!
//! - **PC** : plugin updater de Tauri. Il télécharge l'installateur, vérifie
//!   sa signature (clé publique dans `tauri.conf.json`), l'installe sans
//!   question puis relance l'appli.
//! - **Téléphone** : l'appli télécharge l'APK dans `cache/maj/`, vérifie son
//!   empreinte SHA-256 annoncée dans `latest.json`, puis ouvre l'installeur
//!   d'Android (`tauri-plugin-apk-install`) : il reste à appuyer sur « Mettre
//!   à jour ». Android refuse en plus un APK signé avec une autre clé.
//!
//! Rien n'est vérifié tout seul en arrière-plan : l'interface demande à
//! l'ouverture et quand la fenêtre revient au premier plan, au plus toutes les
//! 6 heures (réponse gardée en mémoire), ou quand on appuie sur « Rechercher ».

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime, State};

/// Délai entre deux vérifications automatiques.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Ce que l'écran Paramètres affiche.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    /// Version installée.
    pub current: String,
    /// Nouvelle version, s'il y en a une.
    pub available: Option<String>,
    /// Notes de la nouvelle version (courtes).
    pub notes: String,
    /// Faux si ce système n'a pas de mise à jour automatique (PC Linux).
    pub supported: bool,
}

/// Avancement du téléchargement (évènement `update-progress`).
#[derive(Debug, Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

/// Mise à jour trouvée, à installer.
#[cfg(desktop)]
type Pending = tauri_plugin_updater::Update;
#[cfg(mobile)]
type Pending = couple_core::update::AndroidUpdate;

#[derive(Default)]
pub struct UpdateState {
    last: Mutex<Option<(Instant, UpdateStatus, Option<Pending>)>>,
    /// Un seul téléchargement à la fois.
    busy: AtomicBool,
}

impl UpdateState {
    fn last(&self) -> std::sync::MutexGuard<'_, Option<(Instant, UpdateStatus, Option<Pending>)>> {
        self.last
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Libère `busy` même en cas d'erreur.
struct Busy<'a>(&'a AtomicBool);

impl<'a> Busy<'a> {
    fn take(flag: &'a AtomicBool) -> Result<Self, String> {
        if flag.swap(true, Ordering::SeqCst) {
            return Err("Une mise à jour est déjà en cours de téléchargement.".to_owned());
        }
        Ok(Busy(flag))
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn current_version<R: Runtime>(app: &AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

/// Cherche une nouvelle version. `force` ignore la réponse gardée en mémoire.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    force: bool,
) -> Result<UpdateStatus, String> {
    if !force {
        if let Some((at, status, _)) = state.last().as_ref() {
            if at.elapsed() < CHECK_EVERY {
                return Ok(status.clone());
            }
        }
    }
    let (status, pending) = platform::check(&app).await?;
    *state.last() = Some((Instant::now(), status.clone(), pending));
    Ok(status)
}

/// Télécharge et installe la version trouvée par [`check_update`].
/// Renvoie `"started"` (installeur ouvert ou appli relancée) ou
/// `"needsPermission"` (Android : autoriser l'installation, puis recommencer).
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<String, String> {
    let _busy = Busy::take(&state.busy)?;
    let pending = state
        .last()
        .as_ref()
        .and_then(|(_, _, pending)| pending.clone())
        .ok_or("Aucune mise à jour à installer : recherche d'abord une nouvelle version.")?;
    platform::install(&app, pending).await
}

fn progress<R: Runtime>(app: &AppHandle<R>, downloaded: u64, total: Option<u64>) {
    let _ = app.emit("update-progress", Progress { downloaded, total });
}

#[cfg(desktop)]
mod platform {
    use tauri::{AppHandle, Url};
    use tauri_plugin_updater::{Error, UpdaterExt};

    use super::{current_version, progress, Pending, UpdateStatus};

    pub async fn check(app: &AppHandle) -> Result<(UpdateStatus, Option<Pending>), String> {
        let url = Url::parse(&couple_core::update::manifest_url()).map_err(|e| e.to_string())?;
        let updater = app
            .updater_builder()
            .endpoints(vec![url])
            .and_then(|b| b.build())
            .map_err(|e| format!("Mise à jour impossible : {e}"))?;
        let mut status = UpdateStatus {
            current: current_version(app),
            available: None,
            notes: String::new(),
            supported: true,
        };
        match updater.check().await {
            Ok(Some(update)) => {
                status.available = Some(update.version.clone());
                status.notes =
                    couple_core::update::clean_notes(update.body.as_deref().unwrap_or_default());
                Ok((status, Some(update)))
            }
            Ok(None) => Ok((status, None)),
            // Pas d'installateur pour ce système (seul Windows est publié).
            Err(Error::TargetNotFound(_) | Error::TargetsNotFound(_)) => {
                status.supported = false;
                Ok((status, None))
            }
            Err(e) => Err(format!(
                "Impossible de vérifier les mises à jour ({e}). Vérifie la connexion à Internet."
            )),
        }
    }

    pub async fn install(app: &AppHandle, update: Pending) -> Result<String, String> {
        let mut downloaded = 0u64;
        update
            .download_and_install(
                |chunk, total| {
                    downloaded += chunk as u64;
                    progress(app, downloaded, total);
                },
                || {},
            )
            .await
            .map_err(|e| format!("Mise à jour impossible : {e}"))?;
        // Sous Windows, l'installateur a déjà fermé l'appli ; ailleurs, on la relance.
        app.restart()
    }
}

#[cfg(mobile)]
mod platform {
    use std::{path::PathBuf, time::Duration};

    use couple_core::update::{self, AndroidUpdate, MAX_APK_BYTES, MAX_MANIFEST_BYTES};
    use sha2::{Digest, Sha256};
    use tauri::{AppHandle, Manager};
    use tauri_plugin_apk_install::{ApkInstallExt, InstallResult};
    use tokio::io::AsyncWriteExt;

    use super::{current_version, progress, Pending, UpdateStatus};

    /// Hôtes de GitHub vers lesquels une adresse de version peut rediriger.
    const HOSTS: &[&str] = &[
        "github.com",
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
    ];

    /// Comme `animes::client` (certificats de Mozilla), mais les
    /// redirections de GitHub vers son stockage sont suivies, en HTTPS et
    /// vers ses seuls hôtes.
    fn client(timeout: Duration) -> Result<reqwest::Client, String> {
        let roots =
            rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let redirect = reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            let allowed =
                url.scheme() == "https" && url.host_str().is_some_and(|h| HOSTS.contains(&h));
            if attempt.previous().len() >= 5 || !allowed {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            .user_agent("eve-et-lucas")
            .redirect(redirect)
            .build()
            .map_err(|e| e.to_string())
    }

    pub async fn check(app: &AppHandle) -> Result<(UpdateStatus, Option<Pending>), String> {
        let current = current_version(app);
        let response = client(Duration::from_secs(30))?
            .get(update::manifest_url())
            .send()
            .await
            .map_err(|e| {
                format!("Impossible de vérifier les mises à jour ({e}). Vérifie la connexion à Internet.")
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "Impossible de vérifier les mises à jour (réponse {}).",
                response.status()
            ));
        }
        let bytes = crate::animes::read_limited(response, MAX_MANIFEST_BYTES).await?;
        let found = update::android_update(&bytes, &current).map_err(|e| e.to_string())?;
        let status = UpdateStatus {
            current,
            available: found.as_ref().map(|u| u.version.clone()),
            notes: found.as_ref().map(|u| u.notes.clone()).unwrap_or_default(),
            supported: true,
        };
        Ok((status, found))
    }

    fn dir(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("maj"))
    }

    pub async fn install(app: &AppHandle, update: AndroidUpdate) -> Result<String, String> {
        let dir = dir(app)?;
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(|e| format!("Dossier de mise à jour impossible à créer : {e}"))?;
        let part = dir.join("eve-et-lucas.apk.part");
        let apk = dir.join("eve-et-lucas.apk");

        // Déjà téléchargé et vérifié (nouvel appui, installeur peut-être
        // encore ouvert) : on le garde tel quel. Sinon, un seul APK à la fois.
        if file_sha256(&apk).await.as_deref() != Some(update.sha256.as_str()) {
            let _ = tokio::fs::remove_file(&apk).await;
            let result = download(app, &update, &part).await;
            if let Err(e) = result {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e);
            }
            tokio::fs::rename(&part, &apk)
                .await
                .map_err(|e| format!("APK impossible à enregistrer : {e}"))?;
        }

        // Appel bloquant (réponse d'Android) : hors des tâches asynchrones.
        let handle = app.clone();
        let result =
            tauri::async_runtime::spawn_blocking(move || handle.apk_install().install(&apk))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
        Ok(match result {
            InstallResult::Started => "started",
            InstallResult::NeedsPermission => "needsPermission",
        }
        .to_owned())
    }

    /// Empreinte d'un fichier déjà téléchargé, s'il existe.
    async fn file_sha256(path: &std::path::Path) -> Option<String> {
        use tokio::io::AsyncReadExt;
        let mut file = tokio::fs::File::open(path).await.ok()?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = file.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Some(data_encoding::HEXLOWER.encode(&hasher.finalize()))
    }

    async fn download(
        app: &AppHandle,
        update: &AndroidUpdate,
        part: &std::path::Path,
    ) -> Result<(), String> {
        // Adresse déjà vérifiée par le cœur (page des versions du dépôt).
        let mut response = client(Duration::from_secs(15 * 60))?
            .get(&update.url)
            .send()
            .await
            .map_err(|e| {
                format!("Téléchargement impossible ({e}). Vérifie la connexion à Internet.")
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "Téléchargement impossible (réponse {}).",
                response.status()
            ));
        }
        let total = response.content_length();
        if total.is_some_and(|t| t > MAX_APK_BYTES) {
            return Err("APK trop gros : téléchargement annulé.".to_owned());
        }
        let mut file = tokio::fs::File::create(part)
            .await
            .map_err(|e| format!("APK impossible à enregistrer : {e}"))?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("Téléchargement interrompu : {e}"))?
        {
            downloaded += chunk.len() as u64;
            if downloaded > MAX_APK_BYTES {
                return Err("APK trop gros : téléchargement annulé.".to_owned());
            }
            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("APK impossible à enregistrer : {e}"))?;
            progress(app, downloaded, total);
        }
        file.sync_all()
            .await
            .map_err(|e| format!("APK impossible à enregistrer : {e}"))?;
        let digest = data_encoding::HEXLOWER.encode(&hasher.finalize());
        if digest != update.sha256 {
            return Err(
                "Le fichier téléchargé ne correspond pas à la version publiée : mise à jour annulée."
                    .to_owned(),
            );
        }
        Ok(())
    }
}
