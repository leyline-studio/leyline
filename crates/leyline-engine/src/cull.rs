//! Assisted culling: measures into a **proposal** (ADR 0084).
//!
//! [`leyline_cull`] computes numbers and decides nothing. This module is
//! where the numbers become a suggestion, and where the two rules of
//! ADR 0084 are enforced rather than restated:
//!
//! * **§1 — classification and nothing else.** What a proposal names is a
//!   `pick`/`reject`, which `catalog.md` §18 already versions, filters and
//!   undoes. There is no new state, no migration, no stage, and therefore
//!   nothing for `pipeline.md` §5.1 to be concerned about;
//! * **§2 — it proposes, the photographer applies.** Nothing here writes.
//!   A [`CullProposal`] is a value the caller shows and then acts on with
//!   the ordinary `set_pick` — the same call `leyline pick` makes. The
//!   reason that rule is not negotiable is worth repeating where the code
//!   is: a wrongly rejected photograph does not look wrong, it looks
//!   absent, so the mistake is invisible exactly where undo would need the
//!   user to notice it.
//!
//! **Sharpness is never a threshold.** [`leyline_cull::Quality::focus`]
//! rises with detail as much as with sharpness, so a sharp wall scores
//! below a blurred hedge. It is used here only to rank frames *within one
//! burst*, which is the one comparison it is good for. The two absolute
//! judgements this module does make — a black frame, a blown frame — are
//! about exposure, where an absolute number means something.

use std::path::Path;

use leyline_catalog::Catalog;
use leyline_core::{AssetId, LeylineError, PreviewKind, Result, VersionId};
use leyline_cull::{Fingerprint, Frame, Quality};

/// How tight "the same scene" is, in bits of fingerprint distance.
///
/// The crate exposes this as the caller's business, and it is: a
/// photographer bracketing a tripod shot produces near-identical frames,
/// one panning a bird produces frames that drift. 10 of 64 bits is the
/// default because it holds a burst together through a pan without joining
/// two sides of a room; a `--burst-distance` reaches it from the clients.
pub const DEFAULT_BURST_DISTANCE: u32 = 10;

/// What a culling run reads and how tightly it groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CullOptions {
    /// Fingerprint distance below which two consecutive frames are one
    /// burst. See [`DEFAULT_BURST_DISTANCE`].
    pub burst_distance: u32,
}

impl Default for CullOptions {
    fn default() -> Self {
        CullOptions {
            burst_distance: DEFAULT_BURST_DISTANCE,
        }
    }
}

/// Why a frame is proposed for rejection.
///
/// A typed reason rather than the free text ADR 0084 §1 imagined: the
/// clients that show it are translated, and a sentence built in the engine
/// arrives in English in a French window. What the ADR asked for — that a
/// proposal say *why* — is what this carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Another frame of the same burst is sharper. Comparative, which is
    /// the only way this measure is used.
    Softer {
        /// The frame that beat it.
        sharper: AssetId,
    },
    /// Almost nothing in it is not blown.
    Blown,
    /// Almost nothing in it is not black — a lens cap, a pocket, a shutter
    /// fired by accident.
    Black,
}

/// What the assistant proposes for one photograph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing against it.
    Keep,
    /// The sharpest frame of a burst of several — the one to keep.
    Pick,
    /// Proposed for rejection.
    Reject(RejectReason),
}

/// One photograph in a proposal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CullEntry {
    /// The photograph.
    pub asset: AssetId,
    /// Its current version — what a `set_pick` would be written against.
    pub version: VersionId,
    /// Which burst it belongs to, counting from 0 in capture order. A
    /// photograph in no burst has a group of its own.
    pub burst: usize,
    /// What was measured.
    pub quality: Quality,
    /// What is proposed.
    pub verdict: Verdict,
}

/// The outcome of a run: **held by the caller, written by nobody**.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CullProposal {
    /// Every photograph considered, in capture order.
    pub entries: Vec<CullEntry>,
    /// Photographs that could not be measured, and why. A frame with no
    /// readable thumbnail is skipped, never guessed at — a proposal that
    /// invented a verdict for an unreadable file would be proposing to
    /// reject the file it failed to read.
    pub skipped: Vec<(AssetId, String)>,
}

