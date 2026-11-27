//! Commandes Tauri des animés regardés ensemble, recherche sur AniList et
//! cache des affiches.
//!
//! La liste est le document `animes` (synchronisé). Les affiches, elles, sont
//! téléchargées par chaque appareil depuis AniList et gardées dans le dossier
//! `affiches/` : elles restent visibles hors ligne. Seules les adresses
//! hébergées par AniList sont acceptées (vérifié par le cœur), et seul ce qui
//! ressemble vraiment à une image est gardé.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use couple_core::{
    anilist,
    animes::{self, valid_cover_url},
    notify::Event,
    Anime, AnimeInfo,
};
use tauri::{ipc::Response, AppHandle, Emitter, State};

use crate::{
    now_ms,
    store::{write_atomic, AppState},
};

/// Taille maximale d'une réponse d'AniList.
const MAX_RESPONSE: usize = 1024 * 1024;
/// Taille maximale d'une affiche.
const MAX_COVER: usize = 3 * 1024 * 1024;
/// Nouvel essai des affiches manquantes (réseau revenu…).
const RETRY_EVERY: Duration = Duration::from_secs(10 * 60);

fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
    state.animes_wanted.notify_one();
}

fn covers_dir(state: &AppState) -> PathBuf {
    state.data_dir.join("affiches")
}

/// Fichier de l'affiche d'un animé : nommé par son identifiant AniList (un
/// nombre), jamais par un texte venu d'ailleurs.
fn cover_path(state: &AppState, anilist_id: i64) -> PathBuf {
    covers_dir(state).join(format!("{anilist_id}.img"))
}

/// Client HTTPS avec les autorités de certification de Mozilla
/// (`webpki-roots`) : le vérificateur du système demande, sur Android, une
/// initialisation côté Java que l'appli n'a pas.
pub(crate) fn client() -> Result<reqwest::Client, String> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent("eve-et-lucas")
        // Une redirection pourrait mener hors d'AniList : refusée.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())
}

/// Lit le corps d'une réponse sans dépasser `max` octets.
pub(crate) async fn read_limited(
    mut response: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len() + chunk.len() > max {
            return Err("réponse trop grosse".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Envoie une requête GraphQL à AniList et lit les animés de la réponse.
async fn query_anilist(body: String) -> Result<Vec<AnimeInfo>, String> {
    let response = client()?
        .post(anilist::ENDPOINT)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("AniList injoignable ({e}). Vérifie la connexion à Internet."))?;
    let status = response.status();
    let bytes = read_limited(response, MAX_RESPONSE).await?;
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("AniList reçoit trop de recherches : réessaie dans une minute.".to_owned());
    }
    // AniList répond aussi en JSON pour ses erreurs : le cœur les explique.
    anilist::parse_search(&bytes).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_animes(state: State<'_, Arc<AppState>>) -> Vec<Anime> {
    let doc = state.animes.lock();
    let unreadable = doc.unreadable_count();
    if unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les animés");
    }
    doc.list()
}

#[tauri::command]
pub async fn search_animes(query: String) -> Result<Vec<AnimeInfo>, String> {
    let body = anilist::search_body(&query).map_err(|e| e.to_string())?;
    query_anilist(body).await
}

#[tauri::command]
pub fn add_anime(
    state: State<'_, Arc<AppState>>,
    author: String,
    info: AnimeInfo,
) -> Result<Anime, String> {
    let anime = state
        .animes
        .change("d'ajouter l'animé", |doc| doc.add(&author, info, now_ms()))?;
    changed(&state);
    crate::notify::announce(&state, |me, at| Event::anime_added(&anime, me, at));
    Ok(anime)
}

#[tauri::command]
pub fn set_anime_episode(
    state: State<'_, Arc<AppState>>,
    id: String,
    episode: u32,
    by: String,
) -> Result<Anime, String> {
    // Seulement en avançant : revenir en arrière (erreur de clic) ne prévient pas.
    let (anime, advanced) = state.animes.change("de noter l'épisode", |doc| {
        let before = doc.get(&id).map(|a| a.episode).ok();
        doc.set_episode(&id, episode, &by, now_ms()).map(|a| {
            let advanced = before.is_some_and(|b| a.episode > b);
            (a, advanced)
        })
    })?;
    changed(&state);
    if advanced {
        crate::notify::announce(&state, |me, at| Event::anime_episode(&anime, &by, me, at));
    }
    Ok(anime)
}

#[tauri::command]
pub fn set_anime_status(
    state: State<'_, Arc<AppState>>,
    id: String,
    status: String,
) -> Result<Anime, String> {
    let anime = state.animes.change("de changer le statut", |doc| {
        doc.set_status(&id, &status, now_ms())
    })?;
    changed(&state);
    Ok(anime)
}

/// Relit les informations d'un animé sur AniList (nombre d'épisodes d'un
/// animé encore en diffusion, nouvelle affiche…).
#[tauri::command]
pub async fn refresh_anime(state: State<'_, Arc<AppState>>, id: String) -> Result<Anime, String> {
    let anime = state.animes.lock().get(&id).map_err(|e| e.to_string())?;
    let anilist_id = anime
        .info
        .anilist_id
        .ok_or_else(|| "Cet animé a été ajouté à la main : rien à mettre à jour.".to_owned())?;
    let body = anilist::by_id_body(anilist_id).map_err(|e| e.to_string())?;
    let info = query_anilist(body)
        .await?
        .into_iter()
        .find(|i| i.anilist_id == Some(anilist_id))
        .ok_or_else(|| "AniList ne connaît plus cet animé.".to_owned())?;
    let anime = state.animes.change("de mettre à jour l'animé", |doc| {
        doc.update_info(&id, info, now_ms())
    })?;
    changed(&state);
    Ok(anime)
}

#[tauri::command]
pub fn delete_anime(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .animes
        .change("de supprimer l'animé", |doc| doc.delete(&id))?;
    changed(&state);
    Ok(())
}

/// Les statuts possibles, pour l'interface.
#[tauri::command]
pub fn anime_statuses() -> Vec<&'static str> {
    animes::STATUSES.to_vec()
}

