//! Cœur partagé de l'appli Eve & Lucas.
//!
//! Ce crate ne dépend ni de Tauri ni du réseau : il contient uniquement les
//! données du couple et leurs règles. Il sera utilisé tel quel par l'appli PC,
//! l'appli Android et, plus tard, par la synchro iroh.

pub mod appearance;
pub mod notes;

pub use appearance::{Appearance, AppearanceSettings};
pub use notes::{Note, NotesDoc};

/// Erreurs du cœur.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("la note {0} n'existe pas")]
    NoteNotFound(String),
    #[error("une note doit avoir un titre ou un contenu")]
    EmptyNote,
    #[error("une note doit avoir un auteur")]
    MissingAuthor,
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
