//! Réveil des appareils par ntfy, avec des messages chiffrés.
//!
//! Quand un téléphone arrive à la maison, il envoie un court message aux
//! autres appareils par ntfy.sh (UnifiedPush sur téléphone, abonnement direct
//! sur PC) pour qu'ils sonnent même si leur appli est fermée. Le serveur ntfy
//! ne voit qu'un message chiffré (AES-256-GCM) : la clé n'est connue que des
//! appareils appairés, elle voyage dans ce document, synchronisé par iroh.
//!
//! Document : `key` (clé en base64) et, pour chaque appareil, l'adresse où le
//! joindre (`target:<id iroh>`).
//!
//! Format d'un message (le même dans le code Kotlin du plugin geofence) :
//! `"EL1." + base64(nonce de 12 octets || chiffré || tag de 16 octets)`, le
//! clair étant le JSON de [`PushMessage`]. Le même canal annonce aussi les
//! nouveautés (note ajoutée, tâche faite…), voir [`crate::notify`].

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use data_encoding::BASE64;
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Arrival, Error, PresenceEvent, Result};

const KEY_PROP: &str = "key";
const TARGET_PREFIX: &str = "target:";
const PREFIX: &str = "EL1.";
/// Longueur maximale d'une adresse ntfy.
pub const MAX_ENDPOINT: usize = 500;

/// L'adresse ntfy d'un appareil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct PushTarget {
    /// Identifiant iroh de l'appareil.
    pub device: String,
    /// Adresse https où lui envoyer un message (ntfy.sh).
    pub endpoint: String,
    pub updated_at: i64,
}

/// Le document des adresses de réveil. `Clone` garde l'identifiant
/// d'appareil Automerge : utiliser [`PushDoc::fork`] pour simuler un autre
/// appareil.
#[derive(Debug, Default, Clone)]
pub struct PushDoc {
    doc: AutoCommit,
}

/// Une adresse acceptable : https, sans espace, pas trop longue.
pub fn valid_endpoint(endpoint: &str) -> bool {
    endpoint.len() <= MAX_ENDPOINT
        && endpoint.starts_with("https://")
        && endpoint.len() > "https://".len()
        && !endpoint
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
}

