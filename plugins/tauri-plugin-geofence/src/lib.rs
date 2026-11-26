//! Plugin Tauri de géofencing : détecte l'arrivée à la maison sur Android,
//! même quand l'appli est fermée.
//!
//! Le module Kotlin (`android/`) enregistre une géofence « maison » auprès des
//! services Google Play. À chaque entrée (sauf moins de 5 min après la
//! précédente), son récepteur note l'arrivée (identifiant et date) dans les
//! préférences Android, affiche une notification et, si demandé, fait sonner
//! le téléphone. L'appli récupère ensuite ces arrivées avec
//! [`Geofence::peek_arrivals`] puis les confirme avec [`Geofence::ack_arrivals`] (au démarrage, ou quand l'évènement
//! [`Geofence::on_arrival`] la prévient alors qu'elle tourne déjà).
//!
//! Réveil par ntfy (UnifiedPush) : à chaque arrivée, Android chiffre
//! `{"v":1,"kind":"arrival","id","who","device","at"}` en AES-256-GCM avec la
//! clé commune ([`Geofence::set_push_config`]) et l'envoie (`POST` du texte
//! `EL1.<base64(nonce || chiffré || tag)>`) aux adresses ntfy des autres
//! appareils. À la réception, même appli fermée, Android déchiffre, ignore ce
//! qui est illisible, ancien (plus de 15 min), venu de lui-même ou déjà
//! sonné ([`Geofence::mark_rung`]), puis affiche « Eve arrive à la maison »
//! avec le son du canal d'arrivée. L'appli lit ces arrivées avec
//! [`Geofence::peek_received`] / [`Geofence::ack_received`] ou
//! [`Geofence::on_received`]. Son adresse ntfy s'obtient avec
//! [`Geofence::register_push`] puis [`Geofence::on_push_endpoint`].
//!
//! Sur PC, chaque appel renvoie [`Error::Unsupported`] (sauf la lecture des
//! arrivées et le réveil par ntfy, sans effet).

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
