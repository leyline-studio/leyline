//! Keystone from lines the photographer drew (ADR 0119).
//!
//! **This module renders nothing that is kept**, like [`crate::auto_tone`]
//! and [`crate::auto_tca`]: its whole output is two numbers a client writes
//! through an ordinary `EditSession`, so a guided correction lands in the
//! history as one revision like any other and `docs/pipeline.md` §5.1 is
//! untouched by construction. No stage calls this file, and none may.
//!
//! Unlike those two it does not even look at the pixels. It is a pure
//! function of four points and an aspect ratio — nothing here detects
//! anything, which is the whole difference between this and the automatic
//! correction ADR 0052 §1 refused and this ADR leaves refused.
//!
//! # Why it enumerates
//!
//! The setting is two integers in `[-100, 100]`: 40 401 possibilities, each
//! costing a 3×3 build and two point transforms per line. So this does not
//! run an optimiser, it walks the whole set — and the answer is then, by
//! construction, the best value the setting can express, with no initial
//! guess, no convergence criterion and no local minimum (ADR 0119 §2).

use leyline_core::{LeylineError, Perspective, Point, Result};

use crate::stages::perspective::v1::{MAX_SHIFT, homography};

/// One line the photographer drew along something that ought to be straight.
///
/// Coordinates are normalized, post-rotation, pre-crop — ADR 0026's frame,
/// the one every placed tool in this program already uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuideLine {
    /// Where the drag started.
    pub a: Point,
    /// Where it ended.
    pub b: Point,
}

impl GuideLine {
    /// Whether this guide says *vertical*. Its own slope decides: steeper
    /// than 45° in the frame means the photographer drew along something
    /// upright (ADR 0119 §3). There is no mode to pick.
    fn is_vertical(&self, aspect: f64) -> bool {
        let dx = (self.b.x - self.a.x).abs() * aspect;
        let dy = (self.b.y - self.a.y).abs();
        dy >= dx
    }

    /// Length in the frame's own units, aspect included.
    fn length(&self, aspect: f64) -> f64 {
        let dx = (self.b.x - self.a.x) * aspect;
        let dy = self.b.y - self.a.y;
        dx.hypot(dy)
    }
}

/// What one guided keystone found (ADR 0119 §3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeystoneSolution {
    /// The two slider values, ready to be written as they are.
    pub perspective: Perspective,
    /// The angle the guides still make with true vertical or horizontal
    /// after that correction, in degrees, root-mean-square over the lines.
    ///
    /// Reported rather than hidden: either the guides disagree about where
    /// the vanishing point is, or what they need is beyond the third of a
    /// frame ADR 0052 caps a corner shift at. The correction is still the
    /// right one; it is just not the whole of what the photograph needs.
    pub residual_degrees: f64,
}

/// The smallest number of guides that pins an answer. One line can be made
/// vertical by a whole family of corrections, and the enumeration would
/// return whichever it met first — an arbitrary answer wearing a
/// measurement's clothes (ADR 0119 §3).
pub const MIN_GUIDES: usize = 2;

/// Solves for the two sliders that best bring `lines` upright, on a frame of
/// the given aspect ratio (width ÷ height of the buffer they were drawn on).
///
/// The homography evaluated is the render's own, `perspective::v1`'s, called
/// rather than restated: a solver that agreed with the renderer only
/// approximately would hand back numbers that do not do what they promised.
pub fn solve(lines: &[GuideLine], aspect: f64) -> Result<KeystoneSolution> {
    if lines.len() < MIN_GUIDES {
        return Err(LeylineError::InvalidSettings(format!(
            "a guided keystone needs at least {MIN_GUIDES} lines, got {}",
            lines.len()
        )));
    }
    if !(aspect.is_finite() && aspect > 0.0) {
        return Err(LeylineError::InvalidSettings(format!(
            "aspect must be a positive ratio, got {aspect}"
        )));
    }
    for (i, line) in lines.iter().enumerate() {
        if line.length(aspect) < 1e-6 {
            return Err(LeylineError::InvalidSettings(format!(
                "guide line {i} has no length: it was a click, not a drag"
            )));
        }
    }

    // The frame in the units the homography works in: a rectangle as wide as
    // the aspect ratio and one unit tall, so nothing here depends on the
    // render size (ADR 0052 §5).
    let (w, h) = (aspect, 1.0);
    let prepared: Vec<Prepared> = lines
        .iter()
        .map(|line| Prepared {
            a: (line.a.x * w, line.a.y * h),
            b: (line.b.x * w, line.b.y * h),
            vertical: line.is_vertical(aspect),
        })
        .collect();

    let mut best = (f64::MAX, 0, Perspective::default());
    for vertical in -100..=100 {
        for horizontal in -100..=100 {
            let candidate = Perspective {
                vertical,
                horizontal,
            };
            let Some(error) = residual(&prepared, w, h, &candidate) else {
                continue;
            };
            // Ties go to the smaller correction. Without that, an axis the
            // guides say nothing about — two verticals say nothing about the
            // horizontal one — would come back at whatever value the scan
            // happened to start at, which is -100 (ADR 0119 §2).
            let magnitude = vertical.abs() + horizontal.abs();
            let better = error < best.0 - TIE || (error <= best.0 + TIE && magnitude < best.1);
            if better {
                best = (error, magnitude, candidate);
            }
        }
    }
    if best.0 == f64::MAX {
        return Err(LeylineError::InvalidSettings(
            "no perspective correction could be evaluated for these guides".to_owned(),
        ));
    }

    Ok(KeystoneSolution {
        perspective: best.2,
        // `best.0` is the mean squared sine of the remaining angle.
        residual_degrees: best.0.sqrt().clamp(0.0, 1.0).asin().to_degrees(),
    })
}

