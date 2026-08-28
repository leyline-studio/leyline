//! What the culling measures must and must not claim (ADR 0084 §4).
//!
//! The tests that matter here are the ones pinning the *limits* of the
//! measures, not the ones showing they work on a happy case. A focus score
//! that ranks a blurred copy below its sharp original proves very little; a
//! test showing it must **not** be read as an absolute threshold across
//! different scenes is what stops the next caller from misusing it.

use leyline_cull::{Fingerprint, Frame, fingerprint, group_bursts, quality, sharpest};

const W: u32 = 160;
const H: u32 = 120;

/// A deterministic textured frame: fine checker detail over a gradient, so
/// blurring it has something to destroy.
fn textured(seed: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity((W * H * 3) as usize);
    for y in 0..H {
        for x in 0..W {
            let checker = if (x / 2 + y / 2) % 2 == 0 { 90 } else { 165 };
            let ramp = (x * 40 / W) as u8;
            let v = (checker as u8).wrapping_add(ramp).wrapping_add(seed);
            data.extend_from_slice(&[v, v, v]);
        }
    }
    data
}

/// A flat frame of one value — no detail at all.
fn flat(value: u8) -> Vec<u8> {
    vec![value; (W * H * 3) as usize]
}

/// Separable 3-tap blur, applied `passes` times: a stand-in for missed focus.
fn blur(src: &[u8], passes: usize) -> Vec<u8> {
    let mut cur = src.to_vec();
    for _ in 0..passes {
        let mut out = cur.clone();
        for y in 0..H as usize {
            for x in 1..W as usize - 1 {
                for c in 0..3 {
                    let i = (y * W as usize + x) * 3 + c;
                    let a = u16::from(cur[i - 3]);
                    let b = u16::from(cur[i]);
                    let d = u16::from(cur[i + 3]);
                    out[i] = ((a + 2 * b + d) / 4) as u8;
                }
            }
        }
        cur = out.clone();
        let mut out = cur.clone();
        for y in 1..H as usize - 1 {
            for x in 0..W as usize {
                for c in 0..3 {
                    let i = (y * W as usize + x) * 3 + c;
                    let a = u16::from(cur[i - W as usize * 3]);
                    let b = u16::from(cur[i]);
                    let d = u16::from(cur[i + W as usize * 3]);
                    out[i] = ((a + 2 * b + d) / 4) as u8;
                }
            }
        }
        cur = out;
    }
    cur
}

fn frame(rgb: &[u8]) -> Frame<'_> {
    Frame {
        width: W,
        height: H,
        rgb,
    }
}

#[test]
fn blurring_a_frame_lowers_its_focus_score() {
    let sharp = textured(0);
    let soft = blur(&sharp, 2);
    let softer = blur(&sharp, 6);

    let qs = quality(frame(&sharp)).unwrap();
    let qm = quality(frame(&soft)).unwrap();
    let ql = quality(frame(&softer)).unwrap();

    assert!(
        qs.focus > qm.focus && qm.focus > ql.focus,
        "focus must fall monotonically with blur: {} then {} then {}",
        qs.focus,
        qm.focus,
        ql.focus
    );
}

#[test]
fn the_focus_score_is_not_an_absolute_threshold_across_scenes() {
    // The limit this crate documents, pinned so nobody "fixes" it into a
    // library-wide blur threshold: a *blurred* detailed frame can outscore a
    // *sharp* plain one, because gradient energy measures detail as much as
    // sharpness.
    let detailed_but_blurred = blur(&textured(0), 2);
    let sharp_but_plain = {
        // Perfectly sharp edge, but almost no detail: two flat halves.
        let mut data = flat(60);
        for y in 0..H as usize {
            for x in (W as usize / 2)..W as usize {
                let i = (y * W as usize + x) * 3;
                data[i] = 200;
                data[i + 1] = 200;
                data[i + 2] = 200;
            }
        }
        data
    };

    let blurred = quality(frame(&detailed_but_blurred)).unwrap();
    let plain = quality(frame(&sharp_but_plain)).unwrap();

    assert!(
        blurred.focus > plain.focus,
        "a blurred detailed frame ({}) is expected to outscore a sharp plain \
         one ({}) — this is the documented limit of the measure, and the \
         reason `sharpest` ranks within a burst instead of thresholding",
        blurred.focus,
        plain.focus
    );
}

