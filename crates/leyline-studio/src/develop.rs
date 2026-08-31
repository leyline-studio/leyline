//! Maps develop sliders to engine parameter updates (`docs/pipeline.md`).
//!
//! Pure functions kept free of any Slint type so every rule is
//! unit-testable: the UI forwards the slider name and released value, and
//! the session applies whatever comes back.

use leyline_sdk::{
    ColorGrading, Crop, CurvePoint, Demosaic, HighlightReconstruction, HslBand, LensCorrection,
    NoiseReduction, Param, Point, Settings, Sharpening, SpotRemoval, ToneCurve, Value,
};

/// Decodes a slider release into an engine parameter update.
///
/// `current` supplies the multi-field steps the slider merges into: the
/// white balance pair (starting from the engine default when the photo is
/// still as-shot), the noise reduction and sharpening pairs, and the crop
/// rectangle — in percent of the frame, clamped so it stays inside the
/// frame; a crop grown back to the full frame clears the override.
pub fn action(slider: &str, value: f64, current: &Settings) -> Option<(Param, Value)> {
    if let Some(rest) = slider.strip_prefix("crop-") {
        return crop_action(rest, value, current.crop.clone())
            .map(|next| (Param::Crop, Value::Crop(next)));
    }
    let int = Value::Int(value.round() as i32);
    Some(match slider {
        "exposure" => (Param::Exposure, Value::Float(value)),
        "rotation" => (Param::Rotation, Value::Float(value)),
        // The two sliders are one tool, so each merges into the other's
        // current value — the same pattern white balance and noise reduction
        // follow. Both back at zero clears the override entirely, which is
        // what keeps a neutral revision free of the field (ADR 0052 §1).
        "perspective-vertical" | "perspective-horizontal" => {
            let mut perspective = current.perspective.unwrap_or_default();
            if slider == "perspective-vertical" {
                perspective.vertical = value.round() as i32;
            } else {
                perspective.horizontal = value.round() as i32;
            }
            let perspective =
                (perspective.vertical != 0 || perspective.horizontal != 0).then_some(perspective);
            (Param::Perspective, Value::Perspective(perspective))
        }
        "contrast" => (Param::Contrast, int),
        "highlights" => (Param::Highlights, int),
        "shadows" => (Param::Shadows, int),
        "whites" => (Param::Whites, int),
        "blacks" => (Param::Blacks, int),
        "clarity" => (Param::Clarity, int),
        "texture" => (Param::Texture, int),
        "dehaze" => (Param::Dehaze, int),
        "highlight-rolloff" => (Param::HighlightRolloff, int),
        "vibrance" => (Param::Vibrance, int),
        "saturation" => (Param::Saturation, int),
        // Toggling only ever flips `enabled` on a profile the user has
        // already chosen (ADR 0035): the path and checksum come from the
        // engine's import, never from this pure mapping, so a toggle with
        // nothing referenced is a no-op rather than an invented reference.
        "camera-profile" => {
            let mut profile = current.camera_profile.clone()?;
            profile.enabled = value != 0.0;
            (Param::CameraProfile, Value::CameraProfile(Some(profile)))
        }
        // Like `camera-profile`: a toggle only ever flips `enabled` on a look
        // the user already chose, and the strength only moves a reference that
        // exists (ADR 0053 §1).
        "lut" => {
            let mut lut = current.lut.clone()?;
            lut.enabled = value != 0.0;
            (Param::Lut, Value::Lut(Some(lut)))
        }
        "lut-strength" => {
            let mut lut = current.lut.clone()?;
            lut.strength = value.round() as i32;
            (Param::Lut, Value::Lut(Some(lut)))
        }
        // A toggle and nothing else (ADR 0088 §5): it does not zero the
        // saturation, does not touch the mixer, does not "apply a look".
        // Pressing it twice gives the photo back exactly as it was.
        "monochrome" => (Param::Monochrome, Value::Bool(value != 0.0)),
        "lens-correction" => (
            Param::LensCorrection,
            Value::LensCorrection(LensCorrection {
                enabled: value != 0.0,
                profile: "auto".to_owned(),
            }),
        ),
        "wb-temp" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.temperature = value.round() as u32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "wb-tint" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.tint = value.round() as i32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "nr-luminance" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "nr-color" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                color: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "sharpen-amount" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: value.round() as i32,
                ..current.sharpening.clone()
            }),
        ),
        "sharpen-masking" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                masking: value.round() as i32,
                ..current.sharpening.clone()
            }),
        ),
        "sharpen-radius" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                radius: value.max(0.1),
                ..current.sharpening.clone()
            }),
        ),
        // Each slider merges into the other three, the pattern white
        // balance and noise reduction already follow: the engine commits a
        // vignette whole because a shape and a strength are one tool
        // (ADR 0090 §2).
        "vignette-amount" | "vignette-midpoint" | "vignette-roundness" | "vignette-feather" => {
            let mut vignette = current.vignette;
            let v = value.round() as i32;
            match slider {
                "vignette-amount" => vignette.amount = v,
                "vignette-midpoint" => vignette.midpoint = v,
                "vignette-roundness" => vignette.roundness = v,
                _ => vignette.feather = v,
            }
            (Param::Vignette, Value::Vignette(vignette))
        }
        "grain-amount" | "grain-size" | "grain-roughness" => {
            let mut grain = current.grain;
            let v = value.round() as i32;
            match slider {
                "grain-amount" => grain.amount = v,
                "grain-size" => grain.size = v,
                _ => grain.roughness = v,
            }
            (Param::Grain, Value::Grain(grain))
        }
        "color-grading-balance" => (
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                balance: value.round() as i32,
                ..current.color_grading
            }),
        ),
        "color-grading-blending" => (
            Param::ColorGrading,
            Value::ColorGrading(ColorGrading {
                blending: value.round() as i32,
                ..current.color_grading
            }),
        ),
        _ => return None,
    })
}

