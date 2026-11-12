//! Backdrop extension v1 (ADR 0172) — rank 32, after spot removal and
//! before every tonal stage.
//!
//! For each region: what lies inside the traced source polygon, minus the
//! protected subjects, is **measured**; what lies in the zone is
//! **replaced** by the source's slow field extrapolated outwards (a
//! push-pull pyramid) plus a synthetic mottle whose energy, per frequency
//! band and per direction, is the one measured on the source (ADR 0172 §3).
//!
//! Every length is a fraction of the buffer's larger side, and the noise is
//! a function of the position normalised the same way, so a preview and an
//! export compute the same picture at two resolutions with no seed stored.
//! Sums are `f64`; nothing reads a clock, a random generator or the
//! scheduling of the rows.
//!
//! **Frozen.** Its pixels are part of the reproducibility contract
//! (`docs/pipeline.md` §5.1): a revision citing this stage version renders
//! through exactly this code, forever. A change of rendering is a new
//! version module next to this one, never an edit here (ADR 0042 §1).

use leyline_core::{Backdrop, BackdropRegion, BackdropZone, Point, ProtectWithin};
use rayon::prelude::*;

use crate::mask::rasterize_coverage;
use crate::mask_coverage::MaskCoverages;
use crate::pixels::Pixels;
use crate::stages::kernel::v1::post_rotation_point_to_buffer;

/// Erosion of the measured area beyond the feather, so neither a cloth's hem
/// nor a subject's edge is measured as mottle.
const MEASURE_MARGIN: f64 = 0.002;

/// Radius of the normalised convolution that gives the slow field, and of
/// the smoothing applied once it is extrapolated.
const SLOW_RADIUS: f64 = 0.0156;
const SLOW_SMOOTHING: f64 = 0.0094;

/// The five band edges of the mottle (box radii); a band is the difference
/// between two successive blurs. Measured on a muslin backdrop at 1600 px:
/// 1, 3, 8, 20 and 45 px.
const BAND_RADII: [f64; 6] = [0.0, 0.000_625, 0.001_875, 0.005, 0.0125, 0.028_125];

/// Below this many measured pixels a region has nothing to say, and is
/// left untouched rather than filled with a guess.
const MIN_KNOWN_PIXELS: usize = 64;

/// The coarsest band also holds what is left of the lighting once the slow
/// field is taken out; at full weight it painted blotches twice the cloth's
/// (measured on the muslin session, ADR 0172 finding 3).
const COARSEST_BAND_WEIGHT: f32 = 0.5;

/// Below this linear value the logarithm is clamped: a black pixel has no
/// mottle to measure.
const LOG_FLOOR: f32 = 1e-4;

/// How far each part of the stage may be worked on a reduced buffer: the
/// smallest blur radius, in pixels, a reduction may leave. A slow field
/// and a coarse mottle band are smooth over many pixels, so they are
/// measured and synthesised small, then enlarged: the full-resolution
/// blurs were most of the stage's time (ADR 0172 §6).
#[derive(Clone, Copy)]
struct Floors {
    /// For each mottle band (its finer radius) and the channel gains.
    mottle: f64,
    /// For the slow field's smoothing radius. Higher: its push-pull fill
    /// is not scale-free, and at 3 px the level near the cloth's edge
    /// moved by a percent.
    slow: f64,
}

impl Floors {
    const PRODUCTION: Self = Self {
        mottle: 3.0,
        slow: 8.0,
    };
    /// Everything at full resolution: what a test compares against.
    #[cfg(test)]
    const NONE: Self = Self {
        mottle: f64::INFINITY,
        slow: f64::INFINITY,
    };
}

/// The robust standard deviation of a normal law from its median absolute
/// deviation.
const MAD_TO_SIGMA: f64 = 1.4826;

