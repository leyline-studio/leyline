//! Wires `DevelopState`: the edit session, every adjustment, the revision
//! history and the settings clipboard (ADR 0045 §4).
//!
//! Split by what an edit *is* rather than by dialog, since the develop panel
//! is one surface: moving between photos (`session`), changing values
//! (`adjustments`, `curve`, `spot`), and moving through what has already been
//! changed (`history`). `clipboard` carries settings between photos, and
//! `masks` the local adjustments, whose entries are traced on the image rather
//! than moved with a slider (ADR 0049).
//!
//! The rules deciding *what* a slider does live in `crate::develop`, free of
//! any Slint type; these modules only move values across the boundary.

pub(crate) mod adjustments;
pub(crate) mod clipboard;
pub(crate) mod curve;
pub(crate) mod history;
pub(crate) mod masks;
pub(crate) mod profiles;
pub(crate) mod proof;
pub(crate) mod session;
pub(crate) mod spot;
pub(crate) mod wb;

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, report_error};
use crate::develop;
use crate::format;
use crate::models::{CURVE_CANVAS_SIZE, dev_model};
use crate::ui::{CurveMarker, DevelopState, MaskState, StudioWindow};
use leyline_sdk::{AssetId, PreviewKind, VersionId};
use slint::{Global, ModelRc, SharedString, VecModel};

/// Wires every callback the develop panel can raise.
pub(crate) fn wire_develop(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    session::wire_session(app, window);
    adjustments::wire_adjustments(app, window);
    curve::wire_curve(app, window);
    spot::wire_spot(app, window);
    wb::wire_wb(app, window);
    masks::wire_masks(app, window);
    profiles::wire_profiles(app, window);
    proof::wire_proof(app, window);
    history::wire_history(app, window);
}

/// How often a drag may repaint the preview (ADR 0074 §3): 25 images per
/// second at most, the limit being the eye rather than the machine.
const LIVE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(40);

/// Shows what a value *would* do, without writing anything (ADR 0074).
///
/// `action` maps the control's raw value onto a `(Param, Value)` against the
/// session's current settings — the very same closure its committing twin
/// uses, so the two can never drift apart on what a slider means.
///
/// Everything here is deliberately silent:
///
/// * **nothing is committed.** The session is set in memory (`engine-api.md`
///   §10.1) and dropped without `commit`, so a drag across a slider creates
///   no revision and touches no preview cache;
/// * **nothing is reported.** A failure during a drag would raise a dialog on
///   every mouse move; the committing edit that follows on release reports
///   for real. A live frame that cannot be rendered is simply not shown, and
///   the previous one stays on screen;
/// * **nothing is shown while the mask overlay is on.** The overlay is
///   composited by [`refresh_develop`] from a second render, so a live frame
///   would drop it for the duration of the drag — a change the user did not
///   ask for. Better one honest repaint on release than a flicker.
pub(super) fn live_preview(
    app: &mut App,
    window: &StudioWindow,
    action: impl FnOnce(&leyline_sdk::Settings) -> Option<(leyline_sdk::Param, leyline_sdk::Value)>,
) {
    let Some((asset, version)) = app.develop else {
        return;
    };
    if app.soft_proof.is_some()
        || (app.show_mask_overlay && MaskState::get(window).get_selected_mask() >= 0)
    {
        return;
    }
    let now = std::time::Instant::now();
    if app
        .last_live_render
        .is_some_and(|last| now.duration_since(last) < LIVE_INTERVAL)
    {
        return;
    }
    app.last_live_render = Some(now);

    let shown = (|| {
        let mut session = app.library.edit(version)?;
        let Some((param, value)) = action(session.settings()) else {
            return Ok(None);
        };
        session.set(param, value)?;
        let settings = session.settings().clone();
        // Dropped without committing: that is the whole point.
        drop(session);
        app.library
            .preview_live(asset, PreviewKind::Small, &settings)
            .map(Some)
    })();
    if let Ok(Some(image)) = shown {
        DevelopState::get(window).set_develop_image(crate::models::rgb8_to_slint_image(&image));
    }
}

