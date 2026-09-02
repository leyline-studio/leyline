#!/usr/bin/env bash
# Build the Leyline Studio NSIS installer (ADR 0019 —
# docs/adr/0019-distribution-i18n.md). cargo-packager shells out to
# `makensis`, which (unlike LibRaw/Lensfun/LittleCMS) is a pure builder tool
# with no native-library cross-compile problem: `apt install nsis` on Linux
# gives you a `makensis` that emits a Windows PE `.exe` directly, no Windows
# host required for this step.
#
# 2026-07-20: cross-compiled and packaged successfully from this Linux/WSL2
# box for target x86_64-pc-windows-gnu (`rustup target add
# x86_64-pc-windows-gnu` + `apt install mingw-w64 nsis`). Things to know
# before repeating this:
#
# 1. PKG_CONFIG_LIBDIR, not just PKG_CONFIG_PATH, must be restricted to
#    mingw-only paths when cross-building. `leyline-color`'s `lcms2-sys`
#    defaults to feature set ["dynamic", "static-fallback", "parallel"]: it
#    tries pkg-config first and only falls back to compiling its vendored
#    LittleCMS source statically (via the `cc` crate, which already knows to
#    invoke `x86_64-w64-mingw32-gcc` for this target) if that pkg-config
#    probe fails. Setting only PKG_CONFIG_PATH still leaves the host's
#    default search paths active, so pkg-config finds the native Linux
#    lcms2.pc, lcms2-sys concludes dynamic linking will work, and the build
#    fails at the final link step with `cannot find -llcms2` instead of
#    failing fast. Fix: set PKG_CONFIG_LIBDIR to *only* the mingw sysroot's
#    pkgconfig dir(s), e.g.:
#      export PKG_CONFIG_ALLOW_CROSS=1
#      export PKG_CONFIG_LIBDIR=/usr/x86_64-w64-mingw32/lib/pkgconfig:<libraw-mingw-prefix>/lib/pkgconfig
#    `leyline-lens`'s `lensfun` crate (v0.7, the vdavid/lensfun-rs port) has
#    no C dependency at all — pure Rust, nothing to configure for it.
#
# 2. `leyline-raw`'s build.rs only knows pkg-config (see its `.expect(...)`
#    message: "apt install libraw-dev ... Windows: vcpkg install libraw") —
#    no vendored-source fallback, and ADR 0004 forbids static linking of
#    LibRaw (LGPL-2.1 substitution obligation), so `libraw_r` must be found
#    dynamically. There is no mingw-w64 LibRaw package in Debian/Ubuntu's
#    archives. It was cross-built by hand from upstream source with
#    autotools, which DOES support cross-compiling LibRaw despite the
#    project shipping a separate (non-reentrant, static-only) Makefile.mingw
#    that looks like the intended mingw path but isn't what leyline-raw
#    needs:
#      curl -sLO https://github.com/LibRaw/LibRaw/archive/refs/tags/0.21.4.tar.gz
#      tar xzf 0.21.4.tar.gz && cd LibRaw-0.21.4
#      autoreconf -fiv
#      export PKG_CONFIG_LIBDIR=/usr/x86_64-w64-mingw32/lib/pkgconfig  # zlib only; see below
#      ./configure --host=x86_64-w64-mingw32 --prefix=<libraw-mingw-prefix> \
#          CC=x86_64-w64-mingw32-gcc CXX=x86_64-w64-mingw32-g++ \
#          --disable-examples --disable-static --enable-shared --disable-lcms
#      make -j"$(nproc)" && make install
#    `--disable-lcms` avoids the same host-pkg-config leak described above
#    (LibRaw's optional embedded-ICC-profile support isn't needed here;
#    leyline-color does its own ICC handling independently via lcms2-sys).
#    `apt install libz-mingw-w64-dev` provides the mingw zlib pkg-config
#    file LibRaw's ./configure needs. This produces `libraw_r-23.dll` +
#    `libraw_r.dll.a` + `libraw_r.pc` for the mingw target.
#
# 3. leyline-raw links libraw_r dynamically, so the built .exe needs
#    `libraw_r-23.dll` next to it — plus every DLL that DLL itself was
#    linked against (check with
#    `x86_64-w64-mingw32-objdump -p libraw_r-23.dll | grep 'DLL Name'`):
#    `zlib1.dll` (LibRaw's real zlib dependency — `--disable-lcms` in point 2
#    does NOT disable zlib), `libgcc_s_seh-1.dll`, `libstdc++-6.dll` (must be
#    the "posix" thread-model variant — check with `update-alternatives
#    --list x86_64-w64-mingw32-gcc`, since Debian's "win32" variant is
#    ABI-incompatible and produces the same missing-DLL error), and
#    transitively `libwinpthread-1.dll`. Verifying this under Wine is not
#    enough on its own: Wine ships its own `zlib1.dll` in its fake
#    `system32`, which silently satisfies that import even when the vendor
#    dir is missing it — this exact gap shipped once and only surfaced on a
#    real Windows machine ("zlib1.dll est introuvable" on first launch).
#    Trust `objdump -p`'s import list over a Wine smoke test for whether a
#    DLL needs bundling. All five are staged into
#    `packaging/windows/vendor/` (gitignored — rebuilt every run, not
#    committed) and picked up by the `resources` entry in
#    `crates/leyline-studio/Cargo.toml`'s `[package.metadata.packager]`,
#    which cargo-packager places next to the exe for the nsis/wix formats.
#
#    IMPORTANT: `resources` is a format-agnostic config key, and
#    cargo-packager's `appimage` packager internally reuses the `deb`
#    module's data-generation step, which *does* read `resources` — so
#    leaving Windows DLLs sitting in `packaging/windows/vendor/` while
#    running the Linux AppImage build would bundle them into the AppImage
#    too (discovered by testing, not assumed). This script therefore always
#    empties `packaging/windows/vendor/` on exit (success or failure) via
#    the trap below — the vendor DLLs must never outlive this script's run.
#    Don't remove that trap without re-verifying the AppImage build's
#    contents (`--appimage-extract` + `find -iname '*.dll'`) afterward.
#
# Weak sanity check available (not a substitute for real Windows testing):
# `apt install wine64` lets you smoke-test that the produced .exe/.dll are
# well-formed PE binaries, actually load their dependencies, and see
# missing-DLL errors if any are still absent. Wine 6.0.3 (Ubuntu jammy)
# can't resolve `bcryptprimitives.dll` for this binary — that's a Wine gap
# in this environment (older bcrypt shim), not evidence of a problem with
# the .exe itself; real Windows 10/11 ships bcryptprimitives.dll natively.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

