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
            if rx < 0.005 || ry < 0.005 {
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
            if (a.0 - b.0).abs() < 0.005 && (a.1 - b.1).abs() < 0.005 {
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
    let index = usize::try_from(selected)
        .ok()
        .filter(|&i| matches!(current.get(i).map(|e| &e.mask), Some(Mask::Brush { .. })));
    match index {
        Some(index) => {
            let mut entry = current[index].clone();
            if let Mask::Brush { strokes } = &mut entry.mask {
                strokes.push(stroke);
            }
            (
                Param::LocalAdjustment(index),
                Value::LocalAdjustment(Some(entry)),
            )
        }
        None => (
            Param::LocalAdjustment(current.len()),
            Value::LocalAdjustment(Some(fresh(Mask::Brush {
                strokes: vec![stroke],
            }))),
        ),
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

/// Converts an image file into the coverage samples
/// `Library::store_mask_coverage` takes (ADR 0070 §7).
///
/// Which channel becomes the coverage is the whole decision, and getting it
/// wrong would silently invert or flatten someone's work:
///
/// 1. **alpha**, when the image has one and it is not uniformly opaque — a
///    selection exported with its transparency, whose alpha *is* the mask;
/// 2. **luminance** otherwise — a black-and-white mask, white = covered.
///
/// The order matters: a selection exported as PNG often carries black pixels
/// *and* an alpha channel, and reading its luminance would import an empty
/// mask. A fully opaque image says nothing through its alpha, hence the
/// fallback.
///
/// No resampling: the file's own resolution is what gets stored (§2).
pub fn coverage_from_image(image: &image::DynamicImage) -> (u32, u32, Vec<u16>) {
    let rgba = image.to_rgba16();
    let (width, height) = (rgba.width(), rgba.height());
    let opaque = rgba.pixels().all(|p| p.0[3] == u16::MAX);
    let samples = rgba
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            if opaque {
                // Rec. 709 luma, the same axis the develop panel's histogram
                // uses — a mask's grey is a *display* grey, not linear light.
                let luma = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
                luma.round().clamp(0.0, f64::from(u16::MAX)) as u16
            } else {
                a
            }
        })
        .collect();
    (width, height, samples)
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
pub fn paint_overlay(base: &mut [u8], base_size: (u32, u32), coverage: &[u8], coverage_size: (u32, u32)) -> bool {
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

    /// ADR 0070 §7: a transparent selection is read through its alpha, an
    /// opaque grey image through its luminance. Reading the wrong one is a
    /// silently empty — or silently full — mask.
    #[test]
    fn a_transparent_selection_is_read_through_its_alpha() {
        // Black pixels, half of them transparent: luminance would say "no
        // coverage anywhere", alpha says "the opaque half".
        let mut selection = image::RgbaImage::new(2, 1);
        selection.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        selection.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        let (width, height, samples) =
            coverage_from_image(&image::DynamicImage::ImageRgba8(selection));
        assert_eq!((width, height), (2, 1));
        assert_eq!(samples, vec![u16::MAX, 0]);
    }

    #[test]
    fn an_opaque_grey_mask_is_read_through_its_luminance() {
        let mut painted = image::RgbaImage::new(3, 1);
        painted.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        painted.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        painted.put_pixel(2, 0, image::Rgba([128, 128, 128, 255]));
        let (_, _, samples) = coverage_from_image(&image::DynamicImage::ImageRgba8(painted));
        assert_eq!(samples[0], 0);
        assert_eq!(samples[1], u16::MAX);
        // Mid grey lands mid range, whatever the 8->16 bit expansion does.
        assert!(
            (samples[2] as i32 - (u16::MAX / 2) as i32).abs() < 600,
            "got {}",
            samples[2]
        );
    }

    /// A file with no alpha channel at all still imports, through luminance.
    #[test]
    fn an_image_without_alpha_imports_through_luminance() {
        let mut rgb = image::RgbImage::new(2, 1);
        rgb.put_pixel(0, 0, image::Rgb([255, 255, 255]));
        rgb.put_pixel(1, 0, image::Rgb([0, 0, 0]));
        let (_, _, samples) = coverage_from_image(&image::DynamicImage::ImageRgb8(rgb));
        assert_eq!(samples, vec![u16::MAX, 0]);
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
}
