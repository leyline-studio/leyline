//! Wires `GridState`: selection, the loaded window of rows, and the detail
//! panel that follows the focus (ADR 0045 §4).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::app::{App, desired_window, item_at};
use crate::format;
use crate::models::label_color;
use crate::ui::{Cell, DetailState, GridState, LibraryState, StudioWindow, Tr};
use crate::wiring::keywords::keyword_rows;
use leyline_sdk::{PickState, Preview, PreviewKind, VersionId};
use slint::{ComponentHandle, Global, Model, ModelRc, SharedString, VecModel};

/// Fills the side panel when a cell is clicked or reached with the arrows.
pub(crate) fn wire_select(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        // The loupe was turned on or off (ADR 0055 §3). Nothing is opened
        // and nothing is written: a cached preview is read into the view,
        // and dropped on the way out so a photo that has since been edited
        // is not still hanging around in memory. What is *not* cached is
        // rendered by a job rather than inline — see `show_loupe`.
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_loupe_mode_changed(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let selected = GridState::get(&window).get_selected();
            match (
                GridState::get(&window).get_loupe_mode(),
                item_at(&app, selected).map(|item| item.asset_id),
            ) {
                (true, Some(asset)) => show_loupe(&mut app, &window, asset),
                _ => GridState::get(&window).set_loupe_image(slint::Image::default()),
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_select(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // Every other path to `select` (arrow keys, double-click,
            // right-click menu actions) is a single-photo intention: it
            // always replaces whatever was multi-selected, exactly like
            // clicking a cell plainly does.
            {
                let mut app = app.borrow_mut();
                app.multi_selected.clear();
                refresh_multi_selected_cells(&app, &window);
            }
            show_details(&mut app.borrow_mut(), &window, index);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        GridState::get(window).on_cell_clicked(move |index, ctrl, shift| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let focused = GridState::get(&window).get_selected();
            {
                let mut app = app.borrow_mut();
                if ctrl {
                    if let Ok(index) = usize::try_from(index) {
                        // The previously lone focus joins the set it's
                        // about to be toggled within, so a first Ctrl-click
                        // right after a plain click still keeps that photo.
                        if let Ok(focused) = usize::try_from(focused) {
                            app.multi_selected.insert(focused);
                        }
                        if !app.multi_selected.remove(&index) {
                            app.multi_selected.insert(index);
                        }
                    }
                } else if shift {
                    if let (Ok(from), Ok(to)) = (usize::try_from(focused), usize::try_from(index)) {
                        let (from, to) = (from.min(to), from.max(to));
                        app.multi_selected.extend(from..=to);
                    }
                } else {
                    app.multi_selected.clear();
                }
                refresh_multi_selected_cells(&app, &window);
            }
            GridState::get(&window).set_selected(index);
            show_details(&mut app.borrow_mut(), &window, index);
        });
    }
}

/// Re-marks every loaded cell's `multi-selected` flag from
/// `app.multi_selected`, without refetching anything from the catalog —
/// called after every Ctrl/Shift-click.
pub(crate) fn refresh_multi_selected_cells(app: &App, window: &StudioWindow) {
    // How many photographs a batch action would act on, which is what the
    // label under a drag says (ADR 0130 §1). Set from the one place the
    // selection ever changes, so it cannot drift: an empty multi-selection
    // means the lone focused photo, exactly as `selected_indices` reads it.
    let size = if app.multi_selected.len() > 1 {
        app.multi_selected.len()
    } else {
        usize::from(GridState::get(window).get_selected() >= 0)
    };
    GridState::get(window).set_selection_size(i32::try_from(size).unwrap_or(i32::MAX));
    for i in 0..app.cells.row_count() {
        let Some(mut cell) = app.cells.row_data(i) else {
            continue;
        };
        let selected = app.multi_selected.contains(&(app.window_start + i));
        if cell.multi_selected != selected {
            cell.multi_selected = selected;
            app.cells.set_row_data(i, cell);
        }
    }
}

/// Fetches the window of rows serving the current viewport and rebuilds
/// the cell model from it (virtual scrolling: the rest of the grid only
/// exists as the scrollbar's extent).
pub(crate) fn load_window(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let range = desired_window(app.viewport, app.total);
    app.query.range = u32::try_from(range.start).unwrap_or(u32::MAX)
        ..u32::try_from(range.end).unwrap_or(u32::MAX);
    let items = app
        .library
        .catalog()
        .grid(&app.query)
        .map_err(|e| e.to_string())?;

    // Only thumbnails already cached are loaded here, so the window appears
    // instantly; the rest are queued and rendered by the thumbnail timer,
    // visible cells before the overscan rows above them.
    let first_visible = app.viewport.0.saturating_sub(range.start);
    // Asked once per page, not once per cell. Almost always empty — a library
    // with every volume plugged in is the normal case — and when it is, no
    // cell is tested at all.
    let offline = crate::wiring::dialogs::roots::offline_root_ids(app);
    let mut cells = Vec::with_capacity(items.len());
    let mut missing = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let thumbnail = app
            .library
            .cached_preview(item.asset_id, PreviewKind::Thumbnail)
            .ok()
            .flatten()
            .and_then(|file| slint::Image::load_from_path(&file.path).ok());
        if thumbnail.is_none() {
            missing.push(index);
        }
        cells.push(Cell {
            thumbnail: thumbnail.unwrap_or_default(),
            filename: SharedString::from(item.filename.as_str()),
            stars: SharedString::from(format::stars(item.rating)),
            label: label_color(item.color_label),
            has_label: item.color_label.is_some(),
            multi_selected: app.multi_selected.contains(&(range.start + index)),
            flagged: item.pick == PickState::Pick,
            rejected: item.pick == PickState::Reject,
            edited: item.edited,
            paired: item.paired,
            version_count: i32::try_from(item.version_count).unwrap_or(1),
            offline: !offline.is_empty() && offline.contains(&item.root_id),
        });
    }
    let (visible, above): (VecDeque<usize>, VecDeque<usize>) = missing
        .into_iter()
        .partition(|&index| index >= first_visible);
    app.items = items;
    app.window_start = range.start;
    app.pending = visible.into_iter().chain(above).collect();
    app.cells = Rc::new(VecModel::from(cells));

    GridState::get(window).set_cells(ModelRc::from(Rc::clone(&app.cells)));
    GridState::get(window).set_window_start(i32::try_from(range.start).unwrap_or(i32::MAX));
    // Here as well as on every selection change, because a window loaded
    // without one — at startup, or coming back from Develop — would leave
    // the count at its default and a drag would offer "0 photos" while
    // filing one (ADR 0130 §1).
    refresh_multi_selected_cells(app, window);

    // The selection may have just scrolled into the loaded window (arrow
    // navigation past the edge): fill the side panel now that its row exists.
    let selected = GridState::get(window).get_selected();
    if item_at(app, selected).is_some() {
        show_details(app, window, selected);
    }
    Ok(())
}

