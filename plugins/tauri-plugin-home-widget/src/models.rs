use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Une action faite sur le widget (par exemple cocher une tâche), notée par
/// Android même appli fermée, jusqu'à sa confirmation avec
/// [`crate::HomeWidget::ack_actions`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// Identifiant aléatoire (UUID) de l'action, à renvoyer à `ack_actions`.
    pub id: String,
    /// Nature de l'action : `"done"` (tâche cochée) pour l'instant.
    pub kind: String,
    /// Tâche concernée.
    pub task_id: String,
    /// Date de l'action, en millisecondes Unix.
    pub at: i64,
}

/// Vérifie que le texte à confier au widget est bien du JSON, pour ne pas
/// enregistrer côté Android quelque chose que le widget ne saurait pas lire.
pub(crate) fn check_data_json(data_json: &str) -> Result<()> {
    serde_json::from_str::<serde_json::Value>(data_json)
        .map(|_| ())
        .map_err(|e| Error::InvalidArgs(format!("Données du widget illisibles : {e}")))
}

#[derive(Debug, Serialize)]
// Utilisé seulement par l'implémentation Android.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct UpdateArgs {
    pub data: String,
}

#[derive(Debug, Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct AckArgs {
    pub ids: Vec<String>,
}

/// Réponse de `peekActions` côté Kotlin : chaque action est lue à part, pour
/// qu'une entrée illisible soit ignorée sans faire perdre les autres.
#[derive(Debug, Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct ActionsPayload {
    #[serde(default)]
    pub actions: Vec<serde_json::Value>,
}

impl ActionsPayload {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn into_actions(self) -> Vec<Action> {
        self.actions
            .into_iter()
            .filter_map(|value| match serde_json::from_value::<Action>(value) {
                Ok(action) => Some(action),
                Err(e) => {
                    eprintln!("Action du widget illisible ignorée : {e}");
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_lue_en_camel_case() {
        let a: Action = serde_json::from_str(
            r#"{"id":"0b6c…","kind":"done","taskId":"t-1","at":1700000000000}"#,
        )
        .unwrap();
        assert_eq!(
            a,
            Action {
                id: "0b6c…".into(),
                kind: "done".into(),
                task_id: "t-1".into(),
                at: 1_700_000_000_000,
            }
        );
        // Et réécrite à l'identique (aller-retour).
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::json!({"id":"0b6c…","kind":"done","taskId":"t-1","at":1_700_000_000_000_i64})
        );
    }

    #[test]
    fn action_en_snake_case_refusee() {
        assert!(
            serde_json::from_str::<Action>(r#"{"id":"a","kind":"done","task_id":"t","at":1}"#)
                .is_err()
        );
    }

    #[test]
    fn actions_illisibles_ignorees() {
        let payload: ActionsPayload = serde_json::from_str(
            r#"{"actions":[
                {"id":"a","kind":"done","taskId":"t1","at":1},
                {"id":"b","kind":"done","at":2},
                "n'importe quoi",
                {"id":"c","kind":"done","taskId":"t3","at":3}
            ]}"#,
        )
        .unwrap();
        let ids: Vec<_> = payload.into_actions().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["a", "c"]);
    }

    #[test]
    fn reponse_sans_actions() {
        let payload: ActionsPayload = serde_json::from_str("{}").unwrap();
        assert!(payload.into_actions().is_empty());
    }

    #[test]
    fn arguments_envoyes_a_kotlin() {
        assert_eq!(
            serde_json::to_value(AckArgs {
                ids: vec!["a".into(), "b".into()]
            })
            .unwrap(),
            serde_json::json!({ "ids": ["a", "b"] })
        );
        assert_eq!(
            serde_json::to_value(UpdateArgs { data: "{}".into() }).unwrap(),
            serde_json::json!({ "data": "{}" })
        );
    }

    #[test]
    fn donnees_du_widget_verifiees() {
        let ok = r#"{"tasks":[{"id":"t","title":"Pain","assignee":"Eve","dueAt":null,"late":false}],"openCount":1,"lastArrival":null,"updatedAt":1700000000000}"#;
        assert!(check_data_json(ok).is_ok());
        let e = check_data_json("{pas du json").unwrap_err();
        assert!(serde_json::to_string(&e)
            .unwrap()
            .contains("Données du widget illisibles"));
    }
}
