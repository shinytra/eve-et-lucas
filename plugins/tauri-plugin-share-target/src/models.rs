use serde::{Deserialize, Serialize};

/// Un texte reçu par le menu Partager d'Android, noté même appli fermée,
/// jusqu'à sa confirmation avec [`crate::ShareTarget::ack_shares`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    /// Identifiant aléatoire (UUID) du partage, à renvoyer à `ack_shares`.
    pub id: String,
    /// Texte partagé (`EXTRA_TEXT`), par exemple un lien YouTube Music.
    pub text: String,
    /// Sujet éventuel (`EXTRA_SUBJECT`), souvent le titre du morceau.
    #[serde(default)]
    pub subject: String,
    /// Date de réception, en millisecondes Unix.
    pub at: i64,
}

#[derive(Debug, Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct AckArgs {
    pub ids: Vec<String>,
}

/// Réponse de `peekShares` côté Kotlin : chaque partage est lu à part, pour
/// qu'une entrée illisible soit ignorée sans faire perdre les autres.
#[derive(Debug, Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct SharesPayload {
    #[serde(default)]
    pub shares: Vec<serde_json::Value>,
}

impl SharesPayload {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn into_shares(self) -> Vec<Share> {
        self.shares
            .into_iter()
            .filter_map(|value| match serde_json::from_value::<Share>(value) {
                Ok(share) => Some(share),
                Err(e) => {
                    eprintln!("Partage illisible ignoré : {e}");
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
    fn partage_lu_en_camel_case() {
        let s: Share = serde_json::from_str(
            r#"{"id":"a","text":"https://music.youtube.com/watch?v=dQw4w9WgXcQ","at":5}"#,
        )
        .unwrap();
        assert_eq!(s.subject, "");
        assert_eq!(s.at, 5);
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"id":"a","text":"https://music.youtube.com/watch?v=dQw4w9WgXcQ","subject":"","at":5})
        );
    }

    #[test]
    fn partages_illisibles_ignores() {
        let payload: SharesPayload = serde_json::from_str(
            r#"{"shares":[
                {"id":"a","text":"x","subject":"Titre","at":1},
                {"id":"b","at":2},
                "n'importe quoi",
                {"id":"c","text":"y","at":3}
            ]}"#,
        )
        .unwrap();
        let ids: Vec<_> = payload.into_shares().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, vec!["a", "c"]);
        let empty: SharesPayload = serde_json::from_str("{}").unwrap();
        assert!(empty.into_shares().is_empty());
    }

    #[test]
    fn arguments_envoyes_a_kotlin() {
        assert_eq!(
            serde_json::to_value(AckArgs {
                ids: vec!["a".into()]
            })
            .unwrap(),
            serde_json::json!({ "ids": ["a"] })
        );
    }
}
