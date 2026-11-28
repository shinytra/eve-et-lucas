//! Mises à jour de l'appli depuis l'appli.
//!
//! Chaque version publiée (workflow « Version à télécharger ») joint un
//! fichier `latest.json` au format du plugin updater de Tauri :
//!
//! ```json
//! {
//!   "version": "0.5.0",
//!   "notes": "…",
//!   "pub_date": "2026-11-27T20:00:00Z",
//!   "platforms": {
//!     "windows-x86_64": { "signature": "…", "url": "https://github.com/…/Eve-et-Lucas-0.5.0-Windows.exe" },
//!     "android-aarch64": { "url": "https://github.com/…/Eve-et-Lucas-0.5.0-Android.apk", "sha256": "…" }
//!   }
//! }
//! ```
//!
//! Sur PC, le plugin updater lit ce fichier et vérifie lui-même la signature
//! de l'installateur. Sur Android (que le plugin ne gère pas), ce module lit
//! l'entrée `android-aarch64` : version plus récente, adresse sur la page des
//! versions du dépôt seulement, empreinte SHA-256 vérifiée après le
//! téléchargement. Android refuse en plus tout APK qui n'est pas signé avec
//! la même clé que l'appli installée.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Dépôt public où sont publiées les versions (celui du code).
pub const RELEASES_REPO: &str = "shinytra/eve-et-lucas";

/// Adresse du fichier de la dernière version.
pub fn manifest_url() -> String {
    format!("https://github.com/{RELEASES_REPO}/releases/latest/download/latest.json")
}

/// Seul début d'adresse accepté pour télécharger un APK.
pub fn download_prefix() -> String {
    format!("https://github.com/{RELEASES_REPO}/releases/download/")
}

/// Clé de plateforme de l'APK dans `latest.json`.
pub const ANDROID_PLATFORM: &str = "android-aarch64";

/// Taille maximale de `latest.json`.
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// Taille maximale d'un APK téléchargé.
pub const MAX_APK_BYTES: u64 = 300 * 1024 * 1024;

/// Longueur maximale des notes de version montrées à l'écran.
pub const MAX_NOTES_CHARS: usize = 2000;

/// Numéro de version `majeur.mineur.correctif` (un `v` devant est accepté).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || -> Result<u32> {
            let part = parts.next().unwrap_or_default();
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::InvalidUpdate(format!(
                    "numéro de version illisible : {text}"
                )));
            }
            part.parse()
                .map_err(|_| Error::InvalidUpdate(format!("numéro de version illisible : {text}")))
        };
        let version = Version(next()?, next()?, next()?);
        if parts.next().is_some() {
            return Err(Error::InvalidUpdate(format!(
                "numéro de version illisible : {text}"
            )));
        }
        Ok(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    platforms: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct AndroidEntry {
    url: String,
    sha256: String,
}

/// Nouvelle version disponible pour le téléphone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AndroidUpdate {
    pub version: String,
    pub notes: String,
    pub url: String,
    /// Empreinte SHA-256 attendue, en hexadécimal minuscule.
    pub sha256: String,
}

/// Raccourcit les notes de version à [`MAX_NOTES_CHARS`] caractères.
pub fn clean_notes(notes: &str) -> String {
    let notes = notes.trim();
    match notes.char_indices().nth(MAX_NOTES_CHARS) {
        Some((end, _)) => format!("{}…", &notes[..end]),
        None => notes.to_owned(),
    }
}

