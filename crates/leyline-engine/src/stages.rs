//! The develop pipeline as a composition of independently versioned stages
//! (ADR 0042, ADR 0043).
//!
//! Until ADR 0042 the pipeline was eleven `processN.rs` modules, each a full
//! copy of the previous one — 14 968 lines, 70 to 93 % of them identical to
//! their neighbor. This module is the operators themselves, one frozen
//! module per version, plus the registry that composes them.
//!
//! A revision names the version of each stage it uses ([`pin`]); there is no
//! global rendering counter any more. ADR 0043 collapsed the pre-publication
//! history, so every operator here is at `v1` — the mechanism that lets an
//! older version keep rendering is exercised by a two-version fixture stage
//! (`stages/fixture.rs`, test builds only) until the first real `v2`
//! arrives.
//!
//! # What is frozen, and where
//!
//! The unit of freeze is the **stage version**, not the pipeline: a
//! `sharpen::v1` is frozen the day it ships, exactly as `process3.rs` was.
//! Correcting sharpening produces `sharpen::v2` and never touches `v1`, so
//! a revision citing `v1` renders through the same code forever
//! (`docs/pipeline.md` §5.1). Three things carry that guarantee together,
//! and all three are equally frozen:
//!
//! 1. the version modules themselves (`sharpen/v1.rs`, …);
//! 2. [`kernel::v1`], where a body shared by several stages lives;
//! 3. the [`STAGES`] table below — the `rank` and the `apply` binding of an
//!    *already published* `(name, version)` pair are as immutable as the
//!    body they point at, since together they are what the old
//!    `develop_scaled` used to spell out. Adding entries is allowed;
//!    editing published ones is not.
//!
//! What proves all of this is not review but `tests/golden_renders.rs`,
//! which pins the digest of one render per operator family (ADR 0042 §7).
//!
//! # Rank, not order of statements
//!
//! Every stage version declares its own position in the pipeline
//! ([`Version::rank`], in tens so a later stage can be inserted between two
//! existing ones). Moving a stage is therefore a new version that declares
//! another rank, never an edit of an existing one — revisions citing the old
//! version keep the old position.
//!
//! # Neutral stages
//!
//! A stage whose setting sits at its neutral value does not run at all
//! ([`Stage::active`]) — the rule that makes a neutral rendering bit-for-bit
//! the decoded image. It is also why a neutral stage has no behavior to pin
//! and does not need to appear in a revision.

pub(crate) mod kernel {
    pub(crate) mod v1;
}
pub(crate) mod camera_profile {
    pub(crate) mod v1;
}
pub(crate) mod lens {
    pub(crate) mod v1;
}
pub(crate) mod spot_removal {
    pub(crate) mod v1;
}
pub(crate) mod gains {
    pub(crate) mod v1;
}
pub(crate) mod contrast {
    pub(crate) mod v1;
}
pub(crate) mod highlights_shadows {
    pub(crate) mod v1;
}
pub(crate) mod whites_blacks {
    pub(crate) mod v1;
}
pub(crate) mod tone_curve {
    pub(crate) mod v1;
}
pub(crate) mod clarity {
    pub(crate) mod v1;
}
pub(crate) mod texture {
    pub(crate) mod v1;
}
pub(crate) mod dehaze {
    pub(crate) mod v1;
}
pub(crate) mod hsl {
    pub(crate) mod v1;
}
pub(crate) mod color_grading {
    pub(crate) mod v1;
}
pub(crate) mod local_adjustments {
    pub(crate) mod v1;
}
pub(crate) mod noise_luminance {
    pub(crate) mod v1;
}
pub(crate) mod noise_color {
    pub(crate) mod v1;
}
pub(crate) mod sharpen {
    pub(crate) mod v1;
}
pub(crate) mod rotate {
    pub(crate) mod v1;
}
pub(crate) mod crop {
    pub(crate) mod v1;
}

use leyline_color::DcpProfile;
use leyline_core::{ColorGrading, HslBand, LeylineError, Result, Settings};
use leyline_raw::RawImage;

