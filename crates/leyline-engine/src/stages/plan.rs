//! From settings to an ordered list of stage versions: which version each
//! stage renders through, pinning, the plan of a render, and its fingerprint.

use super::*;

/// The version of `stage_name` this render uses: the one `settings` records,
/// or the current one compatible with the space it is already committed to.
///
/// Only meaningful for the stages that are always active; a caller asking
/// about a stage absent from the registry gets a panic, since the name is
/// always a literal from this module.
pub(super) fn version_of(stage_name: &'static str, settings: &Settings) -> &'static Version {
    #[allow(
        clippy::expect_used,
        reason = "the stage name is always a literal of a registered stage from this module"
    )]
    let stage = registry()
        .find(|stage| stage.name == stage_name)
        .expect("the stage name is a literal from this module");
    match settings.stages.get(stage_name) {
        Some(&recorded) => stage
            .versions
            .iter()
            .find(|v| v.version == recorded)
            .unwrap_or_else(|| stage.current()),
        None => pinned_version(stage, settings),
    }
}

/// The parametric curve as `samples` points of the very table the stage
/// renders through (ADR 0137 §4).
///
/// What a client draws is therefore computed by the code that renders, and
/// the two cannot drift — the same reason the keystone guides arrive from
/// Rust as a polyline rather than being redrawn in the panel (ADR 0119).
#[must_use]
pub fn parametric_curve_samples(
    curve: &leyline_core::ParametricCurve,
    samples: usize,
) -> Vec<leyline_core::CurvePoint> {
    let table = tone_curve::v1::build_curve_lut(&parametric_curve::v1::control_points(curve));
    let last = samples.max(2) - 1;
    (0..=last)
        .map(|i| {
            let x = i as f32 / last as f32;
            leyline_core::CurvePoint {
                x: f64::from(x),
                y: f64::from(tone_curve::v1::curve_lookup(&table, x)),
            }
        })
        .collect()
}

/// Stages this engine implements. In `cfg(test)` builds it also carries the
/// two-version fixture stage of ADR 0043 §7, which keeps the "an older
/// version still renders what it rendered" path exercised while every real
/// operator sits at v1.
pub(super) fn registry() -> impl Iterator<Item = &'static Stage> {
    STAGES.iter().chain(extra_stages())
}

#[cfg(test)]
pub(super) fn extra_stages() -> &'static [Stage] {
    fixture::STAGES
}

#[cfg(not(test))]
pub(super) fn extra_stages() -> &'static [Stage] {
    &[]
}

/// Looks a `(name, version)` pair up in the registry.
pub(super) fn find(name: &str, version: u16) -> Option<(&'static Stage, &'static Version)> {
    let stage = registry().find(|stage| stage.name == name)?;
    let version = stage.versions.iter().find(|v| v.version == version)?;
    Some((stage, version))
}

/// Fails when `settings` records a stage, or a version of one, this engine
/// does not implement — the rendering half of the §3.4 guard
/// (`docs/pipeline.md`), checked wherever a revision is opened or rendered.
pub(crate) fn check_known(settings: &Settings) -> Result<()> {
    for (name, &version) in &settings.stages {
        if find(name, version).is_none() {
            return Err(LeylineError::UnknownStage {
                stage: name.clone(),
                version,
            });
        }
    }
    Ok(())
}

/// The working space `settings` is already committed to (ADR 0044 §4).
///
/// Read off the versions it records, since the space is a property of those
/// and never a field of its own: a revision recording nothing yet — fresh
/// settings, a preset, a test — gets the space this engine renders in today.
/// A revision whose records disagree resolves to the first of them here and
/// is refused by [`plan`], which can name both sides.
#[allow(
    clippy::expect_used,
    reason = "the input stage is always registered: every render starts with it"
)]
pub(super) fn revision_space(settings: &Settings) -> Space {
    settings
        .stages
        .iter()
        .find_map(|(name, &version)| find(name, version).map(|(_, v)| v.space))
        .unwrap_or_else(|| {
            registry()
                .find(|stage| stage.name == "input")
                .expect("the input stage is always registered")
                .current()
                .space
        })
}

