//! Sur Android, chaque appel est transmis au plugin Kotlin `HomeWidgetPlugin`.
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

const PLUGIN_IDENTIFIER: &str = "fr.eveetlucas.homewidget";

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<HomeWidget<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "HomeWidgetPlugin")?;
    Ok(HomeWidget(handle))
}

/// Accès au widget d'écran d'accueil d'Android.
pub struct HomeWidget<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> HomeWidget<R> {
    /// Enregistre les données affichées (JSON de `couple_core::widget`) et
    /// redessine tous les widgets posés sur l'écran d'accueil.
    pub fn update(&self, data_json: String) -> Result<()> {
        check_data_json(&data_json)?;
        self.0
            .run_mobile_plugin("update", UpdateArgs { data: data_json })
            .map_err(Into::into)
    }

    /// Enregistre les images du widget photo (JSON préparé par l'appli :
    /// chemins des images, intervalle des GIF) et redessine ces widgets.
    pub fn update_photos(&self, data_json: String) -> Result<()> {
        check_data_json(&data_json)?;
        self.0
            .run_mobile_plugin("updatePhotos", UpdateArgs { data: data_json })
            .map_err(Into::into)
    }

    /// Actions faites sur le widget et pas encore confirmées avec
    /// [`HomeWidget::ack_actions`].
    pub fn peek_actions(&self) -> Result<Vec<Action>> {
        self.0
            .run_mobile_plugin::<ActionsPayload>("peekActions", ())
            .map(ActionsPayload::into_actions)
            .map_err(Into::into)
    }

    /// Oublie ces actions, une fois appliquées aux documents.
    pub fn ack_actions(&self, ids: Vec<String>) -> Result<()> {
        self.0
            .run_mobile_plugin("ackActions", AckArgs { ids })
            .map_err(Into::into)
    }

    /// `handler` est appelé à chaque action faite sur le widget pendant que
    /// l'appli tourne (l'action reste aussi lisible avec `peek_actions`).
    pub fn on_action<F: Fn(Action) + Send + Sync + 'static>(&self, handler: F) -> Result<()> {
        #[derive(Serialize)]
        struct RegisterListener {
            event: &'static str,
            handler: Channel,
        }
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(payload) = body {
                match serde_json::from_str::<Action>(&payload) {
                    Ok(action) => handler(action),
                    Err(e) => eprintln!("Action du widget illisible ignorée : {e}"),
                }
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<()>(
                "registerListener",
                RegisterListener {
                    event: "action",
                    handler: channel,
                },
            )
            .map_err(Into::into)
    }
}
