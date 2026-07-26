//! Clarity v1 (ADR 0033) — rank 90.
//!
//! Introduced by process 10. Large-radius local contrast on the luma plane;
//! the operator body is [`crate::stages::kernel::v1::local_contrast`],
//! shared with [`crate::stages::texture`], and this version *is* the radius
//! bound to it.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

/// Blur radius (same "sigma" units [`crate::stages::kernel::v1::gaussian_blur`] uses) for the clarity
/// stage: a large-radius local contrast boost, the classic "clarity" look.
pub(crate) const CLARITY_RADIUS: f32 = 40.0;