/// Applies every region, in list order: a later region measures what an
/// earlier one wrote, and so wins where two zones overlap.
pub(crate) fn backdrop(
    px: &mut Pixels,
    backdrop: &Backdrop,
    rotation_degrees: f64,
    coverages: &MaskCoverages,
) {
    backdrop_at(
        px,
        backdrop,
        rotation_degrees,
        coverages,
        Floors::PRODUCTION,
    );
}

/// [`backdrop`] with the reduction floors as a parameter, so a test can
/// compare the reduced path with the full-resolution one.
fn backdrop_at(
    px: &mut Pixels,
    backdrop: &Backdrop,
    rotation_degrees: f64,
    coverages: &MaskCoverages,
    floors: Floors,
) {
    let release = union_of(
        backdrop
            .release
            .iter()
            .map(|mask| rasterize_coverage(mask, coverages, px.width, px.height, rotation_degrees)),
        px.width as usize * px.height as usize,
    );
    for region in &backdrop.regions {
        let shapes = RegionShapes::rasterize(region, px.width, px.height, rotation_degrees);
        let protect: Vec<f32> = {
            let mut protect = vec![0.0f32; px.width as usize * px.height as usize];
            for entry in &backdrop.protect {
                let coverage = rasterize_coverage(
                    &entry.mask,
                    coverages,
                    px.width,
                    px.height,
                    rotation_degrees,
                );
                for ((p, c), s) in protect.iter_mut().zip(&coverage).zip(&shapes.source) {
                    let within = match entry.within {
                        ProtectWithin::Everywhere => 1.0,
                        ProtectWithin::Source => *s,
                        ProtectWithin::OutsideSource => 1.0 - *s,
                    };
                    *p = p.max(c * within);
                }
            }
            protect
                .iter()
                .zip(&release)
                .map(|(p, r)| p * (1.0 - r))
                .collect()
        };
        fill_region(
            px,
            &shapes,
            &protect,
            backdrop.feather,
            backdrop.texture,
            floors,
        );
    }
}

/// The hard masks of one region on the buffer: 1 inside, 0 outside.
struct RegionShapes {
    source: Vec<f32>,
    zone: Vec<f32>,
}

impl RegionShapes {
    fn rasterize(region: &BackdropRegion, width: u32, height: u32, rotation: f64) -> Self {
        let polygon = |points: &[Point]| rasterize_polygon(points, width, height, rotation);
        let mut source = polygon(&region.source);
        let mut zone = match &region.zone {
            BackdropZone::OutsideSource => source.iter().map(|s| 1.0 - s).collect(),
            BackdropZone::Polygons(polygons) => {
                union_of(polygons.iter().map(|p| polygon(p)), source.len())
            }
        };
        for hole in &region.holes {
            for ((s, z), h) in source.iter_mut().zip(zone.iter_mut()).zip(polygon(hole)) {
                *s *= 1.0 - h;
                *z = z.max(h);
            }
        }
        Self { source, zone }
    }
}

/// Measures one region and writes its zone.
fn fill_region(
    px: &mut Pixels,
    shapes: &RegionShapes,
    protect: &[f32],
    feather: f64,
    texture: i32,
    floors: Floors,
) {
    let (w, h) = (px.width as usize, px.height as usize);
    let side = w.max(h) as f64;
    let radius = |fraction: f64| (fraction * side).round() as usize;

    // Where the fill goes: the zone with a feathered edge, never a subject.
    let feather_px = radius(feather);
    let fill: Vec<f32> = blur3(&shapes.zone, w, h, feather_px)
        .iter()
        .zip(protect)
        .map(|(z, p)| z * (1.0 - p))
        .collect();
    if fill.iter().all(|f| *f <= 0.0) {
        return;
    }

    // What is measured: the source without the subjects, eroded.
    let free: Vec<f32> = shapes
        .source
        .iter()
        .zip(protect)
        .map(|(s, p)| s * (1.0 - p))
        .collect();
    let known: Vec<f32> = blur3(&free, w, h, feather_px + radius(MEASURE_MARGIN).max(1))
        .iter()
        .map(|k| if *k > 0.999 { 1.0 } else { 0.0 })
        .collect();
    if known.iter().filter(|k| **k > 0.0).count() < MIN_KNOWN_PIXELS {
        return;
    }

    let channels = split_channels(px);
    let slow = slow_field(
        &channels,
        &known,
        w,
        h,
        radius(SLOW_RADIUS),
        radius(SLOW_SMOOTHING),
        floors.slow,
    );
    let mottle = mottle(&channels, &known, w, h, side, texture, floors.mottle);

    for (i, f) in fill.iter().enumerate() {
        if *f <= 0.0 {
            continue;
        }
        for (c, plane) in slow.iter().enumerate() {
            // Multiplicative: a mottle is a pattern of reflectance, so it
            // is as strong, relatively, at the dark edge as at the lit
            // centre.
            let synthetic = plane[i] * (mottle.gains[c] * mottle.field[i]).exp();
            let value = &mut px.data[i * 3 + c];
            *value += (synthetic - *value) * f;
        }
    }
}