/// Aspect ratio of the buffer the `perspective` stage sees: the sensor's
/// own, swapped when EXIF says the camera was held sideways, then widened by
/// the bounding box `rotate::v1` renders into.
///
/// Computed rather than measured, so a guided keystone costs no decode. The
/// three inputs are all the catalog already holds.
pub(crate) fn frame_aspect(
    width: u32,
    height: u32,
    orientation: Option<u16>,
    rotation: f64,
) -> f64 {
    // EXIF 5-8 are the quarter turns; the decoder applies them, so the
    // buffer is the transpose of what the catalog stored (`leyline_raw`
    // records the sensor's dimensions *before* orientation).
    let sideways = matches!(orientation, Some(5..=8));
    let (w, h) = if sideways {
        (f64::from(height), f64::from(width))
    } else {
        (f64::from(width), f64::from(height))
    };
    let (sin, cos) = rotation.to_radians().sin_cos();
    let rotated_w = w * cos.abs() + h * sin.abs();
    let rotated_h = w * sin.abs() + h * cos.abs();
    if rotated_h > 0.0 {
        rotated_w / rotated_h
    } else {
        1.0
    }
}

/// How close two residuals must be to count as equal, so the tie-break of
/// [`solve`] can prefer the smaller correction. Well below any angle a
/// person could see, and well above the arithmetic's own noise.
const TIE: f64 = 1e-12;

/// A guide with its endpoints already in frame units and its kind decided.
struct Prepared {
    a: (f64, f64),
    b: (f64, f64),
    vertical: bool,
}

/// Mean squared sine of the angle each guide still makes with true vertical
/// or horizontal once `candidate` is applied.
///
/// Dividing by the transformed line's own length is what makes the objective
/// an angle rather than a distance, so a short guide along a window frame
/// weighs exactly as much as a long one down the whole building (ADR 0119
/// §3).
fn residual(lines: &[Prepared], w: f64, h: f64, candidate: &Perspective) -> Option<f64> {
    let forward = homography(w, h, corners(w, h, candidate))?;
    let mut total = 0.0;
    for line in lines {
        let a = project(&forward, line.a)?;
        let b = project(&forward, line.b)?;
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy);
        if length < 1e-9 {
            return None;
        }
        let sine = if line.vertical { dx } else { dy } / length;
        total += sine * sine;
    }
    Some(total / lines.len() as f64)
}

/// Where the four source corners end up under `perspective`.
///
/// Restated from `perspective::v1::correct` rather than shared with it: that
/// module is frozen, and `keystone_agrees_with_the_render` below is what
/// holds the two together — it reads the corner back out of an actual render
/// instead of trusting this arithmetic (ADR 0119 §2).
fn corners(w: f64, h: f64, perspective: &Perspective) -> [(f64, f64); 4] {
    let vertical = f64::from(perspective.vertical) / 100.0 * MAX_SHIFT;
    let horizontal = f64::from(perspective.horizontal) / 100.0 * MAX_SHIFT;
    let (dx, dy) = (w * vertical, h * horizontal);
    [(dx, dy), (w - dx, -dy), (w + dx, h + dy), (-dx, h - dy)]
}

