//! Plugin Tauri d'installation des mises à jour sur Android.
//!
//! L'appli télécharge elle-même l'APK de la nouvelle version (dans
//! `cache/maj/`) et vérifie son empreinte, puis appelle
//! [`ApkInstall::install`] : le module Kotlin ouvre l'installeur d'Android,
//! où il reste à appuyer sur « Mettre à jour ». Android refuse tout APK qui
//! n'est pas signé avec la même clé que l'appli installée.
//!
//! La première fois, Android demande d'autoriser l'appli à installer des
//! applis : le module ouvre ce réglage et [`InstallResult::NeedsPermission`]
//! est renvoyé.
//!
//! Sur PC, chaque appel renvoie une erreur (le plugin updater de Tauri s'en
//! charge).

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

mod error;
mod models;

pub use error::{Error, Result};
pub use models::InstallResult;

#[cfg(not(target_os = "android"))]
pub use desktop::ApkInstall;
#[cfg(target_os = "android")]
pub use mobile::ApkInstall;

/// Donne accès à l'installeur : `app.apk_install()`.
pub trait ApkInstallExt<R: Runtime> {
    fn apk_install(&self) -> &ApkInstall<R>;
}

impl<R: Runtime, T: Manager<R>> ApkInstallExt<R> for T {
    fn apk_install(&self) -> &ApkInstall<R> {
        self.state::<ApkInstall<R>>().inner()
    }
}

/// Crée le plugin, à enregistrer avec `.plugin(tauri_plugin_apk_install::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    // Pas d'`invoke_handler` : aucune commande n'est exposée à l'interface.
    Builder::new("apk-install")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let plugin = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let plugin = desktop::init(app, api)?;
            app.manage(plugin);
            Ok(())
        })
        .build()
}
