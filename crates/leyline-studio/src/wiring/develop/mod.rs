//! Wires `DevelopState`: the edit session, every adjustment, the revision
//! history and the settings clipboard (ADR 0045 §4).
//!
//! Split by what an edit *is* rather than by dialog, since the develop panel
//! is one surface: moving between photos (`session`), changing values
//! (`adjustments`, `curve`, `spot`), and moving through what has already been
//! changed (`history`). `clipboard` carries settings between photos.
//!
//! The rules deciding *what* a slider does live in `crate::develop`, free of
//! any Slint type; these modules only move values across the boundary.

pub(crate) mod adjustments;
pub(crate) mod clipboard;
pub(crate) mod curve;
pub(crate) mod history;
pub(crate) mod session;
pub(crate) mod spot;

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::{App, report_error};
use crate::develop;
use crate::format;
use crate::models::{CURVE_CANVAS_SIZE, dev_model};
use crate::ui::{CurveMarker, DevelopState, StudioWindow};
use leyline_sdk::{AssetId, PreviewKind, VersionId};
use slint::{Global, ModelRc, SharedString, VecModel};

/// Wires every callback the develop panel can raise.
pub(crate) fn wire_develop(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    session::wire_session(app, window);
    adjustments::wire_adjustments(app, window);
    curve::wire_curve(app, window);
    spot::wire_spot(app, window);
    history::wire_history(app, window);
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
    let file = app
        .library
        .preview(asset, PreviewKind::Small)
        .map_err(|e| e.to_string())?;
    let image = slint::Image::load_from_path(&file.path)
        .map_err(|_| format!("cannot load preview {}", file.path.display()))?;
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
    // The develop target just changed (entered develop, or navigated to a
    // neighboring photo): any cached "before" render is for the wrong photo
    // now, and Compare Before/After starts back on "after" each time.
    app.dev_before = None;
    DevelopState::get(window).set_dev_compare(false);
    DevelopState::get(window).set_develop_image_before(slint::Image::default());
    Ok(())
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
