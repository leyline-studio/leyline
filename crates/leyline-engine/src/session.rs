//! Edit sessions: the coalescence policy (`docs/engine-api.md` §10.1).
//!
//! A session materializes the rule of `docs/catalog.md` §17: a revision is a
//! *user intention*, never a UI event. `set` only updates the in-memory
//! state — called on every cursor movement, it writes nothing. `commit` is
//! called at commit points (control released, tool changed...) and decides
//! alone between a new revision and an amendment of the head: successive
//! adjustments of the *same* parameter within the amendment window coalesce
//! into one revision.
//!
//! Dropping a session commits any pending state: nothing is ever lost.

use std::ops::DerefMut;
use std::time::{Duration, Instant};

use leyline_catalog::{Catalog, RevisionRow};
use leyline_core::{
    CURRENT_PROCESS, CURRENT_SCHEMA, CameraProfile, ColorGrading, Crop, HslBand, LensCorrection,
    LeylineError, LocalAdjustment, NoiseReduction, Result, RevisionId, Settings, Sharpening,
    SpotRemoval, ToneCurve, VersionId, WhiteBalance,
};

/// Default amendment window of `docs/catalog.md` §17.
pub const DEFAULT_AMEND_WINDOW: Duration = Duration::from_secs(2);

/// One user-facing develop parameter (`docs/pipeline.md` §3.2, schema 1).
///
/// The granularity is the *control*: successive changes to the same `Param`
/// are one intention and coalesce; changing `Param` is a new intention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    /// White balance override (temperature + tint together: one tool).
    WhiteBalance,
    /// Exposure compensation in EV.
    Exposure,
    /// Contrast slider.
    Contrast,
    /// Highlights recovery slider.
    Highlights,
    /// Shadows lift slider.
    Shadows,
    /// White point slider.
    Whites,
    /// Black point slider.
    Blacks,
    /// Local contrast at a large blur radius (ADR 0033).
    Clarity,
    /// Local contrast at a small blur radius (ADR 0033).
    Texture,
    /// Dark-channel-prior haze removal (ADR 0033).
    Dehaze,
    /// Vibrance slider.
    Vibrance,
    /// Saturation slider.
    Saturation,
    /// Tone curve step.
    ToneCurve,
    /// Spot removal clones (the whole list, replaced atomically).
    SpotRemoval,
    /// One local adjustment (ADR 0029): geometry and re-parameterized
    /// values together as one tool, addressed by its index in
    /// `local_adjustments` — the same grouping [`Param::WhiteBalance`]
    /// already applies to temperature + tint.
    LocalAdjustment(usize),
    /// Lens correction step.
    LensCorrection,
    /// One band of the 8-band HSL mixer (ADR 0031), addressed by its index
    /// in `hsl` (always `0..8`) — the same per-index coalescing
    /// [`Param::LocalAdjustment`] applies.
    HslBand(usize),
    /// Shadows/midtones/highlights color grading (ADR 0031): the whole
    /// struct as one commit unit, like [`Param::LensCorrection`].
    ColorGrading,
    /// Noise reduction step.
    NoiseReduction,
    /// Sharpening step.
    Sharpening,
    /// Rotation in degrees, clockwise.
    Rotation,
    /// Crop rectangle; `None` clears it.
    Crop,
    /// Camera profile (ADR 0035): the referenced `.dcp` file, its
    /// checksum, and whether it's enabled, all as one commit unit — the
    /// same whole-struct grouping [`Param::LensCorrection`] applies.
    CameraProfile,
}

