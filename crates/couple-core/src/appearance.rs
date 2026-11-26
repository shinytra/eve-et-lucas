//! Apparence de l'appli (interface « verre dépoli » personnalisable).
//!
//! Contrairement aux notes, l'apparence est propre à chaque appareil : Eve et
//! Lucas peuvent avoir chacun leur thème. Elle n'est donc pas dans un document
//! Automerge mais dans un simple fichier JSON local. Pour partager un thème,
//! on passe par un « code de thème » (export / import), voir
//! [`Appearance::to_code`] et [`Appearance::from_code`].
//!
//! Toute valeur venue de l'interface, d'un fichier ou d'un code importé passe
//! par [`Appearance::sanitized`] : les nombres sont ramenés dans leurs bornes
//! et les couleurs invalides remplacées, pour que l'affichage reste toujours
//! utilisable.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

/// Nombre maximal de thèmes personnels enregistrés.
pub const MAX_SAVED_THEMES: usize = 30;
/// Longueur maximale du nom d'un thème personnel (en caractères).
pub const MAX_THEME_NAME: usize = 40;
/// Taille maximale d'une image de fond (data URL, en octets).
pub const MAX_WALLPAPER_BYTES: usize = 12 * 1024 * 1024;
/// Préfixe des codes de thème, pour reconnaître un code collé.
const CODE_PREFIX: &str = "EVELUCAS-THEME:";

/// Police de l'interface. Ce sont des familles du système (rien à
/// télécharger, l'appli marche hors ligne) ; l'interface choisit la meilleure
/// police disponible pour chaque famille.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Font {
    #[default]
    System,
    Rounded,
    Serif,
    Elegant,
    Mono,
    Handwritten,
}

/// Espacement entre les éléments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Spacious,
}

/// Type de fond d'écran derrière le verre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WallpaperKind {
    /// Taches de couleur floues (fond typique du glassmorphism).
    #[default]
    Mesh,
    /// Dégradé linéaire entre les couleurs du fond.
    Linear,
    /// Première couleur du fond, unie.
    Solid,
    /// Image choisie sur l'appareil (enregistrée à part).
    Image,
}

/// Tous les réglages d'apparence. La lecture est tolérante (voir
/// `From<Value>`) : un champ absent ou illisible (ancienne ou future version
/// de l'appli, fichier modifié à la main) garde la valeur du thème par défaut
/// sans faire perdre les autres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Value")]
pub struct Appearance {
    /// Préréglage d'origine (sert à afficher « modifié » et à réinitialiser).
    pub preset: String,
    /// Couleur principale (boutons, titres), `#rrggbb`.
    pub accent: String,
    /// Deuxième couleur, pour les dégradés des boutons.
    pub accent2: String,
    /// Couleur du texte.
    pub text: String,
    /// Teinte du verre.
    pub glass_tint: String,
    /// Opacité du verre, en %.
    pub glass_opacity: u16,
    /// Flou derrière le verre, en px.
    pub blur: u16,
    /// Saturation des couleurs vues à travers le verre, en %.
    pub saturation: u16,
    /// Épaisseur de la bordure, en px.
    pub border_width: u16,
    /// Opacité de la bordure, en %.
    pub border_opacity: u16,
    /// Arrondi des coins, en px.
    pub radius: u16,
    /// Intensité de l'ombre, en %.
    pub shadow: u16,
    /// Reflet lumineux en haut des panneaux.
    pub shine: bool,
    /// Grain (texture), en %.
    pub grain: u16,
    pub font: Font,
    /// Taille du texte, en % de la taille normale.
    pub font_scale: u16,
    pub density: Density,
    pub wallpaper: WallpaperKind,
    /// Trois couleurs du fond (taches ou dégradé).
    pub wallpaper_colors: [String; 3],
    /// Flou du fond, en px.
    pub wallpaper_blur: u16,
    /// Assombrissement du fond, en %.
    pub wallpaper_dim: u16,
    /// Fond animé (désactivé de toute façon si le système demande moins
    /// d'animations).
    pub animate: bool,
}

