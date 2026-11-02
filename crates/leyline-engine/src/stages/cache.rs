//! The stage cache: checkpoints of the preview pipeline, so a slider
//! move restarts from the nearest unchanged stage (ADR 0041 §3).

use super::*;

/// Ranks a checkpoint is taken *before* (ADR 0041 §3), one snapshot each.
///
/// They sit where the ADR put them — after lens correction and spot removal
/// (expensive, almost never touched in a burst of slider moves), after the
/// tonal block, after clarity/texture/dehaze, and after local adjustments —
/// expressed as the rank of the stage that follows, so a checkpoint lands
/// in the right place whether or not the stages around it are active.
pub(super) const CHECKPOINT_BEFORE_RANK: &[u16] = &[40, 90, 120, 165];

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
    /// One entry per input recently rendered, most recent first, at most
    /// [`CONTEXTS`]: the develop view's `Small` render and its loupe-sized
    /// upgrade alternate, and one context would have each discard the
    /// other's checkpoints at every call (ADR 0166 §4).
    pub(super) contexts: Vec<ContextCache>,
}

/// How many inputs the stage cache keeps checkpoints for (ADR 0166 §4).
pub(super) const CONTEXTS: usize = 2;

/// A frame longer than this keeps only its first checkpoint (ADR 0166 §4):
/// at 4K each is ~134 MB, and the deeper ones serve only CPU drags that
/// Studio renders at `Small` instead.
pub(super) const ONE_CHECKPOINT_ABOVE: u32 = 2048;

/// Which render a cache entry serves: the asset, and the longest edge of
/// the preview class asked for (`None` for full resolution). The class is a
/// label for [`StageCache::would_render_on_gpu`]; what invalidates is
/// [`ContextCache::key`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CacheTarget {
    pub asset: leyline_core::AssetId,
    pub class_edge: Option<u32>,
}

impl From<leyline_core::AssetId> for CacheTarget {
    fn from(asset: leyline_core::AssetId) -> Self {
        CacheTarget {
            asset,
            class_edge: None,
        }
    }
}

/// The checkpoints of one input.
#[derive(Debug)]
pub(super) struct ContextCache {
    /// What the buffers were rendered from: asset, proxy scale and the
    /// input buffer's dimensions. None is a setting, so none is covered by
    /// [`prefix_fingerprint`] — a change here starts another context.
    ///
    /// The scale alone does not identify the input: it is relative to the
    /// *decoded* buffer, and a half-size decode reduced to 1024 px can have
    /// exactly the scale of a full decode reduced to 2048 (a 6720-px sensor:
    /// 3360 → 1024 and 6720 → 2048 are both ×0.3048). Measured by E0
    /// (`docs/measured-findings.md` §B5): a `Medium` render after a `Small`
    /// one came back at 1024 px.
    pub(super) key: (leyline_core::AssetId, u32, u32, u32),
    #[cfg_attr(not(feature = "gpu"), allow(dead_code))]
    pub(super) class_edge: Option<u32>,
    pub(super) checkpoints: Vec<Checkpoint>,
    /// The GPU-resident copy of one checkpoint (ADR 0156 §4): uploaded once,
    /// rendered from at every frame of a drag. Purely derived, like the
    /// checkpoints, and dropped with them.
    #[cfg(feature = "gpu")]
    pub(super) gpu_frame: Option<crate::gpu_preview::ResidentFrame>,
}

/// One intermediate buffer, and what it is only valid for.
#[derive(Debug)]
pub(super) struct Checkpoint {
    /// How many stages of the plan had been applied when it was taken.
    pub(super) applied: usize,
    /// [`prefix_fingerprint`] over exactly those stages.
    pub(super) fingerprint: u64,
    pub(super) pixels: Pixels,
}

impl StageCache {
    /// Makes the context of this input the current one — found among the
    /// kept ones, or started empty — and forgets the oldest beyond
    /// [`CONTEXTS`].
    pub(super) fn retarget(&mut self, target: CacheTarget, scale: f32, input: &RawImage) {
        let key = (target.asset, scale.to_bits(), input.width, input.height);
        match self.contexts.iter().position(|c| c.key == key) {
            Some(0) => {}
            Some(found) => {
                let context = self.contexts.remove(found);
                self.contexts.insert(0, context);
            }
            None => {
                self.contexts.insert(
                    0,
                    ContextCache {
                        key,
                        class_edge: target.class_edge,
                        checkpoints: Vec::new(),
                        #[cfg(feature = "gpu")]
                        gpu_frame: None,
                    },
                );
                self.contexts.truncate(CONTEXTS);
            }
        }
    }

