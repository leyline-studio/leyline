# ADR 0153 — An export knows where it comes from

**Status:** Accepted — 2026-10

## Context

`docs/catalog.md` §28 says the export history "makes it possible to reproduce
an export". It does not. A row of `export_history` holds the asset, the preset
id, the format, the destination and a date. It does not say **which version**
or **which revision** was exported, nor **with which settings**: a preset may
have been edited since, and an ad-hoc recipe is not stored at all. It does not
record **the decoder**, which [ADR 0086](0086-decoder-in-the-promise.md)
made a condition of the bit-for-bit promise. And it keeps **nothing of the file
written**, so nothing can tell whether a file on a client's drive is the one
Leyline wrote.

The file says nothing either. Since [ADR 0157](0157-what-the-exported-file-says.md)
it carries the capture, the author and the keywords. It still carries nothing
that leads back to the edit. That ADR already gave the place: provenance
"will travel in the same XMP packet under its own `leyline:` namespace,
subject to this same policy".

The project calls itself the git of photography: every edit is an immutable
revision ([ADR 0007](0007-git-develop-model.md)), and the render of a revision
is promised bit for bit under the conditions of `pipeline.md` §5.1. What is
missing is the last link: going from a file **back** to the revision that made
it.

Three things are easy to confuse here, and this ADR keeps them apart:

* **Traceability**: which revision, which recipe, which decoder made this
  file. This ADR.
* **Integrity**: whether the file is still the one written. A BLAKE3 checksum,
  the one the catalog already uses for its assets (`catalog.md` §12). This ADR
  records it; it does not sign it.
* **Reproducibility**: whether the same inputs give the same bytes again.
  That is §5.1, which exists. This ADR gives it a verb.

## Decision

### 1. The revision fingerprint

```
revision_fingerprint = BLAKE3( asset checksum ‖ 0x00 ‖ revision settings_json )
```

* The **asset checksum** is the BLAKE3 of the source file, already stored
  (`assets.checksum`). The RAW is never modified, so it does not move.
* The **settings** are the revision's `settings_json` **as stored**, byte for
  byte. A revision is immutable, so this string is too. It already holds the
  `stages` map ([ADR 0042](0042-versioned-stage-pipeline.md)), so two revisions that only
  differ by a stage version have different fingerprints.
* It is computed **once, at export time**, and **stored**. Nothing ever
  recomputes it to match it. This matters: re-serializing `Settings` with a
  later build could add a field with its default. The same revision would then
  get a new fingerprint, and every file exported before would stop matching.
* It names a **content**, not a row. The same RAW developed with the same
  settings in two libraries has the same fingerprint, and that is the point of
  calling it a fingerprint and not an id.

A pure function in `leyline-engine`. Its test pins a fixed vector, so a change
of the formula fails a test instead of silently orphaning every exported file.

### 2. The journal: migration v14

`export_history` gains six columns. They are **NULL for the rows written before**:
nothing is invented for them.

| Column | Holds |
|---|---|
| `version_id` | the version exported (`ON DELETE SET NULL`) |
| `revision_id` | the head revision at export time (`ON DELETE SET NULL`) |
| `revision_fingerprint` | §1, 32 bytes |
| `export_settings_json` | the **resolved** recipe — what was applied, not the preset's id |
| `decoder` | `leyline_raw::decoder_version()` at export time |
| `output_checksum` | BLAKE3 of the file written, 32 bytes, indexed |

The journal can hold what the file must not: its date, the application version.
It is local, and it is where the export date has always lived.

`record_export` and `journal_export` take one `ExportProvenance` value instead of
a growing argument list. `plan_export` already reads the head revision; the plan
keeps it. `catalog.md` §28 is corrected in the same change, and its sentence
about reproducing an export becomes true.

v15 stays [ADR 0154](0154-an-agent-surface-over-stdio.md)'s (`develop_revisions.origin`);
whichever is built first takes the next number, and both ADRs mean "the next
migration".

### 3. The provenance in the file

Under `Full` and `WithoutLocation`, the XMP packet of ADR 0157 gains a
`leyline:` description, namespace `https://leyline.photo/ns/provenance/1.0/`:

| Property | Holds |
|---|---|
| `leyline:Revision` | the fingerprint, 64 hexadecimal digits |
| `leyline:Stages` | the `stages` map, `name=version` joined by `;`, sorted |
| `leyline:Decoder` | the LibRaw version |
| `leyline:ExportSettings` | the resolved recipe's JSON |

* **No library id.** A library UUID would link every photograph ever published
  from one library, a tracking id in every file. The fingerprint is enough to
  find a revision in the library that holds it, and is meaningless elsewhere.
* **No clock, no application version**, for ADR 0157 §3's reason: either
  would make two exports of the same inputs differ.
* **No `output_checksum` in the file.** A file cannot hold its own checksum.
  The journal holds it.
