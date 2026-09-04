//! Maps the local-adjustment panel's gestures and sliders to engine parameter
//! updates (ADR 0029, ADR 0048, ADR 0049).
//!
//! Kept apart from [`crate::develop`], and free of any Slint type for the same
//! reason: a masked adjustment is a *list entry* rather than a slider, so its
//! rules — which gesture creates an entry, which value means "not set" — are
//! worth unit-testing on their own.

use leyline_sdk::{
    BrushStroke, ColorRange, LocalAdjustment, LocalAdjustmentValues, LuminanceRange, Mask, Param,
    RangeMask, Settings, Value,
};

/// Decodes a drag over the develop preview into one mask geometry (ADR 0049
/// §1): a radial is the ellipse inscribed in the dragged rectangle, a gradient
/// is the dragged axis itself — full coverage where the press landed, none
/// where it was released.
///
/// `view` and `image` are the viewport and preview sizes in their own pixels,
/// mapped through the same letterbox as [`crate::develop::drag_crop`].
/// Degenerate drags (a rectangle thinner than 1 % of the frame, a gradient
/// whose two ends coincide) yield `None` rather than a geometry
/// `Settings::validate` would refuse.
pub fn drag_geometry(
    kind: &str,
    press: (f64, f64),
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
) -> Option<Mask> {
    let a = crate::develop::letterbox_unit(press, view, image)?;
    let b = crate::develop::letterbox_unit(release, view, image)?;
    match kind {
        "radial" => {
            let (rx, ry) = ((a.0 - b.0).abs() / 2.0, (a.1 - b.1).abs() / 2.0);
            if rx < MIN_EXTENT || ry < MIN_EXTENT {
                return None;
            }
            Some(Mask::Radial {
                cx: (a.0 + b.0) / 2.0,
                cy: (a.1 + b.1) / 2.0,
                rx,
                ry,
                // The drag has no rotation to report: an axis-aligned
                // ellipse is what a two-corner gesture can express
                // (ADR 0049 §1).
                angle: 0.0,
                feather: DEFAULT_FEATHER,
                inverted: false,
            })
        }
        "gradient" => {
            if (a.0 - b.0).abs() < MIN_EXTENT && (a.1 - b.1).abs() < MIN_EXTENT {
                return None;
            }
            Some(Mask::Gradient {
                x0: a.0,
                y0: a.1,
                x1: b.0,
                y1: b.1,
            })
        }
        _ => None,
    }
}

/// Moves one handle of an already-drawn geometry (ADR 0097 §1).
///
/// `handle` names which one was grabbed — `center`, `rx`, `ry` for a
/// radial, `from`, `to` for a gradient — and `release` is where it was let
/// go, in view pixels. Returns the new geometry, or `None` when the entry
/// is not of the handle's kind or the result would be degenerate
/// (ADR 0097 §3): the gesture is then ignored and the geometry stands.
///
/// Only the geometry moves: the feather, the inversion, the range band and
/// the values of the entry are the caller's to preserve, and it does so by
/// rewriting nothing but `mask`.
pub fn drag_handle(
    handle: &str,
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    current: &Mask,
) -> Option<Mask> {
    let (x, y) = crate::develop::letterbox_unit(release, view, image)?;
    match (current, handle) {
        (
            Mask::Radial {
                cx,
                cy,
                rx,
                ry,
                angle,
                feather,
                inverted,
            },
            _,
        ) => {
            let (cx, cy, rx, ry) = match handle {
                "center" => (x, y, *rx, *ry),
                // Resized from the centre outward, so the opposite rim
                // moves with it — the ellipse stays centred where it is.
                "rx" => (*cx, *cy, (x - cx).abs(), *ry),
                "ry" => (*cx, *cy, *rx, (y - cy).abs()),
                _ => return None,
            };
            if rx < MIN_EXTENT || ry < MIN_EXTENT {
                return None;
            }
            Some(Mask::Radial {
                cx,
                cy,
                rx,
                ry,
                angle: *angle,
                feather: *feather,
                inverted: *inverted,
            })
        }
        (Mask::Gradient { x0, y0, x1, y1 }, _) => {
            let (x0, y0, x1, y1) = match handle {
                "from" => (x, y, *x1, *y1),
                "to" => (*x0, *y0, x, y),
                _ => return None,
            };
            if (x0 - x1).abs() < MIN_EXTENT && (y0 - y1).abs() < MIN_EXTENT {
                return None;
            }
            Some(Mask::Gradient { x0, y0, x1, y1 })
        }
        _ => None,
    }
}

/// The smallest extent a gesture may leave behind: below it the geometry
/// covers nothing, which `Settings::validate` refuses anyway. Shared by
/// [`drag_geometry`] and [`drag_handle`] so the two cannot disagree about
/// what "degenerate" means.
const MIN_EXTENT: f64 = 0.005;

/// The feather a freshly traced radial starts with: half its radius, the
/// midpoint of the parameter's range — a hard-edged local adjustment is
/// almost never what is wanted, and a fully feathered one barely shows.
const DEFAULT_FEATHER: f64 = 0.5;

