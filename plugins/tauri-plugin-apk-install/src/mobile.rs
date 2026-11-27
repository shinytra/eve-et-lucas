//! Sur Android, chaque appel est transmis au plugin Kotlin `ApkInstallPlugin`.
//!
//! Ces appels sont bloquants (ils attendent la réponse d'Android) : à faire
//! depuis une tâche de fond, jamais depuis le fil principal.

use std::path::Path;

use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{models::*, Error, Result};

const PLUGIN_IDENTIFIER: &str = "fr.eveetlucas.apkinstall";

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<ApkInstall<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "ApkInstallPlugin")?;
    Ok(ApkInstall(handle))
}

/// Ouvre l'installeur d'Android.
pub struct ApkInstall<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> ApkInstall<R> {
    /// `path` doit être dans `cache/maj/` (seul dossier partagé avec
    /// l'installeur) ; Kotlin le vérifie aussi.
    pub fn install(&self, path: &Path) -> Result<InstallResult> {
        let path = path
            .to_str()
            .ok_or_else(|| Error::InvalidArgs("chemin de l'APK illisible".into()))?
            .to_owned();
        self.0
            .run_mobile_plugin::<InstallPayload>("install", InstallArgs { path })
            .map(InstallPayload::into_result)
            .map_err(Into::into)
    }
}
