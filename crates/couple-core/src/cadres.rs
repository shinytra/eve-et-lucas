//! Widget photo : les images et GIF choisis par Eve ou Lucas pour le widget
//! d'écran d'accueil « Photo » du téléphone.
//!
//! Le document `cadres` est partagé : une image mise sur le widget par l'un
//! apparaît aussi sur le widget de l'autre. Même principe de fusion que les
//! notes : table `id -> Cadre`, identifiants UUID v4. Une image n'est qu'une
//! référence vers un fichier partagé (document `files`) : son contenu voyage
//! déjà par iroh-blobs et un téléphone télécharge d'office les images de
//! [`MAX_IMAGE_SIZE`] au plus.
//!
//! Le widget Android (RemoteViews) ne sait pas lire un GIF : [`prepare`]
//! découpe l'image en quelques images fixes réduites (PNG), que le widget
//! fait défiler à intervalle régulier. Le nombre d'images et leur taille sont
//! bornés (mémoire permise par Android pour un widget, batterie).

use std::io::Cursor;

use automerge::{transaction::Transactable, AutoCommit, ReadDoc, ROOT};
use autosurgeon::{hydrate_prop, reconcile_prop, Hydrate, Reconcile};
use image::{
    codecs::{gif::GifDecoder, webp::WebPDecoder},
    imageops::FilterType,
    AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, RgbaImage,
};
use serde::{Deserialize, Serialize};

use crate::{files::SharedFile, sync::Syncable, Error, Result};

/// Nombre maximal d'images sur le widget (on passe de l'une à l'autre en
/// touchant le widget).
pub const MAX_CADRES: usize = 12;
/// Taille maximale du fichier : celle que les téléphones téléchargent d'office.
pub const MAX_IMAGE_SIZE: u64 = 10 * 1024 * 1024;
/// L'image remplit tout le widget (les bords sont rognés).
pub const FIT_FILL: &str = "remplir";
/// L'image est montrée en entier (bandes vides si les proportions diffèrent).
pub const FIT_WHOLE: &str = "entiere";

/// Mémoire totale des images préparées (RGBA, 4 octets par pixel). Android
/// refuse un widget dont les images dépassent 1,5 fois la taille de l'écran
/// (6,9 Mo pour un écran 720 × 1600) : on reste en dessous partout.
pub const FRAME_BUDGET: u64 = 5 * 1024 * 1024;
/// Plus grand côté d'une image fixe préparée, en pixels.
pub const MAX_STILL_SIDE: u32 = 1024;
/// Plus grand côté d'une image d'animation, en pixels.
pub const MAX_ANIMATED_SIDE: u32 = 640;
/// Nombre maximal d'images d'une animation sur le widget.
pub const MAX_FRAMES: usize = 40;
/// Intervalle minimal entre deux images : 10 images/s au plus, comme les
/// animations sans fin de l'appli (batterie).
pub const MIN_FRAME_MS: u32 = 100;
/// Au-delà, les images suivantes du GIF sont ignorées (temps de calcul).
const MAX_SOURCE_FRAMES: usize = 1000;
/// Dimensions maximales de l'image d'origine (photo de 100 Mpx au plus).
const MAX_SOURCE_SIDE: u32 = 12_000;
/// Mémoire maximale demandée au décodeur.
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;

/// Une image mise sur le widget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hydrate, Reconcile)]
pub struct Cadre {
    pub id: String,
    /// Fichier partagé montré (document `files`).
    pub file_id: String,
    /// [`FIT_FILL`] ou [`FIT_WHOLE`].
    pub fit: String,
    /// Qui l'a mise sur le widget.
    pub added_by: String,
    pub added_at: i64,
}

/// Une image du widget avec son fichier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CadreView {
    #[serde(flatten)]
    pub cadre: Cadre,
    pub file: SharedFile,
}

fn valid_fit(fit: &str) -> bool {
    fit == FIT_FILL || fit == FIT_WHOLE
}

/// Un fichier que le widget sait montrer.
pub fn is_usable(file: &SharedFile) -> bool {
    file.is_image() && file.size <= MAX_IMAGE_SIZE
}

