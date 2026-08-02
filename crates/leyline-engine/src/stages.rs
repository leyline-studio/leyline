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
//! What proves all of this is not review but [`golden`], which pins the
//! digest of one render per operator family (ADR 0042 §7) *together with the
//! stage versions it rendered through* — so a new version adds an entry
//! there instead of moving the existing ones.
//!
//! # Rank, not order of statements
//!
//! Every stage version declares its own position in the pipeline
//! ([`Version::rank`], in tens so a later stage can be inserted between two
//! existing ones). Moving a stage is therefore a new version that declares
//! another rank, never an edit of an existing one — revisions citing the old
//! version keep the old position.
//!
//! # Neutral stages, and the two that are never neutral
//!
//! A stage whose setting sits at its neutral value does not run at all
//! ([`Stage::active`]) — the rule that makes a neutral rendering bit-for-bit
//! the decoded image. It is also why a neutral stage has no behavior to pin
//! and does not need to appear in a revision.
//!
//! `input` and `output_rendering` are the exceptions (ADR 0044 §3): a
//! rendering without an entry or an exit is not a neutral rendering, it is
//! an incomplete one. They run and are recorded always, which is what lets a
//! revision say — without any global counter — where its pixels came from
//! and which working space they travelled in.
//!
//! # Working space
//!
//! Each version declares the buffer it reads and writes ([`Version::space`]).
//! Two spaces never compose, so a plan mixing them is refused
//! ([`LeylineError::MixedWorkingSpaces`]) instead of rendered at best, and a
//! stage entering a revision takes the current version *of that revision's
//! space* ([`pinned_version`]). Everything published so far renders in
//! gamma-encoded sRGB; moving the pipeline to linear Rec. 2020 is what
//! ADR 0044 §7 step 2 does, one new version per operator.

