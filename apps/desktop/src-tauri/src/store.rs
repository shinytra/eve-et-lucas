//! Documents enregistrés sur le disque, et leur branchement sur la synchro.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
};

use couple_core::{ArrivalsDoc, DevicesDoc, FilesDoc, NotesDoc, SyncState, Syncable, TasksDoc};
use couple_sync::{
    iroh::{EndpointId, SecretKey},
    Store,
};
use tauri::{AppHandle, Emitter};
use tokio::sync::Notify;

use crate::now_ms;

/// Un document qui sait se charger et s'enregistrer.
pub trait Persist: Sized + Clone + Default + Syncable {
    fn load(bytes: &[u8]) -> couple_core::Result<Self>;
    fn save(&mut self) -> Vec<u8>;
}

impl Persist for NotesDoc {
    fn load(bytes: &[u8]) -> couple_core::Result<Self> {
        NotesDoc::load(bytes)
    }
    fn save(&mut self) -> Vec<u8> {
        NotesDoc::save(self)
    }
}

impl Persist for TasksDoc {
    fn load(bytes: &[u8]) -> couple_core::Result<Self> {
        TasksDoc::load(bytes)
    }
    fn save(&mut self) -> Vec<u8> {
        TasksDoc::save(self)
    }
}

impl Persist for FilesDoc {
    fn load(bytes: &[u8]) -> couple_core::Result<Self> {
        FilesDoc::load(bytes)
    }
    fn save(&mut self) -> Vec<u8> {
        FilesDoc::save(self)
    }
}

impl Persist for ArrivalsDoc {
    fn load(bytes: &[u8]) -> couple_core::Result<Self> {
        ArrivalsDoc::load(bytes)
    }
    fn save(&mut self) -> Vec<u8> {
        ArrivalsDoc::save(self)
    }
}

impl Persist for DevicesDoc {
    fn load(bytes: &[u8]) -> couple_core::Result<Self> {
        DevicesDoc::load(bytes)
    }
    fn save(&mut self) -> Vec<u8> {
        DevicesDoc::save(self)
    }
}

/// Un document en mémoire et son fichier.
pub struct Persisted<D> {
    doc: Mutex<D>,
    path: PathBuf,
}

impl<D: Persist> Persisted<D> {
    /// Charge le document. Un fichier illisible n'empêche pas l'appli de
    /// démarrer : il est mis de côté (renommé) et un avertissement est ajouté.
    pub fn load(path: PathBuf, warnings: &mut Vec<String>) -> std::io::Result<Self> {
        let doc = match fs::read(&path) {
            Ok(bytes) => match D::load(&bytes) {
                Ok(doc) => doc,
                Err(e) => {
                    let backup = path.with_extension(format!("corrompu-{}", now_ms()));
                    fs::rename(&path, &backup)?;
                    let message = format!(
                        "Le fichier {} était illisible ({e}). Il a été mis de côté dans {} \
                         et l'appli repart d'un document vide.",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        backup.display()
                    );
                    eprintln!("{message}");
                    warnings.push(message);
                    D::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => D::default(),
            Err(e) => return Err(e),
        };
        Ok(Self {
            doc: Mutex::new(doc),
            path,
        })
    }

    pub fn lock(&self) -> MutexGuard<'_, D> {
        // Un panic pendant une commande ne doit pas bloquer toutes les suivantes :
        // le document reste cohérent car il n'est remplacé qu'après une écriture réussie.
        self.doc.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Applique une modification sur une copie, l'enregistre, puis seulement
    /// remplace le document en mémoire. Si l'écriture échoue, rien ne change.
    pub fn change<T>(
        &self,
        action: &str,
        f: impl FnOnce(&mut D) -> couple_core::Result<T>,
    ) -> Result<T, String> {
        let mut doc = self.lock();
        let mut next = doc.clone();
        let out = f(&mut next).map_err(|e| format!("Impossible {action} : {e}"))?;
        write_atomic(&self.path, &next.save()).map_err(|e| {
            format!(
                "Impossible {action} : l'enregistrement dans {} a échoué ({e})",
                self.path.display()
            )
        })?;
        *doc = next;
        Ok(out)
    }
}

/// Écrit dans un fichier temporaire, force l'écriture sur le disque, puis
/// renomme. Même après une coupure de courant, le fichier contient soit
/// l'ancienne, soit la nouvelle version complète.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    // Sous Unix, le renommage n'est durable qu'une fois le dossier synchronisé.
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// Clé iroh de l'appareil : créée au premier lancement, puis toujours la même.
/// C'est l'identité de l'appareil pour les autres : la perdre oblige à
/// recommencer l'appairage.
pub fn load_or_create_key(path: &Path) -> std::io::Result<SecretKey> {
    match fs::read(path) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("clé d'appareil illisible : {}", path.display()),
                )
            })?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let key = SecretKey::generate();
            write_atomic(path, &key.to_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
            Ok(key)
        }
        Err(e) => Err(e),
    }
}

