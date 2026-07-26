//! Output rendering v1 (ADR 0044 §3) — rank 900, after every other stage.
//!
//! The pipeline's exit: what turns the working buffer into a display signal.
//! At this version that is nothing at all — the buffer is already
//! display-referred sRGB, clamped to [0, 1] (`crate::pixels`), so there is
//! no range to compress and no encoding to apply.
//!
//! It exists anyway, and is recorded by every revision, because it is the
//! anchor the linear working space needs: once the buffer holds unbounded
//! linear light, *some* version of this stage has to bring it back, and a
//! revision has to say which one. Introducing it while it is still a no-op
//! is what makes that step a version bump instead of a change in the shape
//! of what a revision records (ADR 0044 §7.1).
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).
