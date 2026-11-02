//! Maps develop sliders to engine parameter updates (`docs/pipeline.md`).
//!
//! Pure functions kept free of any Slint type so every rule is
//! unit-testable: the UI forwards the slider name and released value, and
//! the session applies whatever comes back.

use leyline_sdk::{
    ColorGrading, Crop, CurvePoint, Demosaic, GuideLine, HighlightReconstruction, HslBand,
    LensCorrection, NoiseReduction, Param, Point, RedEye, ReshapePoint, Settings, Sharpening,
    SpotRemoval, ToneCurve, Value,
};

// The curve editors and the retouching marks live beside this file,
// re-exported so `develop::…` paths are unchanged.
mod curves;
mod retouch;

pub use curves::*;
pub use retouch::*;

/// Decodes a slider release into an engine parameter update.
///
/// `current` supplies the multi-field steps the slider merges into: the
/// white balance pair (starting from the engine default when the photo is
/// still as-shot), the noise reduction and sharpening pairs, and the crop
/// rectangle — in percent of the frame, clamped so it stays inside the
/// frame; a crop grown back to the full frame clears the override.
pub fn action(slider: &str, value: f64, current: &Settings) -> Option<(Param, Value)> {
    if let Some(rest) = slider.strip_prefix("crop-") {
        return crop_action(rest, value, current.crop.clone())
            .map(|next| (Param::Crop, Value::Crop(next)));
    }
    let int = Value::Int(value.round() as i32);
    Some(match slider {
        "exposure" => (Param::Exposure, Value::Float(value)),
        "rotation" => (Param::Rotation, Value::Float(value)),
        // The two sliders are one tool, so each merges into the other's
        // current value — the same pattern white balance and noise reduction
        // follow. Both back at zero clears the override entirely, which is
        // what keeps a neutral revision free of the field (ADR 0052 §1).
        "perspective-vertical" | "perspective-horizontal" => {
            let mut perspective = current.perspective.unwrap_or_default();
            if slider == "perspective-vertical" {
                perspective.vertical = value.round() as i32;
            } else {
                perspective.horizontal = value.round() as i32;
            }
            let perspective =
                (perspective.vertical != 0 || perspective.horizontal != 0).then_some(perspective);
            (Param::Perspective, Value::Perspective(perspective))
        }
        // Levels (ADR 0164): five sliders, one stage value. The interface
        // keeps the black point below the white point by clamping against
        // the other one, so a slider never asks for what `validate` refuses.
        "levels-input-black"
        | "levels-input-white"
        | "levels-gamma"
        | "levels-output-black"
        | "levels-output-white" => {
            let mut levels = current.levels;
            let v = value.round() as i32;
            match slider {
                "levels-input-black" => levels.input_black = v.clamp(0, levels.input_white - 1),
                "levels-input-white" => levels.input_white = v.clamp(levels.input_black + 1, 255),
                "levels-gamma" => levels.gamma = (value * 100.0).round() / 100.0,
                "levels-output-black" => levels.output_black = v.clamp(0, levels.output_white),
                _ => levels.output_white = v.clamp(levels.output_black, 255),
            }
            (Param::Levels, Value::Levels(levels))
        }
        // The parametric curve's four regions (ADR 0137). Read-modify-write
        // on the whole struct, like `perspective` above and for the same
        // reason: the stage takes one value, so a slider that moved only its
        // own field would have to be reassembled somewhere else.
        "parametric-shadows"
        | "parametric-darks"
        | "parametric-lights"
        | "parametric-highlights" => {
            let mut curve = current.parametric_curve;
            let v = value.round() as i32;
            match slider {
                "parametric-shadows" => curve.shadows = v,
                "parametric-darks" => curve.darks = v,
                "parametric-lights" => curve.lights = v,
                _ => curve.highlights = v,
            }
            (Param::ParametricCurve, Value::ParametricCurve(curve))
        }
        "contrast" => (Param::Contrast, int),
        "highlights" => (Param::Highlights, int),
        "shadows" => (Param::Shadows, int),
        "whites" => (Param::Whites, int),
        "blacks" => (Param::Blacks, int),
        "clarity" => (Param::Clarity, int),
        "texture" => (Param::Texture, int),
        "dehaze" => (Param::Dehaze, int),
        "moire" => (Param::Moire, int),
        "highlight-rolloff" => (Param::HighlightRolloff, int),
        "vibrance" => (Param::Vibrance, int),
        "saturation" => (Param::Saturation, int),
        // Toggling only ever flips `enabled` on a profile the user has
        // already chosen (ADR 0035): the path and checksum come from the
        // engine's import, never from this pure mapping, so a toggle with
        // nothing referenced is a no-op rather than an invented reference.
        "camera-profile" => {
            let mut profile = current.camera_profile.clone()?;
            profile.enabled = value != 0.0;
            (Param::CameraProfile, Value::CameraProfile(Some(profile)))
        }
        // Like `camera-profile`: a toggle only ever flips `enabled` on a look
        // the user already chose, and the strength only moves a reference that
        // exists (ADR 0053 §1).
        "lut" => {
            let mut lut = current.lut.clone()?;
            lut.enabled = value != 0.0;
            (Param::Lut, Value::Lut(Some(lut)))
        }
        "lut-strength" => {
            let mut lut = current.lut.clone()?;
            lut.strength = value.round() as i32;
            (Param::Lut, Value::Lut(Some(lut)))
        }
        // A toggle and nothing else (ADR 0088 §5): it does not zero the
        // saturation, does not touch the mixer, does not "apply a look".
        // Pressing it twice gives the photo back exactly as it was.
        "monochrome" => (Param::Monochrome, Value::Bool(value != 0.0)),
        // The measured coefficients survive the toggle: they are not
        // Lensfun's, and switching the profile correction off is not a
        // reason to lose them (ADR 0111 §5).
        "lens-correction" => (
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                enabled: value != 0.0,
                ..current.lens_correction.clone()
            }),
        ),
        "lens-tca-red" => (
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                tca_red: value,
                ..current.lens_correction.clone()
            }),
        ),
        "defringe-purple" => (
            Param::Defringe,
            Value::Defringe(leyline_sdk::Defringe {
                purple: value.round() as i32,
                ..current.defringe
            }),
        ),
        "defringe-green" => (
            Param::Defringe,
            Value::Defringe(leyline_sdk::Defringe {
                green: value.round() as i32,
                ..current.defringe
            }),
        ),
        "lens-tca-blue" => (
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                tca_blue: value,
                ..current.lens_correction.clone()
            }),
        ),
        "wb-temp" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.temperature = value.round() as u32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "wb-tint" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.tint = value.round() as i32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "nr-luminance" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "nr-color" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                color: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "sharpen-amount" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: value.round() as i32,
                ..current.sharpening.clone()
            }),
        ),
        "sharpen-masking" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                masking: value.round() as i32,
                ..current.sharpening.clone()
            }),
        ),
        "sharpen-radius" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                radius: value.max(0.1),
                ..current.sharpening.clone()
            }),
        ),
        // Each slider merges into the other three, the pattern white
        // balance and noise reduction already follow: the engine commits a
        // vignette whole because a shape and a strength are one tool
        // (ADR 0090 §2).
        "vignette-amount" | "vignette-midpoint" | "vignette-roundness" | "vignette-feather" => {
            let mut vignette = current.vignette;
            let v = value.round() as i32;
            match slider {
                "vignette-amount" => vignette.amount = v,
                "vignette-midpoint" => vignette.midpoint = v,
                "vignette-roundness" => vignette.roundness = v,
                _ => vignette.feather = v,
            }
            (Param::Vignette, Value::Vignette(vignette))
        }
        "grain-amount" | "grain-size" | "grain-roughness" | "grain-color" => {
            let mut grain = current.grain;
            let v = value.round() as i32;
            match slider {
                "grain-amount" => grain.amount = v,
                "grain-size" => grain.size = v,
                "grain-roughness" => grain.roughness = v,
                _ => grain.color = v,
            }
            (Param::Grain, Value::Grain(grain))
        }
        "color-grading-balance" => (
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                balance: value.round() as i32,
                ..current.color_grading
            }),
        ),
        "color-grading-blending" => (
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                blending: value.round() as i32,
                ..current.color_grading
            }),
        ),
        _ => return None,
    })
}

