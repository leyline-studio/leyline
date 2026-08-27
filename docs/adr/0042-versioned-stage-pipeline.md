# ADR 0042 — A pipeline composed of versioned stages: the freeze bears on the operator, no longer on the whole version

**Status:** Accepted — 2026-07
**Supersedes:** ADR 0028 (one process version per feature, per-module duplication)

## Context

ADR 0028 froze the current convention: one process version per pixel feature,
each in its own `processN.rs` module, a **complete copy** of the previous one.
Its last consequence explicitly provided for its own reopening:

> Should the number of modules one day become a real burden — a number well
> above today's five, after several V2 features have actually shipped — a
> future ADR can reopen the question **with real data**.

That data now exists.

**Volume.** ADR 0028 measured 3,433 lines across five modules. Today:

| Module | Lines | | Module | Lines |
|---|---|---|---|---|
| `process1.rs` | 471 | | `process7.rs` | 1,384 |
| `process2.rs` | 580 | | `process8.rs` | 1,633 |
| `process3.rs` | 750 | | `process9.rs` | 2,048 |
| `process4.rs` | 804 | | `process10.rs` | 2,556 |
| `process5.rs` | 954 | | `process11.rs` | 2,671 |
| `process6.rs` | 1,117 | | **Total** | **14,968** |

**Duplication rate.** Each module is identical to its predecessor at **70–93 %**
(strictly identical lines) — `process11.rs` is 93 % identical to
`process10.rs`: 2,482 lines out of 2,671.

**The real nature of the versions shipped.** `docs/pipeline.md` §3.3's table is
unambiguous: **ten of the eleven versions** are defined by the formula
"Identical to N−1, **plus** X". Only one — process 2, the table-based transfer
functions (ADR 0013) — modified the rendering of *already existing* settings. In
other words: **ten complete copies of the pipeline were paid for changes that
could affect no existing photo.** A photo with no `dehaze` value renders exactly
the same whether `dehaze` exists in the engine or not.

**The maintenance cost, observed.** ADR 0041 (proxy rendering) had to send a
single scale factor down to the radii expressed in pixels. The same three-line
change had to be applied **eleven times**, plus 33 cross-module test calls and
eleven documentation blocks. `process3.rs`'s sharpening code is byte for byte
`process9.rs`'s: modifying it eleven times brings no guarantee that modifying it
once would not.

**What is not at issue.** The promise itself — *this RAW, these settings, these
pixels, in ten years* (`docs/pipeline.md` §3.3, §5) — is neither weakened nor
renegotiated here. It is **behavioural**. Complete duplication was only one
*possible implementation* of it, never its statement.

**What is at issue, however: its scope.** The original §5 promised a result
identical "to the pixel" without naming the platform. Yet the pipeline calls
`powf`, `ln` and `exp`, which come out of the system's libm: their last bit
changes from one platform, one libm version or one LLVM to another. The promise
was therefore, as written, untenable — not through a lack of rigour, but
because no engine holds it: Lightroom does not give the same pixels on its GPU
and CPU paths, and darktable migrates old modules' parameters to the current
code (`legacy_params`) instead of freezing that code. §5 is therefore split in
two: what is guaranteed (§5.1) and what is not (§5.2).

It must be stressed that these two relaxations are **independent**, and that
only one is retained. Giving up cross-platform exactness is *forced* by
floating point. Giving up the code freeze, in darktable's manner, would be a
*choice* — and the present ADR makes it unnecessary: what made freezing costly
was copying 2,700 lines per feature, not the freeze itself. Once the stages are
composed, `sharpen::v2` weighs a few dozen lines beside `sharpen::v1`. We
therefore abandon the physically impossible guarantee, and keep the one that no
longer costs much.

## Decision

The pipeline stops being a sequence of duplicated version modules. It becomes
the **composition of independently versioned stages**.

### 1. The unit of freezing is the operator, not the pipeline

Each operator lives in its own versioned module — `sharpen/v1.rs`,
`dehaze/v1.rs`, `tone_curve/v1.rs` — **frozen on the day it ships**, exactly as
a `processN.rs` is today.

