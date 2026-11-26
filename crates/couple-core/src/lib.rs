//! Cœur partagé de l'appli Eve & Lucas.
//!
//! Ce crate ne dépend ni de Tauri ni du réseau : il contient uniquement les
//! données du couple et leurs règles. Il sera utilisé tel quel par l'appli PC,
//! l'appli Android et, plus tard, par la synchro iroh.

pub mod notes;

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