/// Decodes one brush click into a dab (ADR 0049 §1). `radius_flow_hardness`
/// comes from the panel's own fields, already in the `[0, 1]` units
/// [`BrushStroke`] stores; a non-positive radius yields `None`, matching
/// `Settings::validate`.
pub fn dab(
    click: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    radius_flow_hardness: (f64, f64, f64),
) -> Option<BrushStroke> {
    let (radius, flow, hardness) = radius_flow_hardness;
    let (x, y) = crate::develop::letterbox_unit(click, view, image)?;
    if radius <= 0.0 {
        return None;
    }
    Some(BrushStroke {
        x,
        y,
        radius,
        flow: flow.clamp(0.0, 1.0),
        hardness: hardness.clamp(0.0, 1.0),
    })
}

/// Where a traced geometry goes: onto the selected entry if it is of the same
/// kind, otherwise appended as a new one (ADR 0049 §2).
///
/// `selected` is the panel's selected row, `-1` for none. Retracing keeps the
/// entry's values and opacity — only its geometry moves — which is what makes
/// a badly placed radial fixable without deleting it first.
///
/// The returned [`Param`] carries the row that was written, which is the row
/// the panel then selects.
pub fn place_geometry(selected: i32, mask: Mask, current: &[LocalAdjustment]) -> (Param, Value) {
    let index = usize::try_from(selected).ok().filter(|&i| {
        current
            .get(i)
            .is_some_and(|entry| same_kind(&entry.mask, &mask))
    });
    match index {
        Some(index) => {
            let mut entry = current[index].clone();
            entry.mask = mask;
            (
                Param::LocalAdjustment(index),
                Value::LocalAdjustment(Some(entry)),
            )
        }
        None => (
            Param::LocalAdjustment(current.len()),
            Value::LocalAdjustment(Some(fresh(mask))),
        ),
    }
}

/// A newly traced adjustment: the geometry, fully applied, changing nothing
/// yet — the values are what the panel's sliders then fill in.
///
/// Public under [`fresh_entry`] for the one mask that is not traced but
/// imported (ADR 0070 §7): it starts life exactly like the others.
pub fn fresh_entry(mask: Mask) -> LocalAdjustment {
    fresh(mask)
}

fn fresh(mask: Mask) -> LocalAdjustment {
    LocalAdjustment {
        mask,
        range: None,
        opacity: 1.0,
        adjustments: LocalAdjustmentValues::default(),
    }
}

/// Appends one dab to the selected brush, or starts a new brush with it
/// (ADR 0049 §2) — a `Mask::Brush` cannot exist without a dab, so the first
/// dab is what creates the entry.
pub fn paint_dab(
    selected: i32,
    stroke: BrushStroke,
    current: &[LocalAdjustment],
) -> (Param, Value) {
    paint_stroke(selected, &[stroke], current)
}

/// The same, for a whole traced stroke: every dab of one drag lands in one
/// entry, and the drag is **one** revision rather than one per dab.
///
/// `Mask::Brush` was always a *path* — `BrushStroke` is documented as "a
/// single dab in the brush's path" — so tracing needs no engine change, no
/// stage version and no migration. What was missing was a client that drew
/// more than one dab at a time.
///
/// An empty slice returns the entry unchanged rather than an empty brush,
/// which `Settings::validate()` refuses (ADR 0049 §2).
pub fn paint_stroke(
    selected: i32,
    strokes: &[BrushStroke],
    current: &[LocalAdjustment],
) -> (Param, Value) {
    let index = usize::try_from(selected)
        .ok()
        .filter(|&i| matches!(current.get(i).map(|e| &e.mask), Some(Mask::Brush { .. })));
    match index {
        Some(index) => {
            let mut entry = current[index].clone();
            if let Mask::Brush { strokes: existing } = &mut entry.mask {
                existing.extend_from_slice(strokes);
            }
            (
                Param::LocalAdjustment(index),
                Value::LocalAdjustment(Some(entry)),
            )
        }
        None => (
            Param::LocalAdjustment(current.len()),
            Value::LocalAdjustment(Some(fresh(Mask::Brush {
                strokes: strokes.to_vec(),
            }))),
        ),
    }
}

/// Whether a dab belongs in a stroke being traced, given the last one kept.
///
/// Spacing is **half the radius** — a quarter of the diameter, the figure
/// every painting program uses: the disks overlap heavily, so the stroke reads
/// as continuous, and a drag across the whole frame costs tens of dabs rather
/// than one per mouse event. Without a rule the pointer's own sampling rate
/// would decide how many dabs a revision holds.
#[must_use]
pub fn dab_is_far_enough(last: Option<&BrushStroke>, next: &BrushStroke) -> bool {
    match last {
        None => true,
        Some(last) => (next.x - last.x).hypot(next.y - last.y) >= next.radius / 2.0,
    }
}

/// Adds an entry whose geometry needs no tracing — today only
/// [`Mask::Everything`], the geometry of "no geometry" a range mask stands on
/// (ADR 0048 §1).
pub fn add_mask(kind: &str, current: &[LocalAdjustment]) -> Option<(Param, Value)> {
    let mask = match kind {
        "everything" => Mask::Everything,
        _ => return None,
    };
    Some((
        Param::LocalAdjustment(current.len()),
        Value::LocalAdjustment(Some(fresh(mask))),
    ))
}

