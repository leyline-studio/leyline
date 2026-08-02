//! Input v3 (ADR 0061) — rank 0, ahead of every other stage.
//!
//! A copy of `v2` with one addition: the decoder is told **which
//! interpolation** to reconstruct the missing channels with
//! ([`decode_params`]). `v2` asked for LibRaw's default — AHD — without ever
//! naming it, so the very first rendering decision of the pipeline was the
//! one thing a revision did not record (ADR 0061 §1).
//!
//! **Nothing else changes.** The conversion into the working space is `v2`'s,
//! called rather than copied: `v2` is frozen, so what it does can never move
//! under this version, and the alternative is 120 duplicated lines that would
//! have to be proved identical instead of being identical by construction.
//! This is the same reasoning that puts a shared body in `kernel::v1`
//! (ADR 0042 §1).
//!
//! A revision in `v3` that leaves `demosaic` at its neutral `ahd` therefore
//! renders **bit for bit** like the same revision in `v2` — which is what
//! makes reprocessing to `v3` safe to offer.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::{Demosaic, HighlightReconstruction, Settings};
use leyline_raw::{DecodeParams, Demosaic as RawDemosaic, HighlightMode};

pub(crate) use super::v2::to_working_space;

/// What this version asks the decoder for.
///
/// `demosaic` is passed even when `half_size` is set, where LibRaw ignores
/// it: half-size decoding takes one pixel per 2x2 Bayer group and skips
/// interpolation entirely (ADR 0061 §Contexte). Suppressing it here would
/// only hide that fact one layer lower.
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
    }
}
