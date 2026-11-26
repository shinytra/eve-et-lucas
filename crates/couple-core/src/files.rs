//! Fichiers partagés : la liste (nom, taille, empreinte…) est un document
//! Automerge synchronisé comme les notes ; le contenu, lui, est transféré
//! à part, d'appareil à appareil, et retrouvé grâce à son empreinte BLAKE3.
//!
//! Même principe de fusion que les notes : table `id -> SharedFile`, avec
//! des identifiants UUID v4. Deux personnes qui partagent le même fichier en
//! même temps obtiennent deux entrées (même contenu, stocké une seule fois).

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use serde::{Deserialize, Serialize};

use crate::{sync::Syncable, Error, Result};

/// Longueur maximale d'un nom de fichier, en caractères.
pub const MAX_NAME: usize = 200;
/// Taille maximale d'un fichier partagé (500 Mo).
pub const MAX_SIZE: u64 = 500 * 1024 * 1024;

/// Un fichier partagé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct SharedFile {
    pub id: String,
    /// Nom affiché et proposé à l'enregistrement (sans dossier).
    pub name: String,
    /// Taille en octets.
    pub size: u64,
    /// Empreinte BLAKE3 du contenu, en hexadécimal (64 caractères).
    pub hash: String,
    /// Type MIME annoncé par l'appareil qui l'a partagé (peut être vide).
    pub mime: String,
    /// Prénom de la personne qui l'a partagé.
    pub author: String,
    pub added_at: i64,
}

impl SharedFile {
    /// Une image que l'interface peut afficher en aperçu.
    pub fn is_image(&self) -> bool {
        matches!(
            self.mime.as_str(),
            "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        )
    }
}

/// Le document des fichiers partagés. `Clone` garde l'identifiant
/// d'appareil : utiliser [`FilesDoc::fork`] pour simuler un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct FilesDoc {
    doc: AutoCommit,
}

impl FilesDoc {
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

    /// Tous les fichiers lisibles, le plus récent en premier.
    pub fn list(&self) -> Vec<SharedFile> {
        let mut files: Vec<SharedFile> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        files.sort_by(|a, b| b.added_at.cmp(&a.added_at).then_with(|| a.id.cmp(&b.id)));
        files
    }

    /// Nombre d'entrées du document qui ne sont pas des fichiers valides.
    pub fn unreadable_count(&self) -> usize {
        self.doc
            .keys(ROOT)
            .filter(|id| self.read(id).is_none())
            .count()
    }

    /// Une entrée venue d'un autre appareil n'est acceptée que si elle
    /// respecte les mêmes règles qu'un ajout local (nom sans dossier,
    /// empreinte valide, taille raisonnable).
    fn read(&self, key: &str) -> Option<SharedFile> {
        hydrate_prop::<_, SharedFile, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|f| {
                f.id == key
                    && clean_name(&f.name).is_ok_and(|n| n == f.name)
                    && is_hash(&f.hash)
                    && f.size <= MAX_SIZE
            })
    }

    pub fn get(&self, id: &str) -> Result<SharedFile> {
        self.read(id)
            .ok_or_else(|| Error::FileNotFound(id.to_owned()))
    }

    /// Ajoute un fichier dont le contenu (`size` octets, empreinte `hash`)
    /// est déjà enregistré sur cet appareil.
    pub fn add(
        &mut self,
        author: &str,
        name: &str,
        size: u64,
        hash: &str,
        mime: &str,
        now_ms: i64,
    ) -> Result<SharedFile> {
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        if size > MAX_SIZE {
            return Err(Error::FileTooLarge(MAX_SIZE / (1024 * 1024)));
        }
        if !is_hash(hash) {
            return Err(Error::InvalidFile("empreinte invalide".to_owned()));
        }
        let file = SharedFile {
            id: uuid::Uuid::new_v4().to_string(),
            name: clean_name(name)?,
            size,
            hash: hash.to_owned(),
            mime: clean_mime(mime),
            author: author.to_owned(),
            added_at: now_ms,
        };
        reconcile_prop(&mut self.doc, ROOT, file.id.as_str(), &file)?;
        Ok(file)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::FileNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }

    /// Empreintes encore utilisées : le contenu des autres peut être effacé.
    pub fn hashes(&self) -> Vec<String> {
        let mut hashes: Vec<String> = self.list().into_iter().map(|f| f.hash).collect();
        hashes.sort();
        hashes.dedup();
        hashes
    }

    pub fn merge(&mut self, other: &mut FilesDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }
}