/// Paints the mask overlay over a preview, in place (ADR 0071 §5).
///
/// `coverage` is the engine's grey coverage at the same size as `base`: red at
/// half strength where the mask applies, untouched where it does not. Half is
/// the convention everywhere in the trade, and it is the point — one still has
/// to judge the photo underneath.
///
/// A size mismatch paints nothing rather than smearing a stale overlay across
/// the frame: the two images come from separate renders, and one can arrive
/// before the other has caught up.
pub fn paint_overlay(
    base: &mut [u8],
    base_size: (u32, u32),
    coverage: &[u8],
    coverage_size: (u32, u32),
) -> bool {
    if base_size != coverage_size || base.len() != coverage.len() {
        return false;
    }
    for (pixel, mask) in base.chunks_exact_mut(3).zip(coverage.chunks_exact(3)) {
        let alpha = f32::from(mask[0]) / 255.0 * 0.5;
        pixel[0] = (f32::from(pixel[0]) * (1.0 - alpha) + 255.0 * alpha).round() as u8;
        pixel[1] = (f32::from(pixel[1]) * (1.0 - alpha)).round() as u8;
        pixel[2] = (f32::from(pixel[2]) * (1.0 - alpha)).round() as u8;
    }
    true
}

/// The row a local-adjustment update wrote to — the row the panel selects
/// after a gesture, since a gesture can create as well as modify.
pub fn written_row(param: &Param) -> Option<usize> {
    match param {
        Param::LocalAdjustment(index) => Some(*index),
        _ => None,
    }
}

/// Removes the entry at `index`, if there is one.
pub fn remove_mask(index: i32, current: &[LocalAdjustment]) -> Option<(Param, Value)> {
    let index = usize::try_from(index).ok().filter(|&i| i < current.len())?;
    Some((Param::LocalAdjustment(index), Value::LocalAdjustment(None)))
}

/// Decodes one slider or toggle of the local-adjustment editor.
///
/// `field` names what moved, `value` is the released value in the panel's own
/// unit (percent for the `[0, 1]` fields, degrees for hue, Kelvin for
/// temperature, 0/1 for a toggle). `settings` supplies both the entry being
/// edited and the photo's global white balance, which is what an enabled local
/// white balance starts from.
///
/// Neutral means *absent* (ADR 0049 §3): a slider returned to zero clears its
/// `Option` instead of storing a zero, so a stored adjustment lists only what
/// it changes. Temperature/tint and the two range terms have no such neutral
/// and are driven by their own toggles.
pub fn edit_field(
    index: i32,
    field: &str,
    value: f64,
    settings: &Settings,
) -> Option<(Param, Value)> {
    let index = usize::try_from(index).ok()?;
    let mut entry = settings.local_adjustments.get(index)?.clone();
    let on = value != 0.0;
    let unit = || (value / 100.0).clamp(0.0, 1.0);
    let level = || {
        let rounded = value.round() as i32;
        (rounded != 0).then_some(rounded)
    };
    let positive_level = || {
        let rounded = (value.round() as i32).clamp(0, 100);
        (rounded != 0).then_some(rounded)
    };
    match field {
        "opacity" => entry.opacity = unit(),
        "feather" => match &mut entry.mask {
            Mask::Radial { feather, .. } => *feather = unit(),
            _ => return None,
        },
        "invert" => match &mut entry.mask {
            Mask::Radial { inverted, .. } => *inverted = on,
            _ => return None,
        },
        // The local white balance is seeded from the photo's own, so
        // enabling it is visually a no-op the user then moves away from —
        // starting at some fixed daylight value would recolor the mask the
        // moment the toggle is flipped.
        "wb" => {
            let (temperature, tint) = if on {
                let global = settings.white_balance.clone().unwrap_or_default();
                (Some(global.temperature), Some(global.tint))
            } else {
                (None, None)
            };
            entry.adjustments.temperature = temperature;
            entry.adjustments.tint = tint;
        }
        "temperature" => {
            let kelvin = value.round().max(1.0) as u32;
            entry.adjustments.temperature = Some(kelvin);
        }
        "tint" => entry.adjustments.tint = Some(value.round() as i32),
        "exposure" => entry.adjustments.exposure = (value != 0.0).then_some(value),
        "contrast" => entry.adjustments.contrast = level(),
        "highlights" => entry.adjustments.highlights = level(),
        "shadows" => entry.adjustments.shadows = level(),
        "whites" => entry.adjustments.whites = level(),
        "blacks" => entry.adjustments.blacks = level(),
        "vibrance" => entry.adjustments.vibrance = level(),
        "saturation" => entry.adjustments.saturation = level(),
        // The five of ADR 0108, neutral-is-absent like every level above.
        "clarity" => entry.adjustments.clarity = level(),
        "texture" => entry.adjustments.texture = level(),
        "sharpness" => entry.adjustments.sharpness = level(),
        // Noise reduction has no meaningful negative, and `Settings::validate`
        // refuses one: clamp at the panel rather than let a drag build a
        // document the engine will reject.
        "noise-luminance" => entry.adjustments.noise_luminance = positive_level(),
        "noise-color" => entry.adjustments.noise_color = positive_level(),
        // Same clamp for the defringe pair (ADR 0116): two doses, no
        // meaningful negative.
        "defringe-purple" => entry.adjustments.defringe_purple = positive_level(),
        "defringe-green" => entry.adjustments.defringe_green = positive_level(),
        "range-luminance" => {
            let mut range = entry.range.clone().unwrap_or_default();
            range.luminance = on.then(LuminanceRange::default);
            entry.range = keep_range(range);
        }
        "range-color" => {
            let mut range = entry.range.clone().unwrap_or_default();
            range.color = on.then(ColorRange::default);
            entry.range = keep_range(range);
        }
        "lum-min" | "lum-max" | "lum-softness" => {
            let mut range = entry.range.clone().unwrap_or_default();
            let mut luminance = range.luminance.unwrap_or_default();
            match field {
                "lum-min" => luminance.min = unit().min(luminance.max),
                "lum-max" => luminance.max = unit().max(luminance.min),
                _ => luminance.softness = unit(),
            }
            range.luminance = Some(luminance);
            entry.range = keep_range(range);
        }
        "color-center" | "color-width" | "color-softness" => {
            let mut range = entry.range.clone().unwrap_or_default();
            let mut color = range.color.unwrap_or_default();
            match field {
                "color-center" => color.center = value.rem_euclid(360.0),
                "color-width" => color.width = value.clamp(1.0, 180.0),
                _ => color.softness = value.clamp(0.0, 180.0),
            }
            range.color = Some(color);
            entry.range = keep_range(range);
        }
        _ => return None,
    }
    Some((
        Param::LocalAdjustment(index),
        Value::LocalAdjustment(Some(entry)),
    ))
}

