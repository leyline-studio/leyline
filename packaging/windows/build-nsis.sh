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
# x86_64-pc-windows-gnu` + `apt install mingw-w64 nsis`). Two things to know
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
# KNOWN GAP — not yet fixed: the NSIS installer built this way does NOT
# bundle libraw_r-23.dll. leyline-raw links it dynamically (see point 2), so
# the installed app will fail to start on a real Windows machine with
# "libraw_r-23.dll not found" (reproduced locally under Wine — see below)
# until that DLL ships next to leyline-studio.exe. cargo-packager supports
# this via a `resources` entry in `[package.metadata.packager]`
# (placed next to the executable for the nsis/wix formats — see
# cargo-packager's Config::resources docs), but it was deliberately NOT
# wired up here: `resources` is a top-level (format-agnostic) config key,
# and adding a Windows-only DLL path there risked changing behavior of the
# already-verified Linux AppImage build without being able to re-verify it
# end-to-end in the same sitting. Whoever picks this up next should either
# scope it correctly (confirm cargo-packager ignores unknown-platform
# resources cleanly, or gate it behind a separate config file passed via
# `-c`) or have the script copy the cross-built DLL next to the binary and
# pass it explicitly before invoking cargo packager.
#
# Weak sanity check available (not a substitute for real Windows testing):
# `apt install wine64` lets you smoke-test that the produced .exe/.dll are
# well-formed PE binaries and see missing-DLL errors like the one above.
# Wine 6.0.3 (Ubuntu jammy) also can't resolve `bcryptprimitives.dll` for
# this binary — that's a Wine gap in this environment (older bcrypt shim),
# not evidence of a problem with the .exe itself; real Windows 10/11 ships
# bcryptprimitives.dll natively.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo packager --release -p leyline-studio -f nsis "$@"