impl Syncable for FilesDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

/// Garde uniquement le nom du fichier : pas de dossier, pas de caractère de
/// contrôle, pas de nom spécial. Le nom sert à l'enregistrement sur le
/// disque de l'autre appareil, il ne doit donc jamais pouvoir en sortir.
pub fn clean_name(name: &str) -> Result<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').trim_end().to_owned();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        return Err(Error::InvalidFile("nom de fichier vide".to_owned()));
    }
    if cleaned.chars().count() > MAX_NAME {
        return Err(Error::InvalidFile(format!(
            "nom de plus de {MAX_NAME} caractères"
        )));
    }
    Ok(cleaned)
}

/// Nom à utiliser pour enregistrer le fichier sur le disque : celui de
/// [`clean_name`], plus prudent encore. Les noms réservés de Windows (`CON`,
/// `NUL.txt`…) reçoivent un `_` devant, et le nom est raccourci à 200 octets
/// (limite de 255 octets des systèmes de fichiers) en gardant l'extension.
pub fn disk_name(name: &str) -> Result<String> {
    const MAX_BYTES: usize = 200;
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let mut name = clean_name(name)?;
    let first = name.split('.').next().unwrap_or_default().trim_end();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(first)) {
        name.insert(0, '_');
    }
    if name.len() > MAX_BYTES {
        let (stem, ext) = match name.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() && ext.len() <= 16 => {
                (stem.to_owned(), format!(".{ext}"))
            }
            _ => (name.clone(), String::new()),
        };
        let mut cut = MAX_BYTES - ext.len();
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        name = format!("{}{ext}", &stem[..cut]);
    }
    Ok(name)
}

fn clean_mime(mime: &str) -> String {
    let mime = mime.trim();
    let valid = mime.len() <= 100
        && mime
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/+-.".contains(c));
    if valid {
        mime.to_ascii_lowercase()
    } else {
        String::new()
    }
}

