//! The tools' contracts: what an agent sends and what it reads back.
//!
//! These are their own types rather than the catalog's, which carry no serde
//! and should not have to: a tool's JSON is a contract with a client's saved
//! prompts (ADR 0154 §3), and it changes only here, on purpose.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `search`: the grid's filters, the subset an agent can name in words.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchArgs {
    /// Full-text search over file names, keywords, titles and captions.
    pub text: Option<String>,
    /// Minimum star rating, 1 to 5.
    pub rating_at_least: Option<u8>,
    /// Camera name, as `facets` would list it (e.g. "Canon EOS 5D Mark IV").
    pub camera: Option<String>,
    /// Lens name.
    pub lens: Option<String>,
    /// Earliest capture instant, UTC epoch milliseconds, inclusive.
    pub captured_from_ms: Option<i64>,
    /// Latest capture instant, UTC epoch milliseconds, inclusive.
    pub captured_to_ms: Option<i64>,
    /// Maximum number of results (default 50, at most 500).
    pub limit: Option<u32>,
}

/// One photograph in a search result.
#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub version_id: i64,
    pub asset_id: i64,
    pub filename: String,
    /// UTC epoch milliseconds.
    pub captured_at_ms: Option<i64>,
    pub rating: Option<u8>,
    pub pick: String,
    pub edited: bool,
    pub version_count: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetArgs {
    /// Asset id, as `search` returns it.
    pub asset_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VersionArgs {
    /// Version id, as `search` or `list_versions` returns it.
    pub version_id: i64,
}

/// What the camera recorded, and what a person wrote about the photograph.
#[derive(Debug, Default, Serialize)]
pub struct AssetMetadata {
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    /// Seconds.
    pub shutter_speed: Option<f64>,
    /// f-number.
    pub aperture: Option<f64>,
    /// Millimetres.
    pub focal_length: Option<f64>,
    pub exposure_bias: Option<f64>,
    pub flash: Option<bool>,
    pub gps_latitude: Option<f64>,
    pub gps_longitude: Option<f64>,
    pub gps_altitude: Option<f64>,
    pub artist: Option<String>,
    pub copyright: Option<String>,
    pub title: Option<String>,
    pub caption: Option<String>,
}

/// One version of an asset: an independent line of edits on the same file.
#[derive(Debug, Serialize)]
pub struct VersionEntry {
    pub version_id: i64,
    pub name: String,
    pub head_revision_id: i64,
    pub rating: Option<u8>,
    pub pick: String,
    pub created_at_ms: i64,
}

/// One revision of a version's history, newest first.
#[derive(Debug, Serialize)]
pub struct RevisionEntry {
    pub revision_id: i64,
    pub parent_revision_id: Option<i64>,
    pub created_at_ms: i64,
    pub from_preset: Option<i64>,
    /// The program that wrote it — `studio`, `cli` or `mcp:<client>` — or
    /// null when unknown (ADR 0154 §6). A client name is self-declared.
    pub origin: Option<String>,
    /// Settings keys this revision changed relative to its parent.
    pub changed: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderPreviewArgs {
    pub version_id: i64,
    /// Long edge in pixels, at most 1024 (the default).
    pub long_edge: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyEditsArgs {
    pub version_id: i64,
    /// Settings to change, keyed as `get_settings` names them — e.g.
    /// `{"exposure": 0.5, "contrast": 10}`. Nested objects merge into the
    /// current value (`{"vignette": {"amount": -20}}` keeps the other
    /// vignette fields); arrays replace it. Keys not listed here are kept.
    pub settings: Map<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct ApplyEditsOutcome {
    pub version_id: i64,
    /// The revision this call wrote; undo returns to its parent.
    pub revision_id: i64,
    pub changed: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportArgs {
    /// Folder to import, an absolute path on this machine.
    pub source: String,
    /// Copy the files into the library instead of referencing them where
    /// they are (default false: references only, nothing is copied).
    #[serde(default)]
    pub copy_files: bool,
    /// Descend into sub-folders (default true).
    pub recursive: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ImportOutcome {
    pub imported: Vec<ImportedEntry>,
    pub skipped: Vec<PathFailure>,
}

#[derive(Debug, Serialize)]
pub struct ImportedEntry {
    pub asset_id: i64,
    pub version_id: i64,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct PathFailure {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportArgs {
    pub version_ids: Vec<i64>,
    /// Destination folder, an absolute path on this machine.
    pub destination_dir: String,
    /// Export settings: `format` (jpeg, png, tiff, webp, avif), `quality`
    /// (1–100), `max_edge` (pixels), `metadata` (`full`, `without-location`,
    /// `none`: what the file says about the photograph — `full` adds the GPS
    /// position and place names). Omitted fields take their defaults
    /// (full-size JPEG, without location).
    pub settings: Option<Map<String, Value>>,
}

#[derive(Debug, Serialize)]
pub struct ExportOutcome {
    pub exported: Vec<ExportedEntry>,
    pub failed: Vec<VersionFailure>,
    /// Set when the policy asked for metadata the format cannot carry
    /// (ADR 0157 §5), so the agent does not report it as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyExportArgs {
    /// The file to check, an absolute path on this machine.
    pub path: String,
}

/// ADR 0153 §4: `intact`, `modified` or `unknown`, with the export it
/// matched when there is one.
#[derive(Debug, Serialize)]
pub struct VerifyExportOutcome {
    pub verdict: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExportedEntry {
    pub version_id: i64,
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct VersionFailure {
    pub version_id: i64,
    pub reason: String,
}
