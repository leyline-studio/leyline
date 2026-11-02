//! The point curve and the parametric curve: hit-testing and editing points,
//! the region splits, and the paths the panel draws.

use super::*;

/// How close a click needs to land to an existing tone-curve point (in unit
/// curve-graph coordinates) to be treated as "on that point" rather than a
/// new one.
pub(super) const CURVE_POINT_RADIUS: f64 = 0.05;

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
pub fn curve_point(
    click: (f64, f64),
    channel: &str,
    current: &ToneCurve,
) -> Option<(Param, Value)> {
    let (x, y) = (click.0.clamp(0.0, 1.0), click.1.clamp(0.0, 1.0));
    let mut points = channel_points(current, channel).to_vec();
    if let Some(i) = points
        .iter()
        .position(|p| (p.x - x).powi(2) + (p.y - y).powi(2) < CURVE_POINT_RADIUS.powi(2))
    {
        points.remove(i);
        if points.len() < 2 {
            points.clear();
        }
        return Some(written(current, channel, points));
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
    points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    Some(written(current, channel, points))
}

/// The points of one of the four curves (ADR 0098): `red`, `green`, `blue`,
/// or the master for anything else — the panel's own selector is the only
/// caller, and an unknown name meaning "master" keeps it total.
pub fn channel_points<'a>(curve: &'a ToneCurve, channel: &str) -> &'a [CurvePoint] {
    match channel {
        "red" => &curve.red,
        "green" => &curve.green,
        "blue" => &curve.blue,
        _ => &curve.points,
    }
}

/// `curve` with one of its four curves replaced — the other three are
/// carried over untouched, so editing the red curve never clears the master.
pub(super) fn written(curve: &ToneCurve, channel: &str, points: Vec<CurvePoint>) -> (Param, Value) {
    let mut curve = curve.clone();
    match channel {
        "red" => curve.red = points,
        "green" => curve.green = points,
        "blue" => curve.blue = points,
        _ => curve.points = points,
    }
    (Param::ToneCurve, Value::ToneCurve(curve))
}

/// Clears the selected channel's curve back to the identity, leaving the
/// other three where they are (ADR 0098 §4).
pub fn reset_curve(channel: &str, current: &ToneCurve) -> (Param, Value) {
    written(current, channel, Vec::new())
}

/// A parametric split moved to `percent` (ADR 0137 §4).
///
/// Read-modify-write on the whole struct, like the four region sliders: the
/// stage takes one value. The clamp is the interface's half of the rule
/// `Settings::validate` enforces — splits are strictly increasing, and the
/// panel must never be able to ask for anything else.
#[must_use]
pub fn split_action(which: &str, percent: i32, current: &Settings) -> Option<(Param, Value)> {
    let mut curve = current.parametric_curve;
    match which {
        "shadow" => curve.shadow_split = percent.clamp(1, curve.midtone_split - 1),
        "midtone" => {
            curve.midtone_split = percent.clamp(curve.shadow_split + 1, curve.highlight_split - 1);
        }
        "highlight" => curve.highlight_split = percent.clamp(curve.midtone_split + 1, 99),
        _ => return None,
    }
    Some((Param::ParametricCurve, Value::ParametricCurve(curve)))
}

/// The parametric curve's polyline in the same `size`-pixel-square canvas
/// space [`curve_layout`] uses, sampled from the table the stage renders
/// through (ADR 0137 §4).
#[must_use]
pub fn parametric_layout(curve: &leyline_sdk::ParametricCurve, size: f64) -> String {
    let (path, _) = curve_layout(&leyline_sdk::parametric_curve_samples(curve, 96), size);
    path
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
