//! Web Mercator projection and tile compositing for the GPS map view
//! (`docs/adr/0040-gps-map-view.md`).
//!
//! Deliberately Studio-only: the ADR keeps map rendering out of the
//! CLI/SDK (a visual feature, not a scriptable operation), so this math
//! lives here instead of in the engine — `Library::map_tile`/`map_pins`
//! are the only engine surface it needs.

use image::{ImageBuffer, Rgb, RgbImage};
use leyline_sdk::{Library, MapPin, VersionId};

/// Canvas size used before the layout has reported its own.
///
/// The map canvas is *not* a fixed-size surface: it fills the window, and
/// `panels/map.slint` reports its real pixel size back on every layout
/// change (ADR 0040). These values only cover the instant between opening
/// the map and the first such report, and the smallest sane window.
pub const DEFAULT_WIDTH: u32 = 720;
/// Fallback canvas height, see [`DEFAULT_WIDTH`].
pub const DEFAULT_HEIGHT: u32 = 480;
/// Never composite a canvas smaller than this on either axis. A layout can
/// briefly report 0 (before the first real pass, or a fully collapsed
/// pane), and a zero-sized `ImageBuffer` would divide by zero in the
/// projection below.
pub const MIN_CANVAS: u32 = 16;
/// MBTiles/OSM tile edge length — the size every tile source in practice
/// uses.
const TILE_SIZE: f64 = 256.0;
/// Deepest zoom this renderer supports.
///
/// The Web Mercator grid is `2^zoom` tiles wide, so a zoom of 32 or more
/// overflows the `1u32 << zoom` shift the projection is built on (a panic
/// in debug, a wrapped grid in release). Real tile sources stop around
/// 19–24; the ceiling sits at 24 so the whole tile grid also stays well
/// inside `f64`'s exact-integer range. Zoom values reaching this module
/// come from MBTiles `metadata`, which is a user-supplied file and
/// therefore untrusted — [`normalize_zoom_range`] is what keeps a
/// malformed pack from reaching the shift at all.
pub const MAX_ZOOM: u8 = 24;

/// The world's edge length in pixels at `zoom`, the scale factor the whole
/// Web Mercator projection is expressed in. Saturates at [`MAX_ZOOM`] so
/// no caller can drive the shift out of range.
fn world_size(zoom: u8) -> f64 {
    TILE_SIZE * f64::from(1u32 << zoom.min(MAX_ZOOM))
}

/// Turns a pack's declared `(min_zoom, max_zoom)` into a range this
/// renderer can actually honor: clamped to [`MAX_ZOOM`], and with an
/// inverted range (a pack declaring `minzoom = 12, maxzoom = 3`) treated
/// as absent metadata rather than trusted — `u8::clamp` panics outright
/// when `min > max`, and MBTiles metadata is user input.
pub fn normalize_zoom_range(min_zoom: Option<u8>, max_zoom: Option<u8>) -> (u8, u8) {
    let (min, max) = match (min_zoom, max_zoom) {
        (Some(min), Some(max)) if min > max => (0, MAX_ZOOM),
        (min, max) => (min.unwrap_or(0), max.unwrap_or(19)),
    };
    (min.min(MAX_ZOOM), max.min(MAX_ZOOM))
}

/// The smallest lon/lat rectangle containing a set of pins (ADR 0150 §2).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bounds {
    west: f64,
    east: f64,
    south: f64,
    north: f64,
}

impl Bounds {
    /// `None` for an empty set — there is no rectangle around nothing.
    fn around(pins: &[MapPin]) -> Option<Bounds> {
        let (first, rest) = pins.split_first()?;
        let mut bounds = Bounds {
            west: first.longitude,
            east: first.longitude,
            south: first.latitude,
            north: first.latitude,
        };
        for pin in rest {
            bounds.west = bounds.west.min(pin.longitude);
            bounds.east = bounds.east.max(pin.longitude);
            bounds.south = bounds.south.min(pin.latitude);
            bounds.north = bounds.north.max(pin.latitude);
        }
        Some(bounds)
    }

    fn center_lon(&self) -> f64 {
        (self.west + self.east) / 2.0
    }

    fn center_lat(&self) -> f64 {
        (self.south + self.north) / 2.0
    }

