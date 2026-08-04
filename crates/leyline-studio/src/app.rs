//! Application state shared by every wiring module (ADR 0045 §4).
//!
//! `App` is the Rust half of what the UI shows: the open library, the loaded
//! window of grid rows, the edit session and the jobs in flight. It knows
//! nothing of Slint beyond the handles it holds.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::rc::Rc;

use crate::map_view;
use crate::ui::{Cell, LibraryState, StudioWindow, Tr};
use leyline_sdk::{
    AssetId, CollectionId, Event, ExportPreset, FolderId, GridItem, GridQuery, ImportCandidate,
    JobId, KeywordId, Library, MapPin, Preset, PresetSettings, PrintPreset, RevisionRow, Sort,
    VersionId,
};
use slint::{Global, SharedString, VecModel};

/// Sort orders the header button cycles through, with their labels.
pub(crate) const SORTS: [(Sort, &str); 8] = [
    (Sort::CaptureDate { ascending: false }, "capture ↓"),
    (Sort::CaptureDate { ascending: true }, "capture ↑"),
    (Sort::Filename { ascending: true }, "filename A–Z"),
    (Sort::Filename { ascending: false }, "filename Z–A"),
    (Sort::ImportedAt { ascending: false }, "imported ↓"),
    (Sort::ImportedAt { ascending: true }, "imported ↑"),
    (Sort::Rating { ascending: false }, "rating ↓"),
    (Sort::Rating { ascending: true }, "rating ↑"),
];

/// Cells kept loaded beyond each edge of the visible window; a new window
/// is fetched once the viewport gets within half this margin of an edge.
pub(crate) const OVERSCAN: usize = 48;

