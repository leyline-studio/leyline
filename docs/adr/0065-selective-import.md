# ADR 0065 — Choosing what to import, by seeing it

**Status:** Accepted — 2026-08

## Context

Leyline's import takes a folder and takes **everything** in it.
`Library::import` enumerates the source, filters on the extension, and writes
one asset per accepted file (`crates/leyline-engine/src/import.rs`). There is
no way — not in the engine, not in the CLI, not in Studio — to say "these yes,
those no".

On a card of 800 shutter releases of which forty are kept, that means writing
800 catalog rows and 800 thumbnails, and then removing 760 photos one by one.
The culling therefore happens *after* the writing, when the photographer has
already done it in their head *before* — they know, looking at the thumbnails,
which ones they want.

It is also the last place where Leyline asks for trust without showing.
Everywhere else, a mass operation announces itself: export says what it will
write, deletion asks for confirmation, and the grid shows before one
classifies. Import, for its part, swallows a folder.

A technical lock partly explains the wait: **the thumbnail of a file not yet
catalogued exists nowhere**. The preview cache is indexed by `asset_id`
(`docs/catalog.md` §21), and a candidate has none. Developing the file in order
to see it would be absurd — it has no revision, and a full decode per file
would cost more than the import one is trying to avoid. Yet cameras already
write a JPEG thumbnail into every RAW, and LibRaw knows how to get it out
(`libraw_unpack_thumb`) — a function present in the linked library, never
exposed by `leyline-raw`.

## Decision

**Scanning and importing become two distinct operations.** The scan looks and
describes; the import writes, and receives an explicit list of files.

### 1. `scan_import` — what is there, without writing anything

```rust
pub struct ScanOptions {
    pub recursive: bool,
    /// Extract each candidate's thumbnail (see §2).
    pub thumbnails: bool,
}

pub struct ImportCandidate {
    pub path: PathBuf,
    pub filename: String,
    pub media_type: MediaType,
    pub file_size: u64,
    pub capture_date: Option<i64>,
    pub camera: Option<String>,
    /// Already in the library, very probably (§3).
    pub already_imported: bool,
    /// JPEG, 256 px longest edge, orientation applied; `None` if the file
    /// carries none or if `ScanOptions::thumbnails` was false.
    pub thumbnail: Option<Vec<u8>>,
}
```

The scan **writes nothing**: no asset, no copied file, no cache entry. It reads
headers. That is what makes it safe to run on a card one is still hesitating to
import.

It enumerates exactly what `import` would have retained — the same walk, the
same extension filter, the same sort by path. Two lists that diverged would be
worse than no list at all: the enumeration code is therefore shared, not
copied.

### 2. The thumbnail comes from the file, never from the pipeline

For a RAW, it is **the thumbnail the camera wrote**: `libraw_unpack_thumb` gets
it out as it is. For a JPEG/PNG/TIFF, it is the image itself. In both cases it
is reduced to a 256 px edge and re-encoded as JPEG, with orientation applied —
a contact sheet of photos lying on their side is of no use.

No rendering by the develop pipeline: there is no revision to render, and the
whole point of the exercise is precisely not to pay for a decode per file
before knowing which ones are kept.

The thumbnails are **carried by the scan**, not requested one by one
afterwards. The file is already open and its header already read; adding the
extraction avoids a second traversal and an asynchronous round trip per row.
`thumbnails: false` exists for the caller that displays nothing (the CLI), and
the scan is then purely metadata.

They are **not cached to disk**: the cache is indexed by asset, these files have
none, and a candidate's thumbnail does not survive the decision it serves to
make.

### 3. Duplicates are shown, and the truth stays the fingerprint

A candidate is marked `already_imported` when the catalog already holds an
asset of the **same name and same size**. It is an index comparison, with no
reading of the content.

The real refusal stays the import's: the content's BLAKE3 fingerprint
(`import.rs`), which is the only exact answer. Marking at scan time by
fingerprint would mean reading every file in full — 20 GB for a full card,
before the user has chosen anything at all. The marking is therefore a
**reliable indication in practice and never authoritative**, and it is named for
what it is. A marked candidate stays listed and stays checkable: that is
precisely the gesture ADR 0043 §5 needed (re-importing after a collapse of the
history).

### 4. `import_files` — importing a list, not a folder

```rust
pub fn import_files(&self, source: &Path, files: &[PathBuf],
                    options: &ImportOptions) -> Result<ImportReport>;
```

The same per-file pipeline as `import`, the same `ImportReport`, the same
copying rules. `source` stays necessary: it is what gives the relative path
under `Photos/` when `copy_files` is true, and it is therefore the root the
files must be found under — a file outside `source` is **refused**, not filed at
random.

`import(source)` **does not change**. It is what the CLI, the watched folder
(ADR 0039) and tethering (ADR 0038) use, and turning "import this folder" into
a two-step ceremony would be a regression for all three. `import` becomes a
call to `import_files` with everything the scan found.

### 5. The three clients

* **Engine**: `scan_import_async` is added to the background jobs, with
  `JobResult::Scan`. A card scan reads hundreds of headers; doing it on the
  interface thread would freeze the window, which ADR 0033 already forbids for
  the import itself.
* **CLI**: `leyline scan <library> <source> [--flat]` lists the candidates with
  duplicates marked, and `import` gains `--only <name>` (repeatable) to import
  a selection without an interface. Without that option, the CLI would see the
  list without being able to use it.
* **Studio**: the import dialog shows the candidates' contact sheet, with
  checkboxes and duplicates unchecked by default, plus "all / none". The import
  button imports only what is checked.

## Consequences

* `leyline-raw` at last exposes a thumbnail (`thumbnail`), with its own
  extraction — that is the only addition to the FFI surface.
* Studio's import dialog becomes a view in its own right: it keeps a list in
  memory for the duration of the decision, and releases it on closing.
* A scan and then an import pay for two traversals of the folder. That is the
  price of the decision taken in between, and it is paid once per card, not per
  photo.
* `docs/engine-api.md` §6 describes the scan beside the import.

## Alternatives rejected

* **Importing and then culling in the grid**: that is the current state, and it
  is what the decision refuses — 760 assets written for 40 kept, and a catalog
  carrying the trace of what was never wanted.
* **One thumbnail per asynchronous call, as each row is displayed**: lazier,
  and more expensive here — the file would be reopened a second time per row,
  and a list filling in cell by cell while one scrolls it is harder to read
  than a complete list two seconds later.
* **Detecting duplicates by fingerprint at scan time**: exact, and it requires
  reading the whole card to obtain (§3).
* **A pattern filter (`*.CR2`, a date range) rather than a selection**: useful
  one day, but it is not the question asked — one chooses by *looking*, not by
  describing what one is after. Nothing prevents adding a filter above the list
  later.