    /// The closest zoom whose viewport still holds the whole rectangle.
    ///
    /// Found by trying each level from `max` down — twenty iterations of a
    /// projection the renderer runs per tile anyway — rather than by
    /// inverting it: the viewport is in pixels, the box is in degrees, and
    /// Mercator makes the relation between them depend on the latitude. A
    /// loop over twenty integers is the obviously-correct version of that.
    fn zoom_that_fits(&self, canvas: (u32, u32), min: u8, max: u8) -> u8 {
        // A layout can report zero before its first real pass; the renderer
        // floors the canvas the same way rather than dividing by it.
        let width = f64::from(canvas.0.max(MIN_CANVAS));
        let height = f64::from(canvas.1.max(MIN_CANVAS));
        for zoom in (min..=max).rev() {
            let (x0, y0) = lonlat_to_pixel(self.west, self.north, zoom);
            let (x1, y1) = lonlat_to_pixel(self.east, self.south, zoom);
            if (x1 - x0).abs() <= width && (y1 - y0).abs() <= height {
                return zoom;
            }
        }
        min
    }
}

/// Pan/zoom state of the map view: purely local UI state, never mirrored
/// to the catalog, reset every time the map is opened.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// Zoom level (0 = whole world in one 256px tile).
    pub zoom: u8,
    /// Longitude of the viewport's center, decimal degrees.
    pub center_lon: f64,
    /// Latitude of the viewport's center, decimal degrees.
    pub center_lat: f64,
}

impl View {
    /// Where the map opens (ADR 0150 §2): on `focus` when the photograph one
    /// was looking at has a position, otherwise framing every pin.
    ///
    /// It used to open on the **mean** of the pins, and a mean is not a
    /// place: measured on a real library, the mean of 9 626 pins scattered
    /// over France and Belgium is in Algeria, at a zoom close enough to show
    /// nothing. Averaging Paris and Brussels gives a point in neither.
    ///
    /// `min`/`max` are the active pack's normalized zoom range
    /// ([`normalize_zoom_range`]): the chosen zoom is clamped into it,
    /// because opening at a level the pack has no tiles for renders a
    /// blank base map with no hint that zooming out would fix it.
    pub fn initial(
        pins: &[MapPin],
        focus: Option<&MapPin>,
        canvas: (u32, u32),
        min: u8,
        max: u8,
    ) -> View {
        // The photograph one came from, when it is on the map at all: the
        // street rather than the region.
        if let Some(pin) = focus {
            return View {
                zoom: 14.clamp(min, max),
                center_lon: pin.longitude,
                center_lat: pin.latitude,
            };
        }
        let Some(bounds) = Bounds::around(pins) else {
            return View {
                zoom: 2.clamp(min, max),
                center_lon: 0.0,
                center_lat: 20.0,
            };
        };
        View {
            zoom: bounds.zoom_that_fits(canvas, min, max),
            center_lon: bounds.center_lon(),
            center_lat: bounds.center_lat(),
        }
    }

    /// Pans by a screen-pixel drag delta: dragging the pointer right moves
    /// the map's visible content right, i.e. the center moves left (west).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (cx, cy) = lonlat_to_pixel(self.center_lon, self.center_lat, self.zoom);
        let (lon, lat) = pixel_to_lonlat(cx - dx, cy - dy, self.zoom);
        self.center_lon = lon;
        // Web Mercator is undefined at the poles; clamp well short of
        // them rather than let a drag walk the center off to infinity.
        self.center_lat = lat.clamp(-85.0, 85.0);
    }

    /// Adjusts the zoom level by `delta`, clamped to `[min, max]` — the
    /// pack's own declared range (`TilePackInfo::min_zoom`/`max_zoom`) when
    /// it has one, a generous default otherwise. Pass a range that has been
    /// through [`normalize_zoom_range`]; this re-clamps it anyway so a
    /// caller that forgets cannot turn malformed pack metadata into a
    /// `clamp` panic.
    pub fn zoom_by(&mut self, delta: i8, min: u8, max: u8) {
        let (min, max) = normalize_zoom_range(Some(min), Some(max));
        let clamped =
            (i16::from(self.zoom) + i16::from(delta)).clamp(i16::from(min), i16::from(max));
        self.zoom = clamped as u8;
    }
}

/// One pin projected into the canvas' own pixel space, ready for the Slint
/// overlay to position directly.
#[derive(Debug, Clone, Copy)]
pub struct ProjectedPin {
    /// X position within the `WIDTH`x`HEIGHT` canvas.
    pub x: f32,
    /// Y position within the `WIDTH`x`HEIGHT` canvas.
    pub y: f32,
    /// The version a click on this pin navigates to.
    pub version_id: VersionId,
}

