//! Sur PC (et sur iOS, pas encore pris en charge), pas de géofencing :
//! chaque appel renvoie une erreur explicite, sauf la lecture des arrivées
//! et le réveil par ntfy, sans effet.

use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{models::*, Error, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<Geofence<R>> {
    Ok(Geofence(PhantomData))
}

/// Accès au géofencing (inexistant sur PC).
pub struct Geofence<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> Geofence<R> {
    pub fn set_home(&self, lat: f64, lng: f64, radius_m: f32) -> Result<()> {
        HomeArgs::new(lat, lng, radius_m)?;
        Err(Error::Unsupported)
    }

    pub fn clear_home(&self) -> Result<()> {
        Err(Error::Unsupported)
    }

    pub fn current_position(&self) -> Result<Position> {
        Err(Error::Unsupported)
    }

    pub fn set_ring_on_arrival(&self, _ring: bool) -> Result<()> {
        Err(Error::Unsupported)
    }

    /// Aucune arrivée n'est jamais enregistrée sur PC.
    pub fn peek_arrivals(&self) -> Result<Vec<PendingArrival>> {
        Ok(Vec::new())
    }

    pub fn ack_arrivals(&self, _ids: Vec<String>) -> Result<()> {
        Ok(())
    }

    /// Sans effet sur PC : pas de réveil par ntfy (l'appli tourne en tâche de fond).
    pub fn set_push_config(&self, _config: PushConfig) -> Result<()> {
        Ok(())
    }

    /// Pas de distributeur UnifiedPush sur PC.
    pub fn register_push(&self) -> Result<PushStatus> {
        Ok(PushStatus {
            available: false,
            endpoint: None,
        })
    }

    pub fn on_push_endpoint<F: Fn(String) + Send + Sync + 'static>(
        &self,
        _handler: F,
    ) -> Result<()> {
        Ok(())
    }

    pub fn peek_received(&self) -> Result<Vec<ReceivedArrival>> {
        Ok(Vec::new())
    }

    pub fn ack_received(&self, _ids: Vec<String>) -> Result<()> {
        Ok(())
    }

    pub fn on_received<F: Fn(ReceivedArrival) + Send + Sync + 'static>(
        &self,
        _handler: F,
    ) -> Result<()> {
        Ok(())
    }

    pub fn mark_rung(&self, _ids: Vec<String>) -> Result<()> {
        Ok(())
    }

    pub fn check_permissions(&self) -> Result<PermissionStatus> {
        Err(Error::Unsupported)
    }

    pub fn request_permissions(
        &self,
        _permissions: Vec<PermissionType>,
    ) -> Result<PermissionStatus> {
        Err(Error::Unsupported)
    }

    pub fn on_arrival<F: Fn(Arrival) + Send + Sync + 'static>(&self, _handler: F) -> Result<()> {
        Err(Error::Unsupported)
    }
}
