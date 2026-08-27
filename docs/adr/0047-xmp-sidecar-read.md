# ADR 0047 — Reading XMP sidecars: a seeding at import, never a synchronization

**Status:** Accepted — 2026-07

## Context

`crates/leyline-engine/src/xmp.rs` knows how to **write** a sidecar, and says
so plainly in its second line:

> The catalog is always the source of truth; sidecars exist purely for
> interoperability with other tools and are **never read back**.

`docs/catalog.md` §29 says the same ("the engine never reads XMP as a primary
source"), and §2.4 states that the catalog is the sole source of truth. Those
statements aim at a good target — no implicit synchronization, no external file
able to contradict the catalog — but they have a consequence nobody chose:
**Leyline can ingest nothing**.

Yet that is exactly step 2 of any migration from another program, and the only
one the guides recommend: *export the XMP from Lightroom, have the new tool
read them, and the keywords and ratings follow the RAWs*. It is also the only
channel that exists: develop settings are transferable by nobody (proprietary
algorithms), but the **culling work** — ratings, labels, hierarchical keywords,
sometimes years of classification — is, and it takes far longer to redo than a
retouch.

The result today: a photographer importing their 40,000 RAWs into Leyline
arrives at a catalog empty of all classification, when the information is
sitting beside every file, in a format Leyline itself already writes. That is
not a missing feature, it is an adoption blocker.

**What is not at issue.** The catalog stays the source of truth, and nothing
here opens a second channel of authority: that is precisely what §3's policy
guarantees, and it is the decision's core rather than its reservation.

## Decision

### 1. A sidecar seeds, it does not synchronize

Leyline reads a sidecar to **fill in what is empty**, never to replace what
exists. That is what distinguishes a seeding from a synchronization, and what
leaves `docs/catalog.md` §2.4 exact: after reading, the only authority stays
the catalog, and nothing will read that file again afterwards.

There is therefore **no** symmetry with §29's three writing modes: no *Always*
mode on reading, no file watching, no re-read on change, and no reconciliation.
A sidecar is read at §2's two moments, and at no other.

### 2. Two moments of reading, and only two

* **At import**, automatically: if a `photo.xmp` is found beside the imported
  file, it is applied to the asset just created. That is the moment that counts
  — the asset is new, it has nothing to overwrite, and it is the gesture the
  migrant makes without having to know a command exists. No setting: the
  sidecar is beside the file, so it concerns that file. It is the **source
  file's** sidecar that is read — where the other program left it — and it is
  not copied into `Photos/`: once read, it has no authority any more, and hence
  no reason to be kept.
* **Explicitly**, on an already-imported asset: `Library::read_xmp(asset)`, the
  exact mirror of `Library::write_xmp(asset)`, for the library already built
  before the sidecars were exported from the other program.

### 2.1 "Beside the file" is not enough to name it

The point above says "if a `photo.xmp` is found beside the file", and that is
what the code did: replace the extension with `.xmp`. That sentence settled a
question without asking it — **other programs do not all name the sidecar the
same way**, and two conventions coexist:

| Convention | Written by | Example for `5D4_7998.CR2` |
|---|---|---|
| Full name + `.xmp` | darktable, exiftool | `5D4_7998.CR2.xmp` |
| Extension replaced | Lightroom, Bridge, Leyline | `5D4_7998.xmp` |

Only one of the two was read. The real test corpus showed what that costs: out
of nine sidecars, **the only one carrying keywords was darktable's**, and hence
the only invisible one. Nothing signalled it — that is exactly the failure mode
§6 refuses for parsing, arriving one level higher, on the file name.

**Decision: we write one convention, we read two.**

* **Reading**: the full name first, the replaced extension next. The first that
  answers wins. That order is not arbitrary: `photo.CR2.xmp` designates **one**
  photo, whereas `photo.xmp` is shared by every file of the same stem in the
  folder — an `IMG_2048.xmp` sitting beside an `IMG_2048.CR2`, an
  `IMG_2048.JPG` and an `IMG_2048.dng` (a real case from the corpus) does not
  say which one it is about. The unambiguous form therefore goes first, and it
  is the one that decides when the other program has written one.
* **Writing**: unchanged, the extension replaced. It is Adobe's convention,
  hence the one the program targeted by a reverse migration looks for, and the
  reading above takes it up — §4's round trip still holds.

The shared form's ambiguity remains; it is inherent to Adobe's convention and
is not ours to resolve: when several files of the same stem live together, they
receive the same seeding. Under §3's policy, at import, that amounts to giving
each copy of one shot the classification the other program gave it — the
acceptable consequence of what the sidecar does not say.

### 3. The conflict policy: fill in, never overwrite

The point that required a decision rather than code.

| Data | On reading |
|---|---|
| Rating (`xmp:Rating`) | Applied **if** the current version has none |
| Label (`xmp:Label`) | Applied **if** the current version has none |
| Keywords | **Union** — the sidecar's keywords are added, none is removed |
| Artist, copyright | Applied **if** the corresponding field is empty |

Three reasons, in the order in which they weigh:

1. **At import, the interesting case, the question does not arise**: everything
   is empty, so "filling in" applies the sidecar whole. The policy costs
   nothing where it serves most.
2. **On an explicit re-read, the user cannot undo.** A sidecar may be years old
   and poorer than the work done since in Leyline; "the file wins" would
   destroy that work silently, in bulk, on a command from which nobody expects
   it.
3. **It is the only policy that loses no data**, in either direction. The union
   on keywords is in the same spirit: `add_keyword` is already idempotent, so
   reading the same sidecar twice is a no-op.

A "the sidecar wins" mode would be a *separate* decision, with its own
confirmation in the interface. It is not taken here.

### 4. The field read is exactly the field written

`write_xmp_sidecar`'s interoperable core: rating, label, flat and hierarchical
keywords, artist, copyright. No more, no less — the write → read round trip is
the invariant, and a test verifies it rather than a comment asserting it.

**Hierarchical keywords are authoritative** when both forms are present:
Lightroom writes `lr:hierarchicalSubject` (`Travel|Japan|Kyoto`) *and* its
flattening `dc:subject`, and keeping only the second would lose the tree the
catalog can represent. Paths absent from the keyword tree are created, level by
level, under the existing nodes.

### 5. Reading is tolerant; import never breaks

A sidecar that is unreadable, malformed, or that contains none of the fields
above is **not** an import error: the photo file imports normally, without
metadata from the sidecar. It is the rule `import.rs` already applies to its
own steps ("Per-file problems never abort the batch"), extended one notch: here
even one file's problem does not set that file aside, it sets its sidecar
aside.

Symmetrically, no sidecar is **written** by a read. Reading does not trigger
§29's synchronization.

### 6. Parsing takes a dependency: `roxmltree`

Unlike [ADR 0037](0037-dcp-parsing-dependency.md), which wrote a minimal
in-house reader for the DCP tags, this reader does not read our own files: it
reads Lightroom's, darktable's, exiftool's, Capture One's. In practice that
means variable namespace prefixes (`xmp:` is only a convention, and only the
URI counts), the same datum sometimes as an attribute and sometimes as an
element (`xmp:Rating="4"` or `<xmp:Rating>4</xmp:Rating>`), XMP packets framed
by `<?xpacket?>`, and whitespace everywhere. An in-house reader does not fail
outright on those, it fails **silently** — it returns a catalog with no
keywords with nothing to signal it, which is the worst possible failure mode
for a migration feature.

`roxmltree` is retained: a read-only tree, namespace-aware (hence indifferent
to prefixes), pure Rust, with no heavy transitive dependency and no C brick —
unlike LibRaw, Lensfun or LittleCMS, it adds nothing to the build chain nor to
the installers.

### 7. Out of scope, explicitly

* **Develop settings from the `crs:` namespace** (Adobe Camera Raw). They are
  readable but untranslatable: applying a `crs:Exposure2012` to our pipeline
  would produce an image *different* from the one the user saw in Lightroom,
  while claiming to have reproduced it. No program does it, and doing it
  halfway would be lying on the one point where Leyline promises exactness.
* **Embedded XMP** in the RAW, the DNG or the JPEG. Only `.xmp` sidecars sitting
  beside the file are read; embedded XMP would require writing into the original
  files to stay consistent, which the non-destructiveness contract forbids
  (`docs/pipeline.md` §6).
* **Collections, stacks, snapshots.** No interoperable representation exists:
  every program stores them in its own catalog.

## Consequences

* **Migration from Lightroom becomes real with no new interface.** The path is
  the one everyone already documents: *Metadata → Save Metadata to Files* at
  Adobe's end, and then an ordinary Leyline import. Studio, the CLI and the SDK
  all three benefit from the mere fact that they import, without any of them
  adding a screen.
* **`docs/catalog.md` §29 gains a "Reading" section**, and its sentence "the
  engine never reads XMP" is corrected into what it meant: the engine never
  reads them **as authority**. §2.4 is unchanged.
* **One more dependency in `leyline-engine`**, the first for XML parsing. It is
  confined to the `xmp` module: nothing else in the engine sees `roxmltree`.
* **The round trip becomes a tested invariant.** Writing a sidecar from a
  classified asset, reading it back into a blank one, comparing: that is the
  test that keeps §4 honest when either side moves.
* **The risk of an import slowing down** is bounded: two `stat`s per imported
  file when no sidecar exists (the common case), and a parse of a few kilobytes
  when one does.
* **Deletion to the trash takes both forms with it.** Leaving behind the
  sidecar of a photo that no longer exists means seeing it resurrected at the
  next import of the same folder.

## Alternatives rejected

* **"The sidecar wins"**, on an explicit read. Rejected in §3: silent and
  irreversible data loss, on an innocuous-looking command. Still possible
  later, as a distinct and confirmed mode.
* **An *Always* mode on reading** (watching the sidecars and resynchronizing).
  That is a second channel of authority over the same fields, hence the end of
  §2.4 — and a whole class of conflicts to arbitrate for a need nobody has
  expressed.
* **Reading `crs:` too**, so as to "at least approach" Lightroom's rendering.
  Rejected in §7: an approximation presented as a reproduction is worse than an
  announced absence.
* **An in-house XML reader**, in ADR 0037's spirit. Rejected in §6: the failure
  mode on foreign files is silence, and that is unacceptable for migration. The
  comparison with DCP does not hold — a TIFF container we read for our own
  profile files has none of the variability of an RDF packet written by four
  competing programs.
* **A Lightroom catalog import** (reading the `.lrcat`, which is SQLite). It
  would transfer the collections, which XMP does not carry. Rejected for now:
  an undocumented format versioned by Adobe, hence a permanent
  reverse-engineering surface — to be reopened on a real request, with its own
  ADR.