/// The source's slow field, extrapolated into the whole buffer. Worked on
/// a buffer reduced so the smoothing radius stays above the floor.
fn slow_field(
    channels: &[Vec<f32>; 3],
    known: &[f32],
    w: usize,
    h: usize,
    radius: usize,
    smoothing: usize,
    min_reduced_radius: f64,
) -> [Vec<f32>; 3] {
    let k = reduction_for(smoothing as f64, min_reduced_radius);
    if k > 1 {
        let low = Reduced::new(w, h, k);
        let known_low = low.known(known);
        // A source narrower than one reduced block has nothing to say at
        // that scale: measure it at full resolution instead.
        if known_low.iter().any(|k| *k > 0.0) {
            let planes = channels.each_ref().map(|plane| low.reduce(plane));
            let scaled = |r: usize| ((r as f64 / k as f64).round() as usize).max(1);
            let field = slow_field_on(
                &planes,
                &known_low,
                low.w,
                low.h,
                scaled(radius),
                scaled(smoothing),
            );
            return field.map(|plane| low.enlarge(&plane));
        }
    }
    let planes = channels.each_ref().map(|plane| plane.as_slice());
    slow_field_on(&planes, known, w, h, radius, smoothing)
}

fn slow_field_on<P: AsRef<[f32]>>(
    channels: &[P; 3],
    known: &[f32],
    w: usize,
    h: usize,
    radius: usize,
    smoothing: usize,
) -> [Vec<f32>; 3] {
    let density = blur3(known, w, h, radius);
    let trusted: Vec<f32> = density
        .iter()
        .map(|d| if *d > 0.7 { 1.0 } else { 0.0 })
        .collect();
    let extend = |plane: &[f32]| {
        let weighted: Vec<f32> = plane.iter().zip(known).map(|(v, k)| v * k).collect();
        let local: Vec<f32> = blur3(&weighted, w, h, radius)
            .iter()
            .zip(&density)
            .map(|(v, d)| if *d > 1e-4 { v / d } else { 0.0 })
            .collect();
        blur3(&push_pull(&local, &trusted, w, h), w, h, smoothing)
    };
    channels.each_ref().map(|plane| extend(plane.as_ref()))
}

/// The synthetic mottle as a log factor, and how each channel follows it.
struct Mottle {
    field: Vec<f32>,
    gains: [f32; 3],
}

