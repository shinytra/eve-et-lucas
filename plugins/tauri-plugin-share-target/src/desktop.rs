//! Sur PC (et sur iOS, pas encore pris en charge), pas de menu Partager :
//! chaque appel est sans effet.

use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{models::*, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<ShareTarget<R>> {
    Ok(ShareTarget(PhantomData))
}

/// Accès aux partages reçus (jamais aucun sur PC).
pub struct ShareTarget<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> ShareTarget<R> {
    pub fn peek_shares(&self) -> Result<Vec<Share>> {
        Ok(Vec::new())
    }

    pub fn ack_shares(&self, _ids: Vec<String>) -> Result<()> {
        Ok(())
    }

    /// Le gestionnaire n'est jamais appelé sur PC.
    pub fn on_share<F: Fn(Share) + Send + Sync + 'static>(&self, _handler: F) -> Result<()> {
        Ok(())
    }
}
