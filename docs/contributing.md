# Contributing

This document is the complete guide. The repository root carries a [`CONTRIBUTING.md`](../CONTRIBUTING.md) — GitHub looks for it there — which is its summary and points back here.

## Philosophy

* Readability before optimisation.
* No dead code.
* Public APIs must be documented.
* Tests for every feature.
* Layered architecture.
* No circular dependencies.

## Style

* rustfmt
* clippy with no warnings
* green CI required.

## Commands

A `Makefile` at the root gathers what comes up often — `make` on its own lists the targets. Nothing in it is mandatory: each target is only a wrapper around a `cargo` invocation or a script in `packaging/`, and everything stays runnable by hand. The point is to keep the exact options in a single place, several of them being easy to misremember.

The only one to know by heart:

```bash
make check      # fmt + clippy + tests — to be run before every commit
```

A GitHub Actions CI ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) replays those three steps — `fmt`, `clippy -D warnings`, `test --workspace` — on Linux and macOS, on **every `v*` tag**, on manual dispatch (`workflow_dispatch`), and — Linux leg only — on **every pull request**, but on no ordinary push. The reason: `make check` already runs before every commit, so replaying the same three steps on every push added nothing but a second Linux opinion. A pull request is the exception, because an external contributor's `make check` ran on a machine the reviewer cannot see — and it gets the Linux leg alone, so a PR never waits on a macOS runner to be reviewable. What CI alone can say is whether macOS still builds and whether the Windows deliverable still cross-compiles — a publication question, not a commit question. Any change touching a system dependency, a build script or the toolchain does deserve a manual dispatch without waiting for the tag. **Windows is not a matrix leg**: a native build cannot work today, `leyline-raw` and `leyline-tether` finding LibRaw and libgphoto2 through pkg-config only, neither of which exists on a Windows runner. What ships there is cross-compiled from Linux ([ADR 0019](adr/0019-distribution-i18n.md)), so that is what the `windows-cross` job builds — the real installer, and a result that can be green. It is kept as a run artifact (`leyline-studio-windows-x64`), downloadable from the run's own page: that is how a build gets tested on a real Windows machine without a tag and without publishing anything. The `macos-package` job does the same for the macOS `.dmg` (`leyline-studio-macos-arm64`, built by `packaging/macos/build-dmg.sh` with the pinned LibRaw) — the runner being the only macOS host the project has; the `.dmg` is not yet self-contained (nothing copies the pinned dylibs into the `.app`), and no one has launched it on real hardware, both written down in the job rather than implied. **CI publishes no release** — the signing key lives outside this repository and outside CI ([ADR 0077](adr/0077-application-updates.md) §1), and publishing stays `packaging/release-manifest.sh`, run by hand. In every case CI arrives after the fact: `make check` remains what separates a mistake from `main`.

The others, as needed: `make run` / `make cli` (with `ARGS=…`), `make golden` and `make golden-bless` (reference renders, next section), `make test-raw LEYLINE_TEST_RAW=…` (the ignored tests that require a real RAW file), `LEYLINE_TEST_DCP=…` (a folder of real `.dcp` profiles, unversioned: third-party works of unknown licence), `make bench`, `make i18n` (see below), and `make windows` / `make appimage` / `make dmg` for the packages ([ADR 0019](adr/0019-distribution-i18n.md)). A publication adds `make release-manifest VERSION=x.y.z NOTES=<file>`, which signs the built packages and writes the `latest.json` the release must carry ([ADR 0077](adr/0077-application-updates.md) §1) — the private key lives outside the repository, and `LEYLINE_SIGN_KEY` says where.

## Adding or fixing a render stage

This is the most constrained contribution in the project, because it is the one that touches the "same pixels ten years from now" promise ([`pipeline.md`](pipeline.md) §5.1). The *what* is specified in §3.3 of that same document; here is the *how*.

Everything lives in `crates/leyline-engine/src/stages/`: one module per operator version (`sharpen/v1.rs`), the `STAGES` registry that composes them, and `golden.rs` that freezes them.

### The single rule

> Once published, a stage version never moves. Not its body, not its rank, not the `apply` binding that names it in `STAGES`.

Fixing a render therefore means **adding** `v2`, never editing `v1`. An optimisation that produces exactly the same bytes is not a render change and stays in `v1`.

### Fixing an existing operator