pub(crate) mod kernel {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod input {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
}
pub(crate) mod camera_profile {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
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
pub(crate) mod lut {
    pub(crate) mod v1;
}
pub(crate) mod local_adjustments {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod noise_luminance {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod noise_color {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod sharpen {
    pub(crate) mod v1;
}
pub(crate) mod rotate {
    pub(crate) mod v1;
}
pub(crate) mod perspective {
    pub(crate) mod v1;
}
pub(crate) mod crop {
    pub(crate) mod v1;
}
pub(crate) mod output_rendering {
    pub(crate) mod v1;
}

use leyline_color::DcpProfile;
use leyline_core::{ColorGrading, HslBand, LeylineError, Result, Settings};
use leyline_raw::{DecodeParams, RawImage};

use crate::pixels::Pixels;
use crate::render::{LensShot, Rendered};

/// What the decoder handed over, colorimetrically — the `input` stage's
/// other half (ADR 0044 §3).
///
/// It is not a setting: the same revision renders through the same stage
/// versions whatever the file is. It is a property of the source, resolved
/// by whoever opened it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SourceColor {
    /// A RAW decoded camera-native, with the body's XYZ→camera matrix when
    /// LibRaw knows one for it
    /// ([`leyline_raw::RawMetadata::camera_to_xyz`]).
    Camera {
        /// The body's XYZ→camera matrix, `None` for an unknown body.
        to_xyz: Option<leyline_color::Matrix3>,
        /// The body's as-shot channel multipliers
        /// ([`leyline_raw::RawMetadata::camera_multipliers`]), `None` when
        /// the file records none.
        ///
        /// Only one stage version reads them, and for one purpose: undoing
        /// the global gain change the decoder applies when it is asked to
        /// reconstruct highlights instead of clipping them (ADR 0050 §3).
        multipliers: Option<[f64; 4]>,
    },
    /// A JPEG, PNG or TIFF: gamma-encoded sRGB from a native codec.
    Srgb,
}

/// Everything a stage may read besides the buffer it renders.
pub(crate) struct Context<'a> {
    /// What the decoder produced, colorimetrically (ADR 0044 §3).
    pub source: SourceColor,
    /// The revision being rendered, already validated by the caller.
    pub settings: &'a Settings,
    /// EXIF identification of the shot, for the lens stage.
    pub shot: Option<&'a LensShot>,
    /// The already-resolved DCP matrix, for the camera profile stage.
    pub camera_profile: Option<&'a DcpProfile>,
    /// The already-parsed creative LUT, for the LUT stage (ADR 0053).
    pub lut: Option<&'a leyline_color::CubeLut>,
    /// Factor by which the image was already reduced for a preview
    /// (ADR 0041). Stages expressing a radius in *pixels* multiply by it;
    /// everything normalized to `[0, 1]` ignores it.
    pub scale: f32,
}

/// The pixel encoding a stage version reads and writes — the working
/// buffer's contract, declared per version (ADR 0044 §4).
///
/// It is a property of the *version* for the same reason the rank is: an
/// operator moved to another space is a new version, and revisions citing
/// the old one keep rendering in the old space. Stages of two different
/// spaces never compose, so a plan mixing them is refused ([`plan`]) rather
/// than rendered "at best".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    /// Gamma-encoded sRGB, clamped to [0, 1] — what the pipeline worked in
    /// until ADR 0044.
    ///
    /// No shipped operator renders here any more: the pre-publication
    /// history was collapsed rather than doubled (ADR 0044 §5), so nothing
    /// cites it. It stays because the *rule* it takes part in outlives it —
    /// the day an operator moves space again, that will be a new version
    /// beside an old one, and the refusal below has to already work. The
    /// test fixture stage is what keeps it exercised meanwhile.
    #[cfg_attr(not(test), allow(dead_code))]
    SrgbGamma,
    /// The working space: Rec. 2020 primaries, D65, linear light, unbounded
    /// above (ADR 0044 §1–2).
    LinearRec2020,
}

impl Space {
    /// How a rendering error names this space.
    fn label(self) -> &'static str {
        match self {
            Space::SrgbGamma => "gamma-encoded sRGB",
            Space::LinearRec2020 => "linear Rec. 2020",
        }
    }
}

/// One frozen version of one operator.
pub(crate) struct Version {
    /// Version number, as a revision cites it.
    pub version: u16,
    /// Position in the pipeline; lower runs first. A property of the
    /// version, not of the operator (ADR 0042 §3).
    pub rank: u16,
    /// The working buffer this version expects and leaves behind
    /// (ADR 0044 §4).
    pub space: Space,
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
    /// Top-level `settings_json` keys this operator's rendering reads
    /// (ADR 0041 §3).
    ///
    /// This is what makes a stage checkpoint possible: a cached buffer
    /// taken after stage *n* stays valid exactly as long as none of the
    /// keys the first *n* stages declare has changed. Hashing the whole of
    /// `Settings` instead would invalidate every checkpoint on every edit,
    /// which is precisely the waste the cache exists to remove.
    ///
    /// **A missing key here is a correctness bug, not a slow render**: the
    /// pipeline would reuse a buffer that the changed setting should have
    /// invalidated, and put wrong pixels on screen. Two entries are less
    /// obvious than the rest and were found by reading the `apply` bodies
    /// rather than the `active` predicates:
    ///
    /// * `input` reads `camera_profile`, because whether a profile is
    ///   resolved changes how it enters the working space;
    /// * `spot_removal` and `local_adjustments` read `rotation`, their
    ///   coordinates being expressed before it.
    ///
    /// `schema` and `stages` are deliberately absent: they are folded into
    /// every fingerprint by [`prefix_fingerprint`], since a change to
    /// either one rebuilds the plan itself.
    pub reads: &'static [&'static str],
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

    /// The newest version of this operator working in `space`, if it has
    /// one — what a revision already committed to a space is allowed to
    /// pin (ADR 0044 §4). A stage that never rendered in `space` returns
    /// `None`, and [`pin`] falls back to [`Stage::current`] so the
    /// incoherence is written down and refused at render time by name,
    /// rather than silently resolved into some other stage's space.
    fn current_in(&self, space: Space) -> Option<&'static Version> {
        self.versions.iter().rev().find(|v| v.space == space)
    }
}