/// Application state shared by every UI callback.
pub(crate) struct App {
    pub(crate) library: Library,
    pub(crate) query: GridQuery,
    /// Rows of the loaded window, parallel to the grid cell model;
    /// `items[0]` is grid row `window_start` (virtual scrolling).
    pub(crate) items: Vec<GridItem>,
    /// Grid index of the first loaded row.
    pub(crate) window_start: usize,
    /// Total rows matching the query, loaded or not.
    pub(crate) total: u64,
    /// Last viewport reported by the UI: first visible cell, cell capacity.
    pub(crate) viewport: (usize, usize),
    /// Grid indices additionally selected via Ctrl/Shift-click, beyond the
    /// single `selected` focus/anchor the UI tracks itself. Empty means
    /// "just `selected`" — batch actions (rate/label/flag/export) fall back
    /// to the lone focused photo whenever this is empty. Indices scroll out
    /// of the loaded window are simply dropped from any batch action, since
    /// only currently-loaded `items` can resolve to a `VersionId`.
    pub(crate) multi_selected: std::collections::BTreeSet<usize>,
    /// The photo open in the develop view, when in develop mode.
    pub(crate) develop: Option<(AssetId, VersionId)>,
    /// The neutral-settings render of the photo currently open in develop,
    /// fetched once per develop session and reused for every Compare
    /// Before/After toggle — `None` until the first toggle actually needs
    /// it, reset back to `None` whenever the develop target changes.
    pub(crate) dev_before: Option<slint::Image>,
    /// The screen soft proof in effect, if any (ADR 0034): while it is set,
    /// the develop preview is shown *through* this destination profile.
    /// Nothing about it is stored — it is a way of looking, and it lives
    /// exactly as long as the window does.
    pub(crate) soft_proof: Option<leyline_sdk::SoftProof>,
    /// Whether the selected mask's coverage is painted over the develop
    /// preview (ADR 0071). Interface state, never written to a revision.
    pub(crate) show_mask_overlay: bool,
    /// When the last live render of a slider drag started (ADR 0074 §3).
    /// The renders are synchronous, so this is all the throttling needed:
    /// a move arriving too soon is dropped, never queued — what matters is
    /// where the slider *is*, not the path it took to get there.
    pub(crate) last_live_render: Option<std::time::Instant>,
    /// Develop settings copied from one photo (White Balance/Tone/Presence/
    /// Lens Correction/Detail — the same default groups a saved preset
    /// captures, Geometry excluded), waiting to be pasted onto the current
    /// grid selection.
    pub(crate) dev_clipboard: Option<PresetSettings>,
    /// Rows of the develop history panel, parallel to the `dev-history`
    /// display model, sorted by `created_at` ascending — merged across
    /// refreshes rather than replaced by each one, since
    /// `EditSession::history()` only walks *backward* from the current head
    /// (`docs/catalog.md` §16): after jumping back and browsing, a plain
    /// replace would make already-shown rows vanish from the panel just
    /// because the fresh backward walk from the new head doesn't reach them
    /// (they're still real, still reachable by redo — just not *behind* the
    /// new head). Reset to just the fresh fetch when the develop target
    /// itself changes (`dev_history_version`).
    pub(crate) dev_history: Vec<RevisionRow>,
    /// Which version `dev_history` was accumulated for.
    pub(crate) dev_history_version: Option<VersionId>,
    /// Stored export presets, parallel to the dialog's preset chips.
    pub(crate) presets: Vec<ExportPreset>,
    /// Stored print presets (ADR 0036), parallel to the print dialog's
    /// preset chips.
    pub(crate) print_presets: Vec<PrintPreset>,
    /// Stored develop presets, parallel to the develop sidebar's rows.
    pub(crate) dev_presets: Vec<Preset>,
    /// Flattened collection ids, parallel to the sidebar rows.
    pub(crate) collections: Vec<CollectionId>,
    /// Depth of each sidebar row, parallel to `collections` — kept because
    /// the subtree of a row is the run of following rows deeper than it,
    /// which is how a deletion knows what it would take before doing it
    /// (`docs/catalog.md` §24).
    pub(crate) collection_depths: Vec<i32>,
    /// Where the collection being moved may land, parallel to
    /// `CollectionState.move-targets`; `None` is the root.
    pub(crate) move_targets: Vec<Option<CollectionId>>,
    /// Folder ids of the sidebar's folder tree, in the order it lists them
    /// (ADR 0055 §2) — the index a click reports is an index into this.
    pub(crate) folders: Vec<FolderId>,
    /// Grid index of the compare view's *candidate* (ADR 0057 §3), the photo
    /// challenging the selected one. `None` outside compare, and whenever the
    /// grid holds nothing to challenge with.
    pub(crate) compare_candidate: Option<i32>,
    /// Grid indices shown by the survey view, in display order (ADR 0057 §4)
    /// — the index a removal reports is an index into this.
    pub(crate) survey: Vec<usize>,
    /// Keywords of the selected photo, parallel to the panel's rows.
    pub(crate) keywords: Vec<KeywordId>,
    /// The keyword the grid is filtered to, when one is active.
    pub(crate) keyword_filter: Option<KeywordId>,
    /// The removal the confirmation dialog is currently asking about
    /// (ADR 0060): the assets, and whether their files go to the trash.
    /// Held here rather than re-derived on accept because the selection can
    /// change under an open dialog — what the user confirmed is what must
    /// happen, not whatever is selected a moment later.
    pub(crate) pending_removal: Option<(Vec<AssetId>, bool)>,
    /// The asset the loupe is waiting on a render for, if any. The loupe
    /// no longer blocks the event loop to render (ADR 0055 §3): it shows
    /// what exists now and swaps in the fresh file when `PreviewReady`
    /// names this asset. Cleared as soon as it lands, or when the loupe
    /// moves to another photo — a late event for a photo the user has
    /// already left must not overwrite what they are looking at.
    pub(crate) loupe_pending: Option<AssetId>,
    /// The live cell model, so thumbnails can be filled in row by row.
    pub(crate) cells: Rc<VecModel<Cell>>,
    /// Grid rows still waiting for a thumbnail, drained by the event pump.
    pub(crate) pending: VecDeque<usize>,
    /// Engine event stream, polled by the pump timer (Slint is
    /// single-threaded: pull, don't push across threads).
    pub(crate) events: std::sync::mpsc::Receiver<Event>,
    /// The import job the dialog is waiting on, when one runs.
    pub(crate) import_job: Option<JobId>,
    /// The import scan the dialog is waiting on (ADR 0065 §5).
    pub(crate) scan_job: Option<JobId>,
    /// What the last scan found, each line with the tick the user gave it.
    /// Empty until a scan runs: the dialog then still means "import this
    /// whole folder", exactly as it did before.
    pub(crate) candidates: Vec<(ImportCandidate, bool)>,
    /// The folder those candidates came from — what an import of a chosen
    /// list needs to place the files under `Photos/`.
    pub(crate) candidate_source: std::path::PathBuf,
    /// The export job the dialog is waiting on, when one runs.
    pub(crate) export_job: Option<JobId>,
    /// The print job the dialog is waiting on, when one runs (ADR 0036).
    pub(crate) print_job: Option<JobId>,
    /// Thumbnail render jobs currently in flight.
    pub(crate) preview_jobs: HashSet<JobId>,
    /// Whether a tether session (`docs/adr/0038`) is currently open.
    pub(crate) tether_connected: bool,
    /// Shots captured by the current tether session, for the panel's counter.
    pub(crate) tether_captured: u32,
    /// Whether a watched-folder session (`docs/adr/0039`) is currently
    /// active.
    pub(crate) watch_active: bool,
    /// Files imported by the current watch session, for the panel's
    /// counter.
    pub(crate) watch_imported: u32,
    /// GPS map view state (`docs/adr/0040-gps-map-view.md`), `None` outside
    /// map mode — reset (pins re-fetched, view re-centered) every time the
    /// map is entered.
    pub(crate) map: Option<MapSession>,
}

