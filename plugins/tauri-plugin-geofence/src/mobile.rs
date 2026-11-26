//! Sur Android, chaque appel est transmis au plugin Kotlin `GeofencePlugin`.
//!
//! Ces appels sont bloquants (ils attendent la réponse d'Android) : à faire
//! depuis une commande `async` ou une tâche de fond, jamais depuis le fil
//! principal (`setup`, gestionnaires d'évènements de fenêtre).

use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{models::*, Result};

const PLUGIN_IDENTIFIER: &str = "fr.eveetlucas.geofence";

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<Geofence<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "GeofencePlugin")?;
    Ok(Geofence(handle))
}

/// Accès au géofencing d'Android.
pub struct Geofence<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Geofence<R> {
    pub fn set_home(&self, lat: f64, lng: f64, radius_m: f32) -> Result<()> {
        let args = HomeArgs::new(lat, lng, radius_m)?;
        self.0
            .run_mobile_plugin("setHome", args)
            .map_err(Into::into)
    }

    pub fn clear_home(&self) -> Result<()> {
        self.0
            .run_mobile_plugin("clearHome", ())
            .map_err(Into::into)
    }

    pub fn current_position(&self) -> Result<Position> {
        self.0
            .run_mobile_plugin("currentPosition", ())
            .map_err(Into::into)
    }

    pub fn set_ring_on_arrival(&self, ring: bool) -> Result<()> {
        self.0
            .run_mobile_plugin("setRingOnArrival", RingArgs { ring })
            .map_err(Into::into)
    }

    pub fn take_arrivals(&self) -> Result<Vec<i64>> {
        self.0
            .run_mobile_plugin::<Arrivals>("takeArrivals", ())
            .map(|a| a.arrivals)
            .map_err(Into::into)
    }

    pub fn check_permissions(&self) -> Result<PermissionStatus> {
        self.0
            .run_mobile_plugin("checkPermissions", ())
            .map_err(Into::into)
    }

    pub fn request_permissions(
        &self,
        permissions: Vec<PermissionType>,
    ) -> Result<PermissionStatus> {
        self.0
            .run_mobile_plugin("requestPermissions", RequestPermissionsArgs { permissions })
            .map_err(Into::into)
    }

    pub fn on_arrival<F: Fn(Arrival) + Send + Sync + 'static>(&self, handler: F) -> Result<()> {
        #[derive(Serialize)]
        struct RegisterListener {
            event: &'static str,
            handler: Channel,
        }
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(payload) = body {
                match serde_json::from_str::<Arrival>(&payload) {
                    Ok(arrival) => handler(arrival),
                    Err(e) => eprintln!("Évènement d'arrivée illisible ignoré : {e}"),
                }
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<()>(
                "registerListener",
                RegisterListener {
                    event: "arrival",
                    handler: channel,
                },
            )
            .map_err(Into::into)
    }
}
