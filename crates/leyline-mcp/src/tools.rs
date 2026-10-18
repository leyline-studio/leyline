//! What each tool does: one façade call, its arguments read from the
//! agent's JSON and its result written back as JSON or an image.
//!
//! Every function here is synchronous, like the engine; the server runs them
//! on Tokio's blocking pool. A failure is a `String` the agent reads — a
//! refusal names what was wrong, never a bare "error" (the stage capability
//! rule).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use leyline_sdk::{
    AssetId, ExportMetadata, ExportRecipe, ExportRequest, ExportSettings, ExportVerdict, GridQuery,
    ImportOptions, Library, PresetSettings, PreviewKind, Rational, Settings, SettingsGroup,
    VersionId, changed_settings,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::dto::{
    ApplyEditsArgs, ApplyEditsOutcome, AssetArgs, AssetMetadata, ExportArgs, ExportOutcome,
    ExportedEntry, ImportArgs, ImportOutcome, ImportedEntry, PathFailure, RenderPreviewArgs,
    RevisionEntry, SearchArgs, SearchHit, VerifyExportArgs, VerifyExportOutcome, VersionArgs,
    VersionEntry, VersionFailure,
};
use crate::registry::ToolId;

/// The long edge `render_preview` never exceeds (ADR 0154 §4): an agent
/// reasons on a look, and every byte it is sent is context it pays for.
const PREVIEW_MAX_EDGE: u32 = 1024;
const SEARCH_DEFAULT_LIMIT: u32 = 50;
const SEARCH_MAX_LIMIT: u32 = 500;

/// What a tool hands back to the agent.
pub enum Reply {
    Json(Value),
    Jpeg(Vec<u8>),
}

pub fn call(library: &Library, tool: ToolId, arguments: Value) -> Result<Reply, String> {
    match tool {
        ToolId::Search => json(search(library, parse(arguments)?)),
        ToolId::GetMetadata => json(get_metadata(library, parse(arguments)?)),
        ToolId::GetSettings => get_settings(library, parse(arguments)?).map(Reply::Json),
        ToolId::ListVersions => json(list_versions(library, parse(arguments)?)),
        ToolId::History => json(history(library, parse(arguments)?)),
        ToolId::RenderPreview => render_preview(library, parse(arguments)?).map(Reply::Jpeg),
        ToolId::ApplyEdits => json(apply_edits(library, parse(arguments)?)),
        ToolId::Import => json(import(library, parse(arguments)?)),
        ToolId::Export => json(export(library, parse(arguments)?)),
        ToolId::VerifyExport => json(verify_export(library, parse(arguments)?)),
    }
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|e| format!("invalid arguments: {e}"))
}

fn json<T: Serialize>(result: Result<T, String>) -> Result<Reply, String> {
    let value = serde_json::to_value(result?).map_err(|e| e.to_string())?;
    Ok(Reply::Json(value))
}

fn search(library: &Library, args: SearchArgs) -> Result<Vec<SearchHit>, String> {
    let limit = args
        .limit
        .unwrap_or(SEARCH_DEFAULT_LIMIT)
        .min(SEARCH_MAX_LIMIT);
    let capture_range = match (args.captured_from_ms, args.captured_to_ms) {
        (None, None) => None,
        (from, to) => Some((from.unwrap_or(i64::MIN), to.unwrap_or(i64::MAX))),
    };
    let query = GridQuery {
        text: args.text,
        rating_at_least: args.rating_at_least,
        camera: args.camera,
        lens: args.lens,
        capture_range,
        range: 0..limit,
        ..GridQuery::default()
    };
    let items = library.catalog().grid(&query).map_err(|e| e.to_string())?;
    Ok(items
        .into_iter()
        .map(|item| SearchHit {
            version_id: item.version_id.get(),
            asset_id: item.asset_id.get(),
            filename: item.filename,
            captured_at_ms: item.capture_date,
            rating: item.rating,
            pick: lowercase_name(&item.pick),
            edited: item.edited,
            version_count: item.version_count,
        })
        .collect())
}

fn get_metadata(library: &Library, args: AssetArgs) -> Result<AssetMetadata, String> {
    let asset = AssetId::new(args.asset_id);
    let catalog = library.catalog();
    // An unknown asset is the catalog's named error; `None` only means the
    // file carried no EXIF (a PNG, a scan), which is an empty answer.
    let metadata = catalog
        .metadata(asset)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    let description = catalog.description(asset).map_err(|e| e.to_string())?;
    let (title, caption) = description.map_or((None, None), |d| (d.title, d.caption));
    Ok(AssetMetadata {
        camera: metadata
            .camera
            .map(|c| format!("{} {}", c.manufacturer, c.model)),
        lens: metadata
            .lens
            .map(|l| format!("{} {}", l.manufacturer, l.model)),
        iso: metadata.iso,
        shutter_speed: metadata.shutter.and_then(ratio),
        aperture: metadata.aperture.and_then(ratio),
        focal_length: metadata.focal_length.and_then(ratio),
        exposure_bias: metadata.exposure_bias,
        flash: metadata.flash,
        gps_latitude: metadata.gps_latitude,
        gps_longitude: metadata.gps_longitude,
        gps_altitude: metadata.gps_altitude,
        artist: metadata.artist,
        copyright: metadata.copyright,
        title,
        caption,
    })
}