/// Live state of the GPS map view while it's open.
///
/// Named for the session rather than the state because `MapState` is now the
/// Slint global carrying what the map view shows (ADR 0045 §1); this struct is
/// the Rust-side half, and none of it crosses to the UI as-is.
pub(crate) struct MapSession {
    pub(crate) view: map_view::View,
    /// Fetched once on entry, not re-queried per pan/zoom — a GPS tag never
    /// changes while the map is open.
    pub(crate) pins: Vec<MapPin>,
    /// The active pack's declared zoom range (`docs/adr/0040`), clamping
    /// `map-zoom-in`/`map-zoom-out`.
    pub(crate) min_zoom: u8,
    pub(crate) max_zoom: u8,
    /// The pins actually on screen after the last `render_map`, in the
    /// same order as the `map-pins` Slint model — what `map-pin-clicked`'s
    /// index resolves against.
    pub(crate) visible_pins: Vec<(AssetId, VersionId)>,
    /// Canvas size in pixels, as the layout last reported it. The map fills
    /// the window, so this is the size tiles are composited at — it stays
    /// at the fallback only until the first layout pass.
    pub(crate) canvas: (u32, u32),
}

/// Thumbnail render jobs kept in flight at once: enough to hide latency,
/// few enough to leave the catalog responsive for the UI thread.
pub(crate) const MAX_PREVIEW_JOBS: usize = 3;

pub(crate) fn report_error(window: &StudioWindow, message: &str) {
    eprintln!("error: {message}");
    LibraryState::get(window)
        .set_status_line(Tr::get(window).invoke_error_prefix(SharedString::from(message)));
}

/// The window worth loading for a viewport: the visible cells plus the
/// overscan margin on each side, clamped to the grid.
pub(crate) fn desired_window(viewport: (usize, usize), total: u64) -> std::ops::Range<usize> {
    let (first, capacity) = viewport;
    let total = usize::try_from(total).unwrap_or(usize::MAX);
    let start = first.saturating_sub(OVERSCAN).min(total);
    let end = first
        .saturating_add(capacity)
        .saturating_add(OVERSCAN)
        .min(total);
    start..end.max(start)
}