/// Decodes the highlight-reconstruction picker (ADR 0050): `mode` is
/// `"clip"`/`"blend"`/`"rebuild"`, the three the decoder can be asked for.
/// An unknown name is no update rather than a guess.
///
/// Unlike every other control of the panel this one carries a name, not a
/// number: the modes are not points on a scale — `rebuild` is not "more
/// blend" — so a slider would invite an interpolation that does not exist.
pub fn highlight_reconstruction_action(mode: &str) -> Option<(Param, Value)> {
    let mode = match mode {
        "clip" => HighlightReconstruction::Clip,
        "blend" => HighlightReconstruction::Blend,
        "rebuild" => HighlightReconstruction::Rebuild,
        _ => return None,
    };
    Some((
        Param::HighlightReconstruction,
        Value::HighlightReconstruction(mode),
    ))
}

/// Decodes the demosaic picker (ADR 0061): `"ahd"`, `"vng"`, `"dcb"` or
/// `"dht"`. An unknown name is no update rather than a guess.
///
/// A name and not a number, for the same reason as the mode above: the
/// algorithms are not points on a scale, and a slider would invite an
/// interpolation between them that does not exist.
pub fn demosaic_action(algorithm: &str) -> Option<(Param, Value)> {
    let algorithm = match algorithm {
        "ahd" => Demosaic::Ahd,
        "vng" => Demosaic::Vng,
        "dcb" => Demosaic::Dcb,
        "dht" => Demosaic::Dht,
        _ => return None,
    };
    Some((Param::Demosaic, Value::Demosaic(algorithm)))
}

/// Decodes one HSL mixer band's slider release (ADR 0031). `index` is the
/// band's position in `current.hsl` (fixed order: red, orange, yellow,
/// green, aqua, blue, purple, magenta); `field` is `"hue"`/`"saturation"`/
/// `"luminance"`. Reads the band's other two fields from `current` so
/// moving one slider never resets its siblings — the same merge pattern
/// `action`'s `wb-temp`/`nr-luminance` cases use.
pub fn hsl_band_action(
    index: usize,
    field: &str,
    value: f64,
    current: &Settings,
) -> Option<(Param, Value)> {
    let mut band: HslBand = *current.hsl.get(index)?;
    match field {
        "hue" => band.hue = value.round() as i32,
        "saturation" => band.saturation = value.round() as i32,
        "luminance" => band.luminance = value.round() as i32,
        _ => return None,
    }
    Some((Param::HslBand(index), Value::HslBand(band)))
}

/// Decodes one color grading zone's slider release (ADR 0031). `zone` is
/// `"shadows"`/`"midtones"`/`"highlights"`, `field` is `"hue"`/
/// `"saturation"`/`"luminance"`. `Param::ColorGrading` commits the whole
/// struct as one tool, so this merges into `current.color_grading` rather
/// than replacing it, same reasoning as [`hsl_band_action`].
pub fn color_grading_zone_action(
    zone: &str,
    field: &str,
    value: f64,
    current: &Settings,
) -> Option<(Param, Value)> {
    let mut grading: ColorGrading = current.color_grading;
    let target = match zone {
        "shadows" => &mut grading.shadows,
        "midtones" => &mut grading.midtones,
        "highlights" => &mut grading.highlights,
        _ => return None,
    };
    match field {
        "hue" => target.hue = value.round() as i32,
        "saturation" => target.saturation = value.round() as i32,
        "luminance" => target.luminance = value.round() as i32,
        _ => return None,
    }
    Some((Param::ColorGrading, Value::ColorGrading(grading)))
}

/// Decodes a mouse drag over the develop preview into a crop update.
///
/// The preview is letterboxed inside its viewport (`image-fit: contain`)
/// and already shows the current crop, so the dragged rectangle selects a
/// sub-rectangle of the *displayed* frame; the returned crop is that
/// selection composed with `current`, back in full-frame coordinates.
/// `view` and `image` are the viewport and preview sizes in their own
/// pixels. Drags covering less than 1 % of the frame in either direction
/// are ignored, as is a degenerate viewport or preview.
pub fn drag_crop(
    press: (f64, f64),
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    current: &Option<Crop>,
) -> Option<(Param, Value)> {
    let a = letterbox_unit(press, view, image)?;
    let b = letterbox_unit(release, view, image)?;
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (width, height) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    if width < 0.01 || height < 0.01 {
        return None;
    }
    let base = current.clone().unwrap_or(FULL_FRAME);
    let crop = Crop {
        x: base.x + x0 * base.width,
        y: base.y + y0 * base.height,
        width: width * base.width,
        height: height * base.height,
    };
    (crop != FULL_FRAME).then_some((Param::Crop, Value::Crop(Some(crop))))
}

