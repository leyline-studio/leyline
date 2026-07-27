//! Wires `MapState`: the GPS map view (ADR 0040) (ADR 0045 §4).

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, MapSession, report_error};
use crate::map_view;
use crate::ui::{DevelopState, MapState, StudioWindow};
use crate::wiring::develop::{enter_develop_for, refresh_develop};
use leyline_sdk::{CameraProfile, Library, MapPin, Param, Value};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

/// Wires the GPS map view (`docs/adr/0040-gps-map-view.md`): enter/exit,
/// importing a pack, pan/zoom, and clicking a pin to jump to that photo.
pub(crate) fn wire_map(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_enter_map(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // A failed pin query must not look like "no GPS photos": an
            // empty map is a legitimate state, so a silent fallback to it
            // makes a catalog problem indistinguishable from a normal
            // library.
            let (pins, status) = match app.library.map_pins() {
                Ok(pins) => (pins, None),
                Err(error) => (Vec::new(), Some(error.to_string())),
            };
            app.map = Some(new_map_session(pins, &app.library));
            let unsupported = apply_pack_info(&app, &window);
            MapState::get(&window).set_map_status(SharedString::from(
                status.or(unsupported).unwrap_or_default(),
            ));
            MapState::get(&window).set_map_mode(true);
            render_map(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_exit_map(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            app.borrow_mut().map = None;
            MapState::get(&window).set_map_mode(false);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_browse_camera_profile(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("DCP camera profile", &["dcp"])
                .pick_file()
            else {
                return;
            };
            let referenced = (|| {
                // Importing copies the file into `Profiles/Camera/` and
                // hands back the checksum of the bytes it copied — the
                // reference is never built from a path the user typed.
                let imported = app.library.import_camera_profile(&path)?;
                let mut session = app.library.edit(version)?;
                session.set(
                    Param::CameraProfile,
                    Value::CameraProfile(Some(CameraProfile {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                    })),
                )?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = referenced
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_browse_map_pack(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("MBTiles", &["mbtiles"])
                .pick_file()
            else {
                return;
            };
            let mut app = app.borrow_mut();
            match app.library.import_map_pack(&path) {
                Ok(()) => {
                    let unsupported = apply_pack_info(&app, &window);
                    MapState::get(&window)
                        .set_map_status(SharedString::from(unsupported.unwrap_or_default()));
                    // The new pack brings its own zoom range: re-derive the
                    // whole map state rather than keep a view centered at a
                    // zoom the new pack may not cover.
                    let pins = app
                        .map
                        .as_mut()
                        .map(|state| std::mem::take(&mut state.pins))
                        .unwrap_or_default();
                    app.map = Some(new_map_session(pins, &app.library));
                    render_map(&mut app, &window);
                }
                Err(error) => {
                    MapState::get(&window).set_map_status(SharedString::from(error.to_string()))
                }
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_map_pan(move |dx, dy| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if let Some(state) = &mut app.map {
                state.view.pan(f64::from(dx), f64::from(dy));
            }
            render_map(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_map_zoom_in(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if let Some(state) = &mut app.map {
                state.view.zoom_by(1, state.min_zoom, state.max_zoom);
            }
            render_map(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_map_zoom_out(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if let Some(state) = &mut app.map {
                state.view.zoom_by(-1, state.min_zoom, state.max_zoom);
            }
            render_map(&mut app, &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MapState::get(window).on_map_pin_clicked(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some(state) = &app.map else {
                return;
            };
            let Some(&(asset, version)) = state.visible_pins.get(index as usize) else {
                return;
            };
            app.map = None;
            MapState::get(&window).set_map_mode(false);
            enter_develop_for(&mut app, &window, asset, version);
        });
    }
}

/// Builds a fresh [`MapSession`] for `pins`, centered on them, with the
/// active pack's declared zoom range (a generous default when it declares
/// none, or there is no pack yet).
pub(crate) fn new_map_session(pins: Vec<MapPin>, library: &Library) -> MapSession {
    let info = library.map_pack_info().ok().flatten();
    // MBTiles metadata is whatever the pack's author wrote: normalize it
    // into a range this renderer can honor before anything projects with
    // it (`map_view::normalize_zoom_range`).
    let (min_zoom, max_zoom) = map_view::normalize_zoom_range(
        info.as_ref().and_then(|i| i.min_zoom),
        info.as_ref().and_then(|i| i.max_zoom),
    );
    MapSession {
        view: map_view::View::initial(&pins, min_zoom, max_zoom),
        pins,
        min_zoom,
        max_zoom,
        visible_pins: Vec::new(),
    }
}

/// The message to show for a pack whose tiles this renderer cannot draw,
/// or `None` when the pack is fine.
///
/// ADR 0040 chose pre-rendered *raster* tiles. A vector pack (`format =
/// pbf`, the other common MBTiles flavor) opens, reads and imports without
/// complaint — its tiles simply fail to decode as images, one by one, and
/// the map renders as flat empty fill. Saying so beats an unexplained
/// blank map.
pub(crate) fn unsupported_pack_message(info: Option<&leyline_sdk::TilePackInfo>) -> Option<String> {
    // A pack that declares no format at all is left alone: plenty of
    // real raster packs omit the key, and the tiles either decode or
    // they don't.
    let format = info?.format.as_deref()?.to_ascii_lowercase();
    match format.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => None,
        other => Some(format!(
            "this pack stores \"{other}\" tiles; Leyline displays pre-rendered raster packs \
             (png, jpg, webp) only"
        )),
    }
}

/// Re-queries the map's pins and recomposes the canvas, keeping the
/// current pan/zoom — unlike entering map mode, which re-centers.
pub(crate) fn refresh_map_pins(app: &mut App, window: &StudioWindow) {
    match app.library.map_pins() {
        Ok(pins) => {
            if let Some(state) = &mut app.map {
                state.pins = pins;
            }
            render_map(app, window);
        }
        Err(error) => MapState::get(window).set_map_status(SharedString::from(error.to_string())),
    }
}

/// Mirrors the active pack's presence/attribution into the map panel, and
/// returns the pack's unsupported-format message if it has one so the
/// caller can decide whether it outranks whatever status it was about to
/// show.
pub(crate) fn apply_pack_info(app: &App, window: &StudioWindow) -> Option<String> {
    let info = app.library.map_pack_info().ok().flatten();
    MapState::get(window).set_map_pack_imported(info.is_some());
    let unsupported = unsupported_pack_message(info.as_ref());
    MapState::get(window).set_map_pack_attribution(SharedString::from(
        info.and_then(|i| i.attribution).unwrap_or_default(),
    ));
    unsupported
}

/// Recomposes the map canvas for the current pan/zoom state and updates
/// every map-view Slint property, a no-op outside map mode. Also refreshes
/// `MapSession::visible_pins` so a later `map-pin-clicked(index)` resolves
/// against exactly the pins this call just put on screen.
pub(crate) fn render_map(app: &mut App, window: &StudioWindow) {
    if !MapState::get(window).get_map_pack_imported() {
        return;
    }
    let Some(state) = &app.map else {
        return;
    };
    let (canvas, projected) = map_view::render(&app.library, &state.view, &state.pins);
    MapState::get(window).set_map_image(map_view::to_slint_image(&canvas));
    let markers: Vec<crate::ui::MapPinMarker> = projected
        .iter()
        .map(|pin| crate::ui::MapPinMarker { x: pin.x, y: pin.y })
        .collect();
    MapState::get(window).set_map_pins(ModelRc::from(Rc::new(VecModel::from(markers))));

    let visible_pins = projected
        .iter()
        .filter_map(|pin| {
            state
                .pins
                .iter()
                .find(|p| p.version_id == pin.version_id)
                .map(|p| (p.asset_id, p.version_id))
        })
        .collect();
    if let Some(state) = &mut app.map {
        state.visible_pins = visible_pins;
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn a_vector_tile_pack_is_reported_rather_than_rendered_blank() {
        // ADR 0040 chose raster tiles. A `pbf` pack imports and activates
        // fine, then every tile fails to decode and the map shows flat
        // fill — the user needs to be told why.
        let vector = leyline_sdk::TilePackInfo {
            format: Some("pbf".to_owned()),
            ..Default::default()
        };
        let message = unsupported_pack_message(Some(&vector)).expect("pbf is unsupported");
        assert!(message.contains("pbf"), "{message}");

        for raster in ["png", "jpg", "jpeg", "webp", "PNG"] {
            let info = leyline_sdk::TilePackInfo {
                format: Some(raster.to_owned()),
                ..Default::default()
            };
            assert_eq!(unsupported_pack_message(Some(&info)), None, "{raster}");
        }
        // Plenty of real raster packs declare no format at all; leave them
        // to succeed or fail on their tiles.
        assert_eq!(
            unsupported_pack_message(Some(&leyline_sdk::TilePackInfo::default())),
            None
        );
        assert_eq!(unsupported_pack_message(None), None);
    }
}