pub const NOTES_DOC: &str = "notes";
pub const TASKS_DOC: &str = "tasks";
pub const FILES_DOC: &str = "files";
pub const ARRIVALS_DOC: &str = "arrivals";
pub const DEVICES_DOC: &str = "devices";
/// Sur téléphone : taille maximale d'une image téléchargée sans le demander.
const AUTO_DOWNLOAD_MAX: u64 = 10 * 1024 * 1024;

/// Tout l'état de l'appli.
pub struct AppState {
    pub notes: Persisted<NotesDoc>,
    pub tasks: Persisted<TasksDoc>,
    pub files: Persisted<FilesDoc>,
    pub arrivals: Persisted<ArrivalsDoc>,
    pub devices: Persisted<DevicesDoc>,
    /// Messages à montrer au démarrage (fichier illisible, etc.).
    pub startup_warnings: Vec<String>,
    /// Problèmes survenus après le démarrage (notifications refusées…),
    /// montrés à l'ouverture de l'interface.
    pub notices: Mutex<Vec<String>>,
    /// Dossier de données de l'appli.
    pub data_dir: PathBuf,
    /// Identifiant iroh de cet appareil.
    pub self_id: EndpointId,
    pub secret: SecretKey,
    /// Réveille la boucle de synchro après une modification locale.
    pub sync_wanted: Notify,
    /// Réveille la boucle des rappels quand les tâches ont changé.
    pub reminders_wanted: Notify,
    /// Réveille la boucle qui fait sonner à l'arrivée de l'autre.
    pub arrivals_wanted: Notify,
    /// Réveille la boucle des téléchargements de fichiers.
    pub downloads_wanted: Notify,
    /// Réveille la mise à jour du widget (téléphone).
    pub widget_wanted: Notify,
    /// Empêche le ménage des contenus pendant qu'un fichier est partagé
    /// (entre le rangement du contenu et son ajout à la liste).
    pub blob_guard: tokio::sync::Mutex<()>,
    /// Pour prévenir l'interface quand la synchro a changé des documents.
    pub app: OnceLock<AppHandle>,
}

impl AppState {
    pub fn load(data_dir: &Path, device_name: &str) -> std::io::Result<Self> {
        fs::create_dir_all(data_dir)?;
        let mut startup_warnings = Vec::new();
        let notes = Persisted::load(data_dir.join("notes.automerge"), &mut startup_warnings)?;
        let tasks = Persisted::load(data_dir.join("tasks.automerge"), &mut startup_warnings)?;
        let files = Persisted::load(data_dir.join("files.automerge"), &mut startup_warnings)?;
        let arrivals = Persisted::load(data_dir.join("arrivals.automerge"), &mut startup_warnings)?;
        let devices = Persisted::load(data_dir.join("devices.automerge"), &mut startup_warnings)?;
        let secret = load_or_create_key(&data_dir.join("device.key"))?;
        let self_id = secret.public();
        let state = Self {
            notes,
            tasks,
            files,
            arrivals,
            devices,
            startup_warnings,
            notices: Mutex::new(Vec::new()),
            data_dir: data_dir.to_owned(),
            self_id,
            secret,
            sync_wanted: Notify::new(),
            reminders_wanted: Notify::new(),
            downloads_wanted: Notify::new(),
            arrivals_wanted: Notify::new(),
            widget_wanted: Notify::new(),
            blob_guard: tokio::sync::Mutex::new(()),
            app: OnceLock::new(),
        };
        // L'appareil s'inscrit lui-même : les autres apprennent ainsi son nom.
        let known = state.devices.lock().contains(&self_id.to_string());
        if !known {
            if let Err(e) = state.devices.change("d'enregistrer cet appareil", |d| {
                d.upsert(&self_id.to_string(), device_name, now_ms())
            }) {
                eprintln!("{e}");
            }
        }
        Ok(state)
    }

    /// Signale un problème à la personne (affiché par l'interface).
    pub fn notice(&self, message: String) {
        eprintln!("{message}");
        self.notices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(message.clone());
        if let Some(app) = self.app.get() {
            let _ = app.emit("notice", message);
        }
    }

