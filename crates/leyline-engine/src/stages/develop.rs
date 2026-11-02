//! Running a plan: full renders, cached renders, renders up to a rank, and
//! mask coverage.

use super::*;

/// Renders like [`develop_scaled`], reusing and refreshing `cache`.
///
/// Same pixels as [`develop_scaled`], always: the cache only decides where
/// the work starts, never what it computes. `stage_cache_matches_a_cold_render`
/// is what holds that claim.
#[allow(clippy::too_many_arguments)]
pub(crate) fn develop_scaled_cached(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    sensor: Option<&SensorShot>,
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    coverages: &crate::mask_coverage::MaskCoverages,
    source: &Source,
    scale: f32,
    target: impl Into<CacheTarget>,
    cache: &mut StageCache,
) -> Result<Rendered> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        sensor,
        camera_profile,
        lut,
        coverages,
        source: source.color,
        profile: source.profile.as_ref(),
        scale,
    };

    // Where the snapshots go, resolved against *this* plan. A threshold
    // names a position in the pipeline, not a stage: matching an exact
    // rank would silently skip a checkpoint whenever the stage sitting at
    // it happens to be neutral — which is the common case, and which
    // measurement caught leaving three of the four checkpoints untaken.
    let cuts: Vec<usize> = CHECKPOINT_BEFORE_RANK
        .iter()
        .filter_map(|&rank| plan.iter().position(|(_, v)| v.rank >= rank))
        .collect();

    cache.retarget(target.into(), scale, image);
    // The GPU first, from the deepest checkpoint, *before* that checkpoint
    // is cloned for the CPU: at 4K the clone alone is ~134 MB (ADR 0156 §4).
    #[cfg(feature = "gpu")]
    if let Some(rendered) = cache.render_on_gpu(settings, &plan) {
        return Ok(rendered);
    }
    let (mut applied, mut px) = match cache.resume_from(settings, &plan) {
        Some((applied, pixels)) => (applied, pixels),
        None => (0, Pixels::from_raw(image)?),
    };

    while applied < plan.len() {
        // Snapshot before the stage that opens a block, not after the one
        // that closed it: what matters is that the buffer entering an
        // expensive run is kept, whatever ran before it.
        if cuts.contains(&applied) {
            cache.store(
                settings,
                &plan,
                applied,
                &px,
                cuts.first() == Some(&applied),
            );
        }
        plan[applied].1.run(&mut px, &ctx);
        applied += 1;
    }

    Ok(Rendered {
        width: px.width,
        height: px.height,
        data: px.to_rgb8(),
    })
}

/// Renders a decoded image through the stages its settings record.
/// `settings` has already been validated and checked against
/// `CURRENT_SCHEMA` by [`crate::render::render_scaled`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn develop_scaled(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    sensor: Option<&SensorShot>,
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    coverages: &crate::mask_coverage::MaskCoverages,
    source: &Source,
    scale: f32,
) -> Result<Rendered> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        sensor,
        camera_profile,
        lut,
        coverages,
        source: source.color,
        profile: source.profile.as_ref(),
        scale,
    };
    let mut px = Pixels::from_raw(image)?;
    for (_, version) in plan {
        version.run(&mut px, &ctx);
    }
    Ok(Rendered {
        width: px.width,
        height: px.height,
        data: px.to_rgb8(),
    })
}

/// Rank of the local adjustments stage — where a mask's coverage is decided.
pub(super) const LOCAL_ADJUSTMENTS_RANK: u16 = 160;

/// Rank the exchange image of a derivation is taken at (ADR 0107 §4): after
/// `input` (0) and `camera_profile` (10), before everything else.
///
/// The first rank at which the buffer has a **single** meaning. Below it,
/// `input` leaves the samples camera-native when a DCP is set and rotates
/// them into Rec. 2020 when there is none, so a file handed over at rank 0
/// would be in one of two spaces depending on a setting — which no protocol
/// document can paper over.
pub(crate) const DERIVE_RANK: u16 = 20;

