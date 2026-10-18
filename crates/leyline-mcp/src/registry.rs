//! The registry: each stable tool id and the façade call it makes
//! (ADR 0154 §3).
//!
//! An id, once released, is renamed only by adding a new one — a client's
//! saved prompt naming a tool is a contract like any other.

use std::sync::Arc;

use rmcp::handler::server::common::schema_for_type;
use rmcp::model::{JsonObject, Tool, ToolAnnotations};

use crate::dto::{
    ApplyEditsArgs, AssetArgs, ExportArgs, ImportArgs, RenderPreviewArgs, SearchArgs,
    VerifyExportArgs, VersionArgs,
};

/// Every tool of the first surface (§4): verbs whose effect is nothing, a
/// new revision, or new files. Removing, renaming, rating and flagging are
/// absent on purpose — they overwrite, and an agent's overwrite cannot be
/// walked back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolId {
    Search,
    GetMetadata,
    GetSettings,
    ListVersions,
    History,
    RenderPreview,
    ApplyEdits,
    Import,
    Export,
    VerifyExport,
}

impl ToolId {
    pub const ALL: [ToolId; 10] = [
        ToolId::Search,
        ToolId::GetMetadata,
        ToolId::GetSettings,
        ToolId::ListVersions,
        ToolId::History,
        ToolId::RenderPreview,
        ToolId::ApplyEdits,
        ToolId::Import,
        ToolId::Export,
        ToolId::VerifyExport,
    ];

    /// The id a client calls the tool by.
    pub fn name(self) -> &'static str {
        match self {
            ToolId::Search => "search",
            ToolId::GetMetadata => "get_metadata",
            ToolId::GetSettings => "get_settings",
            ToolId::ListVersions => "list_versions",
            ToolId::History => "history",
            ToolId::RenderPreview => "render_preview",
            ToolId::ApplyEdits => "apply_edits",
            ToolId::Import => "import",
            ToolId::Export => "export",
            ToolId::VerifyExport => "verify_export",
        }
    }

    pub fn from_name(name: &str) -> Option<ToolId> {
        ToolId::ALL.into_iter().find(|tool| tool.name() == name)
    }

    /// Whether the tool leaves the library as it found it.
    pub fn is_read_only(self) -> bool {
        !matches!(self, ToolId::ApplyEdits | ToolId::Import | ToolId::Export)
    }

    fn description(self) -> &'static str {
        match self {
            ToolId::Search => {
                "Find photographs in the library. Returns version ids, asset ids and \
                 file names."
            }
            ToolId::GetMetadata => {
                "What the camera recorded for an asset (camera, lens, exposure, GPS) \
                 and the title and caption a person wrote."
            }
            ToolId::GetSettings => {
                "The develop settings of a version's current revision, as a JSON \
                 document. Its keys are the ones apply_edits accepts."
            }
            ToolId::ListVersions => "The versions of an asset: independent lines of edits.",
            ToolId::History => {
                "The revision history of a version, newest first, with the settings \
                 each revision changed."
            }
            ToolId::RenderPreview => {
                "Render a version as a JPEG image (long edge at most 1024 px) to judge \
                 how it looks. Writes nothing."
            }
            ToolId::ApplyEdits => {
                "Change develop settings of one version. Writes exactly one revision, \
                 undoable from Leyline Studio's history. Invalid values are refused by name."
            }
            ToolId::Import => {
                "Import a folder of photographs. By default the files are referenced \
                 where they are, never copied or moved."
            }
            ToolId::Export => {
                "Render versions to image files in a folder. Never touches the \
                 originals or the edits."
            }
            ToolId::VerifyExport => {
                "Say whether an image file is one this library exported: intact, \
                 modified since, or unknown here — and which version and revision \
                 it came from. A matching checksum, not a signature."
            }
        }
    }

    fn input_schema(self) -> Arc<JsonObject> {
        match self {
            ToolId::Search => schema_for_type::<SearchArgs>(),
            ToolId::GetMetadata | ToolId::ListVersions => schema_for_type::<AssetArgs>(),
            ToolId::GetSettings | ToolId::History => schema_for_type::<VersionArgs>(),
            ToolId::RenderPreview => schema_for_type::<RenderPreviewArgs>(),
            ToolId::ApplyEdits => schema_for_type::<ApplyEditsArgs>(),
            ToolId::Import => schema_for_type::<ImportArgs>(),
            ToolId::Export => schema_for_type::<ExportArgs>(),
            ToolId::VerifyExport => schema_for_type::<VerifyExportArgs>(),
        }
    }

    /// The tool as `tools/list` announces it.
    pub fn describe(self) -> Tool {
        let annotations = ToolAnnotations::new()
            .read_only(self.is_read_only())
            // Nothing this surface does overwrites: a revision, an asset or a
            // file is always new.
            .destructive(false)
            .open_world(false);
        Tool::new(self.name(), self.description(), self.input_schema())
            .with_annotations(annotations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_round_trips_through_its_name() {
        for tool in ToolId::ALL {
            assert_eq!(ToolId::from_name(tool.name()), Some(tool));
        }
        assert_eq!(ToolId::from_name("delete_assets"), None);
    }
}
