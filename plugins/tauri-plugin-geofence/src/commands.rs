//! Commandes appelables depuis l'interface (`plugin:geofence|…`).
//!
//! Toutes `async` : sur Android, l'appel au Kotlin est bloquant et ne doit pas
//! occuper le fil principal. `peek_arrivals`, `ack_arrivals` et le réveil par
//! ntfy (`set_push_config`, `register_push`, `peek_received`, `ack_received`,
//! `mark_rung`) ne sont volontairement pas exposés : c'est le code Rust de
//! l'appli qui traite les arrivées et détient la clé.

use tauri::{command, AppHandle, Runtime, State};

use crate::{Geofence, PermissionStatus, PermissionType, Position, Result};

#[command]
pub(crate) async fn set_home<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
    lat: f64,
    lng: f64,
    radius_m: f32,
) -> Result<()> {
    geofence.set_home(lat, lng, radius_m)
}

#[command]
pub(crate) async fn clear_home<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
) -> Result<()> {
    geofence.clear_home()
}

#[command]
pub(crate) async fn current_position<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
) -> Result<Position> {
    geofence.current_position()
}

#[command]
pub(crate) async fn set_ring_on_arrival<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
    ring: bool,
) -> Result<()> {
    geofence.set_ring_on_arrival(ring)
}

#[command]
pub(crate) async fn check_permissions<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
) -> Result<PermissionStatus> {
    geofence.check_permissions()
}

#[command]
pub(crate) async fn request_permissions<R: Runtime>(
    _app: AppHandle<R>,
    geofence: State<'_, Geofence<R>>,
    permissions: Vec<PermissionType>,
) -> Result<PermissionStatus> {
    geofence.request_permissions(permissions)
}