/// The version a stage receives when it enters a revision that does not
/// record it yet: the newest one working in the revision's space, or — when
/// this operator has never rendered there — the newest one at all, so the
/// mismatch is written down and refused by name instead of disappearing.
pub(super) fn pinned_version(stage: &'static Stage, settings: &Settings) -> &'static Version {
    stage
        .current_in(revision_space(settings))
        .unwrap_or_else(|| stage.current())
}

/// Records, in `settings`, the version of every stage it activates
/// (ADR 0043 §3). Called on the way *in* to a revision, never on the way out.
///
/// A stage already recorded keeps its version — that is the whole promise.
/// A stage that just left its neutral value gets [`pinned_version`]: the
/// current version *in the revision's working space*, never a version that
/// would silently move the revision to another one (ADR 0044 §4). A stage
/// back at its neutral value loses its entry, since it no longer renders
/// anything to pin — except the two that frame the pipeline, `input` and
/// `output_rendering`, which have no neutral value and are therefore always
/// recorded.
pub(crate) fn pin(settings: &mut Settings) {
    let mut pinned = leyline_core::StageVersions::new();
    for stage in registry() {
        if !(stage.active)(settings) {
            continue;
        }
        let version = settings
            .stages
            .get(stage.name)
            .copied()
            .unwrap_or_else(|| pinned_version(stage, settings).version);
        pinned.insert(stage.name.to_owned(), version);
    }
    settings.stages = pinned;
}

/// The develop state a stored revision starts from: neutral values, already
/// pinned.
///
/// Neutral or not, a *stored* revision records the versions it renders
/// through (`docs/pipeline.md` §3.3) — which for a neutral one means the two
/// stages that frame every pipeline (ADR 0044 §3). Public because
/// [`leyline_catalog::Catalog::add_asset`] takes the initial settings from
/// its caller: the catalog sits below the engine and cannot know what this
/// engine pins.
pub fn neutral_settings() -> Settings {
    let mut settings = Settings::default();
    pin(&mut settings);
    settings
}

/// Fails when the stages about to run do not agree on one working space.
///
/// Two spaces never compose: a `gains` working in linear Rec. 2020 handed a
/// gamma-encoded buffer would produce plausible, wrong pixels. Refusing is
/// the same posture as [`LeylineError::UnknownStage`] — a revision this
/// engine cannot render exactly is not rendered approximately.
pub(super) fn single_space(
    stages: impl IntoIterator<Item = (&'static str, u16, Space)>,
) -> Result<Option<Space>> {
    let mut agreed: Option<(&'static str, u16, Space)> = None;
    for (name, version, space) in stages {
        match agreed {
            None => agreed = Some((name, version, space)),
            Some((_, _, other)) if other == space => {}
            Some((other_name, other_version, other)) => {
                return Err(LeylineError::MixedWorkingSpaces {
                    stage: other_name.to_owned(),
                    version: other_version,
                    space: other.label().to_owned(),
                    other_stage: name.to_owned(),
                    other_version: version,
                    other_space: space.label().to_owned(),
                });
            }
        }
    }
    Ok(agreed.map(|(_, _, space)| space))
}

/// The stages `settings` renders through, in the order they run.
///
/// A stage that is active but carries no recorded version renders at this
/// engine's current version. That case is for settings built in memory —
/// through the SDK, a test, a preset applied to fresh values — since [`pin`]
/// gives every *stored* revision its entries at write time. It is also why
/// reading is never a silent guess: what a revision records, it gets.
///
/// A recorded stage this engine does not implement fails the render with
/// [`LeylineError::UnknownStage`] rather than being skipped (ADR 0043 §4):
/// dropping it would render the photo without an operator its author saw.
/// A plan whose stages do not agree on one working space fails the same way
/// ([`LeylineError::MixedWorkingSpaces`], ADR 0044 §4).
pub(super) fn plan(settings: &Settings) -> Result<Vec<(&'static Stage, &'static Version)>> {
    check_known(settings)?;
    let mut plan: Vec<_> = registry()
        .filter(|stage| (stage.active)(settings))
        .map(|stage| {
            #[allow(clippy::expect_used, reason = "check_known just refused every recorded stage this engine does not implement")]
            let version = match settings.stages.get(stage.name) {
                Some(&recorded) => {
                    find(stage.name, recorded)
                        .expect("every recorded stage was checked just above")
                        .1
                }
                None => pinned_version(stage, settings),
            };
            (stage, version)
        })
        .collect();
    plan.sort_by_key(|(_, version)| version.rank);
    single_space(
        plan.iter()
            .map(|(stage, version)| (stage.name, version.version, version.space)),
    )?;
    Ok(plan)
}

/// Fingerprints everything the first `applied` stages of `plan` depend on
/// (ADR 0041 §3): a checkpoint taken after that many stages is reusable
/// exactly while this value is unchanged.
///
/// The render *context* — decoded source, lens shot, camera profile, LUT,
/// scale — is deliberately not covered. None of it lives in `Settings`,
/// and it identifies the cache as a whole rather than one checkpoint
/// within it, so the caller drops the whole cache when it changes.
pub(crate) fn prefix_fingerprint(
    settings: &Settings,
    plan: &[(&'static Stage, &'static Version)],
    applied: usize,
) -> u64 {
    let keys: Vec<&str> = plan
        .iter()
        .take(applied)
        .flat_map(|(stage, _)| stage.reads.iter().copied())
        .collect();
    settings.fingerprint(&keys)
}

/// The scene's colour temperature, for a profile that calibrates against
/// two illuminants (ADR 0062 §2).
///
/// The revision's own white balance when it names one — Leyline stores it in
/// kelvin already, so no estimation is needed and none is done. Otherwise
/// the camera's as-shot neutral, which the decoder reports as channel
/// multipliers, run through the DNG spec's iteration. With neither, D65,
/// because a calibration at one end of the interval is a defensible choice
/// where an average of both is not.
pub(super) fn scene_temperature(ctx: &Context<'_>, profile: &DcpProfile) -> f64 {
    if let Some(wb) = &ctx.settings.white_balance {
        return f64::from(wb.temperature);
    }
    if let SourceColor::Camera {
        multipliers: Some(multipliers),
        ..
    } = ctx.source
    {
        // The as-shot neutral is the reciprocal of the multipliers the
        // decoder would apply: the camera colour that white balances to
        // grey. A zero multiplier is a malformed file, not a neutral.
        let neutral = [multipliers[0], multipliers[1], multipliers[2]];
        if neutral.iter().all(|m| *m > 0.0) {
            let reciprocal = [1.0 / neutral[0], 1.0 / neutral[1], 1.0 / neutral[2]];
            return profile.temperature_from_neutral(reciprocal);
        }
    }
    leyline_color::FALLBACK_TEMPERATURE_K
}