/// Decodes one eyedropper click into the band it proposes (ADR 0093 §2).
///
/// `kind` is `"range-lum"` or `"range-color"`. A luminance click writes
/// full coverage at the sample ±0.1, clamped; a hue click moves the band's
/// center only. Width and softness are kept when present, defaults when
/// the term was off — turning it on is implicit, pointing at a luminance
/// *is* asking for a luminance band.
pub fn sample_field(
    index: i32,
    kind: &str,
    sample: (f64, f64),
    entries: &[LocalAdjustment],
) -> Option<(Param, Value)> {
    let (luminance, hue) = sample;
    let index = usize::try_from(index).ok()?;
    let mut entry = entries.get(index)?.clone();
    let mut range = entry.range.clone().unwrap_or_default();
    match kind {
        "range-lum" => {
            let softness = range.luminance.map_or(0.1, |l| l.softness);
            range.luminance = Some(LuminanceRange {
                min: (luminance - 0.1).clamp(0.0, 1.0),
                max: (luminance + 0.1).clamp(0.0, 1.0),
                softness,
            });
        }
        "range-color" => {
            let kept = range.color.unwrap_or_default();
            range.color = Some(ColorRange {
                center: hue.rem_euclid(360.0),
                ..kept
            });
        }
        _ => return None,
    }
    entry.range = keep_range(range);
    Some((
        Param::LocalAdjustment(index),
        Value::LocalAdjustment(Some(entry)),
    ))
}

/// A range with neither term is the absence of a range, not a stored struct
/// full of `None` — the shape ADR 0048 §2 chose for the field.
fn keep_range(range: RangeMask) -> Option<RangeMask> {
    (range != RangeMask::default()).then_some(range)
}