/// L'affiche d'un animé, si elle est déjà sur cet appareil.
/// Asynchrone (lecture hors du fil principal) : la liste en demande une par carte.
#[tauri::command]
pub async fn anime_cover(
    state: State<'_, Arc<AppState>>,
    anilist_id: i64,
) -> Result<Response, String> {
    if anilist_id <= 0 {
        return Err("identifiant invalide".to_owned());
    }
    let path = cover_path(&state, anilist_id);
    tauri::async_runtime::spawn_blocking(move || fs::read(path))
        .await
        .map_err(|e| e.to_string())?
        .map(Response::new)
        .map_err(|_| "affiche pas encore téléchargée".to_owned())
}

/// Reconnaît une image JPEG, PNG, WebP ou GIF à ses premiers octets.
fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"GIF8")
        || (bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP")
}

async fn download_cover(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    // Vérifié à nouveau ici, et les redirections sont refusées (voir `client`).
    if !valid_cover_url(url) {
        return Err("adresse refusée".to_owned());
    }
    let response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    let bytes = read_limited(response, MAX_COVER).await?;
    if !is_image(&bytes) {
        return Err("ce n'est pas une image".to_owned());
    }
    Ok(bytes)
}

/// Ce que l'appareil doit faire pour les affiches (calculé hors du runtime
/// asynchrone : lectures de fichiers).
struct CoverPlan {
    /// Affiches absentes ou dont l'adresse a changé (« Mettre à jour »).
    download: Vec<(i64, String)>,
}

/// Fichier qui garde l'adresse d'où vient l'affiche en cache.
fn source_path(state: &AppState, anilist_id: i64) -> PathBuf {
    covers_dir(state).join(format!("{anilist_id}.url"))
}

/// Liste les affiches à télécharger et supprime celles des animés retirés.
fn plan_covers(state: &AppState) -> std::io::Result<CoverPlan> {
    let dir = covers_dir(state);
    fs::create_dir_all(&dir)?;
    let covers = state.animes.lock().covers();
    let download = covers
        .iter()
        .filter(|(id, url)| {
            !cover_path(state, *id).exists()
                || fs::read_to_string(source_path(state, *id)).ok().as_deref() != Some(url.as_str())
        })
        .cloned()
        .collect();
    // Ménage : seulement nos fichiers `<nombre>.img`, `.url` (et temporaires).
    let keep: HashSet<String> = covers.iter().map(|(id, _)| id.to_string()).collect();
    for entry in fs::read_dir(&dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stem = [".img", ".url", ".tmp"]
            .iter()
            .find_map(|ext| name.strip_suffix(ext))
            .filter(|stem| !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit()));
        if let Some(stem) = stem {
            if !keep.contains(stem) {
                if let Err(e) = fs::remove_file(entry.path()) {
                    eprintln!("Affiche {name} non supprimée : {e}");
                }
            }
        }
    }
    Ok(CoverPlan { download })
}

/// Enregistre une affiche, puis l'adresse d'où elle vient.
fn save_cover(state: &AppState, anilist_id: i64, url: &str, bytes: &[u8]) -> std::io::Result<()> {
    write_atomic(&cover_path(state, anilist_id), bytes)?;
    write_atomic(&source_path(state, anilist_id), url.as_bytes())
}

/// Échecs récents : identifiant AniList -> (adresse, moment de l'échec).
type Failures = HashMap<i64, (String, Instant)>;