impl PushDoc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            doc: AutoCommit::load(bytes)?,
        })
    }

    pub fn fork(&mut self) -> Self {
        Self {
            doc: self.doc.fork(),
        }
    }

    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// La clé commune (32 octets), si elle existe et est lisible.
    pub fn key(&self) -> Option<[u8; 32]> {
        let text: String = hydrate_prop(&self.doc, ROOT, KEY_PROP).ok()?;
        BASE64.decode(text.as_bytes()).ok()?.try_into().ok()
    }

    /// La clé en base64 (pour le code Kotlin), si elle existe.
    pub fn key_base64(&self) -> Option<String> {
        self.key().map(|k| BASE64.encode(&k))
    }

    /// Crée la clé si elle manque. Si deux appareils la créent en même temps,
    /// Automerge en garde une seule, la même partout ; les messages chiffrés
    /// avec l'autre sont ignorés le temps que tout le monde se synchronise.
    /// Renvoie `true` si une clé a été créée.
    pub fn ensure_key(&mut self) -> Result<bool> {
        if self.key().is_some() {
            return Ok(false);
        }
        let key = Aes256Gcm::generate_key(OsRng);
        self.doc
            .put(ROOT, KEY_PROP, BASE64.encode(key.as_slice()))?;
        Ok(true)
    }

    /// Remplace la clé par une nouvelle et oublie l'adresse de `removed` :
    /// à faire quand un appareil est retiré. Il garde l'ancienne clé, mais ne
    /// reçoit plus la synchro : il ne peut ni lire les réveils suivants ni en
    /// fabriquer qui soient acceptés.
    pub fn rotate_key(&mut self, removed: &str) -> Result<()> {
        let key = Aes256Gcm::generate_key(OsRng);
        self.doc
            .put(ROOT, KEY_PROP, BASE64.encode(key.as_slice()))?;
        self.remove_target(removed)
    }

    /// Oublie l'adresse d'un appareil (plus joignable par ntfy).
    pub fn remove_target(&mut self, device: &str) -> Result<()> {
        let key = format!("{TARGET_PREFIX}{device}");
        if self.doc.get(ROOT, key.as_str())?.is_some() {
            self.doc.delete(ROOT, key.as_str())?;
        }
        Ok(())
    }

    /// Enregistre (ou remplace) l'adresse d'un appareil.
    pub fn set_target(&mut self, device: &str, endpoint: &str, now_ms: i64) -> Result<()> {
        let endpoint = endpoint.trim();
        if !valid_endpoint(endpoint) {
            return Err(Error::InvalidEndpoint);
        }
        let target = PushTarget {
            device: device.to_owned(),
            endpoint: endpoint.to_owned(),
            updated_at: now_ms,
        };
        reconcile_prop(
            &mut self.doc,
            ROOT,
            format!("{TARGET_PREFIX}{device}").as_str(),
            &target,
        )?;
        Ok(())
    }

    /// L'adresse enregistrée pour un appareil.
    pub fn target(&self, device: &str) -> Option<PushTarget> {
        self.read_target(&format!("{TARGET_PREFIX}{device}"))
    }

    fn read_target(&self, key: &str) -> Option<PushTarget> {
        let device = key.strip_prefix(TARGET_PREFIX)?;
        hydrate_prop::<_, PushTarget, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|t| t.device == device && valid_endpoint(&t.endpoint))
    }

    /// Adresses des autres appareils parmi `allowed` (les appareils
    /// appairés), triées. Une entrée illisible est ignorée.
    pub fn targets_except(&self, self_device: &str, allowed: &[String]) -> Vec<String> {
        let mut list: Vec<String> = self
            .doc
            .keys(ROOT)
            .filter_map(|key| self.read_target(&key))
            .filter(|t| t.device != self_device && allowed.contains(&t.device))
            .map(|t| t.endpoint)
            .collect();
        list.sort();
        list.dedup();
        list
    }

    pub fn merge(&mut self, other: &mut PushDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for PushDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

/// Contenu (en clair) d'un message de réveil.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushMessage {
    pub v: u32,
    pub kind: String,
    pub id: String,
    pub who: String,
    pub device: String,
    pub at: i64,
    /// Nouveautés seulement (voir [`crate::notify`]) : élément concerné,
    /// titre et texte de la notification. Absents d'une arrivée.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// Arrivées et présences (`kind` = `presence`) : nom et type du lieu
    /// (voir [`crate::presence`]). Absents d'une nouveauté.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub place_kind: String,
    /// Présences seulement : entrée dans un lieu (`true`) ou sortie de tous
    /// (`false`). Une arrivée est toujours une entrée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inside: Option<bool>,
}

impl PushMessage {
    pub fn arrival(arrival: &Arrival) -> Self {
        Self {
            v: 1,
            kind: "arrival".to_owned(),
            id: arrival.id.clone(),
            who: arrival.who.clone(),
            device: arrival.device.clone(),
            at: arrival.at,
            item: String::new(),
            title: String::new(),
            body: String::new(),
            place: String::new(),
            place_kind: String::new(),
            inside: None,
        }
    }

    /// Où en est la personne, si le message est une arrivée ou une présence
    /// (version 1). Une arrivée d'avant les lieux nommés : à la maison.
    pub fn presence(&self) -> Option<PresenceEvent> {
        if self.v != 1 || self.who.trim().is_empty() {
            return None;
        }
        let inside = match self.kind.as_str() {
            "arrival" => true,
            "presence" => self.inside?,
            _ => return None,
        };
        Some(PresenceEvent {
            id: self.id.clone(),
            who: self.who.clone(),
            device: self.device.clone(),
            at: self.at,
            inside,
            place: self.place.clone(),
            place_kind: self.place_kind.clone(),
        })
    }

    /// L'arrivée annoncée, si le message en est une (version 1).
    pub fn into_arrival(self) -> Option<Arrival> {
        if self.v != 1 || self.kind != "arrival" || self.who.trim().is_empty() {
            return None;
        }
        Some(Arrival {
            id: self.id,
            who: self.who,
            device: self.device,
            at: self.at,
        })
    }
}

/// Chiffre un message pour ntfy.
pub fn seal(key: &[u8; 32], message: &PushMessage) -> Result<String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let plain = serde_json::to_vec(message).map_err(|_| Error::InvalidPush)?;
    let sealed = cipher
        .encrypt(&nonce, plain.as_slice())
        .map_err(|_| Error::InvalidPush)?;
    let mut bytes = nonce.to_vec();
    bytes.extend_from_slice(&sealed);
    Ok(format!("{PREFIX}{}", BASE64.encode(&bytes)))
}

