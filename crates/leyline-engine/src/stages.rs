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
    pub(crate) mod v3;
}
pub(crate) mod input {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
    pub(crate) mod v4;
    pub(crate) mod v5;
    pub(crate) mod v6;
}
pub(crate) mod camera_profile {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
}
pub(crate) mod defringe {
    pub(crate) mod v1;
}
pub(crate) mod lens {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod reshape {
    pub(crate) mod v1;
}
pub(crate) mod red_eye {
    pub(crate) mod v1;
}
pub(crate) mod backdrop {
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
pub(crate) mod parametric_curve {
    pub(crate) mod v1;
}
pub(crate) mod levels {
    pub(crate) mod v1;
}
pub(crate) mod moire {
    pub(crate) mod v1;
}
pub(crate) mod tone_curve {
    pub(crate) mod v1;
    pub(crate) mod v2;
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
pub(crate) mod monochrome {
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
    pub(crate) mod v3;
    pub(crate) mod v4;
    pub(crate) mod v5;
}
pub(crate) mod noise_luminance {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
}
pub(crate) mod noise_color {
    pub(crate) mod v1;
    pub(crate) mod v2;
    pub(crate) mod v3;
}
pub(crate) mod sharpen {
    pub(crate) mod v1;
    pub(crate) mod v2;
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
pub(crate) mod vignette {
    pub(crate) mod v1;
}
pub(crate) mod grain {
    pub(crate) mod v1;
    pub(crate) mod v2;
}
pub(crate) mod output_rendering {
    pub(crate) mod v1;
}

use leyline_color::DcpProfile;
use leyline_core::{ColorGrading, HslBand, LeylineError, Result, Settings};
use leyline_raw::{DecodeParams, RawImage};

use crate::pixels::Pixels;
use crate::render::{LensShot, Rendered, SensorShot};

// What runs the registry below lives beside it; the frozen part (the version
// modules, `STAGES` and the input decode tables) stays in this file.
mod cache;
mod develop;
mod plan;

pub(crate) use cache::*;
pub(crate) use develop::*;
pub use plan::*;

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

/// What the decoder handed over, colorimetrically — [`SourceColor`], plus
/// what the file *said* about itself when it said anything (ADR 0115).
///
/// The two are separate on purpose. Adding a case to `SourceColor` would
/// have forced every frozen `input` version to grow an arm for something it
/// must never read; a second field beside it leaves them untouched, and only
/// `input::v6` looks at it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    /// What kind of samples the decoder produced.
    pub color: SourceColor,
    /// The colour space the file declares, when it declares one this engine
    /// can reduce to primaries and a curve (ADR 0115 §3).
    pub profile: Option<leyline_color::TaggedSource>,
}

impl Source {
    /// A source that says nothing: the convention every untagged file
    /// follows, and what every caller passed before ADR 0115.
    pub fn plain(color: SourceColor) -> Source {
        Source {
            color,
            profile: None,
        }
    }
}

/// Everything a stage may read besides the buffer it renders.
pub(crate) struct Context<'a> {
    /// What the decoder produced, colorimetrically (ADR 0044 §3).
    pub source: SourceColor,
    /// What the file said it was, for the one stage version that reads it
    /// (ADR 0115). `None` for a RAW, for an untagged file, and for every
    /// profile this engine cannot reduce.
    pub profile: Option<&'a leyline_color::TaggedSource>,
    /// The revision being rendered, already validated by the caller.
    pub settings: &'a Settings,
    /// EXIF identification of the shot, for the lens stage.
    pub shot: Option<&'a LensShot>,
    /// EXIF identification of the sensor and its sensitivity, for the
    /// profiled denoising stages (ADR 0072).
    pub sensor: Option<&'a SensorShot>,
    /// The already-resolved DCP matrix, for the camera profile stage.
    pub camera_profile: Option<&'a DcpProfile>,
    /// The already-parsed creative LUT, for the LUT stage (ADR 0053).
    pub lut: Option<&'a leyline_color::CubeLut>,
    /// The already-read, already-verified stored mask coverages, for the
    /// local adjustments stage (ADR 0070). Empty for the overwhelming
    /// majority of revisions, which reference none.
    pub coverages: &'a crate::mask_coverage::MaskCoverages,
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
    /// The rendering itself. Called through [`Version::run`], never
    /// directly.
    pub apply: fn(&mut Pixels, &Context<'_>),
}

impl Version {
    /// Runs this version on `px` — unless the buffer holds no pixel at all.
    ///
    /// An image of zero area has nothing to render, and every stage would
    /// leave it as empty as it found it. Skipping it here is what lets the
    /// frozen modules keep dividing by the width and chunking by the row
    /// without each carrying its own guard (ADR 0152 §6): `Pixels::from_raw`
    /// already refuses a zero dimension, so only a stage shrinking the
    /// buffer to nothing could hand one on.
    fn run(&self, px: &mut Pixels, ctx: &Context<'_>) {
        if px.width == 0 || px.height == 0 {
            return;
        }
        (self.apply)(px, ctx);
    }
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
    #[allow(
        clippy::expect_used,
        reason = "a test checks that every registered stage has at least one version"
    )]
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
// Published bindings are not restyled when a newer MSRV unlocks a lint (ADR 0168).
#[allow(clippy::collapsible_if)]
pub(crate) static STAGES: &[Stage] = &[
    Stage {
        // Always active: there is no rendering without an input, so this
        // stage has no neutral value (ADR 0044 §3). Its version is what
        // pins the *decoder's* configuration — see [`decode_params`] —
        // which changes pixels and was, until ADR 0044, the one input to
        // the rendering that no revision recorded.
        name: "input",
        active: |_| true,
        reads: &[
            "highlight_reconstruction",
            "camera_profile",
            "demosaic",
            "source_encoding",
        ],
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
            // Same buffer work once more, a decoder told which level the
            // *camera* calls white (ADR 0066). Unlike `v2` and `v3`, this
            // one is deliberately **not** bit-identical to its predecessor
            // at neutral settings: a photo reprocessed into it comes out
            // brighter, which is the correction it exists for.
            Version {
                version: 4,
                rank: 0,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    input::v4::to_working_space(
                        px,
                        ctx.source,
                        ctx.camera_profile.is_some(),
                        ctx.settings.highlight_reconstruction,
                    );
                },
            },
            // A decoder that stops truncating a non-RAW source to eight
            // bits, and a stage that understands a buffer already in the
            // working space (ADR 0107 §6). On every source the earlier
            // versions could decode, it renders bit for bit like `v4`.
            Version {
                version: 5,
                rank: 0,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    input::v5::to_working_space(
                        px,
                        ctx.source,
                        ctx.camera_profile.is_some(),
                        ctx.settings,
                    );
                },
            },
            // A non-RAW file that declares its colour space is read as what
            // it says rather than as sRGB (ADR 0115). For a RAW, an untagged
            // file, or a profile this engine cannot reduce, bit for bit
            // `v5`.
            Version {
                version: 6,
                rank: 0,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    input::v6::to_working_space(
                        px,
                        ctx.source,
                        ctx.profile,
                        ctx.camera_profile.is_some(),
                        ctx.settings,
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
        // The manual coefficients activate the stage on their own
        // (ADR 0111 §5): an uncalibrated lens is the case they exist for,
        // and enabling a distortion correction that has no data to work
        // from would be a strange price to pay for a colour fringe.
        active: |settings| {
            settings.lens_correction.enabled || settings.lens_correction.has_manual_tca()
        },
        reads: &["lens_correction"],
        versions: &[
            Version {
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
                        let correction = leyline_lens::Correction::new(
                            &profile,
                            shot.focal_mm,
                            px.width,
                            px.height,
                        );
                        *px = lens::v1::undistort(px, &correction);
                        *px = lens::v1::correct_tca(px, &correction);
                        if let Some(aperture_f) = shot.aperture_f {
                            lens::v1::devignette(px, &profile, shot.focal_mm, aperture_f);
                        }
                    }
                },
            },
            // Manual transverse chromatic aberration (ADR 0111): the same
            // Lensfun corrections, plus two measured coefficients folded
            // into the one per-channel resample. At zero coefficients this
            // renders v1's pixels bit for bit.
            Version {
                version: 2,
                rank: 20,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    let lens = &ctx.settings.lens_correction;
                    // The Lensfun half only when the caller asked for it,
                    // and only when the shot says what it was taken with.
                    let profile = lens.enabled.then_some(ctx.shot).flatten().and_then(|shot| {
                        leyline_lens::find_profile(
                            &shot.camera_make,
                            &shot.camera_model,
                            shot.lens_make.as_deref(),
                            shot.lens_model.as_deref().unwrap_or(""),
                        )
                        .map(|profile| (profile, shot))
                    });
                    let correction = profile.as_ref().map(|(profile, shot)| {
                        leyline_lens::Correction::new(profile, shot.focal_mm, px.width, px.height)
                    });
                    if let Some(correction) = &correction {
                        *px = lens::v2::undistort(px, correction);
                    }
                    *px =
                        lens::v2::correct_tca(px, correction.as_ref(), lens.tca_red, lens.tca_blue);
                    if let Some((profile, shot)) = &profile {
                        if let Some(aperture_f) = shot.aperture_f {
                            lens::v2::devignette(px, profile, shot.focal_mm, aperture_f);
                        }
                    }
                },
            },
        ],
    },
    Stage {
        // Between `lens` (20) and `spot_removal` (30), and the rank is the
        // decision (ADR 0109 §1): everything a person places afterwards is
        // placed on what they see, a reshape states something about the
        // subject rather than the composition, and resampling before
        // `sharpen` means what gets sharpened is the final geometry.
        name: "reshape",
        active: |settings| !settings.reshape.is_empty(),
        reads: &["reshape", "rotation"],
        versions: &[Version {
            version: 1,
            rank: 25,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                *px = reshape::v1::reshape(px, &ctx.settings.reshape, ctx.settings.rotation);
            },
        }],
    },
    Stage {
        // Right after `lens` (ADR 0113 §1): a fringe is a defect of the
        // capture, like the lateral aberration corrected two ranks earlier,
        // and correcting it before `spot_removal` means a clone copies
        // pixels that are already clean.
        name: "defringe",
        active: |settings| !settings.defringe.is_neutral(),
        reads: &["defringe"],
        versions: &[Version {
            version: 1,
            rank: 22,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                defringe::v1::defringe(
                    px,
                    ctx.settings.defringe.purple,
                    ctx.settings.defringe.green,
                    ctx.scale,
                );
            },
        }],
    },
    Stage {
        // After `spot_removal`, so a pole cloned out of the cloth is gone
        // before the cloth is measured, and before every tonal stage, so
        // the extension and the real cloth are developed alike
        // (ADR 0172 §1).
        name: "backdrop",
        active: |settings| !settings.backdrop.is_neutral(),
        reads: &["backdrop", "rotation"],
        versions: &[Version {
            version: 1,
            rank: 32,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                backdrop::v1::backdrop(
                    px,
                    &ctx.settings.backdrop,
                    ctx.settings.rotation,
                    ctx.coverages,
                );
            },
        }],
    },
    Stage {
        // Right after `spot_removal` and before every tonal stage
        // (ADR 0103 §1): a red pupil is a defect of the capture, and
        // correcting it once the tone sliders have stretched it would be
        // correcting a different red.
        name: "red_eye",
        active: |settings| !settings.red_eye.is_empty(),
        reads: &["red_eye", "rotation"],
        versions: &[Version {
            version: 1,
            rank: 35,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                red_eye::v1::red_eye(px, &ctx.settings.red_eye, ctx.settings.rotation);
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
        // The parametric curve (ADR 0137): a stage of its own and not a new
        // version of `tone_curve`, so every revision written before it keeps
        // rendering exactly what it rendered — absent from their maps, it
        // falls back to its pinned version, and `active` is false while
        // every region is 0 (the shape ADR 0103 established for red-eye).
        name: "parametric_curve",
        active: |settings| !settings.parametric_curve.is_neutral(),
        reads: &["parametric_curve"],
        versions: &[Version {
            version: 1,
            rank: 75,
            space: Space::LinearRec2020,
            apply: |px, ctx| {
                parametric_curve::v1::parametric_curve(px, &ctx.settings.parametric_curve);
            },
        }],
    },
    Stage {
        // Levels (ADR 0164): a stage of its own for the reason the
        // parametric curve above is one — absent from every existing map,
        // and inactive at neutral.
        name: "levels",
        active: |settings| !settings.levels.is_neutral(),
        reads: &["levels"],
        versions: &[Version {
            version: 1,
            rank: 78,
            space: Space::LinearRec2020,
            apply: |px, ctx| levels::v1::levels(px, &ctx.settings.levels),
        }],
    },
    Stage {
        name: "tone_curve",
        active: |settings| {
            !settings.tone_curve.points.is_empty() || settings.tone_curve.has_channel_curves()
        },
        reads: &["tone_curve"],
        versions: &[
            Version {
                version: 1,
                rank: 80,
                space: Space::LinearRec2020,
                apply: |px, ctx| tone_curve::v1::tone_curve(px, &ctx.settings.tone_curve.points),
            },
            // Per-channel curves (ADR 0098): with none set, this delegates
            // to v1 and renders exactly what v1 renders.
            Version {
                version: 2,
                rank: 80,
                space: Space::LinearRec2020,
                apply: |px, ctx| tone_curve::v2::tone_curve(px, &ctx.settings.tone_curve),
            },
        ],
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
    // Black and white (ADR 0088 §3). Ranked between `hsl` and
    // `color_grading` on purpose, and the rank is the design: after the
    // mixer, so its eight luminance sliders are the black-and-white mix;
    // before colour grading, so split-toning a black and white works with
    // no code of its own.
    Stage {
        name: "monochrome",
        active: |settings| settings.monochrome,
        reads: &["monochrome"],
        versions: &[Version {
            version: 1,
            rank: 145,
            space: Space::LinearRec2020,
            apply: |px, _ctx| monochrome::v1::monochrome(px),
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
            // Stored coverages (ADR 0070): a mask may be a file of samples
            // instead of a formula. Everything v2 can express renders here
            // exactly as v2 renders it.
            Version {
                version: 3,
                rank: 160,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    local_adjustments::v3::local_adjustments(
                        px,
                        &ctx.settings.local_adjustments,
                        ctx.settings.rotation,
                        ctx.coverages,
                    );
                },
            },
            // The five neighbourhood operators (ADR 0108): clarity, texture,
            // sharpness and the two noise reductions, which existed globally
            // and nowhere else. An entry that sets none of them renders here
            // exactly as v3 renders it.
            //
            // This is the first version of the stage to read `scale`: every
            // radius it binds is expressed at full resolution, so a preview
            // must shrink them or show a different picture than the export.
            Version {
                version: 4,
                rank: 160,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    local_adjustments::v4::local_adjustments(
                        px,
                        &ctx.settings.local_adjustments,
                        ctx.settings.rotation,
                        ctx.coverages,
                        ctx.scale,
                    );
                },
            },
            // The defringe pair on a mask (ADR 0116). An adjustment that
            // sets neither amount renders exactly what `v4` renders.
            Version {
                version: 5,
                rank: 160,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    local_adjustments::v5::local_adjustments(
                        px,
                        &ctx.settings.local_adjustments,
                        ctx.settings.rotation,
                        ctx.coverages,
                        ctx.scale,
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
        // `camera_profile` is read by `v3` and by it alone: whether a profile
        // is resolved decides whether the buffer it denoises is still
        // camera-native, and therefore how the measured model is carried into
        // it (ADR 0072 §5). Declaring it for `v1` and `v2` too costs nothing —
        // `input` already reads the same key at rank 0, so a profile change
        // invalidates everything downstream anyway — and a missing key here
        // would be a correctness bug rather than a slow render.
        reads: &["noise_reduction", "camera_profile"],
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
            // Measured threshold, and therefore a new rank: the model only
            // means something where the buffer is still a linear transform
            // of the sensor's counts (ADR 0072 §4).
            Version {
                version: 3,
                rank: 5,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    let model =
                        kernel::v3::model_for(ctx.sensor, ctx.source, ctx.camera_profile.is_some());
                    noise_luminance::v3::luminance_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.luminance,
                        ctx.scale,
                        &model,
                    );
                },
            },
        ],
    },
    Stage {
        // Colour moiré (ADR 0165): next to the other sensor artefacts, and
        // before anything resamples the image. A stage of its own, so no
        // existing revision's map changes.
        name: "moire",
        active: |settings| settings.moire != 0,
        reads: &["moire"],
        versions: &[Version {
            version: 1,
            rank: 7,
            space: Space::LinearRec2020,
            apply: |px, ctx| moire::v1::moire(px, ctx.settings.moire, ctx.scale),
        }],
    },
    Stage {
        name: "noise_color",
        active: |settings| settings.noise_reduction.color != 0,
        // Same reason as `noise_luminance` above.
        reads: &["noise_reduction", "camera_profile"],
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
            // Measured threshold, one rank behind `noise_luminance::v3`
            // (ADR 0072 §4).
            Version {
                version: 3,
                rank: 6,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    let model =
                        kernel::v3::model_for(ctx.sensor, ctx.source, ctx.camera_profile.is_some());
                    noise_color::v3::color_noise_reduction(
                        px,
                        ctx.settings.noise_reduction.color,
                        ctx.scale,
                        &model,
                    );
                },
            },
        ],
    },
    Stage {
        name: "sharpen",
        active: |settings| settings.sharpening.amount != 0,
        reads: &["sharpening"],
        versions: &[
            Version {
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
            },
            // The edge mask (ADR 0096): at `masking = 0` this renders
            // exactly what v1 renders, which is what makes the bump safe.
            Version {
                version: 2,
                rank: 190,
                space: Space::LinearRec2020,
                apply: |px, ctx| {
                    sharpen::v2::sharpen(
                        px,
                        ctx.settings.sharpening.amount,
                        ctx.settings.sharpening.radius * f64::from(ctx.scale),
                        ctx.settings.sharpening.masking,
                    );
                },
            },
        ],
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
    // The two effects of ADR 0090, and their rank *is* the decision: after
    // `crop` (210), so both are drawn on the frame the photographer
    // composed rather than on the one the sensor delivered. The distance
    // from `lens` (20), which *removes* the vignetting a lens produced, is
    // the whole difference between a correction and an effect.
    Stage {
        name: "vignette",
        active: |settings| !settings.vignette.is_neutral(),
        reads: &["vignette"],
        versions: &[Version {
            version: 1,
            rank: 220,
            space: Space::LinearRec2020,
            apply: |px, ctx| vignette::v1::vignette(px, &ctx.settings.vignette),
        }],
    },
    Stage {
        name: "grain",
        active: |settings| !settings.grain.is_neutral(),
        reads: &["grain"],
        versions: &[
            Version {
                version: 1,
                rank: 230,
                space: Space::LinearRec2020,
                // The only stage reading `ctx.scale` for anything but a radius:
                // its lattice is in full-resolution pixels, so a preview
                // samples the same field as the export (ADR 0090 §3).
                apply: |px, ctx| grain::v1::grain(px, &ctx.settings.grain, ctx.scale),
            },
            // Layers that disagree (ADR 0118). Same rank and same slider; at
            // `color: 0` it calls v1, so the two are bit-identical there by
            // control flow rather than by argument.
            Version {
                version: 2,
                rank: 230,
                space: Space::LinearRec2020,
                apply: |px, ctx| grain::v2::grain(px, &ctx.settings.grain, ctx.scale),
            },
        ],
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
    (4, input::v4::decode_params),
    (5, input::v5::decode_params),
    (6, input::v6::decode_params),
];

/// Whether one `input` version decodes a non-RAW source at its own bit depth
/// (ADR 0107 §6), rather than normalizing it to eight bits.
///
/// The other half of [`INPUT_DECODE`], and frozen for the same reason: it
/// changes pixels, so it belongs to the version a revision cites and not to
/// whatever the decoder happens to do this year. A table rather than a
/// `version >= 5` test, so that adding a version is a line here and a
/// decision, never an inherited default.
static INPUT_NATIVE_DEPTH: &[(u16, bool)] = &[
    (1, false),
    (2, false),
    (3, false),
    (4, false),
    (5, true),
    (6, true),
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
    #[allow(
        clippy::expect_used,
        reason = "a test checks that every registered input version has an entry in INPUT_DECODE"
    )]
    let decode = INPUT_DECODE
        .iter()
        .find(|(v, _)| *v == version.version)
        .map(|(_, decode)| decode)
        .expect("every published input version has a decoder configuration");
    decode(settings, half_size)
}

/// Whether the non-RAW decoder keeps the file's own bit depth for this
/// revision (ADR 0107 §6). Ignored by RAW and DNG sources, which LibRaw
/// always hands over at sixteen bits.
#[allow(
    clippy::expect_used,
    reason = "a test checks that every registered input version has an entry in INPUT_NATIVE_DEPTH"
)]
pub(crate) fn native_bit_depth(settings: &Settings) -> bool {
    let version = version_of("input", settings);
    INPUT_NATIVE_DEPTH
        .iter()
        .find(|(v, _)| *v == version.version)
        .map(|(_, native)| *native)
        .expect("every published input version says what bit depth it decodes at")
}

#[cfg(test)]
mod fixture;

#[cfg(all(test, feature = "gpu"))]
mod gpu_coverage;

#[cfg(test)]
mod golden;

#[cfg(test)]
mod hostile;

#[cfg(test)]
mod tests;
