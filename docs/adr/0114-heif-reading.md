# ADR 0114 — Reading HEIF with the decoder the platform already has

**Status:** Accepted — 2026-09

## Context

`MediaType::Heif` has existed since V1. A `.heic` imports, is catalogued,
pairs with the RAW of the same frame ([ADR 0079](0079-raw-jpeg-pairing.md)) —
and then `source::decode` returns `Undecodable(Heif)`. The catalogue knows the
file; the engine cannot open it. Every phone made in the last eight years
writes these files by default.

An investigation on 2026-07-31 settled three engineering points and stopped
there: libheif would be a fourth system library, its version floor is 1.16,
and no `.heic` existed in the test corpus. **What it never opened was the
licence question**, and that question has two layers that are easy to
conflate:

* **Copyright.** libheif is LGPL-3 and `libde265` — the HEVC *decoder* — is
  LGPL-3+; both are compatible with Leyline's GPL-3. `x265` is GPL-2+, but it
  is an *encoder*: reading a HEIC never calls it. This layer is settled and
  favourable. (Verified on the build machine rather than recalled.)
* **Patents.** HEVC is covered by patent pools. Distributing a decoder inside
  our own packages puts us in their field, and GPL-3 §11 forbids conveying the
  work under a patent licence that would not extend to everyone downstream.
  This layer was never decided, and it is the one that matters.

## Decision

### 1. We link a decoder. We never ship one.

The posture is not new; it is written in `THIRD-PARTY-NOTICES.md` for the
brick that came first: LibRaw is *"linked as a system library, never
vendored"*. HEIF gets the same treatment, for a stronger reason — with LibRaw
it was hygiene, here it is what keeps a patented codec out of the binaries we
hand people.

Concretely, the `heif` feature follows `tether`'s pattern exactly
(`leyline-engine`, forwarded by `leyline-sdk` and `leyline-studio`), and:

* **an ordinary build has it on**, so a source build, a distribution's
  package, or a developer's `cargo run` reads HEIF using the libheif that
  distribution ships;
* **our own packages have it off** — the AppImage, the dmg and the Windows
  installer bundle their dependencies, so a package built with the feature on
  would carry an HEVC decoder inside it, which is the thing being refused.
  Windows already gets this for free: `build-nsis.sh` passes
  `--no-default-features`.

### 2. The consequence, stated rather than discovered

**Someone who installs the AppImage gets no HEIF.** They get a named refusal —
"this build has no HEIF decoder" — not a crash and not a mystery.

That is a real cost and this ADR does not dress it up. The way out is not to
bundle a codec, it is to use the decoder each operating system already has:
Image I/O on macOS, WIC plus the user's HEVC extension on Windows, libheif on
Linux. Three backends, one per platform, none of them shipped by us — a
follow-up decision, and the honest destination.

### 3. What comes out of the decoder

* **Eight bits, or sixteen when the file carries more.** `luma_bits_per_pixel`
  is read from the handle and the interleaved plane is requested at the
  matching width, which is exactly `input: 5`'s rule for a non-RAW source
  ([ADR 0107](0107-derived-assets-and-the-pixel-socket.md)) — a 10- or 12-bit
  HEIC must not be truncated on the way in.
* **The container's own rotation, applied once.** libheif applies `irot`/`imir`
  while decoding; the EXIF orientation tag is therefore **not** applied again
  on top, unlike the JPEG path where the codec applies nothing.

### 4. Colour: sRGB, like every other non-RAW source — and why that is the
### honest answer today

A HEIC from a phone carries its colour as **nclx** (primaries, transfer,
matrix — usually Display P3) and sometimes as an embedded ICC profile.
Leyline reads neither, and treats the samples as sRGB.

That is wrong, and it is **already wrong for JPEG**: `source::decode`'s native
path ignores the embedded profile too, and the same phones write Display P3
JPEGs. The gap is not HEIF's, it is the non-RAW colour path's, and it has been
there since V1.

Fixing it for HEIF alone would leave the codebase with two rules for non-RAW
sources — one honest, one not — decided by which format happened to arrive
last. Fixing it properly means reading the embedded profile (ICC *or* nclx)
for **every** non-RAW format, which changes what an existing JPEG revision
renders, and therefore costs an `input` version, its capability rule and its
golden entries ([ADR 0042](0042-versioned-stage-pipeline.md) §1). That is a
decision of its own, and naming it here is the point of this section.

So: HEIF enters at parity with JPEG, not below it, and the colour question is
opened for both at once.

### 5. `leyline-heif`, a crate the size of its job

The decode lives in its own crate, beside `leyline-raw`, for the reason
`leyline-raw` exists: a system library with an FFI boundary is a
responsibility, and the engine's job is to orchestrate, not to hold a second
`unsafe` surface. It is ~150 lines, it depends on `libheif-rs` 0.18 — the
binding whose floor (libheif ≥ 1.16) the 2026-07-31 investigation already
verified — and it exposes one function returning the same `RawImage` shape
LibRaw's path produces.

## Consequences

* A `.heic` develops, exports and prints like a JPEG, on any machine whose
  distribution provides libheif ≥ 1.16 and an HEVC decoder plugin.
* Our three packages do not gain a codec, and the AppImage and dmg lose HEIF
  they never had. The Windows installer is unaffected — it was already built
  with `--no-default-features`.
* `docs/pipeline.md`'s "HEIF and PSD are catalogued but have no decoder" is
  now half true, and says so: PSD stays refused.
* One more system dependency to document for people building from source, and
  one more line in `THIRD-PARTY-NOTICES.md` — under the same wording LibRaw
  uses, because it is the same arrangement.
* **Verified on a real file, and it changed the ADR.** A 6560 x 4928 HEVC
  HEIC from a phone decodes in **394 ms** and goes through import, develop and
  export in 0.7 s. Its orientation matches ImageMagick's render of the same
  file byte for byte in placement — including the mirroring, which is in the
  file (a selfie) and not something either of us applied twice.
* **The first attempt on that file failed, and that failure is the posture
  working as designed.** Ubuntu ships libheif without an HEVC decoder: the
  plugin is a separate package (`libheif-plugin-libde265`), which
  `libheif-dev` does **not** pull in. A machine can therefore build Leyline
  perfectly and refuse every HEIC a phone made — with libheif's own message,
  "Unsupported codec", which tells a person nothing they can act on. The
  error now names the package. That split is not an inconvenience to route
  around; it is a distribution making the same patent choice this ADR makes,
  one layer down, and the reason "the platform provides it" is a real answer
  rather than a slogan.

## Alternatives rejected

* **Bundling libheif and libde265 in our packages.** The straightforward
  thing, and what most applications do. Rejected by decision: it puts an HEVC
  decoder in binaries we distribute, which is the patent question, and GPL-3
  §11 makes accepting a pool licence for those binaries a contradiction with
  the licence we ship under.
* **Supporting only AV1-coded HEIF** (royalty-free, no pool). Consistent, and
  useless: the files people actually have are HEVC-coded, and a decoder that
  opens the rare file and refuses the common one is a worse answer than the
  refusal we have today.
* **`dlopen`-ing libheif at runtime** so a bundled package could use a system
  decoder when one exists. It is the technically exact expression of "the
  platform provides it", and it is a hand-written FFI surface plus a symbol
  table to maintain — for a gain the per-platform backends of §2 deliver
  better. Left there.
* **Making HEIF the one format that reads its embedded profile.** Rejected in
  §4: it buys correct colour for the newest format by giving the codebase two
  rules, and it hides the fact that JPEG has the same defect.