/// Re-runs the grid query — count plus the visible window — and rebuilds
/// the cell model, keeping the current selection when the same version is
/// still in the loaded window.
pub(crate) fn reload(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let keep: Option<VersionId> =
        item_at(app, GridState::get(window).get_selected()).map(|item| item.version_id);

    app.total = app
        .library
        .catalog()
        .count(&app.query)
        .map_err(|e| e.to_string())?;
    let total = i32::try_from(app.total).unwrap_or(i32::MAX);
    GridState::get(window).set_total_cells(total);
    // An empty grid means two different things, and only the query knows
    // which: nothing imported yet, or criteria matching nothing (ADR 0054 §1).
    GridState::get(window).set_narrowed(app.query.narrows());
    LibraryState::get(window).set_status_line(Tr::get(window).invoke_photo_count(total));
    load_window(app, window)?;

    let selected = keep
        .and_then(|version| app.items.iter().position(|item| item.version_id == version))
        .and_then(|i| i32::try_from(i + app.window_start).ok())
        .unwrap_or(-1);
    GridState::get(window).set_selected(selected);
    if selected >= 0 {
        show_details(app, window, selected);
    }
    Ok(())
}

/// One photo's `Small` preview, ready to display.
///
/// The same size the develop view reads, on purpose: the loupe, the compare
/// view, the survey and develop all share one cache entry, so looking at a
/// photo before working on it costs the render once rather than four times. A
/// preview that cannot be produced comes back empty and says so in the log —
/// looking at a photo is never a reason to interrupt what the user was doing.
pub(crate) fn preview_image(app: &mut App, asset: leyline_sdk::AssetId) -> slint::Image {
    match app
        .library
        .preview(asset, PreviewKind::Small)
        .map_err(|e| e.to_string())
        .and_then(|file| {
            slint::Image::load_from_path(&file.path)
                .map_err(|_| format!("cannot load preview {}", file.path.display()))
        }) {
        Ok(image) => image,
        Err(error) => {
            eprintln!("error: {error}");
            slint::Image::default()
        }
    }
}