    pub fn device_name(&self) -> String {
        self.devices
            .lock()
            .list()
            .into_iter()
            .find(|d| d.id == self.self_id.to_string())
            .map_or_else(|| "Appareil".to_owned(), |d| d.name)
    }

    fn receive_blocking(
        &self,
        doc: &str,
        state: &mut SyncState,
        message: &[u8],
    ) -> Result<bool, String> {
        match doc {
            NOTES_DOC => self.notes.change("de synchroniser les notes", |d| {
                d.receive_sync_message(state, message)
            }),
            TASKS_DOC => self.tasks.change("de synchroniser les tâches", |d| {
                d.receive_sync_message(state, message)
            }),
            FILES_DOC => self.files.change("de synchroniser les fichiers", |d| {
                d.receive_sync_message(state, message)
            }),
            ARRIVALS_DOC => self.arrivals.change("de synchroniser les arrivées", |d| {
                d.receive_sync_message(state, message)
            }),
            DEVICES_DOC => self.devices.change("de synchroniser les appareils", |d| {
                d.receive_sync_message(state, message)
            }),
            _ => Err(format!("document inconnu : {doc}")),
        }
    }

    fn with_doc<T>(&self, doc: &str, f: impl FnOnce(&mut dyn Syncable) -> T) -> Result<T, String> {
        match doc {
            NOTES_DOC => Ok(f(&mut *self.notes.lock())),
            TASKS_DOC => Ok(f(&mut *self.tasks.lock())),
            FILES_DOC => Ok(f(&mut *self.files.lock())),
            ARRIVALS_DOC => Ok(f(&mut *self.arrivals.lock())),
            DEVICES_DOC => Ok(f(&mut *self.devices.lock())),
            _ => Err(format!("document inconnu : {doc}")),
        }
    }
}

impl Store for AppState {
    fn doc_names(&self) -> Vec<String> {
        // Les appareils d'abord : un nouvel appareil est connu avant ses notes.
        vec![
            DEVICES_DOC.into(),
            NOTES_DOC.into(),
            TASKS_DOC.into(),
            FILES_DOC.into(),
            ARRIVALS_DOC.into(),
        ]
    }

    fn generate(&self, doc: &str, state: &mut SyncState) -> Result<Option<Vec<u8>>, String> {
        self.with_doc(doc, |d| d.generate_sync_message(state))
    }

    fn receive(&self, doc: &str, state: &mut SyncState, message: &[u8]) -> Result<bool, String> {
        // L'enregistrement (avec fsync) bloque : on prévient le runtime pour
        // qu'il ne gèle pas les autres tâches pendant ce temps.
        let multi_thread = tokio::runtime::Handle::try_current()
            .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
        if multi_thread {
            tokio::task::block_in_place(|| self.receive_blocking(doc, state, message))
        } else {
            self.receive_blocking(doc, state, message)
        }
    }

    fn peers(&self) -> Vec<EndpointId> {
        self.devices
            .lock()
            .list()
            .into_iter()
            .filter_map(|d| d.id.parse().ok())
            .filter(|id| *id != self.self_id)
            .collect()
    }

    fn add_peer(&self, id: EndpointId, name: &str) -> Result<(), String> {
        self.devices.change("d'ajouter l'appareil", |d| {
            d.upsert(&id.to_string(), name, now_ms())
        })?;
        self.changed(&[DEVICES_DOC.to_owned()]);
        // Les autres appareils apprennent le nouveau venu à la synchro suivante.
        self.sync_wanted.notify_one();
        Ok(())
    }

    fn device_name(&self) -> String {
        AppState::device_name(self)
    }

