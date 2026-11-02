//! Placing retouching marks on the photo: red eye, reshape (liquify) and spot
//! removal, from a click in the viewer.

use super::*;

/// Decodes a red-eye click over the develop preview (ADR 0103): one click
/// places one disk, unlike spot removal's two.
///
/// `radius_feather_darken` comes from the panel's own fields, already in
/// the units [`RedEye`] stores. A non-positive radius yields `None`,
/// matching `Settings::validate`.
pub fn place_red_eye(
    click: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    radius_feather_darken: (f64, f64, f64),
    current: &[RedEye],
) -> Option<(Param, Value)> {
    let (radius, feather, darken) = radius_feather_darken;
    let center = letterbox_unit(click, view, image)?;
    if radius <= 0.0 {
        return None;
    }
    let mut eyes = current.to_vec();
    eyes.push(RedEye {
        center: Point {
            x: center.0,
            y: center.1,
        },
        radius,
        feather,
        darken,
    });
    Some((Param::RedEye, Value::RedEye(eyes)))
}

/// Decodes a reshape drag over the develop preview (ADR 0109): press where
/// the content is, release where it should go.
///
/// A drag that strays into the letterbox is **clamped to the frame's edge**,
/// like every other tool in this panel — pushing content toward the border
/// is a thing people mean to do, and inventing an exception for this one
/// tool would be a rule nobody could guess. A drag of no length yields
/// `None`: that is a click, and a click means nothing here.
pub fn place_reshape(
    press: (f64, f64),
    release: (f64, f64),
    view: (f64, f64),
    image: (f64, f64),
    radius_strength: (f64, f64),
    current: &[ReshapePoint],
) -> Option<(Param, Value)> {
    let (radius, strength) = radius_strength;
    if radius <= 0.0 || strength <= 0.0 {
        return None;
    }
    let from = letterbox_unit(press, view, image)?;
    let to = letterbox_unit(release, view, image)?;
    if (from.0 - to.0).abs() < 1e-6 && (from.1 - to.1).abs() < 1e-6 {
        return None;
    }
    let mut points = current.to_vec();
    points.push(ReshapePoint {
        from: Point {
            x: from.0,
            y: from.1,
        },
        to: Point { x: to.0, y: to.1 },
        radius,
        strength,
    });
    Some((Param::Reshape, Value::Reshape(points)))
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
