use super::*;
use leyline_core::{BackdropProtect, Mask};

const SIZE: u32 = 96;

/// A buffer whose pixel at `(x, y)` is `paint(x, y)`.
fn image(paint: impl Fn(usize, usize) -> [f32; 3]) -> Pixels {
    image_of(SIZE, paint)
}

fn image_of(size: u32, paint: impl Fn(usize, usize) -> [f32; 3]) -> Pixels {
    let mut data = Vec::new();
    for y in 0..size as usize {
        for x in 0..size as usize {
            data.extend_from_slice(&paint(x, y));
        }
    }
    Pixels {
        width: size,
        height: size,
        data,
    }
}

fn at(px: &Pixels, x: usize, y: usize) -> [f32; 3] {
    let i = (y * px.width as usize + x) * 3;
    [px.data[i], px.data[i + 1], px.data[i + 2]]
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
        .into_iter()
        .map(|(x, y)| Point { x, y })
        .collect()
}

/// The left half is the backdrop; the right half is the wall.
fn left_half() -> Backdrop {
    Backdrop {
        regions: vec![BackdropRegion {
            source: rect(0.0, 0.0, 0.5, 1.0),
            zone: BackdropZone::OutsideSource,
            holes: Vec::new(),
        }],
        ..Backdrop::default()
    }
}

const CLOTH: [f32; 3] = [0.2, 0.25, 0.4];
const WALL: [f32; 3] = [0.9, 0.9, 0.85];

fn cloth_and_wall(x: usize, _y: usize) -> [f32; 3] {
    if x < SIZE as usize / 2 { CLOTH } else { WALL }
}

fn run(px: &mut Pixels, settings: &Backdrop) {
    backdrop(px, settings, 0.0, &MaskCoverages::default());
}

fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tolerance)
}

#[test]
fn a_flat_backdrop_replaces_the_wall_by_its_own_colour() {
    let mut px = image(cloth_and_wall);
    run(&mut px, &left_half());
    assert!(close(at(&px, 90, 48), CLOTH, 1e-4), "{:?}", at(&px, 90, 48));
    assert!(
        close(at(&px, 10, 48), CLOTH, 0.0),
        "the source is never written"
    );
}

#[test]
fn a_protected_subject_on_the_wall_is_left_alone() {
    let mut px = image(cloth_and_wall);
    let mut settings = left_half();
    settings.protect = vec![BackdropProtect {
        mask: Mask::Radial {
            cx: 0.8,
            cy: 0.5,
            rx: 0.1,
            ry: 0.1,
            angle: 0.0,
            feather: 0.0,
            inverted: false,
        },
        within: ProtectWithin::Everywhere,
    }];
    run(&mut px, &settings);
    assert!(close(at(&px, 77, 48), WALL, 0.0), "inside the subject");
    assert!(close(at(&px, 92, 10), CLOTH, 1e-4), "away from it");
}

#[test]
fn a_protection_limited_to_the_source_does_not_keep_a_stand_on_the_wall() {
    let mut px = image(cloth_and_wall);
    let mut settings = left_half();
    settings.protect = vec![BackdropProtect {
        mask: Mask::Radial {
            cx: 0.8,
            cy: 0.5,
            rx: 0.1,
            ry: 0.1,
            angle: 0.0,
            feather: 0.0,
            inverted: false,
        },
        within: ProtectWithin::Source,
    }];
    run(&mut px, &settings);
    assert!(close(at(&px, 77, 48), CLOTH, 1e-4), "{:?}", at(&px, 77, 48));
}

#[test]
fn a_release_mask_takes_back_what_the_protection_covered() {
    let mut px = image(cloth_and_wall);
    let mut settings = left_half();
    settings.protect = vec![BackdropProtect {
        mask: Mask::Everything,
        within: ProtectWithin::Everywhere,
    }];
    settings.release = vec![Mask::Everything];
    run(&mut px, &settings);
    assert!(close(at(&px, 90, 48), CLOTH, 1e-4));
}

