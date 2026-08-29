# ADR 0086 — The decoder is part of the promise

**Status:** Accepted — 2026-08.

## Context

[`pipeline.md`](../pipeline.md) §5.1 states the strongest thing this project
says about itself: two runs produce the same result, bit for bit, if and only
if four conditions hold — identical parameters, identical stage versions,
identical input `checksum`, and the same platform and build toolchain.

**The decoder appears in none of the four.**

That is not a wording slip, and the gap is wider than it looks:

* `crates/leyline-raw/build.rs` probes LibRaw with
  `.atleast_version("0.19")`. Any LibRaw from 0.19 upwards links, silently.
* LibRaw is linked **dynamically** and must stay so — [ADR 0004](0004-libraw-decoding.md)
  reads the LGPL-2.1 substitution obligation as forbidding a static link. The
  decoder is therefore a property of the *machine*, not of the build.
* The reference renders that make §5.1 *mechanically verified* rather than
  promised (`crates/leyline-engine/src/stages/golden.rs`) are built on
  **synthetic images** — the file says so in its own header. No fixture in this
  repository has ever opened a RAW file. The decode path has never been under a
  frozen reference.
* The only tests that decode a real RAW are `#[ignore]`d behind
  `LEYLINE_TEST_RAW`, so they run neither in `make check` nor in CI.

And the three release legs already disagree with each other. `ci.yml` pins
`LIBRAW_VERSION: 0.21.4` for the cross-built Windows deliverable; the Linux leg
installs `libraw-dev` and takes whatever the distribution ships (0.21.2 on
Ubuntu 24.04); the macOS leg takes whatever Homebrew has that day. **One
version of Leyline can ship three decoders**, and nothing in the tree notices.

This surfaced on 2026-08-29, and how it surfaced is the argument. A local
LibRaw moved 0.20 → 0.21; a stale test binary still wanted `libraw_r.so.20`
and `make check` failed at load time with `cannot open shared object file`.
The *linker* shouted. Had the soname not changed — a minor upgrade, the common
case — nothing would have shouted, and the pixels would simply have been
different.

The tempting fix is to call LibRaw part of "the platform" and let §5.2 absorb
it. That would be false. §5.2 covers `powf`, `ln` and `exp`, and it promises
renders "visually identical, up to a last-bit drift". A demosaic change is not
a last-bit drift: between LibRaw releases, AHD interpolation, highlight
recovery and per-camera white levels have all moved by amounts a photographer
can see. Filing a visible change under a clause about floating-point noise
would use the honesty of §5.2 to hide something it was not written to cover.

## Decision

The decoder is named where it belongs — in §5.1, as a condition — and a change
of decoder is made **loud** rather than silent.

### 1. §5.1 gains a fifth condition

The conditions become: identical parameters, identical stage versions,
identical input `checksum`, the same platform and build toolchain, **and the
same decoder at the same version**.

This is the honest statement, and it costs the promise nothing it actually
had: a reader who upgrades LibRaw is told, up front, that they have changed
one of the terms. The alternative — a promise that quietly excluded the one
component that turns a file into pixels — was not a stronger promise, only a
vaguer one.

### 2. The decoder's version is readable at runtime

`leyline_raw::decoder_version()` returns what LibRaw reports about *itself* at
run time (`libraw_version()`), not what `build.rs` found at compile time. With
dynamic linking those two can differ on the very same machine, and the one
that determines the pixels is the one that answers at run time.

It is surfaced where a user can quote it in a bug report: `leyline --version`
and Studio's About dialog.

### 3. A decoder change fails `make check`, and blessing it is a deliberate act

`crates/leyline-raw/tests/decoder.txt` lists the decoders this tree has been
validated against — plain text, one version per line, because it holds strings
and a `serde_json` dependency bought for a list of strings is bought for
nothing. A test asks whether `decoder_version()` is among them and, when it is
not, fails with a message that says what happened and what to do — not
`assertion failed`.

**A set, not a single value**, and that is forced by §5 below rather than
chosen for comfort: Debian, Homebrew and the cross-built Windows leg each
bring their own LibRaw, so recording one and calling it *the* decoder would
state something untrue about the deliverables. What the set still buys is the
whole point — a decoder nobody has looked at cannot get in quietly — and what
it honestly concedes is that two entries are two renders not promised to
agree.

Blessing is `LEYLINE_BLESS_DECODER=1`, deliberately spelled like
`LEYLINE_BLESS_GOLDEN=1`, and it *appends*, exactly as blessing a golden
render never rewrites an existing entry. It means the same thing: *a human
looked, and accepts this*.

This guard covers the **trigger**, completely and on every machine, with no
fixture. It does not by itself observe whether pixels moved — §4 is what does
that, for whoever can.