/// The size class the loupe displays. Named here rather than spelled at
/// each call site so the loupe's kind and the pump's filter cannot drift
/// apart — a mismatch would leave the loupe permanently waiting for an
/// event that names another kind.
///
/// `Medium` (2048 px) rather than `Small` (1024): the loupe fills the
/// window, and 1024 px upscaled onto a 1920-wide screen is visibly soft —
/// which defeats the point of a view whose whole job is to look closely.
/// The extra render cost is affordable now that it no longer blocks the
/// event loop (`show_loupe`) and that the stage cache absorbs the repeat
/// (ADR 0041 §3).
pub(crate) const LOUPE_KIND: PreviewKind = PreviewKind::Medium;

/// Puts one photo in the loupe (ADR 0055 §3), without ever blocking the
/// event loop.
///
/// The loupe used to call `Library::preview`, which renders the whole
/// pipeline inline when nothing is cached — on the UI thread, so every
/// first look at a photo froze the window for as long as the render took.
/// `preview_state` is the engine's answer to exactly that (`engine-api.md`
/// §11): it hands back whatever can be shown *now* and queues the rest.
///
/// A stale preview is shown rather than withheld: a slightly out-of-date
/// image beats a blank window, and the fresh one replaces it the moment
/// `PreviewReady` arrives.
fn show_loupe(app: &mut App, window: &StudioWindow, asset: leyline_sdk::AssetId) {
    let state = match app.library.preview_state(asset, LOUPE_KIND) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("error: {error}");
            return;
        }
    };
    match state {
        Preview::Ready(path) => {
            app.loupe_pending = None;
            set_loupe_image(window, &path);
        }
        Preview::Stale { path, .. } => {
            // Still pending: the job under way will render the head
            // revision, and the pump swaps it in.
            app.loupe_pending = Some(asset);
            set_loupe_image(window, &path);
        }
        Preview::Generating(_) => {
            // Nothing to show yet. Clearing beats leaving the previous
            // photo on screen, which would read as "this is that photo".
            app.loupe_pending = Some(asset);
            GridState::get(window).set_loupe_image(slint::Image::default());
        }
    }
}

/// Loads a cache file into the loupe, or leaves it blank on failure.
fn set_loupe_image(window: &StudioWindow, path: &std::path::Path) {
    match slint::Image::load_from_path(path) {
        Ok(image) => GridState::get(window).set_loupe_image(image),
        Err(_) => eprintln!("error: cannot load preview {}", path.display()),
    }
}