/// Decodes the highlight-reconstruction picker (ADR 0050): `mode` is
/// `"clip"`/`"blend"`/`"rebuild"`, the three the decoder can be asked for.
/// An unknown name is no update rather than a guess.
///
/// Unlike every other control of the panel this one carries a name, not a
/// number: the modes are not points on a scale — `rebuild` is not "more
/// blend" — so a slider would invite an interpolation that does not exist.
pub fn highlight_reconstruction_action(mode: &str) -> Option<(Param, Value)> {
    let mode = match mode {
        "clip" => HighlightReconstruction::Clip,
        "blend" => HighlightReconstruction::Blend,
        "rebuild" => HighlightReconstruction::Rebuild,
        _ => return None,
    };
    Some((
        Param::HighlightReconstruction,
        Value::HighlightReconstruction(mode),
    ))
}

/// Decodes the demosaic picker (ADR 0061): `"ahd"`, `"vng"`, `"dcb"` or
/// `"dht"`. An unknown name is no update rather than a guess.
///
/// A name and not a number, for the same reason as the mode above: the
/// algorithms are not points on a scale, and a slider would invite an
/// interpolation between them that does not exist.
pub fn demosaic_action(algorithm: &str) -> Option<(Param, Value)> {
    let algorithm = match algorithm {
        "ahd" => Demosaic::Ahd,
        "vng" => Demosaic::Vng,
        "dcb" => Demosaic::Dcb,
        "dht" => Demosaic::Dht,
        _ => return None,
    };
    Some((Param::Demosaic, Value::Demosaic(algorithm)))
}