/// Bornes de chaque réglage numérique, partagées avec l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Range {
    pub min: u16,
    pub max: u16,
}

/// Bornes de tous les réglages numériques.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Limits {
    pub glass_opacity: Range,
    pub blur: Range,
    pub saturation: Range,
    pub border_width: Range,
    pub border_opacity: Range,
    pub radius: Range,
    pub shadow: Range,
    pub grain: Range,
    pub font_scale: Range,
    pub wallpaper_blur: Range,
    pub wallpaper_dim: Range,
}

pub const LIMITS: Limits = Limits {
    glass_opacity: Range { min: 0, max: 100 },
    blur: Range { min: 0, max: 60 },
    saturation: Range { min: 50, max: 250 },
    border_width: Range { min: 0, max: 4 },
    border_opacity: Range { min: 0, max: 100 },
    radius: Range { min: 0, max: 40 },
    shadow: Range { min: 0, max: 100 },
    grain: Range { min: 0, max: 100 },
    font_scale: Range { min: 80, max: 140 },
    wallpaper_blur: Range { min: 0, max: 40 },
    wallpaper_dim: Range { min: 0, max: 80 },
};

impl Default for Appearance {
    fn default() -> Self {
        frost()
    }
}

impl Appearance {
    /// Copie valide : nombres ramenés dans leurs bornes, couleurs invalides
    /// remplacées par celles du thème par défaut.
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        let l = LIMITS;
        let clamp = |v: u16, r: Range| v.clamp(r.min, r.max);
        self.glass_opacity = clamp(self.glass_opacity, l.glass_opacity);
        self.blur = clamp(self.blur, l.blur);
        self.saturation = clamp(self.saturation, l.saturation);
        self.border_width = clamp(self.border_width, l.border_width);
        self.border_opacity = clamp(self.border_opacity, l.border_opacity);
        self.radius = clamp(self.radius, l.radius);
        self.shadow = clamp(self.shadow, l.shadow);
        self.grain = clamp(self.grain, l.grain);
        self.font_scale = clamp(self.font_scale, l.font_scale);
        self.wallpaper_blur = clamp(self.wallpaper_blur, l.wallpaper_blur);
        self.wallpaper_dim = clamp(self.wallpaper_dim, l.wallpaper_dim);

        fix_color(&mut self.accent, &d.accent);
        fix_color(&mut self.accent2, &d.accent2);
        fix_color(&mut self.text, &d.text);
        fix_color(&mut self.glass_tint, &d.glass_tint);
        for (color, default) in self.wallpaper_colors.iter_mut().zip(&d.wallpaper_colors) {
            fix_color(color, default);
        }

        let preset = self.preset.trim();
        self.preset = if preset_by_id(preset).is_some() {
            preset.to_owned()
        } else {
            d.preset
        };
        self
    }

    /// Code texte à copier pour partager ce thème (avec l'autre téléphone,
    /// par message…). L'image de fond n'en fait pas partie.
    pub fn to_code(&self) -> Result<String> {
        let mut shared = self.clone();
        // L'image reste sur cet appareil : l'autre reçoit le fond par défaut.
        if shared.wallpaper == WallpaperKind::Image {
            shared.wallpaper = WallpaperKind::Mesh;
        }
        let json =
            serde_json::to_string(&shared).map_err(|e| Error::InvalidTheme(e.to_string()))?;
        Ok(format!("{CODE_PREFIX}{json}"))
    }

    /// Lit un code produit par [`Appearance::to_code`] (le préfixe est
    /// facultatif, les espaces autour sont ignorés).
    pub fn from_code(code: &str) -> Result<Self> {
        let code = code.trim();
        let json = code.strip_prefix(CODE_PREFIX).unwrap_or(code).trim();
        if !json.starts_with('{') {
            return Err(Error::InvalidTheme(
                "ce n'est pas un code de thème Eve & Lucas".into(),
            ));
        }
        let appearance: Self =
            serde_json::from_str(json).map_err(|e| Error::InvalidTheme(e.to_string()))?;
        Ok(appearance.sanitized())
    }
}