* Under **`None`**, nothing — the policy means *nothing but the ICC profile*.
  The journal still records everything: provenance in the file is a
  choice, and the journal is not.
* **AVIF** carries no packet (ADR 0157 §5), so it carries no provenance; its
  journal row is complete, and `verify` works on it by checksum (§4).

The block is inside the packet ADR 0157 already writes, so it is covered by the
same "no truncation" rule: a packet too large for a JPEG segment is still a named
error.

### 4. `verify`: is this file one we wrote?

`Library::verify_export(path) → ExportVerdict`, reading the open library only:

1. **Intact.** The file's BLAKE3 matches an `output_checksum`: the verdict
   names the version, the revision, the date, the recipe.
2. **Modified.** No checksum matches, but the file's `leyline:Revision` matches
   a recorded `revision_fingerprint`: the file came from that revision and has
   been changed since (re-saved, re-compressed, edited elsewhere). The
   revision still exists, because revisions are immutable, even if the photo
   has been edited since.
3. **Unknown to this library.** Neither matches. The verdict says so and
   nothing more: the file may come from another library, or from before
   migration v14.

The verdict never says "authentic". BLAKE3 says the bytes are the same,
not who wrote them. Signing is **out of scope**, and stays a separate
decision: it means a key, where it lives, and what happens when it is lost.

### 5. `reproduce`: does it still come out the same?

`Library::reproduce_export(path, destination) → ReproduceVerdict` finds the
journal row as `verify` does (case 1 or 2). It renders that revision again, with
that row's `export_settings_json`, into `destination`. Then it compares:

* **Bit for bit**: the promise of §5.1 kept.
* **Same pixels, different bytes**: the metadata differs. A caption, a keyword
  or a creator changed since the export (ADR 0157 §4 made them an input). The
  verdict says so and does not call it a failure of the promise. The pixels are
  compared by decoding both files (`image` for JPEG, PNG and TIFF; `image-webp`
  for WebP). **AVIF has no decoder in the tree**, so an AVIF compares by bytes
  only, and says so.
* **Different pixels**: the verdict names the §5.1 condition that differs and
  can be known — the **decoder**, recorded against the one now loaded — and
  otherwise says that the platform or the toolchain is the remaining suspect.
  It does not guess further.

### 6. Clients, at parity

* **SDK**: both verbs and their verdicts re-exported (`tests/surface.rs`).
* **CLI**: `leyline verify <library> <file>` and
  `leyline reproduce <library> <file> <dest-dir>`. The exit code is 0 only for
  *intact* and *bit for bit*.
* **MCP**: `verify_export`, a read with no effect, joins the tools; `reproduce`
  writes a file and follows the rule of ADR 0154 (a tool's effect is a new file).
* **Studio**: the details panel lists a photograph's exports from the journal
  (date, format, destination, recipe). *File ▸ Verify an Export…* picks a file
  and shows the verdict.

### 7. Slices

1. The fingerprint, migration v14, the journal filled; `catalog.md` §28.
2. The `leyline:` block in the packet; `pipeline.md` documents the namespace.
3. `verify`: engine, SDK, CLI, MCP.
4. `reproduce`: engine, SDK, CLI.
5. Studio: the history in the details panel and the verify dialog, on screen in
   English and French.

Each slice ends green and pushed. A real CR2 round trip — export, edit, verify
finds the old revision — is run on the 60D and 5D IV corpus before slice 3 is
called done.

## Consequences

* An exported file leads back to the revision that made it, as long as the
  library that made it is open, and only that library can say so.
* The journal grows six columns per export, two of them 32-byte blobs: about
  200 bytes a row with a typical recipe.
* `export_history` rows from before v14 stay as they were, and `verify` calls
  their files *unknown*. Honest, and it cannot be otherwise: the checksum of a
  file never recorded cannot be recovered.
* Under `WithoutLocation`, the default, an exported file carries its recipe
  and its stage versions. Neither says anything about the photographer or the
  place. They do show that the file came from Leyline, which `Software` already
  shows.

## Alternatives rejected

* **Hashing a re-serialized `Settings`** (the first plan). The intent was a
  canonical form; the effect would be a fingerprint that moves when a later
  build adds a field. The stored string is canonical by construction: it never
  changes.
* **The library UUID in the file.** Linking every published photograph to one
  library is a privacy cost for a convenience the fingerprint already gives.
* **The revision id in the file.** A local integer: meaningless outside one
  library, and it says how many edits came before.
* **A signature.** A key to generate, protect and rotate is a separate
  decision with its own failure modes; a checksum without a signature claims
  integrity and nothing else.
* **Provenance in the file under `None`.** `None` means nothing but the ICC
  profile. A policy that strips everything except one block would be a fourth
  policy in disguise.
* **Storing the metadata snapshot to make `reproduce` bit for bit whatever the
  edits.** It would freeze a copy of every description at every export to
  answer a question the pixel comparison answers already.
