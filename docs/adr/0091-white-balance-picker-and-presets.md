# ADR 0091 — White balance by pointing: the picker, Auto, and the fixed presets

**Status:** Accepted — 2026-08

## Context

Lightroom's Basic panel resolves white balance three ways Leyline does not:
an eyedropper (click a neutral grey, temperature and tint are solved), an
`Auto` entry, and a menu of fixed presets (`Daylight`, `Cloudy`, `Shade`,
`Tungsten`, `Fluorescent`, `Flash`). Leyline has the two sliders and nothing
else — the most common white-balance gesture in a real session, *point at
the grey card*, has no answer here.

Two facts about the existing pipeline frame the whole decision:

* The gains stage (`stages/gains`, rank 40) derives its per-channel gains
  from the ratio of two blackbody colors — the 6500 K reference against the
  slider's temperature — normalized on green, with tint a power-of-two on
  the green channel. The model is small, closed and invertible.
* The decode applies the camera's as-shot multipliers, so the slider is a
  **correction dial referenced at 6500 K**, not a reading of the scene
  illuminant: `(6500, 0)` — the `Settings` default — *is* as-shot.

## Decision

### 1. The picker produces settings, never pixels

`Library::neutralize_wb(asset, x, y)` takes a point in the unit coordinates
of the rendered image and returns the `WhiteBalance` that makes the 5×5
neighbourhood around it neutral. It writes nothing; the client feeds the
answer through the ordinary `EditSession`, so a picked white balance is one
undoable revision like any other. No stage, no stage version, nothing new
in `settings_json`: `docs/pipeline.md` §5.1 is untouched **by
construction** — the exact shape ADR 0088 §1 gave `auto_tone`, for the
exact reason.

### 2. Solved against the real pipeline, not against a formula alone

The sample is read from the proxy render (`preview_live`, the ADR 0074
path), which is the image *after* every stage — tone curve, output
rendering — not the buffer the gains stage sees. Those later operators are
per-channel-identical, so they preserve neutrality but distort ratios;
one-shot algebra on the sample would land close and wrong. So the solver
iterates the way `auto_tone`'s ends-search does: render at the candidate,
sample, linearize, divide out the candidate's own gains, re-solve
(temperature by binary search on the red/blue balance — the blackbody
ratio is monotone in temperature — tint in closed form on green), until
the sample is neutral or the slider range is exhausted. The gains model is
restated in `wb.rs` from the frozen stage file rather than exported from
it, and a test holds the two in agreement — the frozen file stays frozen
(ADR 0042 §1).

A sample that cannot answer is **refused, never guessed**: clicking a
clipped highlight or a near-black shadow returns an error naming the
reason, the same severity the capability rule gives an inexpressible
setting.

### 3. Auto is the same solver fed the whole frame

`Library::auto_wb(asset)` runs the identical loop on the frame's mean
color — grey-world, the oldest trick in the book — excluding clipped
pixels so a blown sky does not vote. It earns its place not by being
clever but by being the same code path as the picker: one solver, two ways
of choosing the sample.

### 4. The presets are a fixed table, and the approximation is stated

`WHITE_BALANCE_PRESETS` in `leyline-core`: Daylight 5500 K +10, Cloudy
6500 K +10, Shade 7500 K +10, Tungsten 2850 K, Fluorescent 3800 K +21,
Flash 5500 K — the conventional values. Because the slider is a correction
dial referenced at 6500 K (Context), these are exact only for a shot the
camera balanced to daylight; on anything else they are the same honest
approximation Lightroom applies to a JPEG, and the docstring says so
rather than hiding it. `As Shot` is not a table entry pretending to know
the illuminant: it clears the override (`None`), the true neutral of the
`Settings` model. Studio shows the table as one row group above the
Temperature slider; the CLI accepts the preset names wherever
`white-balance` already takes `<kelvin> <tint>`.

## Consequences

* One new engine module (`wb.rs`), two `Library` methods, one core table.
  No migration, no stage, no golden case touched.
* The picker's sampling infrastructure — click on the proxy, read the
  neighbourhood — is exactly what the range-mask eyedropper (ADR 0049's
  stated leftover) needs; it should reuse `neutralize_wb`'s plumbing.
* Improving the solver changes what the *next* click produces and moves
  nothing already developed (ADR 0088 §1's consequence, unchanged).

## Rejected

* **Exporting the gains function from the frozen stage file** — an edit to
  a frozen rendering file to serve a non-rendering caller; the agreement
  test costs three lines and keeps the freeze absolute.
* **A one-shot algebraic answer** — lands visibly off through the tone
  curve; a solver that is *almost* right teaches people the tool lies.
* **An `Auto` preset row in the table** — Auto is a measurement, not a
  constant; putting it in the table would freeze one frame's answer into
  every frame.