/// Le document des images du widget. `Clone` garde l'identifiant
/// d'appareil : utiliser [`CadresDoc::fork`] pour simuler un autre appareil.
#[derive(Debug, Default, Clone)]
pub struct CadresDoc {
    doc: AutoCommit,
}

impl CadresDoc {
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

    pub fn merge(&mut self, other: &mut CadresDoc) -> Result<()> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    /// Une entrée venue d'un autre appareil n'est gardée que si elle respecte
    /// les mêmes règles qu'une entrée créée ici.
    fn read(&self, key: &str) -> Option<Cadre> {
        hydrate_prop::<_, Cadre, _, _>(&self.doc, ROOT, key)
            .ok()
            .filter(|c| {
                c.id == key
                    && valid_fit(&c.fit)
                    && !c.file_id.is_empty()
                    && !c.added_by.trim().is_empty()
            })
    }

    /// Toutes les entrées lisibles, de la plus ancienne à la plus récente.
    fn all(&self) -> Vec<Cadre> {
        let mut all: Vec<Cadre> = self
            .doc
            .keys(ROOT)
            .filter_map(|id| self.read(&id))
            .collect();
        all.sort_by(|a, b| a.added_at.cmp(&b.added_at).then_with(|| a.id.cmp(&b.id)));
        all
    }

    /// Images du widget dont le fichier existe encore, la plus récente en
    /// premier. Une même image mise sur le widget par deux appareils avant
    /// leur synchro n'apparaît qu'une fois (la plus ancienne entrée, la même
    /// partout), et jamais plus de [`MAX_CADRES`].
    pub fn list(&self, files: &[SharedFile]) -> Vec<CadreView> {
        let mut seen = std::collections::HashSet::new();
        let mut list: Vec<CadreView> = self
            .all()
            .into_iter()
            .filter_map(|cadre| {
                let file = files.iter().find(|f| f.id == cadre.file_id)?;
                (is_usable(file) && seen.insert(cadre.file_id.clone())).then(|| CadreView {
                    cadre,
                    file: file.clone(),
                })
            })
            .take(MAX_CADRES)
            .collect();
        list.reverse();
        list
    }

    /// Met une image partagée sur le widget.
    pub fn add(
        &mut self,
        author: &str,
        file: &SharedFile,
        files: &[SharedFile],
        now_ms: i64,
    ) -> Result<Cadre> {
        let author = author.trim();
        if author.is_empty() {
            return Err(Error::MissingAuthor);
        }
        if !file.is_image() {
            return Err(Error::InvalidCadre(
                "seules les images (PNG, JPEG, GIF, WebP) vont sur le widget".to_owned(),
            ));
        }
        if file.size > MAX_IMAGE_SIZE {
            return Err(Error::InvalidCadre(format!(
                "l'image ne doit pas dépasser {} Mo",
                MAX_IMAGE_SIZE / (1024 * 1024)
            )));
        }
        let list = self.list(files);
        if list.iter().any(|v| v.cadre.file_id == file.id) {
            return Err(Error::InvalidCadre(format!(
                "« {} » est déjà sur le widget",
                file.name
            )));
        }
        if list.len() >= MAX_CADRES {
            return Err(Error::InvalidCadre(format!(
                "{MAX_CADRES} images au plus : retires-en une d'abord"
            )));
        }
        let cadre = Cadre {
            id: uuid::Uuid::new_v4().to_string(),
            file_id: file.id.clone(),
            fit: FIT_FILL.to_owned(),
            added_by: author.to_owned(),
            added_at: now_ms,
        };
        reconcile_prop(&mut self.doc, ROOT, cadre.id.as_str(), &cadre)?;
        Ok(cadre)
    }

    /// Change la façon de montrer l'image ([`FIT_FILL`] ou [`FIT_WHOLE`]).
    pub fn set_fit(&mut self, id: &str, fit: &str) -> Result<Cadre> {
        if !valid_fit(fit) {
            return Err(Error::InvalidCadre(format!("affichage inconnu : {fit}")));
        }
        let mut cadre = self
            .read(id)
            .ok_or_else(|| Error::CadreNotFound(id.to_owned()))?;
        if cadre.fit != fit {
            cadre.fit = fit.to_owned();
            reconcile_prop(&mut self.doc, ROOT, id, &cadre)?;
        }
        Ok(cadre)
    }

