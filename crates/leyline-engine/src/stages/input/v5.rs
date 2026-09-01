//! Input v5 (ADR 0107) — rank 0, ahead of every other stage.
//!
//! A copy of `v4` with two additions, both of them about what the decoder
//! hands over rather than about what is done to it afterwards — which is
//! what an `input` version is for (`docs/pipeline.md` §3.3).
//!
//! 1. **A non-RAW source keeps its own bit depth.** Until `v4`,
//!    `source::decode_native` normalized every JPEG, PNG and TIFF to eight
//!    bits, so a 16-bit TIFF lost 65 280 of its 65 536 levels on the way in
//!    and nobody was told. That truncation is pinned by the version like
//!    everything else: a revision left at `v4` still gets eight bits, which
//!    is what it was rendered with.
//! 2. **[`SourceEncoding::LinearWorkspace`] is understood.** A derived asset
//!    (ADR 0107 §4) holds the develop buffer as it stood before rank 20 —
//!    linear Rec. 2020, white at 1.0 — so there is no transfer function to
//!    decode and no primaries to rotate. This version passes it through.
//!
//! At [`SourceEncoding::Srgb`], which is every file this engine decodes
//! itself, `v5` renders **bit for bit** like `v4`: the same decoder
//! configuration, the same conversion, called rather than copied. The only
//! way to tell them apart is a source they disagree about, and a 16-bit
//! non-RAW file is the only such source.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract: a
//! revision citing this stage version renders through exactly this code,
//! forever. A change of rendering is a new version module next to this one,
//! never an edit here (ADR 0042 §1).

use leyline_core::{Settings, SourceEncoding};

use crate::pixels::Pixels;
use crate::stages::SourceColor;

pub(crate) use super::v4::decode_params;

/// Brings the decoded buffer into the working space — `v4`'s conversion,
/// skipped entirely when the file is already there.
///
/// The skip reads the *setting*, not the file: what a `.tif` on disk holds
/// is not decidable from its name, and the revision is where this project
/// records everything that changes pixels. A revision claiming
/// `linear_workspace` for a camera-native file is refused by
/// `Settings::validate` before any of this runs (ADR 0107 §6).
pub(crate) fn to_working_space(
    px: &mut Pixels,
    source: SourceColor,
    has_camera_profile: bool,
    settings: &Settings,
) {
    if settings.source_encoding == SourceEncoding::LinearWorkspace {
        return;
    }
    super::v2::to_working_space(
        px,
        source,
        has_camera_profile,
        settings.highlight_reconstruction,
    );
}
