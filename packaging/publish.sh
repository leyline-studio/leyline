#!/usr/bin/env bash
# Publish what `packaging/release.sh` prepared as a GitHub release
# (ADR 0019, ADR 0077).
#
#   packaging/publish.sh <release-notes.md>
#
# Nothing is rebuilt or re-signed here. The signatures are checked again
# against the key compiled into Studio and the packages against
# `SHA256SUMS` — `target/` may have moved since — and the release is tagged
# on the commit the packages were built from, not on whatever `main` is by
# now. It asks before sending anything.
set -euo pipefail
shopt -s inherit_errexit

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

notes_file="${1:?usage: publish.sh <release-notes.md>}"
out_dir="target/release-manifest"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
appimage="target/release/leyline-studio_${version}_x86_64.AppImage"
nsis="target/x86_64-pc-windows-gnu/release/leyline-studio_${version}_x64-setup.exe"

for file in "$notes_file" "$out_dir/latest.json" "$out_dir/SHA256SUMS" "$out_dir/COMMIT" \
    "$appimage" "$nsis"; do
    if [[ ! -f "$file" ]]; then
        echo "missing $file — run 'make release' first" >&2
        exit 1
    fi
done
manifest_version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' \
    "$out_dir/latest.json")"
if [[ "$manifest_version" != "$version" ]]; then
    echo "latest.json is for $manifest_version, the workspace is at $version" >&2
    exit 1
fi

python3 packaging/verify-manifest.py
(cd "$(dirname "$appimage")" && sha256sum --quiet -c <(grep AppImage "$repo_root/$out_dir/SHA256SUMS"))
(cd "$(dirname "$nsis")" && sha256sum --quiet -c <(grep setup.exe "$repo_root/$out_dir/SHA256SUMS"))
commit="$(cat "$out_dir/COMMIT")"

# A version with a suffix (0.2.0-rc.1) is a prerelease: GitHub then keeps
# `releases/latest` — the URL the updater reads — on the last stable one.
prerelease=()
[[ "$version" == *-* ]] && prerelease=(--prerelease)

echo "v$version on ${commit:0:7}, ${prerelease[*]:-stable}:"
echo "  $(basename "$appimage")"
echo "  $(basename "$nsis")"
echo "  latest.json, SHA256SUMS"
read -rp "publish to GitHub? [y/N] " answer
[[ "$answer" == [yY] ]] || { echo "nothing sent"; exit 1; }

gh release create "v$version" \
    --target "$commit" \
    --title "Leyline Studio $version" \
    --notes-file "$notes_file" \
    "${prerelease[@]}" \
    "$appimage" "$nsis" "$out_dir/latest.json" "$out_dir/SHA256SUMS"
