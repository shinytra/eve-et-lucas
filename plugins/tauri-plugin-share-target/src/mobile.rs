//! Sur Android, chaque appel est transmis au plugin Kotlin `ShareTargetPlugin`.
//!
//! Ces appels sont bloquants (ils attendent la réponse d'Android) : à faire
//! depuis une tâche de fond, jamais depuis le fil principal.

use serde::{de::DeserializeOwned, Serialize};
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{models::*, Result};

const PLUGIN_IDENTIFIER: &str = "fr.eveetlucas.sharetarget";

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<ShareTarget<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "ShareTargetPlugin")?;
    Ok(ShareTarget(handle))
}

/// Accès aux textes reçus par le menu Partager d'Android.
pub struct ShareTarget<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> ShareTarget<R> {
    /// Partages reçus et pas encore confirmés avec [`ShareTarget::ack_shares`].
    pub fn peek_shares(&self) -> Result<Vec<Share>> {
        self.0
            .run_mobile_plugin::<SharesPayload>("peekShares", ())
            .map(SharesPayload::into_shares)
            .map_err(Into::into)
    }

    /// Oublie ces partages, une fois traités.
    pub fn ack_shares(&self, ids: Vec<String>) -> Result<()> {
        self.0
            .run_mobile_plugin("ackShares", AckArgs { ids })
            .map_err(Into::into)
    }

    /// `handler` est appelé à chaque partage reçu pendant que l'appli tourne
    /// (le partage reste aussi lisible avec `peek_shares`).
    pub fn on_share<F: Fn(Share) + Send + Sync + 'static>(&self, handler: F) -> Result<()> {
        #[derive(Serialize)]
        struct RegisterListener {
            event: &'static str,
            handler: Channel,
        }
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(payload) = body {
                match serde_json::from_str::<Share>(&payload) {
                    Ok(share) => handler(share),
                    Err(e) => eprintln!("Partage illisible ignoré : {e}"),
                }
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<()>(
                "registerListener",
                RegisterListener {
                    event: "share",
                    handler: channel,
                },
            )
            .map_err(Into::into)
    }
}