    /// The current context; only called after [`StageCache::retarget`].
    pub(super) fn current(&mut self) -> Option<&mut ContextCache> {
        self.contexts.first_mut()
    }

    /// Renders the rest of `plan` on the GPU from the deepest valid
    /// checkpoint, when every stage after it has a kernel (ADR 0156 §4).
    #[cfg(feature = "gpu")]
    pub(super) fn render_on_gpu(
        &mut self,
        settings: &Settings,
        plan: &[(&'static Stage, &'static Version)],
    ) -> Option<Rendered> {
        let current = self.current()?;
        current
            .checkpoints
            .retain(|c| c.fingerprint == prefix_fingerprint(settings, plan, c.applied));
        let deepest = current.checkpoints.iter().max_by_key(|c| c.applied)?;
        let (width, height, data) = crate::gpu_preview::render(
            &mut current.gpu_frame,
            plan.get(deepest.applied..)?,
            settings,
            f32::from_bits(current.key.1),
            deepest.applied,
            deepest.fingerprint,
            &deepest.pixels,
        )?;
        Some(Rendered {
            width,
            height,
            data,
        })
    }

    /// Whether a render of `settings` for `asset` at the class of
    /// `class_edge` would run on the GPU now (ADR 0166 §3): a device, a
    /// context of that class with a checkpoint still valid for these
    /// settings, and a kernel for every stage after it. Reads only; it
    /// drops nothing.
    #[cfg(feature = "gpu")]
    pub(crate) fn would_render_on_gpu(
        &self,
        asset: leyline_core::AssetId,
        class_edge: Option<u32>,
        settings: &Settings,
    ) -> bool {
        let Ok(plan) = plan(settings) else {
            return false;
        };
        let Some(context) = self
            .contexts
            .iter()
            .find(|c| c.key.0 == asset && c.class_edge == class_edge)
        else {
            return false;
        };
        context
            .checkpoints
            .iter()
            .filter(|c| c.fingerprint == prefix_fingerprint(settings, &plan, c.applied))
            .max_by_key(|c| c.applied)
            .is_some_and(|deepest| {
                crate::gpu_preview::covers(
                    plan.get(deepest.applied..).unwrap_or(&[]),
                    settings,
                    f32::from_bits(context.key.1),
                )
            })
    }

    /// The deepest checkpoint still valid for `settings`, if any.
    ///
    /// Deepest wins: it is the one that skips the most work. A checkpoint
    /// whose fingerprint no longer matches is dropped rather than kept —
    /// the settings that produced it are gone, and nothing will bring
    /// them back within this session.
    pub(super) fn resume_from(
        &mut self,
        settings: &Settings,
        plan: &[(&'static Stage, &'static Version)],
    ) -> Option<(usize, Pixels)> {
        let current = self.current()?;
        current
            .checkpoints
            .retain(|c| c.fingerprint == prefix_fingerprint(settings, plan, c.applied));
        current
            .checkpoints
            .iter()
            .max_by_key(|c| c.applied)
            .map(|c| (c.applied, c.pixels.clone()))
    }

    /// Records a buffer taken after `applied` stages, replacing any
    /// checkpoint already held at that depth. A frame longer than
    /// [`ONE_CHECKPOINT_ABOVE`] keeps only its `first` (ADR 0166 §4).
    pub(super) fn store(
        &mut self,
        settings: &Settings,
        plan: &[(&'static Stage, &'static Version)],
        applied: usize,
        pixels: &Pixels,
        first: bool,
    ) {
        if !first && pixels.width.max(pixels.height) > ONE_CHECKPOINT_ABOVE {
            return;
        }
        let fingerprint = prefix_fingerprint(settings, plan, applied);
        let Some(current) = self.current() else {
            return;
        };
        current.checkpoints.retain(|c| c.applied != applied);
        current.checkpoints.push(Checkpoint {
            applied,
            fingerprint,
            pixels: pixels.clone(),
        });
    }
}