/// A value for one [`Param`]. The pairing is type-checked by
/// [`EditSession::set`]: a mismatch is an [`LeylineError::InvalidSettings`].
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// For [`Param::Exposure`] and [`Param::Rotation`].
    Float(f64),
    /// For the unitless [-100, +100] sliders.
    Int(i32),
    /// For [`Param::WhiteBalance`]; `None` returns to as-shot.
    WhiteBalance(Option<WhiteBalance>),
    /// For [`Param::ToneCurve`].
    ToneCurve(ToneCurve),
    /// For [`Param::SpotRemoval`].
    SpotRemoval(Vec<SpotRemoval>),
    /// For [`Param::LocalAdjustment`]: `Some` replaces the whole entry at
    /// that index (or appends, if the index equals the current length —
    /// there is no separate `add_mask` method, ADR 0029); `None` removes
    /// the entry at that index.
    LocalAdjustment(Option<LocalAdjustment>),
    /// For [`Param::LensCorrection`].
    LensCorrection(LensCorrection),
    /// For [`Param::HslBand`].
    HslBand(HslBand),
    /// For [`Param::ColorGrading`].
    ColorGrading(ColorGrading),
    /// For [`Param::NoiseReduction`].
    NoiseReduction(NoiseReduction),
    /// For [`Param::Sharpening`].
    Sharpening(Sharpening),
    /// For [`Param::Crop`]; `None` returns to the full frame.
    Crop(Option<Crop>),
    /// For [`Param::CameraProfile`]; `None` returns to no camera profile
    /// (the decoder's default sRGB rendering).
    CameraProfile(Option<CameraProfile>),
}

/// What changed since the last commit point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    /// Nothing to commit.
    Clean,
    /// Exactly one parameter changed: an amendment candidate.
    One(Param),
    /// Several parameters changed: always a new revision.
    Many,
}

/// Callback invoked after every write to the version's history.
type Notifier = Box<dyn FnMut(VersionId) + Send>;

/// An open edit session on one develop version (`docs/engine-api.md` §10.1).
///
/// The session holds the only write handle: while it lives, nothing else
/// mutates the version, so its in-memory state is authoritative. It is
/// generic over how that handle is held — a plain `&mut Catalog`, or the
/// lock guard a shared [`crate::Library`] hands out.
pub struct EditSession<C: DerefMut<Target = Catalog>> {
    catalog: C,
    version: VersionId,
    settings: Settings,
    pending: Pending,
    /// Last commit, when it changed exactly one parameter: the §17
    /// amendment chain. `None` after undo/redo or a multi-parameter commit.
    last_commit: Option<(Param, Instant)>,
    amend_window: Duration,
    /// Called after each commit, amendment, undo or redo that actually
    /// wrote — the `Library` plugs `VersionChanged` in here (§3.2).
    notify: Option<Notifier>,
}

impl<C: DerefMut<Target = Catalog>> std::fmt::Debug for EditSession<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditSession")
            .field("version", &self.version)
            .field("settings", &self.settings)
            .field("pending", &self.pending)
            .field("last_commit", &self.last_commit)
            .field("amend_window", &self.amend_window)
            .field("notify", &self.notify.as_ref().map(|_| "…"))
            .finish_non_exhaustive()
    }
}

impl<C: DerefMut<Target = Catalog>> EditSession<C> {
    /// Opens a session on the version's head.
    ///
    /// A head written by a newer engine (newer `schema` or `process`) is
    /// refused with [`LeylineError::NewerSettings`]: the client shows the
    /// best cached preview with a warning instead (`docs/pipeline.md` §3.4).
    pub fn open(catalog: C, version: VersionId) -> Result<EditSession<C>> {
        let head = catalog.version_head(version)?;
        let settings = Settings::parse(&catalog.revision(head)?.settings_json)?;
        if settings.schema > CURRENT_SCHEMA || settings.process > CURRENT_PROCESS {
            return Err(LeylineError::NewerSettings {
                schema: settings.schema,
                process: settings.process,
            });
        }
        Ok(EditSession {
            catalog,
            version,
            settings,
            pending: Pending::Clean,
            last_commit: None,
            amend_window: DEFAULT_AMEND_WINDOW,
            notify: None,
        })
    }

    /// Registers a callback invoked after every write to the version's
    /// history (commit, amendment, undo, redo). One callback at most.
    pub fn with_notifier(mut self, notify: impl FnMut(VersionId) + Send + 'static) -> Self {
        self.notify = Some(Box::new(notify));
        self
    }

    /// Overrides the amendment window (§17: 2 seconds, configurable).
    pub fn set_amend_window(&mut self, window: Duration) {
        self.amend_window = window;
    }

    /// The version this session edits.
    pub fn version(&self) -> VersionId {
        self.version
    }

