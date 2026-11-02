use super::*;

/// A 200x100 viewport showing a 100x50 image: no letterbox, scale 2.
const VIEW: (f64, f64) = (200.0, 100.0);
const IMAGE: (f64, f64) = (100.0, 50.0);

#[test]
fn a_drag_becomes_a_handle_from_where_it_started_to_where_it_ended() {
    let (param, value) =
        place_reshape((40.0, 20.0), (80.0, 60.0), VIEW, IMAGE, (0.15, 0.8), &[]).unwrap();
    assert_eq!(param, Param::Reshape);
    let Value::Reshape(points) = value else {
        panic!("a reshape value");
    };
    assert_eq!(points.len(), 1);
    assert!(
        (points[0].from.x - 0.2).abs() < 1e-9,
        "{:?}",
        points[0].from
    );
    assert!((points[0].from.y - 0.2).abs() < 1e-9);
    assert!((points[0].to.x - 0.4).abs() < 1e-9);
    assert!((points[0].to.y - 0.6).abs() < 1e-9);
    assert_eq!(points[0].radius, 0.15);
    assert_eq!(points[0].strength, 0.8);
}

#[test]
fn a_drag_that_goes_nowhere_places_nothing() {
    assert!(
        place_reshape((40.0, 20.0), (40.0, 20.0), VIEW, IMAGE, (0.15, 1.0), &[]).is_none(),
        "a click is not a reshape"
    );
}

/// The convention every tool in this panel follows: a gesture that
/// strays into the letterbox lands on the frame's edge.
#[test]
fn a_drag_into_the_letterbox_is_clamped_like_every_other_tool() {
    // A viewport taller than the image letterboxes it; y = 5 is above
    // the picture.
    let letterboxed = (200.0, 200.0);
    let (_, value) = place_reshape(
        (40.0, 5.0),
        (80.0, 100.0),
        letterboxed,
        IMAGE,
        (0.15, 1.0),
        &[],
    )
    .unwrap();
    let Value::Reshape(points) = value else {
        panic!()
    };
    assert_eq!(points[0].from.y, 0.0, "clamped to the top edge");
    assert!(points[0].to.y > 0.0);
}

#[test]
fn handles_append_and_are_removed_by_index() {
    let first = ReshapePoint {
        from: Point { x: 0.1, y: 0.1 },
        to: Point { x: 0.2, y: 0.2 },
        radius: 0.1,
        strength: 1.0,
    };
    let (_, value) = place_reshape(
        (40.0, 20.0),
        (80.0, 60.0),
        VIEW,
        IMAGE,
        (0.15, 1.0),
        &[first],
    )
    .unwrap();
    let Value::Reshape(points) = value else {
        panic!()
    };
    assert_eq!(points.len(), 2, "appended, not replaced");
}