That is the point that answers ADR 0028's decisive objection ("shared code is
precisely what exposes a frozen rendering to the risk that a future, unrelated
change silently alters it"). That objection targets the **sharing of a mutable
implementation** across several versions. That is not what the present ADR
describes: `sharpen::v1` is not a shared, modifiable implementation, it is a
module frozen on the same footing as `process3.rs`. Fixing sharpening produces
`sharpen::v2`; `v1` is **never** touched. The guarantee stays mechanically
unfalsifiable, identically — what disappears is only the re-freezing of eleven
copies of an operator that did not change.

### 2. A revision records the version of the stages it uses

The `process` field ceases to be the versioning axis. A revision records the
version of each **actually active** stage:

```json
"stages": { "exposure": 1, "tone_curve": 1, "dehaze": 1 }
```

A **neutral stage does not run** — that is already the engine's behaviour, each
stage being skipped when its setting is at its neutral value. It therefore has
no behaviour to pin and **does not appear** in the map. The map is by
construction proportional to the real editing: three entries for a lightly
retouched photo, fifteen or so for a heavily worked one.

That choice makes the revision **self-describing**: nothing is inferred from a
correspondence table on the engine side, so no global counter reintroduces
itself by the back door.

### 3. The position in the pipeline is a property of the stage version

The stages' order is observable state: nearly all our features have inserted
themselves **in the middle** of the pipeline. Each stage version therefore
declares its own rank (`sharpen::v1` at rank 90). Moving a stage is not a
modification of an existing version but a **new version** declaring another rank
— revisions referencing `v1` keep rank 90.

The order thus becomes reproducible again **without** a global layout version.

### 4. Adding a feature touches nothing existing

A new stage = a module plus a registry entry. No copying. Existing photos do not
mention that name in their `stages` map, so the stage does not exist in their
pipeline: their rendering is unchanged **by construction**, and not because care
was taken not to touch their module.

### 5. Existing revisions go on rendering identically

> **Moot since [ADR 0043](0043-collapse-prerelease-render-history.md).**
> This paragraph was applied as it stands (an expansion table for the eleven
> versions, bit-for-bit equality proven, commit `bf63df1`), and then withdrawn:
> Leyline not having been published, those eleven versions committed us to
> nobody. The render history is collapsed onto one version per operator, and
> `process` disappears in favour of §2's `stages` map. The rest of the present
> ADR is intact.

