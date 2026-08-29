#!/usr/bin/env bash
# Build the Leyline Studio AppImage (ADR 0019 — docs/adr/0019-distribution-i18n.md).
#
# Run from anywhere; always builds from the repo root so the relative
# paths in `crates/leyline-studio/Cargo.toml`'s
# `[package.metadata.packager]` resolve correctly.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# The decoder is pinned, and the deliverable carries the pinned one
# (ADR 0086). Left to the package managers this was three different
# decoders for one release: Ubuntu 0.21.2, the cross-built Windows prefix
# 0.21.4, Homebrew 0.22.2 — the last one a different *minor*. LibRaw is what
# turns a RAW into pixels and `docs/pipeline.md` §5.1 counts it among the
# terms of the bit-for-bit promise, so "whatever the distribution ships" is
# not a defensible answer for something we hand to someone else.
#
# Build the prefix once:
#   curl -sLO https://github.com/LibRaw/LibRaw/archive/refs/tags/0.21.4.tar.gz
#   tar xzf 0.21.4.tar.gz && cd LibRaw-0.21.4
#   autoreconf -fiv
#   ./configure --prefix=/opt/leyline/libraw-linux --disable-examples \
#               --disable-static --enable-shared
#   make -j"$(nproc)" && make install
#
# `LD_LIBRARY_PATH` matters as much as `PKG_CONFIG_PATH` here: 0.21.2 and
# 0.21.4 share the soname `libraw_r.so.23`, so without it the linker would
# take the pinned headers and the loader would still hand the AppImage
# bundler the system library — a silent mismatch that only shows up as a
# version string in the About dialog.
LIBRAW_VERSION="${LIBRAW_VERSION:-0.21.4}"
LIBRAW_LINUX_PREFIX="${LIBRAW_LINUX_PREFIX:-/opt/leyline/libraw-linux}"
if [[ ! -f "$LIBRAW_LINUX_PREFIX/lib/pkgconfig/libraw_r.pc" ]]; then
    echo "error: no pinned LibRaw at $LIBRAW_LINUX_PREFIX" >&2
    echo "       build LibRaw $LIBRAW_VERSION there first — recipe in the header of this script." >&2
    exit 1
fi
export PKG_CONFIG_PATH="$LIBRAW_LINUX_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$LIBRAW_LINUX_PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# cargo-packager does not build the binary itself, it packages one that
# already exists at the target path — without this it fails late with a
# "Failed to copy file ... No such file or directory" on
# `target/release/leyline-studio`, which reads like a packaging bug rather
# than a missing build. Same order as `packaging/windows/build-nsis.sh`.
cargo build --release -p leyline-studio
cargo packager --release -p leyline-studio -f appimage "$@"
