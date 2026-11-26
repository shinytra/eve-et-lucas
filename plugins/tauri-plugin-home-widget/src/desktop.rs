//! Sur PC (et sur iOS, pas encore pris en charge), pas de widget d'écran
//! d'accueil : chaque appel est sans effet.

use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::{models::*, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<HomeWidget<R>> {
    Ok(HomeWidget(PhantomData))
}

/// Accès au widget d'écran d'accueil (inexistant sur PC).
pub struct HomeWidget<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> HomeWidget<R> {
    /// Vérifie seulement que les données sont du JSON, comme sur Android.
    pub fn update(&self, data_json: String) -> Result<()> {
        check_data_json(&data_json)
    }

    /// Aucune action n'est jamais faite sur PC.
    pub fn peek_actions(&self) -> Result<Vec<Action>> {
        Ok(Vec::new())
    }

    pub fn ack_actions(&self, _ids: Vec<String>) -> Result<()> {
        Ok(())
    }

    /// Le gestionnaire n'est jamais appelé sur PC.
    pub fn on_action<F: Fn(Action) + Send + Sync + 'static>(&self, _handler: F) -> Result<()> {
        Ok(())
    }
}