/// Measures the source's mottle band by band and direction by direction,
/// on the logarithm of the luminance, then synthesises a field of the same
/// energy everywhere. Each band is measured and synthesised on a buffer
/// reduced until its finer radius reaches the floor, then enlarged.
///
/// The logarithm because a cloth's mottle multiplies the light falling on
/// it: measured additively in linear light, the lit centre's amplitude
/// painted blotches twice too dark at the cloth's darker edge.
fn mottle(
    channels: &[Vec<f32>; 3],
    known: &[f32],
    w: usize,
    h: usize,
    side: f64,
    texture: i32,
    min_reduced_radius: f64,
) -> Mottle {
    let log = |v: f32| v.max(LOG_FLOOR).ln();
    let luma: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map(|i| log((channels[0][i] + channels[1][i] + channels[2][i]) / 3.0))
        .collect();
    let mut scales = Scales::new(&luma, known, w, h);
    let pixels = |r: f64| r * side;
    let amount = texture as f32 / 100.0;
    let mut field = vec![0.0f32; w * h];
    for band in 0..BAND_RADII.len() - 1 {
        let (fine_px, coarse_px) = (pixels(BAND_RADII[band]), pixels(BAND_RADII[band + 1]));
        let k = reduction_for(fine_px, min_reduced_radius);
        let scaled = |r: f64| (r / k as f64).round() as usize;
        let (fine_r, coarse_r) = (scaled(fine_px), scaled(coarse_px));
        scales.prepare(k, &[fine_r, coarse_r]);
        let (lw, lh) = scales.size(k);
        let known_low = scales.known(k);
        let detail: Vec<f32> = scales
            .level(k, fine_r)
            .iter()
            .zip(scales.level(k, coarse_r))
            .map(|(a, b)| a - b)
            .collect();
        let sigma = robust_sigma(
            detail
                .iter()
                .zip(known_low)
                .filter(|(_, k)| **k > 0.0)
                .map(|(d, _)| *d),
        );
        if sigma <= 0.0 {
            continue;
        }
        // Folds and seams have more energy across than along: stretch the
        // noise along them, by the square root of the ratio each way.
        let across = robust_sigma(gradients(&detail, known_low, lw, lh, 1, 0));
        let along = robust_sigma(gradients(&detail, known_low, lw, lh, 0, 1));
        // A ratio, bounded to 16 each way: a perfectly vertical fold has
        // no energy along it at all.
        let stretch = if across.max(along) > 0.0 {
            (across.max(along / 16.0) / along.max(across / 16.0)).sqrt()
        } else {
            1.0
        };
        // The noise goes through the very band filter that measured the
        // source, then is scaled by its own deviation: each band carries the
        // measured energy and nothing leaks into its neighbours. Its lattice
        // is placed in full-resolution coordinates, so a reduced band draws
        // the same pattern, only sampled more sparsely.
        let cell = BAND_RADII[band].max(1.0 / side);
        let (cell_x, cell_y) = (cell / stretch, cell * stretch);
        let step = k as f64;
        let mut noise = vec![0.0f32; lw * lh];
        noise.par_chunks_mut(lw).enumerate().for_each(|(y, row)| {
            let v = (y as f64 + 0.5) * step / side;
            for (x, out) in row.iter_mut().enumerate() {
                let u = (x as f64 + 0.5) * step / side;
                *out = value_noise((u / cell_x) as f32, (v / cell_y) as f32, band as u32);
            }
        });
        let fine_noise = blur3(&noise, lw, lh, fine_r);
        let coarse_noise = blur3(&noise, lw, lh, coarse_r);
        let banded: Vec<f32> = fine_noise
            .iter()
            .zip(&coarse_noise)
            .map(|(a, b)| a - b)
            .collect();
        let noise_sigma = standard_deviation(&banded);
        if noise_sigma <= 0.0 {
            continue;
        }
        let weight = if band == BAND_RADII.len() - 2 {
            COARSEST_BAND_WEIGHT
        } else {
            1.0
        };
        let scale = (sigma / noise_sigma) as f32 * amount * weight;
        let banded = if k > 1 {
            Reduced::new(w, h, k).enlarge(&banded)
        } else {
            banded
        };
        field
            .par_iter_mut()
            .zip(&banded)
            .for_each(|(out, n)| *out += scale * n);
    }

    // Each channel follows the luminance mottle by its own regression gain,
    // each measured against its own coarse level.
    let coarsest_px = pixels(BAND_RADII[BAND_RADII.len() - 1]);
    let kc = reduction_for(coarsest_px, min_reduced_radius);
    let coarsest = (coarsest_px / kc as f64).round() as usize;
    let low = Reduced::new(w, h, kc);
    let coarse_of = |plane: &[f32]| {
        if kc > 1 {
            low.enlarge(&blur3(&low.reduce(plane), low.w, low.h, coarsest))
        } else {
            blur3(plane, w, h, coarsest)
        }
    };
    let base = coarse_of(&luma);
    let channel_detail: Vec<Vec<f32>> = channels
        .iter()
        .map(|plane| {
            let logged: Vec<f32> = plane.par_iter().map(|v| log(*v)).collect();
            let coarse = coarse_of(&logged);
            logged.iter().zip(&coarse).map(|(a, b)| a - b).collect()
        })
        .collect();
    let mut gains = [1.0f32; 3];
    let (mut num, mut den) = ([0.0f64; 3], 0.0f64);
    for i in (0..w * h).filter(|i| known[*i] > 0.0) {
        let l = f64::from(luma[i] - base[i]);
        den += l * l;
        for c in 0..3 {
            num[c] += f64::from(channel_detail[c][i]) * l;
        }
    }
    if den > 0.0 {
        for c in 0..3 {
            gains[c] = (num[c] / den) as f32;
        }
    }
    Mottle { field, gains }
}