/// True when the loaded window no longer serves the viewport: part of the
/// visible range is missing, or an edge with more rows beyond it is closer
/// than half the overscan margin.
pub(crate) fn window_is_stale(
    viewport: (usize, usize),
    loaded: &std::ops::Range<usize>,
    total: u64,
) -> bool {
    let (first, capacity) = viewport;
    let total = usize::try_from(total).unwrap_or(usize::MAX);
    let visible_end = first.saturating_add(capacity).min(total);
    first < loaded.start
        || visible_end > loaded.end
        || (loaded.start > 0 && first < loaded.start + OVERSCAN / 2)
        || (loaded.end < total && visible_end + OVERSCAN / 2 > loaded.end)
}

/// The grid item shown at a whole-grid index, when it is loaded.
pub(crate) fn item_at(app: &App, index: i32) -> Option<&GridItem> {
    usize::try_from(index)
        .ok()?
        .checked_sub(app.window_start)
        .and_then(|i| app.items.get(i))
}

/// The whole-grid indices a batch action should act on: `multi_selected`
/// when it holds more than the lone `focused` index, otherwise just
/// `focused` alone — so every existing single-photo call site keeps working
/// unchanged when nothing is multi-selected.
pub(crate) fn selected_indices(app: &App, focused: i32) -> Vec<usize> {
    if app.multi_selected.len() > 1 {
        return app.multi_selected.iter().copied().collect();
    }
    match usize::try_from(focused) {
        Ok(index) => vec![index],
        Err(_) => Vec::new(),
    }
}

/// The version ids of [`selected_indices`], silently dropping any index that
/// has since scrolled out of the loaded window (see [`App::multi_selected`]).
pub(crate) fn selected_versions(app: &App, focused: i32) -> Vec<VersionId> {
    selected_indices(app, focused)
        .into_iter()
        .filter_map(|index| {
            let signed = i32::try_from(index).ok()?;
            item_at(app, signed).map(|item| item.version_id)
        })
        .collect()
}

/// The asset ids of [`selected_indices`], deduplicated: several versions of
/// one photo occupy several grid cells but name a single asset, and ADR 0060
/// removes *assets*, so a multi-selection spanning two versions of the same
/// photo must not ask the engine to remove it twice.
pub(crate) fn selected_assets(app: &App, focused: i32) -> Vec<leyline_sdk::AssetId> {
    let mut assets = Vec::new();
    for index in selected_indices(app, focused) {
        let Ok(signed) = i32::try_from(index) else {
            continue;
        };
        if let Some(item) = item_at(app, signed)
            && !assets.contains(&item.asset_id)
        {
            assets.push(item.asset_id);
        }
    }
    assets
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn desired_window_pads_the_viewport_and_clamps_to_the_grid() {
        assert_eq!(desired_window((0, 30), 10_000), 0..30 + OVERSCAN);
        assert_eq!(
            desired_window((500, 30), 10_000),
            500 - OVERSCAN..530 + OVERSCAN
        );
        assert_eq!(desired_window((980, 30), 1_000), 980 - OVERSCAN..1_000);
        assert_eq!(desired_window((0, 30), 10), 0..10);
    }

    #[test]
    fn window_goes_stale_near_an_edge_with_rows_beyond_it() {
        let loaded = 452..578; // desired_window((500, 30), 10_000)
        assert!(!window_is_stale((500, 30), &loaded, 10_000));
        // Drifting towards an edge crosses the half-overscan threshold.
        assert!(window_is_stale((460, 30), &loaded, 10_000));
        assert!(window_is_stale((530, 30), &loaded, 10_000));
        // A jump lands entirely outside the loaded window.
        assert!(window_is_stale((2_000, 30), &loaded, 10_000));
        // At the ends of the grid the margin has nothing left to fetch.
        assert!(!window_is_stale((0, 30), &(0..126), 10_000));
        assert!(!window_is_stale((970, 30), &(922..1_000), 1_000));
    }
}
