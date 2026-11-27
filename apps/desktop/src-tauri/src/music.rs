//! Commandes Tauri des musiques recommandées (liens YouTube Music), et ajout
//! depuis le menu Partager d'Android.
//!
//! La liste est le document `musiques` (synchronisé). Le titre et l'artiste
//! viennent de l'oEmbed de YouTube (sans clé, au mieux : sans réseau, la
//! musique est ajoutée sans titre et peut être renommée). Le lien ouvert est
//! toujours reconstruit par le cœur depuis l'identifiant vérifié du morceau.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use couple_core::{
    music::{oembed_url, parse_link, parse_oembed, Link},
    notify::Event,
    Music,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::{now_ms, store::AppState};

/// Taille maximale de la réponse oEmbed.
const MAX_OEMBED: usize = 64 * 1024;
/// La recherche du titre ne doit pas faire attendre longtemps.
const OEMBED_TIMEOUT: Duration = Duration::from_secs(8);

/// Résultats des partages traités, pour l'interface : gardés jusqu'à ce
/// qu'elle les lise (au démarrage, un partage peut être traité avant que
/// l'interface écoute les évènements).
static SHARE_RESULTS: Mutex<Vec<ShareResult>> = Mutex::new(Vec::new());

/// Ce que l'interface affiche après un partage depuis une autre appli.
#[derive(Debug, Clone, Serialize)]
pub struct ShareResult {
    pub ok: bool,
    pub message: String,
}

/// Une musique et ce qu'il faut pour l'afficher.
#[derive(Debug, Clone, Serialize)]
pub struct MusicView {
    #[serde(flatten)]
    pub music: Music,
    pub thumbnail: String,
}

fn view(music: Music) -> MusicView {
    let thumbnail = Link::from_code(&music.link)
        .map(|l| l.thumbnail())
        .unwrap_or_default();
    MusicView { music, thumbnail }
}

fn changed(state: &AppState) {
    state.sync_wanted.notify_one();
}

/// Titre et artiste d'après YouTube, ou rien si YouTube est injoignable.
async fn fetch_title(link: &Link) -> Option<(String, String)> {
    let request = async {
        let response = crate::animes::client()?
            .get(oembed_url(link))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("réponse {}", response.status()));
        }
        let bytes = crate::animes::read_limited(response, MAX_OEMBED).await?;
        parse_oembed(&bytes).map_err(|e| e.to_string())
    };
    match tokio::time::timeout(OEMBED_TIMEOUT, request).await {
        Ok(Ok(found)) => Some(found),
        Ok(Err(e)) => {
            eprintln!("Titre de la musique introuvable : {e}");
            None
        }
        Err(_) => {
            eprintln!("Titre de la musique introuvable : YouTube trop lent");
            None
        }
    }
}

/// Pourquoi un ajout n'a pas eu lieu.
pub(crate) enum AddError {
    /// Lien refusé, ou morceau déjà dans la liste : réessayer n'y changera rien.
    Refused(String),
    /// Déjà recommandé (le message le dit, avec qui l'a recommandé).
    Duplicate(String),
    /// Enregistrement impossible (disque plein…) : à réessayer plus tard.
    Save(String),
}

impl From<AddError> for String {
    fn from(e: AddError) -> Self {
        match e {
            AddError::Refused(m) | AddError::Duplicate(m) | AddError::Save(m) => m,
        }
    }
}

/// Ajoute une musique à partir d'un texte qui contient son lien. Le titre
/// donné l'emporte sur celui de YouTube ; `fallback_title` ne sert que si
/// YouTube ne répond pas (sujet envoyé par le menu Partager).
async fn add_from_text(
    state: &AppState,
    author: &str,
    text: &str,
    title: &str,
    fallback_title: &str,
    message: &str,
) -> Result<Music, AddError> {
    let refused = |e: couple_core::Error| {
        let message = format!("Impossible d'ajouter la musique : {e}");
        match e {
            couple_core::Error::MusicAlreadyAdded(..) => AddError::Duplicate(message),
            _ => AddError::Refused(message),
        }
    };
    let link = parse_link(text).map_err(refused)?;
    // Déjà là, ou texte refusé : inutile d'interroger YouTube.
    state
        .music
        .lock()
        .check_new(author, &link, title, "", message)
        .map_err(refused)?;
    let (found_title, artist) = fetch_title(&link).await.unwrap_or_default();
    let title = [title, &found_title, fallback_title]
        .into_iter()
        .map(str::trim)
        .find(|t| !t.is_empty())
        .unwrap_or("")
        .chars()
        .take(couple_core::music::MAX_TITLE)
        .collect::<String>();
    let music = state
        .music
        .change("d'ajouter la musique", |doc| {
            doc.add(author, &link, &title, &artist, message, now_ms())
        })
        .map_err(AddError::Save)?;
    changed(state);
    crate::notify::announce(state, |me, at| Event::music_added(&music, me, at));
    Ok(music)
}

#[tauri::command]
pub fn list_musics(state: State<'_, Arc<AppState>>) -> Vec<MusicView> {
    let doc = state.music.lock();
    let unreadable = doc.unreadable_count();
    if unreadable > 0 {
        eprintln!("{unreadable} entrée(s) illisible(s) ignorée(s) dans les musiques");
    }
    doc.list().into_iter().map(view).collect()
}