impl CullProposal {
    /// The versions the proposal would reject, in capture order — what a
    /// caller passes to `set_pick` when the photographer accepts.
    #[must_use]
    pub fn rejects(&self) -> Vec<VersionId> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.verdict, Verdict::Reject(_)))
            .map(|entry| entry.version)
            .collect()
    }

    /// The versions the proposal would mark as the keeper of their burst.
    #[must_use]
    pub fn picks(&self) -> Vec<VersionId> {
        self.entries
            .iter()
            .filter(|entry| entry.verdict == Verdict::Pick)
            .map(|entry| entry.version)
            .collect()
    }

    /// How many bursts of more than one frame were found.
    #[must_use]
    pub fn bursts(&self) -> usize {
        let mut seen: Vec<usize> = self.entries.iter().map(|e| e.burst).collect();
        seen.dedup();
        seen.iter()
            .filter(|&&burst| self.entries.iter().filter(|e| e.burst == burst).count() > 1)
            .count()
    }
}

/// A frame is proposed as blown when this fraction of its pixels has a
/// channel at 255.
///
/// A third, not a tenth: a backlit portrait or a snow scene legitimately
/// blows a large area, and this judgement has to survive being wrong in
/// front of a photographer who meant it. What it is for is the frame where
/// the exposure ran away entirely.
const BLOWN_FRACTION: f32 = 0.33;

/// And as black when this fraction is at or below 2 in every channel, with
/// nothing anywhere else to look at.
const BLACK_FRACTION: f32 = 0.90;

/// Mean luma below which a frame is dark enough for [`BLACK_FRACTION`] to
/// mean "there is nothing in it" rather than "this is a night shot".
const BLACK_MEAN_LUMA: f32 = 0.02;

/// Everything one run needs from the catalog, read up front so the slow
/// half holds no lock (ADR 0023's split, applied once more).
pub(crate) struct CullPlan {
    /// Assets in capture order, with the version a verdict names.
    pub(crate) frames: Vec<(AssetId, VersionId)>,
}

/// Reads the assets to consider, in **capture order** — which is not a
/// convenience: a burst is contiguous in time, and `leyline_cull`'s
/// grouping walks consecutive frames.
///
/// Two kinds of photograph are left out here rather than judged:
///
/// * **companions** (ADR 0079) — `grid_order` already drops them, and the
///   reason holds: the camera's JPEG of a shot is not a second frame of
///   that shot;
/// * **derived assets** (ADR 0107) — found on a real library the first
///   time this was run over one. A denoised frame is pixel-for-pixel its
///   parent's scene, so it fingerprints as a burst with it, and the
///   assistant duly proposed throwing away the original in favour of the
///   copy made *from* it. That is not a near-duplicate the photographer
///   should be asked about: it is a decision they took ten minutes ago,
///   and offering to undo it by rejecting one of the two is worse than
///   saying nothing.
pub(crate) fn plan_cull(catalog: &Catalog, assets: &[AssetId]) -> Result<CullPlan> {
    let ordered = catalog.grid_order(assets)?;
    let mut frames = Vec::with_capacity(ordered.len());
    for asset in ordered {
        if catalog.derived_from(asset).ok().flatten().is_some() {
            continue;
        }
        // A photograph whose current version cannot be read is left out
        // rather than defaulting to some other version of it: a verdict
        // written against the wrong version is a verdict about a
        // development nobody was looking at.
        if let Ok(version) = catalog.current_version(asset) {
            frames.push((asset, version));
        }
    }
    Ok(CullPlan { frames })
}

/// Measures one thumbnail from disk.
pub(crate) fn measure(path: &Path) -> std::result::Result<(Quality, Fingerprint), String> {
    let image = image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgb8();
    let frame = Frame {
        width: image.width(),
        height: image.height(),
        rgb: image.as_raw(),
    };
    let quality = leyline_cull::quality(frame)
        .ok_or_else(|| format!("{} is too small to measure", path.display()))?;
    let fingerprint = leyline_cull::fingerprint(frame)
        .ok_or_else(|| format!("{} is too small to fingerprint", path.display()))?;
    Ok((quality, fingerprint))
}