    /// Retire l'image du widget (le fichier partagé reste).
    pub fn remove(&mut self, id: &str) -> Result<()> {
        if self.doc.get(ROOT, id)?.is_none() {
            return Err(Error::CadreNotFound(id.to_owned()));
        }
        self.doc.delete(ROOT, id)?;
        Ok(())
    }
}

impl Syncable for CadresDoc {
    fn automerge(&mut self) -> &mut AutoCommit {
        &mut self.doc
    }
}

/// Image préparée pour le widget : une seule image fixe, ou les images d'une
/// animation à faire défiler toutes les `frame_ms` millisecondes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub width: u32,
    pub height: u32,
    /// Intervalle entre deux images (0 pour une image fixe).
    pub frame_ms: u32,
    /// Chaque image différente, encodée en PNG.
    pub frames: Vec<Vec<u8>>,
    /// Ordre d'affichage (indices dans `frames`) : une image qui dure plus
    /// longtemps que les autres y revient plusieurs fois, sans prendre plus
    /// de mémoire.
    pub sequence: Vec<usize>,
}

fn invalid(e: impl std::fmt::Display) -> Error {
    Error::InvalidCadre(format!("image illisible ({e})"))
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

/// Prépare une image (PNG, JPEG, GIF ou WebP, animés compris) pour le widget.
/// Les photos sont redressées (orientation EXIF des téléphones).
pub fn prepare(bytes: &[u8]) -> Result<Prepared> {
    if bytes.len() as u64 > MAX_IMAGE_SIZE {
        return Err(Error::InvalidCadre(format!(
            "l'image ne doit pas dépasser {} Mo",
            MAX_IMAGE_SIZE / (1024 * 1024)
        )));
    }
    let format = image::guess_format(bytes).map_err(invalid)?;
    match format {
        ImageFormat::Gif => prepare_animation(|| {
            let mut d = GifDecoder::new(Cursor::new(bytes)).map_err(invalid)?;
            d.set_limits(limits()).map_err(invalid)?;
            Ok(d.into_frames())
        }),
        ImageFormat::WebP => {
            let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(invalid)?;
            if decoder.has_animation() {
                prepare_animation(|| {
                    let mut d = WebPDecoder::new(Cursor::new(bytes)).map_err(invalid)?;
                    d.set_limits(limits()).map_err(invalid)?;
                    Ok(d.into_frames())
                })
            } else {
                prepare_still(bytes)
            }
        }
        ImageFormat::Png | ImageFormat::Jpeg => prepare_still(bytes),
        other => Err(Error::InvalidCadre(format!(
            "format {other:?} non pris en charge (PNG, JPEG, GIF ou WebP)"
        ))),
    }
}

fn prepare_still(bytes: &[u8]) -> Result<Prepared> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(invalid)?;
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(invalid)?;
    let orientation = decoder.orientation().map_err(invalid)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(invalid)?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    let (w, h) = target_size(rgba.width(), rgba.height(), 1, MAX_STILL_SIDE);
    let frame = resize(&rgba, w, h);
    Ok(Prepared {
        width: w,
        height: h,
        frame_ms: 0,
        frames: vec![encode_png(&frame)?],
        sequence: vec![0],
    })
}

/// Délai d'une image d'animation : comme les navigateurs, un délai de 10 ms
/// ou moins compte pour 100 ms.
fn delay_ms(frame: &image::Frame) -> u64 {
    let (num, den) = frame.delay().numer_denom_ms();
    let ms = if den == 0 {
        0
    } else {
        u64::from(num) / u64::from(den)
    };
    if ms <= 10 {
        100
    } else {
        ms
    }
}