fn ratio(value: Rational) -> Option<f64> {
    (value.denominator != 0).then(|| value.numerator as f64 / value.denominator as f64)
}

fn head_settings(library: &Library, version: VersionId) -> Result<Settings, String> {
    let catalog = library.catalog();
    let head = catalog.version_head(version).map_err(|e| e.to_string())?;
    let row = catalog.revision(head).map_err(|e| e.to_string())?;
    Settings::parse(&row.settings_json).map_err(|e| e.to_string())
}

fn get_settings(library: &Library, args: VersionArgs) -> Result<Value, String> {
    let settings = head_settings(library, VersionId::new(args.version_id))?;
    serde_json::to_value(&settings).map_err(|e| e.to_string())
}

fn list_versions(library: &Library, args: AssetArgs) -> Result<Vec<VersionEntry>, String> {
    let versions = library
        .catalog()
        .versions(AssetId::new(args.asset_id))
        .map_err(|e| e.to_string())?;
    Ok(versions
        .into_iter()
        .map(|info| VersionEntry {
            version_id: info.version.get(),
            name: info.name,
            head_revision_id: info.head.get(),
            rating: info.rating,
            pick: lowercase_name(&info.pick),
            created_at_ms: info.created_at,
        })
        .collect())
}

fn history(library: &Library, args: VersionArgs) -> Result<Vec<RevisionEntry>, String> {
    let catalog = library.catalog();
    let rows = catalog
        .version_history(VersionId::new(args.version_id))
        .map_err(|e| e.to_string())?;
    rows.iter()
        .map(|row| {
            let after = Settings::parse(&row.settings_json).map_err(|e| e.to_string())?;
            let changed = match row.parent {
                Some(parent) => {
                    let parent = catalog.revision(parent).map_err(|e| e.to_string())?;
                    let before =
                        Settings::parse(&parent.settings_json).map_err(|e| e.to_string())?;
                    changed_settings(&before, &after)
                }
                None => Vec::new(),
            };
            Ok(RevisionEntry {
                revision_id: row.revision.get(),
                parent_revision_id: row.parent.map(|p| p.get()),
                created_at_ms: row.created_at,
                from_preset: row.from_preset.map(|p| p.get()),
                changed,
            })
        })
        .collect()
}

fn render_preview(library: &Library, args: RenderPreviewArgs) -> Result<Vec<u8>, String> {
    let long_edge = args
        .long_edge
        .unwrap_or(PREVIEW_MAX_EDGE)
        .clamp(1, PREVIEW_MAX_EDGE);
    let version = VersionId::new(args.version_id);
    let settings = head_settings(library, version)?;
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    // A live render of the version's head: nothing is written, not even a
    // cache row, and any version can be shown — not only the current one.
    let pixels = library
        .preview_live(asset, PreviewKind::Small, &settings)
        .map_err(|e| e.to_string())?;
    let image = image::RgbImage::from_raw(pixels.width(), pixels.height(), pixels.data().to_vec())
        .ok_or("the renderer returned a malformed image")?;
    let image = if image.width().max(image.height()) > long_edge {
        let (width, height) = scaled(image.width(), image.height(), long_edge);
        image::imageops::thumbnail(&image, width, height)
    } else {
        image
    };
    let mut jpeg = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(jpeg)
}

/// `width × height` shrunk so its long edge is `long_edge`, ratio kept.
fn scaled(width: u32, height: u32, long_edge: u32) -> (u32, u32) {
    let long = width.max(height) as f64;
    let shrink = |side: u32| ((side as f64 * long_edge as f64 / long).round() as u32).max(1);
    (shrink(width), shrink(height))
}

/// Every settings group, so a capture holds every field `apply_edits` can
/// change. The match below has no wildcard: a group added to the core fails
/// to compile here until it is listed.
const ALL_GROUPS: [SettingsGroup; 17] = [
    SettingsGroup::WhiteBalance,
    SettingsGroup::Tone,
    SettingsGroup::Presence,
    SettingsGroup::ToneCurve,
    SettingsGroup::ColorMixer,
    SettingsGroup::ColorGrading,
    SettingsGroup::CameraProfile,
    SettingsGroup::CreativeLut,
    SettingsGroup::Effects,
    SettingsGroup::LensCorrection,
    SettingsGroup::Detail,
    SettingsGroup::Rendering,
    SettingsGroup::Geometry,
    SettingsGroup::Reshape,
    SettingsGroup::SpotRemoval,
    SettingsGroup::RedEye,
    SettingsGroup::LocalAdjustments,
];

