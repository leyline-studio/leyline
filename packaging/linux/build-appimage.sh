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
#   curl -sLO https://github.com/LibRaw/LibRaw/archive/refs/tags/0.22.2.tar.gz
#   tar xzf 0.22.2.tar.gz && cd LibRaw-0.22.2
#   autoreconf -fiv
#   ./configure --prefix=/opt/leyline/libraw-linux --disable-examples \
#               --disable-static --enable-shared
#   make -j"$(nproc)" && make install
#
# `LD_LIBRARY_PATH` matters as much as `PKG_CONFIG_PATH` here. The pinned
# library is `libraw_r.so.25`; Ubuntu's 0.21.2 is `.so.23`, so without it the
# AppImage bundler cannot resolve the pinned one at all. Worse is the machine
# whose distribution ships another 0.22.x: the two share the soname, and the
# loader would hand the bundler the system library — a silent mismatch that
# only shows up as a version string in the About dialog.
LIBRAW_VERSION="${LIBRAW_VERSION:-0.22.2}"
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
# `--no-default-features --features tether,bundled-basemap` keeps everything
# an AppImage should have and drops one thing on purpose: `heif` (ADR 0114).
# The feature links the system libheif, and an AppImage bundles the shared
# libraries its binary needs — so building it on would put an HEVC decoder
# inside the image we distribute, which ADR 0114 §1 refuses. Someone running
# the AppImage therefore gets a named refusal on a `.heic`; someone running a
# distribution's package, or a build from source, reads it with the libheif
# their system already provides.
#
# `RUSTFLAGS` puts the pinned prefix first on the *link* line, and
# `PKG_CONFIG_PATH` alone does not: `lcms2-sys` adds `-L /usr/lib/x86_64-linux-gnu`
# on its own, and on a machine that also has `libraw-dev` the linker resolves
# `-lraw_r` there first. With 0.21.4 the two libraries shared soname 23, so it
# never showed; with 0.22.2 the AppImage came out carrying the system 0.21.2
# (ADR 0086 §7). Setting it changes cargo's fingerprint, so the first run
# after this rebuilds everything once.
export RUSTFLAGS="-L native=$LIBRAW_LINUX_PREFIX/lib${RUSTFLAGS:+ $RUSTFLAGS}"
cargo build --release -p leyline-studio \
    --no-default-features --features tether,bundled-basemap

# And the soname the binary asks for has to exist in the prefix, checked
# rather than trusted: nothing failed the one time it did not.
linked="$(readelf -d target/release/leyline-studio \
    | sed -n 's/.*(NEEDED).*\[\(libraw_r\.so\.[0-9]*\)\].*/\1/p')"
if [[ -z "$linked" || ! -e "$LIBRAW_LINUX_PREFIX/lib/$linked" ]]; then
    echo "error: target/release/leyline-studio links '${linked:-no libraw_r}'," >&2
    echo "       which the pinned prefix $LIBRAW_LINUX_PREFIX does not provide." >&2
    echo "       Another LibRaw came first on the link line (see RUSTFLAGS above)." >&2
    exit 1
fi

cargo packager --release -p leyline-studio -f appimage "$@"