#[test]
fn a_hole_inside_the_source_is_filled_and_not_measured() {
    let logo = [1.0, 0.0, 0.0];
    let mut px = image(|x, y| {
        if (20..30).contains(&x) && (40..50).contains(&y) {
            logo
        } else {
            cloth_and_wall(x, y)
        }
    });
    let mut settings = left_half();
    settings.regions[0].holes = vec![rect(0.19, 0.4, 0.33, 0.54)];
    run(&mut px, &settings);
    assert!(close(at(&px, 25, 45), CLOTH, 1e-4), "{:?}", at(&px, 25, 45));
    assert!(
        close(at(&px, 90, 48), CLOTH, 1e-4),
        "the logo did not leak into the fill"
    );
}

#[test]
fn the_slow_field_continues_past_the_edge() {
    // Darker towards the right edge of the cloth, as a lit muslin is.
    let ramp = |x: usize, _y: usize| {
        let v = 0.5 - 0.004 * x as f32;
        if x < SIZE as usize / 2 {
            [v, v, v]
        } else {
            WALL
        }
    };
    let mut px = image(ramp);
    run(&mut px, &left_half());
    // The extrapolation mirrors softly: at a distance d from the edge it
    // reads the cloth about d/2 inside it, closer to the edge than to the
    // far side, and never beyond the cloth's own range.
    let (edge, far) = (ramp(47, 0)[0], ramp(0, 0)[0]);
    let filled = at(&px, 60, 48)[0];
    assert!(
        filled > edge && filled - edge < far - filled,
        "{filled} between {edge} and {far}, nearer the edge"
    );
}

/// Large enough for every band of the mottle to exist.
const LARGE: u32 = 384;

/// A mottled source: vertical stripes of varying brightness, like folds.
fn folds(x: usize, _y: usize) -> [f32; 3] {
    if x >= LARGE as usize / 2 {
        return WALL;
    }
    let v = 0.3 + 0.05 * value_noise(x as f32 / 6.0, 0.5, 7);
    [v, v, v * 1.2]
}

#[test]
fn the_fill_carries_a_mottle_oriented_like_the_folds() {
    let mut px = image_of(LARGE, folds);
    run(&mut px, &left_half());
    let (mut across, mut along) = (0.0f32, 0.0f32);
    for y in 20..360 {
        for x in 220..370 {
            across += (at(&px, x + 1, y)[0] - at(&px, x, y)[0]).abs();
            along += (at(&px, x, y + 1)[0] - at(&px, x, y)[0]).abs();
        }
    }
    assert!(across > 0.0, "a mottle was synthesised");
    assert!(across > 1.5 * along, "across {across} vs along {along}");
}

#[test]
fn texture_zero_leaves_only_the_slow_field() {
    let mut px = image_of(LARGE, folds);
    let mut settings = left_half();
    settings.texture = 0;
    run(&mut px, &settings);
    let (a, b) = (at(&px, 320, 30)[0], at(&px, 321, 30)[0]);
    assert!((a - b).abs() < 1e-3, "{a} vs {b}");
}

#[test]
fn the_same_inputs_give_the_same_pixels() {
    let mut first = image_of(LARGE, folds);
    let mut second = image_of(LARGE, folds);
    run(&mut first, &left_half());
    run(&mut second, &left_half());
    assert_eq!(first.data, second.data);
}

#[test]
fn a_region_with_almost_nothing_to_measure_writes_nothing() {
    let mut px = image(cloth_and_wall);
    let before = px.data.clone();
    let mut settings = left_half();
    settings.regions[0].source = rect(0.0, 0.0, 0.02, 0.02);
    run(&mut px, &settings);
    assert_eq!(px.data, before);
}

#[test]
fn a_polygon_is_filled_at_pixel_centres() {
    let mask = rasterize_polygon(&rect(0.25, 0.0, 0.75, 1.0), 8, 4, 0.0);
    let row: Vec<f32> = mask[..8].to_vec();
    assert_eq!(row, vec![0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0]);
}