    fn pairing_used(&self, accepted: bool) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("pairing-used", accepted);
        }
    }

    fn wanted_blobs(&self) -> Vec<String> {
        // Le PC (« boîte aux lettres ») garde tout. Un téléphone ne télécharge
        // d'office que les images raisonnables (aperçus) ; le reste à la demande.
        self.files
            .lock()
            .list()
            .into_iter()
            .filter(|f| cfg!(desktop) || (f.is_image() && f.size <= AUTO_DOWNLOAD_MAX))
            .map(|f| f.hash)
            .collect()
    }

    fn blobs_received(&self, _hashes: &[String]) {
        // Seulement l'interface : pas besoin de relancer les téléchargements.
        if let Some(app) = self.app.get() {
            let _ = app.emit("docs-changed", [FILES_DOC]);
        }
    }

    fn changed(&self, docs: &[String]) {
        if docs.iter().any(|d| d == TASKS_DOC) {
            self.reminders_wanted.notify_one();
            self.widget_wanted.notify_one();
        }
        if docs.iter().any(|d| d == ARRIVALS_DOC) {
            self.arrivals_wanted.notify_one();
            self.widget_wanted.notify_one();
        }
        if docs.iter().any(|d| d == FILES_DOC) {
            self.downloads_wanted.notify_one();
        }
        if let Some(app) = self.app.get() {
            if let Err(e) = app.emit("docs-changed", docs) {
                eprintln!("Impossible de prévenir l'interface : {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier temporaire propre à chaque test.
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "eve-et-lucas-test-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_files_start_empty_and_changes_are_saved() {
        let dir = temp_dir("roundtrip");
        let state = AppState::load(&dir, "PC").unwrap();
        assert!(state.startup_warnings.is_empty());
        assert!(state.notes.lock().list().unwrap().is_empty());
        assert_eq!(state.device_name(), "PC");
        assert!(
            state.peers().is_empty(),
            "l'appareil ne se compte pas comme pair"
        );

        let note = state
            .notes
            .change("d'ajouter", |doc| doc.add("Lucas", "Titre", "", 1))
            .unwrap();

        let reloaded = AppState::load(&dir, "autre nom").unwrap();
        assert_eq!(reloaded.notes.lock().list().unwrap(), vec![note]);
        assert_eq!(
            reloaded.self_id, state.self_id,
            "la clé de l'appareil est gardée"
        );
        assert_eq!(reloaded.device_name(), "PC", "le nom choisi est gardé");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside_with_a_warning() {
        let dir = temp_dir("corrupt");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notes.automerge"), b"pas un document").unwrap();

        let state = AppState::load(&dir, "PC").unwrap();
        assert_eq!(state.startup_warnings.len(), 1);
        assert!(state.notes.lock().list().unwrap().is_empty());
        let backups = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("corrompu")
            })
            .count();
        assert_eq!(backups, 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_write_leaves_memory_unchanged() {
        let dir = temp_dir("failed-write");
        let state = AppState::load(&dir, "PC").unwrap();
        // Rend l'écriture impossible : le dossier de données disparaît.
        fs::remove_dir_all(&dir).unwrap();

        let err = state
            .notes
            .change("d'ajouter la note", |doc| doc.add("Eve", "Titre", "", 1))
            .unwrap_err();
        assert!(err.starts_with("Impossible d'ajouter la note"));
        assert!(state.notes.lock().list().unwrap().is_empty());
    }

    #[test]
    fn core_errors_are_explained_in_french() {
        let dir = temp_dir("errors");
        let state = AppState::load(&dir, "PC").unwrap();
        let err = state
            .notes
            .change("d'ajouter la note", |doc| doc.add("Eve", " ", " ", 1))
            .unwrap_err();
        assert_eq!(
            err,
            "Impossible d'ajouter la note : une note doit avoir un titre ou un contenu"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn received_sync_messages_are_saved_to_disk() {
        let (a, b) = (temp_dir("sync-a"), temp_dir("sync-b"));
        let pc = AppState::load(&a, "PC").unwrap();
        let phone = AppState::load(&b, "Téléphone").unwrap();
        pc.notes
            .change("d'ajouter", |d| d.add("Lucas", "Depuis le PC", "", 1))
            .unwrap();

        pc.tasks
            .change("d'ajouter", |d| d.add("Lucas", "Pain", "", Some(5), 1))
            .unwrap();

        for doc in [NOTES_DOC, TASKS_DOC] {
            let (mut s_pc, mut s_phone) = (SyncState::new(), SyncState::new());
            for _ in 0..10 {
                let m1 = pc.generate(doc, &mut s_pc).unwrap();
                let m2 = phone.generate(doc, &mut s_phone).unwrap();
                if m1.is_none() && m2.is_none() {
                    break;
                }
                if let Some(m) = m1 {
                    phone.receive(doc, &mut s_phone, &m).unwrap();
                }
                if let Some(m) = m2 {
                    pc.receive(doc, &mut s_pc, &m).unwrap();
                }
            }
        }
        let reloaded = AppState::load(&b, "Téléphone").unwrap();
        assert_eq!(reloaded.notes.lock().list().unwrap().len(), 1);
        assert_eq!(reloaded.tasks.lock().list().len(), 1);
        assert!(phone.generate("inconnu", &mut SyncState::new()).is_err());
        fs::remove_dir_all(a).unwrap();
        fs::remove_dir_all(b).unwrap();
    }
}
