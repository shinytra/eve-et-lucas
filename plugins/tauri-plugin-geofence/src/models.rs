use serde::{Deserialize, Serialize};
use tauri::plugin::PermissionState;

use crate::{Error, Result};

/// Position actuelle du téléphone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub lat: f64,
    pub lng: f64,
    /// Précision estimée, en mètres.
    pub accuracy_m: f32,
}

/// Une arrivée à la maison détectée par Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrival {
    /// Date de l'arrivée, en millisecondes Unix.
    pub at: i64,
}

/// Autorisations Android demandées par le plugin.
///
/// Sur Android 10 et plus, `BackgroundLocation` doit être demandée
/// séparément, après que `Location` a été accordée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionType {
    /// Position précise et approximative (premier plan).
    Location,
    /// Position en arrière-plan (« Toujours autoriser »).
    BackgroundLocation,
    /// Notifications (Android 13+).
    Notifications,
}

/// État de chaque autorisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub location: PermissionState,
    pub background_location: PermissionState,
    pub notifications: PermissionState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HomeArgs {
    pub lat: f64,
    pub lng: f64,
    pub radius_m: f32,
}

impl HomeArgs {
    /// Vérifie les paramètres avant de les confier à Android.
    pub(crate) fn new(lat: f64, lng: f64, radius_m: f32) -> Result<Self> {
        if !lat.is_finite() || !(-90.0..=90.0).contains(&lat) {
            return Err(Error::InvalidArgs(format!(
                "Latitude invalide : {lat} (attendu entre -90 et 90)"
            )));
        }
        if !lng.is_finite() || !(-180.0..=180.0).contains(&lng) {
            return Err(Error::InvalidArgs(format!(
                "Longitude invalide : {lng} (attendu entre -180 et 180)"
            )));
        }
        if !radius_m.is_finite() || radius_m <= 0.0 {
            return Err(Error::InvalidArgs(format!(
                "Rayon invalide : {radius_m} m (doit être positif)"
            )));
        }
        Ok(Self { lat, lng, radius_m })
    }
}

#[derive(Debug, Serialize)]
// Utilisé seulement par l'implémentation Android.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct RingArgs {
    pub ring: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct AckArgs {
    pub up_to: i64,
}

#[derive(Debug, Serialize)]
// Utilisé seulement par l'implémentation Android.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct RequestPermissionsArgs {
    pub permissions: Vec<PermissionType>,
}

#[derive(Debug, Deserialize)]
// Utilisé seulement par l'implémentation Android.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct Arrivals {
    pub arrivals: Vec<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_args_valides() {
        let args = HomeArgs::new(48.85, 2.35, 150.0).unwrap();
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            serde_json::json!({ "lat": 48.85, "lng": 2.35, "radiusM": 150.0 })
        );
    }

    #[test]
    fn home_args_refuse_les_valeurs_absurdes() {
        assert!(HomeArgs::new(91.0, 0.0, 100.0).is_err());
        assert!(HomeArgs::new(0.0, -181.0, 100.0).is_err());
        assert!(HomeArgs::new(f64::NAN, 0.0, 100.0).is_err());
        assert!(HomeArgs::new(0.0, 0.0, 0.0).is_err());
        assert!(HomeArgs::new(0.0, 0.0, f32::INFINITY).is_err());
    }

    #[test]
    fn position_lue_depuis_kotlin() {
        let pos: Position =
            serde_json::from_str(r#"{"lat":1.5,"lng":-2.25,"accuracyM":12.5}"#).unwrap();
        assert_eq!(
            pos,
            Position {
                lat: 1.5,
                lng: -2.25,
                accuracy_m: 12.5
            }
        );
    }

    #[test]
    fn noms_des_autorisations() {
        let args = RequestPermissionsArgs {
            permissions: vec![
                PermissionType::Location,
                PermissionType::BackgroundLocation,
                PermissionType::Notifications,
            ],
        };
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            serde_json::json!({ "permissions": ["location", "backgroundLocation", "notifications"] })
        );
    }

    #[test]
    fn etat_des_autorisations() {
        let status: PermissionStatus = serde_json::from_str(
            r#"{"location":"granted","backgroundLocation":"prompt-with-rationale","notifications":"denied"}"#,
        )
        .unwrap();
        assert_eq!(status.location, PermissionState::Granted);
        assert_eq!(
            status.background_location,
            PermissionState::PromptWithRationale
        );
        assert_eq!(status.notifications, PermissionState::Denied);
    }

    #[test]
    fn arrivees_lues_depuis_kotlin() {
        let a: Arrivals =
            serde_json::from_str(r#"{"arrivals":[1700000000000,1700000100000]}"#).unwrap();
        assert_eq!(a.arrivals, vec![1_700_000_000_000, 1_700_000_100_000]);
        let e: Error = HomeArgs::new(0.0, 0.0, -1.0).unwrap_err();
        assert!(serde_json::to_string(&e)
            .unwrap()
            .contains("Rayon invalide"));
    }
}
