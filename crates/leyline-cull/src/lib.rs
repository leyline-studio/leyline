//! Technical quality measures and burst fingerprints for assisted culling
//! ([ADR 0084](../../../docs/adr/0084-assisted-culling.md) §4).
//!
//! This crate is the half of assisted culling that needs **no model and no
//! weights**: focus and motion blur are gradient energy, clipping is a
//! histogram, and grouping a burst is a perceptual hash. It is ordinary image
//! processing, GPL-3.0-only, in the open repository, and it acts on the
//! largest number of photos — a shoot's rejects are mostly rejects for banal
//! reasons.
//!
//! It computes numbers and nothing else. It decides no verdict, applies no
//! rating, and knows nothing of the catalog: ADR 0084 §1 puts classification
//! behind an ordinary edit session, and §2 requires that a proposal be shown
//! before it is written. Turning these measures into a proposal is the
//! engine's business, not this crate's.
//!
//! # What the numbers are worth
//!
//! [`Quality::focus`] is **comparable within a group of similar frames, and
//! only weakly meaningful on its own**. That is a property of the measurement,
//! not a caveat about the implementation: gradient energy rises with detail as
//! much as with sharpness, so a sharp photo of a plain wall scores below a
//! blurred photo of a hedge. Every other tool that claims an absolute
//! "blurriness" number is either hiding this or has been tuned to one kind of
//! photograph.
//!
//! The consequence is a design rule rather than a footnote: **rank inside a
//! burst, do not threshold across a library**. [`sharpest`] exists so callers
//! fall into the right use, and the absolute score is exposed for display and
//! for tie-breaking, not for judgement.

#![forbid(unsafe_code)]

/// A frame to measure: 8-bit RGB, tightly packed, `width * height * 3` bytes.
///
/// Deliberately a borrowed view over whatever the caller already decoded. The
/// intended source is the thumbnail-sized decode of
/// [ADR 0083](../../../docs/adr/0083-scaled-jpeg-thumbnail-decode.md) — that
/// is what makes culling a shoot affordable at all, and [`Quality`] documents
/// what measuring at that scale costs.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes, R, G, B per pixel.
    pub rgb: &'a [u8],
}

/// What went wrong with a frame, in numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    /// Mean squared luma gradient, normalised by the frame's own contrast.
    ///
    /// Higher is sharper. Comparable **within** a group of frames of the same
    /// scene; see this module's header for why it is not a library-wide
    /// threshold.
    pub focus: f32,
    /// Fraction of pixels with at least one channel at 255, in `[0, 1]`.
    pub clipped_highlights: f32,
    /// Fraction of pixels whose channels are all at or below 2, in `[0, 1]`.
    pub crushed_shadows: f32,
    /// Mean luma over the frame, in `[0, 1]` — how a frame that is merely
    /// dark is told apart from one that is crushed.
    pub mean_luma: f32,
}

/// A 64-bit perceptual fingerprint, for telling a burst from a scene change.
///
/// A difference hash: the frame is reduced to a 9×8 luma grid and each bit
/// records whether a cell is brighter than the one to its right. It survives
/// resizing, re-encoding and small exposure shifts, which is exactly what
/// distinguishes two frames of one burst; it does not survive a change of
/// subject, which is the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint(pub u64);

impl Fingerprint {
    /// Bits that differ — 0 for identical frames, 64 for opposites.
    ///
    /// Frames of one burst typically sit in the low single digits. A
    /// threshold belongs to the caller: how tight "the same scene" is depends
    /// on whether the photographer is bracketing, panning or waiting.
    #[must_use]
    pub fn distance(self, other: Self) -> u32 {
        (self.0 ^ other.0).count_ones()
    }
}

/// Rec. 709 luma, the same weighting the rest of the platform uses, in
/// `[0, 255]`.
fn luma(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)
}

/// Luma plane of a frame, row-major.
fn luma_plane(frame: Frame) -> Vec<f32> {
    let pixels = (frame.width as usize) * (frame.height as usize);
    let mut plane = Vec::with_capacity(pixels);
    for p in 0..pixels {
        let i = p * 3;
        plane.push(luma(frame.rgb[i], frame.rgb[i + 1], frame.rgb[i + 2]));
    }
    plane
}

