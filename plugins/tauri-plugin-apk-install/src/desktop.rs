//! Sur PC (et sur iOS, pas pris en charge), pas d'APK : chaque appel renvoie
//! une erreur.

use std::{marker::PhantomData, path::Path};

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{Error, InstallResult, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<ApkInstall<R>> {
    Ok(ApkInstall(PhantomData))
}

/// Installeur d'APK (absent sur PC).
pub struct ApkInstall<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> ApkInstall<R> {
    pub fn install(&self, _path: &Path) -> Result<InstallResult> {
        Err(Error::InvalidArgs(
            "l'installation d'un APK n'existe que sur Android".into(),
        ))
    }
}
