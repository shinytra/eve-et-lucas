use serde::{Deserialize, Serialize};

/// Ce qu'a fait Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallResult {
    /// L'installeur est ouvert : il reste à appuyer sur « Mettre à jour ».
    Started,
    /// Le réglage « Installer des applis inconnues » est ouvert : l'autoriser
    /// pour Eve & Lucas, puis recommencer.
    NeedsPermission,
}

#[derive(Debug, Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct InstallArgs {
    pub path: String,
}

/// Réponse de `install` côté Kotlin.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct InstallPayload {
    #[serde(default)]
    pub needs_permission: bool,
}

impl InstallPayload {
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn into_result(self) -> InstallResult {
        if self.needs_permission {
            InstallResult::NeedsPermission
        } else {
            InstallResult::Started
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reponse_de_kotlin() {
        let p: InstallPayload = serde_json::from_str(r#"{"needsPermission":true}"#).unwrap();
        assert_eq!(p.into_result(), InstallResult::NeedsPermission);
        let p: InstallPayload = serde_json::from_str("{}").unwrap();
        assert_eq!(p.into_result(), InstallResult::Started);
        assert_eq!(
            serde_json::to_value(InstallArgs { path: "/a".into() }).unwrap(),
            serde_json::json!({ "path": "/a" })
        );
        assert_eq!(
            serde_json::to_value(InstallResult::NeedsPermission).unwrap(),
            "needsPermission"
        );
    }
}