/// The stage registry. Published `(name, version)` entries are frozen —
/// see the module docs.
pub(crate) static STAGES: &[Stage] = &[
    Stage {
        // Always active: there is no rendering without an input, so this
        // stage has no neutral value (ADR 0044 §3). Its version is what
        // pins the *decoder's* configuration — see [`decode_params`] —
        // which changes pixels and was, until ADR 0044, the one input to
        // the rendering that no revision recorded.
        name: "input",
        active: |_| true,
        reads: &["highlight_reconstruction", "camera_profile", "demosaic"],
        versions: &[
            Version {
                version: 1,
                rank: 0,
                space: Space::LinearRec2020,
                // Nothing to do on the buffer: at this version the decoder
                // already hands over gamma-encoded sRGB, and the camera
                // profile stage does the matrix when there is a profile.
                apply: |px, ctx| {
                    input::v1::to_working_space(px, ctx.source, ctx.camera_profile.is_some());
                },
            },
            // Same buffer work, a decoder that is told what to do with
            // clipped highlights (ADR 0050): the difference lives entirely
            // in `INPUT_DECODE` below, which is the other half of what an
            // `input` version pins.
            Version {
                version: 2,
                rank: 0,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    input::v2::to_working_space(
                        px,
                        ctx.source,
                        ctx.camera_profile.is_some(),
                        ctx.settings.highlight_reconstruction,
                    );
                },
            },
            // Same buffer work again, a decoder that is told *which*
            // interpolation to use (ADR 0061). Like `v2` before it, the
            // whole difference lives in `INPUT_DECODE`: `v3` at a neutral
            // `demosaic` renders bit for bit like `v2`, which is what makes
            // reprocessing into it safe to offer.
            Version {
                version: 3,
                rank: 0,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    input::v3::to_working_space(
                        px,
                        ctx.source,
                        ctx.camera_profile.is_some(),
                        ctx.settings.highlight_reconstruction,
                    );
                },
            },
        ],
    },
    Stage {
        name: "camera_profile",
        active: |settings| {
            settings
                .camera_profile
                .as_ref()
                .is_some_and(|profile| profile.enabled)
        },
        // `white_balance` since ADR 0062: `v2` picks the calibration from
        // the scene temperature, so a checkpoint taken before this stage is
        // only valid while that temperature holds. Omitting it here would
        // let the stage cache reuse a buffer rendered under another light —
        // wrong pixels, silently (ADR 0041 §3).
        reads: &["camera_profile", "white_balance"],
        versions: &[
            Version {
                version: 1,
                rank: 10,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    if let Some(profile) = ctx.camera_profile {
                        camera_profile::v1::apply_camera_profile(px, profile);
                    }
                },
            },
            // Same conversion, the calibration interpolated for the light
            // instead of averaged (ADR 0062).
            Version {
                version: 2,
                rank: 10,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    if let Some(profile) = ctx.camera_profile {
                        let temperature = scene_temperature(ctx, profile);
                        camera_profile::v2::apply_camera_profile(px, profile, temperature);
                    }
                },
            },
            // And the profile's tables on top of its matrices (ADR 0063) —
            // the look, where `v2` stopped at the colorimetry.
            Version {
                version: 3,
                rank: 10,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    if let Some(profile) = ctx.camera_profile {
                        let temperature = scene_temperature(ctx, profile);
                        camera_profile::v3::apply_camera_profile(px, profile, temperature);
                    }
                },
            },
        ],
    },
    Stage {
        name: "lens",
        active: |settings| settings.lens_correction.enabled,
        reads: &["lens_correction"],
        versions: &[Version {
            version: 1,
            rank: 20,
            space: Space::LinearRec2020,
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
        reads: &["spot_removal", "rotation"],
        versions: &[Version {
            version: 1,
            rank: 30,
            space: Space::LinearRec2020,
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
        reads: &["exposure", "white_balance"],
        versions: &[Version {
            version: 1,
            rank: 40,
            space: Space::LinearRec2020,
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
        reads: &["contrast"],
        versions: &[Version {
            version: 1,
            rank: 50,
            space: Space::LinearRec2020,
            apply: |px, ctx| contrast::v1::contrast(px, ctx.settings.contrast),
        }],
    },
    Stage {
        name: "highlights_shadows",
        active: |settings| settings.highlights != 0 || settings.shadows != 0,
        reads: &["highlights", "shadows"],
        versions: &[Version {
            version: 1,
            rank: 60,
            space: Space::LinearRec2020,
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
        reads: &["blacks", "whites"],
        versions: &[Version {
            version: 1,
            rank: 70,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                whites_blacks::v1::whites_blacks(px, ctx.settings.whites, ctx.settings.blacks);
            },
        }],
    },
    Stage {
        name: "tone_curve",
        active: |settings| !settings.tone_curve.points.is_empty(),
        reads: &["tone_curve"],
        versions: &[Version {
            version: 1,
            rank: 80,
            space: Space::LinearRec2020,
            apply: |px, ctx| tone_curve::v1::tone_curve(px, &ctx.settings.tone_curve.points),
        }],
    },
    Stage {
        name: "clarity",
        active: |settings| settings.clarity != 0,
        reads: &["clarity"],
        versions: &[Version {
            version: 1,
            rank: 90,
            space: Space::LinearRec2020,
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
        reads: &["texture"],
        versions: &[Version {
            version: 1,
            rank: 100,
            space: Space::LinearRec2020,
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
        reads: &["dehaze"],
        versions: &[Version {
            version: 1,
            rank: 110,
            space: Space::LinearRec2020,
            apply: |px, ctx| dehaze::v1::dehaze(px, ctx.settings.dehaze, ctx.scale),
        }],
    },
    // Vibrance and saturation are one body ([`kernel::v1::saturate`]) bound
    // twice, at two ranks: the `vibrance` flag is what tells them apart.
    Stage {
        name: "vibrance",
        active: |settings| settings.vibrance != 0,
        reads: &["vibrance"],
        versions: &[Version {
            version: 1,
            rank: 120,
            space: Space::LinearRec2020,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.vibrance, true),
        }],
    },
    Stage {
        name: "saturation",
        active: |settings| settings.saturation != 0,
        reads: &["saturation"],
        versions: &[Version {
            version: 1,
            rank: 130,
            space: Space::LinearRec2020,
            apply: |px, ctx| kernel::v1::saturate(px, ctx.settings.saturation, false),
        }],
    },
    Stage {
        name: "hsl",
        active: |settings| settings.hsl.iter().any(|band| *band != HslBand::default()),
        reads: &["hsl"],
        versions: &[Version {
            version: 1,
            rank: 140,
            space: Space::LinearRec2020,
            apply: |px, ctx| hsl::v1::hsl_mixer(px, &ctx.settings.hsl),
        }],
    },
    Stage {
        name: "color_grading",
        active: |settings| settings.color_grading != ColorGrading::default(),
        reads: &["color_grading"],
        versions: &[Version {
            version: 1,
            rank: 150,
            space: Space::LinearRec2020,
            apply: |px, ctx| color_grading::v1::color_grading(px, &ctx.settings.color_grading),
        }],
    },
    Stage {
        name: "local_adjustments",
        active: |settings| !settings.local_adjustments.is_empty(),
        reads: &["local_adjustments", "rotation"],
        versions: &[
            Version {
                version: 1,
                rank: 160,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    local_adjustments::v1::local_adjustments(
                        px,
                        &ctx.settings.local_adjustments,
                        ctx.settings.rotation,
                    );
                },
            },
            // Range masks (ADR 0048): an entry without a `range` renders
            // exactly as v1 renders it.
            Version {
                version: 2,
                rank: 160,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    local_adjustments::v2::local_adjustments(
                        px,
                        &ctx.settings.local_adjustments,
                        ctx.settings.rotation,
                    );
                },
            },
        ],
    },
    Stage {
        // The last color decision, after every operator that grades the image
        // and before the ones that work on local structure (ADR 0053 §4).
        //
        // `active` reads the settings only, like every predicate here: whether
        // the file resolves is `apply`'s problem, and a reference that cannot be
        // read is an error rather than a silently skipped stage.
        name: "lut",
        active: |settings| settings.lut.as_ref().is_some_and(|lut| lut.enabled),
        reads: &["lut"],
        versions: &[Version {
            version: 1,
            rank: 165,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                if let (Some(lut), Some(reference)) = (ctx.lut, ctx.settings.lut.as_ref()) {
                    lut::v1::apply(px, lut, reference.strength);
                }
            },
        }],
    },
    Stage {
        name: "noise_luminance",
        active: |settings| settings.noise_reduction.luminance != 0,
        reads: &["noise_reduction"],
        versions: &[
            Version {
                version: 1,
                rank: 170,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    noise_luminance::v1::luminance_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.luminance,
                        ctx.scale,
                    );
                },
            },
            // Edge-preserving, same slider and same rank (ADR 0046).
            Version {
                version: 2,
                rank: 170,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    noise_luminance::v2::luminance_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.luminance,
                        ctx.scale,
                    );
                },
            },
        ],
    },
    Stage {
        name: "noise_color",
        active: |settings| settings.noise_reduction.color != 0,
        reads: &["noise_reduction"],
        versions: &[
            Version {
                version: 1,
                rank: 180,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    noise_color::v1::color_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.color,
                        ctx.scale,
                    );
                },
            },
            // Edge-preserving, same slider and same rank (ADR 0046).
            Version {
                version: 2,
                rank: 180,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    noise_color::v2::color_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.color,
                        ctx.scale,
                    );
                },
            },
        ],
    },
    Stage {
        name: "sharpen",
        active: |settings| settings.sharpening.amount != 0,
        reads: &["sharpening"],
        versions: &[Version {
            version: 1,
            rank: 190,
            space: Space::LinearRec2020,
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
        reads: &["rotation"],
        versions: &[Version {
            version: 1,
            rank: 200,
            space: Space::LinearRec2020,
            apply: |px, ctx| *px = rotate::v1::rotate(px, ctx.settings.rotation),
        }],
    },
    Stage {
        // Between rotation and crop, and constrained on both sides: a level
        // horizon is what makes "vertical" meaningful, and one crops what one
        // sees (ADR 0052 §3).
        name: "perspective",
        active: |settings| {
            settings
                .perspective
                .is_some_and(|p| p.vertical != 0 || p.horizontal != 0)
        },
        reads: &["perspective"],
        versions: &[Version {
            version: 1,
            rank: 205,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                if let Some(perspective) = &ctx.settings.perspective {
                    *px = perspective::v1::correct(px, perspective);
                }
            },
        }],
    },
    Stage {
        name: "crop",
        active: |settings| settings.crop.is_some(),
        reads: &["crop"],
        versions: &[Version {
            version: 1,
            rank: 210,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                if let Some(rect) = &ctx.settings.crop {
                    *px = crop::v1::crop(px, rect);
                }
            },
        }],
    },
    Stage {
        // Always active, like `input` and for the same reason: the buffer
        // has to become a display signal, and "not converting" is not a
        // neutral value but a missing step (ADR 0044 §3).
        name: "output_rendering",
        active: |_| true,
        reads: &["output_rendering"],
        versions: &[Version {
            version: 1,
            rank: 900,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                output_rendering::v1::render_output(
                    px,
                    ctx.settings.output_rendering.highlight_rolloff,
                );
            },
        }],
    },
];

