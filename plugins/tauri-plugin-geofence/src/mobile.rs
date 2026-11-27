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
    /// Ajoute un lieu à la position donnée (le téléphone y est : il compte
    /// comme une entrée, sans faire sonner).
    pub fn add_place(
        &self,
        name: &str,
        kind: &str,
        announce: bool,
        lat: f64,
        lng: f64,
        radius_m: f32,
    ) -> Result<Place> {
        let args = HomeArgs::new(name, kind, announce, lat, lng, radius_m)?;
        self.0
            .run_mobile_plugin("addPlace", args)
            .map_err(Into::into)
    }

    pub fn rename_place(&self, id: String, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        self.0
            .run_mobile_plugin("renamePlace", RenameArgs { id, name })
            .map_err(Into::into)
    }

    pub fn remove_place(&self, id: String) -> Result<()> {
        self.0
            .run_mobile_plugin("removePlace", IdArgs { id })
            .map_err(Into::into)
    }

    pub fn list_places(&self) -> Result<Vec<Place>> {
        self.0
            .run_mobile_plugin::<Places>("listPlaces", ())
            .map(|p| p.places)
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

    /// Arrivées notées par Android et pas encore confirmées avec
    /// [`Geofence::ack_arrivals`].
    pub fn peek_arrivals(&self) -> Result<Vec<PendingArrival>> {
        self.0
            .run_mobile_plugin::<Arrivals>("peekArrivals", ())
            .map(|a| a.arrivals)
            .map_err(Into::into)
    }

    /// Efface ces arrivées, une fois enregistrées dans le document partagé.
    pub fn ack_arrivals(&self, ids: Vec<String>) -> Result<()> {
        self.0
            .run_mobile_plugin("ackArrivals", IdsArgs { ids })
            .map_err(Into::into)
    }

    /// Réglages du réveil par ntfy (clé vide : pas d'envoi).
    pub fn set_push_config(&self, config: PushConfig) -> Result<()> {
        self.0
            .run_mobile_plugin("setPushConfig", config)
            .map_err(Into::into)
    }

    /// S'enregistre auprès du distributeur UnifiedPush (l'appli ntfy), s'il y
    /// en a un. L'adresse arrive ensuite par [`Geofence::on_push_endpoint`].
    pub fn register_push(&self) -> Result<PushStatus> {
        self.0
            .run_mobile_plugin("registerPush", ())
            .map_err(Into::into)
    }

    /// Prévient quand le distributeur donne (ou change) l'adresse ntfy de ce
    /// téléphone.
    pub fn on_push_endpoint<F: Fn(String) + Send + Sync + 'static>(
        &self,
        handler: F,
    ) -> Result<()> {
        self.listen("pushEndpoint", move |e: PushEndpointEvent| {
            handler(e.endpoint)
        })
    }

    /// Arrivées des autres reçues par ntfy, pas encore confirmées avec
    /// [`Geofence::ack_received`].
    pub fn peek_received(&self) -> Result<Vec<ReceivedArrival>> {
        self.0
            .run_mobile_plugin::<Received>("peekReceived", ())
            .map(|r| r.received)
            .map_err(Into::into)
    }

    pub fn ack_received(&self, ids: Vec<String>) -> Result<()> {
        self.0
            .run_mobile_plugin("ackReceived", IdsArgs { ids })
            .map_err(Into::into)
    }

    /// Arrivée d'un autre appareil reçue par ntfy pendant que l'appli tourne.
    pub fn on_received<F: Fn(ReceivedArrival) + Send + Sync + 'static>(
        &self,
        handler: F,
    ) -> Result<()> {
        self.listen("received", handler)
    }

    /// Arrivées que l'appli a déjà fait sonner : Android ne les fera pas
    /// sonner une deuxième fois en les recevant par ntfy.
    pub fn mark_rung(&self, ids: Vec<String>) -> Result<()> {
        self.0
            .run_mobile_plugin("markRung", IdsArgs { ids })
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
        self.listen("arrival", handler)
    }

    /// Abonne `handler` à un évènement du plugin Kotlin (`trigger`).
    fn listen<T, F>(&self, event: &'static str, handler: F) -> Result<()>
    where
        T: DeserializeOwned,
        F: Fn(T) + Send + Sync + 'static,
    {
        #[derive(Serialize)]
        struct RegisterListener {
            event: &'static str,
            handler: Channel,
        }
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(payload) = body {
                match serde_json::from_str::<T>(&payload) {
                    Ok(value) => handler(value),
                    Err(e) => eprintln!("Évènement « {event} » illisible ignoré : {e}"),
                }
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<()>(
                "registerListener",
                RegisterListener {
                    event,
                    handler: channel,
                },
            )
            .map_err(Into::into)
    }
}
