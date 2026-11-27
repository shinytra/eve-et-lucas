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

/// Une arrivée de ce téléphone notée par Android, pas encore confirmée avec
/// [`crate::Geofence::ack_arrivals`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingArrival {
    /// Identifiant UUID v4 créé par Android : le même que celui envoyé par
    /// ntfy, pour que les autres appareils ne sonnent pas deux fois.
    pub id: String,
    /// Date de l'arrivée, en millisecondes Unix.
    pub at: i64,
}

/// Arrivée d'un autre appareil, reçue par ntfy et déchiffrée par Android.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedArrival {
    pub id: String,
    /// Qui arrive (« Eve » ou « Lucas »).
    pub who: String,
    /// Identifiant iroh de l'appareil qui a détecté l'arrivée.
    pub device: String,
    /// Date de l'arrivée, en millisecondes Unix.
    pub at: i64,
}

/// Réglages du réveil par ntfy, enregistrés par Android (réécrits à chaque
/// appel de [`crate::Geofence::set_push_config`]).
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushConfig {
    /// Clé commune AES-256-GCM : 32 octets en base64 standard (avec
    /// remplissage). Vide : aucun envoi, et rien n'est déchiffré.
    pub key: String,
    /// Qui porte ce téléphone, annoncé avec chaque arrivée.
    pub who: String,
    /// Identifiant iroh de ce téléphone (les messages qu'il a lui-même
    /// envoyés sont ignorés à la réception).
    pub device: String,
    /// Adresses ntfy des autres appareils (https, 500 caractères au plus ;
    /// les autres sont ignorées).
    pub targets: Vec<String>,
    /// Types de nouveautés à afficher sur ce téléphone (`couple_core::notify`) ;
    /// les autres annonces reçues sont ignorées.
    #[serde(default)]
    pub notify: Vec<String>,
    /// Prénom choisi pour ce téléphone (vide s'il ne l'a pas été) : ses
    /// propres ajouts, faits sur le PC, ne lui sont pas annoncés.
    #[serde(default)]
    pub owner: String,
    /// Identifiants des appareils appairés : une annonce d'un appareil retiré
    /// est ignorée.
    #[serde(default)]
    pub devices: Vec<String>,
}

/// La clé n'apparaît jamais dans les journaux.
impl std::fmt::Debug for PushConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushConfig")
            .field("key", &if self.key.is_empty() { "" } else { "…" })
            .field("who", &self.who)
            .field("device", &self.device)
            .field("targets", &self.targets)
            .field("notify", &self.notify)
            .field("owner", &self.owner)
            .field("devices", &self.devices)
            .finish()
    }
}

/// État de l'enregistrement UnifiedPush.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushStatus {
    /// Un distributeur UnifiedPush (l'appli ntfy) est installé.
    pub available: bool,
    /// Adresse ntfy de ce téléphone, si le distributeur l'a déjà donnée
    /// (sinon elle arrive par [`crate::Geofence::on_push_endpoint`]).
    #[serde(default)]
    pub endpoint: Option<String>,
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

/// Identifiants transmis à `ackArrivals`, `ackReceived` et `markRung`.
#[derive(Debug, Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct IdsArgs {
    pub ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct Received {
    pub received: Vec<ReceivedArrival>,
}

/// Évènement `pushEndpoint`.
#[derive(Debug, Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct PushEndpointEvent {
    pub endpoint: String,
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
    pub arrivals: Vec<PendingArrival>,
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
        let a: Arrivals = serde_json::from_str(
            r#"{"arrivals":[{"id":"00000000-0000-4000-8000-000000000000","at":1700000000000}]}"#,
        )
        .unwrap();
        assert_eq!(
            a.arrivals,
            vec![PendingArrival {
                id: "00000000-0000-4000-8000-000000000000".into(),
                at: 1_700_000_000_000
            }]
        );
        let e: Error = HomeArgs::new(0.0, 0.0, -1.0).unwrap_err();
        assert!(serde_json::to_string(&e)
            .unwrap()
            .contains("Rayon invalide"));
    }

    #[test]
    fn evenement_d_arrivee_avec_identifiant() {
        // Kotlin envoie maintenant aussi l'identifiant : il est ignoré par `Arrival`.
        let a: Arrival =
            serde_json::from_str(r#"{"id":"00000000-0000-4000-8000-000000000000","at":5}"#)
                .unwrap();
        assert_eq!(a, Arrival { at: 5 });
    }

    #[test]
    fn config_d_envoi_vers_kotlin() {
        let config = PushConfig {
            key: "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".into(),
            who: "Eve".into(),
            device: "d".into(),
            targets: vec!["https://ntfy.sh/upAbc?up=1".into()],
            notify: vec!["note".into()],
            owner: "Eve".into(),
            devices: vec!["d".into(), "pc".into()],
        };
        assert_eq!(
            serde_json::to_value(&config).unwrap(),
            serde_json::json!({
                "key": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
                "who": "Eve",
                "device": "d",
                "targets": ["https://ntfy.sh/upAbc?up=1"],
                "notify": ["note"],
                "owner": "Eve",
                "devices": ["d", "pc"],
            })
        );
        // La clé ne fuit pas dans les journaux.
        let debug = format!("{config:?}");
        assert!(!debug.contains("AAEC"), "{debug}");
        assert!(debug.contains("Eve"));
    }

    #[test]
    fn etat_unifiedpush_lu_depuis_kotlin() {
        let sans: PushStatus = serde_json::from_str(r#"{"available":false}"#).unwrap();
        assert_eq!(sans, PushStatus::default());
        let avec: PushStatus =
            serde_json::from_str(r#"{"available":true,"endpoint":"https://ntfy.sh/upX?up=1"}"#)
                .unwrap();
        assert!(avec.available);
        assert_eq!(avec.endpoint.as_deref(), Some("https://ntfy.sh/upX?up=1"));
        let attente: PushStatus =
            serde_json::from_str(r#"{"available":true,"endpoint":null}"#).unwrap();
        assert_eq!(attente.endpoint, None);
    }

    #[test]
    fn arrivees_recues_lues_depuis_kotlin() {
        let r: Received = serde_json::from_str(
            r#"{"received":[{"id":"00000000-0000-4000-8000-000000000000","who":"Lucas","device":"abc","at":1,"autre":true}]}"#,
        )
        .unwrap();
        assert_eq!(
            r.received,
            vec![ReceivedArrival {
                id: "00000000-0000-4000-8000-000000000000".into(),
                who: "Lucas".into(),
                device: "abc".into(),
                at: 1
            }]
        );
        let e: PushEndpointEvent =
            serde_json::from_str(r#"{"endpoint":"https://ntfy.sh/upX?up=1"}"#).unwrap();
        assert_eq!(e.endpoint, "https://ntfy.sh/upX?up=1");
    }

    #[test]
    fn identifiants_vers_kotlin() {
        let args = IdsArgs {
            ids: vec!["a".into(), "b".into()],
        };
        assert_eq!(
            serde_json::to_value(&args).unwrap(),
            serde_json::json!({ "ids": ["a", "b"] })
        );
    }
}