/// The reduction factor that keeps `radius_px` at or above the floor: 1,
/// full resolution, when it is already small.
fn reduction_for(radius_px: f64, min_reduced_radius: f64) -> usize {
    ((radius_px / min_reduced_radius).floor() as usize).max(1)
}

/// A buffer reduced by an integer factor: each reduced pixel is the mean
/// of a `k`×`k` block (a partial block at the right and bottom edges).
struct Reduced {
    k: usize,
    full_w: usize,
    full_h: usize,
    w: usize,
    h: usize,
}

impl Reduced {
    fn new(full_w: usize, full_h: usize, k: usize) -> Self {
        Self {
            k,
            full_w,
            full_h,
            w: full_w.div_ceil(k),
            h: full_h.div_ceil(k),
        }
    }

    /// Block means.
    fn reduce(&self, plane: &[f32]) -> Vec<f32> {
        self.blocks(plane, |values| {
            values.iter().map(|v| f64::from(*v)).sum::<f64>() as f32 / values.len() as f32
        })
    }

    /// A block is measured only when every pixel of it is: the measure
    /// never reaches past the eroded source.
    fn known(&self, known: &[f32]) -> Vec<f32> {
        self.blocks(known, |values| {
            if values.iter().all(|k| *k > 0.0) {
                1.0
            } else {
                0.0
            }
        })
    }

    fn blocks(&self, plane: &[f32], of: impl Fn(&[f32]) -> f32 + Sync) -> Vec<f32> {
        let mut out = vec![0.0f32; self.w * self.h];
        out.par_chunks_mut(self.w)
            .enumerate()
            .for_each(|(by, row)| {
                let mut values = Vec::with_capacity(self.k * self.k);
                for (bx, out) in row.iter_mut().enumerate() {
                    values.clear();
                    for y in by * self.k..((by + 1) * self.k).min(self.full_h) {
                        let start = y * self.full_w;
                        let x_end = ((bx + 1) * self.k).min(self.full_w);
                        values.extend_from_slice(&plane[start + bx * self.k..start + x_end]);
                    }
                    *out = of(&values);
                }
            });
        out
    }