/// `#rrggbb` (minuscules ou majuscules) ; tout le reste est refusé pour
/// qu'aucune valeur ne puisse injecter autre chose dans le CSS.
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn fix_color(color: &mut String, default: &str) {
    if is_hex_color(color) {
        color.make_ascii_lowercase();
    } else {
        *color = default.to_owned();
    }
}

/// Un préréglage proposé par l'appli.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub appearance: Appearance,
}

/// Copie dans `slot` la valeur de `key` si elle est lisible.
fn take<T: DeserializeOwned>(map: &Map<String, Value>, key: &str, slot: &mut T) {
    if let Some(value) = map.get(key) {
        if let Ok(parsed) = serde_json::from_value(value.clone()) {
            *slot = parsed;
        }
    }
}

/// Comme [`take`] pour un nombre, ramené dans les limites d'un `u16` (les
/// bornes de chaque réglage sont appliquées ensuite par `sanitized`).
fn take_number(map: &Map<String, Value>, key: &str, slot: &mut u16) {
    if let Some(n) = map.get(key).and_then(Value::as_f64) {
        if n.is_finite() {
            *slot = n.clamp(0.0, f64::from(u16::MAX)).round() as u16;
        }
    }
}

impl From<Value> for Appearance {
    fn from(value: Value) -> Self {
        let mut a = Appearance::default();
        let Value::Object(map) = value else {
            return a;
        };
        take(&map, "preset", &mut a.preset);
        take(&map, "accent", &mut a.accent);
        take(&map, "accent2", &mut a.accent2);
        take(&map, "text", &mut a.text);
        take(&map, "glass_tint", &mut a.glass_tint);
        take_number(&map, "glass_opacity", &mut a.glass_opacity);
        take_number(&map, "blur", &mut a.blur);
        take_number(&map, "saturation", &mut a.saturation);
        take_number(&map, "border_width", &mut a.border_width);
        take_number(&map, "border_opacity", &mut a.border_opacity);
        take_number(&map, "radius", &mut a.radius);
        take_number(&map, "shadow", &mut a.shadow);
        take(&map, "shine", &mut a.shine);
        take_number(&map, "grain", &mut a.grain);
        take(&map, "font", &mut a.font);
        take_number(&map, "font_scale", &mut a.font_scale);
        take(&map, "density", &mut a.density);
        take(&map, "wallpaper", &mut a.wallpaper);
        if let Some(Value::Array(colors)) = map.get("wallpaper_colors") {
            for (slot, value) in a.wallpaper_colors.iter_mut().zip(colors) {
                if let Some(color) = value.as_str() {
                    *slot = color.to_owned();
                }
            }
        }
        take_number(&map, "wallpaper_blur", &mut a.wallpaper_blur);
        take_number(&map, "wallpaper_dim", &mut a.wallpaper_dim);
        take(&map, "animate", &mut a.animate);
        a
    }
}

/// Un thème enregistré par l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTheme {
    pub id: String,
    pub name: String,
    pub appearance: Appearance,
}

/// Contenu du fichier d'apparence d'un appareil. Lecture tolérante : un thème
/// personnel illisible est ignoré sans faire perdre les autres.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(from = "Value")]
pub struct AppearanceSettings {
    pub current: Appearance,
    pub saved: Vec<SavedTheme>,
}

impl From<Value> for AppearanceSettings {
    fn from(value: Value) -> Self {
        let mut settings = AppearanceSettings::default();
        let Value::Object(mut map) = value else {
            return settings;
        };
        if let Some(current) = map.remove("current") {
            settings.current = Appearance::from(current);
        }
        if let Some(Value::Array(saved)) = map.remove("saved") {
            settings.saved = saved
                .into_iter()
                .filter_map(|theme| serde_json::from_value(theme).ok())
                .collect();
        }
        settings
    }
}