1. Create `stages/<operator>/v2.rs` and declare it in `stages.rs`. Starting from a copy of `v1.rs` is the norm, not an admission of failure: duplication is the price of the freeze ([ADR 0042](adr/0042-versioned-stage-pipeline.md)).
2. Add a `Version` entry **at the end** of that stage's `versions` array — the last one is what the engine pins for new revisions.
3. Choose its rank: the same as `v1` if the position does not change, a free rank between two tens otherwise. Moving an operator is that choice, not an edit of the existing rank.
4. Mind the shared body: several stages go through `kernel::v1`. Fixing it would move the render of all of them. A fix there creates `kernel::v2`, which only new stage versions call.
5. Test both versions in `stages/tests.rs`: what `v2` does better, and that `v1` still does what it did.

### Adding an operator

1. The setting first, in `leyline-core`: a `Settings` field, a neutral value in `Default`, bounds in `validate()`, documentation. An optional parameter with a neutral value does not increment `schema` (`pipeline.md` §3.4); a change of type, unit or range does.
2. A `Stage`'s `active` predicate reads **nothing but** the settings — never the image, the camera body or the resolved profile. It decides both what runs and what a revision records, and a revision is written without an image in hand. Unavailability at render time (no Lensfun profile, no DCP) is handled in `apply`, by leaving the pixels as they are.
3. A stage at its neutral value does not run and does not appear in the `stages` map. That is what makes a neutral render bit-for-bit identical to the decoded image.
4. A radius expressed in pixels is multiplied by `ctx.scale` ([ADR 0041](adr/0041-interactive-preview-rendering.md)), without which the preview and the export will not show the same effect. Anything normalised to `[0, 1]` ignores it.
5. Determinism: no clock, no `HashMap` iteration order, no cross-thread floating-point reduction — parallelisation is done over disjoint rows ([ADR 0012](adr/0012-rayon-data-parallelism.md)).
6. A structural decision is written as an ADR **before** the code, and the row in the table of `pipeline.md` §3.3 is part of the same change.

### The reference renders

`stages/golden.rs` pins, in `tests/golden/renders.json`, the BLAKE3 checksum of one render per operator family **and the `stages` map that produced it**. Each entry is replayed through its own map: a `v2` therefore cannot move an existing checksum, it adds one.

Three guards:

* every pinned entry still renders exactly its pixels;
* what the engine would pin today appears in the manifest;
* no published `(stage, version)` pair escapes it — an operator that no case activates makes the tests fail.

```bash
make golden          # verify
make golden-bless    # add the missing entries
```

The first of those guards runs only on the reference platform, the one where the checksums were blessed: comparing bytes across platforms would promise what [`pipeline.md`](pipeline.md) §5.2 refuses to assert. The other two run everywhere.

Blessing is **additive**: it never overwrites an existing entry. If a checksum already in the manifest changes, that is a defect — frozen code has been touched — and it is fixed in the code, not in the manifest. The cases themselves are frozen for the same reason: exercising an operator differently is a new case.

### The decoder

The reference renders above are built on **synthetic images**, so nothing in them ever opens a RAW file: the decode path is not under a frozen reference, and it cannot be — LibRaw is linked dynamically ([ADR 0004](adr/0004-libraw-decoding.md)), so it is a property of the machine rather than of the build. [`pipeline.md`](pipeline.md) §5.1 nevertheless counts it among the terms of the bit-for-bit promise ([ADR 0086](adr/0086-decoder-in-the-promise.md)), so it is guarded in the two ways that are available:

* `crates/leyline-raw/tests/decoder.txt` lists the decoders this tree has been validated against, and `make check` fails on one that is not in the list. It is a **set**, because Debian, Homebrew and the cross-built Windows leg each bring their own, and calling any one of them *the* decoder would be untrue.
* `crates/leyline-raw/tests/decodes.json` pins actual decoded pixels, keyed by the **checksum of the input file** so anyone can build a reference set out of their own photographs. It ships empty, and `make test-raw LEYLINE_TEST_RAW=…` fills it: the first run over a file records it, later runs verify it.

```bash
LEYLINE_BLESS_DECODER=1 cargo test -p leyline-raw --test decoder_version
```

Blessing **appends**, like the golden one, and it means the same thing: someone accepted that this decoder may render differently from the others already listed. Reach for it when `make check` reports a decoder it does not know — after checking, if you have RAW files, whether any pixels actually moved.

## Touching Studio's interface

