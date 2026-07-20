//! Develop settings: the in-memory form of `settings_json` (`docs/pipeline.md` §3.2).
//!
//! A [`Settings`] value is always a complete, self-contained develop state —
//! never a delta. An omitted parameter takes its neutral value, frozen per
//! schema version: `{}` with `schema: 1` always produces the neutral rendering
//! of schema 1.
//!
//! Documents written by a newer schema are read losslessly: unknown fields are
//! preserved verbatim and re-serialized as-is, so an old engine never destroys
//! the work of a newer one (`docs/pipeline.md` §3.4).

use serde::{Deserialize, Serialize};

use crate::error::{LeylineError, Result};

/// Most recent settings format version this engine knows how to write.
pub const CURRENT_SCHEMA: u32 = 1;

/// Most recent process (rendering) version this engine implements.
/// Process 2 (ADR 0013) renders like process 1 with the sRGB transfer
/// functions computed by lookup table. Process 3 (ADR 0016) additionally
/// renders `lens_correction` as a Lensfun-backed geometric undistortion.
pub const CURRENT_PROCESS: u32 = 3;

/// White balance override, in physical units.
///
/// Neutral state is the *absence* of an override (`None` in [`Settings`]):
/// the as-shot white balance of the camera is used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WhiteBalance {
    /// Color temperature in Kelvin.
    pub temperature: u32,
    /// Green–magenta tint, unitless slider in [-100, +100], 0 = neutral.
    pub tint: i32,
}

impl Default for WhiteBalance {
    fn default() -> Self {
        Self {
            temperature: 6500,
            tint: 0,
        }
    }
}

/// Lens correction step. Neutral: disabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LensCorrection {
    /// Whether the correction is applied at all.
    pub enabled: bool,
    /// Profile selection; `"auto"` matches the lens from metadata.
    pub profile: String,
}

impl Default for LensCorrection {
    fn default() -> Self {
        Self {
            enabled: false,
            profile: "auto".to_owned(),
        }
    }
}

/// Noise reduction strengths, unitless sliders in [0, 100]. Neutral: 0.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseReduction {
    /// Luminance noise reduction strength.
    pub luminance: i32,
    /// Color noise reduction strength.
    pub color: i32,
}

/// Sharpening step. Neutral: amount 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sharpening {
    /// Strength, unitless slider in [0, 100], 0 = no sharpening.
    pub amount: i32,
    /// Radius in pixels, strictly positive.
    pub radius: f64,
}

impl Default for Sharpening {
    fn default() -> Self {
        Self {
            amount: 0,
            radius: 1.0,
        }
    }
}

/// Crop rectangle in normalized [0, 1] coordinates, relative to the image
/// *after* rotation. Neutral state is the absence of a crop (`None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    /// Left edge, in [0, 1].
    pub x: f64,
    /// Top edge, in [0, 1].
    pub y: f64,
    /// Width, in (0, 1].
    pub width: f64,
    /// Height, in (0, 1].
    pub height: f64,
}

/// Complete develop state of one revision (`docs/pipeline.md` §3.2, schema 1).
///
/// [`Settings::default`] is the neutral state of schema 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Version of the settings *format* (JSON structure).
    pub schema: u32,
    /// Version of the *rendering* (algorithms producing the pixels).
    pub process: u32,

    /// White balance override; `None` = as-shot (neutral).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white_balance: Option<WhiteBalance>,
    /// Exposure compensation in EV. Neutral: 0.
    pub exposure: f64,
    /// Contrast, slider in [-100, +100]. Neutral: 0.
    pub contrast: i32,
    /// Highlights recovery, slider in [-100, +100]. Neutral: 0.
    pub highlights: i32,
    /// Shadows lift, slider in [-100, +100]. Neutral: 0.
    pub shadows: i32,
    /// White point, slider in [-100, +100]. Neutral: 0.
    pub whites: i32,
    /// Black point, slider in [-100, +100]. Neutral: 0.
    pub blacks: i32,
    /// Vibrance, slider in [-100, +100]. Neutral: 0.
    pub vibrance: i32,
    /// Saturation, slider in [-100, +100]. Neutral: 0.
    pub saturation: i32,

    /// Lens correction step.
    pub lens_correction: LensCorrection,
    /// Noise reduction step.
    pub noise_reduction: NoiseReduction,
    /// Sharpening step.
    pub sharpening: Sharpening,

    /// Rotation in degrees, clockwise. Neutral: 0.
    pub rotation: f64,
    /// Crop rectangle; `None` = full frame (neutral).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<Crop>,

    /// Fields from schema versions this engine does not know, preserved
    /// verbatim for lossless round-tripping.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: CURRENT_SCHEMA,
            process: CURRENT_PROCESS,
            white_balance: None,
            exposure: 0.0,
            contrast: 0,
            highlights: 0,
            shadows: 0,
            whites: 0,
            blacks: 0,
            vibrance: 0,
            saturation: 0,
            lens_correction: LensCorrection::default(),
            noise_reduction: NoiseReduction::default(),
            sharpening: Sharpening::default(),
            rotation: 0.0,
            crop: None,
            extra: serde_json::Map::new(),
        }
    }
}