impl AppearanceSettings {
    /// Lit le fichier JSON. Les valeurs hors bornes ou illisibles sont
    /// corrigées ; seul un fichier qui n'est pas du JSON est une erreur
    /// (l'appli le met alors de côté).
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let mut settings: Self =
            serde_json::from_slice(bytes).map_err(|e| Error::InvalidTheme(e.to_string()))?;
        settings.current = settings.current.sanitized();
        let mut seen = std::collections::HashSet::new();
        settings
            .saved
            .retain(|t| !t.id.is_empty() && seen.insert(t.id.clone()));
        settings.saved.truncate(MAX_SAVED_THEMES);
        for theme in &mut settings.saved {
            theme.appearance = theme.appearance.clone().sanitized();
            theme.name = clean_name(&theme.name).unwrap_or_else(|_| "Sans nom".into());
        }
        Ok(settings)
    }

    pub fn to_json(&self) -> Result<Vec<u8>> {
        serde_json::to_vec_pretty(self).map_err(|e| Error::InvalidTheme(e.to_string()))
    }

    /// Remplace l'apparence actuelle et renvoie la version corrigée.
    pub fn set_current(&mut self, appearance: Appearance) -> Appearance {
        self.current = appearance.sanitized();
        self.current.clone()
    }

    /// Applique un préréglage de l'appli.
    pub fn apply_preset(&mut self, id: &str) -> Result<Appearance> {
        let preset = preset_by_id(id).ok_or_else(|| Error::PresetNotFound(id.to_owned()))?;
        self.current = preset.appearance;
        Ok(self.current.clone())
    }

    /// Enregistre l'apparence actuelle sous un nom. Un thème du même nom
    /// (sans tenir compte des majuscules) est remplacé.
    pub fn save_current_as(&mut self, name: &str) -> Result<SavedTheme> {
        let name = clean_name(name)?;
        let appearance = self.current.clone();
        if let Some(existing) = self
            .saved
            .iter_mut()
            .find(|t| t.name.to_lowercase() == name.to_lowercase())
        {
            existing.appearance = appearance;
            existing.name = name;
            return Ok(existing.clone());
        }
        if self.saved.len() >= MAX_SAVED_THEMES {
            return Err(Error::TooManyThemes(MAX_SAVED_THEMES));
        }
        let theme = SavedTheme {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            appearance,
        };
        self.saved.push(theme.clone());
        Ok(theme)
    }

    /// Applique un thème personnel.
    pub fn apply_saved(&mut self, id: &str) -> Result<Appearance> {
        let theme = self
            .saved
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Error::ThemeNotFound(id.to_owned()))?;
        self.current = theme.appearance.clone();
        Ok(self.current.clone())
    }

    pub fn delete_saved(&mut self, id: &str) -> Result<()> {
        let before = self.saved.len();
        self.saved.retain(|t| t.id != id);
        if self.saved.len() == before {
            return Err(Error::ThemeNotFound(id.to_owned()));
        }
        Ok(())
    }
}

fn clean_name(name: &str) -> Result<String> {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(Error::EmptyThemeName);
    }
    let name: String = name.chars().take(MAX_THEME_NAME).collect();
    Ok(name.trim_end().to_owned())
}

/// Vérifie une image de fond envoyée par l'interface : une data URL JPEG,
/// PNG ou WebP en base64, de taille raisonnable.
pub fn validate_wallpaper(data_url: &str) -> Result<()> {
    if data_url.len() > MAX_WALLPAPER_BYTES {
        return Err(Error::InvalidWallpaper(format!(
            "image trop lourde (maximum {} Mo)",
            MAX_WALLPAPER_BYTES / 1024 / 1024
        )));
    }
    let data = ["image/jpeg", "image/png", "image/webp"]
        .iter()
        .find_map(|mime| data_url.strip_prefix(&format!("data:{mime};base64,")))
        .ok_or_else(|| Error::InvalidWallpaper("format accepté : JPEG, PNG ou WebP".into()))?;
    let valid_base64 = !data.is_empty()
        && data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
    if !valid_base64 {
        return Err(Error::InvalidWallpaper(
            "contenu de l'image illisible".into(),
        ));
    }
    Ok(())
}