/// One point through a row-major 3×3, `None` on the line at infinity.
fn project(m: &[f64; 9], (x, y): (f64, f64)) -> Option<(f64, f64)> {
    let denominator = m[6] * x + m[7] * y + m[8];
    if denominator.abs() < 1e-12 {
        return None;
    }
    Some((
        (m[0] * x + m[1] * y + m[2]) / denominator,
        (m[3] * x + m[4] * y + m[5]) / denominator,
    ))
}

#[cfg(test)]
mod tests {
    //! ADR 0119's claims, each measured rather than argued.

    use super::*;
    use crate::pixels::Pixels;

    fn line(ax: f64, ay: f64, bx: f64, by: f64) -> GuideLine {
        GuideLine {
            a: Point { x: ax, y: ay },
            b: Point { x: bx, y: by },
        }
    }

    /// The load-bearing test of §2: this module restates `perspective::v1`'s
    /// corner arithmetic instead of sharing it, because that module is
    /// frozen — so the agreement has to be *read out of an actual render*,
    /// never assumed. A single lit pixel goes through `correct`, and the
    /// forward map must say where it came out.
    #[test]
    fn the_solver_agrees_with_the_render() {
        let (w, h) = (64u32, 48u32);
        for perspective in [
            Perspective {
                vertical: 60,
                horizontal: 0,
            },
            Perspective {
                vertical: -35,
                horizontal: 45,
            },
        ] {
            let mut source = Pixels {
                width: w,
                height: h,
                data: vec![0.0; (w * h * 3) as usize],
            };
            let (mx, my) = (20u32, 12u32);
            let marker = ((my * w + mx) * 3) as usize;
            source.data[marker..marker + 3].copy_from_slice(&[1.0, 1.0, 1.0]);

            let rendered = crate::stages::perspective::v1::correct(&source, &perspective);
            let brightest = rendered
                .data
                .chunks_exact(3)
                .enumerate()
                .max_by(|a, b| a.1[0].partial_cmp(&b.1[0]).unwrap())
                .map(|(i, _)| (i as u32 % rendered.width, i as u32 / rendered.width))
                .expect("a rendered pixel");

            // Where this module's own arithmetic says it went, less the
            // bounding-box offset `correct` subtracts.
            let (fw, fh) = (f64::from(w), f64::from(h));
            let quad = corners(fw, fh, &perspective);
            let forward = homography(fw, fh, quad).expect("a homography");
            let placed = project(&forward, (f64::from(mx) + 0.5, f64::from(my) + 0.5)).unwrap();
            let min_x = quad.iter().map(|p| p.0).fold(f64::MAX, f64::min);
            let min_y = quad.iter().map(|p| p.1).fold(f64::MAX, f64::min);
            let predicted = (placed.0 - min_x, placed.1 - min_y);

            let off = (
                (predicted.0 - f64::from(brightest.0)).abs(),
                (predicted.1 - f64::from(brightest.1)).abs(),
            );
            assert!(
                off.0 < 1.5 && off.1 < 1.5,
                "{perspective:?}: the render put it at {brightest:?}, the map says {predicted:?}"
            );
        }
    }

    /// Guides already upright ask for nothing.
    #[test]
    fn upright_guides_return_the_neutral_correction() {
        let solution = solve(
            &[
                line(0.2, 0.1, 0.2, 0.9),
                line(0.8, 0.1, 0.8, 0.9),
                // Off-centre deliberately: the frame's middle row is the
                // horizontal correction's fixed line, so a guide drawn on it
                // would say nothing about that slider.
                line(0.1, 0.3, 0.9, 0.3),
            ],
            1.5,
        )
        .unwrap();
        assert_eq!(solution.perspective, Perspective::default());
        assert!(solution.residual_degrees < 0.01, "{solution:?}");
    }

    /// Two edges converging upward — a building shot from below — ask for a
    /// vertical correction, and it actually brings them together: what is
    /// left is a fraction of the tilt they started with.
    #[test]
    fn converging_verticals_are_brought_upright() {
        let guides = [line(0.30, 0.05, 0.20, 0.95), line(0.70, 0.05, 0.80, 0.95)];
        let solution = solve(&guides, 1.5).unwrap();

        assert!(
            solution.perspective.vertical != 0,
            "a converging pair must move the vertical slider: {solution:?}"
        );
        assert_eq!(
            solution.perspective.horizontal, 0,
            "a symmetric pair says nothing about the horizontal: {solution:?}"
        );

        let before = degrees(&guides, 1.5, &Perspective::default());
        assert!(
            solution.residual_degrees < before / 5.0,
            "{:.3}° left of {before:.3}°",
            solution.residual_degrees
        );
    }

