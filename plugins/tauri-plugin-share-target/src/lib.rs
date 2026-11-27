//! Plugin Tauri du menu Partager d'Android pour Eve & Lucas.
//!
//! L'appli apparaît dans le menu Partager des autres applis (YouTube Music,
//! YouTube…) pour du texte. Le module Kotlin (`android/`) note le texte
//! reçu, même appli fermée, puis ouvre l'appli. Celle-ci lit les partages avec
//! [`ShareTarget::peek_shares`], les ajoute à ses documents, puis les confirme
//! avec [`ShareTarget::ack_shares`] (au démarrage, ou quand l'évènement
//! [`ShareTarget::on_share`] la prévient alors qu'elle tourne déjà). Rien n'est
//! perdu si l'enregistrement échoue.
//!
//! Sur PC, chaque appel est sans effet.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::Share;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

mod error;
mod models;

pub use error::{Error, Result};

#[cfg(not(target_os = "android"))]
pub use desktop::ShareTarget;
#[cfg(target_os = "android")]
pub use mobile::ShareTarget;

/// Donne accès aux partages reçus : `app.share_target()`.
pub trait ShareTargetExt<R: Runtime> {
    fn share_target(&self) -> &ShareTarget<R>;
}

impl<R: Runtime, T: Manager<R>> ShareTargetExt<R> for T {
    fn share_target(&self) -> &ShareTarget<R> {
        self.state::<ShareTarget<R>>().inner()
    }
}

/// Crée le plugin, à enregistrer avec `.plugin(tauri_plugin_share_target::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    // Pas d'`invoke_handler` : aucune commande n'est exposée à l'interface.
    Builder::new("share-target")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let share_target = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let share_target = desktop::init(app, api)?;
            app.manage(share_target);
            Ok(())
        })
        .build()
}