/// Maps a point over the letterboxed develop preview (`image-fit: contain`)
/// into `[0, 1]` unit coordinates of the *image itself* — the referential
/// [`Crop`] and [`Point`] (ADR 0026) both share. `None` for a degenerate
/// viewport or preview.
pub(crate) fn letterbox_unit(
    point: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
) -> Option<(f64, f64)> {
    if view.0 <= 0.0 || view.1 <= 0.0 || image.0 <= 0.0 || image.1 <= 0.0 {
        return None;
    }
    let scale = (view.0 / image.0).min(view.1 / image.1);
    let (offset_x, offset_y) = (
        (view.0 - image.0 * scale) / 2.0,
        (view.1 - image.1 * scale) / 2.0,
    );
    Some((
        ((point.0 - offset_x) / (image.0 * scale)).clamp(0.0, 1.0),
        ((point.1 - offset_y) / (image.1 * scale)).clamp(0.0, 1.0),
    ))
}

/// How close a click needs to land to an existing tone-curve point (in unit
/// curve-graph coordinates) to be treated as "on that point" rather than a
/// new one.
const CURVE_POINT_RADIUS: f64 = 0.05;

/// Decodes a click on the tone-curve graph (ADR 0030): clicking near an
/// existing point removes it, clicking elsewhere adds a new one — the
/// simplest editable interaction that keeps every point's `x` unique and
/// the list sorted, both required by `ToneCurve` (`leyline-core::settings`).
/// A click within [`CURVE_POINT_RADIUS`] of another point's `x` (but not
/// close enough to remove it) is ignored rather than creating an
/// ambiguous, near-duplicate control point.
///
/// `ToneCurve` also requires **either zero points or at least two**
/// (`leyline-core::settings::Settings::validate`) — a lone point is not a
/// valid curve. So the first point ever placed silently seeds the fixed
/// `(0, 0)`/`(1, 1)` identity endpoints alongside it (three points total),
/// and removing a point back down to one clears the curve entirely rather
/// than leaving that invalid single-point state.
pub fn curve_point(click: (f64, f64), current: &[CurvePoint]) -> Option<(Param, Value)> {
    let (x, y) = (click.0.clamp(0.0, 1.0), click.1.clamp(0.0, 1.0));
    let mut points = current.to_vec();
    if let Some(i) = points
        .iter()
        .position(|p| (p.x - x).powi(2) + (p.y - y).powi(2) < CURVE_POINT_RADIUS.powi(2))
    {
        points.remove(i);
        if points.len() < 2 {
            points.clear();
        }
        return Some((Param::ToneCurve, Value::ToneCurve(ToneCurve { points })));
    }
    // The identity endpoints always end up in the list once it holds any
    // point at all, so a too-close-to-them click is rejected up front even
    // before they are actually seeded.
    let seeded_endpoints: &[f64] = if points.is_empty() { &[0.0, 1.0] } else { &[] };
    if points
        .iter()
        .map(|p| p.x)
        .chain(seeded_endpoints.iter().copied())
        .any(|px| (px - x).abs() < CURVE_POINT_RADIUS)
    {
        return None;
    }
    if points.is_empty() {
        points.push(CurvePoint { x: 0.0, y: 0.0 });
        points.push(CurvePoint { x: 1.0, y: 1.0 });
    }
    points.push(CurvePoint { x, y });
    points.sort_by(|a, b| a.x.partial_cmp(&b.x).expect("curve x is never NaN"));
    Some((Param::ToneCurve, Value::ToneCurve(ToneCurve { points })))
}

/// Clears every tone-curve point back to the identity curve.
pub fn reset_curve() -> (Param, Value) {
    (Param::ToneCurve, Value::ToneCurve(ToneCurve::default()))
}

/// Builds the tone-curve graph's SVG-style line commands and marker
/// positions, both in the same `size`-pixel-square canvas space — pure so
/// the coordinate math is unit-tested without a live Slint canvas. Empty
/// `points` (the identity curve) yields no commands and no markers; the
/// canvas itself always draws the diagonal reference line.
pub fn curve_layout(points: &[CurvePoint], size: f64) -> (String, Vec<(f64, f64)>) {
    let markers: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.x * size, (1.0 - p.y) * size))
        .collect();
    let mut path = String::new();
    for (i, (x, y)) in markers.iter().enumerate() {
        if i > 0 {
            path.push(' ');
        }
        path.push_str(&format!("{}{x} {y}", if i == 0 { "M" } else { "L" }));
    }
    (path, markers)
}

/// Paints display-referred clipping over an 8-bit sRGB render, in place
/// (ADR 0092 §1): a clipped highlight is a channel at 255, painted red when
/// `paint_highlights`; a crushed shadow is all three at 0, painted blue
/// when `paint_shadows`. Returns whether each end is occupied at all — the
/// histogram's two triangles light from that, overlay on or off.
pub fn paint_clipping(
    data: &mut [u8],
    paint_highlights: bool,
    paint_shadows: bool,
) -> (bool, bool) {
    let mut any_high = false;
    let mut any_low = false;
    for rgb in data.chunks_exact_mut(3) {
        let high = rgb.iter().any(|&v| v == 255);
        let low = rgb.iter().all(|&v| v == 0);
        any_high |= high;
        any_low |= low;
        if high && paint_highlights {
            (rgb[0], rgb[1], rgb[2]) = (255, 59, 48);
        } else if low && paint_shadows {
            (rgb[0], rgb[1], rgb[2]) = (10, 132, 255);
        }
    }
    (any_high, any_low)
}