    /// The same pair mirrored asks for the opposite sign — the enumeration
    /// has no preferred direction.
    #[test]
    fn the_sign_follows_the_convergence() {
        let up = solve(
            &[line(0.30, 0.05, 0.20, 0.95), line(0.70, 0.05, 0.80, 0.95)],
            1.5,
        )
        .unwrap();
        let down = solve(
            &[line(0.20, 0.05, 0.30, 0.95), line(0.80, 0.05, 0.70, 0.95)],
            1.5,
        )
        .unwrap();
        assert_eq!(
            up.perspective.vertical, -down.perspective.vertical,
            "{up:?} vs {down:?}"
        );
        assert!(up.perspective.vertical != 0);
    }

    /// A guide's own slope says what it is (§3): the same drag, laid flat,
    /// is read as a horizontal one and moves the other slider.
    #[test]
    fn a_flat_guide_is_read_as_horizontal() {
        let solution = solve(
            &[line(0.05, 0.30, 0.95, 0.20), line(0.05, 0.70, 0.95, 0.80)],
            1.5,
        )
        .unwrap();
        assert_eq!(solution.perspective.vertical, 0, "{solution:?}");
        assert!(solution.perspective.horizontal != 0, "{solution:?}");
    }

    /// §3's angle normalisation: drawing a quarter of the same edge must not
    /// move the answer.
    #[test]
    fn a_short_guide_weighs_as_much_as_a_long_one() {
        let long = solve(
            &[line(0.30, 0.05, 0.20, 0.95), line(0.70, 0.05, 0.80, 0.95)],
            1.5,
        )
        .unwrap();
        let short = solve(
            &[line(0.30, 0.05, 0.275, 0.275), line(0.70, 0.05, 0.80, 0.95)],
            1.5,
        )
        .unwrap();
        assert!(
            (long.perspective.vertical - short.perspective.vertical).abs() <= 2,
            "{long:?} vs {short:?}"
        );
    }

    /// The two refusals of §3, by name, and the guard on the aspect ratio.
    #[test]
    fn one_line_and_a_click_are_refused() {
        let single = solve(&[line(0.3, 0.05, 0.2, 0.95)], 1.5)
            .unwrap_err()
            .to_string();
        assert!(single.contains("at least 2 lines"), "{single}");

        let click = solve(&[line(0.3, 0.4, 0.3, 0.4), line(0.7, 0.05, 0.8, 0.95)], 1.5)
            .unwrap_err()
            .to_string();
        assert!(click.contains("no length"), "{click}");

        let aspect = solve(
            &[line(0.3, 0.05, 0.2, 0.95), line(0.7, 0.05, 0.8, 0.95)],
            0.0,
        )
        .unwrap_err()
        .to_string();
        assert!(aspect.contains("aspect"), "{aspect}");
    }

    /// `frame_aspect` follows the decoder, not the file: a portrait frame is
    /// stored landscape with an orientation tag, and the buffer this solver
    /// works in is the one the decoder produced.
    #[test]
    fn the_frame_aspect_follows_the_decoder() {
        assert!((frame_aspect(6000, 4000, Some(1), 0.0) - 1.5).abs() < 1e-12);
        assert!((frame_aspect(6000, 4000, Some(6), 0.0) - 2.0 / 3.0).abs() < 1e-12);
        assert!((frame_aspect(6000, 4000, None, 0.0) - 1.5).abs() < 1e-12);
        // A rotation widens the frame toward square, both ways alike.
        let square = frame_aspect(6000, 4000, Some(1), 45.0);
        assert!((square - 1.0).abs() < 1e-9, "{square}");
        assert!(
            (frame_aspect(6000, 4000, Some(1), 5.0) - frame_aspect(6000, 4000, Some(1), -5.0))
                .abs()
                < 1e-12
        );
    }

    /// The residual of a given correction, in degrees — what the tests above
    /// compare a solution against.
    fn degrees(lines: &[GuideLine], aspect: f64, perspective: &Perspective) -> f64 {
        let prepared: Vec<Prepared> = lines
            .iter()
            .map(|line| Prepared {
                a: (line.a.x * aspect, line.a.y),
                b: (line.b.x * aspect, line.b.y),
                vertical: line.is_vertical(aspect),
            })
            .collect();
        residual(&prepared, aspect, 1.0, perspective)
            .unwrap()
            .sqrt()
            .asin()
            .to_degrees()
    }
}
