//! Preset capture and application (`docs/engine-api.md` §10.3, `docs/presets.md`).
//!
//! A preset never rewrites a revision directly: capturing one reads the
//! current head through the same [`Settings`] the engine already knows, and
//! applying one feeds the same [`EditSession::set`]/[`commit`](EditSession::commit)
//! path a manual edit would — one call per field of the included groups, on
//! a fresh session per version so the result is always a new revision, never
//! an amendment (`docs/presets.md` §5.1, `docs/adr/0014-develop-presets.md`).

use leyline_catalog::Catalog;
use leyline_core::{
    CURRENT_SCHEMA, LeylineError, PresetId, PresetSettings, Result, Settings, SettingsGroup,
    VersionId,
};

use crate::session::{EditSession, Param, Value};

/// Outcome of one preset application batch — same shape as [`crate::ExportReport`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PresetApplyReport {
    /// Versions that received a new revision, in request order.
    pub applied: Vec<VersionId>,
    /// Versions left untouched, with the human-readable reason.
    pub failed: Vec<FailedApply>,
}

/// One version a preset batch could not apply to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedApply {
    /// The version left unchanged.
    pub version: VersionId,
    /// Why the application failed.
    pub reason: String,
}

/// Captures the fields of `groups` from `from`'s head (`docs/presets.md` §3.1).
///
/// Refused with [`LeylineError::NewerSettings`] when the head was written by
/// a newer engine, the same guard [`EditSession::open`] applies.
pub fn capture(
    catalog: &Catalog,
    from: VersionId,
    groups: &[SettingsGroup],
) -> Result<PresetSettings> {
    let head = catalog.version_head(from)?;
    let settings = Settings::parse(&catalog.revision(head)?.settings_json)?;
    if settings.schema > CURRENT_SCHEMA {
        return Err(LeylineError::NewerSettings {
            schema: settings.schema,
        });
    }
    Ok(PresetSettings::capture(&settings, groups))
}

/// Applies `preset` to each version of `versions`, one fresh session (and
/// therefore one new revision) per version (`docs/presets.md` §5.1–§5.2).
/// One failing version does not stop the batch; `progress` receives
/// `(done, total)` after each version, the batching contract of
/// `docs/engine-api.md` §3.1.
pub fn apply_batch(
    catalog: &mut Catalog,
    preset: &PresetSettings,
    versions: &[VersionId],
    progress: impl FnMut(u64, u64),
) -> PresetApplyReport {
    apply_batch_from(catalog, preset, None, versions, progress)
}

/// [`apply_batch`], recording which stored preset — and which version of it —
/// produced each revision (ADR 0058 §5).
///
/// `from_preset` is `None` for a set of settings that is not a stored preset:
/// pasted settings, or a preset applied from a file. Provenance names a
/// catalog row, and inventing one for something that has none would be worse
/// than saying nothing.
pub fn apply_batch_from(
    catalog: &mut Catalog,
    preset: &PresetSettings,
    from_preset: Option<(PresetId, u32)>,
    versions: &[VersionId],
    mut progress: impl FnMut(u64, u64),
) -> PresetApplyReport {
    let values = param_values(preset);
    let total = versions.len() as u64;
    let mut report = PresetApplyReport::default();
    for (done, &version) in versions.iter().enumerate() {
        match apply_one(catalog, version, &values, from_preset) {
            Ok(()) => report.applied.push(version),
            Err(error) => report.failed.push(FailedApply {
                version,
                reason: error.to_string(),
            }),
        }
        progress(done as u64 + 1, total);
    }
    report
}

/// The settings `preset` would produce on top of `base`, computed in memory
/// and written nowhere (ADR 0058 §4).
///
/// What a client renders to show a preset **before** applying it. It goes
/// through [`param_values`] and `session::apply`, the same two steps
/// [`apply_one`] takes, precisely so the trial cannot show something the
/// application would not produce: the only difference is that nothing is
/// committed.
pub fn overlay(base: &Settings, preset: &PresetSettings) -> Result<Settings> {
    let mut settings = base.clone();
    for (param, value) in param_values(preset) {
        crate::session::apply(&mut settings, param, value)?;
    }
    Ok(settings)
}

/// Opens a fresh session on `version`, sets every captured field, and
/// commits once — never an amendment (`docs/presets.md` §5.1): a session
/// that has never committed has no amendment chain to extend.
fn apply_one(
    catalog: &mut Catalog,
    version: VersionId,
    values: &[(Param, Value)],
    from_preset: Option<(PresetId, u32)>,
) -> Result<()> {
    let mut session = EditSession::open(&mut *catalog, version)?;
    for (param, value) in values {
        session.set(*param, value.clone())?;
    }
    session.commit_from(from_preset)?;
    Ok(())
}

