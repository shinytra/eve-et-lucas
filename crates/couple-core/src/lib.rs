//! Cœur partagé de l'appli Eve & Lucas.
//!
//! Ce crate ne dépend ni de Tauri ni du réseau : il contient uniquement les
//! données du couple et leurs règles. Il sera utilisé tel quel par l'appli PC,
//! l'appli Android et, plus tard, par la synchro iroh.

pub mod anilist;
pub mod animes;
pub mod appearance;
pub mod arrivals;
pub mod cadres;
pub mod dates;
pub mod devices;
pub mod files;
pub mod music;
pub mod notes;
pub mod notify;
pub mod palette;
pub mod presence;
pub mod push;
pub mod sync;
pub mod tasks;
pub mod update;
pub mod widget;

pub use animes::{Anime, AnimeInfo, AnimesDoc};
pub use appearance::{Appearance, AppearanceSettings};
pub use arrivals::{Arrival, ArrivalsDoc};
pub use cadres::{Cadre, CadreView, CadresDoc};
pub use dates::{DateInput, DateReminder, DatesDoc, Day, ImportantDate, UpcomingDate};
pub use devices::{Device, DevicesDoc};
pub use files::{FilesDoc, SharedFile};
pub use music::{Music, MusicDoc};
pub use notes::{Note, NotesDoc};
pub use presence::{PresenceDoc, PresenceEvent};
pub use push::{PushDoc, PushMessage};
pub use sync::{SyncState, Syncable};
pub use tasks::{Task, TasksDoc};
pub use widget::{WidgetAction, WidgetData};

/// Erreurs du cœur.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("adresse de réveil invalide (https uniquement, 500 caractères au plus)")]
    InvalidEndpoint,
    #[error("message de réveil illisible ou falsifié")]
    InvalidPush,
    #[error("type de notification inconnu : {0}")]
    UnknownNotification(String),
    #[error("la note {0} n'existe pas")]
    NoteNotFound(String),
    #[error("une note doit avoir un titre ou un contenu")]
    EmptyNote,
    #[error("la tâche {0} n'existe pas")]
    TaskNotFound(String),
    #[error("une tâche doit avoir un titre")]
    EmptyTask,
    #[error("le titre d'une tâche ne doit pas dépasser {0} caractères")]
    TaskTooLong(usize),
    #[error("l'animé {0} n'existe pas")]
    AnimeNotFound(String),
    #[error("un animé doit avoir un titre")]
    EmptyAnime,
    #[error("le titre ne doit pas dépasser {0} caractères")]
    AnimeTitleTooLong(usize),
    #[error("« {0} » est déjà dans votre liste")]
    AnimeAlreadyAdded(String),
    #[error("cet animé n'a que {0} épisodes")]
    EpisodeOutOfRange(u32),
    #[error("statut inconnu : {0}")]
    InvalidAnimeStatus(String),
    #[error("recherche AniList impossible : {0}")]
    AniList(String),
    #[error("la musique {0} n'existe pas")]
    MusicNotFound(String),
    #[error("lien de musique refusé : {0}")]
    InvalidMusicLink(String),
    #[error("{0} est déjà dans la liste (recommandée par {1})")]
    MusicAlreadyAdded(String, String),
    #[error("ce texte ne doit pas dépasser {0} caractères")]
    MusicTextTooLong(usize),
    #[error("le fichier {0} n'existe pas")]
    FileNotFound(String),
    #[error("un fichier partagé ne doit pas dépasser {0} Mo")]
    FileTooLarge(u64),
    #[error("fichier invalide : {0}")]
    InvalidFile(String),
    #[error("cette image n'est plus sur le widget ({0})")]
    CadreNotFound(String),
    #[error("widget photo : {0}")]
    InvalidCadre(String),
    #[error("la date {0} n'existe pas")]
    DateNotFound(String),
    #[error("date invalide : {0}")]
    InvalidDate(String),
    #[error("il manque le prénom")]
    MissingAuthor,
    #[error("appareil invalide : {0}")]
    InvalidDevice(String),
    #[error("message de synchro illisible : {0}")]
    SyncMessage(String),
    #[error("code de thème invalide : {0}")]
    InvalidTheme(String),
    #[error("le préréglage {0} n'existe pas")]
    PresetNotFound(String),
    #[error("le thème {0} n'existe pas")]
    ThemeNotFound(String),
    #[error("un thème doit avoir un nom")]
    EmptyThemeName,
    #[error("{0} thèmes personnels au maximum : supprimes-en un d'abord")]
    TooManyThemes(usize),
    #[error("image de fond refusée : {0}")]
    InvalidWallpaper(String),
    #[error("mise à jour refusée : {0}")]
    InvalidUpdate(String),
    // Les erreurs des bibliothèques sont « boxées » pour garder `Error` petit.
    #[error("document Automerge invalide : {0}")]
    Automerge(Box<automerge::AutomergeError>),
    #[error("lecture du document impossible : {0}")]
    Hydrate(Box<autosurgeon::HydrateError>),
    #[error("écriture du document impossible : {0}")]
    Reconcile(Box<autosurgeon::ReconcileError>),
}

macro_rules! boxed_from {
    ($($source:ty => $variant:ident),* $(,)?) => {
        $(impl From<$source> for Error {
            fn from(e: $source) -> Self {
                Error::$variant(Box::new(e))
            }
        })*
    };
}

boxed_from! {
    automerge::AutomergeError => Automerge,
    autosurgeon::HydrateError => Hydrate,
    autosurgeon::ReconcileError => Reconcile,
}

pub type Result<T> = std::result::Result<T, Error>;