use crate::pixels::Pixels;
use crate::render::{LensShot, Rendered};

/// Everything a stage may read besides the buffer it renders.
pub(crate) struct Context<'a> {
    /// The revision being rendered, already validated by the caller.
    pub settings: &'a Settings,
    /// EXIF identification of the shot, for the lens stage.
    pub shot: Option<&'a LensShot>,
    /// The already-resolved DCP matrix, for the camera profile stage.
    pub camera_profile: Option<&'a DcpProfile>,
    /// Factor by which the image was already reduced for a preview
    /// (ADR 0041). Stages expressing a radius in *pixels* multiply by it;
    /// everything normalized to `[0, 1]` ignores it.
    pub scale: f32,
}

/// One frozen version of one operator.
pub(crate) struct Version {
    /// Version number, as a revision cites it.
    pub version: u16,
    /// Position in the pipeline; lower runs first. A property of the
    /// version, not of the operator (ADR 0042 §3).
    pub rank: u16,
    /// The rendering itself.
    pub apply: fn(&mut Pixels, &Context<'_>),
}

/// One operator, with every version of it this engine can still render.
pub(crate) struct Stage {
    /// Name a revision cites in its `stages` map.
    pub name: &'static str,
    /// Whether this operator's settings are away from their neutral value.
    ///
    /// A property of the operator and of the *settings alone*: it decides
    /// both what runs and what a revision records, and a revision is written
    /// without a decoded image, a shot or a resolved profile in hand.
    /// Runtime availability — no EXIF match, no profile — is the `apply`
    /// function's business, and leaves the pixels untouched there.
    pub active: fn(&Settings) -> bool,
    /// Versions, oldest first.
    pub versions: &'static [Version],
}

impl Stage {
    /// The version this engine pins when the stage leaves its neutral value
    /// for the first time (ADR 0043 §3).
    fn current(&self) -> &'static Version {
        self.versions
            .last()
            .expect("a registered stage has at least one version")
    }
}