/// Measures one frame.
///
/// Returns `None` for a frame whose buffer does not match its dimensions, or
/// which is too small to have a gradient — a caller with a 1-pixel image has
/// a bug, and inventing a score for it would hide the bug rather than the
/// image.
#[must_use]
pub fn quality(frame: Frame) -> Option<Quality> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w < 3 || h < 3 || frame.rgb.len() != w * h * 3 {
        return None;
    }
    let plane = luma_plane(frame);

    // Focus: mean squared gradient over the interior, normalised by the
    // frame's own luma variance.
    //
    // The normalisation is what makes two frames of the *same* scene
    // comparable when one is a stop brighter: without it, raising exposure
    // raises the gradient and a brighter frame reads as a sharper one. It
    // does not, and cannot, make two different scenes comparable.
    let mut gradient_sum = 0.0f64;
    let mut samples = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let i = y * w + x;
            let dx = plane[i + 1] - plane[i - 1];
            let dy = plane[i + w] - plane[i - w];
            gradient_sum += f64::from(dx * dx + dy * dy);
            samples += 1;
        }
    }
    let mean = f64::from(plane.iter().sum::<f32>()) / (plane.len() as f64);
    let variance = plane
        .iter()
        .map(|v| {
            let d = f64::from(*v) - mean;
            d * d
        })
        .sum::<f64>()
        / (plane.len() as f64);

    // A frame of one flat colour has no gradient and no variance. Its focus
    // is 0, not a division by zero: there is nothing in it to be sharp.
    let focus = if variance < 1e-6 || samples == 0 {
        0.0
    } else {
        ((gradient_sum / samples as f64) / variance) as f32
    };

    // Exposure: clipping is counted per pixel, not per channel, because a
    // pixel with one blown channel is already unrecoverable colour.
    let mut clipped = 0u64;
    let mut crushed = 0u64;
    for p in 0..w * h {
        let i = p * 3;
        let (r, g, b) = (frame.rgb[i], frame.rgb[i + 1], frame.rgb[i + 2]);
        if r == 255 || g == 255 || b == 255 {
            clipped += 1;
        }
        if r <= 2 && g <= 2 && b <= 2 {
            crushed += 1;
        }
    }
    let pixels = (w * h) as f32;

    Some(Quality {
        focus,
        clipped_highlights: clipped as f32 / pixels,
        crushed_shadows: crushed as f32 / pixels,
        mean_luma: (mean / 255.0) as f32,
    })
}

/// Reduces a frame to a 64-bit perceptual fingerprint.
///
/// Returns `None` on a buffer that does not match its dimensions, or on a
/// frame smaller than the 9×8 grid the hash needs.
#[must_use]
pub fn fingerprint(frame: Frame) -> Option<Fingerprint> {
    const COLS: usize = 9;
    const ROWS: usize = 8;
    let (w, h) = (frame.width as usize, frame.height as usize);
    if w < COLS || h < ROWS || frame.rgb.len() != w * h * 3 {
        return None;
    }
    let plane = luma_plane(frame);

    // Box-average each cell rather than point-sample it: sampling one pixel
    // per cell makes the hash depend on where the grid happens to land, so
    // the same photo at two scales fingerprints differently — which would
    // defeat the only thing this hash is for.
    let mut cells = [[0.0f32; COLS]; ROWS];
    for (row, cell_row) in cells.iter_mut().enumerate() {
        let y0 = row * h / ROWS;
        let y1 = ((row + 1) * h / ROWS).max(y0 + 1);
        for (col, cell) in cell_row.iter_mut().enumerate() {
            let x0 = col * w / COLS;
            let x1 = ((col + 1) * w / COLS).max(x0 + 1);
            let mut sum = 0.0f64;
            let mut n = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += f64::from(plane[y * w + x]);
                    n += 1;
                }
            }
            *cell = (sum / f64::from(n)) as f32;
        }
    }

    let mut bits = 0u64;
    let mut bit = 0;
    for row in &cells {
        for col in 0..COLS - 1 {
            if row[col] > row[col + 1] {
                bits |= 1 << bit;
            }
            bit += 1;
        }
    }
    Some(Fingerprint(bits))
}

/// Index of the sharpest frame among `scores`, or `None` if empty.
///
/// The intended use of [`Quality::focus`], and the reason it is exposed at
/// all: ranking frames of one burst against each other. Ties go to the
/// earliest index, so a burst of identical frames keeps its first — an
/// arbitrary rule, but a stable one, and stability is what stops a re-run
/// from proposing a different keeper each time.
#[must_use]
pub fn sharpest(scores: &[Quality]) -> Option<usize> {
    scores
        .iter()
        .enumerate()
        .fold(None, |best: Option<(usize, f32)>, (i, q)| match best {
            Some((_, b)) if b >= q.focus => best,
            _ => Some((i, q.focus)),
        })
        .map(|(i, _)| i)
}

/// Groups consecutive frames whose fingerprints stay within `max_distance`.
///
/// Consecutive, not all-pairs: a burst is contiguous in capture order, and
/// comparing every pair would both cost O(n²) over a shoot and merge two
/// visits to the same place months apart. The caller is expected to pass
/// frames in capture order.
///
/// Returns one slice of indices per group, groups of one included — a photo
/// that belongs to no burst is a group with a single member, not an absence.
#[must_use]
pub fn group_bursts(prints: &[Fingerprint], max_distance: u32) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, print) in prints.iter().enumerate() {
        match groups.last_mut() {
            Some(group)
                if prints[*group.last().expect("a group is never empty")].distance(*print)
                    <= max_distance =>
            {
                group.push(i);
            }
            _ => groups.push(vec![i]),
        }
    }
    groups
}