/// Decodes one HSL mixer band's slider release (ADR 0031). `index` is the
/// band's position in `current.hsl` (fixed order: red, orange, yellow,
/// green, aqua, blue, purple, magenta); `field` is `"hue"`/`"saturation"`/
/// `"luminance"`. Reads the band's other two fields from `current` so
/// moving one slider never resets its siblings — the same merge pattern
/// `action`'s `wb-temp`/`nr-luminance` cases use.
pub fn hsl_band_action(
    index: usize,
    field: &str,
    value: f64,
    current: &Settings,
) -> Option<(Param, Value)> {
    let mut band: HslBand = *current.hsl.get(index)?;
    match field {
        "hue" => band.hue = value.round() as i32,
        "saturation" => band.saturation = value.round() as i32,
        "luminance" => band.luminance = value.round() as i32,
        _ => return None,
    }
    Some((Param::HslBand(index), Value::HslBand(band)))
}

/// Decodes one color grading zone's slider release (ADR 0031). `zone` is
/// `"shadows"`/`"midtones"`/`"highlights"`, `field` is `"hue"`/
/// `"saturation"`/`"luminance"`. `Param::ColorGrading` commits the whole
/// struct as one tool, so this merges into `current.color_grading` rather
/// than replacing it, same reasoning as [`hsl_band_action`].
pub fn color_grading_zone_action(
    zone: &str,
    field: &str,
    value: f64,
    current: &Settings,
) -> Option<(Param, Value)> {
    let mut grading: ColorGrading = current.color_grading;
    let target = match zone {
        "shadows" => &mut grading.shadows,
        "midtones" => &mut grading.midtones,
        "highlights" => &mut grading.highlights,
        _ => return None,
    };
    match field {
        "hue" => target.hue = value.round() as i32,
        "saturation" => target.saturation = value.round() as i32,
        "luminance" => target.luminance = value.round() as i32,
        _ => return None,
    }
    Some((Param::ColorGrading, Value::ColorGrading(grading)))
}