/// Builds one channel's filled-area histogram path in a `width` x `height`
/// viewbox, `sqrt`-scaled against `scale_max` (the tallest bin *across all
/// three channels*, so R/G/B stay on the same vertical scale — passing each
/// channel's own max instead would make every channel look equally tall no
/// matter its actual weight). `sqrt` rather than linear: a single dominant
/// bin (a large flat sky, a black border) would otherwise flatten every
/// other bin to near-zero height. Pure so the scaling math is unit-tested
/// without a live Slint canvas.
pub fn histogram_layout(bins: &[u32; 256], scale_max: u32, width: f64, height: f64) -> String {
    let scale = (scale_max as f64).sqrt().max(1.0);
    let step = width / 255.0;
    let mut path = format!("M0 {height}");
    for (i, &count) in bins.iter().enumerate() {
        let x = i as f64 * step;
        let bar_height = (count as f64).sqrt() / scale * height;
        path.push_str(&format!(" L{x} {}", height - bar_height));
    }
    path.push_str(&format!(" L{width} {height} Z"));
    path
}

/// Decodes a two-click spot-removal placement over the develop preview
/// (ADR 0032): `source_click`/`target_click` are the first and second
/// clicks in view pixels, mapped through the same letterbox as
/// [`drag_crop`] into the shared `Point` referential (ADR 0026). Appends
/// one spot to `current` with `radius_feather_opacity` (already in their
/// stored units, `[0, 1]`) — grouped into one tuple to keep the parameter
/// count reasonable.
pub fn place_spot(
    source_click: (f64, f64),
    target_click: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    radius_feather_opacity: (f64, f64, f64),
    current: &[SpotRemoval],
) -> Option<(Param, Value)> {
    let (radius, feather, opacity) = radius_feather_opacity;
    let source = letterbox_unit(source_click, view, image)?;
    let target = letterbox_unit(target_click, view, image)?;
    if radius <= 0.0 {
        return None;
    }
    let mut spots = current.to_vec();
    spots.push(SpotRemoval {
        source: Point {
            x: source.0,
            y: source.1,
        },
        target: Point {
            x: target.0,
            y: target.1,
        },
        radius,
        feather: feather.clamp(0.0, 1.0),
        opacity: opacity.clamp(0.0, 1.0),
    });
    Some((Param::SpotRemoval, Value::SpotRemoval(spots)))
}

/// Removes the most recently placed spot, if any.
pub fn undo_last_spot(current: &[SpotRemoval]) -> Option<(Param, Value)> {
    if current.is_empty() {
        return None;
    }
    let mut spots = current.to_vec();
    spots.pop();
    Some((Param::SpotRemoval, Value::SpotRemoval(spots)))
}

/// Clears every placed spot.
pub fn reset_spots() -> (Param, Value) {
    (Param::SpotRemoval, Value::SpotRemoval(Vec::new()))
}

/// The crop edge sliders, in percent of the frame. The smallest accepted
/// edge is 1 % so the rectangle never collapses.
fn crop_action(edge: &str, percent: f64, current: Option<Crop>) -> Option<Option<Crop>> {
    if edge == "reset" {
        return Some(None);
    }
    let mut crop = current.unwrap_or(FULL_FRAME);
    let value = (percent / 100.0).clamp(0.0, 1.0);
    match edge {
        "left" => {
            crop.x = value.min(0.99);
            crop.width = crop.width.min(1.0 - crop.x);
        }
        "top" => {
            crop.y = value.min(0.99);
            crop.height = crop.height.min(1.0 - crop.y);
        }
        "width" => crop.width = value.clamp(0.01, 1.0 - crop.x),
        "height" => crop.height = value.clamp(0.01, 1.0 - crop.y),
        _ => return None,
    }
    Some(if crop == FULL_FRAME { None } else { Some(crop) })
}