#[tauri::command]
pub async fn add_music(
    state: State<'_, Arc<AppState>>,
    author: String,
    link: String,
    title: String,
    message: String,
) -> Result<MusicView, String> {
    add_from_text(&state, &author, &link, &title, "", &message)
        .await
        .map(view)
        .map_err(String::from)
}

#[tauri::command]
pub fn update_music(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    artist: String,
    message: String,
) -> Result<MusicView, String> {
    let music = state.music.change("de modifier la musique", |doc| {
        doc.update(&id, &title, &artist, &message, now_ms())
    })?;
    changed(&state);
    Ok(view(music))
}

#[tauri::command]
pub fn set_music_listened(
    state: State<'_, Arc<AppState>>,
    id: String,
    listened: bool,
    by: String,
) -> Result<MusicView, String> {
    let music = state.music.change("de noter la musique", |doc| {
        doc.set_listened(&id, listened, &by, now_ms())
    })?;
    changed(&state);
    Ok(view(music))
}

#[tauri::command]
pub fn delete_music(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state
        .music
        .change("de supprimer la musique", |doc| doc.delete(&id))?;
    changed(&state);
    Ok(())
}

/// Ouvre le morceau dans YouTube Music (l'appli sur téléphone si elle est
/// installée, le navigateur sur PC).
#[tauri::command]
pub fn open_music(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let music = state.music.lock().get(&id).map_err(|e| e.to_string())?;
    let link = Link::from_code(&music.link).ok_or_else(|| "Lien invalide".to_owned())?;
    app.opener()
        .open_url(link.url(), None::<&str>)
        .map_err(|e| format!("Impossible d'ouvrir YouTube Music : {e}"))
}

/// Résultats des derniers partages, une seule fois.
#[tauri::command]
pub fn take_music_shares() -> Vec<ShareResult> {
    std::mem::take(&mut *SHARE_RESULTS.lock().unwrap_or_else(PoisonError::into_inner))
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn share_done(app: &AppHandle, result: ShareResult) {
    SHARE_RESULTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(result);
    let _ = app.emit("music-shared", ());
}

/// Message montré après l'ajout d'une musique partagée.
#[cfg_attr(not(mobile), allow(dead_code))]
fn added_message(music: &Music) -> String {
    match music.title.as_str() {
        "" => "Musique ajoutée : elle attend d'être écoutée.".to_owned(),
        title => format!("« {title} » ajoutée : elle attend d'être écoutée."),
    }
}

/// Lance l'ajout des musiques partagées depuis d'autres applis (téléphone
/// seulement ; sans effet sur PC).
#[cfg_attr(not(mobile), allow(unused_variables))]
pub fn start(app: &AppHandle, state: Arc<AppState>) {
    #[cfg(mobile)]
    phone::start(app, state);
}

/// Retour au premier plan : un partage a pu arriver entre-temps.
#[cfg(mobile)]
pub fn resumed(state: &AppState) {
    state.music_wanted.notify_one();
}

#[cfg(mobile)]
mod phone {
    use std::sync::Arc;

    use tauri::AppHandle;
    use tauri_plugin_share_target::ShareTargetExt;

    use super::{add_from_text, added_message, share_done, AddError, ShareResult};
    use crate::store::AppState;

    pub fn start(app: &AppHandle, state: Arc<AppState>) {
        {
            let state = state.clone();
            if let Err(e) = app
                .share_target()
                .on_share(move |_| state.music_wanted.notify_one())
            {
                eprintln!("Écoute du menu Partager impossible : {e}");
            }
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                ingest(&app, &state).await;
                state.music_wanted.notified().await;
            }
        });
    }

    /// Ajoute les musiques partagées, puis confirme chaque partage à Android.
    /// Un texte sans lien de morceau, ou une musique déjà là, est confirmé
    /// aussi (avec un message) : il ne servirait à rien de le réessayer.
    async fn ingest(app: &AppHandle, state: &AppState) {
        let shares = {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || app.share_target().peek_shares())
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r.map_err(|e| e.to_string()))
        };
        let shares = match shares {
            Ok(list) => list,
            Err(e) => {
                eprintln!("Partages indisponibles : {e}");
                return;
            }
        };
        if shares.is_empty() {
            return;
        }
        // Pas encore de prénom (fenêtre de bienvenue) : les partages attendent,
        // ils seront ajoutés dès qu'il est choisi.
        let who = crate::arrivals::load_settings(state).who;
        if who.trim().is_empty() {
            return;
        }
        let mut done = Vec::new();
        for share in shares {
            let result = add_from_text(state, &who, &share.text, "", &share.subject, "").await;
            let (ok, message) = match result {
                Ok(music) => (true, added_message(&music)),
                // Déjà là (peut-être ce même partage, ajouté juste avant que
                // la confirmation à Android échoue) : rien d'inquiétant.
                Err(AddError::Duplicate(message)) => (
                    true,
                    message.replace("Impossible d'ajouter la musique : ", ""),
                ),
                Err(AddError::Refused(message)) => (false, message),
                // Android le garde : nouvel essai au prochain passage.
                Err(AddError::Save(message)) => {
                    state.notice(message);
                    break;
                }
            };
            share_done(app, ShareResult { ok, message });
            done.push(share.id);
        }
        if !done.is_empty() {
            let app = app.clone();
            let acked =
                tauri::async_runtime::spawn_blocking(move || app.share_target().ack_shares(done))
                    .await;
            if !matches!(acked, Ok(Ok(()))) {
                eprintln!("Impossible de confirmer les partages reçus");
            }
        }
    }
}