/// Decodes a mouse drag over the develop preview into a crop update.
///
/// The preview is letterboxed inside its viewport (`image-fit: contain`)
/// and already shows the current crop, so the dragged rectangle selects a
/// sub-rectangle of the *displayed* frame; the returned crop is that
/// selection composed with `current`, back in full-frame coordinates.
/// `view` and `image` are the viewport and preview sizes in their own
/// pixels. Drags covering less than 1 % of the frame in either direction
/// are ignored, as is a degenerate viewport or preview.
pub fn drag_crop(
    press: (f64, f64),
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    current: &Option<Crop>,
) -> Option<(Param, Value)> {
    let a = letterbox_unit(press, view, image)?;
    let b = letterbox_unit(release, view, image)?;
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (width, height) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    if width < 0.01 || height < 0.01 {
        return None;
    }
    let base = current.clone().unwrap_or(FULL_FRAME);
    let crop = Crop {
        x: base.x + x0 * base.width,
        y: base.y + y0 * base.height,
        width: width * base.width,
        height: height * base.height,
    };
    (crop != FULL_FRAME).then_some((Param::Crop, Value::Crop(Some(crop))))
}

/// Maps a point over the letterboxed develop preview (`image-fit: contain`)
/// into `[0, 1]` unit coordinates of the *image itself* — the referential
/// [`Crop`] and [`Point`] (ADR 0026) both share. `None` for a degenerate
/// viewport or preview.
/// The R/G/B under the pointer, formatted for the viewer's corner
/// (ADR 0112 §2), or `None` when the pointer is off the photograph.
///
/// `u` and `v` are the pointer in [0, 1] image coordinates; anything outside
/// that square is off the image, which is a readout of nothing rather than a
/// clamped edge pixel.
pub fn pixel_readout(image: &leyline_sdk::Rgb8, u: f32, v: f32) -> Option<String> {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return None;
    }
    // The last row and column are reachable at exactly 1.0, hence the min.
    let x = ((u * image.width() as f32) as u32).min(image.width().saturating_sub(1));
    let y = ((v * image.height() as f32) as u32).min(image.height().saturating_sub(1));
    let offset = (y as usize * image.width() as usize + x as usize) * 3;
    let rgb = image.data().get(offset..offset + 3)?;
    Some(format!("R {}  G {}  B {}", rgb[0], rgb[1], rgb[2]))
}

/// Which tonal setting a tone belongs to (ADR 0130 §3).
///
/// `u` and `v` are the pointer in [0, 1] image coordinates, as
/// [`pixel_readout`] takes them; outside that square there is no tone and
/// therefore no setting.
///
/// The thresholds cut the displayed range into the five bands the Basic
/// panel names, and they are **bands rather than weights** on purpose: a
/// weighted blend would move three sliders at once from one gesture, and a
/// photographer dragging on a sky wants to be able to say afterwards what
/// changed. The boundaries are the conventional ones — a sixth of the way
/// up is black, a quarter is shadow, three fifths is midtone — and the
/// midtone band is deliberately the widest, being where most of a
/// photograph is.
///
/// Luminance is Rec. 709's, on the **displayed** pixel: this is a question
/// about what the eye is looking at, not about scene-referred energy.
pub fn target_setting(image: &leyline_sdk::Rgb8, u: f32, v: f32) -> Option<&'static str> {
    if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
        return None;
    }
    let x = ((u * image.width() as f32) as u32).min(image.width().saturating_sub(1));
    let y = ((v * image.height() as f32) as u32).min(image.height().saturating_sub(1));
    let offset = (y as usize * image.width() as usize + x as usize) * 3;
    let rgb = image.data().get(offset..offset + 3)?;
    let luma = 0.2126 * f32::from(rgb[0]) + 0.7152 * f32::from(rgb[1]) + 0.0722 * f32::from(rgb[2]);
    Some(match luma {
        l if l < 40.0 => "blacks",
        l if l < 80.0 => "shadows",
        l if l < 160.0 => "exposure",
        l if l < 215.0 => "highlights",
        _ => "whites",
    })
}

/// Every group [`reset_group`] answers to, in panel order — the list
/// `reset_group("all")` walks (ADR 0124 §4). The three headers that are not
/// settings are absent for the reason they are absent from the `match`:
/// Soft Proof is a view, Versions and History are lists.
const GROUPS: [&str; 16] = [
    "basic",
    "camera-profile",
    "lut",
    "lens",
    "detail",
    "effects",
    "tone-curve",
    "levels",
    "hsl",
    "color-grading",
    "red-eye",
    "reshape",
    "spot-removal",
    "local",
    "crop",
    "geometry",
];