/// The stage registry. Published `(name, version)` entries are frozen —
/// see the module docs.
pub(crate) static STAGES: &[Stage] = &[
    Stage {
        name: "camera_profile",
        active: |settings| {
            settings
                .camera_profile
                .as_ref()
                .is_some_and(|profile| profile.enabled)
        },
        versions: &[Version {
            version: 1,
            rank: 10,
            apply: |px, ctx| {
                if let Some(profile) = ctx.camera_profile {
                    camera_profile::v1::apply_camera_profile(px, profile);
                }
            },
        }],
    },
    Stage {
        name: "lens",
        active: |settings| settings.lens_correction.enabled,
        versions: &[Version {
            version: 1,
            rank: 20,
            apply: |px, ctx| {
                let Some(shot) = ctx.shot else { return };
                if let Some(profile) = leyline_lens::find_profile(
                    &shot.camera_make,
                    &shot.camera_model,
                    shot.lens_make.as_deref(),
                    shot.lens_model.as_deref().unwrap_or(""),
                ) {
                    let correction =
                        leyline_lens::Correction::new(&profile, shot.focal_mm, px.width, px.height);
                    *px = lens::v1::undistort(px, &correction);
                    *px = lens::v1::correct_tca(px, &correction);
                    if let Some(aperture_f) = shot.aperture_f {
                        lens::v1::devignette(px, &profile, shot.focal_mm, aperture_f);
                    }
                }
            },
        }],
    },
    Stage {
        name: "spot_removal",
        active: |settings| !settings.spot_removal.is_empty(),
        versions: &[Version {
            version: 1,
            rank: 30,
            apply: |px, ctx| {
                spot_removal::v1::spot_removal(
                    px,
                    &ctx.settings.spot_removal,
                    ctx.settings.rotation,
                );
            },
        }],
    },
    Stage {
        name: "gains",
        active: |settings| settings.white_balance.is_some() || settings.exposure != 0.0,
        versions: &[Version {
            version: 1,
            rank: 40,
            apply: |px, ctx| {
                gains::v1::linear_gains(
                    px,
                    ctx.settings.white_balance.as_ref(),
                    ctx.settings.exposure,
                );
            },
        }],
    },
    Stage {
        name: "contrast",
        active: |settings| settings.contrast != 0,
        versions: &[Version {
            version: 1,
            rank: 50,
            apply: |px, ctx| contrast::v1::contrast(px, ctx.settings.contrast),
        }],
    },
    Stage {
        name: "highlights_shadows",
        active: |settings| settings.highlights != 0 || settings.shadows != 0,
        versions: &[Version {
            version: 1,
            rank: 60,
            apply: |px, ctx| {
                highlights_shadows::v1::highlights_shadows(
                    px,
                    ctx.settings.highlights,
                    ctx.settings.shadows,
                );
            },
        }],
    },
    Stage {
        name: "whites_blacks",
        active: |settings| settings.whites != 0 || settings.blacks != 0,
        versions: &[Version {
            version: 1,
            rank: 70,
            apply: |px, ctx| {
                whites_blacks::v1::whites_blacks(px, ctx.settings.whites, ctx.settings.blacks);
            },
        }],
    },
    Stage {
        name: "tone_curve",
        active: |settings| !settings.tone_curve.points.is_empty(),
        versions: &[Version {
            version: 1,
            rank: 80,
            apply: |px, ctx| tone_curve::v1::tone_curve(px, &ctx.settings.tone_curve.points),
        }],
    },
    Stage {
        name: "clarity",
        active: |settings| settings.clarity != 0,
        versions: &[Version {
            version: 1,
            rank: 90,
            apply: |px, ctx| {
                kernel::v1::local_contrast(
                    px,
                    ctx.settings.clarity,
                    clarity::v1::CLARITY_RADIUS * ctx.scale,
                );
            },
        }],
    },
    Stage {
        name: "texture",
        active: |settings| settings.texture != 0,
        versions: &[Version {
            version: 1,
            rank: 100,
            apply: |px, ctx| {
                kernel::v1::local_contrast(
                    px,
                    ctx.settings.texture,
                    texture::v1::TEXTURE_RADIUS * ctx.scale,
                );
            },
        }],
    },
    Stage {
        name: "dehaze",
        active: |settings| settings.dehaze != 0,
        versions: &[Version {
            version: 1,
            rank: 110,
            apply: |px, ctx| dehaze::v1::dehaze(px, ctx.settings.dehaze, ctx.scale),
        }],
    },
    // Vibrance and saturation are one body ([`kernel::v1::saturate`]) bound
    // twice, at two ranks: the `vibrance` flag is what tells them apart.
    Stage {
        name: "vibrance",
        active: |settings| settings.vibrance != 0,
        versions: &[Version {
            version: 1,
            rank: 120,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.vibrance, true),
        }],
    },
    Stage {
        name: "saturation",
        active: |settings| settings.saturation != 0,
        versions: &[Version {
            version: 1,
            rank: 130,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.saturation, false),
        }],
    },
    Stage {
        name: "hsl",
        active: |settings| settings.hsl.iter().any(|band| *band != HslBand::default()),
        versions: &[Version {
            version: 1,
            rank: 140,
            apply: |px, ctx| hsl::v1::hsl_mixer(px, &ctx.settings.hsl),
        }],
    },
    Stage {
        name: "color_grading",
        active: |settings| settings.color_grading != ColorGrading::default(),
        versions: &[Version {
            version: 1,
            rank: 150,
            apply: |px, ctx| color_grading::v1::color_grading(px, &ctx.settings.color_grading),
        }],
    },
    Stage {
        name: "local_adjustments",
        active: |settings| !settings.local_adjustments.is_empty(),
        versions: &[Version {
            version: 1,
            rank: 160,
            apply: |px, ctx| {
                local_adjustments::v1::local_adjustments(
                    px,
                    &ctx.settings.local_adjustments,
                    ctx.settings.rotation,
                );
            },
        }],
    },
    Stage {
        name: "noise_luminance",
        active: |settings| settings.noise_reduction.luminance != 0,
        versions: &[Version {
            version: 1,
            rank: 170,
            apply: |px, ctx| {
                noise_luminance::v1::luminance_noise_reduction(
                    px,
                    ctx.settings.noise_reduction.luminance,
                    ctx.scale,
                );
            },
        }],
    },
    Stage {
        name: "noise_color",
        active: |settings| settings.noise_reduction.color != 0,
        versions: &[Version {
            version: 1,
            rank: 180,
            apply: |px, ctx| {
                noise_color::v1::color_noise_reduction(
                    px,
                    ctx.settings.noise_reduction.color,
                    ctx.scale,
                );
            },
        }],
    },
    Stage {
        name: "sharpen",
        active: |settings| settings.sharpening.amount != 0,
        versions: &[Version {
            version: 1,
            rank: 190,
            apply: |px, ctx| {
                sharpen::v1::sharpen(
                    px,
                    ctx.settings.sharpening.amount,
                    ctx.settings.sharpening.radius * f64::from(ctx.scale),
                );
            },
        }],
    },
    Stage {
        name: "rotate",
        active: |settings| settings.rotation.rem_euclid(360.0) != 0.0,
        versions: &[Version {
            version: 1,
            rank: 200,
            apply: |px, ctx| *px = rotate::v1::rotate(px, ctx.settings.rotation),
        }],
    },
    Stage {
        name: "crop",
        active: |settings| settings.crop.is_some(),
        versions: &[Version {
            version: 1,
            rank: 210,
            apply: |px, ctx| {
                if let Some(rect) = &ctx.settings.crop {
                    *px = crop::v1::crop(px, rect);
                }
            },
        }],
    },
];