/// Déchiffre un message reçu par ntfy. Tout ce qui n'est pas un message
/// authentique (mauvaise clé, modifié en route, mal formé) est refusé.
pub fn open(key: &[u8; 32], text: &str) -> Result<PushMessage> {
    let encoded = text.trim().strip_prefix(PREFIX).ok_or(Error::InvalidPush)?;
    let bytes = BASE64
        .decode(encoded.as_bytes())
        .map_err(|_| Error::InvalidPush)?;
    if bytes.len() < 12 + 16 {
        return Err(Error::InvalidPush);
    }
    let (nonce, sealed) = bytes.split_at(12);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let plain = cipher
        .decrypt(Nonce::from_slice(nonce), sealed)
        .map_err(|_| Error::InvalidPush)?;
    serde_json::from_slice(&plain).map_err(|_| Error::InvalidPush)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        std::array::from_fn(|i| i as u8)
    }

    fn message() -> PushMessage {
        PushMessage {
            v: 1,
            kind: "arrival".into(),
            id: "00000000-0000-4000-8000-000000000000".into(),
            who: "Eve".into(),
            device: "d".into(),
            at: 1,
            item: String::new(),
            title: String::new(),
            body: String::new(),
            place: String::new(),
            place_kind: String::new(),
            inside: None,
        }
    }

    #[test]
    fn presence_messages_from_android() {
        // Tel que PushCrypto.kt (ArrivalPayload.encode) l'écrit.
        let exit: PushMessage = serde_json::from_str(
            r#"{"v":1,"kind":"presence","id":"x","who":"Eve","device":"d","at":5,"inside":false,"place":"Université","place_kind":"etudes"}"#,
        )
        .unwrap();
        let p = exit.presence().unwrap();
        assert!(!p.inside);
        assert_eq!(
            (p.place.as_str(), p.place_kind.as_str()),
            ("Université", "etudes")
        );
        // Une présence n'est pas une arrivée : elle ne fait pas sonner.
        assert!(exit.into_arrival().is_none());
        let arrival: PushMessage = serde_json::from_str(
            r#"{"v":1,"kind":"arrival","id":"y","who":"Eve","device":"d","at":5,"place":"Maison 2","place_kind":"maison"}"#,
        )
        .unwrap();
        let p = arrival.presence().unwrap();
        assert!(p.inside);
        assert_eq!(p.place, "Maison 2");
        assert!(arrival.into_arrival().is_some());
        // Ancienne arrivée, sans lieu : à la maison.
        let p = message().presence().unwrap();
        assert!(p.inside && p.place.is_empty());
        // Présence sans « inside », ou nouveauté : rien.
        let mut odd = message();
        odd.kind = "presence".into();
        assert!(odd.presence().is_none());
        odd.kind = "note".into();
        assert!(odd.presence().is_none());
    }

    #[test]
    fn seal_and_open() {
        let sealed = seal(&key(), &message()).unwrap();
        assert!(sealed.starts_with("EL1."));
        assert!(!sealed.contains("Eve"));
        assert_eq!(open(&key(), &sealed).unwrap(), message());
        // Deux envois du même message ne se ressemblent pas (nonce aléatoire).
        assert_ne!(sealed, seal(&key(), &message()).unwrap());
    }

    #[test]
    fn opens_message_sealed_by_android() {
        // Produit par PushCrypto.kt (nonce fixe 00..0b) : même format des deux côtés.
        let from_kotlin = "EL1.AAECAwQFBgcICQoLPCCgOf/U7jnmKPnvk9NaDPGk7kKRF31QGg6BpydLMIIxIJ7Mn/E/qESUT8C8txgIw2FQvWr7k+oPpxopKNPF3sAM5Fbxpk4OPm6IK5nqLMQa7qwUHwA2Ssff+u5U0tj56cJom9ygNzBXVS6M89Nh2TmpHoo=";
        assert_eq!(open(&key(), from_kotlin).unwrap(), message());
    }

    #[test]
    fn open_refuses_forged_messages() {
        let sealed = seal(&key(), &message()).unwrap();
        let mut other = key();
        other[0] ^= 1;
        assert!(open(&other, &sealed).is_err());
        // Un octet modifié en route.
        let mut bytes = BASE64.decode(&sealed.as_bytes()[4..]).unwrap();
        bytes[20] ^= 1;
        let tampered = format!("EL1.{}", BASE64.encode(&bytes));
        assert!(open(&key(), &tampered).is_err());
        for junk in ["", "EL1.", "EL1.AAAA", "bonjour", "EL2.AAAA"] {
            assert!(open(&key(), junk).is_err(), "{junk}");
        }
    }

    #[test]
    fn known_plaintext_format() {
        // Le clair est exactement le JSON attendu par le code Kotlin.
        let json = serde_json::to_string(&message()).unwrap();
        assert_eq!(
            json,
            r#"{"v":1,"kind":"arrival","id":"00000000-0000-4000-8000-000000000000","who":"Eve","device":"d","at":1}"#
        );
        let unknown: PushMessage = serde_json::from_str(
            r#"{"v":1,"kind":"arrival","id":"x","who":"Eve","device":"d","at":1,"plus":true}"#,
        )
        .unwrap();
        assert!(unknown.into_arrival().is_some());
        let mut later = message();
        later.v = 2;
        assert!(later.into_arrival().is_none());
    }

    #[test]
    fn key_is_created_once_and_merges() {
        let mut pc = PushDoc::new();
        assert!(pc.key().is_none());
        assert!(pc.ensure_key().unwrap());
        assert!(!pc.ensure_key().unwrap());
        let phone = pc.fork();
        assert_eq!(phone.key(), pc.key());

        // Deux appareils qui n'ont jamais communiqué créent chacun une clé :
        // après fusion, ils ont la même.
        let mut a = PushDoc::new();
        let mut b = PushDoc::new();
        a.ensure_key().unwrap();
        b.ensure_key().unwrap();
        a.merge(&mut b).unwrap();
        b.merge(&mut a).unwrap();
        assert!(a.key().is_some());
        assert_eq!(a.key(), b.key());
    }

    #[test]
    fn removed_device_loses_the_key() {
        let mut pc = PushDoc::new();
        pc.ensure_key().unwrap();
        pc.set_target("pc", "https://ntfy.sh/abc", 1).unwrap();
        pc.set_target("vole", "https://ntfy.sh/upVOLE", 1).unwrap();
        // Le téléphone volé a une copie de tout.
        let stolen = pc.fork();
        let old_key = stolen.key().unwrap();
        pc.rotate_key("vole").unwrap();
        let new_key = pc.key().unwrap();
        assert_ne!(old_key, new_key);
        assert!(pc.target("vole").is_none());
        assert!(pc.target("pc").is_some());
        // Un message fabriqué avec l'ancienne clé est refusé.
        let forged = seal(&old_key, &message()).unwrap();
        assert!(open(&new_key, &forged).is_err());
        // Les autres appareils apprennent la nouvelle clé par la synchro.
        let mut phone = PushDoc::new();
        phone.merge(&mut pc).unwrap();
        assert_eq!(phone.key(), Some(new_key));
        pc.remove_target("inconnu").unwrap();
    }

    #[test]
    fn targets_merge_and_filter() {
        let mut pc = PushDoc::new();
        let mut phone = pc.fork();
        pc.set_target("pc", "https://ntfy.sh/abc", 1).unwrap();
        phone
            .set_target("tel-eve", "https://ntfy.sh/upXYZ?up=1", 2)
            .unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        let allowed = vec!["pc".to_owned(), "tel-eve".to_owned()];
        assert_eq!(
            phone.targets_except("tel-eve", &allowed),
            vec!["https://ntfy.sh/abc".to_owned()]
        );
        assert_eq!(
            pc.targets_except("pc", &allowed),
            vec!["https://ntfy.sh/upXYZ?up=1".to_owned()]
        );
        // Appareil retiré : plus réveillé.
        assert!(pc.targets_except("pc", &["pc".to_owned()]).is_empty());
        // Adresses refusées.
        for bad in ["http://ntfy.sh/a", "https://", "https://a b", "ftp://x"] {
            assert!(pc.set_target("pc", bad, 3).is_err(), "{bad}");
        }
        assert!(pc
            .set_target("pc", &format!("https://{}", "a".repeat(600)), 3)
            .is_err());
        assert_eq!(pc.target("pc").unwrap().endpoint, "https://ntfy.sh/abc");
    }
}