fn colors(a: &str, b: &str, c: &str) -> [String; 3] {
    [a.to_owned(), b.to_owned(), c.to_owned()]
}

/// Givre : verre clair sur pastels (thème par défaut).
fn frost() -> Appearance {
    Appearance {
        preset: "givre".into(),
        accent: "#5b6cff".into(),
        accent2: "#b16cff".into(),
        text: "#1f2333".into(),
        glass_tint: "#ffffff".into(),
        glass_opacity: 42,
        blur: 24,
        saturation: 180,
        border_width: 1,
        border_opacity: 55,
        radius: 20,
        shadow: 35,
        shine: true,
        grain: 0,
        font: Font::System,
        font_scale: 100,
        density: Density::Normal,
        wallpaper: WallpaperKind::Mesh,
        wallpaper_colors: colors("#a8c0ff", "#fbc2eb", "#c2f0e6"),
        wallpaper_blur: 0,
        wallpaper_dim: 0,
        animate: true,
    }
}

/// Tous les préréglages, dans l'ordre d'affichage.
pub fn presets() -> Vec<Preset> {
    let base = frost();
    vec![
        Preset {
            id: "givre",
            name: "Givre",
            description: "Verre clair et pastels, lumineux et doux",
            appearance: base.clone(),
        },
        Preset {
            id: "nuit",
            name: "Nuit étoilée",
            description: "Sombre et profond, bleu nuit et violet",
            appearance: Appearance {
                preset: "nuit".into(),
                accent: "#8ab4ff".into(),
                accent2: "#c49bff".into(),
                text: "#eef1ff".into(),
                glass_tint: "#141a33".into(),
                glass_opacity: 40,
                blur: 28,
                saturation: 150,
                border_opacity: 22,
                shadow: 60,
                grain: 12,
                wallpaper_colors: colors("#1b2a6b", "#4b1f6f", "#0b5872"),
                wallpaper_dim: 25,
                ..base.clone()
            },
        },
        Preset {
            id: "aurore",
            name: "Aurore boréale",
            description: "Vert et turquoise qui ondulent dans le noir",
            appearance: Appearance {
                preset: "aurore".into(),
                accent: "#4ff0b4".into(),
                accent2: "#4fc3ff".into(),
                text: "#e9fff7".into(),
                glass_tint: "#0a1f24".into(),
                glass_opacity: 30,
                blur: 32,
                saturation: 200,
                border_opacity: 25,
                radius: 24,
                shadow: 50,
                wallpaper_colors: colors("#0fbf8a", "#2a3bb7", "#7b2fbf"),
                wallpaper_dim: 45,
                ..base.clone()
            },
        },
        Preset {
            id: "coucher",
            name: "Coucher de soleil",
            description: "Orange, rose et violet, chaleureux",
            appearance: Appearance {
                preset: "coucher".into(),
                accent: "#ff7a45".into(),
                accent2: "#ff3d7f".into(),
                text: "#2a1320".into(),
                glass_tint: "#fff3ea".into(),
                glass_opacity: 38,
                blur: 22,
                saturation: 170,
                border_opacity: 50,
                radius: 26,
                shadow: 40,
                font: Font::Rounded,
                wallpaper_colors: colors("#ff9a56", "#ff4f8b", "#8e54e9"),
                ..base.clone()
            },
        },
        Preset {
            id: "lagon",
            name: "Lagon",
            description: "Turquoise et sable, frais comme la mer",
            appearance: Appearance {
                preset: "lagon".into(),
                accent: "#0088b8".into(),
                accent2: "#00b3a4".into(),
                text: "#0c2a33".into(),
                glass_tint: "#f2fdff".into(),
                glass_opacity: 45,
                blur: 20,
                saturation: 160,
                radius: 18,
                font: Font::Rounded,
                wallpaper_colors: colors("#4dd8e6", "#f9e6c4", "#2f8fd8"),
                ..base.clone()
            },
        },
        Preset {
            id: "romance",
            name: "Romance",
            description: "Rose poudré et bordeaux, police élégante",
            appearance: Appearance {
                preset: "romance".into(),
                accent: "#d6336c".into(),
                accent2: "#ff8fa3".into(),
                text: "#3a1020".into(),
                glass_tint: "#fff0f4".into(),
                glass_opacity: 48,
                blur: 26,
                saturation: 160,
                border_opacity: 60,
                radius: 22,
                shadow: 30,
                grain: 8,
                font: Font::Elegant,
                font_scale: 105,
                wallpaper_colors: colors("#ffb3c6", "#ffd6a5", "#e5989b"),
                ..base.clone()
            },
        },
        Preset {
            id: "minimal",
            name: "Minimal",
            description: "Sobre, presque opaque, coins discrets",
            appearance: Appearance {
                preset: "minimal".into(),
                accent: "#222222".into(),
                accent2: "#555555".into(),
                text: "#1a1a1a".into(),
                glass_tint: "#ffffff".into(),
                glass_opacity: 72,
                blur: 12,
                saturation: 100,
                border_opacity: 30,
                radius: 10,
                shadow: 15,
                shine: false,
                density: Density::Compact,
                wallpaper: WallpaperKind::Linear,
                wallpaper_colors: colors("#eceff3", "#dfe4ea", "#f7f7f7"),
                animate: false,
                ..base.clone()
            },
        },
        Preset {
            id: "neon",
            name: "Néon",
            description: "Noir profond, magenta et cyan électriques",
            appearance: Appearance {
                preset: "neon".into(),
                accent: "#ff2bd6".into(),
                accent2: "#00e5ff".into(),
                text: "#f4f0ff".into(),
                glass_tint: "#07050f".into(),
                glass_opacity: 45,
                blur: 18,
                saturation: 220,
                border_width: 2,
                border_opacity: 45,
                radius: 8,
                shadow: 80,
                shine: false,
                grain: 18,
                font: Font::Mono,
                wallpaper_colors: colors("#ff2bd6", "#00e5ff", "#3b0aff"),
                wallpaper_dim: 55,
                ..base
            },
        },
    ]
}

