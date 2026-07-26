//! Texture v1 (ADR 0033) — rank 100.
//!
//! Introduced by process 10. Same operator as [`crate::stages::clarity`] at
//! a small radius, so it moves fine detail instead of regional contrast.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

/// Blur radius for the texture stage: fine local contrast, wider than
/// sharpening's own radius range but much narrower than clarity's.
pub(crate) const TEXTURE_RADIUS: f32 = 6.0;
