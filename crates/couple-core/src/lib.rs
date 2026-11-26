//! Cœur partagé de l'appli Eve & Lucas.
//!
//! Ce crate ne dépend ni de Tauri ni du réseau : il contient uniquement les
//! données du couple et leurs règles. Il sera utilisé tel quel par l'appli PC,
//! l'appli Android et, plus tard, par la synchro iroh.

pub mod arrivals;
pub mod devices;
pub mod files;
pub mod notes;
pub mod sync;
pub mod tasks;
pub mod widget;

pub use arrivals::{Arrival, ArrivalsDoc};
pub use devices::{Device, DevicesDoc};
pub use files::{FilesDoc, SharedFile};
pub use notes::{Note, NotesDoc};
pub use sync::{SyncState, Syncable};
pub use tasks::{Task, TasksDoc};
pub use widget::{WidgetAction, WidgetData};

/// Erreurs du cœur.
#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    #[error("le fichier {0} n'existe pas")]
    FileNotFound(String),
    #[error("un fichier partagé ne doit pas dépasser {0} Mo")]
    FileTooLarge(u64),
    #[error("fichier invalide : {0}")]
    InvalidFile(String),
    #[error("il manque le prénom")]
    MissingAuthor,
    #[error("appareil invalide : {0}")]
    InvalidDevice(String),
    #[error("message de synchro illisible : {0}")]
    SyncMessage(String),
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
