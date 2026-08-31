# ADR 0105 — Making a detector writable: conformance, and the CLI half of the socket

**Status:** Accepted — 2026-08

## Context

ADR 0073 built the socket and named the first detector: `leyline-assist`,
an executable in a private repository, weights never in the open one. The
socket works — Studio discovers manifests, runs a detection and stores
the coverage as an ordinary mask.

Two things stand between that and someone actually writing a detector,
and both live in *this* repository.

**Nothing but Studio can invoke one.** The CLI has no detector surface at
all, so a detector author debugs by clicking, and a script cannot run a
detection. That is out of step with how everything else here is
delivered — ADR 0049 §4 gave the CLI the local-adjustment payload for
exactly this reason.

**Nothing says what a *correct* detector is.** ADR 0073 §2 fixes the call
and the answer's format in prose. A prose contract with no way to check
it is a contract each implementer interprets alone, and the first time
the interpretation differs it will be a user's mask that is wrong.

## Decision

### 1. The reader of a detector's answer moves into the socket's crate

Turning the PNG a detector wrote into coverage samples lives today in
`leyline-studio` (`masks::coverage_from_image`). It is not interface
code: it is **the second half of the protocol**, the part that says how
an answer is read — grey via Rec. 709 luma when the image is opaque, the
alpha channel when it is not.

It moves to `leyline-detect`, beside the invocation it belongs to, and
the crate gains `detect_coverage` — run, read, return the samples — so a
client handles none of the protocol's plumbing at all. Studio's own
detection path shrank to two calls, and the CLI needs neither `image` nor
`tempfile` to speak the protocol. A second client no longer has to reimplement the
one rule that decides what a detector's output *means*, which is the
rule two implementations would most quietly disagree about.

### 2. The CLI gains the socket's three verbs

* `leyline detectors` — what is installed, with each detection's id.
  Discovery has no library and needs none: a detector is per-user and
  per-machine (ADR 0073 §3).
* `leyline detect <library> <version-id> <detector:detection>` — runs it
  on the version's preview and appends a local adjustment carrying the
  returned coverage. The same gesture Studio performs, with the same
  neutral values: the detection chose *where*, the photographer still
  chooses *what*.
* `leyline detect-check <detector:detection>` — the conformance harness
  of §3. No library, no photograph of the user's: it makes its own image.

All three take `--from <dir>` to read manifests somewhere other than the
user's configuration directory. That is not a testing hook that leaked
into the interface: it is how a detector author tries an executable
*before* installing it, and `leyline-detect` already exposed
`discover_in` for exactly that ("what a packager would use to look
somewhere else"). It also makes the CLI's own end-to-end test possible,
which a per-user directory otherwise forbids.

### 3. What "conformant" means, checkable

`detect-check` runs the detector against a synthetic image and reports,
in order:

1. **it ran and wrote its file** — spawned, exited zero, within
   ADR 0073's timeout, and left a coverage at the `--out` path. Both are
   already enforced by `detect` itself, whose error is passed through
   rather than re-checked: it names the path it handed over, which a
   second check here could only say worse;
2. **the file is readable** as an image;
3. **the dimensions match the input** — a coverage is per-pixel, and one
   of another size cannot be applied to the photograph it describes;
4. **the samples span something** — a coverage that is uniformly 0 or
   uniformly 65535 is *valid* and *useless*, so it is reported as a
   warning rather than a failure. It is also exactly what a detector
   returns when its model did not load.

Point 4 is the one worth having. The others fail loudly on their own; a
detector that silently returns an empty mask looks like a working
detector that found nothing, and a photographer would blame the picture.

The harness checks the **protocol**, never the quality of a segmentation:
whether the sky it found is the sky is between the detector and its
author, and no fixture in this repository could referee it.

## Consequences

* No engine change, no stage, no schema: `docs/pipeline.md` §5.1 is not
  in play, as ADR 0073's own consequences already established.
* A detector author can now write, run and validate an executable
  without opening Studio — which is what makes `leyline-assist`
  buildable by someone who is not sitting at this repository.

## Rejected

* **A reference detector in-tree** — it would need weights, and ADR 0073
  §6 keeps weights out of the open repository. The synthetic image of §3
  checks the protocol without pretending to detect anything.
* **Failing on a uniform coverage** — §3.4: it is legal output, and a
  harness that refuses legal output teaches people to ignore it.
* **Judging segmentation quality** — §3, last paragraph.
