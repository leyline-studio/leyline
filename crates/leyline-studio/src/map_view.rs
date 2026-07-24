//! Web Mercator projection and tile compositing for the GPS map view
//! (`docs/adr/0040-gps-map-view.md`).
//!
//! Deliberately Studio-only: the ADR keeps map rendering out of the
//! CLI/SDK (a visual feature, not a scriptable operation), so this math
//! lives here instead of in the engine — `Library::map_tile`/`map_pins`
//! are the only engine surface it needs.

use image::{ImageBuffer, Rgb, RgbImage};
use leyline_sdk::{Library, MapPin, VersionId};

/// Fixed canvas size, matching the `map-canvas` `Rectangle` in
/// `studio.slint` — the two must stay in step, same as every other
/// fixed-size canvas in this app (develop loupe, histogram).
pub const WIDTH: u32 = 720;
/// Canvas height, see [`WIDTH`].
pub const HEIGHT: u32 = 480;
/// MBTiles/OSM tile edge length — the size every tile source in practice
/// uses.
const TILE_SIZE: f64 = 256.0;

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
    /// Centers on the mean of `pins` at a reasonably close-in zoom, or a
    /// neutral whole-world view when there are no pins yet.
    pub fn initial(pins: &[MapPin]) -> View {
        if pins.is_empty() {
            return View {
                zoom: 2,
                center_lon: 0.0,
                center_lat: 20.0,
            };
        }
        let count = pins.len() as f64;
        let center_lon = pins.iter().map(|p| p.longitude).sum::<f64>() / count;
        let center_lat = pins.iter().map(|p| p.latitude).sum::<f64>() / count;
        View {
            zoom: 10,
            center_lon,
            center_lat,
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
    /// it has one, a generous default otherwise.
    pub fn zoom_by(&mut self, delta: i8, min: u8, max: u8) {
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

/// Renders the current viewport: composites every tile the active map pack
/// covers into one `WIDTH`x`HEIGHT` RGB image, and projects every pin into
/// the same canvas space, dropping the ones that fall outside it. Tiles
/// the pack doesn't have (outside its coverage, zoomed past what it
/// contains) are left as flat fill rather than failing the whole render —
/// same best-effort stance as a missing thumbnail elsewhere in the app.
pub fn render(library: &Library, view: &View, pins: &[MapPin]) -> (RgbImage, Vec<ProjectedPin>) {
    let mut canvas: RgbImage = ImageBuffer::from_pixel(WIDTH, HEIGHT, Rgb([222, 220, 214]));
    let (center_x, center_y) = lonlat_to_pixel(view.center_lon, view.center_lat, view.zoom);
    let origin_x = center_x - f64::from(WIDTH) / 2.0;
    let origin_y = center_y - f64::from(HEIGHT) / 2.0;

    let side = 1i64 << view.zoom;
    let first_tile_x = (origin_x / TILE_SIZE).floor() as i64;
    let first_tile_y = (origin_y / TILE_SIZE).floor() as i64;
    let last_tile_x = ((origin_x + f64::from(WIDTH)) / TILE_SIZE).floor() as i64;
    let last_tile_y = ((origin_y + f64::from(HEIGHT)) / TILE_SIZE).floor() as i64;

    for tile_y in first_tile_y..=last_tile_y {
        if tile_y < 0 || tile_y >= side {
            continue;
        }
        for tile_x in first_tile_x..=last_tile_x {
            // Longitude wraps around the antimeridian; latitude (rows)
            // never does — out-of-range rows were already skipped above.
            let wrapped_x = tile_x.rem_euclid(side) as u32;
            let Ok(Some(bytes)) = library.map_tile(view.zoom, wrapped_x, tile_y as u32) else {
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
            let (px, py) = lonlat_to_pixel(pin.longitude, pin.latitude, view.zoom);
            let x = px - origin_x;
            let y = py - origin_y;
            (x >= 0.0 && y >= 0.0 && x <= f64::from(WIDTH) && y <= f64::from(HEIGHT)).then_some(
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
    let scale = TILE_SIZE * f64::from(1u32 << zoom);
    let x = (lon + 180.0) / 360.0 * scale;
    let lat_rad = lat.to_radians();
    let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
    (x, y)
}

/// Inverse of [`lonlat_to_pixel`].
fn pixel_to_lonlat(x: f64, y: f64, zoom: u8) -> (f64, f64) {
    let scale = TILE_SIZE * f64::from(1u32 << zoom);
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
    fn initial_view_with_no_pins_is_a_neutral_world_view() {
        let view = View::initial(&[]);
        assert_eq!(view.center_lon, 0.0);
    }

    #[test]
    fn initial_view_centers_on_the_mean_of_the_pins() {
        let pins = vec![
            MapPin {
                version_id: VersionId::new(1),
                asset_id: leyline_sdk::AssetId::new(1),
                latitude: 10.0,
                longitude: 10.0,
            },
            MapPin {
                version_id: VersionId::new(2),
                asset_id: leyline_sdk::AssetId::new(2),
                latitude: 20.0,
                longitude: 30.0,
            },
        ];
        let view = View::initial(&pins);
        assert_eq!(view.center_lat, 15.0);
        assert_eq!(view.center_lon, 20.0);
    }

    #[test]
    fn render_produces_a_canvas_of_the_fixed_size_even_without_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::create(&dir.path().join("Library"), "MapView").unwrap();
        let view = View::initial(&[]);
        let (canvas, pins) = render(&library, &view, &[]);
        assert_eq!((canvas.width(), canvas.height()), (WIDTH, HEIGHT));
        assert!(pins.is_empty());
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
        let (_canvas, projected) = render(&library, &view, &pins);
        assert_eq!(projected.len(), 1);
        assert!((projected[0].x - WIDTH as f32 / 2.0).abs() < 1.0);
        assert!((projected[0].y - HEIGHT as f32 / 2.0).abs() < 1.0);
    }
}