#[cfg(test)]
fn is_listed(group: SettingsGroup) -> bool {
    match group {
        SettingsGroup::WhiteBalance
        | SettingsGroup::Tone
        | SettingsGroup::Presence
        | SettingsGroup::ToneCurve
        | SettingsGroup::ColorMixer
        | SettingsGroup::ColorGrading
        | SettingsGroup::CameraProfile
        | SettingsGroup::CreativeLut
        | SettingsGroup::Effects
        | SettingsGroup::LensCorrection
        | SettingsGroup::Detail
        | SettingsGroup::Rendering
        | SettingsGroup::Geometry
        | SettingsGroup::Reshape
        | SettingsGroup::SpotRemoval
        | SettingsGroup::RedEye
        | SettingsGroup::LocalAdjustments => ALL_GROUPS.contains(&group),
    }
}

/// Goes through Studio's "paste settings" (`Library::apply_settings`): every
/// field passes `EditSession::set`'s validation, and the version gets one
/// revision, undoable like any other.
fn apply_edits(library: &Library, args: ApplyEditsArgs) -> Result<ApplyEditsOutcome, String> {
    let version = VersionId::new(args.version_id);
    let before = head_settings(library, version)?;
    let captured = library
        .capture_settings(version, &ALL_GROUPS)
        .map_err(|e| e.to_string())?;
    let mut document = serde_json::to_value(&captured).map_err(|e| e.to_string())?;
    let fields = document
        .as_object_mut()
        .ok_or("a captured preset is always an object")?;
    merge_edits(fields, args.settings)?;
    let preset = PresetSettings::parse(&document.to_string()).map_err(|e| e.to_string())?;
    let report = library
        .apply_settings(&preset, &[version], |_, _| ())
        .map_err(|e| e.to_string())?;
    if let Some(failure) = report.failed.first() {
        return Err(failure.reason.clone());
    }
    let after = head_settings(library, version)?;
    let revision = library
        .catalog()
        .version_head(version)
        .map_err(|e| e.to_string())?;
    Ok(ApplyEditsOutcome {
        version_id: args.version_id,
        revision_id: revision.get(),
        changed: changed_settings(&before, &after),
    })
}

/// Lays the agent's edits over a full capture, refusing by name a key the
/// capture does not carry and a removal it cannot express.
fn merge_edits(fields: &mut Map<String, Value>, edits: Map<String, Value>) -> Result<(), String> {
    if edits.is_empty() {
        return Err("no settings to apply".to_owned());
    }
    for (key, value) in edits {
        if key == "schema" || key == "groups" {
            return Err(format!("`{key}` is not a develop setting"));
        }
        let Some(current) = fields.get_mut(&key) else {
            return Err(format!(
                "unknown setting `{key}`; get_settings lists the keys a version has"
            ));
        };
        if value.is_null() {
            // A preset reads `null` as "not included", so a removal would be
            // dropped without a word; refuse it instead.
            return Err(format!("removing `{key}` (null) is not supported yet"));
        }
        merge_value(current, value);
    }
    Ok(())
}

fn merge_value(current: &mut Value, edit: Value) {
    match (current, edit) {
        (Value::Object(current), Value::Object(edit)) => {
            for (key, value) in edit {
                match current.get_mut(&key) {
                    Some(slot) => merge_value(slot, value),
                    None => {
                        current.insert(key, value);
                    }
                }
            }
        }
        (current, edit) => *current = edit,
    }
}

fn import(library: &Library, args: ImportArgs) -> Result<ImportOutcome, String> {
    let options = ImportOptions {
        copy_files: args.copy_files,
        recursive: args.recursive.unwrap_or(true),
        pair_companions: true,
        // The agent asked for no pictures, and a background pass would
        // outlive the call (ADR 0082 §4).
        thumbnails: false,
    };
    let report = library
        .import(&PathBuf::from(&args.source), &options, |_, _| ())
        .map_err(|e| e.to_string())?;
    Ok(ImportOutcome {
        imported: report
            .imported
            .into_iter()
            .map(|file| ImportedEntry {
                asset_id: file.registered.asset.get(),
                version_id: file.registered.version.get(),
                path: file.relative_path,
            })
            .collect(),
        skipped: report
            .skipped
            .into_iter()
            .map(|file| PathFailure {
                path: file.path.display().to_string(),
                reason: file.reason,
            })
            .collect(),
    })
}

