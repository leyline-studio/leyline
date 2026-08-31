#!/usr/bin/env bash
# Build the Leyline Studio .app bundle + .dmg (ADR 0019 —
# docs/adr/0019-distribution-i18n.md). Must run on macOS — cargo-packager
# shells out to macOS-only tools (hdiutil, etc.) to build the dmg. CI's
# `macos-package` job runs it on a GitHub macOS runner; it has not yet been
# exercised on a developer's own machine.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# The decoder is pinned, and the deliverable carries the pinned one
# (ADR 0086) — same reasoning, same recipe as
# `packaging/linux/build-appimage.sh`, which holds the full argument.
# Homebrew ships 0.22.2, a different *minor* from the one every other
# deliverable carries.
#
# Build the prefix once (macOS needs the same autoreconf as Linux):
#   curl -sLO https://github.com/LibRaw/LibRaw/archive/refs/tags/0.21.4.tar.gz
#   tar xzf 0.21.4.tar.gz && cd LibRaw-0.21.4
#   autoreconf -fiv
#   ./configure --prefix=/opt/leyline/libraw-macos --disable-examples \
#               --disable-static --enable-shared
#   make -j"$(sysctl -n hw.ncpu)" && make install
#
# `DYLD_FALLBACK_LIBRARY_PATH` plays the role `LD_LIBRARY_PATH` plays on
# Linux: 0.21.x releases share the install name `libraw_r.23.dylib`, so
# without it the linker would take the pinned headers and the loader would
# still be free to pick a Homebrew copy at run time.
LIBRAW_VERSION="${LIBRAW_VERSION:-0.21.4}"
LIBRAW_MACOS_PREFIX="${LIBRAW_MACOS_PREFIX:-/opt/leyline/libraw-macos}"
if [[ ! -f "$LIBRAW_MACOS_PREFIX/lib/pkgconfig/libraw_r.pc" ]]; then
    echo "error: no pinned LibRaw at $LIBRAW_MACOS_PREFIX" >&2
    echo "       build LibRaw $LIBRAW_VERSION there first — recipe in the header of this script." >&2
    exit 1
fi
export PKG_CONFIG_PATH="$LIBRAW_MACOS_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$LIBRAW_MACOS_PREFIX/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"

# cargo-packager packages an existing binary, it does not build one — see the
# same two-step order in `packaging/linux/build-appimage.sh` and
# `packaging/windows/build-nsis.sh`.
cargo build --release -p leyline-studio
cargo packager --release -p leyline-studio -f dmg "$@"