/// Turns measures into verdicts (ADR 0084 §4's "the engine's business").
///
/// Two passes, and the order matters. The absolute judgements come first:
/// a black frame is a black frame whether or not something near it in time
/// looks like it. Then the comparative one, inside each burst — and a
/// frame already condemned on its own account does not take part, so a
/// burst of ten good frames and one lens-cap shot still keeps its sharpest
/// good frame rather than crowning the accident.
pub(crate) fn decide(
    frames: &[(AssetId, VersionId)],
    measures: &[(Quality, Fingerprint)],
    options: &CullOptions,
) -> Vec<CullEntry> {
    let prints: Vec<Fingerprint> = measures.iter().map(|(_, print)| *print).collect();
    let groups = leyline_cull::group_bursts(&prints, options.burst_distance);

    let mut entries: Vec<CullEntry> = frames
        .iter()
        .zip(measures)
        .map(|(&(asset, version), &(quality, _))| CullEntry {
            asset,
            version,
            burst: 0,
            quality,
            verdict: absolute_verdict(&quality),
        })
        .collect();

    for (burst, members) in groups.iter().enumerate() {
        for &index in members {
            entries[index].burst = burst;
        }
        // Only the frames nothing is already wrong with compete on
        // sharpness, and a burst of one has nothing to compare against.
        let contenders: Vec<usize> = members
            .iter()
            .copied()
            .filter(|&i| entries[i].verdict == Verdict::Keep)
            .collect();
        if contenders.len() < 2 {
            continue;
        }
        let scores: Vec<Quality> = contenders.iter().map(|&i| entries[i].quality).collect();
        let winner = contenders[leyline_cull::sharpest(&scores).expect("contenders is not empty")];
        entries[winner].verdict = Verdict::Pick;
        for &index in &contenders {
            if index != winner {
                entries[index].verdict = Verdict::Reject(RejectReason::Softer {
                    sharper: entries[winner].asset,
                });
            }
        }
    }
    entries
}

/// What can be said about a frame without looking at its neighbours.
fn absolute_verdict(quality: &Quality) -> Verdict {
    if quality.crushed_shadows >= BLACK_FRACTION && quality.mean_luma <= BLACK_MEAN_LUMA {
        return Verdict::Reject(RejectReason::Black);
    }
    if quality.clipped_highlights >= BLOWN_FRACTION {
        return Verdict::Reject(RejectReason::Blown);
    }
    Verdict::Keep
}

/// The size class a run measures at, and the whole reason it is affordable.
///
/// `Thumbnail` is the class the import pass already fills from the preview
/// the body embedded (ADR 0082), so culling a shoot that has been imported
/// costs no decode at all — where `Small` would put a full develop render
/// behind every photograph, three hours over a library of fifteen
/// thousand, which is the cost ADR 0084 §4 exists to avoid.
///
/// What that costs in return is stated where it is measured: ranking at
/// this scale agrees with full resolution where the difference is real and
/// disagrees where it is negligible, which is why §2 forbids applying a
/// verdict without showing it.
pub(crate) const MEASURED_AT: PreviewKind = PreviewKind::Thumbnail;