impl Settings {
    /// Parses a `settings_json` document.
    ///
    /// Documents from newer schemas parse successfully; their unknown fields
    /// land in [`Settings::extra`]. Editing such a document is the engine's
    /// responsibility to refuse.
    pub fn parse(json: &str) -> Result<Settings> {
        serde_json::from_str(json).map_err(|e| LeylineError::InvalidSettings(e.to_string()))
    }

    /// Serializes the complete state back to a `settings_json` document.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("settings serialization cannot fail")
    }

    /// Validates value ranges for schema 1.
    ///
    /// Only meaningful before *writing* a schema-1 revision; documents from
    /// newer schemas are not covered by these rules.
    pub fn validate(&self) -> Result<()> {
        fn slider(name: &str, value: i32, min: i32, max: i32) -> Result<()> {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(LeylineError::InvalidSettings(format!(
                    "{name} must be in [{min}, {max}], got {value}"
                )))
            }
        }
        fn finite(name: &str, value: f64) -> Result<()> {
            if value.is_finite() {
                Ok(())
            } else {
                Err(LeylineError::InvalidSettings(format!(
                    "{name} must be finite, got {value}"
                )))
            }
        }

        finite("exposure", self.exposure)?;
        finite("rotation", self.rotation)?;
        slider("contrast", self.contrast, -100, 100)?;
        slider("highlights", self.highlights, -100, 100)?;
        slider("shadows", self.shadows, -100, 100)?;
        slider("whites", self.whites, -100, 100)?;
        slider("blacks", self.blacks, -100, 100)?;
        slider("vibrance", self.vibrance, -100, 100)?;
        slider("saturation", self.saturation, -100, 100)?;
        slider(
            "noise_reduction.luminance",
            self.noise_reduction.luminance,
            0,
            100,
        )?;
        slider("noise_reduction.color", self.noise_reduction.color, 0, 100)?;
        slider("sharpening.amount", self.sharpening.amount, 0, 100)?;
        finite("sharpening.radius", self.sharpening.radius)?;
        if self.sharpening.radius <= 0.0 {
            return Err(LeylineError::InvalidSettings(format!(
                "sharpening.radius must be strictly positive, got {}",
                self.sharpening.radius
            )));
        }
        if let Some(wb) = &self.white_balance {
            slider("white_balance.tint", wb.tint, -100, 100)?;
            if wb.temperature == 0 {
                return Err(LeylineError::InvalidSettings(
                    "white_balance.temperature must be strictly positive".to_owned(),
                ));
            }
        }
        if let Some(crop) = &self.crop {
            for (name, value) in [("crop.x", crop.x), ("crop.y", crop.y)] {
                if !(0.0..=1.0).contains(&value) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in [0, 1], got {value}"
                    )));
                }
            }
            for (name, value) in [("crop.width", crop.width), ("crop.height", crop.height)] {
                if !(value > 0.0 && value <= 1.0) {
                    return Err(LeylineError::InvalidSettings(format!(
                        "{name} must be in (0, 1], got {value}"
                    )));
                }
            }
            if crop.x + crop.width > 1.0 || crop.y + crop.height > 1.0 {
                return Err(LeylineError::InvalidSettings(
                    "crop rectangle exceeds the image bounds".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// One category of develop settings a preset can capture (`docs/presets.md`
/// §3.1). Atomic: including a group captures — or applies — all of its
/// fields together, never a single field of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsGroup {
    /// [`Settings::white_balance`].
    WhiteBalance,
    /// [`Settings::exposure`], `contrast`, `highlights`, `shadows`, `whites`, `blacks`.
    Tone,
    /// [`Settings::vibrance`], `saturation`.
    Presence,
    /// [`Settings::lens_correction`].
    LensCorrection,
    /// [`Settings::noise_reduction`], `sharpening`.
    Detail,
    /// [`Settings::rotation`], `crop`. Never included by default when a
    /// preset is created (`docs/presets.md` §3.1): geometry is a per-photo
    /// judgment, not a reproducible style.
    Geometry,
}

/// A named, partial jeu of develop settings (`docs/presets.md` §3.2,
/// `preset_json`), unlike [`Settings`] which is always complete.
///
/// A field is `Some` if and only if its [`SettingsGroup`] is in `groups` —
/// that list is the source of truth for what the preset touches; an absent
/// field means "leave untouched", never "neutral value" (the opposite rule
/// from [`Settings`], `docs/presets.md` §3.2). No `process` field: a preset
/// never fixes a rendering version, only values.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PresetSettings {
    /// Version of the settings *format* this preset's fields use — the same
    /// numbering as [`Settings::schema`], not an independent space.
    pub schema: u32,
    /// The categories this preset touches.
    pub groups: Vec<SettingsGroup>,

    /// `Some(None)` = included, reset to as-shot; `Some(Some(wb))` = included
    /// with an override; `None` = category not included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub white_balance: Option<Option<WhiteBalance>>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f64>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadows: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whites: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Tone`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blacks: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrance: Option<i32>,
    /// Present when `groups` includes [`SettingsGroup::Presence`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<i32>,

    /// Present when `groups` includes [`SettingsGroup::LensCorrection`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_correction: Option<LensCorrection>,
    /// Present when `groups` includes [`SettingsGroup::Detail`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noise_reduction: Option<NoiseReduction>,
    /// Present when `groups` includes [`SettingsGroup::Detail`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sharpening: Option<Sharpening>,

    /// Present when `groups` includes [`SettingsGroup::Geometry`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    /// `Some(None)` = included, cleared to full frame; `Some(Some(c))` =
    /// included with a crop; `None` = category not included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crop: Option<Option<Crop>>,
}

impl PresetSettings {
    /// Captures the fields of `groups` from a complete develop state
    /// (`docs/presets.md` §3.1 — the create-a-preset step).
    pub fn capture(settings: &Settings, groups: &[SettingsGroup]) -> PresetSettings {
        let mut preset = PresetSettings {
            schema: settings.schema,
            groups: groups.to_vec(),
            ..PresetSettings::default()
        };
        for group in groups {
            match group {
                SettingsGroup::WhiteBalance => {
                    preset.white_balance = Some(settings.white_balance.clone());
                }
                SettingsGroup::Tone => {
                    preset.exposure = Some(settings.exposure);
                    preset.contrast = Some(settings.contrast);
                    preset.highlights = Some(settings.highlights);
                    preset.shadows = Some(settings.shadows);
                    preset.whites = Some(settings.whites);
                    preset.blacks = Some(settings.blacks);
                }
                SettingsGroup::Presence => {
                    preset.vibrance = Some(settings.vibrance);
                    preset.saturation = Some(settings.saturation);
                }
                SettingsGroup::LensCorrection => {
                    preset.lens_correction = Some(settings.lens_correction.clone());
                }
                SettingsGroup::Detail => {
                    preset.noise_reduction = Some(settings.noise_reduction.clone());
                    preset.sharpening = Some(settings.sharpening.clone());
                }
                SettingsGroup::Geometry => {
                    preset.rotation = Some(settings.rotation);
                    preset.crop = Some(settings.crop.clone());
                }
            }
        }
        preset
    }

    /// Parses a `preset_json` document.
    pub fn parse(json: &str) -> Result<PresetSettings> {
        serde_json::from_str(json).map_err(|e| LeylineError::InvalidSettings(e.to_string()))
    }

    /// Serializes back to a `preset_json` document.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("preset serialization cannot fail")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact example of `docs/pipeline.md` §3.2.
    const SPEC_EXAMPLE: &str = r#"{
        "schema": 1,
        "process": 1,

        "white_balance": { "temperature": 5400, "tint": 4 },
        "exposure": 0.35,
        "contrast": 12,
        "highlights": -40,
        "shadows": 25,
        "whites": 0,
        "blacks": -5,
        "vibrance": 18,
        "saturation": 0,

        "lens_correction": { "enabled": true, "profile": "auto" },
        "noise_reduction": { "luminance": 15, "color": 25 },
        "sharpening": { "amount": 40, "radius": 1.0 },

        "rotation": 0.0,
        "crop": { "x": 0.1, "y": 0.2, "width": 0.8, "height": 0.7 }
    }"#;

    #[test]
    fn parses_the_spec_example() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(s.schema, 1);
        assert_eq!(s.process, 1);
        assert_eq!(
            s.white_balance,
            Some(WhiteBalance {
                temperature: 5400,
                tint: 4
            })
        );
        assert_eq!(s.exposure, 0.35);
        assert_eq!(s.contrast, 12);
        assert_eq!(s.highlights, -40);
        assert_eq!(s.noise_reduction.color, 25);
        assert_eq!(s.sharpening.amount, 40);
        assert_eq!(
            s.crop,
            Some(Crop {
                x: 0.1,
                y: 0.2,
                width: 0.8,
                height: 0.7
            })
        );
        assert!(s.extra.is_empty());
        s.validate().unwrap();
    }

    #[test]
    fn round_trips_losslessly() {
        let parsed = Settings::parse(SPEC_EXAMPLE).unwrap();
        let reparsed = Settings::parse(&parsed.to_json()).unwrap();
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn omitted_values_are_neutral() {
        let s = Settings::parse(r#"{ "schema": 1, "process": 1 }"#).unwrap();
        assert_eq!(
            s,
            Settings {
                schema: 1,
                process: 1,
                ..Settings::default()
            }
        );
        assert_eq!(Settings::parse("{}").unwrap(), Settings::default());
    }

    #[test]
    fn preserves_unknown_fields_verbatim() {
        let json = r#"{ "schema": 2, "process": 1, "clarity": 30, "exposure": 1.5 }"#;
        let s = Settings::parse(json).unwrap();
        assert_eq!(s.schema, 2);
        assert_eq!(s.extra.get("clarity"), Some(&serde_json::json!(30)));

        let round_tripped: serde_json::Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(round_tripped["clarity"], serde_json::json!(30));
        assert_eq!(round_tripped["exposure"], serde_json::json!(1.5));
    }

    #[test]
    fn rejects_out_of_range_values() {
        let mut s = Settings {
            contrast: 150,
            ..Settings::default()
        };
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));

        s.contrast = 0;
        s.crop = Some(Crop {
            x: 0.5,
            y: 0.0,
            width: 0.8,
            height: 1.0,
        });
        assert!(matches!(
            s.validate(),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(matches!(
            Settings::parse("{ not json"),
            Err(LeylineError::InvalidSettings(_))
        ));
    }

    #[test]
    fn preset_captures_only_the_requested_groups() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::Tone, SettingsGroup::Presence]);
        assert_eq!(preset.schema, 1);
        assert_eq!(preset.exposure, Some(0.35));
        assert_eq!(preset.contrast, Some(12));
        assert_eq!(preset.vibrance, Some(18));
        assert_eq!(preset.saturation, Some(0));
        // Not requested: absent, not neutral.
        assert_eq!(preset.white_balance, None);
        assert_eq!(preset.lens_correction, None);
        assert_eq!(preset.rotation, None);
        assert_eq!(preset.crop, None);
    }

    #[test]
    fn preset_json_omits_fields_of_excluded_groups() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::WhiteBalance]);
        let json = preset.to_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("white_balance").is_some());
        assert!(value.get("exposure").is_none());
        assert!(value.get("crop").is_none());
    }

    #[test]
    fn preset_round_trips_losslessly() {
        let s = Settings::parse(SPEC_EXAMPLE).unwrap();
        let preset = PresetSettings::capture(&s, &[SettingsGroup::Detail, SettingsGroup::Geometry]);
        let reparsed = PresetSettings::parse(&preset.to_json()).unwrap();
        assert_eq!(preset, reparsed);
    }
}