    /// The complete current develop state, pending changes included.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Updates one parameter in memory — no catalog write, real-time preview
    /// only. Called on every cursor movement.
    ///
    /// The value is validated immediately: an out-of-range or mistyped value
    /// leaves the state untouched.
    pub fn set(&mut self, param: Param, value: Value) -> Result<()> {
        let previous = self.settings.clone();
        apply(&mut self.settings, param, value)?;
        if let Err(invalid) = self.settings.validate() {
            self.settings = previous;
            return Err(invalid);
        }
        self.pending = match self.pending {
            Pending::Clean => Pending::One(param),
            Pending::One(p) if p == param => Pending::One(param),
            _ => Pending::Many,
        };
        Ok(())
    }

    /// Commit point: persists the pending state and returns the head.
    ///
    /// The session decides alone between amendment and new revision (§17):
    /// when the pending change touches exactly the parameter of the previous
    /// commit, within the amendment window, and the catalog guards allow it,
    /// the head is amended in place. Otherwise a new revision is committed.
    /// With nothing pending, the current head is returned unchanged.
    pub fn commit(&mut self) -> Result<RevisionId> {
        let now = Instant::now();
        let head = match self.pending {
            Pending::Clean => return self.catalog.version_head(self.version),
            Pending::One(param) => {
                let in_window = self.last_commit.is_some_and(|(p, at)| {
                    p == param && now.duration_since(at) <= self.amend_window
                });
                let amended = if in_window {
                    self.catalog.try_amend_head(self.version, &self.settings)?
                } else {
                    None
                };
                let head = match amended {
                    Some(amendment) => amendment.revision,
                    None => self.catalog.commit_revision(self.version, &self.settings)?,
                };
                self.last_commit = Some((param, now));
                head
            }
            Pending::Many => {
                let head = self.catalog.commit_revision(self.version, &self.settings)?;
                self.last_commit = None;
                head
            }
        };
        self.pending = Pending::Clean;
        self.notify_write();
        Ok(head)
    }

    /// Commits any pending state, then moves the head back one revision.
    ///
    /// Returns the new head, or `None` at the initial revision. The session
    /// state reloads from the new head; the undone revision stays reachable.
    pub fn undo(&mut self) -> Result<Option<RevisionId>> {
        self.commit()?;
        let moved = self.catalog.undo_version(self.version)?;
        self.reload_head(moved)?;
        if moved.is_some() {
            self.notify_write();
        }
        Ok(moved)
    }

    /// Commits any pending state, then moves the head forward one revision.
    ///
    /// Returns the new head, or `None` when there is nothing to redo.
    pub fn redo(&mut self) -> Result<Option<RevisionId>> {
        self.commit()?;
        let moved = self.catalog.redo_version(self.version)?;
        self.reload_head(moved)?;
        if moved.is_some() {
            self.notify_write();
        }
        Ok(moved)
    }

    /// The revision chain of the version, head first.
    pub fn history(&self) -> Result<Vec<RevisionRow>> {
        self.catalog.version_history(self.version)
    }

    /// Commits any pending state, then jumps the head directly to `revision`
    /// — a browsable history panel's "jump to this point", beyond
    /// [`EditSession::undo`]/[`EditSession::redo`]'s one-link-at-a-time
    /// movement. Like those two, no revision is created or deleted, and
    /// jumping away from a revision keeps it reachable.
    pub fn checkout(&mut self, revision: RevisionId) -> Result<RevisionId> {
        self.commit()?;
        self.catalog.checkout_revision(self.version, revision)?;
        self.reload_head(Some(revision))?;
        self.notify_write();
        Ok(revision)
    }

    /// Migrates the version's head to the engine's current process version
    /// (`docs/pipeline.md` §4.5): a new revision with the exact same
    /// parameter values, re-rendered under a newer process contract — e.g.
    /// a photo imported before lens correction existed picking it up
    /// without the user touching a single slider.
    ///
    /// Always its own revision, never an amendment (§4.5: "conserve les
    /// anciennes" — reprocessing keeps prior results reachable, it doesn't
    /// extend the last edit's history entry). A no-op when the head
    /// already declares `CURRENT_PROCESS`: returns the current head
    /// unchanged, without writing a new revision.
    pub fn reprocess(&mut self) -> Result<RevisionId> {
        self.commit()?;
        if self.settings.process == CURRENT_PROCESS {
            return self.catalog.version_head(self.version);
        }
        self.settings.process = CURRENT_PROCESS;
        let head = self.catalog.commit_revision(self.version, &self.settings)?;
        self.last_commit = None;
        self.notify_write();
        Ok(head)
    }