#[test]
fn exposure_tells_a_dark_frame_from_a_crushed_one() {
    let dark = quality(frame(&flat(12))).unwrap();
    let crushed = quality(frame(&flat(0))).unwrap();
    let blown = quality(frame(&flat(255))).unwrap();

    assert_eq!(dark.crushed_shadows, 0.0, "dark is not crushed");
    assert!(dark.mean_luma < 0.1, "but it is dark: {}", dark.mean_luma);
    assert_eq!(crushed.crushed_shadows, 1.0);
    assert_eq!(blown.clipped_highlights, 1.0);
    assert_eq!(blown.crushed_shadows, 0.0);
}

#[test]
fn a_flat_frame_has_no_focus_rather_than_a_division_by_zero() {
    let q = quality(frame(&flat(128))).unwrap();
    assert_eq!(q.focus, 0.0);
    assert!(q.focus.is_finite());
}

#[test]
fn a_frame_whose_buffer_does_not_match_its_size_is_refused() {
    let short = vec![0u8; 10];
    assert!(quality(frame(&short)).is_none());
    assert!(fingerprint(frame(&short)).is_none());
}

#[test]
fn the_fingerprint_survives_blur_and_exposure_but_not_a_new_subject() {
    let base = textured(0);
    let softened = blur(&base, 2);
    // A stop brighter, clamped — what bracketing does inside one burst.
    let brighter: Vec<u8> = base.iter().map(|v| v.saturating_add(40)).collect();
    // A different scene: the texture mirrored, so the same statistics with a
    // different layout.
    let mirrored = {
        let mut out = base.clone();
        for y in 0..H as usize {
            for x in 0..W as usize {
                let src = (y * W as usize + (W as usize - 1 - x)) * 3;
                let dst = (y * W as usize + x) * 3;
                out[dst..dst + 3].copy_from_slice(&base[src..src + 3]);
            }
        }
        out
    };

    let a = fingerprint(frame(&base)).unwrap();
    assert!(
        a.distance(fingerprint(frame(&softened)).unwrap()) <= 4,
        "blur must not break the grouping"
    );
    assert!(
        a.distance(fingerprint(frame(&brighter)).unwrap()) <= 4,
        "a brighter frame of the same scene stays in the burst"
    );
    assert!(
        a.distance(fingerprint(frame(&mirrored)).unwrap()) > 8,
        "a different layout must leave the burst"
    );
}

#[test]
fn sharpest_ranks_within_a_burst_and_breaks_ties_stably() {
    let sharp = textured(0);
    let soft = blur(&sharp, 3);
    let scores = [
        quality(frame(&soft)).unwrap(),
        quality(frame(&sharp)).unwrap(),
        quality(frame(&soft)).unwrap(),
    ];
    assert_eq!(sharpest(&scores), Some(1));

    // Identical frames: the first wins, every time, so a re-run proposes the
    // same keeper.
    let flat_scores = [scores[0], scores[0], scores[0]];
    assert_eq!(sharpest(&flat_scores), Some(0));
    assert_eq!(sharpest(&[]), None);
}

#[test]
fn bursts_group_consecutively_and_a_lone_photo_is_a_group_of_one() {
    // Two bursts of three, separated by one unrelated frame.
    let prints = [
        Fingerprint(0b0000),
        Fingerprint(0b0001),
        Fingerprint(0b0011),
        Fingerprint(u64::MAX),
        Fingerprint(0b1100_0000),
        Fingerprint(0b1100_0001),
    ];
    let groups = group_bursts(&prints, 2);
    assert_eq!(groups, vec![vec![0, 1, 2], vec![3], vec![4, 5]]);
}

#[test]
fn the_same_scene_revisited_later_is_not_folded_into_one_burst() {
    // Identical fingerprints, but not adjacent in capture order: two visits
    // to the same place, which `group_bursts` must keep apart.
    let prints = [
        Fingerprint(0b0000),
        Fingerprint(u64::MAX),
        Fingerprint(0b0000),
    ];
    assert_eq!(group_bursts(&prints, 2), vec![vec![0], vec![1], vec![2]]);
}
