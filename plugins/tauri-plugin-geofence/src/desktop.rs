//! Sur PC (et sur iOS, pas encore pris en charge), pas de géofencing :
//! chaque appel renvoie une erreur explicite.

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
    pub fn take_arrivals(&self) -> Result<Vec<i64>> {
        Ok(Vec::new())
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
