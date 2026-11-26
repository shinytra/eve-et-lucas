use serde::{Serialize, Serializer};

/// Résultat des fonctions du plugin.
pub type Result<T> = std::result::Result<T, Error>;

/// Erreurs du plugin, transmises à l'interface sous forme de texte.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Appelé sur PC : il n'y a ni GPS ni géofencing.
    #[error("Le géofencing n'existe que sur téléphone")]
    Unsupported,
    /// Paramètre refusé avant même d'appeler Android.
    #[error("{0}")]
    InvalidArgs(String),
    /// Erreur renvoyée par le code Kotlin (message en français).
    #[cfg(target_os = "android")]
    #[error(transparent)]
    PluginInvoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