pub(crate) fn refresh_develop(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let Some((asset, version)) = app.develop else {
        return Ok(());
    };
    let (settings, history) = {
        let session = app.library.edit(version).map_err(|e| e.to_string())?;
        let history = session.history().map_err(|e| e.to_string())?;
        (session.settings().clone(), history)
    };
    DevelopState::get(window).set_dev(dev_model(&settings));
    // Only the file name: the panel has no room for `Profiles/Camera/…`,
    // and that prefix is the same for every imported profile anyway.
    DevelopState::get(window).set_camera_profile_name(SharedString::from(
        settings
            .camera_profile
            .as_ref()
            .map_or("", |profile| {
                profile
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(profile.path.as_str())
            })
            .to_owned(),
    ));
    DevelopState::get(window).set_lut_name(SharedString::from(
        settings
            .lut
            .as_ref()
            .map_or("", |lut| lut.path.rsplit('/').next().unwrap_or(&lut.path))
            .to_owned(),
    ));
    if app.dev_history_version != Some(version) {
        app.dev_history.clear();
        app.dev_history_version = Some(version);
    }
    for row in history.iter() {
        if !app
            .dev_history
            .iter()
            .any(|existing| existing.revision == row.revision)
        {
            app.dev_history.push(row.clone());
        }
    }
    app.dev_history.sort_by_key(|row| row.created_at);
    let current = history.first().map(|row| row.revision);
    let rows: Vec<SharedString> = app
        .dev_history
        .iter()
        .map(|row| SharedString::from(format::capture_date(row.created_at)))
        .collect();
    DevelopState::get(window).set_dev_history(ModelRc::from(Rc::new(VecModel::from(rows))));
    DevelopState::get(window).set_dev_history_current(
        i32::try_from(
            app.dev_history
                .iter()
                .position(|row| Some(row.revision) == current)
                .unwrap_or(0),
        )
        .unwrap_or(0),
    );
    let (path, markers) = develop::curve_layout(&settings.tone_curve.points, CURVE_CANVAS_SIZE);
    DevelopState::get(window).set_dev_curve_path(SharedString::from(path));
    DevelopState::get(window).set_dev_curve_points(ModelRc::from(Rc::new(VecModel::from(
        markers
            .into_iter()
            .map(|(x, y)| CurveMarker {
                x: x as f32,
                y: y as f32,
            })
            .collect::<Vec<_>>(),
    ))));
    DevelopState::get(window)
        .set_dev_spot_count(i32::try_from(settings.spot_removal.len()).unwrap_or(i32::MAX));
    // Local adjustments (ADR 0049): the whole list, plus a selection kept
    // inside it — a removal, an undo or a jump through the history can shorten
    // the list under a row that was selected a moment ago.
    let rows = crate::models::mask_rows(&settings);
    let selected = MaskState::get(window).get_selected_mask();
    if usize::try_from(selected).is_ok_and(|row| row >= rows.len()) {
        MaskState::get(window).set_selected_mask(-1);
    }
    MaskState::get(window).set_masks(ModelRc::from(Rc::new(VecModel::from(rows))));
    // With a proof in effect the view shows the photo *through* a destination
    // profile (ADR 0034): the same preview, transformed in memory, never
    // cached — so leaving the proof shows the real render again with nothing
    // to invalidate.
    let image = match &app.soft_proof {
        Some(proof) => {
            let proofed = app
                .library
                .preview_soft_proofed(asset, PreviewKind::Small, proof)
                .map_err(|e| e.to_string())?;
            crate::models::rgb8_to_slint_image(&proofed)
        }
        None => {
            let file = app
                .library
                .preview(asset, PreviewKind::Small)
                .map_err(|e| e.to_string())?;
            slint::Image::load_from_path(&file.path)
                .map_err(|_| format!("cannot load preview {}", file.path.display()))?
        }
    };
    // The mask overlay (ADR 0071): the selected entry's coverage, painted
    // red over the preview. Off when nothing is selected — there is no "the
    // mask" then — and silently skipped if the engine cannot render it, since
    // a diagnostic view must never take the develop panel down with it.
    let image = match overlay_for(app, asset, selected) {
        Some(painted) => painted,
        None => image,
    };
    DevelopState::get(window).set_develop_image(image);
    if let Ok(bins) = app.library.histogram(asset, PreviewKind::Small) {
        const CANVAS: (f64, f64) = (256.0, 90.0);
        let scale_max = bins.iter().flatten().copied().max().unwrap_or(0);
        DevelopState::get(window).set_dev_histogram_r(SharedString::from(
            develop::histogram_layout(&bins[0], scale_max, CANVAS.0, CANVAS.1),
        ));
        DevelopState::get(window).set_dev_histogram_g(SharedString::from(
            develop::histogram_layout(&bins[1], scale_max, CANVAS.0, CANVAS.1),
        ));
        DevelopState::get(window).set_dev_histogram_b(SharedString::from(
            develop::histogram_layout(&bins[2], scale_max, CANVAS.0, CANVAS.1),
        ));
    }
    show_capture_info(app, window, asset);
    // The develop target just changed (entered develop, or navigated to a
    // neighboring photo): any cached "before" render is for the wrong photo
    // now, and Compare Before/After starts back on "after" each time.
    app.dev_before = None;
    DevelopState::get(window).set_dev_compare(false);
    DevelopState::get(window).set_develop_image_before(slint::Image::default());
    Ok(())
}

