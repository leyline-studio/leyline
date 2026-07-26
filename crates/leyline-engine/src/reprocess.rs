//! Batch reprocessing to the current stage versions
//! (`docs/engine-api.md` §10.4, `docs/pipeline.md` §4.5).
//!
//! Migrating a version never rewrites its history: a fresh session per
//! version, one `EditSession::reprocess` call, same as a preset application
//! (`docs/adr/0014-develop-presets.md`) reuses `set`/`commit` rather than
//! inventing a new write path.

use leyline_catalog::Catalog;
use leyline_core::VersionId;

use crate::session::EditSession;

/// Outcome of one reprocess batch — same shape as [`crate::PresetApplyReport`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReprocessReport {
    /// Versions that received a new revision under the current stage
    /// versions.
    pub reprocessed: Vec<VersionId>,
    /// Versions already rendering through the current stage versions: left
    /// untouched, not a failure.
    pub already_current: Vec<VersionId>,
    /// Versions left unchanged, with the human-readable reason.
    pub failed: Vec<FailedReprocess>,
}

/// One version a reprocess batch could not migrate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedReprocess {
    /// The version left unchanged.
    pub version: VersionId,
    /// Why the migration failed.
    pub reason: String,
}

/// Reprocesses each version of `versions`, one fresh session (and therefore
/// one new revision, when needed) per version. One failing version does not
/// stop the batch; `progress` receives `(done, total)` after each version,
/// the batching contract of `docs/engine-api.md` §3.1.
pub fn reprocess_batch(
    catalog: &mut Catalog,
    versions: &[VersionId],
    mut progress: impl FnMut(u64, u64),
) -> ReprocessReport {
    let total = versions.len() as u64;
    let mut report = ReprocessReport::default();
    for (done, &version) in versions.iter().enumerate() {
        match reprocess_one(catalog, version) {
            Ok(Some(_)) => report.reprocessed.push(version),
            Ok(None) => report.already_current.push(version),
            Err(error) => report.failed.push(FailedReprocess {
                version,
                reason: error.to_string(),
            }),
        }
        progress(done as u64 + 1, total);
    }
    report
}

/// Opens a fresh session and reprocesses it. `Ok(None)` when the head was
/// already current — [`EditSession::reprocess`]'s own no-op, not an error.
fn reprocess_one(
    catalog: &mut Catalog,
    version: VersionId,
) -> leyline_core::Result<Option<leyline_core::RevisionId>> {
    let mut session = EditSession::open(&mut *catalog, version)?;
    let before = session.settings().stages.clone();
    let head = session.reprocess()?;
    Ok((session.settings().stages != before).then_some(head))
}
