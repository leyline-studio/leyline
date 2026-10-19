#!/usr/bin/env bash
# Build the Leyline Studio .app bundle + .dmg (ADR 0019 —
# docs/adr/0019-distribution-i18n.md), self-contained and signed ad hoc
# (ADR 0155). Must run on macOS — otool, install_name_tool, codesign and
# hdiutil are macOS tools. CI's `macos-package` job runs it on a GitHub
# macOS runner; it has not yet been exercised on a developer's own machine.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# The decoder is pinned, and the deliverable carries the pinned one
# (ADR 0086) — same reasoning, same recipe as
# `packaging/linux/build-appimage.sh`, which holds the full argument.
# Homebrew ships its own LibRaw, on its own schedule — which is not a pin.
#
# Build the prefix once (macOS needs the same autoreconf as Linux):
#   curl -sLO https://github.com/LibRaw/LibRaw/archive/refs/tags/0.22.2.tar.gz
#   tar xzf 0.22.2.tar.gz && cd LibRaw-0.22.2
#   autoreconf -fiv
#   ./configure --prefix=/opt/leyline/libraw-macos --disable-examples \
#               --disable-static --enable-shared
#   make -j"$(sysctl -n hw.ncpu)" && make install
#
# `DYLD_FALLBACK_LIBRARY_PATH` plays the role `LD_LIBRARY_PATH` plays on
# Linux: 0.22.x releases share the install name `libraw_r.25.dylib`, so
# without it the linker would take the pinned headers and the loader would
# still be free to pick a Homebrew copy at run time.
LIBRAW_VERSION="${LIBRAW_VERSION:-0.22.2}"
LIBRAW_MACOS_PREFIX="${LIBRAW_MACOS_PREFIX:-/opt/leyline/libraw-macos}"
if [[ ! -f "$LIBRAW_MACOS_PREFIX/lib/pkgconfig/libraw_r.pc" ]]; then
    echo "error: no pinned LibRaw at $LIBRAW_MACOS_PREFIX" >&2
    echo "       build LibRaw $LIBRAW_VERSION there first — recipe in the header of this script." >&2
    exit 1
fi
export PKG_CONFIG_PATH="$LIBRAW_MACOS_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$LIBRAW_MACOS_PREFIX/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
# The pinned prefix goes first on the link line too, for the reason
# `packaging/linux/build-appimage.sh` gives: another crate's pkg-config probe
# can add Homebrew's lib directory ahead of it, and a `brew install libraw`
# would then win at link time (ADR 0086 §7).
export RUSTFLAGS="-L native=$LIBRAW_MACOS_PREFIX/lib${RUSTFLAGS:+ $RUSTFLAGS}"

# cargo-packager packages an existing binary, it does not build one — see the
# same two-step order in `packaging/linux/build-appimage.sh` and
# `packaging/windows/build-nsis.sh`.
# `heif` is left out for the reason ADR 0114 §1 gives: a dmg carries the
# libraries its binary links, so building it on would put an HEVC decoder in
# the bundle we distribute. macOS has a HEIF decoder of its own (Image I/O);
# using it is a backend to write, not a library to ship.
# `tether` is left out too (ADR 0155 §4): libgphoto2 loads its camera drivers
# from a directory compiled into it, which a bundle cannot carry.
cargo build --release -p leyline-studio \
    --no-default-features --features bundled-basemap,gpu
cargo packager --release -p leyline-studio -f app "$@"

# Self-contained bundle (ADR 0155 §2): every library named by an absolute
# path outside the system goes into Contents/Frameworks under an @rpath
# install name, and the walk follows what it copies. Bash 3.2 compatible —
# that is the /bin/bash a Mac ships.
app="target/release/Leyline Studio.app"
binary="$app/Contents/MacOS/leyline-studio"
frameworks="$app/Contents/Frameworks"
mkdir -p "$frameworks"

foreign_dependencies() {
    otool -L "$1" | tail -n +2 | awk '{print $1}' | while IFS= read -r dependency; do
        if [[ $dependency == /* && $dependency != /usr/lib/* && $dependency != /System/* ]]; then
            echo "$dependency"
        fi
    done
}

bundle_dependencies() {
    local macho="$1" dependencies dependency name
    dependencies="$(foreign_dependencies "$macho")"
    while IFS= read -r dependency; do
        [[ -n $dependency ]] || continue
        name="$(basename "$dependency")"
        if [[ ! -f "$frameworks/$name" ]]; then
            cp -L "$dependency" "$frameworks/$name"
            chmod u+w "$frameworks/$name"
            install_name_tool -id "@rpath/$name" "$frameworks/$name"
            bundle_dependencies "$frameworks/$name"
        fi
        install_name_tool -change "$dependency" "@rpath/$name" "$macho"
    done <<< "$dependencies"
}

bundle_dependencies "$binary"
install_name_tool -add_rpath "@executable_path/../Frameworks" "$binary"

# The guard of ADR 0155 §6: nothing may still point outside the bundle.
leftover=0
for macho in "$binary" "$frameworks"/*.dylib; do
    [[ -e "$macho" ]] || continue
    remaining="$(foreign_dependencies "$macho")"
    if [[ -n $remaining ]]; then
        echo "error: $macho still links outside the bundle:" >&2
        echo "$remaining" >&2
        leftover=1
    fi
done
[[ $leftover -eq 0 ]] || exit 1
echo "bundled libraries:"
ls "$frameworks"

# Ad hoc signature (ADR 0155 §3): every rewrite above invalidated the
# linker's, and arm64 runs no unsigned code. Libraries first, then the app.
for library in "$frameworks"/*.dylib; do
    [[ -e "$library" ]] || continue
    codesign --force --sign - "$library"
done
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"

# `cargo packager -f dmg` would rebuild the bundle from the binary and lose
# the rewrite (ADR 0155 §5). `ditto` keeps the signature intact.
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
staging="$(mktemp -d)"
ditto "$app" "$staging/Leyline Studio.app"
ln -s /Applications "$staging/Applications"
dmg="target/release/leyline-studio_${version}_aarch64.dmg"
hdiutil create -volname "Leyline Studio" -srcfolder "$staging" -ov -format UDZO "$dmg"
rm -rf "$staging"
echo "built $dmg"