/// Renders the current viewport at `size`: composites every tile the active
/// map pack covers into one RGB image of exactly that size, and projects
/// every pin into the same canvas space, dropping the ones that fall
/// outside it. Tiles
/// the pack doesn't have (outside its coverage, zoomed past what it
/// contains) are left as flat fill rather than failing the whole render —
/// same best-effort stance as a missing thumbnail elsewhere in the app.
pub fn render(
    library: &Library,
    view: &View,
    pins: &[MapPin],
    size: (u32, u32),
) -> (RgbImage, Vec<ProjectedPin>) {
    let (width, height) = (size.0.max(MIN_CANVAS), size.1.max(MIN_CANVAS));
    let mut canvas: RgbImage = ImageBuffer::from_pixel(width, height, Rgb([222, 220, 214]));
    // Every shift below is driven by this, never by `view.zoom` directly:
    // the view can be handed any `u8`, but the grid only exists up to
    // `MAX_ZOOM`.
    let zoom = view.zoom.min(MAX_ZOOM);
    let (center_x, center_y) = lonlat_to_pixel(view.center_lon, view.center_lat, zoom);
    let origin_x = center_x - f64::from(width) / 2.0;
    let origin_y = center_y - f64::from(height) / 2.0;

    let side = 1i64 << zoom;
    let first_tile_x = (origin_x / TILE_SIZE).floor() as i64;
    let first_tile_y = (origin_y / TILE_SIZE).floor() as i64;
    let last_tile_x = ((origin_x + f64::from(width)) / TILE_SIZE).floor() as i64;
    let last_tile_y = ((origin_y + f64::from(height)) / TILE_SIZE).floor() as i64;

    for tile_y in first_tile_y..=last_tile_y {
        if tile_y < 0 || tile_y >= side {
            continue;
        }
        for tile_x in first_tile_x..=last_tile_x {
            // Longitude wraps around the antimeridian; latitude (rows)
            // never does — out-of-range rows were already skipped above.
            let wrapped_x = tile_x.rem_euclid(side) as u32;
            let Ok(Some(bytes)) = library.map_tile(zoom, wrapped_x, tile_y as u32) else {
                continue;
            };
            let Ok(tile_image) = image::load_from_memory(&bytes) else {
                continue;
            };
            let dest_x = (tile_x as f64 * TILE_SIZE - origin_x).round() as i64;
            let dest_y = (tile_y as f64 * TILE_SIZE - origin_y).round() as i64;
            blit(&mut canvas, &tile_image.to_rgb8(), dest_x, dest_y);
        }
    }

    let pins = pins
        .iter()
        .filter_map(|pin| {
            let (px, py) = lonlat_to_pixel(pin.longitude, pin.latitude, zoom);
            let x = px - origin_x;
            let y = py - origin_y;
            (x >= 0.0 && y >= 0.0 && x <= f64::from(width) && y <= f64::from(height)).then_some(
                ProjectedPin {
                    x: x as f32,
                    y: y as f32,
                    version_id: pin.version_id,
                },
            )
        })
        .collect();

    (canvas, pins)
}

/// Longitude/latitude (WGS84) to Web Mercator pixel coordinates at `zoom`
/// (EPSG:3857 tiling — the convention every XYZ tile source, including
/// MBTiles packs built from OpenStreetMap data, uses).
fn lonlat_to_pixel(lon: f64, lat: f64, zoom: u8) -> (f64, f64) {
    let scale = world_size(zoom);
    let x = (lon + 180.0) / 360.0 * scale;
    let lat_rad = lat.to_radians();
    let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
    (x, y)
}

/// Inverse of [`lonlat_to_pixel`].
fn pixel_to_lonlat(x: f64, y: f64, zoom: u8) -> (f64, f64) {
    let scale = world_size(zoom);
    let lon = x / scale * 360.0 - 180.0;
    let n = std::f64::consts::PI - 2.0 * std::f64::consts::PI * y / scale;
    let lat = n.sinh().atan().to_degrees();
    (lon, lat)
}

/// Converts a composited canvas into a displayable Slint image, the same
/// conversion shape as `rgb8_to_slint_image` in `main.rs` (a different
/// pixel type, `image::RgbImage` here rather than `leyline_sdk::Rgb8`).
pub fn to_slint_image(image: &RgbImage) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
        image.as_raw(),
        image.width(),
        image.height(),
    );
    slint::Image::from_rgb8(buffer)
}

/// Copies `src` onto `dest` at `(x, y)`, clipping whatever falls outside.
fn blit(dest: &mut RgbImage, src: &RgbImage, x: i64, y: i64) {
    for sy in 0..src.height() {
        let dy = y + i64::from(sy);
        if dy < 0 || dy >= i64::from(dest.height()) {
            continue;
        }
        for sx in 0..src.width() {
            let dx = x + i64::from(sx);
            if dx < 0 || dx >= i64::from(dest.width()) {
                continue;
            }
            dest.put_pixel(dx as u32, dy as u32, *src.get_pixel(sx, sy));
        }
    }
}