/// Every parameter one group of the develop panel owns, set back to its
/// neutral value (ADR 0112 §4).
///
/// Returns an empty list for a group already at neutral — a reset that
/// changes nothing must not write a revision — and for the three headers
/// that are not settings at all (Soft Proof is a view, Versions and History
/// are lists).
///
/// The mapping is spelled out rather than derived from `SettingsGroup`: the
/// preset machinery names seven families, and the panel has sixteen headers.
/// The three the model does name are taken from it; the rest are named here,
/// where the panel's own division lives.
pub fn reset_group(group: &str, current: &Settings) -> Vec<(Param, Value)> {
    // Everything, in one revision (ADR 0124 §4): the union of every other
    // group, built by calling this same function once per group. Spelled
    // out as a list of *this* function's own arms rather than as a second
    // enumeration of parameters, so a group added below joins it by
    // construction and cannot be left behind.
    if group == "all" {
        return GROUPS
            .iter()
            .flat_map(|group| reset_group(group, current))
            .collect();
    }
    let neutral = leyline_sdk::neutral_settings();
    let mut changes: Vec<(Param, Value)> = Vec::new();
    let mut set = |param: Param, value: Value| changes.push((param, value));

    match group {
        "basic" => {
            if current.white_balance.is_some() {
                set(Param::WhiteBalance, Value::WhiteBalance(None));
            }
            for (param, value, is_neutral) in [
                (Param::Exposure, Value::Float(0.0), current.exposure == 0.0),
                (Param::Contrast, Value::Int(0), current.contrast == 0),
                (Param::Highlights, Value::Int(0), current.highlights == 0),
                (Param::Shadows, Value::Int(0), current.shadows == 0),
                (Param::Whites, Value::Int(0), current.whites == 0),
                (Param::Blacks, Value::Int(0), current.blacks == 0),
                (Param::Texture, Value::Int(0), current.texture == 0),
                (Param::Clarity, Value::Int(0), current.clarity == 0),
                (Param::Dehaze, Value::Int(0), current.dehaze == 0),
                (Param::Vibrance, Value::Int(0), current.vibrance == 0),
                (Param::Saturation, Value::Int(0), current.saturation == 0),
                (Param::Monochrome, Value::Bool(false), !current.monochrome),
                (
                    Param::HighlightRolloff,
                    Value::Int(neutral.output_rendering.highlight_rolloff),
                    current.output_rendering.highlight_rolloff
                        == neutral.output_rendering.highlight_rolloff,
                ),
                (
                    Param::HighlightReconstruction,
                    Value::HighlightReconstruction(neutral.highlight_reconstruction),
                    current.highlight_reconstruction == neutral.highlight_reconstruction,
                ),
                (
                    Param::Demosaic,
                    Value::Demosaic(neutral.demosaic),
                    current.demosaic == neutral.demosaic,
                ),
            ] {
                if !is_neutral {
                    set(param, value);
                }
            }
        }
        "camera-profile" => {
            if current.camera_profile.is_some() {
                set(Param::CameraProfile, Value::CameraProfile(None));
            }
        }
        "lut" => {
            if current.lut.is_some() {
                set(Param::Lut, Value::Lut(None));
            }
        }
        // The group is "what the lens did to this photograph", so the reset
        // takes the defringe pair with the aberration pair (ADR 0113 §5).
        "lens" => {
            if current.lens_correction != neutral.lens_correction {
                set(
                    Param::LensCorrection,
                    Value::LensCorrection(neutral.lens_correction.clone()),
                );
            }
            if current.defringe != neutral.defringe {
                set(Param::Defringe, Value::Defringe(neutral.defringe));
            }
        }
        "detail" => {
            if current.noise_reduction != neutral.noise_reduction {
                set(
                    Param::NoiseReduction,
                    Value::NoiseReduction(neutral.noise_reduction.clone()),
                );
            }
            if current.sharpening != neutral.sharpening {
                set(
                    Param::Sharpening,
                    Value::Sharpening(neutral.sharpening.clone()),
                );
            }
            if current.moire != neutral.moire {
                set(Param::Moire, Value::Int(neutral.moire));
            }
        }
        "effects" => {
            if current.vignette != neutral.vignette {
                set(Param::Vignette, Value::Vignette(neutral.vignette));
            }
            if current.grain != neutral.grain {
                set(Param::Grain, Value::Grain(neutral.grain));
            }
        }
        "levels" => {
            if current.levels != neutral.levels {
                set(Param::Levels, Value::Levels(neutral.levels));
            }
        }
        "tone-curve" => {
            if current.tone_curve != neutral.tone_curve {
                set(
                    Param::ToneCurve,
                    Value::ToneCurve(neutral.tone_curve.clone()),
                );
            }
        }
        // Eight bands, each its own parameter: the mixer has no whole-struct
        // step, so a reset is eight of them — and only the ones that moved.
        "hsl" => {
            for (index, band) in current.hsl.iter().enumerate() {
                if *band != neutral.hsl[index] {
                    set(Param::HslBand(index), Value::HslBand(neutral.hsl[index]));
                }
            }
        }
        "color-grading" => {
            if current.color_grading != neutral.color_grading {
                set(
                    Param::ColorGrading,
                    Value::ColorGrading(neutral.color_grading),
                );
            }
        }
        "red-eye" => {
            if !current.red_eye.is_empty() {
                set(Param::RedEye, Value::RedEye(Vec::new()));
            }
        }
        "reshape" => {
            if !current.reshape.is_empty() {
                set(Param::Reshape, Value::Reshape(Vec::new()));
            }
        }
        "spot-removal" => {
            if !current.spot_removal.is_empty() {
                set(Param::SpotRemoval, Value::SpotRemoval(Vec::new()));
            }
        }
        // Removing by index, last first: each removal shifts the ones after
        // it, and walking backwards is the only order in which the indices
        // stay true.
        "local" => {
            for index in (0..current.local_adjustments.len()).rev() {
                set(Param::LocalAdjustment(index), Value::LocalAdjustment(None));
            }
        }
        // The Crop panel owns the straighten angle with the frame
        // (ADR 0160 §2); Geometry keeps what the Keystone guides write.
        "crop" => {
            if current.rotation != 0.0 {
                set(Param::Rotation, Value::Float(0.0));
            }
            if current.crop.is_some() {
                set(Param::Crop, Value::Crop(None));
            }
        }
        "geometry" if current.perspective.is_some() => {
            set(Param::Perspective, Value::Perspective(None));
        }
        // Soft Proof is a view setting, Versions and History are lists:
        // nothing of a revision to put back.
        _ => {}
    }
    changes
}

