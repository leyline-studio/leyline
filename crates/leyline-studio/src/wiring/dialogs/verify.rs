//! File ▸ Verify an Export… and the photograph's export history
//! (ADR 0153 §6): the journal of §2, read from both ends.
//!
//! The details panel goes from a photograph to the files it became; the
//! dialog goes from a file back to the photograph. Both describe a journal
//! row the same way, which is why they live together.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use leyline_sdk::{AssetId, ExportFormat, ExportRecord, ExportSettings, ExportVerdict};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

use crate::app::App;
use crate::format;
use crate::ui::{DetailState, DialogState, ExportRow, StudioWindow, Tr};

/// The formats Leyline exports, for the file picker's filter.
const EXPORT_EXTENSIONS: [&str; 7] = ["jpg", "jpeg", "png", "tif", "tiff", "webp", "avif"];

/// Fills the details panel's export history for `asset`, newest first.
///
/// A journal that cannot be read empties the section rather than leaving
/// the previous photograph's exports under this one's name.
pub(crate) fn show_export_history(app: &App, window: &StudioWindow, asset: AssetId) {
    let records = app
        .library
        .catalog()
        .export_history(asset)
        .unwrap_or_else(|error| {
            eprintln!("error: {error}");
            Vec::new()
        });
    let tr = Tr::get(window);
    let rows: Vec<ExportRow> = records
        .iter()
        .map(|record| ExportRow {
            when: SharedString::from(format::capture_date(record.exported_at)),
            recipe: SharedString::from(recipe_line(&tr, record)),
            destination: SharedString::from(record.destination.as_str()),
        })
        .collect();
    DetailState::get(window).set_detail_exports(ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// "JPG · quality 90 · 2048 px long edge": the format, then what the
/// resolved recipe says about the pixels, when the row kept it. A row from
/// before migration v14 has no recipe and says its format alone.
fn recipe_line(tr: &Tr, record: &ExportRecord) -> String {
    let mut parts = vec![record.format.to_uppercase()];
    let settings = record
        .export_settings_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<ExportSettings>(json).ok());
    if let Some(settings) = settings {
        if matches!(settings.format, ExportFormat::Jpeg | ExportFormat::Avif) {
            parts.push(
                tr.invoke_export_quality(i32::from(settings.quality))
                    .to_string(),
            );
        }
        parts.push(match settings.max_edge {
            Some(edge) => tr
                .invoke_export_long_edge(i32::try_from(edge).unwrap_or(i32::MAX))
                .to_string(),
            None => tr.invoke_export_full_size().to_string(),
        });
    }
    parts.join(" · ")
}

pub(crate) fn wire_verify(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    let app = Rc::clone(app);
    let handle = window.as_weak();
    DialogState::get(window).on_verify_export(move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &EXPORT_EXTENSIONS)
            .pick_file()
        else {
            return;
        };
        show_verdict(&app.borrow(), &window, &path);
    });
}

/// Asks the library about `path` and opens the dialog on its answer.
fn show_verdict(app: &App, window: &StudioWindow, path: &Path) {
    let state = DialogState::get(window);
    let record = match app.library.verify_export(path) {
        Ok(ExportVerdict::Intact(record)) => {
            state.set_verify_verdict(SharedString::from("intact"));
            Some(record)
        }
        Ok(ExportVerdict::Modified(record)) => {
            state.set_verify_verdict(SharedString::from("modified"));
            Some(record)
        }
        Ok(ExportVerdict::Unknown) => {
            state.set_verify_verdict(SharedString::from("unknown"));
            None
        }
        Err(error) => {
            state.set_verify_verdict(SharedString::from("unreadable"));
            state.set_verify_error(SharedString::from(error.to_string()));
            None
        }
    };
    state.set_verify_file(SharedString::from(path.display().to_string()));
    let tr = Tr::get(window);
    let photo = record
        .as_ref()
        .and_then(|record| app.library.catalog().asset_details(record.asset).ok())
        .map(|details| details.relative_path)
        .unwrap_or_default();
    state.set_verify_photo(SharedString::from(photo));
    state.set_verify_when(SharedString::from(
        record
            .as_ref()
            .map(|record| format::capture_date(record.exported_at))
            .unwrap_or_default(),
    ));
    state.set_verify_recipe(SharedString::from(
        record
            .as_ref()
            .map(|record| recipe_line(&tr, record))
            .unwrap_or_default(),
    ));
    state.set_verify_destination(SharedString::from(
        record
            .as_ref()
            .map(|record| record.destination.clone())
            .unwrap_or_default(),
    ));
    state.set_dialog_result(SharedString::default());
    state.set_dialog(SharedString::from("verify"));
}
