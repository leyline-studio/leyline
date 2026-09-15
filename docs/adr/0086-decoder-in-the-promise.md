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

**A set, not a single value.** The deliverables now share one decoder (§6),
so the set no longer describes them; what it still describes is the machines
this tree is *built and tested* on, where a contributor's `apt install` and a
packaging run legitimately differ. A decoder nobody has looked at still cannot
get in quietly, which is the whole point, and each entry beyond the first has
to earn its place — by measurement (§4) rather than by convenience.

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
valid across machines and checkouts.

The repository ships **ten entries** — Canon 60D and 5D Mark IV CR2s, and
DNGs, from the author's corpus. They are what turned "0.21.2 and 0.21.4 are
probably the same" into a fact (§6), and they are committed so that claim can
be replayed rather than believed. Anyone without those files simply has a
manifest whose keys never match, which costs them nothing.

### 5. The build floor rises to what is actually tested

`.atleast_version("0.19")` becomes `.atleast_version("0.21")`. 0.19 and 0.20
were never tested and are no longer probed for; a machine holding one now
fails at build time with a message that names the requirement, instead of
producing a binary whose renders nobody has checked.

No upper bound is imposed. An upper bound would make a routine distribution
upgrade fail the *build*, and §3 already turns that same upgrade into a
legible test failure — which is the right severity for "this needs looking
at" as opposed to "this cannot work".

### 6. The deliverables carry one decoder — **2026-08-30**

Left open when this ADR was first written, and closed two days later because
the first alpha made it concrete: one tester on Windows, one on Linux, and no
answer to "should we expect the same pixels?".

**Every deliverable now carries LibRaw 0.21.4, built from source** (0.22.2
since §7). The Windows installer bundles the cross-built `libraw_r-23.dll`
(`-25` since §7); the AppImage bundles the matching `.so` from a pinned
prefix, and both packaging scripts refuse to run without it. All three CI
legs build that same tag rather than installing one, which is what the pin
is for: left to the package managers
the same release carried **three** decoders — Ubuntu 0.21.2, the pinned
Windows 0.21.4, and Homebrew **0.22.2**, a different *minor* version that
would have failed §3's guard on the macOS leg the moment it ran.

The one thing that did *not* change is the reason the pin is a version rather
than a promise: 0.21.2 stays in §3's accepted set, because it is what a
contributor gets from `apt install libraw-dev` and requiring the pinned prefix
merely to run the tests would tax a first contribution for nothing. It is
listed on **evidence**: the two were compared on ten real RAW files — Canon
60D and 5D Mark IV CR2s, and DNGs — through §4's manifest, and produced ten
identical digests at identical dimensions. Ten files are not a proof for every
camera LibRaw supports; they are why that line is a measurement rather than an
assumption, and the entries are committed so it can be replayed.

That measurement also found a defect in §4's own test. It compared whole
manifest entries, the recorded decoder version among them — so it could never
pass across two decoders, *even when the pixels were byte-identical*, which is
precisely the case it exists to distinguish. It now compares the pixels and
their shape, and reports the decoder as context. A guard that cannot tell its
own two failure modes apart is worse than no guard: it teaches people to
re-bless on sight.

### 7. The pin moves to 0.22.2 — **2026-09-15**

The pin was a version, not a promise to stay on it, and 0.21.4 had stopped
being a defensible one. LibRaw 0.22.1 (2026-04) fixed a series of reported
vulnerabilities — integer overflows in the floating-point DNG loader and the
X3F decoder among them (TALOS-2026-2330, -2331, -2358, -2359, -2363 and
-2364) — and 0.22.2 (2026-07) a further set: buffer overruns, a stack memory
exposure, unbounded parser recursion. Those are the
paths a photographer's files take at import, and the 0.21 branch does not
carry them: its last release, 0.21.5, predates them.

**Every deliverable now carries 0.22.2, built from the same tag on the three
legs.** The library's own ABI number moved with it: `libraw_r.so.25`,
`libraw_r-25.dll`, `libraw_r.25.dylib`, where 0.21.x was 23. The Windows
DLL's imports are unchanged (`zlib1`, `libgcc_s_seh-1`, `libstdc++-6`), so
the installer bundles the same set. The C shim built unchanged.