/// Whether two geometries are of the same kind, i.e. whether retracing one
/// should replace the other (ADR 0049 §2).
fn same_kind(a: &Mask, b: &Mask) -> bool {
    matches!(
        (a, b),
        (Mask::Radial { .. }, Mask::Radial { .. })
            | (Mask::Gradient { .. }, Mask::Gradient { .. })
            | (Mask::Brush { .. }, Mask::Brush { .. })
            | (Mask::Everything, Mask::Everything)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100×100 preview filling a 100×100 viewport: view pixels are unit
    /// hundredths, which keeps the expectations below readable.
    const VIEW: (f64, f64) = (100.0, 100.0);

    fn radial() -> LocalAdjustment {
        LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.2,
                ry: 0.2,
                angle: 0.0,
                feather: 0.5,
                inverted: false,
            },
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues::default(),
        }
    }

    fn settings_with(adjustments: Vec<LocalAdjustment>) -> Settings {
        Settings {
            local_adjustments: adjustments,
            ..Settings::default()
        }
    }

    /// Unwraps the row and adjustment an update carries.
    fn written(update: Option<(Param, Value)>) -> (usize, LocalAdjustment) {
        match update {
            Some((Param::LocalAdjustment(i), Value::LocalAdjustment(Some(entry)))) => (i, entry),
            other => panic!("expected a local adjustment update, got {other:?}"),
        }
    }

    #[test]
    fn a_radial_drag_inscribes_the_ellipse_in_the_dragged_rectangle() {
        let mask = drag_geometry("radial", (20.0, 30.0), (60.0, 70.0), VIEW, VIEW).unwrap();
        let Mask::Radial {
            cx,
            cy,
            rx,
            ry,
            angle,
            feather,
            inverted,
        } = mask
        else {
            panic!("expected a radial, got {mask:?}");
        };
        assert_eq!((cx, cy), (0.4, 0.5));
        // Halving a difference of view pixels lands a hair off 0.2.
        assert!(
            (rx - 0.2).abs() < 1e-9 && (ry - 0.2).abs() < 1e-9,
            "{rx}/{ry}"
        );
        assert_eq!((angle, feather, inverted), (0.0, DEFAULT_FEATHER, false));
    }

    #[test]
    fn a_gradient_drag_runs_from_full_coverage_to_none() {
        let mask = drag_geometry("gradient", (10.0, 80.0), (10.0, 20.0), VIEW, VIEW).unwrap();
        assert_eq!(
            mask,
            Mask::Gradient {
                x0: 0.1,
                y0: 0.8,
                x1: 0.1,
                y1: 0.2,
            }
        );
    }

    #[test]
    fn degenerate_drags_yield_no_geometry() {
        // A radial thinner than 1 % in one direction, and a gradient whose
        // ends coincide: both would be refused by `Settings::validate`.
        assert!(drag_geometry("radial", (20.0, 20.0), (60.0, 20.2), VIEW, VIEW).is_none());
        assert!(drag_geometry("gradient", (20.0, 20.0), (20.2, 20.2), VIEW, VIEW).is_none());
        assert!(drag_geometry("radial", (0.0, 0.0), (50.0, 50.0), (0.0, 0.0), VIEW).is_none());
        assert!(drag_geometry("brush", (0.0, 0.0), (50.0, 50.0), VIEW, VIEW).is_none());
    }

    #[test]
    fn a_drag_appends_when_nothing_of_that_kind_is_selected() {
        let update = place_geometry(
            -1,
            drag_geometry("radial", (20.0, 20.0), (60.0, 60.0), VIEW, VIEW).unwrap(),
            &[],
        );
        let (row, entry) = written(Some(update));
        assert_eq!(row, 0);
        assert_eq!(entry.opacity, 1.0);
        assert_eq!(entry.adjustments, LocalAdjustmentValues::default());
    }

    #[test]
    fn retracing_the_selected_entry_keeps_its_values() {
        let mut existing = radial();
        existing.opacity = 0.4;
        existing.adjustments.exposure = Some(-1.0);
        let traced = drag_geometry("radial", (0.0, 0.0), (40.0, 40.0), VIEW, VIEW).unwrap();
        let (row, entry) = written(Some(place_geometry(0, traced.clone(), &[existing])));
        assert_eq!(row, 0);
        assert_eq!(entry.mask, traced);
        assert_eq!(entry.opacity, 0.4);
        assert_eq!(entry.adjustments.exposure, Some(-1.0));
    }

    #[test]
    fn a_drag_of_another_kind_never_overwrites_the_selection() {
        let traced = drag_geometry("gradient", (0.0, 0.0), (0.0, 40.0), VIEW, VIEW).unwrap();
        let (row, entry) = written(Some(place_geometry(0, traced, &[radial()])));
        assert_eq!(row, 1);
        assert!(matches!(entry.mask, Mask::Gradient { .. }));
    }

    #[test]
    fn the_first_dab_creates_the_brush_and_the_next_ones_extend_it() {
        let stroke = dab((25.0, 75.0), VIEW, VIEW, (0.05, 0.5, 0.5)).unwrap();
        assert_eq!((stroke.x, stroke.y, stroke.radius), (0.25, 0.75, 0.05));

        let (row, created) = written(Some(paint_dab(-1, stroke, &[])));
        assert_eq!(row, 0);
        let Mask::Brush { strokes } = &created.mask else {
            panic!("expected a brush, got {:?}", created.mask);
        };
        assert_eq!(strokes.len(), 1);

        let (row, extended) = written(Some(paint_dab(0, stroke, &[created])));
        assert_eq!(row, 0);
        let Mask::Brush { strokes } = &extended.mask else {
            panic!("expected a brush");
        };
        assert_eq!(strokes.len(), 2);
    }

    #[test]
    fn a_dab_on_a_selected_non_brush_starts_its_own_brush() {
        let stroke = dab((25.0, 75.0), VIEW, VIEW, (0.05, 0.5, 0.5)).unwrap();
        let (row, _) = written(Some(paint_dab(0, stroke, &[radial()])));
        assert_eq!(row, 1);
    }

    #[test]
    fn a_dab_needs_a_positive_radius() {
        assert!(dab((25.0, 75.0), VIEW, VIEW, (0.0, 0.5, 0.5)).is_none());
    }

    #[test]
    fn everything_is_the_only_mask_addable_without_a_gesture() {
        let (row, entry) = written(add_mask("everything", &[radial()]));
        assert_eq!(row, 1);
        assert_eq!(entry.mask, Mask::Everything);
        assert!(add_mask("radial", &[]).is_none());
    }

    #[test]
    fn the_written_row_is_the_one_the_param_addresses() {
        assert_eq!(written_row(&Param::LocalAdjustment(3)), Some(3));
        assert_eq!(written_row(&Param::Exposure), None);
    }

    #[test]
    fn remove_addresses_an_existing_row_only() {
        assert_eq!(
            remove_mask(0, &[radial()]),
            Some((Param::LocalAdjustment(0), Value::LocalAdjustment(None)))
        );
        assert_eq!(remove_mask(1, &[radial()]), None);
        assert_eq!(remove_mask(-1, &[radial()]), None);
    }

    #[test]
    fn a_slider_back_to_zero_clears_its_option_rather_than_storing_zero() {
        let mut existing = radial();
        existing.adjustments.contrast = Some(30);
        existing.adjustments.exposure = Some(1.5);
        let settings = settings_with(vec![existing]);
        let (_, entry) = written(edit_field(0, "contrast", 0.0, &settings));
        assert_eq!(entry.adjustments.contrast, None);
        let (_, entry) = written(edit_field(0, "exposure", 0.0, &settings));
        assert_eq!(entry.adjustments.exposure, None);
        let (_, entry) = written(edit_field(0, "shadows", 42.4, &settings));
        assert_eq!(entry.adjustments.shadows, Some(42));
    }

    #[test]
    fn the_white_balance_toggle_seeds_from_the_photos_own() {
        let mut settings = settings_with(vec![radial()]);
        settings.white_balance = Some(leyline_sdk::WhiteBalance {
            temperature: 4800,
            tint: -12,
        });
        let (_, entry) = written(edit_field(0, "wb", 1.0, &settings));
        assert_eq!(entry.adjustments.temperature, Some(4800));
        assert_eq!(entry.adjustments.tint, Some(-12));

        let settings = settings_with(vec![entry]);
        let (_, entry) = written(edit_field(0, "wb", 0.0, &settings));
        assert_eq!(
            (entry.adjustments.temperature, entry.adjustments.tint),
            (None, None)
        );
    }

    #[test]
    fn as_shot_seeds_the_local_white_balance_from_the_engine_default() {
        let settings = settings_with(vec![radial()]);
        let (_, entry) = written(edit_field(0, "wb", 1.0, &settings));
        let default = leyline_sdk::WhiteBalance::default();
        assert_eq!(entry.adjustments.temperature, Some(default.temperature));
    }

    #[test]
    fn opacity_and_feather_come_in_as_percent() {
        let settings = settings_with(vec![radial()]);
        let (_, entry) = written(edit_field(0, "opacity", 40.0, &settings));
        assert_eq!(entry.opacity, 0.4);
        let (_, entry) = written(edit_field(0, "feather", 25.0, &settings));
        assert!(matches!(entry.mask, Mask::Radial { feather, .. } if feather == 0.25));
    }

    #[test]
    fn feather_and_invert_only_mean_something_on_a_radial() {
        let settings = settings_with(vec![LocalAdjustment {
            mask: Mask::Everything,
            ..radial()
        }]);
        assert_eq!(edit_field(0, "feather", 50.0, &settings), None);
        assert_eq!(edit_field(0, "invert", 1.0, &settings), None);
    }

    #[test]
    fn a_range_term_toggles_on_with_its_default_band_and_off_to_nothing() {
        let settings = settings_with(vec![radial()]);
        let (_, entry) = written(edit_field(0, "range-luminance", 1.0, &settings));
        assert_eq!(
            entry.range,
            Some(RangeMask {
                luminance: Some(LuminanceRange::default()),
                color: None,
            })
        );

        let settings = settings_with(vec![entry]);
        let (_, entry) = written(edit_field(0, "range-color", 1.0, &settings));
        assert!(entry.range.as_ref().unwrap().color.is_some());

        // Both terms off again is the absence of a range, not an empty one.
        let settings = settings_with(vec![entry]);
        let (_, entry) = written(edit_field(0, "range-luminance", 0.0, &settings));
        let settings = settings_with(vec![entry]);
        let (_, entry) = written(edit_field(0, "range-color", 0.0, &settings));
        assert_eq!(entry.range, None);
    }

    #[test]
    fn the_luminance_band_edges_never_cross() {
        let settings = settings_with(vec![radial()]);
        let (_, entry) = written(edit_field(0, "lum-max", 40.0, &settings));
        let settings = settings_with(vec![entry]);
        // A minimum dragged past the maximum stops at it rather than
        // producing the `min > max` `validate` refuses.
        let (_, entry) = written(edit_field(0, "lum-min", 80.0, &settings));
        let band = entry.range.unwrap().luminance.unwrap();
        assert_eq!((band.min, band.max), (0.4, 0.4));
    }

    #[test]
    fn the_color_band_wraps_its_center_and_keeps_its_width_valid() {
        let settings = settings_with(vec![radial()]);
        let (_, entry) = written(edit_field(0, "color-center", 375.0, &settings));
        assert_eq!(entry.range.unwrap().color.unwrap().center, 15.0);
        let (_, entry) = written(edit_field(0, "color-width", 0.0, &settings));
        assert_eq!(entry.range.unwrap().color.unwrap().width, 1.0);
    }

    /// ADR 0108's five arrive through the same neutral-is-absent path as
    /// every level before them, and the two noise sliders clamp instead of
    /// composing an entry `Settings::validate` would refuse.
    #[test]
    fn the_five_neighbourhood_fields_are_written_and_cleared() {
        let settings = settings_with(vec![radial()]);
        for (field, value, expected) in [
            ("clarity", 40.0, Some(40)),
            ("texture", -60.0, Some(-60)),
            ("sharpness", 25.0, Some(25)),
            ("noise-luminance", 30.0, Some(30)),
            ("noise-color", 20.0, Some(20)),
            ("defringe-purple", 70.0, Some(70)),
            ("defringe-green", 30.0, Some(30)),
        ] {
            let (_, entry) = written(edit_field(0, field, value, &settings));
            let values = &entry.adjustments;
            let got = match field {
                "clarity" => values.clarity,
                "texture" => values.texture,
                "sharpness" => values.sharpness,
                "defringe-purple" => values.defringe_purple,
                "defringe-green" => values.defringe_green,
                "noise-luminance" => values.noise_luminance,
                _ => values.noise_color,
            };
            assert_eq!(got, expected, "{field}");

            // Back to neutral clears the value rather than storing a zero:
            // the entry must not carry a key that means "do nothing".
            let (_, entry) = written(edit_field(0, field, 0.0, &settings));
            let values = &entry.adjustments;
            assert!(
                !values.uses_neighbourhood_operators(),
                "{field} kept a zero"
            );
        }
    }

    /// A negative on a slider that has no negative is clamped at the panel,
    /// not passed on: the engine refuses it, and a drag must not be able to
    /// compose a document it will reject.
    #[test]
    fn a_negative_noise_value_is_clamped_to_neutral() {
        let settings = settings_with(vec![radial()]);
        for field in ["noise-luminance", "noise-color"] {
            let (_, entry) = written(edit_field(0, field, -50.0, &settings));
            assert!(
                !entry.adjustments.uses_neighbourhood_operators(),
                "{field} stored a negative"
            );
        }
    }

    #[test]
    fn unknown_fields_and_rows_do_nothing() {
        let settings = settings_with(vec![radial()]);
        assert_eq!(edit_field(0, "gamma", 1.0, &settings), None);
        assert_eq!(edit_field(1, "opacity", 50.0, &settings), None);
        assert_eq!(edit_field(-1, "opacity", 50.0, &settings), None);
    }

    /// Every value this module writes has to survive the engine's own
    /// validation — the panel must not be able to compose a refused entry.
    #[test]
    fn every_edited_field_stays_valid_for_the_engine() {
        let mut settings = settings_with(vec![radial()]);
        for (field, value) in [
            ("opacity", 0.0),
            ("opacity", 100.0),
            ("feather", 0.0),
            ("invert", 1.0),
            ("wb", 1.0),
            ("temperature", 0.0),
            ("tint", -100.0),
            ("exposure", 5.0),
            ("contrast", -100.0),
            ("highlights", 100.0),
            ("shadows", -100.0),
            ("whites", 100.0),
            ("blacks", -100.0),
            ("vibrance", 100.0),
            ("saturation", -100.0),
            ("clarity", 100.0),
            ("texture", -100.0),
            ("sharpness", -100.0),
            // Past both ends of the two sliders that have no negative: the
            // panel clamps, so the engine never sees the refusal.
            ("noise-luminance", -100.0),
            ("noise-luminance", 100.0),
            ("noise-color", -100.0),
            ("noise-color", 100.0),
            ("range-luminance", 1.0),
            ("lum-min", 0.0),
            ("lum-max", 100.0),
            ("lum-softness", 100.0),
            ("range-color", 1.0),
            ("color-center", 720.0),
            ("color-width", 200.0),
            ("color-softness", 300.0),
        ] {
            let (_, entry) = written(edit_field(0, field, value, &settings));
            settings.local_adjustments = vec![entry];
            settings
                .validate()
                .unwrap_or_else(|e| panic!("{field} at {value} produced invalid settings: {e}"));
        }
    }

    /// ADR 0071 §5: red at half strength where covered, untouched where not,
    /// and nothing at all when the two images disagree on size.
    #[test]
    fn the_overlay_paints_red_where_the_mask_covers_and_nothing_elsewhere() {
        let mut base = vec![100u8, 100, 100, 100, 100, 100];
        let coverage = vec![255u8, 255, 255, 0, 0, 0];
        assert!(paint_overlay(&mut base, (2, 1), &coverage, (2, 1)));
        // Covered: halfway to red.
        assert_eq!(&base[0..3], &[178, 50, 50]);
        // Uncovered: exactly as it was.
        assert_eq!(&base[3..6], &[100, 100, 100]);
    }

    #[test]
    fn a_size_mismatch_paints_nothing() {
        let mut base = vec![100u8; 6];
        let coverage = vec![255u8; 3];
        assert!(!paint_overlay(&mut base, (2, 1), &coverage, (1, 1)));
        assert_eq!(base, vec![100u8; 6]);
    }

    /// ADR 0093 §2: a luminance click writes the band around the sample and
    /// turns the term on; a hue click moves the center and keeps the width;
    /// a click with no selected row proposes nothing.
    #[test]
    fn eyedropper_click_writes_the_band_it_names() {
        let entry = LocalAdjustment {
            mask: Mask::Everything,
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues::default(),
        };
        let entries = vec![entry];

        let Some((_, Value::LocalAdjustment(Some(written)))) =
            sample_field(0, "range-lum", (0.62, 200.0), &entries)
        else {
            panic!("luminance click must propose an entry");
        };
        let lum = written.range.unwrap().luminance.unwrap();
        assert!((lum.min - 0.52).abs() < 1e-9 && (lum.max - 0.72).abs() < 1e-9);
        assert!((lum.softness - 0.1).abs() < 1e-9, "default softness kept");

        // Near white the band clamps instead of leaving the axis.
        let Some((_, Value::LocalAdjustment(Some(written)))) =
            sample_field(0, "range-lum", (0.97, 0.0), &entries)
        else {
            panic!()
        };
        assert!((written.range.unwrap().luminance.unwrap().max - 1.0).abs() < 1e-9);

        // A hue click moves the center only; a chosen width survives.
        let mut colored = entries.clone();
        colored[0].range = Some(RangeMask {
            luminance: None,
            color: Some(ColorRange {
                center: 10.0,
                width: 55.0,
                softness: 5.0,
            }),
        });
        let Some((_, Value::LocalAdjustment(Some(written)))) =
            sample_field(0, "range-color", (0.5, 200.0), &colored)
        else {
            panic!()
        };
        let color = written.range.unwrap().color.unwrap();
        assert!((color.center - 200.0).abs() < 1e-9);
        assert!((color.width - 55.0).abs() < 1e-9 && (color.softness - 5.0).abs() < 1e-9);

        assert!(sample_field(3, "range-lum", (0.5, 0.0), &entries).is_none());
        assert!(sample_field(0, "bogus", (0.5, 0.0), &entries).is_none());
    }

    /// ADR 0097: a handle drag edits the geometry and nothing else — the
    /// feather, the inversion and the angle the entry accumulated survive,
    /// which is the whole reason handles beat retracing.
    #[test]
    fn a_handle_drag_moves_the_geometry_and_keeps_everything_else() {
        let radial = Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.2,
            ry: 0.1,
            angle: 30.0,
            feather: 0.8,
            inverted: true,
        };
        // A 100x100 preview filling a 100x100 viewport: view pixels are
        // percentages, so the arithmetic below is readable.
        let view = (100.0, 100.0);
        let image = (100.0, 100.0);

        let Some(Mask::Radial {
            cx,
            cy,
            rx,
            ry,
            angle,
            feather,
            inverted,
        }) = drag_handle("center", (25.0, 75.0), view, image, &radial)
        else {
            panic!("the centre handle must move the ellipse");
        };
        assert!((cx - 0.25).abs() < 1e-9 && (cy - 0.75).abs() < 1e-9);
        // Everything that is not the position is preserved verbatim.
        assert!((rx - 0.2).abs() < 1e-9 && (ry - 0.1).abs() < 1e-9);
        assert!((angle - 30.0).abs() < 1e-9);
        assert!((feather - 0.8).abs() < 1e-9);
        assert!(inverted, "the inversion must survive a move");

        // Resizing works from the centre outward: the ellipse stays put.
        let Some(Mask::Radial { cx, rx, ry, .. }) =
            drag_handle("rx", (80.0, 50.0), view, image, &radial)
        else {
            panic!()
        };
        assert!((cx - 0.5).abs() < 1e-9, "the centre must not move");
        assert!((rx - 0.3).abs() < 1e-9, "rx follows the handle");
        assert!((ry - 0.1).abs() < 1e-9, "the other axis is untouched");

        let Some(Mask::Radial { ry, .. }) = drag_handle("ry", (50.0, 20.0), view, image, &radial)
        else {
            panic!()
        };
        assert!(
            (ry - 0.3).abs() < 1e-9,
            "dragging above the centre resizes by distance, got {ry}"
        );
    }

    /// Both ends of a gradient move, and only the one grabbed.
    #[test]
    fn a_gradient_handle_moves_the_end_it_names() {
        let gradient = Mask::Gradient {
            x0: 0.2,
            y0: 0.2,
            x1: 0.8,
            y1: 0.8,
        };
        let (view, image) = ((100.0, 100.0), (100.0, 100.0));

        let Some(Mask::Gradient { x0, y0, x1, y1 }) =
            drag_handle("from", (10.0, 30.0), view, image, &gradient)
        else {
            panic!()
        };
        assert!((x0 - 0.1).abs() < 1e-9 && (y0 - 0.3).abs() < 1e-9);
        assert!((x1 - 0.8).abs() < 1e-9 && (y1 - 0.8).abs() < 1e-9);

        let Some(Mask::Gradient { x0, x1, y1, .. }) =
            drag_handle("to", (90.0, 10.0), view, image, &gradient)
        else {
            panic!()
        };
        assert!((x0 - 0.2).abs() < 1e-9, "the other end stays");
        assert!((x1 - 0.9).abs() < 1e-9 && (y1 - 0.1).abs() < 1e-9);
    }

    /// ADR 0097 §3: a degenerate result is refused, so the geometry stands.
    #[test]
    fn a_degenerate_handle_drag_is_refused() {
        let radial = Mask::Radial {
            cx: 0.5,
            cy: 0.5,
            rx: 0.2,
            ry: 0.1,
            angle: 0.0,
            feather: 0.5,
            inverted: false,
        };
        let (view, image) = ((100.0, 100.0), (100.0, 100.0));
        // Dropped onto the centre: a zero-radius ellipse covers nothing.
        assert!(drag_handle("rx", (50.0, 50.0), view, image, &radial).is_none());

        let gradient = Mask::Gradient {
            x0: 0.2,
            y0: 0.2,
            x1: 0.8,
            y1: 0.8,
        };
        // Both ends in the same place: no axis, no gradient.
        assert!(drag_handle("to", (20.0, 20.0), view, image, &gradient).is_none());

        // A handle that does not belong to the geometry proposes nothing,
        // and neither does an unknown name.
        assert!(drag_handle("from", (10.0, 10.0), view, image, &radial).is_none());
        assert!(drag_handle("bogus", (10.0, 10.0), view, image, &gradient).is_none());
    }
}