/// Refuses a run over a library that has none of its previews yet, rather
/// than quietly rendering thousands of them.
pub(crate) fn missing_preview(asset: AssetId, error: &LeylineError) -> (AssetId, String) {
    (asset, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(index: usize) -> (AssetId, VersionId) {
        let n = index as i64 + 1;
        (AssetId::new(n), VersionId::new(n))
    }

    fn measure_of(focus: f32, clipped: f32, crushed: f32, luma: f32) -> (Quality, Fingerprint) {
        (
            Quality {
                focus,
                clipped_highlights: clipped,
                crushed_shadows: crushed,
                mean_luma: luma,
            },
            Fingerprint(0),
        )
    }

    fn sharp(focus: f32) -> (Quality, Fingerprint) {
        measure_of(focus, 0.0, 0.0, 0.4)
    }

    fn proposal(measures: Vec<(Quality, Fingerprint)>) -> CullProposal {
        let frames: Vec<(AssetId, VersionId)> = (0..measures.len()).map(ids).collect();
        CullProposal {
            entries: decide(&frames, &measures, &CullOptions::default()),
            skipped: Vec::new(),
        }
    }

    /// The one comparison sharpness is good for: inside a burst, and only
    /// there. Everything else in this module exists to keep it there.
    #[test]
    fn the_sharpest_of_a_burst_is_kept_and_the_others_are_proposed() {
        let proposal = proposal(vec![sharp(1.0), sharp(3.0), sharp(2.0)]);

        assert_eq!(proposal.entries[1].verdict, Verdict::Pick);
        assert_eq!(
            proposal.entries[0].verdict,
            Verdict::Reject(RejectReason::Softer {
                sharper: AssetId::new(2)
            })
        );
        assert_eq!(
            proposal.rejects(),
            vec![VersionId::new(1), VersionId::new(3)]
        );
        assert_eq!(proposal.picks(), vec![VersionId::new(2)]);
        assert_eq!(proposal.bursts(), 1);
    }

    /// A photograph on its own is never rejected for being soft: there is
    /// nothing to compare it against, and an absolute sharpness threshold
    /// is the mistake this whole module is arranged to avoid.
    #[test]
    fn a_lone_frame_is_never_rejected_for_softness() {
        let mut measures = vec![sharp(0.01)];
        measures[0].1 = Fingerprint(0);
        let proposal = proposal(measures);
        assert_eq!(proposal.entries[0].verdict, Verdict::Keep);
        assert!(proposal.rejects().is_empty());
        assert_eq!(proposal.bursts(), 0);
    }

    /// Two frames of different scenes are two bursts, however soft one of
    /// them is: the grouping decides who competes.
    #[test]
    fn frames_of_different_scenes_do_not_compete() {
        let mut measures = vec![sharp(0.5), sharp(9.0)];
        measures[0].1 = Fingerprint(0);
        measures[1].1 = Fingerprint(u64::MAX);
        let proposal = proposal(measures);
        assert!(proposal.entries.iter().all(|e| e.verdict == Verdict::Keep));
        assert_eq!(proposal.entries[0].burst, 0);
        assert_eq!(proposal.entries[1].burst, 1);
    }

    #[test]
    fn a_black_frame_and_a_blown_one_are_named_for_what_they_are() {
        let proposal = proposal(vec![
            measure_of(0.1, 0.0, 0.99, 0.005),
            measure_of(1.0, 0.80, 0.0, 0.9),
        ]);
        assert_eq!(
            proposal.entries[0].verdict,
            Verdict::Reject(RejectReason::Black)
        );
        assert_eq!(
            proposal.entries[1].verdict,
            Verdict::Reject(RejectReason::Blown)
        );
    }

    /// A dark night shot is not a black frame. The two are told apart by
    /// mean luma, and getting this wrong throws away every photograph
    /// taken after sunset.
    #[test]
    fn a_night_shot_is_not_a_lens_cap() {
        let proposal = proposal(vec![measure_of(0.4, 0.0, 0.95, 0.10)]);
        assert_eq!(proposal.entries[0].verdict, Verdict::Keep);
    }

    /// The accident does not become the keeper. A frame already condemned
    /// on its own account is out of the comparison, so a burst of good
    /// frames plus one lens-cap shot still crowns a good frame.
    #[test]
    fn a_condemned_frame_does_not_win_its_burst() {
        // The black frame has the highest focus of the three, which is the
        // trap: gradient energy over a nearly black frame is noise.
        let proposal = proposal(vec![
            sharp(1.0),
            sharp(2.0),
            measure_of(50.0, 0.0, 0.99, 0.005),
        ]);
        assert_eq!(proposal.entries[1].verdict, Verdict::Pick);
        assert_eq!(
            proposal.entries[2].verdict,
            Verdict::Reject(RejectReason::Black)
        );
    }

    /// Nothing in a proposal is a write, and nothing in it names one: the
    /// versions come back as a list for the caller to pass to `set_pick`
    /// if the photographer says so (ADR 0084 §2).
    #[test]
    fn a_proposal_that_proposes_nothing_is_empty_rather_than_absent() {
        let proposal = proposal(Vec::new());
        assert!(proposal.entries.is_empty());
        assert!(proposal.rejects().is_empty());
        assert_eq!(proposal.bursts(), 0);
    }
}
