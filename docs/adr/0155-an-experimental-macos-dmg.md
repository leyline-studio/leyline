# ADR 0155 — An experimental macOS dmg, self-contained and signed ad hoc

**Status:** Accepted — 2026-10

## Context

[ADR 0019](0019-distribution-i18n.md) promises a `.app` in a `.dmg` for
macOS, and the CI's `macos-package` job has built one on every tag since
the first release. Nobody has ever opened it, and it would not open:

1. **It links libraries by absolute path.** The pinned LibRaw
   ([ADR 0086](0086-decoder-in-the-promise.md)) is built into the runner's
   temporary directory, and the binary records that path as its install
   name. So does every Homebrew library pulled in on the way (libgphoto2
   and its own dependencies under `/opt/homebrew`). On any other Mac, dyld
   finds none of them and the app dies before its first window.
2. **It carries no usable signature.** On Apple silicon, code without a
   valid signature is not run at all. The linker signs the binary ad hoc,
   but any rewrite of its load commands invalidates that signature, and the
   bundle itself is never signed.
3. **Tethering cannot travel.** libgphoto2 loads its camera and port drivers
   with `dlopen` from a directory compiled into it (`/opt/homebrew/lib/libgphoto2/…`).
   Bundling the library without those drivers gives a tether that finds no
   camera; bundling the drivers means relocating a plugin tree the library
   was not built to relocate.

The maintainer has no Mac and no Apple Developer account. Whatever is
decided must be checkable from the CI alone, and must not pretend to more.

## Decision

1. **The dmg is experimental and says so.** It is built for **Apple silicon
   only** (`macos-latest` is arm64), and the readme and release notes call
   it untested on real hardware until someone reports otherwise. No Intel
   build, no universal binary.
2. **Self-contained.** After `cargo packager -f app` builds the bundle,
   `packaging/macos/build-dmg.sh` walks the binary's dependencies
   recursively: every library outside `/usr/lib` and `/System` is copied
   into `Contents/Frameworks`, its install name becomes
   `@rpath/<file name>`, every reference to it is rewritten the same way,
   and the binary gains the rpath `@executable_path/../Frameworks`. The walk
   follows the libraries it copies, so a dependency of LibRaw (a JPEG or
   LittleCMS library its configure happened to find) travels too, without
   a list to keep up to date.
3. **Signed ad hoc.** The bundle is signed with `codesign --sign -` after
   the rewrite, the copied libraries first, then the app. This is what lets
   an arm64 binary run at all; it is not an identity, and Gatekeeper still
   quarantines a downloaded copy. `docs/system-requirements.md` §4 states
   the remedy plainly:
   *System Settings → Privacy & Security → Open Anyway*, or
   `xattr -dr com.apple.quarantine "/Applications/Leyline Studio.app"`.
4. **No tethering on macOS**, as on Windows: the build drops the `tether`
   feature, for reason 3 above. `heif` stays off for the reason
   [ADR 0114](0114-heif-reading.md) §1 gives. The GPU
   preview stays on (wgpu speaks Metal).
5. **The dmg is assembled by `hdiutil`**, from a folder holding the app and
   a link to `/Applications`, rather than by `cargo packager -f dmg`, which
   would rebuild the bundle from the binary and lose the rewrite.
6. **The CI is the check.** Before the dmg is made, the script refuses to
   continue if any Mach-O in the bundle still names a library by an absolute
   path outside `/usr/lib` and `/System`, or if `codesign --verify --deep
   --strict` fails. The job runs on tags and on `gh workflow run CI`, as
   before.

## Consequences

* A Mac user can download the dmg, clear the quarantine once, and run it.
  Whether the pipeline renders correctly there is still unproven: the
  golden suite runs on the CI's macOS `check` leg, but no one has looked at
  the window.
* The rewrite in point 2 is the only place install names are touched; a new
  system dependency on macOS needs no change to the script, only a green CI
  run to show the guard passes.
* The libraries copied from Homebrew keep their own licences, which the
  bundle must honour as the AppImage and the NSIS installer already do.
* Tethering on macOS waits for a decision of its own: either relocating
  libgphoto2's drivers (setting `CAMLIBS` and `IOLIBS` before the first
  `gp_*` call) or a native backend.

## Alternatives rejected

* **Statically linking LibRaw.** It removes one library, not the problem:
  the walk is still needed for the rest, and the static build is a
  different artefact from the one the other two platforms pin.
* **`dylibbundler` or `macdeployqt`-like tools from Homebrew.** One more
  tool to install and trust for a forty-line loop the script owns.
* **Keeping `cargo packager -f dmg`** and fixing the binary before it
  packages: the packager copies the binary but not the libraries, and its
  own frameworks list would be the list point 2 avoids keeping.
* **A Developer ID and notarisation.** No account, and no Mac to test the
  result on; it can replace point 3 the day both exist, without touching
  the rest.
* **Shipping nothing for macOS.** ADR 0019 promises the platform; an honest
  experimental build costs one script and tells us whether the promise holds.