What moving costs was measured rather than assumed, through §4's manifest.
The tenth pinned file — a 5D Mark IV CR2 — could not be found again in the
corpus; the other nine were decoded with 0.22.2 against the digests recorded
on 0.21.2, then compared pixel by pixel with 0.21.4:

| File | Pixels that differ | Largest difference |
|---|---|---|
| three 60D CR2s | 0 | — |
| 60D CR2 | 35 of 10,077,696 | 1 level |
| 60D CR2 | 254 of 10,077,696 | 1 level |
| 5D Mark IV CR2 | 512 of 30,361,488 | 7 levels |
| 5D Mark IV CR2 | 2,498 of 30,361,488 | 6 levels |
| DNG | 186 of 12,000,000 | 12 levels |
| DNG | 499 of 12,000,000 | 6 levels |

8-bit sRGB output, levels out of 255. The differing pixels are scattered
through the frame rather than on its borders, and no channel's mean moves.
Not the same bytes, then — §5.1 is broken exactly as §1 says it can be, and
said so here — but the same image. Nothing in the 0.22 changelog touches
colour matrices, demosaicing or white levels for these bodies; the cause of
the scattered pixels was not traced, and this ADR does not claim one.

The six entries that moved were re-recorded on 0.22.2, which is the decoder
the manifest now describes; the three identical ones and the unfound tenth
keep their 0.21.2 provenance, which is still true of their pixels.

Two consequences for §3's set. **0.21.4 leaves it**: no leg builds it any
more, and a machine still holding that prefix should hear about it. **0.21.2
stays**, for §6's reason — it is what `apt install libraw-dev` gives a
contributor — but now with its cost written beside it: a test run on 0.21.2
validates the code, not the shipped pixels, and `make test-raw` on it reports
the six files above.

**The move itself nearly shipped the wrong decoder, silently.** The first
AppImage built on 0.22.2 carried `libraw_r.so.23` — the *system's* 0.21.2 —
in a package meant to carry the pin, and nothing failed; it was found by
listing what the AppImage contained. The cause is the link line, reproduced
on a two-dependency crate: `PKG_CONFIG_PATH` puts the pinned prefix in
`leyline-raw`'s search paths, but `lcms2-sys` adds `/usr/lib/x86_64-linux-gnu`
on its own, and on a machine that also has `libraw-dev` installed the linker
resolves `-lraw_r` there. The same crate linked with the prefix passed through
`RUSTFLAGS` gets `.so.25` and reports 0.22.2. This was already true under
0.21.4: both libraries had soname 23, so the bundler and the loader picked
the pinned file by path and the About dialog said 0.21.4 — while the link
had been made against the system one. CI runners have no `libraw-dev`, so
their builds were not affected.

Two guards follow. The Linux and macOS packaging scripts pass the pinned
prefix through `RUSTFLAGS`, which puts it first on the link line. And the
scripts that run here check, after building, that the soname the binary asks
for (`readelf -d` on Linux, `objdump -p` on the Windows exe) is the one the
pinned prefix provides, and refuse to package otherwise — the refusal that
caught the second attempt, before `RUSTFLAGS` was added.

## Consequences

* One new public function in `leyline-raw`, and the crate keeps its ADR 0004
  property that no LibRaw type reaches its API: a version string is not a
  LibRaw type.
* `make check` fails on any machine whose LibRaw is not among the accepted
  ones. That is the point, and it is a two-second fix for a contributor who
  accepts the change.
* The release legs carry **one** decoder, built from source on each (§6) —
  LibRaw 0.21.4 from 2026-08-30, 0.22.2 since 2026-09-15 (§7). Building it
  is now a prerequisite of packaging, like the mingw prefix already was for
  Windows.
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
* **It does not make renders comparable across decoder *versions* in general.**
  The deliverables now share one (§6), and 0.21.2 and 0.21.4 were measured
  identical on ten files — neither fact promises anything about a version
  nobody has compared.
