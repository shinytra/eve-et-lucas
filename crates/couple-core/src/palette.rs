//! Couleurs de l'appli tirées de l'image de fond (dans l'esprit de
//! « Material You » sur Android).
//!
//! L'interface réduit l'image (128 px de côté au plus) et envoie ses pixels ;
//! le cœur en tire :
//! - les couleurs dominantes (k-moyennes dans l'espace OKLab, où les
//!   distances suivent ce que l'œil perçoit) ;
//! - une couleur principale et une secondaire, prises parmi les couleurs
//!   vives de l'image ;
//! - un verre clair ou sombre (celui qui laisse le plus voir l'image), dont
//!   l'opacité est juste assez forte pour que le texte reste lisible même
//!   au-dessus des zones les plus claires (ou les plus sombres) de l'image ;
//! - un flou plus fort pour une image chargée de détails, une saturation
//!   plus douce pour une image déjà très colorée.
//!
//! Tout est déterministe : la même image donne toujours les mêmes couleurs.

use data_encoding::BASE64;

use crate::appearance::{Appearance, WallpaperKind, LIMITS};
use crate::{Error, Result};

/// Côté maximal de l'échantillon envoyé par l'interface, en pixels.
pub const MAX_SAMPLE_SIDE: usize = 128;
/// Nombre de couleurs dominantes cherchées.
const CLUSTERS: usize = 6;
/// Part minimale de l'image pour qu'une couleur compte (fond derrière le
/// verre, couleur principale).
const MIN_SHARE: f32 = 0.03;
/// Contraste minimal (WCAG) du texte sur le verre (seuil AAA).
const TEXT_CONTRAST: f32 = 7.0;
/// Contraste minimal du texte atténué (dates, détails) : celui du thème
/// Givre, au-dessus du seuil des textes larges (3).
const MUTED_CONTRAST: f32 = 4.0;
/// Contraste minimal d'une couleur d'accent sur le verre (seuil AA).
const MIN_CONTRAST: f32 = 4.5;
/// Opacité du texte atténué (`--muted` dans style.css).
const MUTED_ALPHA: f32 = 0.62;
/// Opacité du verre la plus forte proposée, en %.
const MAX_OPACITY: u16 = 92;

/// Pixels d'une image réduite (RVB, ligne par ligne).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSample {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 3]>,
}

impl ImageSample {
    /// Échantillon à partir d'octets RVB (3 par pixel).
    pub fn from_rgb(width: usize, height: usize, rgb: &[u8]) -> Result<Self> {
        let invalid = |why: &str| Error::InvalidWallpaper(format!("échantillon illisible : {why}"));
        if width == 0 || height == 0 {
            return Err(invalid("image vide"));
        }
        if width > MAX_SAMPLE_SIDE || height > MAX_SAMPLE_SIDE {
            return Err(invalid("image trop grande"));
        }
        if rgb.len() != width * height * 3 {
            return Err(invalid("taille incohérente"));
        }
        let pixels = rgb.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    /// Comme [`ImageSample::from_rgb`], octets en base64 (l'interface ne peut
    /// pas envoyer de binaire brut sur Android).
    pub fn from_base64(width: usize, height: usize, data: &str) -> Result<Self> {
        if data.len() > MAX_SAMPLE_SIDE * MAX_SAMPLE_SIDE * 4 + 4 {
            return Err(Error::InvalidWallpaper(
                "échantillon illisible : image trop grande".into(),
            ));
        }
        let rgb = BASE64
            .decode(data.as_bytes())
            .map_err(|_| Error::InvalidWallpaper("échantillon illisible : base64".into()))?;
        Self::from_rgb(width, height, &rgb)
    }
}

// ---------- Espaces de couleur ----------

/// Couleur dans l'espace OKLab (`l` de 0 à 1, `a` et `b` vers ±0,4).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Lab {
    l: f32,
    a: f32,
    b: f32,
}

impl Lab {
    fn chroma(self) -> f32 {
        self.a.hypot(self.b)
    }

    /// Teinte en degrés (0 à 360).
    fn hue(self) -> f32 {
        self.b.atan2(self.a).to_degrees().rem_euclid(360.0)
    }

