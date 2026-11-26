//! Plugin Tauri de géofencing : détecte l'arrivée à la maison sur Android,
//! même quand l'appli est fermée.
//!
//! Le module Kotlin (`android/`) enregistre une géofence « maison » auprès des
//! services Google Play. À chaque entrée, son récepteur note la date dans les
//! préférences Android, affiche une notification et, si demandé, fait sonner
//! le téléphone. L'appli récupère ensuite ces dates avec
//! [`Geofence::peek_arrivals`] puis les confirme avec [`Geofence::ack_arrivals`] (au démarrage, ou quand l'évènement
//! [`Geofence::on_arrival`] la prévient alors qu'elle tourne déjà).
//!
//! Sur PC, chaque appel renvoie [`Error::Unsupported`] (sauf `peek_arrivals`
//! et `ack_arrivals`, sans effet).

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;
pub use tauri::plugin::PermissionState;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(not(target_os = "android"))]
pub use desktop::Geofence;
#[cfg(target_os = "android")]
pub use mobile::Geofence;

/// Donne accès au géofencing depuis [`tauri::App`], [`tauri::AppHandle`],
/// une fenêtre ou une webview : `app.geofence()`.
pub trait GeofenceExt<R: Runtime> {
    fn geofence(&self) -> &Geofence<R>;
}

impl<R: Runtime, T: Manager<R>> GeofenceExt<R> for T {
    fn geofence(&self) -> &Geofence<R> {
        self.state::<Geofence<R>>().inner()
    }
}

/// Crée le plugin, à enregistrer avec `.plugin(tauri_plugin_geofence::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("geofence")
        .invoke_handler(tauri::generate_handler![
            commands::set_home,
            commands::clear_home,
            commands::current_position,
            commands::set_ring_on_arrival,
            commands::check_permissions,
            commands::request_permissions,
        ])
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let geofence = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let geofence = desktop::init(app, api)?;
            app.manage(geofence);
            Ok(())
        })
        .build()
}
