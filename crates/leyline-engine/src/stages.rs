//! The develop pipeline as a composition of independently versioned stages
//! (ADR 0042).
//!
//! Until ADR 0042 the pipeline was eleven `processN.rs` modules, each a full
//! copy of the previous one — 14 968 lines, 70 to 93 % of them identical to
//! their neighbor. This module replaces them by the operators that actually
//! differ, plus a table saying which version of each operator a given
//! `process: N` expands to.
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
//! which pins the digest of one render per process version, captured from
//! the pre-migration engine (ADR 0042 §7).
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
    pub(crate) mod v2;
    pub(crate) mod v3;
}
pub(crate) mod spot_removal {
    pub(crate) mod v1;
}
pub(crate) mod gains {
    pub(crate) mod v1;
    pub(crate) mod v2;
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
    /// Name a revision cites, and key of the expansion table.
    pub name: &'static str,
    /// Whether this operator's settings are away from their neutral value.
    /// A property of the operator, shared by all its versions: neutrality
    /// is about the settings, not about how they are rendered.
    pub active: fn(&Context<'_>) -> bool,
    /// Versions, oldest first.
    pub versions: &'static [Version],
}

/// The stage registry. Published `(name, version)` entries are frozen —
/// see the module docs.
pub(crate) static STAGES: &[Stage] = &[
    Stage {
        name: "camera_profile",
        active: |ctx| ctx.camera_profile.is_some(),
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
        active: |ctx| ctx.settings.lens_correction.enabled && ctx.shot.is_some(),
        versions: &[
            Version {
                version: 1,
                rank: 20,
                apply: |px, ctx| {
                    if let Some(shot) = ctx.shot {
                        *px = lens::v1::correct_lens(px, shot);
                    }
                },
            },
            Version {
                version: 2,
                rank: 20,
                apply: |px, ctx| {
                    let Some(shot) = ctx.shot else { return };
                    if let Some(profile) = leyline_lens::find_profile(
                        &shot.camera_make,
                        &shot.camera_model,
                        shot.lens_make.as_deref(),
                        shot.lens_model.as_deref().unwrap_or(""),
                    ) {
                        *px = lens::v2::undistort(px, &profile, shot.focal_mm);
                        if let Some(aperture_f) = shot.aperture_f {
                            lens::v2::devignette(px, &profile, shot.focal_mm, aperture_f);
                        }
                    }
                },
            },
            Version {
                version: 3,
                rank: 20,
                apply: |px, ctx| {
                    let Some(shot) = ctx.shot else { return };
                    if let Some(profile) = leyline_lens::find_profile(
                        &shot.camera_make,
                        &shot.camera_model,
                        shot.lens_make.as_deref(),
                        shot.lens_model.as_deref().unwrap_or(""),
                    ) {
                        let correction = leyline_lens::Correction::new(
                            &profile,
                            shot.focal_mm,
                            px.width,
                            px.height,
                        );
                        *px = lens::v3::undistort(px, &correction);
                        *px = lens::v3::correct_tca(px, &correction);
                        if let Some(aperture_f) = shot.aperture_f {
                            lens::v3::devignette(px, &profile, shot.focal_mm, aperture_f);
                        }
                    }
                },
            },
        ],
    },
    Stage {
        name: "spot_removal",
        active: |ctx| !ctx.settings.spot_removal.is_empty(),
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
        active: |ctx| ctx.settings.white_balance.is_some() || ctx.settings.exposure != 0.0,
        versions: &[
            Version {
                version: 1,
                rank: 40,
                apply: |px, ctx| {
                    gains::v1::linear_gains(
                        px,
                        ctx.settings.white_balance.as_ref(),
                        ctx.settings.exposure,
                    );
                },
            },
            Version {
                version: 2,
                rank: 40,
                apply: |px, ctx| {
                    gains::v2::linear_gains(
                        px,
                        ctx.settings.white_balance.as_ref(),
                        ctx.settings.exposure,
                    );
                },
            },
        ],
    },
    Stage {
        name: "contrast",
        active: |ctx| ctx.settings.contrast != 0,
        versions: &[Version {
            version: 1,
            rank: 50,
            apply: |px, ctx| contrast::v1::contrast(px, ctx.settings.contrast),
        }],
    },
    Stage {
        name: "highlights_shadows",
        active: |ctx| ctx.settings.highlights != 0 || ctx.settings.shadows != 0,
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
        active: |ctx| ctx.settings.whites != 0 || ctx.settings.blacks != 0,
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
        active: |ctx| !ctx.settings.tone_curve.points.is_empty(),
        versions: &[Version {
            version: 1,
            rank: 80,
            apply: |px, ctx| tone_curve::v1::tone_curve(px, &ctx.settings.tone_curve.points),
        }],
    },
    Stage {
        name: "clarity",
        active: |ctx| ctx.settings.clarity != 0,
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
        active: |ctx| ctx.settings.texture != 0,
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
        active: |ctx| ctx.settings.dehaze != 0,
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
        active: |ctx| ctx.settings.vibrance != 0,
        versions: &[Version {
            version: 1,
            rank: 120,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.vibrance, true),
        }],
    },
    Stage {
        name: "saturation",
        active: |ctx| ctx.settings.saturation != 0,
        versions: &[Version {
            version: 1,
            rank: 130,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.saturation, false),
        }],
    },
    Stage {
        name: "hsl",
        active: |ctx| {
            ctx.settings
                .hsl
                .iter()
                .any(|band| *band != HslBand::default())
        },
        versions: &[Version {
            version: 1,
            rank: 140,
            apply: |px, ctx| hsl::v1::hsl_mixer(px, &ctx.settings.hsl),
        }],
    },
    Stage {
        name: "color_grading",
        active: |ctx| ctx.settings.color_grading != ColorGrading::default(),
        versions: &[Version {
            version: 1,
            rank: 150,
            apply: |px, ctx| color_grading::v1::color_grading(px, &ctx.settings.color_grading),
        }],
    },
    Stage {
        name: "local_adjustments",
        active: |ctx| !ctx.settings.local_adjustments.is_empty(),
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
        active: |ctx| ctx.settings.noise_reduction.luminance != 0,
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
        active: |ctx| ctx.settings.noise_reduction.color != 0,
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
        active: |ctx| ctx.settings.sharpening.amount != 0,
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
        active: |ctx| ctx.settings.rotation.rem_euclid(360.0) != 0.0,
        versions: &[Version {
            version: 1,
            rank: 200,
            apply: |px, ctx| *px = rotate::v1::rotate(px, ctx.settings.rotation),
        }],
    },
    Stage {
        name: "crop",
        active: |ctx| ctx.settings.crop.is_some(),
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

/// The expansion of every historical `process: N` into the stage versions
/// it is made of (ADR 0042 §5).
///
/// **Frozen, and load-bearing.** A wrong entry would render an old photo
/// differently — the one failure mode this whole migration exists to
/// prevent. Entry `i` is process `i + 1`; order inside a row is irrelevant
/// (rank decides), and a stage a version never rendered is simply absent:
/// processes 1 and 2 carry no `lens` entry because they declare lens
/// correction and deliberately ignore it.
const PROCESS_STAGES: [&[(&str, u16)]; 11] = [
    // 1 — the first contract: exact `powf` transfer functions.
    &[
        ("gains", 1),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 2 — ADR 0013: transfer functions by lookup table.
    &[
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 3 — ADR 0016: lens distortion.
    &[
        ("lens", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 4 — ADR 0017: vignetting.
    &[
        ("lens", 2),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 5 — ADR 0018: transverse chromatic aberration.
    &[
        ("lens", 3),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 6 — ADR 0024: tone curve.
    &[
        ("lens", 3),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 7 — ADR 0031: spot removal.
    &[
        ("lens", 3),
        ("spot_removal", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 8 — ADR 0029: local adjustments.
    &[
        ("lens", 3),
        ("spot_removal", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("local_adjustments", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 9 — ADR 0032: HSL mixer and color grading.
    &[
        ("lens", 3),
        ("spot_removal", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("hsl", 1),
        ("color_grading", 1),
        ("local_adjustments", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 10 — ADR 0033: clarity, texture, dehaze.
    &[
        ("lens", 3),
        ("spot_removal", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("clarity", 1),
        ("texture", 1),
        ("dehaze", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("hsl", 1),
        ("color_grading", 1),
        ("local_adjustments", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
    // 11 — ADR 0035: DCP camera profiles.
    &[
        ("camera_profile", 1),
        ("lens", 3),
        ("spot_removal", 1),
        ("gains", 2),
        ("contrast", 1),
        ("highlights_shadows", 1),
        ("whites_blacks", 1),
        ("tone_curve", 1),
        ("clarity", 1),
        ("texture", 1),
        ("dehaze", 1),
        ("vibrance", 1),
        ("saturation", 1),
        ("hsl", 1),
        ("color_grading", 1),
        ("local_adjustments", 1),
        ("noise_luminance", 1),
        ("noise_color", 1),
        ("sharpen", 1),
        ("rotate", 1),
        ("crop", 1),
    ],
];

/// Looks a `(name, version)` pair up in [`STAGES`].
fn find(name: &str, version: u16) -> Option<(&'static Stage, &'static Version)> {
    let stage = STAGES.iter().find(|stage| stage.name == name)?;
    let version = stage.versions.iter().find(|v| v.version == version)?;
    Some((stage, version))
}

/// The stages of one process version, in the order they run.
///
/// Fails only for a process this engine does not know; the caller has
/// already refused anything newer than `CURRENT_PROCESS`.
fn plan(process: u32) -> Result<Vec<(&'static Stage, &'static Version)>> {
    let expansion = PROCESS_STAGES
        .get(process.checked_sub(1).unwrap_or(u32::MAX) as usize)
        .ok_or_else(|| {
            LeylineError::InvalidSettings(format!("process version {process} does not exist"))
        })?;
    let mut plan: Vec<_> = expansion
        .iter()
        .map(|&(name, version)| {
            find(name, version).expect("the expansion table only names registered stage versions")
        })
        .collect();
    plan.sort_by_key(|(_, version)| version.rank);
    Ok(plan)
}

/// Renders a decoded image through the stages its process version expands
/// to. `settings` has already been validated and checked against
/// `CURRENT_PROCESS` by [`crate::render::render_scaled`].
pub(crate) fn develop_scaled(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&DcpProfile>,
    scale: f32,
) -> Result<Rendered> {
    let plan = plan(settings.process)?;
    let ctx = Context {
        settings,
        shot,
        camera_profile,
        scale,
    };
    let mut px = Pixels::from_raw(image)?;
    for (stage, version) in plan {
        if (stage.active)(&ctx) {
            (version.apply)(&mut px, &ctx);
        }
    }
    Ok(Rendered {
        width: px.width,
        height: px.height,
        data: px.to_rgb8(),
    })
}

#[cfg(test)]
mod tests;