fn verify_export(library: &Library, args: VerifyExportArgs) -> Result<VerifyExportOutcome, String> {
    let verdict = library
        .verify_export(Path::new(&args.path))
        .map_err(|e| e.to_string())?;
    let (name, row) = match verdict {
        ExportVerdict::Intact(row) => ("intact", Some(row)),
        ExportVerdict::Modified(row) => ("modified", Some(row)),
        ExportVerdict::Unknown => ("unknown", None),
    };
    Ok(VerifyExportOutcome {
        verdict: name,
        version_id: row.as_ref().and_then(|r| r.version).map(|v| v.get()),
        revision_id: row.as_ref().and_then(|r| r.revision).map(|r| r.get()),
        exported_at: row.as_ref().map(|r| r.exported_at),
        destination: row.map(|r| r.destination),
    })
}

fn export(library: &Library, args: ExportArgs) -> Result<ExportOutcome, String> {
    let settings: ExportSettings = match args.settings {
        Some(mut fields) => {
            // A missing `metadata` reads as `none` in a stored recipe, so an
            // old preset keeps its bytes (ADR 0157 §1); a request made now is
            // a new export, and starts where every new export starts.
            if !fields.contains_key("metadata") {
                let default = serde_json::to_value(ExportSettings::default().metadata)
                    .map_err(|e| e.to_string())?;
                fields.insert("metadata".to_owned(), default);
            }
            serde_json::from_value(Value::Object(fields))
                .map_err(|e| format!("invalid export settings: {e}"))?
        }
        None => ExportSettings::default(),
    };
    let note = (settings.metadata != ExportMetadata::None && !settings.format.carries_metadata())
        .then(|| {
            format!(
                "{} files carry no metadata: the encoder offers no way to write it",
                settings.format.extension()
            )
        });
    let request = ExportRequest {
        versions: args
            .version_ids
            .iter()
            .copied()
            .map(VersionId::new)
            .collect(),
        recipe: ExportRecipe::Adhoc(settings),
        destination_dir: PathBuf::from(args.destination_dir),
        // The engine's own default: an agent has no reason to trade memory
        // for wall time on the user's behalf.
        concurrency: None,
    };
    let report = library
        .export(&request, |_, _| ())
        .map_err(|e| e.to_string())?;
    Ok(ExportOutcome {
        exported: report
            .exported
            .into_iter()
            .map(|file| ExportedEntry {
                version_id: file.version.get(),
                path: file.path.display().to_string(),
            })
            .collect(),
        failed: report
            .failed
            .into_iter()
            .map(|failure| VersionFailure {
                version_id: failure.version.get(),
                reason: failure.reason,
            })
            .collect(),
        note,
    })
}

/// `PickState::Picked` → `"picked"`: the variant's name is already the word.
fn lowercase_name(value: &impl std::fmt::Debug) -> String {
    format!("{value:?}").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_group_is_captured() {
        for group in ALL_GROUPS {
            assert!(is_listed(group));
        }
    }

    #[test]
    fn nested_edits_merge_and_arrays_replace() {
        let mut fields = json!({
            "exposure": 0.0,
            "vignette": {"amount": 0, "midpoint": 50},
            "spot_removal": [1, 2],
        })
        .as_object()
        .cloned()
        .unwrap();
        let edits = json!({"exposure": 0.5, "vignette": {"amount": -20}, "spot_removal": []});
        merge_edits(&mut fields, edits.as_object().cloned().unwrap()).unwrap();
        assert_eq!(
            Value::Object(fields),
            json!({
                "exposure": 0.5,
                "vignette": {"amount": -20, "midpoint": 50},
                "spot_removal": [],
            })
        );
    }

    #[test]
    fn unknown_and_removed_keys_are_refused_by_name() {
        let mut fields = json!({"exposure": 0.0, "crop": null})
            .as_object()
            .cloned()
            .unwrap();
        let unknown = merge_edits(
            &mut fields,
            json!({"brightness": 1}).as_object().cloned().unwrap(),
        );
        assert!(unknown.unwrap_err().contains("`brightness`"));
        let removed = merge_edits(
            &mut fields,
            json!({"crop": null}).as_object().cloned().unwrap(),
        );
        assert!(removed.unwrap_err().contains("`crop`"));
        let schema = merge_edits(
            &mut fields,
            json!({"schema": 2}).as_object().cloned().unwrap(),
        );
        assert!(schema.unwrap_err().contains("`schema`"));
    }

    #[test]
    fn a_shrunk_preview_keeps_its_ratio() {
        assert_eq!(scaled(1024, 683, 512), (512, 342));
        assert_eq!(scaled(683, 1024, 512), (342, 512));
    }
}