`process: N` stays read and understood: each N has a **frozen and deterministic
expansion** into a set of stage versions, written once in a compatibility
table. The field becomes a historical shorthand and a display label ("this
photo uses an old process", like Lightroom's PV), plus an axis that grows.

### 6. The stage version, and not the application version, carries the guarantee

The `process` field was not only the rendering's versioning axis: it was also
the only scale at which the promise knew how to state itself. It is now stated
per stage:

> **No published version — a patch, a minor or a major — modifies the rendering
> of an already-published stage version.** If the rendering must change, it is a
> new stage version; existing revisions go on citing the old one.

A change of rendering is therefore **never** an increment of the application's
version: it is a new stage. The application version and the rendering's
identity are decoupled — Leyline 1.0.3 and Leyline 7.2.0 render `sharpen::v1`
identically, since it is the same frozen code in both binaries. It is also the
right scale on the user's side: their revision names the stage versions it
uses, whereas they have no idea which build produced their pixels.

The build profile is not part of the equation either: §7's reference renders
pass identically in `debug` and in `release` (Rust enables neither *fast-math*
nor FMA contraction, and auto-vectorization is not allowed to reassociate a
floating-point reduction).

There remains one input nobody controls by writing code: the toolchain.
`rust-toolchain.toml` is therefore pinned to an **exact version** rather than to
`stable` — otherwise a `rustup update` before a patch release would suffice to
move pixels. Changing it requires replaying the reference renders and recording
the drift.

### 7. Nothing migrates without proof: the reference renders first

**Not a line is refactored before reference renders exist.** The migration
proceeds in this order, strictly:

1. Capture, from the **current** code, one reference render per process version
   (1 to 11) over deterministic test images covering every operator, and commit
   them as fixtures.
2. Refactor towards versioned stages.
3. Prove **bit-for-bit** equality against those fixtures, for all eleven
   versions.

It is the fixtures — not reading the diff — that establish that a 2026 revision
renders in 2036 what it rendered in 2026. They stay in the test suite after the
migration, as a permanent guard.

## Consequences

* **The duplication disappears without the promise moving.** ~15,000 lines of
  pipeline come down to the genuinely distinct operators, plus eleven
  declarations. An operator fix is written once, instead of being applied eleven
  times as under ADR 0041.
* **The cost of a new pixel feature becomes constant** instead of growing with
  the number of versions already shipped. The next one (the DCP tables, ADR
  0037) will be a stage, not a twelfth copy of 2,700 lines.
* **`settings_json` changes shape**: it is the reproducibility contract itself
  that is amended. That is acceptable **only** because the project is
  pre-release; after opening to the world, that JSON shape would be definitive.
  That is the reason to make this change now and not later.
* **The correctness of the `process: N` expansion becomes critical**: a wrong
  expansion would render an old photo differently. That is exactly what step
  7's fixtures verify, version by version.
* **The number of stage versions can grow**, for its part — but only for the
  operators actually fixed, not for the eleven copies of those that were not.
  Over the real history, that would have produced a `v2` for the few operators
  touched by ADR 0013, and **no** other re-versioning.
* **`docs/pipeline.md` §3.3 is rewritten**: the process-version table becomes a
  historical compatibility table, and the versioning section describes the
  stages.
* **`docs/pipeline.md` §5 is split** into what is guaranteed (§5.1, with the
  publication rule above) and what is not (§5.2, cross-platform drift). The
  project now states a promise it holds in full, instead of a wider promise it
  held in part.
* **The toolchain becomes a versioned input of the rendering**:
  `rust-toolchain.toml` is pinned to an exact version, and changing it becomes
  an act that requires replaying the reference renders. It is the only variable
  able to move pixels without a line of code changing.
* **The application version ceases to carry anything about the rendering**: it
  can follow ordinary semver (features, fixes, interface) without the question
  "does this release change any pixels?" ever arising. The answer is
  structurally no.
* **ADR 0028 is superseded, not retroactively annulled**: its reasoning was
  correct for the data it had (five modules, 3,433 lines), and it had itself
  provided for its reopening on real data.

## Alternatives rejected

* **Keeping ADR 0028 as it stands**: the trajectory is not "linear and
  predictable" as it hoped — it is linear in the *number of modules* but
  quadratic in *cumulative lines*, each module being larger than the last. From
  3,433 to 14,968 lines for six shipped features.
* **A library of shared mutable operators** (what ADR 0028 really rejected):
  still rejected, and for its original reason. A single, modifiable operator
  used by every version would make a frozen rendering falsifiable. **Versioned
  and frozen** stages are not that.
* **Keeping a global layout counter** alongside the stage versions: redundant.
  The rank carried by the stage version suffices, and a global counter would
  start growing again with every insertion — the very problem being removed.
* **Writing the complete `stages` map on every revision**, neutral stages
  included: verbose without guaranteeing anything more. A neutral stage does not
  run; pinning the version of code that does not run pins nothing.
* **Deriving the stage versions from an "engine baseline" recorded per
  revision**: that is a global counter in disguise, with the further drawback of
  making the revision non-self-describing.
* **Migrating without reference renders, by reading the diff**: the one part of
  the system where "it should be fine" is not an acceptable criterion.
* **Adopting darktable's `legacy_params` too** — converting old stage versions'
  parameters to the current code rather than freezing the old code. It is the
  model of the field's two references, and it costs nothing in lines kept; it
  was examined seriously, and then rejected. The reason is not doctrinal: it is
  that the present ADR removes its appeal. What made freezing costly was the
  complete copying of the pipeline, not the freeze; once the stages are
  composed, freezing amounts to letting a few dozen lines live on that will
  never demand attention again. We would be trading the one guarantee that
  distinguishes Leyline from Lightroom and darktable for a few hundred lines
  per decade. Should the calculation one day invert, the escape hatch stays
  open **and measurable**: ADR 0042's structure accommodates both semantics,
  and §7's reference renders would say exactly what such a switch would cost,
  to the pixel.
* **Keeping `docs/pipeline.md` §5 as it stands** ("identical to the pixel",
  with no mention of the platform): untenable. A promise unverifiable on
  another machine is not a stronger promise, it is a more fragile one — the
  first libm drift a user observed would demolish it entirely, including the
  part that does hold.
* **Pinning the toolchain to `stable`**: that is what was in place, and it is
  precisely the hole. On a floating channel, the promise depends on the date
  each contributor last ran `rustup update`.
