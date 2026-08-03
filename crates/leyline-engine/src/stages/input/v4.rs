//! Input v4 (ADR 0066) — rank 0, ahead of every other stage.
//!
//! A copy of `v3` with one addition: the decoder normalizes by the level the
//! **camera** calls white, not by the raw format's ceiling
//! ([`decode_params`]).
//!
//! `v3` and everything before it divided by `maximum` — 16383 for any 14-bit
//! file, the same number for every body and every sensitivity. A Canon 60D
//! saturates at 12279 at ISO 100 and at 15094 at ISO 400, values the camera
//! writes into the file and LibRaw reads out of it. Dividing by the word size
//! instead left a neutral rendering 8 to 19 % dark depending on the ISO, and
//! — worse — mapped a fully blown highlight to 0.82 instead of white
//! (ADR 0066 §1).
//!
//! **Nothing else changes.** The conversion into the working space is `v2`'s,
//! called rather than copied, exactly as `v3` calls it: `v2` is frozen, so
//! what it does can never move under this version.
//!
//! Unlike `v3` before it, this version is **not** bit-identical to its
//! predecessor at neutral settings — it cannot be, since changing what white
//! means is the whole point. A photo reprocessed into `v4` gets brighter, and
//! that is the correction. Revisions left in `v3` keep rendering exactly as
//! they always have (`docs/pipeline.md` §5.1).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract: a
//! revision citing this stage version renders through exactly this code,
//! forever. A change of rendering is a new version module next to this one,
//! never an edit here (ADR 0042 §1).

use leyline_core::{Demosaic, HighlightReconstruction, Settings};
use leyline_raw::{DecodeParams, Demosaic as RawDemosaic, HighlightMode, WhiteLevel};

pub(crate) use super::v2::to_working_space;

/// What this version asks the decoder for: `v3`'s configuration, plus the
/// camera's own white level.
///
/// The fallback lives in `leyline-raw`: a body that writes no linearity
/// margin decodes exactly as `v3` did, so this version is never worse than
/// its predecessor — only better where the file says something.
pub(crate) fn decode_params(settings: &Settings, half_size: bool) -> DecodeParams {
    DecodeParams {
        half_size,
        sixteen_bit: true,
        camera_native: true,
        auto_brighten: false,
        highlight: match settings.highlight_reconstruction {
            HighlightReconstruction::Clip => HighlightMode::Clip,
            HighlightReconstruction::Blend => HighlightMode::Blend,
            HighlightReconstruction::Rebuild => HighlightMode::Rebuild,
        },
        demosaic: match settings.demosaic {
            Demosaic::Ahd => RawDemosaic::Ahd,
            Demosaic::Vng => RawDemosaic::Vng,
            Demosaic::Dcb => RawDemosaic::Dcb,
            Demosaic::Dht => RawDemosaic::Dht,
        },
        white_level: WhiteLevel::CameraLinearityMargin,
    }
}