/// Stages this engine implements. In `cfg(test)` builds it also carries the
/// two-version fixture stage of ADR 0043 §7, which keeps the "an older
/// version still renders what it rendered" path exercised while every real
/// operator sits at v1.
fn registry() -> impl Iterator<Item = &'static Stage> {
    STAGES.iter().chain(extra_stages())
}

#[cfg(test)]
fn extra_stages() -> &'static [Stage] {
    fixture::STAGES
}

#[cfg(not(test))]
fn extra_stages() -> &'static [Stage] {
    &[]
}

/// Looks a `(name, version)` pair up in the registry.
fn find(name: &str, version: u16) -> Option<(&'static Stage, &'static Version)> {
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

/// Records, in `settings`, the version of every stage it activates
/// (ADR 0043 §3). Called on the way *in* to a revision, never on the way out.
///
/// A stage already recorded keeps its version — that is the whole promise.
/// A stage that just left its neutral value gets this engine's current
/// version. A stage back at its neutral value loses its entry, since it no
/// longer renders anything to pin.
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
            .unwrap_or_else(|| stage.current().version);
        pinned.insert(stage.name.to_owned(), version);
    }
    settings.stages = pinned;
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
fn plan(settings: &Settings) -> Result<Vec<(&'static Stage, &'static Version)>> {
    check_known(settings)?;
    let mut plan: Vec<_> = registry()
        .filter(|stage| (stage.active)(settings))
        .map(|stage| {
            let version = match settings.stages.get(stage.name) {
                Some(&recorded) => {
                    find(stage.name, recorded)
                        .expect("every recorded stage was checked just above")
                        .1
                }
                None => stage.current(),
            };
            (stage, version)
        })
        .collect();
    plan.sort_by_key(|(_, version)| version.rank);
    Ok(plan)
}

/// Renders a decoded image through the stages its settings record.
/// `settings` has already been validated and checked against
/// `CURRENT_SCHEMA` by [`crate::render::render_scaled`].
pub(crate) fn develop_scaled(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&DcpProfile>,
    scale: f32,
) -> Result<Rendered> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        camera_profile,
        scale,
    };
    let mut px = Pixels::from_raw(image)?;
    for (_, version) in plan {
        (version.apply)(&mut px, &ctx);
    }
    Ok(Rendered {
        width: px.width,
        height: px.height,
        data: px.to_rgb8(),
    })
}

#[cfg(test)]
mod fixture;

#[cfg(test)]
mod tests;