/// Fills the four capture values shown under the histogram — sensitivity,
/// focal length, aperture, time (ADR 0054 §2).
///
/// They describe the *file*, not the revision, so they come from the catalog
/// rather than the edit session, and a photo whose file recorded none of them
/// leaves all four empty: the panel then shows no row at all rather than a
/// line of dashes. A read failure is treated the same way — the capture strip
/// is an aid, never a reason to fail refreshing the view.
fn show_capture_info(app: &mut App, window: &StudioWindow, asset: AssetId) {
    let meta = app
        .library
        .catalog()
        .asset_details(asset)
        .ok()
        .and_then(|details| details.metadata);
    let state = DevelopState::get(window);
    state.set_dev_iso(SharedString::from(
        meta.as_ref()
            .and_then(|m| m.iso)
            .map_or_else(String::new, format::iso),
    ));
    state.set_dev_focal(SharedString::from(
        meta.as_ref()
            .and_then(|m| m.focal_length)
            .map_or_else(String::new, format::focal),
    ));
    state.set_dev_aperture(SharedString::from(
        meta.as_ref()
            .and_then(|m| m.aperture)
            .map_or_else(String::new, format::aperture),
    ));
    state.set_dev_shutter(SharedString::from(
        meta.as_ref()
            .and_then(|m| m.shutter)
            .map_or_else(String::new, format::shutter),
    ));
}

/// Opens develop mode for an explicit `(asset, version)` pair rather than
/// the grid's current selection — `on_enter_develop`'s equivalent for a
/// jump that didn't come from clicking a grid cell (a map pin here; the
/// history panel's `checkout_history_row` has its own similar direct-jump
/// need but for a revision within the version already open).
pub(crate) fn enter_develop_for(
    app: &mut App,
    window: &StudioWindow,
    asset: AssetId,
    version: VersionId,
) {
    let filename = app
        .library
        .catalog()
        .asset_details(asset)
        .map(|details| details.filename)
        .unwrap_or_default();
    app.develop = Some((asset, version));
    match refresh_develop(app, window) {
        Ok(()) => {
            DevelopState::get(window).set_develop_filename(SharedString::from(filename.as_str()));
            DevelopState::get(window).set_develop_mode(true);
        }
        Err(error) => {
            app.develop = None;
            report_error(window, &error);
        }
    }
}

/// The develop preview with the selected mask's coverage painted over it, or
/// `None` when there is nothing to paint (ADR 0071 §4).
///
/// Best-effort by design: an unreadable coverage returns `None` and the panel
/// shows the plain preview. The overlay is a way of *looking* at a mask, and
/// nothing about looking should be able to break editing.
fn overlay_for(app: &App, asset: leyline_sdk::AssetId, selected: i32) -> Option<slint::Image> {
    if !app.show_mask_overlay {
        return None;
    }
    let index = usize::try_from(selected).ok()?;
    let coverage = app
        .library
        .mask_coverage_preview(asset, PreviewKind::Small, index)
        .ok()?;
    let preview = app.library.preview(asset, PreviewKind::Small).ok()?;
    let mut base = image::open(&preview.path).ok()?.to_rgb8();
    let size = (base.width(), base.height());
    crate::masks::paint_overlay(
        base.as_mut(),
        size,
        coverage.data(),
        (coverage.width(), coverage.height()),
    )
    .then(|| {
        let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(size.0, size.1);
        buffer.make_mut_bytes().copy_from_slice(base.as_raw());
        slint::Image::from_rgb8(buffer)
    })
}
