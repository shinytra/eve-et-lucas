//! Plugin Tauri du widget d'écran d'accueil Android d'Eve & Lucas.
//!
//! L'appli confie au widget les données à afficher (tâches à faire, dernière
//! arrivée) avec [`HomeWidget::update`], sous forme de JSON produit par
//! `couple_core::widget`. Le module Kotlin (`android/`) les garde dans ses
//! préférences et dessine le widget, même appli fermée.
//!
//! Cocher une tâche sur le widget note une [`Action`] côté Android et la retire
//! tout de suite de l'affichage. L'appli lit ces actions avec
//! [`HomeWidget::peek_actions`], les applique, puis les confirme avec
//! [`HomeWidget::ack_actions`] (au démarrage, ou quand l'évènement
//! [`HomeWidget::on_action`] la prévient alors qu'elle tourne déjà).
//!
//! Sur PC, chaque appel est sans effet.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::Action;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(not(target_os = "android"))]
pub use desktop::HomeWidget;
#[cfg(target_os = "android")]
pub use mobile::HomeWidget;

/// Donne accès au widget depuis [`tauri::App`], [`tauri::AppHandle`], une
/// fenêtre ou une webview : `app.home_widget()`.
pub trait HomeWidgetExt<R: Runtime> {
    fn home_widget(&self) -> &HomeWidget<R>;
}

impl<R: Runtime, T: Manager<R>> HomeWidgetExt<R> for T {
    fn home_widget(&self) -> &HomeWidget<R> {
        self.state::<HomeWidget<R>>().inner()
    }
}

/// Crée le plugin, à enregistrer avec `.plugin(tauri_plugin_home_widget::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    // Pas d'`invoke_handler` : aucune commande n'est exposée (voir `commands`).
    Builder::new("home-widget")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let home_widget = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let home_widget = desktop::init(app, api)?;
            app.manage(home_widget);
            Ok(())
        })
        .build()
}