    /// Reports a history write to the registered notifier, if any.
    fn notify_write(&mut self) {
        let version = self.version;
        if let Some(notify) = self.notify.as_mut() {
            notify(version);
        }
    }

    /// Reloads the in-memory state after the head moved, and breaks the
    /// amendment chain: the next commit always creates a revision.
    fn reload_head(&mut self, moved: Option<RevisionId>) -> Result<()> {
        if let Some(head) = moved {
            self.settings = Settings::parse(&self.catalog.revision(head)?.settings_json)?;
            self.pending = Pending::Clean;
            self.last_commit = None;
        }
        Ok(())
    }
}

impl<C: DerefMut<Target = Catalog>> Drop for EditSession<C> {
    /// Closing the session commits the pending state: nothing is ever lost
    /// (`docs/engine-api.md` §10.1). A failing drop-commit is unreportable
    /// and ignored; call [`EditSession::commit`] explicitly to observe errors.
    fn drop(&mut self) {
        let _ = self.commit();
    }
}

/// Applies one typed value to the matching settings field.
fn apply(settings: &mut Settings, param: Param, value: Value) -> Result<()> {
    match (param, value) {
        (Param::Exposure, Value::Float(v)) => settings.exposure = v,
        (Param::Rotation, Value::Float(v)) => settings.rotation = v,
        (Param::Contrast, Value::Int(v)) => settings.contrast = v,
        (Param::Highlights, Value::Int(v)) => settings.highlights = v,
        (Param::Shadows, Value::Int(v)) => settings.shadows = v,
        (Param::Whites, Value::Int(v)) => settings.whites = v,
        (Param::Blacks, Value::Int(v)) => settings.blacks = v,
        (Param::Clarity, Value::Int(v)) => settings.clarity = v,
        (Param::Texture, Value::Int(v)) => settings.texture = v,
        (Param::Dehaze, Value::Int(v)) => settings.dehaze = v,
        (Param::Vibrance, Value::Int(v)) => settings.vibrance = v,
        (Param::Saturation, Value::Int(v)) => settings.saturation = v,
        (Param::ToneCurve, Value::ToneCurve(v)) => settings.tone_curve = v,
        (Param::SpotRemoval, Value::SpotRemoval(v)) => settings.spot_removal = v,
        (Param::LocalAdjustment(index), Value::LocalAdjustment(v)) => match v {
            Some(adjustment) if index < settings.local_adjustments.len() => {
                settings.local_adjustments[index] = adjustment;
            }
            Some(adjustment) if index == settings.local_adjustments.len() => {
                settings.local_adjustments.push(adjustment);
            }
            None if index < settings.local_adjustments.len() => {
                settings.local_adjustments.remove(index);
            }
            _ => {
                return Err(LeylineError::InvalidSettings(format!(
                    "local_adjustments index {index} out of bounds"
                )));
            }
        },
        (Param::WhiteBalance, Value::WhiteBalance(v)) => settings.white_balance = v,
        (Param::LensCorrection, Value::LensCorrection(v)) => settings.lens_correction = v,
        (Param::HslBand(index), Value::HslBand(band)) => {
            if index >= settings.hsl.len() {
                return Err(LeylineError::InvalidSettings(format!(
                    "hsl band index {index} out of bounds"
                )));
            }
            settings.hsl[index] = band;
        }
        (Param::ColorGrading, Value::ColorGrading(v)) => settings.color_grading = v,
        (Param::NoiseReduction, Value::NoiseReduction(v)) => settings.noise_reduction = v,
        (Param::Sharpening, Value::Sharpening(v)) => settings.sharpening = v,
        (Param::Crop, Value::Crop(v)) => settings.crop = v,
        (Param::CameraProfile, Value::CameraProfile(v)) => settings.camera_profile = v,
        (param, value) => {
            return Err(LeylineError::InvalidSettings(format!(
                "value {value:?} does not fit parameter {param:?}"
            )));
        }
    }
    Ok(())
}