How the files are divided is described in [`architecture.md`](architecture.md#inside-studio). Three rules are added on top, two of which are easy to break without noticing.

### UI state — global or local?

> **A `global` carries only the state that crosses the Rust ↔ UI boundary. State that concerns a single panel stays a private property of that panel.**

This is the rule that keeps `ui/state/` from becoming again the flat surface of 111 properties that ADR 0045 took apart. A collapsed accordion, the active drag tool, the open menu: Rust never reads them, so they have no business in a global. If a panel must nonetheless expose something to the window, it does so through its own surface — an `in-out` property, a `public function`, a `callback` — and not by widening a global.

The test is mechanical: if no `.get_x()`/`.set_x()`/`.on_x()` on the Rust side matches the property, it does not belong in `ui/state/`.

### Never put geometry around content

Slint 1.13 has, in this project, a repaint-scheduling defect: giving `keys` (the `FocusScope` in `studio.slint`) or an ancestor of its repeaters — the grid, the collection list — **a position or size override, even an inert one**, is enough to leave white areas on screen until a structural change forces a refresh. That is why panels inherit the element type they replace and set no geometry at all, and why a column that must clear the height of the menu bar does so with an extra spacer `Rectangle` child. The comment above `menu-row` in `studio.slint` details the diagnosis.

### The window must never inherit a maximum from its content

Slint derives a window's constraints from what it contains, and the winit
backend passes them on to the window manager. A column made of fixed-height
rows therefore announces a bounded **maximum**, which travels up to
`StudioWindow` and becomes a `program specified maximum size`: the window can
no longer be maximised, and every recomposition that changes that bound —
opening a dialog, closing one, creating a collection — re-applies it, which
abruptly snaps a maximised window back to the size of its content.

For that reason `StudioWindow` declares a deliberately enormous
`max-width`/`max-height`: it is the only way to express "no maximum" in
Slint 1.13, and it neutralises the whole class of regressions.

To check, under X11:

```bash
xprop -id "$(xdotool search --name 'Leyline Studio' | head -1)" WM_NORMAL_HINTS
```

A `program specified maximum size` other than the declared one means a panel
has started constraining the window again.

### The translation catalog goes stale in silence

`slint-tr-extractor` records the file and line of every `@tr(...)`, and the `msgctxt` is **the component's name**. Moving a string from one component to another therefore changes its key and detaches it from its translation, without the slightest warning. After any interface slice:

```bash
make i18n
```

then carry the existing translations over into `translations/fr/LC_MESSAGES/leyline-studio.po`. Verify by launching Studio with `LANG=fr_FR.UTF-8`: a catalog that compiles is not a catalog that translates.

## Terminology

The documentation and the code use one word per concept, always the same one. The ones that matter, because a synonym would make a document ambiguous:

| Term | Means |
|---|---|
| **stage** | One versioned operator of the pipeline (`stages/<operator>/vN.rs`) |
| **stage version** | The frozen body of a stage, pinned by a revision |
| **revision** | The complete, self-contained state of the settings of a version |
| **version** | A library unit: a virtual copy of an asset ([ADR 0008](adr/0008-version-as-library-unit.md)) |
| **asset** | An imported file, with its row in the catalog |
| **preview** | A developed image kept in cache; a **thumbnail** is the smallest class |
| **proxy** | A source buffer reduced to display size, before development |
| **library** | The self-contained folder holding a catalog and its caches |
| **coverage** | The grey image a mask resolves to, per pixel |

## Commits

Conventional Commits. Commit messages are written in French, as the project's history is; the documentation and the code are in English.

## Licence and contributions

Leyline is published under **GPL-3.0**.

Commercial licences will be offered in time: Leyline follows a dual-licence model (Qt-style), the community version staying entirely GPL.

To make that model possible, every contribution is subject to the **CLA** (Contributor License Agreement) described in [`CLA.md`](../CLA.md): the contributor grants the project the right to distribute their contribution under other licences, while keeping their copyright.

By submitting a pull request, you accept the terms of the CLA.

The name "Leyline", the logo and the trademark remain the property of the project and are not covered by the code's licence — see [`TRADEMARK.md`](../TRADEMARK.md).

## Code of conduct and security

Contributors and participants in issues and PRs are bound by the
[`CODE_OF_CONDUCT.md`](../CODE_OF_CONDUCT.md).

Security vulnerabilities are reported privately, not through a public
issue — see [`SECURITY.md`](../SECURITY.md).

## Dependencies

Third-party works that are **embedded or linked** — the ones you do not discover by reading the manifests — are listed with their licence in [`THIRD-PARTY-NOTICES.md`](../THIRD-PARTY-NOTICES.md), at the root. Adding an entry there is part of the change that embeds something, not of a later tidy-up.

* LibRaw is used under its **LGPL-2.1** branch (the CDDL branch is incompatible with the GPL).
* Lensfun (LGPL-3.0) and its database (CC-BY-SA) require attribution.
* RAW decoding is isolated behind the `leyline-raw` API so that it stays substitutable.

## Goal

To build a photographic engine that lasts, not merely an
application.