    /// Bilinear enlargement back to full resolution, reduced pixel centres
    /// at the centres of their blocks.
    fn enlarge(&self, low: &[f32]) -> Vec<f32> {
        let axis = |full: usize, reduced: usize| -> Vec<(usize, usize, f32)> {
            (0..full)
                .map(|x| {
                    let at =
                        ((x as f64 + 0.5) / self.k as f64 - 0.5).clamp(0.0, (reduced - 1) as f64);
                    let i = at.floor() as usize;
                    (i, (i + 1).min(reduced - 1), (at - i as f64) as f32)
                })
                .collect()
        };
        let (columns, rows) = (axis(self.full_w, self.w), axis(self.full_h, self.h));
        let mut out = vec![0.0f32; self.full_w * self.full_h];
        out.par_chunks_mut(self.full_w)
            .zip(&rows)
            .for_each(|(row, (y0, y1, ty))| {
                let (top, bottom) = (&low[y0 * self.w..], &low[y1 * self.w..]);
                for (out, (x0, x1, tx)) in row.iter_mut().zip(&columns) {
                    let upper = top[*x0] + (top[*x1] - top[*x0]) * tx;
                    let lower = bottom[*x0] + (bottom[*x1] - bottom[*x0]) * tx;
                    *out = upper + (lower - upper) * ty;
                }
            });
        out
    }
}

/// The luminance and its measured pixels at each reduction a band asks
/// for, and their blurred levels, each computed once: neighbouring bands
/// share a radius.
struct Scales<'a> {
    luma: &'a [f32],
    known: &'a [f32],
    w: usize,
    h: usize,
    reduced: std::collections::HashMap<usize, (Vec<f32>, Vec<f32>)>,
    levels: std::collections::HashMap<(usize, usize), Vec<f32>>,
}

impl<'a> Scales<'a> {
    fn new(luma: &'a [f32], known: &'a [f32], w: usize, h: usize) -> Self {
        Self {
            luma,
            known,
            w,
            h,
            reduced: Default::default(),
            levels: Default::default(),
        }
    }

    fn size(&self, k: usize) -> (usize, usize) {
        let low = Reduced::new(self.w, self.h, k);
        (low.w, low.h)
    }

    fn prepare(&mut self, k: usize, radii: &[usize]) {
        if k > 1 && !self.reduced.contains_key(&k) {
            let low = Reduced::new(self.w, self.h, k);
            self.reduced
                .insert(k, (low.reduce(self.luma), low.known(self.known)));
        }
        let (w, h) = self.size(k);
        for radius in radii {
            if !self.levels.contains_key(&(k, *radius)) {
                let level = blur3(self.plane(k), w, h, *radius);
                self.levels.insert((k, *radius), level);
            }
        }
    }

    fn plane(&self, k: usize) -> &[f32] {
        if k > 1 {
            &self.reduced[&k].0
        } else {
            self.luma
        }
    }

    fn known(&self, k: usize) -> &[f32] {
        if k > 1 {
            &self.reduced[&k].1
        } else {
            self.known
        }
    }

    fn level(&self, k: usize, radius: usize) -> &[f32] {
        &self.levels[&(k, radius)]
    }
}

/// Differences between a pixel and its neighbour `(dx, dy)` away, where both
/// are measured.
fn gradients<'a>(
    plane: &'a [f32],
    known: &'a [f32],
    w: usize,
    h: usize,
    dx: usize,
    dy: usize,
) -> impl Iterator<Item = f32> + 'a {
    (0..h.saturating_sub(dy)).flat_map(move |y| {
        (0..w.saturating_sub(dx)).filter_map(move |x| {
            let (a, b) = (y * w + x, (y + dy) * w + x + dx);
            (known[a] > 0.0 && known[b] > 0.0).then(|| plane[b] - plane[a])
        })
    })
}

/// Median absolute deviation, scaled to a standard deviation: a fold or a
/// hem that slipped into the measure does not inflate it (ADR 0172,
/// finding 3).
fn robust_sigma(values: impl Iterator<Item = f32>) -> f64 {
    let mut values: Vec<f32> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    let median = select_median(&mut values);
    let mut deviations: Vec<f32> = values.iter().map(|v| (v - median).abs()).collect();
    MAD_TO_SIGMA * f64::from(select_median(&mut deviations))
}