/// Turns the populated fields of a [`PresetSettings`] into the `(Param,
/// Value)` pairs [`EditSession::set`] expects, reusing its existing
/// validation instead of duplicating it.
fn param_values(preset: &PresetSettings) -> Vec<(Param, Value)> {
    let mut values = Vec::new();
    if let Some(wb) = &preset.white_balance {
        values.push((Param::WhiteBalance, Value::WhiteBalance(wb.clone())));
    }
    if let Some(v) = preset.exposure {
        values.push((Param::Exposure, Value::Float(v)));
    }
    if let Some(v) = preset.contrast {
        values.push((Param::Contrast, Value::Int(v)));
    }
    if let Some(v) = preset.highlights {
        values.push((Param::Highlights, Value::Int(v)));
    }
    if let Some(v) = preset.shadows {
        values.push((Param::Shadows, Value::Int(v)));
    }
    if let Some(v) = preset.whites {
        values.push((Param::Whites, Value::Int(v)));
    }
    if let Some(v) = preset.blacks {
        values.push((Param::Blacks, Value::Int(v)));
    }
    // ADR 0132 §1 widened Presence to the whole of Basic below the tonal
    // sliders; a preset written before it carries none of these three, and
    // an absent field is left alone as always.
    if let Some(v) = preset.clarity {
        values.push((Param::Clarity, Value::Int(v)));
    }
    if let Some(v) = preset.texture {
        values.push((Param::Texture, Value::Int(v)));
    }
    if let Some(v) = preset.dehaze {
        values.push((Param::Dehaze, Value::Int(v)));
    }
    if let Some(v) = preset.vibrance {
        values.push((Param::Vibrance, Value::Int(v)));
    }
    if let Some(v) = preset.saturation {
        values.push((Param::Saturation, Value::Int(v)));
    }
    // Captured by `PresetSettings::capture` since ADR 0088, and until
    // ADR 0090 not applied here: a "presence" preset stored the black and
    // white it had been shown and then dropped it on the way back.
    if let Some(v) = preset.monochrome {
        values.push((Param::Monochrome, Value::Bool(v)));
    }
    if let Some(v) = preset.vignette {
        values.push((Param::Vignette, Value::Vignette(v)));
    }
    if let Some(v) = preset.grain {
        values.push((Param::Grain, Value::Grain(v)));
    }
    if let Some(v) = &preset.lens_correction {
        values.push((Param::LensCorrection, Value::LensCorrection(v.clone())));
    }
    if let Some(v) = preset.defringe {
        values.push((Param::Defringe, Value::Defringe(v)));
    }
    if let Some(v) = &preset.noise_reduction {
        values.push((Param::NoiseReduction, Value::NoiseReduction(v.clone())));
    }
    if let Some(v) = &preset.sharpening {
        values.push((Param::Sharpening, Value::Sharpening(v.clone())));
    }
    if let Some(v) = preset.rotation {
        values.push((Param::Rotation, Value::Float(v)));
    }
    if let Some(v) = &preset.crop {
        values.push((Param::Crop, Value::Crop(v.clone())));
    }
    if let Some(v) = &preset.perspective {
        values.push((Param::Perspective, Value::Perspective(*v)));
    }

    // The ten categories ADR 0132 §1 added. Each is one whole value, which
    // is what makes them atomic in the sense `docs/presets.md` §3.1 means:
    // a mix is one decision, not eight.
    if let Some(v) = &preset.tone_curve {
        values.push((Param::ToneCurve, Value::ToneCurve(v.clone())));
    }
    if let Some(v) = preset.parametric_curve {
        values.push((Param::ParametricCurve, Value::ParametricCurve(v)));
    }
    if let Some(bands) = &preset.hsl {
        // The one category with no whole-value parameter: the mixer is
        // addressed a band at a time, so eight entries stand for one
        // checkbox. Indices are fixed `0..8`, never the target's length,
        // which is why this needs no equivalent of `Param::LocalAdjustments`.
        for (index, band) in bands.iter().enumerate() {
            values.push((Param::HslBand(index), Value::HslBand(*band)));
        }
    }
    if let Some(v) = &preset.color_grading {
        values.push((Param::ColorGrading, Value::ColorGrading(*v)));
    }
    if let Some(v) = &preset.camera_profile {
        values.push((Param::CameraProfile, Value::CameraProfile(v.clone())));
    }
    if let Some(v) = &preset.lut {
        values.push((Param::Lut, Value::Lut(v.clone())));
    }
    if let Some(v) = preset.highlight_reconstruction {
        values.push((
            Param::HighlightReconstruction,
            Value::HighlightReconstruction(v),
        ));
    }
    if let Some(v) = preset.demosaic {
        values.push((Param::Demosaic, Value::Demosaic(v)));
    }
    if let Some(v) = &preset.output_rendering {
        values.push((Param::HighlightRolloff, Value::Int(v.highlight_rolloff)));
    }
    if let Some(v) = &preset.reshape {
        values.push((Param::Reshape, Value::Reshape(v.clone())));
    }
    if let Some(v) = &preset.spot_removal {
        values.push((Param::SpotRemoval, Value::SpotRemoval(v.clone())));
    }
    if let Some(v) = &preset.red_eye {
        values.push((Param::RedEye, Value::RedEye(v.clone())));
    }
    if let Some(v) = &preset.local_adjustments {
        values.push((Param::LocalAdjustments, Value::LocalAdjustments(v.clone())));
    }
    values
}