/// Télécharge les affiches manquantes et supprime celles des animés retirés
/// de la liste. Une affiche qui vient d'échouer n'est réessayée qu'après
/// [`RETRY_EVERY`]. Renvoie vrai si une affiche a été ajoutée.
async fn sync_covers(
    client: &reqwest::Client,
    state: &Arc<AppState>,
    failures: &mut Failures,
) -> bool {
    let planner = state.clone();
    let plan = match tauri::async_runtime::spawn_blocking(move || plan_covers(&planner)).await {
        Ok(Ok(plan)) => plan,
        Ok(Err(e)) => {
            eprintln!("Dossier des affiches inaccessible : {e}");
            return false;
        }
        Err(e) => {
            eprintln!("Préparation des affiches interrompue : {e}");
            return false;
        }
    };
    let mut added = false;
    for (anilist_id, url) in plan.download {
        let recent_failure = failures
            .get(&anilist_id)
            .is_some_and(|(failed_url, at)| *failed_url == url && at.elapsed() < RETRY_EVERY);
        if recent_failure {
            continue;
        }
        let bytes = match download_cover(client, &url).await {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("Affiche {anilist_id} non téléchargée : {e}");
                failures.insert(anilist_id, (url, Instant::now()));
                continue;
            }
        };
        let saver = state.clone();
        let saved = tauri::async_runtime::spawn_blocking(move || {
            save_cover(&saver, anilist_id, &url, &bytes)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
        match saved {
            Ok(()) => {
                failures.remove(&anilist_id);
                added = true;
            }
            Err(e) => eprintln!("Affiche {anilist_id} non enregistrée : {e}"),
        }
    }
    added
}

/// Lance le cache des affiches : à chaque changement de la liste, et toutes
/// les 10 minutes pour rattraper un téléchargement raté.
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = match client() {
            Ok(client) => client,
            Err(e) => {
                state.notice(format!("Affiches des animés indisponibles : {e}"));
                return;
            }
        };
        let mut failures = Failures::new();
        loop {
            if sync_covers(&client, &state, &mut failures).await {
                let _ = app.emit("anime-covers", ());
            }
            tokio::select! {
                () = state.animes_wanted.notified() => {}
                () = tokio::time::sleep(RETRY_EVERY) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::temp_dir;

    #[test]
    fn recognises_images() {
        assert!(is_image(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(is_image(b"\x89PNG\r\n\x1a\n...."));
        assert!(is_image(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(is_image(b"GIF89a"));
        assert!(!is_image(b"<html>"));
        assert!(!is_image(b""));
    }

    #[test]
    fn client_builds() {
        assert!(client().is_ok());
    }

    #[test]
    fn cleanup_keeps_only_listed_covers() {
        let dir = temp_dir("covers");
        let state = AppState::load(&dir, "PC").unwrap();
        let covers = covers_dir(&state);
        fs::create_dir_all(&covers).unwrap();
        for name in ["1.img", "1.url", "2.tmp", "a-moi.txt"] {
            fs::write(covers.join(name), b"x").unwrap();
        }
        // Liste vide : rien à télécharger, nos fichiers sont supprimés, pas les autres.
        assert!(plan_covers(&state).unwrap().download.is_empty());
        let left = |dir: &PathBuf| {
            let mut names: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(left(&covers), vec!["a-moi.txt"]);

        // Un animé avec affiche : à télécharger, puis plus rien une fois en
        // cache, et de nouveau si son adresse change (« Mettre à jour »).
        let url = "https://s4.anilist.co/file/a.jpg";
        let info = AnimeInfo {
            anilist_id: Some(7),
            title: "Frieren".to_owned(),
            cover_url: url.to_owned(),
            ..AnimeInfo::default()
        };
        let anime = state
            .animes
            .change("d'ajouter", |d| d.add("Eve", info.clone(), 1))
            .unwrap();
        assert_eq!(
            plan_covers(&state).unwrap().download,
            vec![(7, url.to_owned())]
        );
        save_cover(&state, 7, url, b"\xFF\xD8\xFF").unwrap();
        assert!(plan_covers(&state).unwrap().download.is_empty());
        let new_url = "https://s4.anilist.co/file/b.jpg";
        state
            .animes
            .change("de modifier", |d| {
                d.update_info(
                    &anime.id,
                    AnimeInfo {
                        cover_url: new_url.to_owned(),
                        ..info
                    },
                    2,
                )
            })
            .unwrap();
        assert_eq!(
            plan_covers(&state).unwrap().download,
            vec![(7, new_url.to_owned())]
        );
        assert_eq!(left(&covers), vec!["7.img", "7.url", "a-moi.txt"]);
        fs::remove_dir_all(dir).unwrap();
    }
}