/// Develops `image` up to — and not including — `max_rank`, and returns the
/// working buffer as it stands there.
///
/// The prefix of [`develop_scaled`], stopping early and skipping the
/// quantization to eight bits at the end: what a derivation hands to a
/// processor is light, not a picture (ADR 0107 §3). Same plan, same
/// versions, same order — a derivation renders through exactly what an
/// ordinary render would have run first.
#[allow(clippy::too_many_arguments)]
pub(crate) fn develop_until_rank(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    sensor: Option<&SensorShot>,
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    coverages: &crate::mask_coverage::MaskCoverages,
    source: &Source,
    max_rank: u16,
) -> Result<(u32, u32, Vec<f32>)> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        sensor,
        camera_profile,
        lut,
        coverages,
        source: source.color,
        profile: source.profile.as_ref(),
        scale: 1.0,
    };
    let mut px = Pixels::from_raw(image)?;
    for (_, version) in &plan {
        if version.rank >= max_rank {
            break;
        }
        version.run(&mut px, &ctx);
    }
    Ok((px.width, px.height, px.data))
}

/// Renders the effective coverage of one local adjustment, in the geometry of
/// the finished image (ADR 0071 §2).
///
/// Two passes over the same plan. The first develops the photo up to — and
/// not including — the local adjustments stage, because that is the buffer the
/// range terms read. The coverage is computed there, and the second pass
/// pushes *it* through the three geometry stages that run after rank 160:
/// `rotate`, `perspective`, `crop`, at the versions the revision pins. The
/// pixel operators in between (LUT, noise, sharpening) are skipped: they would
/// distort a mask image without telling anyone anything.
///
/// Returns `width * height` coverage samples in `[0, 1]`, at the size of the
/// developed image.
#[allow(clippy::too_many_arguments)]
pub(crate) fn develop_mask_coverage(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    sensor: Option<&SensorShot>,
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    coverages: &crate::mask_coverage::MaskCoverages,
    source: &Source,
    scale: f32,
    index: usize,
) -> Result<(u32, u32, Vec<f32>)> {
    let adjustment = settings.local_adjustments.get(index).ok_or_else(|| {
        LeylineError::InvalidSettings(format!(
            "no local adjustment at index {index}; this revision has {}",
            settings.local_adjustments.len()
        ))
    })?;
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        sensor,
        camera_profile,
        lut,
        coverages,
        source: source.color,
        profile: source.profile.as_ref(),
        scale,
    };

    let mut px = Pixels::from_raw(image)?;
    for (_, version) in &plan {
        if version.rank >= LOCAL_ADJUSTMENTS_RANK {
            break;
        }
        version.run(&mut px, &ctx);
    }

    // The plan already resolved which version this revision renders through,
    // pinned or defaulted; reading it back beats re-deriving it.
    let pinned = plan
        .iter()
        .find(|(stage, _)| stage.name == "local_adjustments")
        .map_or(1, |(_, version)| version.version);
    let coverage = crate::mask_overlay::effective_coverage(
        pinned,
        &px,
        adjustment,
        settings.rotation,
        coverages,
    );

    // The coverage travels as an ordinary buffer so the geometry stages can
    // resample it exactly as they resample the photo.
    let mut carrier = Pixels {
        width: px.width,
        height: px.height,
        data: coverage.iter().flat_map(|c| [*c, *c, *c]).collect(),
    };
    drop(px);
    for (stage, version) in &plan {
        if matches!(stage.name, "rotate" | "perspective" | "crop") {
            version.run(&mut carrier, &ctx);
        }
    }

    let samples = carrier
        .data
        .chunks_exact(3)
        .map(|rgb| rgb[0].clamp(0.0, 1.0))
        .collect();
    Ok((carrier.width, carrier.height, samples))
}