#[cfg(test)]
mod tests {
    /// A canvas size for the tests that do not care which one.
    const SIZE: (u32, u32) = (DEFAULT_WIDTH, DEFAULT_HEIGHT);

    use super::*;

    #[test]
    fn lonlat_pixel_round_trip_recovers_the_original_coordinate() {
        for (lon, lat) in [(0.0, 0.0), (2.3522, 48.8566), (-122.4194, 37.7749)] {
            let (x, y) = lonlat_to_pixel(lon, lat, 12);
            let (back_lon, back_lat) = pixel_to_lonlat(x, y, 12);
            assert!((lon - back_lon).abs() < 1e-6, "lon: {lon} vs {back_lon}");
            assert!((lat - back_lat).abs() < 1e-6, "lat: {lat} vs {back_lat}");
        }
    }

    #[test]
    fn panning_right_moves_the_center_west() {
        let mut view = View {
            zoom: 10,
            center_lon: 0.0,
            center_lat: 0.0,
        };
        view.pan(100.0, 0.0);
        assert!(view.center_lon < 0.0, "got {}", view.center_lon);
    }

    #[test]
    fn panning_never_pushes_latitude_past_the_mercator_clamp() {
        let mut view = View {
            zoom: 2,
            center_lon: 0.0,
            center_lat: 0.0,
        };
        view.pan(0.0, -1_000_000.0);
        assert!(view.center_lat <= 85.0);
    }

    #[test]
    fn zoom_by_clamps_to_the_given_bounds() {
        let mut view = View {
            zoom: 5,
            center_lon: 0.0,
            center_lat: 0.0,
        };
        view.zoom_by(-10, 2, 18);
        assert_eq!(view.zoom, 2);
        view.zoom_by(100, 2, 18);
        assert_eq!(view.zoom, 18);
    }

    #[test]
    fn a_zoom_past_the_renderer_maximum_saturates_instead_of_overflowing_the_shift() {
        // `1u32 << 32` is undefined; a pack declaring `maxzoom = 255`, or
        // any caller handing over a raw `u8`, must not reach it.
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(&dir.path().join("Library"), "MapViewZoom").unwrap();
        for zoom in [MAX_ZOOM, 31, 32, 64, 255] {
            let view = View {
                zoom,
                center_lon: 2.3522,
                center_lat: 48.8566,
            };
            let (canvas, _) = render(&library, &view, &[], SIZE);
            assert_eq!((canvas.width(), canvas.height()), SIZE);
        }
    }

    #[test]
    fn an_inverted_pack_zoom_range_is_treated_as_absent_metadata() {
        // `u8::clamp` panics when min > max, and MBTiles metadata is a
        // user-supplied file: `minzoom = 12, maxzoom = 3` must normalize,
        // not crash.
        assert_eq!(normalize_zoom_range(Some(12), Some(3)), (0, MAX_ZOOM));
        let mut view = View {
            zoom: 5,
            center_lon: 0.0,
            center_lat: 0.0,
        };
        view.zoom_by(1, 12, 3);
        assert!(view.zoom <= MAX_ZOOM);
    }

    #[test]
    fn a_pack_zoom_range_is_clamped_to_what_the_renderer_supports() {
        assert_eq!(normalize_zoom_range(Some(0), Some(32)), (0, MAX_ZOOM));
        assert_eq!(
            normalize_zoom_range(Some(200), Some(255)),
            (MAX_ZOOM, MAX_ZOOM)
        );
        assert_eq!(normalize_zoom_range(None, None), (0, 19));
        assert_eq!(normalize_zoom_range(Some(3), Some(8)), (3, 8));
    }

    #[test]
    fn the_initial_view_never_opens_deeper_than_the_pack_goes() {
        // A pack that stops at zoom 3 opened on a photograph at zoom 14
        // would render flat fill with no hint that zooming out fixes it.
        let pins = vec![pin(1, 48.8566, 2.3522)];
        assert_eq!(View::initial(&pins, pins.first(), SIZE, 0, 3).zoom, 3);
        assert_eq!(View::initial(&[], None, SIZE, 5, 12).zoom, 5);
        assert_eq!(View::initial(&pins, pins.first(), SIZE, 0, 19).zoom, 14);
    }

    #[test]
    fn initial_view_with_no_pins_is_a_neutral_world_view() {
        let view = View::initial(&[], None, SIZE, 0, 19);
        assert_eq!(view.center_lon, 0.0);
    }