### 4. Real RAW decodes are pinned by whoever has a corpus

The `LEYLINE_TEST_RAW` tests gain a manifest keyed by the **checksum of the
input file** rather than by its name, holding the checksum of its decoded
output. First run over a given file records it; later runs verify it.

Keying by content is what makes it work without shipping a fixture: any
maintainer with any RAW file builds their own reference set, and it stays
valid across machines and checkouts. This repository ships the mechanism and
no entries.

### 5. The build floor rises to what is actually tested

`.atleast_version("0.19")` becomes `.atleast_version("0.21")`. 0.19 and 0.20
were never tested and are no longer probed for; a machine holding one now
fails at build time with a message that names the requirement, instead of
producing a binary whose renders nobody has checked.

No upper bound is imposed. An upper bound would make a routine distribution
upgrade fail the *build*, and §3 already turns that same upgrade into a
legible test failure — which is the right severity for "this needs looking
at" as opposed to "this cannot work".

**The legs are not made to converge here, and that is deliberate.** Pinning one
LibRaw across Linux, macOS and Windows means building it from source on two
more legs; the macOS half of that cannot be validated by anyone on this project
today, for the same reason the `.dmg` has never been built (no machine). Naming
a single version in this ADR and leaving CI to install something else would be
a decision that documents itself as done while not being done. So the set of
§3 records the truth — several decoders, each accepted deliberately — and
convergence stays an open item rather than a claim.

## Consequences

* One new public function in `leyline-raw`, and the crate keeps its ADR 0004
  property that no LibRaw type reaches its API: a version string is not a
  LibRaw type.
* `make check` fails on any machine whose LibRaw is not among the accepted
  ones. That is the point, and it is a two-second fix for a contributor who
  accepts the change.
* The three release legs still carry three decoders — Windows pins 0.21.4, the
  Linux and macOS legs install what their package manager offers. Each one now
  has to be *accepted* to pass `make check`, which is the change; converging
  them on a single build is left open (§5).
* `pipeline.md` §5.1 and §5.2 change, and `contributing.md` gains the blessing
  gesture beside the golden one.
* Nothing about `settings_json`, stages or stage versions changes, so no
  existing revision is touched and no migration is needed.

## Alternatives rejected

* **Call LibRaw part of "the platform" and leave §5.2 to absorb it** — one
  sentence, no code, and it would file a visible change under a clause about
  last-bit floating-point drift. Rejected on honesty, which is the only reason
  §5 is worth anything.
* **Make the decoder version part of the `input` stage's identity**, as
  [ADR 0080](0080-the-promise-and-its-boundary.md) §3 does for the execution
  backend. Consistent in shape, unusable in practice: a revision citing
  `libraw 0.21.2` would *refuse to render* on a machine that upgraded to
  0.21.4, and every distribution upgrade would strand every photograph in the
  catalog. The backend case works because a backend is a build-time choice
  under the project's control; the system decoder is neither.
* **Static-link or vendor LibRaw so the version is fixed by the build** —
  forbidden by [ADR 0004](0004-libraw-decoding.md)'s reading of LGPL-2.1.
* **A synthetic RAW fixture, giving the decode path its own golden render** —
  the right answer in principle, and attempted on 2026-08-29: a hand-built
  minimal uncompressed CFA DNG, tried both with the image in a SubIFD and
  directly in IFD0. LibRaw's identification refuses both
  (`LIBRAW_FILE_UNSUPPORTED`) while accepting a real camera DNG through the
  same harness. Making LibRaw accept a hand-built file is a project of its
  own, and it would in any case exercise the uncompressed-DNG path rather than
  the compressed paths real photographs take. Left open; §4 is what stands in
  for it.
* **Ship a real RAW as a fixture** — licence and repository size, and it
  would pin one camera's path as though it were the decoder.
* **Replace LibRaw with a Rust decoder (`rawler`)** — a whole project, and
  `leyline-raw`'s API was deliberately shaped to keep that door open
  (`lib.rs` says so). Not this decision.

## What this ADR does not do

* **It does not promise identical pixels across decoder versions.** It promises
  that you will be told. The pixels of LibRaw 0.21.2 and 0.21.4 may differ;
  what changes is that the difference has a name and a place in the contract.
* **It does not add a RAW fixture to the repository**, and does not make the
  decode path part of `golden.rs`.
* **It does not re-validate the existing catalog.** Photographs imported
  before this ADR keep their previews and their revisions; nothing is
  re-decoded and nothing is marked stale.
* **It does not make the three deliverables share one decoder.** It makes each
  of them declare which decoder it carries, and refuses the undeclared. The
  convergence itself needs a macOS machine this project does not have.