/// The neutral crop: the whole frame.
const FULL_FRAME: Crop = Crop {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

#[cfg(test)]
mod tests {
    use leyline_sdk::{CameraProfile, Perspective, WhiteBalance};

    use super::*;

    /// Neutral settings, with the given white balance / crop overrides.
    fn settings(wb: Option<WhiteBalance>, crop: Option<Crop>) -> Settings {
        Settings {
            white_balance: wb,
            crop,
            ..Settings::default()
        }
    }

    #[test]
    fn exposure_and_rotation_stay_floats() {
        let neutral = settings(None, None);
        assert_eq!(
            action("exposure", 1.25, &neutral),
            Some((Param::Exposure, Value::Float(1.25)))
        );
        assert_eq!(
            action("rotation", -2.5, &neutral),
            Some((Param::Rotation, Value::Float(-2.5)))
        );
    }

    #[test]
    fn sliders_round_to_integers() {
        let neutral = settings(None, None);
        assert_eq!(
            action("contrast", 24.6, &neutral),
            Some((Param::Contrast, Value::Int(25)))
        );
        assert_eq!(
            action("saturation", -99.7, &neutral),
            Some((Param::Saturation, Value::Int(-100)))
        );
    }

    #[test]
    fn white_balance_sliders_merge_into_the_override() {
        let current = settings(
            Some(WhiteBalance {
                temperature: 5200,
                tint: 12,
            }),
            None,
        );
        assert_eq!(
            action("wb-temp", 7000.0, &current),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: 7000,
                    tint: 12,
                }))
            ))
        );
        assert_eq!(
            action("wb-tint", -20.0, &current),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: 5200,
                    tint: -20,
                }))
            ))
        );
    }

    #[test]
    fn as_shot_starts_from_the_default_override() {
        let default = WhiteBalance::default();
        assert_eq!(
            action("wb-tint", 15.0, &settings(None, None)),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: default.temperature,
                    tint: 15,
                }))
            ))
        );
    }

    #[test]
    fn detail_sliders_keep_their_sibling_field() {
        let mut current = settings(None, None);
        current.noise_reduction.color = 40;
        current.sharpening.radius = 2.0;
        assert_eq!(
            action("nr-luminance", 30.0, &current),
            Some((
                Param::NoiseReduction,
                Value::NoiseReduction(NoiseReduction {
                    luminance: 30,
                    color: 40,
                })
            ))
        );
        assert_eq!(
            action("sharpen-amount", 55.0, &current),
            Some((
                Param::Sharpening,
                Value::Sharpening(Sharpening {
                    amount: 55,
                    radius: 2.0,
                })
            ))
        );
        assert_eq!(
            action("sharpen-radius", 0.0, &current),
            Some((
                Param::Sharpening,
                Value::Sharpening(Sharpening {
                    amount: 0,
                    radius: 0.1,
                })
            ))
        );
    }

    #[test]
    fn camera_profile_toggles_the_referenced_profile_and_never_invents_one() {
        let neutral = settings(None, None);
        assert_eq!(action("camera-profile", 1.0, &neutral), None);

        let referenced = Settings {
            camera_profile: Some(CameraProfile {
                enabled: true,
                path: "Profiles/Camera/mine.dcp".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
            }),
            ..neutral
        };
        let Some((Param::CameraProfile, Value::CameraProfile(Some(off)))) =
            action("camera-profile", 0.0, &referenced)
        else {
            panic!("expected a camera profile update");
        };
        assert!(!off.enabled);
        // The path and checksum are carried through untouched.
        assert_eq!(off.path, "Profiles/Camera/mine.dcp");
        assert_eq!(off.checksum, referenced.camera_profile.unwrap().checksum);
    }

    #[test]
    fn lens_correction_toggles_on_and_off() {
        let neutral = settings(None, None);
        assert_eq!(
            action("lens-correction", 1.0, &neutral),
            Some((
                Param::LensCorrection,
                Value::LensCorrection(LensCorrection {
                    enabled: true,
                    profile: "auto".to_owned(),
                })
            ))
        );
        assert_eq!(
            action("lens-correction", 0.0, &neutral),
            Some((
                Param::LensCorrection,
                Value::LensCorrection(LensCorrection {
                    enabled: false,
                    profile: "auto".to_owned(),
                })
            ))
        );
    }

    #[test]
    fn the_demosaic_picker_names_its_four_algorithms() {
        for (name, expected) in [
            ("ahd", Demosaic::Ahd),
            ("vng", Demosaic::Vng),
            ("dcb", Demosaic::Dcb),
            ("dht", Demosaic::Dht),
        ] {
            assert_eq!(
                demosaic_action(name),
                Some((Param::Demosaic, Value::Demosaic(expected))),
                "{name}"
            );
        }
        // AMaZE and LMMSE live in LibRaw's GPL demosaic packs, absent from
        // the linked build (ADR 0061): naming one must be no update, never
        // a silent fall back to AHD.
        assert_eq!(demosaic_action("amaze"), None);
        assert_eq!(demosaic_action("lmmse"), None);
    }

    #[test]
    fn the_highlight_reconstruction_picker_names_its_three_modes() {
        assert_eq!(
            highlight_reconstruction_action("rebuild"),
            Some((
                Param::HighlightReconstruction,
                Value::HighlightReconstruction(HighlightReconstruction::Rebuild)
            ))
        );
        assert_eq!(
            highlight_reconstruction_action("clip"),
            Some((
                Param::HighlightReconstruction,
                Value::HighlightReconstruction(HighlightReconstruction::Clip)
            ))
        );
        assert_eq!(highlight_reconstruction_action("guess"), None);
    }

    #[test]
    fn the_perspective_sliders_merge_and_clear_together() {
        let neutral = settings(None, None);
        assert_eq!(
            action("perspective-vertical", 40.0, &neutral),
            Some((
                Param::Perspective,
                Value::Perspective(Some(Perspective {
                    vertical: 40,
                    horizontal: 0
                }))
            ))
        );
        let tilted = Settings {
            perspective: Some(Perspective {
                vertical: 40,
                horizontal: 0,
            }),
            ..settings(None, None)
        };
        // Moving one slider keeps the other.
        assert_eq!(
            action("perspective-horizontal", -20.0, &tilted),
            Some((
                Param::Perspective,
                Value::Perspective(Some(Perspective {
                    vertical: 40,
                    horizontal: -20
                }))
            ))
        );
        // Back to zero on the only non-zero term clears the override.
        assert_eq!(
            action("perspective-vertical", 0.0, &tilted),
            Some((Param::Perspective, Value::Perspective(None)))
        );
    }

    /// A look is a referenced file: nothing here invents one, exactly like the
    /// camera profile above.
    #[test]
    fn the_lut_controls_only_move_a_look_that_was_already_chosen() {
        let neutral = settings(None, None);
        assert_eq!(action("lut", 1.0, &neutral), None);
        assert_eq!(action("lut-strength", 50.0, &neutral), None);

        let referenced = Settings {
            lut: Some(leyline_sdk::Lut {
                enabled: true,
                path: "Profiles/LUT/look.cube".to_owned(),
                checksum: format!("blake3:{}", "a".repeat(64)),
                strength: 100,
            }),
            ..neutral
        };
        let Some((Param::Lut, Value::Lut(Some(off)))) = action("lut", 0.0, &referenced) else {
            panic!("expected a LUT update");
        };
        assert!(!off.enabled);
        assert_eq!(off.path, "Profiles/LUT/look.cube");
        let Some((Param::Lut, Value::Lut(Some(dosed)))) = action("lut-strength", 40.4, &referenced)
        else {
            panic!("expected a LUT update");
        };
        assert_eq!((dosed.strength, dosed.enabled), (40, true));
    }

    #[test]
    fn unknown_sliders_do_nothing() {
        let neutral = settings(None, None);
        assert_eq!(action("gamma", 1.0, &neutral), None);
        assert_eq!(action("crop-diagonal", 10.0, &neutral), None);
    }

    #[test]
    fn crop_sliders_merge_and_stay_inside_the_frame() {
        let Some((Param::Crop, Value::Crop(Some(crop)))) =
            action("crop-left", 25.0, &settings(None, None))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.x, crop.width), (0.25, 0.75));

        let Some((_, Value::Crop(Some(crop)))) =
            action("crop-width", 90.0, &settings(None, Some(crop)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.x, crop.width), (0.25, 0.75));

        let Some((_, Value::Crop(Some(crop)))) =
            action("crop-height", 50.0, &settings(None, Some(crop)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.y, crop.height), (0.0, 0.5));
    }

    /// Unwraps the crop rectangle produced by [`drag_crop`].
    fn dragged(update: Option<(Param, Value)>) -> Crop {
        match update {
            Some((Param::Crop, Value::Crop(Some(crop)))) => crop,
            other => panic!("expected a crop update, got {other:?}"),
        }
    }

    #[test]
    fn drag_maps_through_the_letterbox() {
        // A 100×100 preview centered in a 200×100 viewport: 50 px bands.
        let crop = dragged(drag_crop(
            (75.0, 25.0),
            (125.0, 75.0),
            (200.0, 100.0),
            (100.0, 100.0),
            &None,
        ));
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (0.25, 0.25, 0.5, 0.5)
        );
    }

    #[test]
    fn drag_direction_and_overshoot_are_normalized() {
        // Dragged up-left from outside the frame: clamps then reorders.
        let crop = dragged(drag_crop(
            (120.0, 80.0),
            (-10.0, 25.0),
            (100.0, 100.0),
            (100.0, 100.0),
            &None,
        ));
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (0.0, 0.25, 1.0, 0.55)
        );
    }

    #[test]
    fn drag_composes_with_the_current_crop() {
        let current = Some(Crop {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 0.5,
        });
        let crop = dragged(drag_crop(
            (25.0, 25.0),
            (75.0, 75.0),
            (100.0, 100.0),
            (100.0, 100.0),
            &current,
        ));
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (0.625, 0.125, 0.25, 0.25)
        );
    }

    #[test]
    fn tiny_and_degenerate_drags_are_ignored() {
        let view = (100.0, 100.0);
        assert_eq!(
            drag_crop((50.0, 50.0), (50.5, 90.0), view, view, &None),
            None
        );
        assert_eq!(
            drag_crop((50.0, 50.0), (90.0, 50.5), view, view, &None),
            None
        );
        assert_eq!(
            drag_crop((10.0, 10.0), (90.0, 90.0), view, (0.0, 100.0), &None),
            None
        );
        assert_eq!(
            drag_crop((10.0, 10.0), (90.0, 90.0), (0.0, 0.0), view, &None),
            None
        );
    }

    #[test]
    fn full_frame_drag_changes_nothing() {
        let view = (100.0, 100.0);
        assert_eq!(
            drag_crop((-5.0, -5.0), (110.0, 110.0), view, view, &None),
            None
        );
    }

    #[test]
    fn full_frame_and_reset_clear_the_crop() {
        let current = Crop {
            x: 0.2,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        assert_eq!(
            action("crop-reset", 0.0, &settings(None, Some(current.clone()))),
            Some((Param::Crop, Value::Crop(None)))
        );
        let Some((_, Value::Crop(Some(widened)))) =
            action("crop-left", 0.0, &settings(None, Some(current)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!(
            action("crop-width", 100.0, &settings(None, Some(widened))),
            Some((Param::Crop, Value::Crop(None)))
        );
    }

    /// Unwraps the tone-curve points produced by [`curve_point`].
    fn curved(update: Option<(Param, Value)>) -> Vec<CurvePoint> {
        match update {
            Some((Param::ToneCurve, Value::ToneCurve(curve))) => curve.points,
            other => panic!("expected a tone-curve update, got {other:?}"),
        }
    }

    #[test]
    fn curve_click_seeds_the_identity_endpoints_on_an_empty_curve() {
        // ToneCurve requires >= 2 points, or 0 (`leyline-core::settings`):
        // the first point ever placed brings the fixed (0,0)/(1,1)
        // endpoints along with it.
        let points = curved(curve_point((0.5, 0.75), &[]));
        assert_eq!(
            points,
            vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.5, y: 0.75 },
                CurvePoint { x: 1.0, y: 1.0 },
            ]
        );
    }

    #[test]
    fn curve_click_adds_a_sorted_point_to_an_existing_curve() {
        let current = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.75 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        let points = curved(curve_point((0.2, 0.1), &current));
        assert_eq!(
            points,
            vec![
                CurvePoint { x: 0.0, y: 0.0 },
                CurvePoint { x: 0.2, y: 0.1 },
                CurvePoint { x: 0.5, y: 0.75 },
                CurvePoint { x: 1.0, y: 1.0 },
            ]
        );
    }

    #[test]
    fn clicking_near_a_point_removes_it() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.5 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        assert_eq!(
            curved(curve_point((0.51, 0.49), &points)),
            vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }]
        );
    }

    #[test]
    fn removing_a_point_down_to_one_clears_the_whole_curve() {
        let points = vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 1.0, y: 1.0 }];
        assert_eq!(curved(curve_point((0.01, 0.01), &points)), vec![]);
    }

    #[test]
    fn clicking_near_another_points_x_is_ignored() {
        let points = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 0.5, y: 0.5 },
            CurvePoint { x: 1.0, y: 1.0 },
        ];
        assert_eq!(curve_point((0.52, 0.9), &points), None);
    }

    #[test]
    fn a_first_click_too_close_to_an_endpoint_is_ignored() {
        assert_eq!(curve_point((0.02, 0.5), &[]), None);
        assert_eq!(curve_point((0.98, 0.5), &[]), None);
    }

    #[test]
    fn reset_curve_clears_every_point() {
        assert_eq!(
            reset_curve(),
            (Param::ToneCurve, Value::ToneCurve(ToneCurve::default()))
        );
    }

    #[test]
    fn curve_layout_maps_unit_points_into_canvas_pixels_with_y_flipped() {
        let points = vec![CurvePoint { x: 0.0, y: 1.0 }, CurvePoint { x: 1.0, y: 0.0 }];
        let (path, markers) = curve_layout(&points, 200.0);
        assert_eq!(markers, vec![(0.0, 0.0), (200.0, 200.0)]);
        assert_eq!(path, "M0 0 L200 200");
    }

    #[test]
    fn histogram_layout_starts_and_ends_on_the_baseline() {
        let mut bins = [0u32; 256];
        bins[128] = 100;
        let path = histogram_layout(&bins, 100, 256.0, 90.0);
        assert!(path.starts_with("M0 90"));
        assert!(path.ends_with("L256 90 Z"));
    }

    #[test]
    fn histogram_layout_the_tallest_bin_reaches_the_top_when_it_is_the_scale_max() {
        let mut bins = [0u32; 256];
        bins[0] = 400; // sqrt(400) = 20 = scale, so this bin fills the full height
        let path = histogram_layout(&bins, 400, 256.0, 90.0);
        assert!(
            path.contains("L0 0 "),
            "expected bin 0 to reach the top: {path}"
        );
    }

    #[test]
    fn histogram_layout_handles_an_all_empty_scale_without_dividing_by_zero() {
        let bins = [0u32; 256];
        // scale_max of 0 (nothing rendered yet) must not divide by zero or
        // produce NaN/negative coordinates.
        let path = histogram_layout(&bins, 0, 256.0, 90.0);
        assert!(!path.contains("NaN"));
        assert!(path.starts_with("M0 90"));
        assert!(path.ends_with("L256 90 Z"));
    }

    #[test]
    fn curve_layout_of_the_identity_curve_is_empty() {
        let (path, markers) = curve_layout(&[], 200.0);
        assert_eq!((path.as_str(), markers.as_slice()), ("", [].as_slice()));
    }

    /// Unwraps the spot list produced by [`place_spot`]/[`undo_last_spot`].
    fn spots(update: Option<(Param, Value)>) -> Vec<SpotRemoval> {
        match update {
            Some((Param::SpotRemoval, Value::SpotRemoval(spots))) => spots,
            other => panic!("expected a spot-removal update, got {other:?}"),
        }
    }

    #[test]
    fn place_spot_maps_both_clicks_through_the_letterbox() {
        // Same 100x100 preview centered in a 200x100 viewport as
        // `drag_maps_through_the_letterbox`: 50 px side bands.
        let placed = spots(place_spot(
            (75.0, 25.0),
            (125.0, 75.0),
            (200.0, 100.0),
            (100.0, 100.0),
            (0.05, 0.5, 1.0),
            &[],
        ));
        assert_eq!(
            placed,
            vec![SpotRemoval {
                source: Point { x: 0.25, y: 0.25 },
                target: Point { x: 0.75, y: 0.75 },
                radius: 0.05,
                feather: 0.5,
                opacity: 1.0,
            }]
        );
    }

    #[test]
    fn place_spot_rejects_a_non_positive_radius() {
        let view = (100.0, 100.0);
        assert_eq!(
            place_spot((10.0, 10.0), (20.0, 20.0), view, view, (0.0, 0.5, 1.0), &[]),
            None
        );
    }

    #[test]
    fn undo_last_spot_pops_the_most_recent_one() {
        let one = SpotRemoval {
            source: Point { x: 0.1, y: 0.1 },
            target: Point { x: 0.2, y: 0.2 },
            radius: 0.05,
            feather: 0.5,
            opacity: 1.0,
        };
        let two = SpotRemoval { radius: 0.1, ..one };
        assert_eq!(
            undo_last_spot(&[one, two]),
            Some((Param::SpotRemoval, Value::SpotRemoval(vec![one])))
        );
        assert_eq!(undo_last_spot(&[]), None);
    }

    #[test]
    fn reset_spots_clears_every_spot() {
        assert_eq!(
            reset_spots(),
            (Param::SpotRemoval, Value::SpotRemoval(Vec::new()))
        );
    }

    #[test]
    fn color_grading_balance_and_blending_merge_into_the_current_grading() {
        let current = Settings {
            color_grading: ColorGrading {
                blending: 40,
                ..ColorGrading::default()
            },
            ..Settings::default()
        };
        assert_eq!(
            action("color-grading-balance", 25.0, &current),
            Some((
                Param::ColorGrading,
                Value::ColorGrading(ColorGrading {
                    balance: 25,
                    blending: 40,
                    ..ColorGrading::default()
                })
            ))
        );
    }

    #[test]
    fn hsl_band_action_merges_one_field_and_leaves_its_siblings() {
        let mut current = Settings::default();
        current.hsl[2] = HslBand {
            hue: 10,
            saturation: 20,
            luminance: 30,
        };
        assert_eq!(
            hsl_band_action(2, "luminance", -15.0, &current),
            Some((
                Param::HslBand(2),
                Value::HslBand(HslBand {
                    hue: 10,
                    saturation: 20,
                    luminance: -15,
                })
            ))
        );
    }

    #[test]
    fn hsl_band_action_rejects_an_out_of_range_index_or_unknown_field() {
        let current = Settings::default();
        assert_eq!(hsl_band_action(8, "hue", 10.0, &current), None);
        assert_eq!(hsl_band_action(0, "bogus", 10.0, &current), None);
    }

    #[test]
    fn color_grading_zone_action_merges_one_field_and_leaves_its_siblings() {
        let mut current = Settings::default();
        current.color_grading.midtones = leyline_sdk::ColorGradingZone {
            hue: 200,
            saturation: 50,
            luminance: 0,
        };
        assert_eq!(
            color_grading_zone_action("midtones", "hue", 90.0, &current),
            Some((
                Param::ColorGrading,
                Value::ColorGrading(ColorGrading {
                    midtones: leyline_sdk::ColorGradingZone {
                        hue: 90,
                        saturation: 50,
                        luminance: 0,
                    },
                    ..ColorGrading::default()
                })
            ))
        );
    }

    #[test]
    fn color_grading_zone_action_rejects_an_unknown_zone_or_field() {
        let current = Settings::default();
        assert_eq!(
            color_grading_zone_action("bogus", "hue", 10.0, &current),
            None
        );
        assert_eq!(
            color_grading_zone_action("shadows", "bogus", 10.0, &current),
            None
        );
    }

    /// ADR 0092: the scan reports both ends whether or not it paints, the
    /// painting only touches the asked-for end, and a clean image reports
    /// nothing and stays untouched.
    #[test]
    fn clipping_is_scanned_always_and_painted_on_request() {
        // One blown pixel, one crushed, one clean.
        let source = [255u8, 200, 100, 0, 0, 0, 128, 128, 128];

        let mut data = source;
        assert_eq!(paint_clipping(&mut data, false, false), (true, true));
        assert_eq!(data, source, "scan alone must not paint");

        let mut data = source;
        assert_eq!(paint_clipping(&mut data, true, false), (true, true));
        assert_eq!(&data[0..3], &[255, 59, 48], "blown pixel painted red");
        assert_eq!(&data[3..6], &[0, 0, 0], "shadows untouched when not asked");

        let mut data = source;
        paint_clipping(&mut data, false, true);
        assert_eq!(&data[3..6], &[10, 132, 255], "crushed pixel painted blue");
        assert_eq!(&data[0..3], &[255, 200, 100], "highlights untouched");

        // All-zero on one channel only is not a crushed shadow, and 254 is
        // not a blown highlight.
        let mut clean = [254u8, 0, 128];
        assert_eq!(paint_clipping(&mut clean, true, true), (false, false));
        assert_eq!(clean, [254, 0, 128]);
    }
}