/// Lit `latest.json` et renvoie la mise à jour du téléphone, s'il y en a une
/// plus récente que `current`. Une adresse hors de la page des versions ou
/// une empreinte mal formée est refusée.
pub fn android_update(manifest: &[u8], current: &str) -> Result<Option<AndroidUpdate>> {
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidUpdate("fichier de version trop gros".into()));
    }
    let manifest: Manifest = serde_json::from_slice(manifest)
        .map_err(|e| Error::InvalidUpdate(format!("fichier de version illisible : {e}")))?;
    let latest = Version::parse(&manifest.version)?;
    if latest <= Version::parse(current)? {
        return Ok(None);
    }
    let Some(entry) = manifest.platforms.get(ANDROID_PLATFORM) else {
        return Ok(None);
    };
    let entry: AndroidEntry = serde_json::from_value(entry.clone())
        .map_err(|e| Error::InvalidUpdate(format!("entrée Android illisible : {e}")))?;
    let prefix = download_prefix();
    let rest = entry.url.strip_prefix(&prefix).unwrap_or_default();
    // `v0.5.0/Eve-et-Lucas-0.5.0-Android.apk` : rien d'autre (ni `..`, ni requête).
    let safe = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '/');
    if rest.is_empty() || !rest.chars().all(safe) || rest.contains("..") || rest.contains("//") {
        return Err(Error::InvalidUpdate(format!(
            "adresse de téléchargement refusée : {}",
            entry.url
        )));
    }
    let sha256 = entry.sha256.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidUpdate("empreinte SHA-256 mal formée".into()));
    }
    Ok(Some(AndroidUpdate {
        version: latest.to_string(),
        notes: clean_notes(manifest.notes.as_deref().unwrap_or_default()),
        url: entry.url,
        sha256,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn manifest(version: &str, url: &str, sha: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "notes": "Nouveautés",
            "pub_date": "2026-11-27T20:00:00Z",
            "platforms": {
                "windows-x86_64": { "signature": "x", "url": "https://example.com/a.exe" },
                "android-aarch64": { "url": url, "sha256": sha }
            }
        }))
        .unwrap()
    }

    fn apk(version: &str) -> String {
        format!(
            "{}v{version}/Eve-et-Lucas-{version}-Android.apk",
            download_prefix()
        )
    }

    #[test]
    fn versions_comparees_nombre_par_nombre() {
        assert_eq!(Version::parse("v0.10.0").unwrap(), Version(0, 10, 0));
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("0.99.99").unwrap());
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "1.x.3",
            "1.2.-3",
            "1.2.3-beta",
            " . . ",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Version(1, 2, 3).to_string(), "1.2.3");
    }

    #[test]
    fn nouvelle_version_android_trouvee() {
        let update = android_update(&manifest("0.5.0", &apk("0.5.0"), SHA), "0.4.0")
            .unwrap()
            .unwrap();
        assert_eq!(update.version, "0.5.0");
        assert_eq!(update.notes, "Nouveautés");
        assert_eq!(update.url, apk("0.5.0"));
        assert_eq!(update.sha256, SHA);
        // Empreinte en majuscules acceptée, gardée en minuscules.
        let upper = android_update(
            &manifest("0.5.0", &apk("0.5.0"), &SHA.to_uppercase()),
            "0.4.0",
        )
        .unwrap()
        .unwrap();
        assert_eq!(upper.sha256, SHA);
    }

    #[test]
    fn rien_si_deja_a_jour_ou_sans_apk() {
        assert_eq!(
            android_update(&manifest("0.4.0", &apk("0.4.0"), SHA), "0.4.0").unwrap(),
            None
        );
        assert_eq!(
            android_update(&manifest("0.3.9", &apk("0.3.9"), SHA), "0.4.0").unwrap(),
            None
        );
        let sans_android = br#"{"version":"9.0.0","platforms":{}}"#;
        assert_eq!(android_update(sans_android, "0.4.0").unwrap(), None);
    }

    #[test]
    fn adresse_et_empreinte_verifiees() {
        for url in [
            "https://example.com/v0.5.0/a.apk".to_owned(),
            download_prefix().replace("https://", "http://") + "v0.5.0/a.apk",
            format!("{}../../autre/a.apk", download_prefix()),
            format!("{}v0.5.0/a.apk?x=1", download_prefix()),
            format!("{}v0.5.0//a.apk", download_prefix()),
            download_prefix(),
        ] {
            assert!(
                android_update(&manifest("0.5.0", &url, SHA), "0.4.0").is_err(),
                "{url}"
            );
        }
        for sha in ["", "abc", &"g".repeat(64), &"a".repeat(65)] {
            assert!(android_update(&manifest("0.5.0", &apk("0.5.0"), sha), "0.4.0").is_err());
        }
    }

    #[test]
    fn fichier_illisible_ou_trop_gros_refuse() {
        assert!(android_update(b"pas du json", "0.4.0").is_err());
        assert!(android_update(br#"{"version":"truc"}"#, "0.4.0").is_err());
        let gros = vec![b' '; MAX_MANIFEST_BYTES + 1];
        assert!(android_update(&gros, "0.4.0").is_err());
    }

    #[test]
    fn notes_raccourcies() {
        assert_eq!(clean_notes("  court \n"), "court");
        let long = "é".repeat(MAX_NOTES_CHARS + 10);
        let short = clean_notes(&long);
        assert_eq!(short.chars().count(), MAX_NOTES_CHARS + 1);
        assert!(short.ends_with('…'));
    }
}