/// Un préréglage par son identifiant.
pub fn preset_by_id(id: &str) -> Option<Preset> {
    presets().into_iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_unique() {
        let all = presets();
        assert!(all.len() >= 6);
        for p in &all {
            assert_eq!(p.appearance.preset, p.id, "préréglage {}", p.id);
            assert_eq!(
                p.appearance.clone().sanitized(),
                p.appearance,
                "le préréglage {} doit déjà être valide",
                p.id
            );
            assert_eq!(all.iter().filter(|o| o.id == p.id).count(), 1);
        }
        assert_eq!(Appearance::default().preset, all[0].id);
    }

    #[test]
    fn presets_look_different() {
        let all = presets();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.appearance.wallpaper_colors, b.appearance.wallpaper_colors);
                assert_ne!(a.appearance.accent, b.appearance.accent);
            }
        }
    }

    #[test]
    fn sanitize_clamps_numbers_and_fixes_colors() {
        let a = Appearance {
            preset: "inconnu".into(),
            accent: "red; background: url(x)".into(),
            text: "#ABCDEF".into(),
            blur: 255,
            saturation: 9999,
            font_scale: 10,
            radius: 200,
            wallpaper_colors: colors("#123456", "nope", "#fff"),
            ..Appearance::default()
        }
        .sanitized();
        let d = Appearance::default();
        assert_eq!(a.preset, d.preset);
        assert_eq!(a.accent, d.accent);
        assert_eq!(a.text, "#abcdef");
        assert_eq!(a.blur, 60);
        assert_eq!(a.saturation, 250);
        assert_eq!(a.font_scale, 80);
        assert_eq!(a.radius, 40);
        assert_eq!(
            a.wallpaper_colors,
            [
                "#123456".to_owned(),
                d.wallpaper_colors[1].clone(),
                d.wallpaper_colors[2].clone()
            ]
        );
    }

    #[test]
    fn theme_code_roundtrip() {
        let neon = preset_by_id("neon").unwrap().appearance;
        let code = neon.to_code().unwrap();
        assert!(code.starts_with(CODE_PREFIX));
        assert_eq!(Appearance::from_code(&code).unwrap(), neon);
        // Sans le préfixe, avec des espaces autour.
        let json = format!("  {}\n", &code[CODE_PREFIX.len()..]);
        assert_eq!(Appearance::from_code(&json).unwrap(), neon);
    }

    #[test]
    fn theme_code_from_older_version_uses_defaults() {
        // Un code ne contenant que quelques champs (ancienne version).
        let a = Appearance::from_code(r##"{"accent":"#00ff00","blur":500}"##).unwrap();
        assert_eq!(a.accent, "#00ff00");
        assert_eq!(a.blur, 60);
        assert_eq!(a.radius, Appearance::default().radius);
    }

    #[test]
    fn bad_theme_codes_are_rejected() {
        for code in ["", "bonjour", "EVELUCAS-THEME:", "{pas du json"] {
            assert!(
                matches!(Appearance::from_code(code), Err(Error::InvalidTheme(_))),
                "{code:?}"
            );
        }
    }

    #[test]
    fn unreadable_fields_keep_their_default_without_losing_the_others() {
        // Valeurs d'une future version (police inconnue, nombre énorme ou
        // négatif, mauvais type) : seuls ces champs reprennent leur défaut.
        let a = Appearance::from_code(
            r##"{"accent":"#00ff00","font":"script","blur":70000,"radius":-5,
                "shadow":"beaucoup","density":"spacious","wallpaper_colors":["#111111",3]}"##,
        )
        .unwrap();
        let d = Appearance::default();
        assert_eq!(a.accent, "#00ff00");
        assert_eq!(a.font, d.font);
        assert_eq!(a.blur, 60);
        assert_eq!(a.radius, 0);
        assert_eq!(a.shadow, d.shadow);
        assert_eq!(a.density, Density::Spacious);
        assert_eq!(a.wallpaper_colors[0], "#111111");
        assert_eq!(a.wallpaper_colors[1], d.wallpaper_colors[1]);
    }

    #[test]
    fn one_bad_saved_theme_does_not_lose_the_others() {
        let mut s = AppearanceSettings::default();
        s.save_current_as("Bon").unwrap();
        let mut json: Value = serde_json::from_slice(&s.to_json().unwrap()).unwrap();
        json["saved"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"nom": "pas un thème"}));
        json["current"]["font"] = "inconnue".into();
        let reloaded = AppearanceSettings::from_json(json.to_string().as_bytes()).unwrap();
        assert_eq!(reloaded.saved.len(), 1);
        assert_eq!(reloaded.saved[0].name, "Bon");
        assert_eq!(reloaded.current.font, Font::System);
    }

    #[test]
    fn settings_file_roundtrip_and_repair() {
        let mut s = AppearanceSettings::default();
        s.apply_preset("lagon").unwrap();
        s.save_current_as("Mon lagon").unwrap();
        let bytes = s.to_json().unwrap();
        assert_eq!(AppearanceSettings::from_json(&bytes).unwrap(), s);

        // Valeurs hors bornes corrigées au chargement.
        let edited = String::from_utf8(bytes)
            .unwrap()
            .replace(&format!("\"blur\": {}", s.current.blur), "\"blur\": 250");
        let reloaded = AppearanceSettings::from_json(edited.as_bytes()).unwrap();
        assert_eq!(reloaded.current.blur, 60);

        assert!(AppearanceSettings::from_json(b"pas du json").is_err());
        // Fichier vide d'une future version : valeurs par défaut.
        assert_eq!(
            AppearanceSettings::from_json(b"{}").unwrap(),
            AppearanceSettings::default()
        );
    }

    #[test]
    fn saved_themes() {
        let mut s = AppearanceSettings::default();
        assert!(matches!(
            s.save_current_as("   "),
            Err(Error::EmptyThemeName)
        ));

        s.apply_preset("nuit").unwrap();
        let nuit = s.save_current_as("  Le   soir ").unwrap();
        assert_eq!(nuit.name, "Le soir");

        // Même nom : le thème est remplacé, pas dupliqué.
        s.apply_preset("aurore").unwrap();
        let again = s.save_current_as("le SOIR").unwrap();
        assert_eq!(again.id, nuit.id);
        assert_eq!(s.saved.len(), 1);
        assert_eq!(s.saved[0].appearance.preset, "aurore");

        s.apply_preset("givre").unwrap();
        assert_eq!(s.apply_saved(&nuit.id).unwrap().preset, "aurore");

        let long = "x".repeat(100);
        assert_eq!(
            s.save_current_as(&long).unwrap().name.chars().count(),
            MAX_THEME_NAME
        );

        s.delete_saved(&nuit.id).unwrap();
        assert!(matches!(
            s.delete_saved(&nuit.id),
            Err(Error::ThemeNotFound(_))
        ));
        assert!(matches!(
            s.apply_saved(&nuit.id),
            Err(Error::ThemeNotFound(_))
        ));
        assert!(matches!(
            s.apply_preset("inconnu"),
            Err(Error::PresetNotFound(_))
        ));
    }

    #[test]
    fn saved_themes_are_limited() {
        let mut s = AppearanceSettings::default();
        for i in 0..MAX_SAVED_THEMES {
            s.save_current_as(&format!("Thème {i}")).unwrap();
        }
        assert!(matches!(
            s.save_current_as("Un de trop"),
            Err(Error::TooManyThemes(_))
        ));
        // Remplacer un thème existant reste possible.
        assert!(s.save_current_as("Thème 3").is_ok());
    }

    #[test]
    fn set_current_sanitizes() {
        let mut s = AppearanceSettings::default();
        let out = s.set_current(Appearance {
            grain: 200,
            ..Appearance::default()
        });
        assert_eq!(out.grain, 100);
        assert_eq!(s.current, out);
    }

    #[test]
    fn wallpaper_validation() {
        assert!(validate_wallpaper("data:image/jpeg;base64,/9j/4AAQSkZJRg==").is_ok());
        assert!(validate_wallpaper("data:image/png;base64,iVBORw0KGgo=").is_ok());
        for bad in [
            "",
            "data:image/jpeg;base64,",
            "data:image/svg+xml;base64,PHN2Zz4=",
            "data:text/html;base64,PGgxPg==",
            "data:image/png;base64,abc\"); background: red",
            "https://exemple.fr/image.jpg",
        ] {
            assert!(
                matches!(validate_wallpaper(bad), Err(Error::InvalidWallpaper(_))),
                "{bad:?}"
            );
        }
        let huge = format!("data:image/png;base64,{}", "A".repeat(MAX_WALLPAPER_BYTES));
        assert!(validate_wallpaper(&huge).is_err());
    }

    #[test]
    fn shared_code_never_asks_for_the_image() {
        let mine = Appearance {
            wallpaper: WallpaperKind::Image,
            ..Appearance::default()
        };
        let received = Appearance::from_code(&mine.to_code().unwrap()).unwrap();
        assert_eq!(received.wallpaper, WallpaperKind::Mesh);
    }

    #[test]
    fn theme_names_are_trimmed_after_truncation() {
        let name = format!("{} y", "a".repeat(MAX_THEME_NAME - 1));
        assert_eq!(clean_name(&name).unwrap(), "a".repeat(MAX_THEME_NAME - 1));
    }

    #[test]
    fn duplicate_theme_ids_are_dropped_on_load() {
        let theme = SavedTheme {
            id: "t1".into(),
            name: "Un".into(),
            appearance: Appearance::default(),
        };
        let settings = AppearanceSettings {
            saved: vec![
                theme.clone(),
                SavedTheme {
                    name: "Deux".into(),
                    ..theme
                },
            ],
            ..AppearanceSettings::default()
        };
        let loaded = AppearanceSettings::from_json(&settings.to_json().unwrap()).unwrap();
        assert_eq!(loaded.saved.len(), 1);
        assert_eq!(loaded.saved[0].name, "Un");
    }
}