fn standard_deviation(values: &[f32]) -> f64 {
    let n = values.len() as f64;
    let mean = values.iter().map(|v| f64::from(*v)).sum::<f64>() / n;
    (values
        .iter()
        .map(|v| (f64::from(*v) - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt()
}

fn select_median(values: &mut [f32]) -> f32 {
    let middle = values.len() / 2;
    *values.select_nth_unstable_by(middle, f32::total_cmp).1
}

/// Fills where `weight` is 0 from the nearest weighted values, coarse to
/// fine: average down with the weights, fill the coarse level, bring it
/// back up where the fine level had nothing.
fn push_pull(values: &[f32], weight: &[f32], w: usize, h: usize) -> Vec<f32> {
    let total: f64 = weight.iter().map(|x| f64::from(*x)).sum();
    if w <= 2 || h <= 2 || total == 0.0 {
        let sum: f64 = values
            .iter()
            .zip(weight)
            .map(|(v, k)| f64::from(v * k))
            .sum();
        let mean = if total > 0.0 {
            (sum / total) as f32
        } else {
            0.0
        };
        return vec![mean; w * h];
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut coarse_values = vec![0.0f32; cw * ch];
    let mut coarse_weight = vec![0.0f32; cw * ch];
    for y in 0..h {
        for x in 0..w {
            let (i, c) = (y * w + x, (y / 2) * cw + x / 2);
            coarse_values[c] += values[i] * weight[i];
            coarse_weight[c] += weight[i];
        }
    }
    for (v, k) in coarse_values.iter_mut().zip(&coarse_weight) {
        if *k > 0.0 {
            *v /= k;
        }
    }
    let coarse_weight: Vec<f32> = coarse_weight.iter().map(|k| k.min(1.0)).collect();
    let coarse = push_pull(&coarse_values, &coarse_weight, cw, ch);
    let upsampled: Vec<f32> = (0..w * h)
        .map(|i| coarse[(i / w / 2) * cw + (i % w) / 2])
        .collect();
    let upsampled = blur3(&upsampled, w, h, 1);
    values
        .iter()
        .zip(weight)
        .zip(&upsampled)
        .map(|((v, k), u)| v * k + u * (1.0 - k))
        .collect()
}

/// Three box blurs of radius `radius`, edges replicated: close to a
/// Gaussian, in linear time whatever the radius. Radius 0 is the identity.
fn blur3(plane: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 {
        return plane.to_vec();
    }
    let mut out = plane.to_vec();
    for _ in 0..3 {
        out = box_rows(&out, w, h, radius);
        out = box_columns(&out, w, h, radius);
    }
    out
}

/// One horizontal box blur, each row's sums kept in `f64`.
fn box_rows(plane: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h];
    let span = (2 * radius + 1) as f64;
    out.par_chunks_mut(w).enumerate().for_each(|(y, row_out)| {
        let row = &plane[y * w..(y + 1) * w];
        let at = |x: isize| f64::from(row[x.clamp(0, w as isize - 1) as usize]);
        let r = radius as isize;
        let mut sum: f64 = (-r..=r).map(at).sum();
        for (x, out) in row_out.iter_mut().enumerate() {
            *out = (sum / span) as f32;
            let x = x as isize;
            sum += at(x + r + 1) - at(x - r);
        }
    });
    out
}

/// The vertical counterpart of [`box_rows`]: the same `f64` sums in the
/// same order, so the result is the one a transposed horizontal pass gave,
/// bit for bit. Walking rows top to bottom with one running sum per column
/// keeps every read contiguous, where transposing a full-resolution plane
/// twice per pass was most of the stage's time (ADR 0172 §6).
fn box_columns(plane: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    const STRIP: usize = 256;
    let span = (2 * radius + 1) as f64;
    let r = radius as isize;
    let row_at = |y: isize| y.clamp(0, h as isize - 1) as usize * w;
    let strips: Vec<Vec<f32>> = (0..w.div_ceil(STRIP))
        .into_par_iter()
        .map(|strip| {
            let (x0, x1) = (strip * STRIP, ((strip + 1) * STRIP).min(w));
            let sw = x1 - x0;
            let mut sums = vec![0.0f64; sw];
            for k in -r..=r {
                let row = &plane[row_at(k) + x0..row_at(k) + x1];
                for (sum, v) in sums.iter_mut().zip(row) {
                    *sum += f64::from(*v);
                }
            }
            let mut out = vec![0.0f32; sw * h];
            for (y, out_row) in out.chunks_mut(sw).enumerate() {
                for (o, sum) in out_row.iter_mut().zip(&sums) {
                    *o = (sum / span) as f32;
                }
                let y = y as isize;
                let (enter, leave) = (row_at(y + r + 1) + x0, row_at(y - r) + x0);
                for (i, sum) in sums.iter_mut().enumerate() {
                    *sum += f64::from(plane[enter + i]) - f64::from(plane[leave + i]);
                }
            }
            out
        })
        .collect();
    let mut out = vec![0.0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (strip, values) in strips.iter().enumerate() {
            let x0 = strip * STRIP;
            let sw = values.len() / h;
            row[x0..x0 + sw].copy_from_slice(&values[y * sw..(y + 1) * sw]);
        }
    });
    out
}

fn split_channels(px: &Pixels) -> [Vec<f32>; 3] {
    let channel = |c: usize| px.data.iter().skip(c).step_by(3).copied().collect();
    [channel(0), channel(1), channel(2)]
}

/// The pointwise maximum of several coverages; all zero when there are none.
fn union_of(coverages: impl Iterator<Item = Vec<f32>>, len: usize) -> Vec<f32> {
    let mut union = vec![0.0f32; len];
    for coverage in coverages {
        for (u, c) in union.iter_mut().zip(coverage) {
            *u = u.max(c);
        }
    }
    union
}

/// A polygon given in the post-rotation referential (ADR 0026), filled on
/// the still-unrotated buffer with the even-odd rule at pixel centres.
/// Rotation is affine, so the polygon's edges stay straight on the buffer.
fn rasterize_polygon(points: &[Point], width: u32, height: u32, rotation: f64) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let vertices: Vec<(f64, f64)> = points
        .iter()
        .map(|p| post_rotation_point_to_buffer(width, height, rotation, *p))
        .collect();
    let mut out = vec![0.0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let yc = y as f64 + 0.5;
        let mut crossings: Vec<f64> = Vec::new();
        for (i, a) in vertices.iter().enumerate() {
            let b = vertices[(i + 1) % vertices.len()];
            if (a.1 <= yc) != (b.1 <= yc) {
                crossings.push(a.0 + (yc - a.1) / (b.1 - a.1) * (b.0 - a.0));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.chunks_exact(2) {
            // Pixel x is inside when its centre x + 0.5 lies in [start, end).
            let start = (pair[0] - 0.5).ceil().max(0.0) as usize;
            let end = ((pair[1] - 0.5).ceil().max(0.0) as usize).min(w);
            for cell in row.iter_mut().take(end).skip(start) {
                *cell = 1.0;
            }
        }
    });
    out
}

/// Smooth value noise on a unit lattice, in `[-1, 1]`, with an independent
/// lattice per `band`. A pure function of its arguments.
fn value_noise(x: f32, y: f32, band: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (wx, wy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i32, y0 as i32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = lerp(lattice(ix, iy, band), lattice(ix + 1, iy, band), wx);
    let bottom = lerp(lattice(ix, iy + 1, band), lattice(ix + 1, iy + 1, band), wx);
    lerp(top, bottom, wy)
}

/// A hashed lattice value in `[-1, 1]` (the finaliser of MurmurHash3).
fn lattice(x: i32, y: i32, band: u32) -> f32 {
    let mut h = (x as u32 as u64) | ((y as u32 as u64) << 32);
    h ^= u64::from(band).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    ((h >> 40) as f32 / 8_388_608.0) - 1.0
}

#[cfg(test)]
#[path = "v1_tests.rs"]
mod tests;