vendor_dir="packaging/windows/vendor"
mkdir -p "$vendor_dir"
cleanup() { rm -rf "$vendor_dir"; }
trap cleanup EXIT

# Populate the vendor DLLs this run needs. libraw_r-23.dll (and its
# libraw_r.pc) must already exist from the manual cross-build described in
# point 2 above — this script doesn't redo that autotools build itself, but it
# defaults to where that build is kept on this machine so a repeat run needs
# no environment set up at all. Override LIBRAW_MINGW_PREFIX to point
# elsewhere.
: "${LIBRAW_MINGW_PREFIX:=/opt/leyline/libraw-mingw}"
if [[ ! -f "$LIBRAW_MINGW_PREFIX/bin/libraw_r-23.dll" ]]; then
    echo "error: no cross-built LibRaw at $LIBRAW_MINGW_PREFIX" >&2
    echo "       rebuild it following point 2 of the header comment, or set" >&2
    echo "       LIBRAW_MINGW_PREFIX to where it already lives." >&2
    exit 1
fi

# pkg-config must be restricted to mingw-only paths — see point 1 of the
# header comment for why PKG_CONFIG_PATH alone silently produces a
# `cannot find -llcms2` at the final link instead of failing fast. Set here
# rather than left to the caller: getting this wrong is the single easiest
# way to lose an hour on this build.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_LIBDIR="/usr/x86_64-w64-mingw32/lib/pkgconfig:$LIBRAW_MINGW_PREFIX/lib/pkgconfig"
cp "$LIBRAW_MINGW_PREFIX/bin/libraw_r-23.dll" "$vendor_dir/"
cp /usr/x86_64-w64-mingw32/lib/zlib1.dll "$vendor_dir/"
# The runtime DLLs live under a versioned directory whose name is the mingw
# gcc major (`10-posix` on jammy, `13-posix` on noble): resolved rather than
# written down, so this script is not pinned to the distribution that
# happened to run it first. Still the *posix* thread-model variant — Debian's
# win32 one is ABI-incompatible and fails identically to a missing DLL.
gcc_runtime_dir="$(ls -d /usr/lib/gcc/x86_64-w64-mingw32/*-posix 2>/dev/null | sort -V | tail -1)"
if [[ -z "$gcc_runtime_dir" ]]; then
    echo "error: no posix-threads mingw gcc runtime under /usr/lib/gcc/x86_64-w64-mingw32" >&2
    echo "       install mingw-w64, and see point 3 of the header comment." >&2
    exit 1
fi
cp "$gcc_runtime_dir/libgcc_s_seh-1.dll" "$vendor_dir/"
cp "$gcc_runtime_dir/libstdc++-6.dll" "$vendor_dir/"
cp /usr/x86_64-w64-mingw32/lib/libwinpthread-1.dll "$vendor_dir/"

# `--no-default-features` turns off `leyline-engine`'s `tether` feature: there
# is no cross-compilable libgphoto2 for this target (no Debian mingw package,
# weak upstream Windows support, and it pulls ltdl/libusb/gettext/iconv), the
# gap ADR 0038 already recorded as open. The feature removes only the backend
# — `tether_connect` still exists and reports why it cannot run — so nothing
# in Studio, the CLI or the SDK surface changes shape for this build. Drop
# this flag the day libgphoto2 is packaged for Windows.
#
# `--no-default-features` is a blunt instrument: it drops *every* default,
# so each one that must survive has to be named again. `bundled-basemap`
# (ADR 0059) is one of them — without it the Windows installer shipped
# without the world basemap, and its size gave it away (22 Mo instead of
# 31). Any new default feature of `leyline-studio` belongs in this list
# too, unless it is deliberately unwanted on Windows.
#
# `heif` (ADR 0114) is one that is deliberately unwanted here, and stays
# unnamed on purpose: the feature links the *system* libheif, there is none
# to link against on the mingw target, and an installer that bundled one
# would be shipping an HEVC decoder — which is exactly what ADR 0114 §1
# refuses. Windows HEIF, when it comes, goes through WIC and the extension
# the user installs, not through a library we carry.
build_flags=(
    --release --target x86_64-pc-windows-gnu -p leyline-studio
    --no-default-features --features bundled-basemap
)

# cargo-packager does not build the binary itself here, it packages one that
# already exists at the target path.
cargo build "${build_flags[@]}"
cargo packager --release -p leyline-studio -f nsis --target x86_64-pc-windows-gnu "$@"
