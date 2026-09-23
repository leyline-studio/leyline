#!/usr/bin/env bash
# Build, sign and check everything a release publishes — one command, one
# passphrase (ADR 0019, ADR 0077).
#
#   packaging/release.sh [manifest-notes-file]
#
# The passphrase is asked first and tried at once on a scratch file, so a
# typo stops the run before the builds rather than after them. It is then
# handed to the signing step alone: the builds run crates' build scripts,
# and none of them has any business seeing it.
#
# Then, in order: the AppImage, the Linux floor it actually requires checked
# against the one `docs/system-requirements.md` §4 announces, the Windows
# installer, the signed manifest, the signatures verified against the key
# compiled into Studio, and `SHA256SUMS`. It publishes nothing: `make
# publish` does, once what is in `target/release-manifest/` has been looked
# at.
set -euo pipefail
shopt -s inherit_errexit

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

notes_file="${1:-}"
key="${LEYLINE_SIGN_KEY:-$HOME/.config/leyline/leyline-updater.key}"
out_dir="target/release-manifest"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"

# What gets built is what is committed and pushed: a release that cannot be
# rebuilt from its tag is not one (docs/pipeline.md §5.1).
if [[ -n "$(git status --porcelain)" ]]; then
    echo "the working tree has uncommitted changes — commit or stash them first" >&2
    exit 1
fi
if [[ -z "$(git branch -r --contains HEAD)" ]]; then
    echo "HEAD is not pushed — push it first, the release is tagged on it" >&2
    exit 1
fi
if [[ -n "$notes_file" && ! -f "$notes_file" ]]; then
    echo "no such notes file: $notes_file" >&2
    exit 1
fi
if [[ ! -f "$key" ]]; then
    echo "no signing key at $key — see ADR 0077 §1" >&2
    exit 1
fi

echo "Leyline $version from $(git rev-parse --short HEAD)"
read -rsp "passphrase for $key: " passphrase
echo >&2

probe="$(mktemp)"
trap 'rm -f "$probe" "$probe.sig"' EXIT
if ! CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD="$passphrase" \
    cargo packager signer sign --private-key "$key" --quite "$probe" >/dev/null 2>&1; then
    echo "the key does not open with that passphrase" >&2
    exit 1
fi

bash packaging/linux/build-appimage.sh
appimage="target/release/leyline-studio_${version}_x86_64.AppImage"

# The floor is the highest glibc *version need* of any file in the package,
# read as the loader reads it. Symbols marked weak still carry a version
# need without the weak flag, and the loader refuses the binary on an older
# glibc all the same — skipping them is how this was once measured at 2.38
# for a package that needed 2.39.
extract="$(mktemp -d)"
(cd "$extract" && "$repo_root/$appimage" --appimage-extract >/dev/null)
floor="$(find "$extract/squashfs-root" -type f \( -name '*.so*' -o -name leyline-studio \) \
    -exec readelf -V {} \; 2>/dev/null |
    grep -oE 'GLIBC_[0-9.]+  Flags: none' | cut -d' ' -f1 | sort -uV | tail -1)"
rm -rf "$extract"
documented="$(sed -n 's/^| Linux | glibc \*\*\([0-9.]*\)\*\*.*/\1/p' docs/system-requirements.md)"
if [[ "${floor#GLIBC_}" != "$documented" ]]; then
    echo "the AppImage needs ${floor#GLIBC_}, docs/system-requirements.md §4 says $documented" >&2
    echo "fix the documentation (and the README table) before releasing" >&2
    exit 1
fi
echo "Linux floor: glibc $documented, as documented"

bash packaging/windows/build-nsis.sh

LEYLINE_SIGN_PASSWORD="$passphrase" bash packaging/release-manifest.sh "$version" ${notes_file:+"$notes_file"}
unset passphrase

echo "signatures against the key compiled into Studio:"
python3 packaging/verify-manifest.py

nsis="target/x86_64-pc-windows-gnu/release/leyline-studio_${version}_x64-setup.exe"
{
    (cd "$(dirname "$appimage")" && sha256sum "$(basename "$appimage")")
    (cd "$(dirname "$nsis")" && sha256sum "$(basename "$nsis")")
} >"$out_dir/SHA256SUMS"
git rev-parse HEAD >"$out_dir/COMMIT"

echo
echo "ready in $out_dir:"
cat "$out_dir/SHA256SUMS"
echo
echo "publish with: make publish RELEASE_NOTES=<notes.md>"