/// Fills the loupe when the render it was waiting for lands.
pub(crate) fn loupe_preview_ready(
    app: &mut App,
    window: &StudioWindow,
    asset: leyline_sdk::AssetId,
) {
    if app.loupe_pending != Some(asset) || !GridState::get(window).get_loupe_mode() {
        return;
    }
    app.loupe_pending = None;
    if let Ok(Some(file)) = app.library.cached_preview(asset, LOUPE_KIND) {
        set_loupe_image(window, &file.path);
    }
}

/// Reads and formats everything the side panel shows for one grid row.
pub(crate) fn show_details(app: &mut App, window: &StudioWindow, index: i32) {
    let Some(asset) = item_at(app, index).map(|item| item.asset_id) else {
        return;
    };
    // The loupe follows the selection rather than holding one of its own
    // (ADR 0055 §3), so moving through the filmstrip or the arrows changes
    // the photo it shows.
    if GridState::get(window).get_loupe_mode() {
        show_loupe(app, window, asset);
    }
    let details = match app.library.catalog().asset_details(asset) {
        Ok(details) => details,
        Err(error) => {
            eprintln!("error: {error}");
            return;
        }
    };
    match keyword_rows(app, asset) {
        Ok((ids, paths)) => {
            app.keywords = ids;
            DetailState::get(window)
                .set_detail_keywords(ModelRc::from(Rc::new(VecModel::from(paths))));
        }
        Err(error) => eprintln!("error: {error}"),
    }
    // The sidebar tree marks the keywords this photograph carries, so it
    // follows the selection too (ADR 0134 §2). After `app.keywords` above,
    // which is what it reads.
    crate::wiring::keywords::refresh_keyword_panel(app, window);
    let meta = details.metadata.as_ref();
    DetailState::get(window).set_detail_filename(SharedString::from(details.filename.as_str()));
    DetailState::get(window).set_detail_path(SharedString::from(details.relative_path.as_str()));
    DetailState::get(window).set_detail_capture(SharedString::from(
        details
            .capture_date
            .map_or_else(|| "—".to_owned(), format::capture_date),
    ));
    DetailState::get(window).set_detail_dimensions(SharedString::from(format::dimensions(
        details.width,
        details.height,
    )));
    DetailState::get(window)
        .set_detail_file_size(SharedString::from(format::file_size(details.file_size)));
    DetailState::get(window).set_detail_camera(SharedString::from(
        meta.and_then(|m| m.camera.as_ref()).map_or_else(
            || "—".to_owned(),
            |c| format::maker_and_model(&c.manufacturer, &c.model),
        ),
    ));
    DetailState::get(window).set_detail_lens(SharedString::from(
        meta.and_then(|m| m.lens.as_ref()).map_or_else(
            || "—".to_owned(),
            |l| format::maker_and_model(&l.manufacturer, &l.model),
        ),
    ));
    DetailState::get(window).set_detail_exposure(SharedString::from(
        meta.map_or_else(String::new, format::exposure_line),
    ));
    // A paired photo says so, and names the file (ADR 0079 §6): the JPEG
    // left the grid, it did not leave the library, and the one place that
    // can tell the user where it went is here.
    let companion = details
        .companions
        .iter()
        .filter_map(|id| app.library.catalog().asset_details(*id).ok())
        .map(|details| details.filename)
        .collect::<Vec<_>>()
        .join(", ");
    DetailState::get(window).set_detail_companion(SharedString::from(companion));
    // What someone wrote (ADR 0099) — absent until someone has.
    let written = app
        .library
        .catalog()
        .description(asset)
        .ok()
        .flatten()
        .unwrap_or_default();
    let state = DetailState::get(window);
    state.set_written_title(SharedString::from(written.title.unwrap_or_default()));
    state.set_written_caption(SharedString::from(written.caption.unwrap_or_default()));
    state.set_written_creator(SharedString::from(written.creator.unwrap_or_default()));
    state.set_written_copyright(SharedString::from(written.copyright.unwrap_or_default()));
}