    /// ADR 0150 §2: arriving from a photograph opens on *that* photograph,
    /// not on a point computed from every other one.
    #[test]
    fn initial_view_opens_on_the_photograph_one_came_from() {
        let pins = vec![pin(1, 48.8566, 2.3522), pin(2, -33.8688, 151.2093)];
        let view = View::initial(&pins, pins.last(), SIZE, 0, 19);
        assert_eq!(view.center_lat, -33.8688);
        assert_eq!(view.center_lon, 151.2093);
    }

    /// And without one, it frames them all — where the mean it used to take
    /// would have landed in the Indian Ocean, with neither pin in sight.
    #[test]
    fn initial_view_frames_every_pin_when_it_came_from_none() {
        let pins = vec![pin(1, 10.0, 10.0), pin(2, 20.0, 30.0)];
        let view = View::initial(&pins, None, SIZE, 0, 19);
        assert_eq!(view.center_lat, 15.0);
        assert_eq!(view.center_lon, 20.0);

        // The two are 20° of longitude apart, which no close zoom can hold
        // on a canvas this size — and the zoom chosen is the closest that
        // can.
        let bounds = Bounds::around(&pins).unwrap();
        assert!(view.zoom < 10);
        assert!(bounds.zoom_that_fits(SIZE, 0, 19) == view.zoom);
        let (x0, _) = lonlat_to_pixel(bounds.west, bounds.north, view.zoom);
        let (x1, _) = lonlat_to_pixel(bounds.east, bounds.south, view.zoom);
        assert!(x1 - x0 <= f64::from(SIZE.0));
        // One level closer would not fit: this is the *closest* that does.
        let (tx0, _) = lonlat_to_pixel(bounds.west, bounds.north, view.zoom + 1);
        let (tx1, _) = lonlat_to_pixel(bounds.east, bounds.south, view.zoom + 1);
        assert!(tx1 - tx0 > f64::from(SIZE.0));
    }

    /// A single pin has no extent, so every zoom "fits" it and the closest
    /// wins — which is the right answer and worth pinning down, since the
    /// loop reads from `max` downwards.
    #[test]
    fn a_lone_pin_frames_at_the_closest_zoom_the_pack_has() {
        let pins = vec![pin(1, 48.8566, 2.3522)];
        assert_eq!(View::initial(&pins, None, SIZE, 0, 12).zoom, 12);
    }

    fn pin(id: i64, latitude: f64, longitude: f64) -> MapPin {
        MapPin {
            version_id: VersionId::new(id),
            asset_id: leyline_sdk::AssetId::new(id),
            latitude,
            longitude,
        }
    }

    #[test]
    fn render_produces_a_canvas_of_the_asked_size_even_without_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(&dir.path().join("Library"), "MapView").unwrap();
        let view = View::initial(&[], None, SIZE, 0, 19);

        // The canvas follows the window, so the size is an argument, not a
        // constant: whatever the layout reports is what comes back.
        for size in [SIZE, (1920, 1040), (301, 97)] {
            let (canvas, pins) = render(&library, &view, &[], size);
            assert_eq!((canvas.width(), canvas.height()), size);
            assert!(pins.is_empty());
        }

        // A layout can briefly report zero — before the first real pass, or
        // on a collapsed pane. A zero-sized canvas would divide by zero in
        // the projection, so it is floored instead of trusted.
        let (canvas, _) = render(&library, &view, &[], (0, 0));
        assert_eq!((canvas.width(), canvas.height()), (MIN_CANVAS, MIN_CANVAS));
    }

    #[test]
    fn a_pin_at_the_view_center_projects_to_the_middle_of_the_canvas() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(&dir.path().join("Library"), "MapViewPins").unwrap();
        let view = View {
            zoom: 8,
            center_lon: 2.3522,
            center_lat: 48.8566,
        };
        let pins = vec![MapPin {
            version_id: VersionId::new(1),
            asset_id: leyline_sdk::AssetId::new(1),
            latitude: view.center_lat,
            longitude: view.center_lon,
        }];
        // Centred at any canvas size: the pin lands in the middle of
        // whatever surface the window gave us, not of a fixed 720x480.
        for size in [SIZE, (1920, 1040)] {
            let (_canvas, projected) = render(&library, &view, &pins, size);
            assert_eq!(projected.len(), 1);
            assert!((projected[0].x - size.0 as f32 / 2.0).abs() < 1.0);
            assert!((projected[0].y - size.1 as f32 / 2.0).abs() < 1.0);
        }
    }
}