/// How the decoder must be configured, per published `input` version.
///
/// Kept beside [`STAGES`] and frozen exactly like it: an `input` version
/// freezes two things together — what the decoder is asked for, and what the
/// stage then does to the buffer — because both change pixels and a revision
/// cites a single number for them.
static INPUT_DECODE: &[(u16, DecodeConfig)] = &[
    (1, input::v1::decode_params),
    (2, input::v2::decode_params),
    (3, input::v3::decode_params),
];

/// What one `input` version asks the decoder for.
///
/// `half_size` is the caller's size class (ADR 0041), the only thing that
/// varies between two renders of the same revision. The settings arrive
/// because a decoder configuration can itself be a stored choice — the
/// highlight mode of ADR 0050 is one — and each version decides which of
/// them it reads; `v1` reads none, which is what keeps it frozen.
type DecodeConfig = fn(&Settings, bool) -> DecodeParams;

/// The decoder configuration a revision's `input` version calls for
/// (ADR 0044 §3).
///
/// This is the call site that used to read `camera_native:
/// camera_profile.is_some()` in four modules, deciding a pixel-affecting
/// parameter that no `stages` map recorded. It now comes from the version
/// the revision cites, like every other part of its rendering.
pub(crate) fn decode_params(settings: &Settings, half_size: bool) -> DecodeParams {
    let version = version_of("input", settings);
    let decode = INPUT_DECODE
        .iter()
        .find(|(v, _)| *v == version.version)
        .map(|(_, decode)| decode)
        .expect("every published input version has a decoder configuration");
    decode(settings, half_size)
}