    fn from_lch(l: f32, c: f32, h: f32) -> Self {
        let (s, co) = h.to_radians().sin_cos();
        Self {
            l,
            a: c * co,
            b: c * s,
        }
    }

    fn distance2(self, o: Self) -> f32 {
        (self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)
    }
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0 à 1, avec gamma) → OKLab.
fn srgb_to_lab([r, g, b]: [f32; 3]) -> Lab {
    let (r, g, b) = (to_linear(r), to_linear(g), to_linear(b));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    Lab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

/// OKLab → sRGB (0 à 1, avec gamma), sans ramener dans les bornes.
fn lab_to_srgb(c: Lab) -> [f32; 3] {
    let l = (c.l + 0.396_337_78 * c.a + 0.215_803_76 * c.b).powi(3);
    let m = (c.l - 0.105_561_346 * c.a - 0.063_854_17 * c.b).powi(3);
    let s = (c.l - 0.089_484_18 * c.a - 1.291_485_5 * c.b).powi(3);
    [
        to_gamma(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
        to_gamma(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
        to_gamma(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
    ]
}

fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|c| (-0.000_5..=1.000_5).contains(c))
}

/// Couleur affichable la plus proche : on garde la clarté et la teinte, on
/// réduit la vivacité (chroma) jusqu'à rentrer dans le sRGB.
fn lch_to_srgb(l: f32, c: f32, h: f32) -> [f32; 3] {
    let l = l.clamp(0.0, 1.0);
    let rgb = lab_to_srgb(Lab::from_lch(l, c, h));
    if in_gamut(rgb) {
        return rgb.map(|x| x.clamp(0.0, 1.0));
    }
    let (mut lo, mut hi) = (0.0, c);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(lab_to_srgb(Lab::from_lch(l, mid, h))) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lab_to_srgb(Lab::from_lch(l, lo, h)).map(|x| x.clamp(0.0, 1.0))
}

fn hex(rgb: [f32; 3]) -> String {
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Couleur arrondie comme elle sera enregistrée (`#rrggbb`).
fn rounded(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() / 255.0)
}

fn from_hex(s: &str) -> Option<[f32; 3]> {
    let n = u32::from_str_radix(s.strip_prefix('#')?, 16).ok()?;
    Some([(n >> 16) & 255, (n >> 8) & 255, n & 255].map(|c| c as f32 / 255.0))
}

/// Luminance relative (WCAG), de 0 (noir) à 1 (blanc).
fn luminance(rgb: [f32; 3]) -> f32 {
    let [r, g, b] = rgb.map(to_linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn contrast(x: [f32; 3], y: [f32; 3]) -> f32 {
    let (a, b) = (luminance(x), luminance(y));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `top` posé avec l'opacité `alpha` sur `bottom` (comme le navigateur, dans
/// l'espace sRGB avec gamma).
fn over(top: [f32; 3], alpha: f32, bottom: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|i| top[i] * alpha + bottom[i] * (1.0 - alpha))
}

fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

// ---------- Analyse de l'image ----------

/// Une couleur dominante et la part de l'image qu'elle occupe.
#[derive(Debug, Clone, Copy)]
struct Swatch {
    lab: Lab,
    rgb: [f32; 3],
    share: f32,
}

struct Analysis {
    /// Couleurs dominantes, de la plus présente à la moins présente.
    swatches: Vec<Swatch>,
    /// Différence moyenne de clarté entre pixels voisins : une image pleine
    /// de détails (feuillage, foule) en a beaucoup.
    busyness: f32,
    /// Vivacité moyenne des couleurs.
    mean_chroma: f32,
    /// Clarté moyenne (OKLab).
    mean_lightness: f32,
}

fn analyze(sample: &ImageSample) -> Analysis {
    let labs: Vec<Lab> = sample
        .pixels
        .iter()
        .map(|p| srgb_to_lab(p.map(|c| f32::from(c) / 255.0)))
        .collect();
    let n = labs.len() as f32;

    // Détails : écart de clarté avec le voisin de droite et celui du dessous.
    let (w, h) = (sample.width, sample.height);
    let (mut diff, mut pairs) = (0.0, 0u32);
    for y in 0..h {
        for x in 0..w {
            let here = labs[y * w + x].l;
            if x + 1 < w {
                diff += (here - labs[y * w + x + 1].l).abs();
                pairs += 1;
            }
            if y + 1 < h {
                diff += (here - labs[(y + 1) * w + x].l).abs();
                pairs += 1;
            }
        }
    }
    let busyness = if pairs == 0 { 0.0 } else { diff / pairs as f32 };
    let mean_chroma = labs.iter().map(|c| c.chroma()).sum::<f32>() / n;
    let mean_lightness = labs.iter().map(|c| c.l).sum::<f32>() / n;

    Analysis {
        swatches: clusters(&sample.pixels, &labs),
        busyness,
        mean_chroma,
        mean_lightness,
    }
}

/// Couleurs dominantes par k-moyennes. Départ déterministe : les cases les
/// plus remplies d'un histogramme grossier (16 niveaux par canal), assez
/// éloignées les unes des autres.
fn clusters(pixels: &[[u8; 3]], labs: &[Lab]) -> Vec<Swatch> {
    let mut bins = vec![(0u32, Lab::default()); 16 * 16 * 16];
    for (p, lab) in pixels.iter().zip(labs) {
        let i =
            (usize::from(p[0] >> 4) << 8) | (usize::from(p[1] >> 4) << 4) | usize::from(p[2] >> 4);
        let bin = &mut bins[i];
        bin.0 += 1;
        bin.1.l += lab.l;
        bin.1.a += lab.a;
        bin.1.b += lab.b;
    }
    let mut filled: Vec<(u32, Lab)> = bins
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, sum)| {
            let k = count as f32;
            (
                count,
                Lab {
                    l: sum.l / k,
                    a: sum.a / k,
                    b: sum.b / k,
                },
            )
        })
        .collect();
    // Tri stable : à égalité, l'ordre de l'histogramme décide (déterministe).
    filled.sort_by_key(|(count, _)| std::cmp::Reverse(*count));
    let mut centers: Vec<Lab> = Vec::new();
    for (_, lab) in &filled {
        if centers.len() == CLUSTERS {
            break;
        }
        if centers.iter().all(|c| c.distance2(*lab) > 0.05_f32.powi(2)) {
            centers.push(*lab);
        }
    }

    let mut owner = vec![0usize; labs.len()];
    for _ in 0..12 {
        for (o, lab) in owner.iter_mut().zip(labs) {
            *o = nearest(&centers, *lab);
        }
        let mut sums = vec![(0u32, Lab::default()); centers.len()];
        for (o, lab) in owner.iter().zip(labs) {
            let s = &mut sums[*o];
            s.0 += 1;
            s.1.l += lab.l;
            s.1.a += lab.a;
            s.1.b += lab.b;
        }
        for (center, (count, sum)) in centers.iter_mut().zip(&sums) {
            if *count > 0 {
                let k = *count as f32;
                *center = Lab {
                    l: sum.l / k,
                    a: sum.a / k,
                    b: sum.b / k,
                };
            }
        }
    }
    let mut counts = vec![0u32; centers.len()];
    for lab in labs {
        counts[nearest(&centers, *lab)] += 1;
    }
    let total = labs.len().max(1) as f32;
    let mut swatches: Vec<Swatch> = centers
        .into_iter()
        .zip(counts)
        .filter(|(_, count)| *count > 0)
        .map(|(lab, count)| Swatch {
            lab,
            rgb: rounded(lab_to_srgb(lab)),
            share: count as f32 / total,
        })
        .collect();
    swatches.sort_by(|x, y| y.share.total_cmp(&x.share));
    swatches
}

fn nearest(centers: &[Lab], lab: Lab) -> usize {
    let mut best = (0, f32::INFINITY);
    for (i, c) in centers.iter().enumerate() {
        let d = c.distance2(lab);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

// ---------- Thème ----------

/// Couleurs d'un verre clair ou sombre.
struct Tone {
    dark: bool,
    tint: [f32; 3],
    text: [f32; 3],
    dim: u16,
    floor: u16,
}

impl Tone {
    fn new(dark: bool, hue: f32, chroma: f32) -> Self {
        if dark {
            Self {
                dark,
                tint: rounded(lch_to_srgb(0.2, chroma.min(0.04), hue)),
                text: rounded(lch_to_srgb(0.97, chroma.min(0.012), hue)),
                dim: 15,
                floor: 30,
            }
        } else {
            Self {
                dark,
                tint: rounded(lch_to_srgb(0.975, chroma.min(0.018), hue)),
                text: rounded(lch_to_srgb(0.22, chroma.min(0.04), hue)),
                dim: 0,
                floor: 35,
            }
        }
    }

    /// Opacité du verre (en %) juste suffisante pour lire le texte atténué
    /// au-dessus de chaque couleur importante de l'image (assombrie par le
    /// voile du fond). Le flou du verre mélange les détails : les couleurs
    /// dominantes représentent bien ce qu'on voit derrière.
    fn opacity(&self, swatches: &[Swatch]) -> u16 {
        let backgrounds: Vec<[f32; 3]> = swatches
            .iter()
            .filter(|s| s.share >= MIN_SHARE)
            .map(|s| over([0.0; 3], f32::from(self.dim) / 100.0, s.rgb))
            .collect();
        (self.floor..=MAX_OPACITY)
            .find(|op| {
                let alpha = f32::from(*op) / 100.0;
                backgrounds.iter().all(|bg| {
                    let glass = over(self.tint, alpha, *bg);
                    let muted = over(self.text, MUTED_ALPHA, glass);
                    contrast(self.text, glass) >= TEXT_CONTRAST
                        && contrast(muted, glass) >= MUTED_CONTRAST
                })
            })
            .unwrap_or(MAX_OPACITY)
    }
}

/// Couleur d'accent lisible sur le verre : même teinte, clarté ajustée
/// jusqu'à un contraste de 4,5 avec la teinte du verre.
fn accent(hue: f32, chroma: f32, tone: &Tone) -> [f32; 3] {
    let chroma = chroma.clamp(0.09, 0.19);
    let (mut l, step) = if tone.dark {
        (0.78, 0.02)
    } else {
        (0.55, -0.02)
    };
    let mut rgb = lch_to_srgb(l, chroma, hue);
    while contrast(rgb, tone.tint) < MIN_CONTRAST && (0.2..=0.95).contains(&l) {
        l += step;
        rgb = lch_to_srgb(l, chroma, hue);
    }
    rgb
}

impl Appearance {
    /// Copie de ce thème avec les couleurs et le verre tirés de l'image.
    /// Le reste (police, arrondi, ombre…) est gardé.
    pub fn adapted_to_image(&self, sample: &ImageSample) -> Appearance {
        let analysis = analyze(sample);
        let main: Vec<Swatch> = analysis
            .swatches
            .iter()
            .copied()
            .filter(|s| s.share >= MIN_SHARE)
            .collect();
        let main = if main.is_empty() {
            analysis.swatches.clone()
        } else {
            main
        };

        // Couleur principale : vive et assez présente. Une image presque
        // grise garde un bleu-violet discret, comme le thème Givre.
        let score = |s: &Swatch| s.share.sqrt() * s.lab.chroma().powi(2);
        let seed = main
            .iter()
            .copied()
            .max_by(|x, y| score(x).total_cmp(&score(y)));
        let (hue, chroma) = match seed {
            Some(s) if s.lab.chroma() >= 0.025 => (s.lab.hue(), s.lab.chroma()),
            _ => (275.0, 0.03),
        };
        // Secondaire : une autre teinte vive de l'image, sinon une voisine.
        let second = main
            .iter()
            .copied()
            .filter(|s| s.lab.chroma() >= 0.04 && hue_distance(s.lab.hue(), hue) >= 35.0)
            .max_by(|x, y| score(x).total_cmp(&score(y)));
        let (hue2, chroma2) = match second {
            Some(s) => (s.lab.hue(), s.lab.chroma()),
            None => ((hue + 50.0).rem_euclid(360.0), chroma),
        };

        // Verre clair ou sombre : celui qui laisse le plus voir l'image ; à
        // égalité, celui qui va avec l'image (sombre pour une image sombre).
        let light = Tone::new(false, hue, chroma);
        let dark = Tone::new(true, hue, chroma);
        let (op_light, op_dark) = (light.opacity(&main), dark.opacity(&main));
        let prefer_dark = analysis.mean_lightness < 0.62;
        let (tone, opacity) = match op_dark.cmp(&op_light) {
            std::cmp::Ordering::Less => (dark, op_dark),
            std::cmp::Ordering::Greater => (light, op_light),
            std::cmp::Ordering::Equal if prefer_dark => (dark, op_dark),
            std::cmp::Ordering::Equal => (light, op_light),
        };

        let mut wall: Vec<String> = analysis.swatches.iter().map(|s| hex(s.rgb)).collect();
        while wall.len() < 3 {
            let last = wall.last().cloned().unwrap_or_else(|| hex(tone.tint));
            wall.push(last);
        }

        let mut a = self.clone();
        a.accent = hex(accent(hue, chroma, &tone));
        a.accent2 = hex(accent(hue2, chroma2, &tone));
        a.text = hex(tone.text);
        a.glass_tint = hex(tone.tint);
        a.glass_opacity = opacity;
        // Plus de détails dans l'image : plus de flou derrière le verre.
        a.blur = (14.0 + analysis.busyness * 300.0).round().clamp(14.0, 40.0) as u16;
        // Image déjà très colorée : on évite de la saturer encore.
        a.saturation = (170.0 - analysis.mean_chroma * 500.0)
            .round()
            .clamp(110.0, 170.0) as u16;
        a.border_opacity = if tone.dark { 22 } else { 50 };
        a.wallpaper = WallpaperKind::Image;
        a.wallpaper_colors = [wall[0].clone(), wall[1].clone(), wall[2].clone()];
        a.wallpaper_dim = tone.dim.min(LIMITS.wallpaper_dim.max);
        a.image_colors = true;
        a.sanitized()
    }
}

/// Contraste (WCAG) entre deux couleurs `#rrggbb` ; `None` si l'une est
/// invalide. Sert aux tests et à vérifier un thème.
pub fn contrast_ratio(x: &str, y: &str) -> Option<f32> {
    Some(contrast(from_hex(x)?, from_hex(y)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Image faite de bandes horizontales de couleurs `#rrggbb`.
    fn bands(width: usize, colors: &[(&str, usize)]) -> ImageSample {
        let mut rgb = Vec::new();
        let mut height = 0;
        for (color, rows) in colors {
            let c = from_hex(color).unwrap().map(|x| (x * 255.0).round() as u8);
            for _ in 0..rows * width {
                rgb.extend_from_slice(&c);
            }
            height += rows;
        }
        ImageSample::from_rgb(width, height, &rgb).unwrap()
    }

    /// Image pseudo-aléatoire (générateur congruentiel, déterministe).
    fn noise(seed: u32, width: usize, height: usize) -> ImageSample {
        let mut state = seed;
        let rgb: Vec<u8> = (0..width * height * 3)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        ImageSample::from_rgb(width, height, &rgb).unwrap()
    }

    fn lab_of(hex_color: &str) -> Lab {
        srgb_to_lab(from_hex(hex_color).unwrap())
    }

    /// Le texte atténué reste lisible au-dessus de chaque couleur importante.
    fn assert_readable(a: &Appearance, sample: &ImageSample) {
        let analysis = analyze(sample);
        let tint = from_hex(&a.glass_tint).unwrap();
        let text = from_hex(&a.text).unwrap();
        let alpha = f32::from(a.glass_opacity) / 100.0;
        for s in analysis.swatches.iter().filter(|s| s.share >= MIN_SHARE) {
            let bg = over([0.0; 3], f32::from(a.wallpaper_dim) / 100.0, s.rgb);
            let glass = over(tint, alpha, bg);
            let muted = over(text, MUTED_ALPHA, glass);
            assert!(
                a.glass_opacity < MAX_OPACITY,
                "verre au maximum sur {} : {a:?}",
                hex(s.rgb)
            );
            assert!(
                contrast(muted, glass) >= MUTED_CONTRAST,
                "texte atténué illisible sur {} : {a:?}",
                hex(s.rgb)
            );
            assert!(
                contrast(text, glass) >= TEXT_CONTRAST,
                "texte illisible : {a:?}"
            );
        }
    }

    #[test]
    fn oklab_roundtrip() {
        for color in [
            "#000000", "#ffffff", "#ff0000", "#5b6cff", "#2bb673", "#f5b700",
        ] {
            let rgb = from_hex(color).unwrap();
            assert_eq!(hex(lab_to_srgb(srgb_to_lab(rgb))), color);
        }
        // Valeurs de référence d'OKLab.
        let white = lab_of("#ffffff");
        assert!((white.l - 1.0).abs() < 1e-3 && white.chroma() < 1e-3);
        let red = lab_of("#ff0000");
        assert!((red.l - 0.628).abs() < 1e-3);
        assert!((red.hue() - 29.2).abs() < 0.5);
    }

    #[test]
    fn out_of_gamut_colors_keep_hue_and_lightness() {
        let rgb = lch_to_srgb(0.7, 0.4, 145.0);
        assert!(in_gamut(rgb));
        let lab = srgb_to_lab(rgb);
        assert!((lab.l - 0.7).abs() < 0.01);
        assert!(hue_distance(lab.hue(), 145.0) < 2.0);
    }

    #[test]
    fn dominant_colors_are_found() {
        let sample = bands(40, &[("#ff7a1a", 20), ("#3b1a6b", 12), ("#f2c6a0", 8)]);
        let swatches = analyze(&sample).swatches;
        assert_eq!(swatches.len(), 3);
        assert_eq!(hex(swatches[0].rgb), "#ff7a1a");
        assert!((swatches[0].share - 0.5).abs() < 1e-6);
        assert_eq!(hex(swatches[1].rgb), "#3b1a6b");
        assert_eq!(hex(swatches[2].rgb), "#f2c6a0");
    }

    #[test]
    fn sunset_gives_warm_accent_and_its_own_colors() {
        // Coucher de soleil sombre : ciel violet, soleil orange, mer bleue.
        let sample = bands(
            48,
            &[
                ("#2a1650", 24),
                ("#ff8a3d", 10),
                ("#c2457a", 8),
                ("#132a4f", 22),
            ],
        );
        let a = Appearance::default().adapted_to_image(&sample);
        assert!(a.image_colors);
        assert_eq!(a.wallpaper, WallpaperKind::Image);
        // Couleurs du fond = couleurs dominantes de l'image.
        assert_eq!(a.wallpaper_colors[0], "#2a1650");
        assert!(a.wallpaper_colors.contains(&"#132a4f".to_owned()));
        // Accent : l'orange, la couleur la plus vive.
        let accent = lab_of(&a.accent);
        assert!(
            hue_distance(accent.hue(), lab_of("#ff8a3d").hue()) < 10.0,
            "accent {}",
            a.accent
        );
        // Deuxième couleur : une autre teinte de l'image.
        assert!(hue_distance(lab_of(&a.accent2).hue(), accent.hue()) >= 30.0);
        // Image sombre : verre sombre et texte clair.
        assert!(lab_of(&a.glass_tint).l < 0.3);
        assert!(lab_of(&a.text).l > 0.9);
        assert_readable(&a, &sample);
        // Le reste du thème est gardé.
        assert_eq!(a.font, Appearance::default().font);
        assert_eq!(a.radius, Appearance::default().radius);
    }

    #[test]
    fn bright_pastel_image_gets_light_glass() {
        let sample = bands(32, &[("#fde2e4", 16), ("#cde8f6", 10), ("#fff1c1", 6)]);
        let a = Appearance::default().adapted_to_image(&sample);
        assert!(lab_of(&a.glass_tint).l > 0.9);
        assert!(lab_of(&a.text).l < 0.4);
        assert_readable(&a, &sample);
        // Accent assez foncé pour se lire sur le verre clair.
        assert!(contrast_ratio(&a.accent, &a.glass_tint).unwrap() >= MIN_CONTRAST);
        assert!(contrast_ratio(&a.accent2, &a.glass_tint).unwrap() >= MIN_CONTRAST);
    }

    #[test]
    fn harsh_contrast_needs_more_opaque_glass() {
        // Moitié blanc pur, moitié noir pur : le pire cas pour le verre.
        let harsh = bands(16, &[("#ffffff", 8), ("#000000", 8)]);
        let soft = bands(16, &[("#1d2440", 16)]);
        let a = Appearance::default().adapted_to_image(&harsh);
        let b = Appearance::default().adapted_to_image(&soft);
        assert!(a.glass_opacity > b.glass_opacity + 20, "{a:?} / {b:?}");
        assert_readable(&a, &harsh);
        assert_readable(&b, &soft);
    }

    #[test]
    fn grey_image_keeps_a_gentle_accent() {
        let sample = bands(16, &[("#808080", 8), ("#b0b0b0", 8)]);
        let a = Appearance::default().adapted_to_image(&sample);
        let accent = lab_of(&a.accent);
        assert!(accent.chroma() >= 0.08, "accent trop gris : {}", a.accent);
        assert_readable(&a, &sample);
    }

    #[test]
    fn detailed_images_get_more_blur() {
        let busy = noise(7, 64, 64);
        let calm = bands(64, &[("#334455", 32), ("#3a4a5a", 32)]);
        let a = Appearance::default().adapted_to_image(&busy);
        let b = Appearance::default().adapted_to_image(&calm);
        assert!(a.blur > b.blur + 10, "{} / {}", a.blur, b.blur);
        assert!(a.blur <= LIMITS.blur.max);
        assert_readable(&a, &busy);
    }

    #[test]
    fn colorful_images_are_less_saturated() {
        let vivid = bands(16, &[("#ff0040", 8), ("#00c0ff", 8)]);
        let dull = bands(16, &[("#6b6560", 8), ("#8a857f", 8)]);
        let a = Appearance::default().adapted_to_image(&vivid);
        let b = Appearance::default().adapted_to_image(&dull);
        assert!(a.saturation < b.saturation);
    }

    #[test]
    fn random_images_stay_readable_and_valid() {
        for seed in 0..20 {
            // Bruit lissé : grosses taches de couleurs au hasard.
            let raw = noise(seed, 4, 4);
            let mut rgb = Vec::new();
            for y in 0..32 {
                for x in 0..32 {
                    rgb.extend_from_slice(&raw.pixels[(y / 8) * 4 + x / 8]);
                }
            }
            let sample = ImageSample::from_rgb(32, 32, &rgb).unwrap();
            let a = Appearance::default().adapted_to_image(&sample);
            assert_eq!(a.clone().sanitized(), a, "thème déjà valide");
            assert_readable(&a, &sample);
        }
    }

    #[test]
    fn same_image_same_colors() {
        let sample = noise(42, 50, 30);
        assert_eq!(
            Appearance::default().adapted_to_image(&sample),
            Appearance::default().adapted_to_image(&sample)
        );
    }

    #[test]
    fn invalid_samples_are_rejected() {
        assert!(ImageSample::from_rgb(0, 4, &[]).is_err());
        assert!(ImageSample::from_rgb(2, 2, &[0; 11]).is_err());
        assert!(ImageSample::from_rgb(129, 1, &[0; 129 * 3]).is_err());
        assert!(ImageSample::from_base64(1, 1, "pas du base64 !").is_err());
        let ok = BASE64.encode(&[255, 0, 0, 0, 0, 255]);
        assert_eq!(
            ImageSample::from_base64(2, 1, &ok).unwrap().pixels,
            vec![[255, 0, 0], [0, 0, 255]]
        );
        let huge = "A".repeat(MAX_SAMPLE_SIDE * MAX_SAMPLE_SIDE * 5);
        assert!(ImageSample::from_base64(128, 128, &huge).is_err());
    }
}
