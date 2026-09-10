# ADR 0139 — Stopping a batch, and saying what it is doing

**Status:** Accepted — 2026-09

## Context

Counted in the engine before this change: **not one function that stops a
running job**. `import_async`, `export_async`, `print_async`,
`apply_preset_async`, `reprocess_async`, `scan_import_async` and the rest hand
back a `JobId`, and nothing takes one back. An import of thirty-eight thousand
photographs started by mistake — with *Copy into library* on by default, so
tens of gigabytes of copying — ends in exactly one way: killing the
application.

The second half is worse for being invisible. `job-progress` is read in
`ui/panels/dialogs.slint` and nowhere else, so a batch's progress lives inside
the modal that launched it. That modal can be closed — the job goes on — and
the window then shows **nothing at all**: no count, no bar, no clue that the
machine is busy. A photographer who closes the import dialog to look at their
photographs has no way of knowing whether the import is still running.

And one operation was not even a job. *Library ▸ Reprocess the library*
(`Shift+R`) called `Library::reprocess` **synchronously, on the UI thread**,
over every version in the catalog, on the argument written in its own comment:
reprocessing renders no pixels, it only rewrites `settings_json`. True, and
beside the point at the only scale that matters — one revision written per
photograph over a large library is minutes of frozen window.

## Decision

### 1. The checkpoint that already exists is where a batch stops

Every long operation already calls `progress(done, total)` after each unit of
work. That call is the one place a batch is provably **between two
photographs**, with nothing half-written: no file half-copied, no revision
half-committed, no export half-encoded. So it is where the answer is given.

The callback's return type carries it: `progress: impl FnMut(u64, u64) -> F`
where `F: Into<Flow>`, and `Flow` is `Continue` or `Stop`. Two consequences
worth the line:

* **The hundred existing call sites do not change.** `|_, _| {}` returns `()`,
  `()` converts to `Flow::Continue`, and a caller with no cancelling to do goes
  on for ever exactly as before. The CLI, the benches and 109 test closures
  were untouched by this ADR.
* **A loop cannot ignore the answer by accident.** It is a value returned into
  the condition of the loop, not a flag beside it.

`Library::cancel_job(id)` records the id in a set the async wrappers' progress
closure reads. It is deliberately **not** a handle: a client that hears
`JobFinished` and cancels a moment later touches nothing, and the set is
cleared as each job ends so a stale cancellation can never land on the next
job. Both are tested.

### 2. What is done is kept, and the report says the batch stopped

A cancelled job ends with the ordinary `JobFinished`. There is no `Cancelled`
result variant, because a stopped batch is not a failed one: it produced
exactly what its report enumerates. `ImportReport`, `ExportReport`,
`PrintReport`, `PresetApplyReport`, `ReprocessReport` and `ScanReport` each
gain one `cancelled: bool`.

Nothing is rolled back — and that is the decision, not a limitation. An import
that stopped at four thousand photographs has four thousand photographs in the
catalog, which is what the photographer sees on screen and what they would
have to undo by hand if the engine "cleaned up" behind them.

The concurrent export batch ([ADR 0068](0068-concurrent-export-batch.md))
needed one extra part: the progress loop sets an `AtomicBool` that each worker
reads before picking up its next photograph. With four in flight the batch ends
after at most four more files, each of them whole; the versions never begun are
in neither list of the report, which is the honest shape — they are not
exports and they are not failures.

`ScanReport` is new and exists for this flag alone. A partial list of
candidates looks exactly like a complete one, and a dialog that announced
« 412 photos found » after a scan the user stopped would be stating something
false about the folder.

### 3. Two jobs deliberately do not listen

**The contact sheet** produces one PDF. Stopping halfway leaves nothing worth
keeping, and a report naming a file that was never written would be a lie.
**Assisted culling** produces a proposal: one covering half a shoot would say
"reject these" while staying silent about the rest, which is worse than making
the photographer wait. Both are bounded — a sheet is a few dozen photographs, a
culling run is milliseconds per frame ([ADR 0084](0084-assisted-culling.md)) —
and both would need a different answer than "keep what was done", so they are
out of scope rather than half-supported.

`preview_async` renders one photograph and ends before a click could reach it.

### 4. A bar in the corner, outliving the dialog that started the job

`TaskBar` is drawn by the window, next to the tether bar and for the same
reason ([ADR 0087](0087-tethered-capture-bar.md) §7): a job outlives the dialog
that launched it and every view switch, so its progress cannot live inside a
modal. Bottom **right**, because bottom centre is the tether bar's and both can
be up at once.

It shows the kind of work, `done / total`, a three-pixel proportion along its
lower edge, and one button. Three details are decisions:

* **One job at a time** — the most recent to report. The question the corner of
  a window answers is "what is this busy with, and can I stop it", and that
  question has one answer. A second batch takes the bar over; the first keeps
  running and keeps its own dialog.
* **The button is absent, not disabled, on a job that cannot be cancelled**
  (§3). A button that does nothing is worse than no button.
* **It reads *Stopping…* from the click onwards.** The batch ends at its next
  checkpoint, one photograph away at worst, and a button still reading *Cancel*
  would look ignored for exactly as long as the current file takes.

### 5. Reprocessing the library becomes a job

`reprocess_library` now calls `reprocess_async` and reports through the bar and
the status line like every other batch. Its old comment was right that
reprocessing writes no pixels — which is precisely why stopping it costs
nothing: every photograph already migrated stays migrated, and the rest are
still on the stage versions they were on this morning.

## Consequences

* The engine gains one public function, one public type (`Flow`), one new
  report type (`ScanReport`), and one boolean on five reports. `JobResult::Scan`
  changes shape, which is a breaking change to the SDK surface — recorded here
  because the SDK is a pure façade (`tests/surface.rs` guards it) and its
  callers are this repository's own clients.
* Studio gains a global (`JobsState`), a panel (`TaskBar`) and a wiring module.
  The dialogs keep their own progress line: this is what survives closing them.
* A batch stopped mid-way is now an ordinary outcome the interface must phrase,
  which is why the summaries gained « — stopped; what was done is kept » rather
  than a warning colour: nothing went wrong.
* What this does **not** give: a job queue, a history of past jobs, or a pause.
  A pause on a batch holding the catalog lock is a different decision, and
  nobody has asked for one.

## Alternatives rejected

* **A cancellation token passed as a second parameter** (`cancel: &AtomicBool`).
  Same ripple through every signature, and it puts the flag *beside* the
  checkpoint rather than at it — two things to keep in step instead of one.
* **`ControlFlow<()>` from the standard library** instead of `Flow`. It says
  `Break` where this domain says "stop", and no `From<()>` impl can be written
  for it outside `std`, which is exactly the conversion that spared 109 call
  sites.
* **Rolling back a cancelled batch.** Undoing an import means removing assets a
  photographer has already seen appear; it also makes cancellation a *risk*
  rather than a relief, which is the opposite of the point.
* **A `Cancelled` job result.** It throws away the partial report, and every
  client would have to handle a third case that carries no information.
* **Cancelling by dropping a handle.** `JobId` is a number the client keeps and
  may drop at will; making the drop meaningful would turn every stored id into
  a liability.
* **A general task centre with a queue and a history.** More surface than the
  problem: what was missing is knowing what runs *now* and being able to stop
  it.