/// The version of `stage_name` this render uses: the one `settings` records,
/// or the current one compatible with the space it is already committed to.
///
/// Only meaningful for the stages that are always active; a caller asking
/// about a stage absent from the registry gets a panic, since the name is
/// always a literal from this module.
fn version_of(stage_name: &'static str, settings: &Settings) -> &'static Version {
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

/// The working space `settings` is already committed to (ADR 0044 §4).
///
/// Read off the versions it records, since the space is a property of those
/// and never a field of its own: a revision recording nothing yet — fresh
/// settings, a preset, a test — gets the space this engine renders in today.
/// A revision whose records disagree resolves to the first of them here and
/// is refused by [`plan`], which can name both sides.
fn revision_space(settings: &Settings) -> Space {
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
fn pinned_version(stage: &'static Stage, settings: &Settings) -> &'static Version {
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
fn single_space(
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
fn scene_temperature(ctx: &Context<'_>, profile: &DcpProfile) -> f64 {
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

/// Ranks a checkpoint is taken *before* (ADR 0041 §3), one snapshot each.
///
/// They sit where the ADR put them — after lens correction and spot removal
/// (expensive, almost never touched in a burst of slider moves), after the
/// tonal block, after clarity/texture/dehaze, and after local adjustments —
/// expressed as the rank of the stage that follows, so a checkpoint lands
/// in the right place whether or not the stages around it are active.
const CHECKPOINT_BEFORE_RANK: &[u16] = &[40, 90, 120, 165];

/// Intermediate buffers of the preview pipeline (ADR 0041 §3).
///
/// Without it every render restarts from the decoded image, so nudging
/// `sharpening` — the last stage — replays dehaze, clarity, texture, HSL
/// and every local adjustment to produce the exact pixels they produced a
/// moment earlier. That is the structural difference with Lightroom,
/// Capture One and darktable, which only replay downstream of the edited
/// node.
///
/// Purely derived: dropping it at any moment changes no pixel, only the
/// time taken. That is what makes it safe, and why it needs no stage
/// version and touches no part of `docs/pipeline.md` §5.
///
/// Preview path only. Export and print render at full resolution, where
/// these buffers would cost hundreds of megabytes to spare a single
/// render that happens once.
#[derive(Debug, Default)]
pub(crate) struct StageCache {
    /// What the buffers were rendered from: asset and proxy scale. Neither
    /// is a setting, so neither is covered by [`prefix_fingerprint`] — a
    /// change here invalidates everything at once.
    context: Option<(leyline_core::AssetId, u32)>,
    checkpoints: Vec<Checkpoint>,
}

/// One intermediate buffer, and what it is only valid for.
#[derive(Debug)]
struct Checkpoint {
    /// How many stages of the plan had been applied when it was taken.
    applied: usize,
    /// [`prefix_fingerprint`] over exactly those stages.
    fingerprint: u64,
    pixels: Pixels,
}

impl StageCache {
    /// Forgets everything rendered for another asset or another proxy
    /// scale.
    fn retarget(&mut self, asset: leyline_core::AssetId, scale: f32) {
        let context = (asset, scale.to_bits());
        if self.context != Some(context) {
            self.context = Some(context);
            self.checkpoints.clear();
        }
    }

    /// The deepest checkpoint still valid for `settings`, if any.
    ///
    /// Deepest wins: it is the one that skips the most work. A checkpoint
    /// whose fingerprint no longer matches is dropped rather than kept —
    /// the settings that produced it are gone, and nothing will bring
    /// them back within this session.
    fn resume_from(
        &mut self,
        settings: &Settings,
        plan: &[(&'static Stage, &'static Version)],
    ) -> Option<(usize, Pixels)> {
        self.checkpoints
            .retain(|c| c.fingerprint == prefix_fingerprint(settings, plan, c.applied));
        self.checkpoints
            .iter()
            .max_by_key(|c| c.applied)
            .map(|c| (c.applied, c.pixels.clone()))
    }

    /// Records a buffer taken after `applied` stages, replacing any
    /// checkpoint already held at that depth.
    fn store(
        &mut self,
        settings: &Settings,
        plan: &[(&'static Stage, &'static Version)],
        applied: usize,
        pixels: &Pixels,
    ) {
        let fingerprint = prefix_fingerprint(settings, plan, applied);
        self.checkpoints.retain(|c| c.applied != applied);
        self.checkpoints.push(Checkpoint {
            applied,
            fingerprint,
            pixels: pixels.clone(),
        });
    }
}

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
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    source: SourceColor,
    scale: f32,
    asset: leyline_core::AssetId,
    cache: &mut StageCache,
) -> Result<Rendered> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        camera_profile,
        lut,
        source,
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

    cache.retarget(asset, scale);
    let (mut applied, mut px) = match cache.resume_from(settings, &plan) {
        Some((applied, pixels)) => (applied, pixels),
        None => (0, Pixels::from_raw(image)?),
    };

    while applied < plan.len() {
        // Snapshot before the stage that opens a block, not after the one
        // that closed it: what matters is that the buffer entering an
        // expensive run is kept, whatever ran before it.
        if cuts.contains(&applied) {
            cache.store(settings, &plan, applied, &px);
        }
        (plan[applied].1.apply)(&mut px, &ctx);
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
pub(crate) fn develop_scaled(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    source: SourceColor,
    scale: f32,
) -> Result<Rendered> {
    let plan = plan(settings)?;
    let ctx = Context {
        settings,
        shot,
        camera_profile,
        lut,
        source,
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
mod golden;

#[cfg(test)]
mod tests;
