//! `monochrome` v1 (ADR 0088 §3): the collapse to black and white.
//!
//! **Frozen.** Published, therefore immutable: a revision citing
//! `monochrome: 1` renders through this code forever
//! (`docs/pipeline.md` §5.1). Changing the conversion means a `v2`.
//!
//! The operator itself is one line of arithmetic. Everything that makes it
//! a black-and-white *mixer* rather than a desaturation is its **rank**: at
//! 145 it runs after `hsl` (140), so the mixer's eight luminance sliders
//! decide how bright each hue arrives here, and therefore which grey it
//! becomes. Moving this stage earlier would turn it back into a plain
//! desaturation, which is why a rank is as frozen as a body.

use crate::pixels::{Pixels, luma};
use crate::stages::kernel;

/// Writes each pixel's luma to all three of its channels.
///
/// In the linear working buffer, without the display-gamma round trip
/// `kernel::v1::in_display` gives the chroma operators: luminance is a
/// linear quantity, and averaging it under a gamma curve is what makes
/// naive greyscale conversions come out muddy.
///
/// Rec. 2020's own coefficients, through the shared [`luma`] — the working
/// space's primaries, so the weights match the green the buffer actually
/// holds. sRGB's weights here would put the greens of a linear-Rec.-2020
/// buffer in the wrong grey.
///
/// No clamping and no headroom dance: a highlight above 1.0 keeps its
/// value, because a grey highlight is still a highlight (ADR 0044's
/// unbounded buffer) and clipping it here would be a decision belonging to
/// `output_rendering`.
pub(crate) fn monochrome(px: &mut Pixels) {
    kernel::v1::par_rows(px, |row| {
        for rgb in row.chunks_exact_mut(3) {
            let l = luma(rgb);
            rgb[0] = l;
            rgb[1] = l;
            rgb[2] = l;
        }
    });
}