fn is_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::tests::sync_pair;

    const HASH: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
    const HASH2: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn add_list_delete() {
        let mut doc = FilesDoc::new();
        let a = doc
            .add("Lucas", "vacances.jpg", 1234, HASH, "image/jpeg", 1)
            .unwrap();
        let b = doc
            .add("Eve", "liste.txt", 10, HASH2, "text/plain", 2)
            .unwrap();
        assert_eq!(doc.list(), vec![b.clone(), a.clone()]);
        assert!(a.is_image());
        assert!(!b.is_image());
        assert_eq!(doc.hashes(), vec![HASH2.to_owned(), HASH.to_owned()]);
        doc.delete(&a.id).unwrap();
        assert_eq!(doc.list(), vec![b]);
        assert!(matches!(doc.get(&a.id), Err(Error::FileNotFound(_))));
        assert!(matches!(doc.delete(&a.id), Err(Error::FileNotFound(_))));
    }

    #[test]
    fn names_cannot_escape_the_folder() {
        assert_eq!(clean_name("../../etc/passwd").unwrap(), "passwd");
        assert_eq!(clean_name("C:\\Users\\x\\photo.png").unwrap(), "photo.png");
        assert_eq!(clean_name("a:b?c.txt").unwrap(), "a_b_c.txt");
        assert_eq!(
            clean_name("nom\u{0}bizarre\n.txt").unwrap(),
            "nom_bizarre_.txt"
        );
        assert_eq!(clean_name("  rapport.pdf.  ").unwrap(), "rapport.pdf");
        for bad in ["", "  ", "..", "dossier/", "...", "/"] {
            assert!(clean_name(bad).is_err(), "{bad:?} devrait être refusé");
        }
        assert!(clean_name(&"a".repeat(MAX_NAME + 1)).is_err());
    }

    #[test]
    fn disk_names_are_safe_everywhere() {
        assert_eq!(disk_name("photo.png").unwrap(), "photo.png");
        assert_eq!(disk_name("NUL.txt").unwrap(), "_NUL.txt");
        assert_eq!(disk_name("con").unwrap(), "_con");
        assert_eq!(disk_name("Com1.tar.gz").unwrap(), "_Com1.tar.gz");
        assert_eq!(disk_name("console.txt").unwrap(), "console.txt");
        let long = format!("{}.jpg", "é".repeat(150));
        let cut = disk_name(&long).unwrap();
        assert!(cut.len() <= 200 && cut.ends_with(".jpg"), "{cut}");
        assert!(disk_name("../").is_err());
    }

    #[test]
    fn rejects_invalid_files() {
        let mut doc = FilesDoc::new();
        assert!(matches!(
            doc.add("", "a.txt", 1, HASH, "", 1),
            Err(Error::MissingAuthor)
        ));
        assert!(matches!(
            doc.add("Eve", "a.txt", MAX_SIZE + 1, HASH, "", 1),
            Err(Error::FileTooLarge(500))
        ));
        assert!(doc
            .add("Eve", "a.txt", 1, "pas-une-empreinte", "", 1)
            .is_err());
        assert!(doc
            .add("Eve", "a.txt", 1, &HASH.to_uppercase(), "", 1)
            .is_err());
        let f = doc
            .add("Eve", "a.txt", 1, HASH, "text/html; x=<script>", 1)
            .unwrap();
        assert_eq!(f.mime, "");
    }

    #[test]
    fn malicious_entry_from_another_device_is_ignored() {
        let mut doc = FilesDoc::new();
        let ok = doc.add("Eve", "ok.txt", 1, HASH, "", 1).unwrap();
        let mut evil = ok.clone();
        evil.id = "evil".to_owned();
        evil.name = "../../.bashrc".to_owned();
        reconcile_prop(&mut doc.doc, ROOT, "evil", &evil).unwrap();
        let mut bad_hash = ok.clone();
        bad_hash.id = "bad".to_owned();
        bad_hash.hash = "zz".to_owned();
        reconcile_prop(&mut doc.doc, ROOT, "bad", &bad_hash).unwrap();
        assert_eq!(doc.list(), vec![ok]);
        assert_eq!(doc.unreadable_count(), 2);
    }

    #[test]
    fn concurrent_shares_are_kept() {
        let mut pc = FilesDoc::new();
        let mut phone = pc.fork();
        pc.add("Lucas", "a.txt", 1, HASH, "", 1).unwrap();
        phone.add("Eve", "a.txt", 1, HASH, "", 1).unwrap();
        pc.merge(&mut phone).unwrap();
        phone.merge(&mut pc).unwrap();
        assert_eq!(pc.list(), phone.list());
        assert_eq!(pc.list().len(), 2);
        // Même contenu : une seule empreinte à garder.
        assert_eq!(pc.hashes(), vec![HASH.to_owned()]);
    }

    #[test]
    fn syncs_with_protocol_and_survives_reload() {
        let mut pc = FilesDoc::new();
        let mut phone = FilesDoc::new();
        pc.add("Lucas", "a.txt", 1, HASH, "text/plain", 1).unwrap();
        phone.add("Eve", "b.png", 2, HASH2, "image/png", 2).unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(), phone.list());
        let reloaded = FilesDoc::load(&pc.save()).unwrap();
        assert_eq!(reloaded.list(), pc.list());
    }
}