/// Deux passages sur l'animation : le premier compte les images et leur
/// durée (une seule image en mémoire à la fois), le second garde celles à
/// montrer, déjà réduites.
fn prepare_animation<'a, F>(frames: F) -> Result<Prepared>
where
    F: Fn() -> Result<image::Frames<'a>>,
{
    let mut delays = Vec::new();
    let (mut width, mut height) = (0, 0);
    for frame in frames()?.take(MAX_SOURCE_FRAMES) {
        let frame = frame.map_err(invalid)?;
        width = width.max(frame.buffer().width());
        height = height.max(frame.buffer().height());
        delays.push(delay_ms(&frame));
    }
    if delays.is_empty() || width == 0 || height == 0 {
        return Err(Error::InvalidCadre("animation vide".to_owned()));
    }
    let (picks, frame_ms) = pick_frames(&delays);
    let mut unique = picks.clone();
    unique.sort_unstable();
    unique.dedup();
    let still = unique.len() == 1;
    let (w, h) = if still {
        target_size(width, height, 1, MAX_STILL_SIDE)
    } else {
        target_size(width, height, unique.len() as u64, MAX_ANIMATED_SIDE)
    };
    let mut out = Vec::with_capacity(unique.len());
    let mut next = unique.iter().copied().peekable();
    for (i, frame) in frames()?.take(MAX_SOURCE_FRAMES).enumerate() {
        let Some(&wanted) = next.peek() else { break };
        let frame = frame.map_err(invalid)?;
        if i == wanted {
            next.next();
            out.push(encode_png(&resize(&frame.into_buffer(), w, h))?);
        }
    }
    if out.len() != unique.len() {
        return Err(Error::InvalidCadre("animation incomplète".to_owned()));
    }
    let sequence = if still {
        vec![0]
    } else {
        picks
            .iter()
            .filter_map(|p| unique.binary_search(p).ok())
            .collect()
    };
    Ok(Prepared {
        width: w,
        height: h,
        frame_ms: if still { 0 } else { frame_ms },
        frames: out,
        sequence,
    })
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Images à montrer, dans l'ordre, à intervalle régulier (renvoyé en
/// millisecondes) : au plus [`MAX_FRAMES`] et pas plus d'une toutes les
/// [`MIN_FRAME_MS`]. Une image affichée deux fois plus longtemps que les
/// autres revient deux fois de suite. Toujours au moins la première.
pub fn pick_frames(delays_ms: &[u64]) -> (Vec<usize>, u32) {
    let total: u64 = delays_ms.iter().sum();
    if delays_ms.is_empty() || total == 0 {
        return (vec![0], 0);
    }
    // Pas commun à tous les délais (100 ms au moins) : rien n'est doublé
    // pour rien quand toutes les images durent 200 ms.
    let step = delays_ms
        .iter()
        .fold(0, |g, &d| gcd(g, d))
        .max(u64::from(MIN_FRAME_MS));
    let count = (total / step).clamp(1, MAX_FRAMES as u64);
    // Image affichée à l'instant `t` : la dernière qui commence avant `t`.
    let mut starts = Vec::with_capacity(delays_ms.len());
    let mut at = 0;
    for d in delays_ms {
        starts.push(at);
        at += d;
    }
    let picks = (0..count)
        .map(|j| {
            let t = j * total / count;
            starts.partition_point(|&s| s <= t).saturating_sub(1)
        })
        .collect();
    let frame_ms = u32::try_from(total / count).unwrap_or(u32::MAX);
    (picks, frame_ms.max(MIN_FRAME_MS))
}

/// Taille de chaque image préparée : proportions gardées, jamais agrandie,
/// `max_side` au plus et `frames` images dans [`FRAME_BUDGET`].
pub fn target_size(width: u32, height: u32, frames: u64, max_side: u32) -> (u32, u32) {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let by_side = f64::from(max_side) / w.max(h);
    let by_budget = (FRAME_BUDGET as f64 / (4.0 * frames.max(1) as f64 * w * h)).sqrt();
    let scale = by_side.min(by_budget).min(1.0);
    // Arrondi vers le bas pour rester dans le budget.
    let side = |v: f64| ((v * scale).floor() as u32).max(1);
    (side(w), side(h))
}

fn resize(image: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if image.width() == w && image.height() == h {
        image.clone()
    } else {
        image::imageops::resize(image, w, h, FilterType::Triangle)
    }
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|e| Error::InvalidCadre(format!("préparation impossible ({e})")))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{files::FilesDoc, sync::tests::sync_pair};
    use image::{codecs::gif::GifEncoder, Delay, Frame, Rgba};

    const HASH: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
    const HASH2: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn files() -> (SharedFile, SharedFile, SharedFile) {
        let mut doc = FilesDoc::new();
        let photo = doc
            .add("Lucas", "nous.jpg", 1000, HASH, "image/jpeg", 1)
            .unwrap();
        let gif = doc
            .add("Eve", "chat.gif", 2000, HASH2, "image/gif", 2)
            .unwrap();
        let text = doc
            .add("Eve", "liste.txt", 10, HASH2, "text/plain", 3)
            .unwrap();
        (photo, gif, text)
    }

    #[test]
    fn add_list_fit_remove() {
        let (photo, gif, text) = files();
        let all = vec![photo.clone(), gif.clone(), text.clone()];
        let mut doc = CadresDoc::new();
        let a = doc.add("Lucas", &photo, &all, 10).unwrap();
        let b = doc.add("Eve", &gif, &all, 20).unwrap();
        assert_eq!(a.fit, FIT_FILL);
        let list = doc.list(&all);
        assert_eq!(
            list.iter().map(|v| v.cadre.id.clone()).collect::<Vec<_>>(),
            vec![b.id.clone(), a.id.clone()]
        );
        assert_eq!(list[1].file, photo);

        assert_eq!(doc.set_fit(&a.id, FIT_WHOLE).unwrap().fit, FIT_WHOLE);
        assert!(doc.set_fit(&a.id, "etire").is_err());
        assert!(matches!(
            doc.set_fit("inconnu", FIT_FILL),
            Err(Error::CadreNotFound(_))
        ));

        doc.remove(&b.id).unwrap();
        assert_eq!(doc.list(&all).len(), 1);
        assert!(matches!(doc.remove(&b.id), Err(Error::CadreNotFound(_))));
    }

    #[test]
    fn refuses_what_the_widget_cannot_show() {
        let (photo, _, text) = files();
        let mut big = photo.clone();
        big.id = "gros".to_owned();
        big.size = MAX_IMAGE_SIZE + 1;
        let all = vec![photo.clone(), text.clone(), big.clone()];
        let mut doc = CadresDoc::new();
        assert!(matches!(
            doc.add(" ", &photo, &all, 1),
            Err(Error::MissingAuthor)
        ));
        assert!(doc.add("Eve", &text, &all, 1).is_err());
        assert!(doc.add("Eve", &big, &all, 1).is_err());
        doc.add("Eve", &photo, &all, 1).unwrap();
        let e = doc.add("Lucas", &photo, &all, 2).unwrap_err();
        assert!(e.to_string().contains("déjà sur le widget"), "{e}");
    }

    #[test]
    fn at_most_max_cadres() {
        let mut files_doc = FilesDoc::new();
        let all: Vec<SharedFile> = (0..=MAX_CADRES)
            .map(|i| {
                files_doc
                    .add("Eve", &format!("{i}.png"), 1, HASH, "image/png", i as i64)
                    .unwrap()
            })
            .collect();
        let mut doc = CadresDoc::new();
        for f in &all[..MAX_CADRES] {
            doc.add("Eve", f, &all, 1).unwrap();
        }
        assert!(doc.add("Eve", &all[MAX_CADRES], &all, 1).is_err());
    }

    #[test]
    fn deleted_files_disappear_from_the_widget() {
        let (photo, gif, _) = files();
        let all = vec![photo.clone(), gif.clone()];
        let mut doc = CadresDoc::new();
        doc.add("Eve", &photo, &all, 1).unwrap();
        doc.add("Eve", &gif, &all, 2).unwrap();
        let list = doc.list(std::slice::from_ref(&gif));
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file, gif);
    }

    #[test]
    fn invalid_entry_from_another_device_is_ignored() {
        let (photo, _, _) = files();
        let all = vec![photo.clone()];
        let mut doc = CadresDoc::new();
        let ok = doc.add("Eve", &photo, &all, 1).unwrap();
        let mut bad = ok.clone();
        bad.id = "bad".to_owned();
        bad.fit = "n'importe quoi".to_owned();
        reconcile_prop(&mut doc.doc, ROOT, "bad", &bad).unwrap();
        doc.doc.put(ROOT, "texte", "pas un cadre").unwrap();
        let list = doc.list(&all);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].cadre, ok);
    }

    #[test]
    fn same_image_added_on_two_phones_appears_once() {
        let (photo, gif, _) = files();
        let all = vec![photo.clone(), gif.clone()];
        let mut lucas = CadresDoc::new();
        let mut eve = lucas.fork();
        let first = lucas.add("Lucas", &photo, &all, 10).unwrap();
        eve.add("Eve", &photo, &all, 20).unwrap();
        eve.add("Eve", &gif, &all, 30).unwrap();
        lucas.merge(&mut eve).unwrap();
        eve.merge(&mut lucas).unwrap();
        assert_eq!(lucas.list(&all), eve.list(&all));
        let list = lucas.list(&all);
        assert_eq!(list.len(), 2);
        // L'entrée la plus ancienne est gardée, la même partout.
        assert_eq!(list[1].cadre, first);
    }

    #[test]
    fn concurrent_fit_and_remove_merge() {
        let (photo, _, _) = files();
        let all = vec![photo.clone()];
        let mut lucas = CadresDoc::new();
        let c = lucas.add("Lucas", &photo, &all, 1).unwrap();
        let mut eve = lucas.fork();
        lucas.set_fit(&c.id, FIT_WHOLE).unwrap();
        eve.remove(&c.id).unwrap();
        lucas.merge(&mut eve).unwrap();
        eve.merge(&mut lucas).unwrap();
        assert_eq!(lucas.list(&all), eve.list(&all));
    }

    #[test]
    fn syncs_with_protocol_and_survives_reload() {
        let (photo, gif, _) = files();
        let all = vec![photo.clone(), gif.clone()];
        let mut pc = CadresDoc::new();
        let mut phone = CadresDoc::new();
        pc.add("Lucas", &photo, &all, 1).unwrap();
        phone.add("Eve", &gif, &all, 2).unwrap();
        sync_pair(&mut pc, &mut phone);
        assert_eq!(pc.list(&all), phone.list(&all));
        assert_eq!(pc.list(&all).len(), 2);
        let reloaded = CadresDoc::load(&pc.save()).unwrap();
        assert_eq!(reloaded.list(&all), pc.list(&all));
    }

    // ---------- Préparation des images ----------

    fn gif_bytes(width: u32, height: u32, delays_ms: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut out);
            encoder
                .set_repeat(image::codecs::gif::Repeat::Infinite)
                .unwrap();
            for (i, d) in delays_ms.iter().enumerate() {
                let shade = (i * 40 % 256) as u8;
                let buffer = RgbaImage::from_pixel(width, height, Rgba([shade, 0, 255, 255]));
                encoder
                    .encode_frame(Frame::from_parts(
                        buffer,
                        0,
                        0,
                        Delay::from_numer_denom_ms(*d, 1),
                    ))
                    .unwrap();
            }
        }
        out
    }

    fn decode(png: &[u8]) -> RgbaImage {
        image::load_from_memory_with_format(png, ImageFormat::Png)
            .unwrap()
            .into_rgba8()
    }

    #[test]
    fn small_gif_keeps_all_its_frames() {
        let p = prepare(&gif_bytes(40, 20, &[200, 200, 200])).unwrap();
        assert_eq!(p.frames.len(), 3);
        assert_eq!(p.sequence, vec![0, 1, 2]);
        assert_eq!((p.width, p.height), (40, 20));
        assert_eq!(p.frame_ms, 200);
        let first = decode(&p.frames[0]);
        let second = decode(&p.frames[1]);
        assert_eq!(first.dimensions(), (40, 20));
        assert_ne!(first.get_pixel(0, 0), second.get_pixel(0, 0));
    }

    #[test]
    fn fast_gif_is_slowed_to_ten_frames_per_second() {
        // 60 images à 20 ms = 1,2 s : 12 images de 100 ms.
        let p = prepare(&gif_bytes(16, 16, &[20; 60])).unwrap();
        assert_eq!(p.frames.len(), 12);
        assert_eq!(p.sequence.len(), 12);
        assert_eq!(p.frame_ms, MIN_FRAME_MS);
    }

    #[test]
    fn long_gif_keeps_at_most_max_frames() {
        let p = prepare(&gif_bytes(16, 16, &[100; 90])).unwrap();
        assert_eq!(p.frames.len(), MAX_FRAMES);
        // Même durée totale (9 s) : chaque image dure plus longtemps.
        assert_eq!(p.frame_ms, 9000 / MAX_FRAMES as u32);
    }

    #[test]
    fn longer_frames_are_repeated_not_copied() {
        let p = prepare(&gif_bytes(16, 16, &[100, 300, 100])).unwrap();
        assert_eq!(p.frames.len(), 3);
        assert_eq!(p.sequence, vec![0, 1, 1, 1, 2]);
        assert_eq!(p.frame_ms, 100);
    }

    #[test]
    fn single_frame_gif_is_a_still_image() {
        let p = prepare(&gif_bytes(30, 30, &[0])).unwrap();
        assert_eq!(p.frames.len(), 1);
        assert_eq!(p.sequence, vec![0]);
        assert_eq!(p.frame_ms, 0);
    }

    #[test]
    fn still_photos_are_reduced() {
        let photo = RgbaImage::from_pixel(2400, 1200, Rgba([10, 200, 30, 255]));
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(photo)
            .into_rgb8()
            .write_to(&mut jpeg, ImageFormat::Jpeg)
            .unwrap();
        let p = prepare(jpeg.get_ref()).unwrap();
        assert_eq!(p.frames.len(), 1);
        assert_eq!(p.frame_ms, 0);
        assert_eq!((p.width, p.height), (MAX_STILL_SIDE, MAX_STILL_SIDE / 2));
        assert_eq!(decode(&p.frames[0]).dimensions(), (p.width, p.height));
    }

    #[test]
    fn garbage_and_other_formats_are_refused() {
        assert!(prepare(b"pas une image").is_err());
        // En-tête BMP : format reconnu mais refusé.
        let e = prepare(b"BM\0\0\0\0\0\0\0\0\0\0\0\0").unwrap_err();
        assert!(e.to_string().contains("non pris en charge"), "{e}");
        // GIF tronqué.
        let gif = gif_bytes(8, 8, &[100, 100]);
        assert!(prepare(&gif[..gif.len() / 2]).is_err());
        assert!(prepare(&vec![0; MAX_IMAGE_SIZE as usize + 1]).is_err());
    }

    #[test]
    fn frame_picks_follow_time() {
        assert_eq!(pick_frames(&[]), (vec![0], 0));
        assert_eq!(pick_frames(&[500]), (vec![0], 500));
        assert_eq!(pick_frames(&[100, 100, 100]), (vec![0, 1, 2], 100));
        assert_eq!(pick_frames(&[200, 200]), (vec![0, 1], 200));
        // Une image longue au milieu revient plusieurs fois.
        assert_eq!(
            pick_frames(&[100, 300, 100, 100]),
            (vec![0, 1, 1, 1, 2, 3], 100)
        );
        // Trop rapide (6 × 30 ms = 180 ms) : une seule image.
        assert_eq!(pick_frames(&[30; 6]).0, vec![0]);
        assert!(pick_frames(&[100; 500]).0.len() <= MAX_FRAMES);
    }

    #[test]
    fn sizes_stay_within_budget() {
        for (w, h, n, side) in [
            (4000, 3000, 1, MAX_STILL_SIDE),
            (500, 500, 40, MAX_ANIMATED_SIDE),
            (1920, 1080, 30, MAX_ANIMATED_SIDE),
            (10, 10, 40, MAX_ANIMATED_SIDE),
            (1, 9000, 1, MAX_STILL_SIDE),
        ] {
            let (tw, th) = target_size(w, h, n, side);
            assert!(tw <= w && th <= h, "{w}×{h} agrandie en {tw}×{th}");
            assert!(tw.max(th) <= side);
            assert!(u64::from(tw) * u64::from(th) * 4 * n <= FRAME_BUDGET.max(4 * n));
        }
        assert_eq!(target_size(10, 10, 40, MAX_ANIMATED_SIDE), (10, 10));
    }
}