/// Turns one drag on the develop viewer into a keystone guide (ADR 0119).
///
/// Returns the line twice: in the frame the `perspective` stage sees, which
/// is what the solver is given, and in the displayed frame's own units,
/// which is what the overlay draws. The second is resolution-independent, so
/// resizing the window moves the drawn guides with the photograph instead of
/// leaving them behind.
///
/// **The crop is undone on the way**, and this is the one placed tool that
/// does it: the guides answer a question about the buffer `perspective`
/// receives, which is the *uncropped* one, and getting that wrong would tilt
/// every answer on a cropped photograph. A click is refused rather than
/// recorded as a line of no length.
pub fn keystone_line(
    press: (f64, f64),
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    crop: Option<&Crop>,
) -> Option<(GuideLine, [f64; 4])> {
    let a = letterbox_unit(press, view, image)?;
    let b = letterbox_unit(release, view, image)?;
    if (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6 {
        return None;
    }
    Some((
        GuideLine {
            a: uncropped(a, crop),
            b: uncropped(b, crop),
        },
        [a.0, a.1, b.0, b.1],
    ))
}

/// Maps a point of the displayed (cropped) frame back into the frame the
/// `perspective` stage sees — ADR 0026's, pre-crop. Identity when nothing is
/// cropped.
fn uncropped(unit: (f64, f64), crop: Option<&Crop>) -> Point {
    match crop {
        Some(crop) if crop.width > 0.0 && crop.height > 0.0 => Point {
            x: crop.x + unit.0 * crop.width,
            y: crop.y + unit.1 * crop.height,
        },
        _ => Point {
            x: unit.0,
            y: unit.1,
        },
    }
}

pub(crate) fn letterbox_unit(
    point: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
) -> Option<(f64, f64)> {
    if view.0 <= 0.0 || view.1 <= 0.0 || image.0 <= 0.0 || image.1 <= 0.0 {
        return None;
    }
    let scale = (view.0 / image.0).min(view.1 / image.1);
    let (offset_x, offset_y) = (
        (view.0 - image.0 * scale) / 2.0,
        (view.1 - image.1 * scale) / 2.0,
    );
    Some((
        ((point.0 - offset_x) / (image.0 * scale)).clamp(0.0, 1.0),
        ((point.1 - offset_y) / (image.1 * scale)).clamp(0.0, 1.0),
    ))
}

/// Paints display-referred clipping over an 8-bit sRGB render, in place
/// (ADR 0092 §1): a clipped highlight is a channel at 255, painted red when
/// `paint_highlights`; a crushed shadow is all three at 0, painted blue
/// when `paint_shadows`. Returns whether each end is occupied at all — the
/// histogram's two triangles light from that, overlay on or off.
pub fn paint_clipping(
    data: &mut [u8],
    paint_highlights: bool,
    paint_shadows: bool,
) -> (bool, bool) {
    let mut any_high = false;
    let mut any_low = false;
    for rgb in data.chunks_exact_mut(3) {
        let high = rgb.contains(&255);
        let low = rgb.iter().all(|&v| v == 0);
        any_high |= high;
        any_low |= low;
        if high && paint_highlights {
            (rgb[0], rgb[1], rgb[2]) = (255, 59, 48);
        } else if low && paint_shadows {
            (rgb[0], rgb[1], rgb[2]) = (10, 132, 255);
        }
    }
    (any_high, any_low)
}

/// Builds one channel's filled-area histogram path in a `width` x `height`
/// viewbox, `sqrt`-scaled against `scale_max` (the tallest bin *across all
/// three channels*, so R/G/B stay on the same vertical scale — passing each
/// channel's own max instead would make every channel look equally tall no
/// matter its actual weight). `sqrt` rather than linear: a single dominant
/// bin (a large flat sky, a black border) would otherwise flatten every
/// other bin to near-zero height. Pure so the scaling math is unit-tested
/// without a live Slint canvas.
pub fn histogram_layout(bins: &[u32; 256], scale_max: u32, width: f64, height: f64) -> String {
    let scale = (scale_max as f64).sqrt().max(1.0);
    let step = width / 255.0;
    let mut path = format!("M0 {height}");
    for (i, &count) in bins.iter().enumerate() {
        let x = i as f64 * step;
        let bar_height = (count as f64).sqrt() / scale * height;
        path.push_str(&format!(" L{x} {}", height - bar_height));
    }
    path.push_str(&format!(" L{width} {height} Z"));
    path
}

/// The crop edge sliders, in percent of the frame. The smallest accepted
/// edge is 1 % so the rectangle never collapses.
fn crop_action(edge: &str, percent: f64, current: Option<Crop>) -> Option<Option<Crop>> {
    if edge == "reset" {
        return Some(None);
    }
    let mut crop = current.unwrap_or(FULL_FRAME);
    let value = (percent / 100.0).clamp(0.0, 1.0);
    match edge {
        "left" => {
            crop.x = value.min(0.99);
            crop.width = crop.width.min(1.0 - crop.x);
        }
        "top" => {
            crop.y = value.min(0.99);
            crop.height = crop.height.min(1.0 - crop.y);
        }
        "width" => crop.width = value.clamp(0.01, 1.0 - crop.x),
        "height" => crop.height = value.clamp(0.01, 1.0 - crop.y),
        _ => return None,
    }
    Some(if crop == FULL_FRAME { None } else { Some(crop) })
}

/// The neutral crop: the whole frame.
const FULL_FRAME: Crop = Crop {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod reset_tests;

#[cfg(test)]
mod readout_tests;

#[cfg(test)]
mod reshape_tests;
