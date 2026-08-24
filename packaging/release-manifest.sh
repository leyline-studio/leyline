#!/usr/bin/env bash
# Sign the packaged artifacts and write the update manifest
# (ADR 0077 — docs/adr/0077-application-updates.md).
#
# The manifest is `latest.json`, published as an asset of the GitHub
# release alongside the artifacts it describes. The installed binary asks
# for it at `releases/latest/download/latest.json`, a redirection GitHub
# keeps pointing at the newest release — which is why that URL is compiled
# into the binary and never configurable.
#
# The private key lives outside this repository and outside CI: it signs
# here, by hand, at publication time. Point `LEYLINE_SIGN_KEY` at it (the
# default below is where `cargo packager signer generate` was told to put
# it), and set `LEYLINE_SIGN_PASSWORD` if the key carries a password.
#
#   packaging/release-manifest.sh <version> [notes-file]
#
# Writes `target/release-manifest/latest.json` and the `.sig` files next to
# each artifact. It builds nothing: run `make appimage` and `make windows`
# first, and pass the same version they were built from.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

version="${1:?usage: release-manifest.sh <version> [notes-file]}"
notes_file="${2:-}"
key="${LEYLINE_SIGN_KEY:-$HOME/.config/leyline/leyline-updater.key}"
repository="https://github.com/leyline-studio/leyline"
out_dir="target/release-manifest"

if [[ ! -f "$key" ]]; then
    echo "no signing key at $key — see ADR 0077 §1" >&2
    exit 1
fi

mkdir -p "$out_dir"

# Signs one artifact and echoes the signature, which is what the manifest
# carries — the updater verifies the downloaded bytes against it before
# anything is installed.
sign() {
    local file="$1"
    cargo packager signer sign \
        --private-key "$(cat "$key")" \
        ${LEYLINE_SIGN_PASSWORD:+--password "$LEYLINE_SIGN_PASSWORD"} \
        --quite \
        "$file" >/dev/null
    cat "$file.sig"
}

# Each platform contributes a line only if its artifact was actually built:
# a manifest that names a file nobody published sends the updater to a 404,
# and a platform left out simply reports "no update" there — which is the
# honest answer while, say, macOS has no build (ADR 0019, ADR 0077).
platforms=()

appimage="$(ls target/release/*.AppImage 2>/dev/null | head -1 || true)"
if [[ -n "$appimage" ]]; then
    signature="$(sign "$appimage")"
    platforms+=("$(printf '"linux-x86_64": {"url": "%s/releases/download/v%s/%s", "signature": "%s", "format": "appimage"}' \
        "$repository" "$version" "$(basename "$appimage")" "$signature")")
fi

nsis="$(ls target/x86_64-pc-windows-gnu/release/*-setup.exe 2>/dev/null | head -1 || true)"
if [[ -n "$nsis" ]]; then
    signature="$(sign "$nsis")"
    platforms+=("$(printf '"windows-x86_64": {"url": "%s/releases/download/v%s/%s", "signature": "%s", "format": "nsis"}' \
        "$repository" "$version" "$(basename "$nsis")" "$signature")")
fi

dmg="$(ls target/release/*.dmg 2>/dev/null | head -1 || true)"
if [[ -n "$dmg" ]]; then
    signature="$(sign "$dmg")"
    platforms+=("$(printf '"darwin-x86_64": {"url": "%s/releases/download/v%s/%s", "signature": "%s", "format": "app"}' \
        "$repository" "$version" "$(basename "$dmg")" "$signature")")
fi

if [[ ${#platforms[@]} -eq 0 ]]; then
    echo "no packaged artifact found — run 'make appimage' and 'make windows' first" >&2
    exit 1
fi

notes=""
if [[ -n "$notes_file" ]]; then
    notes="$(python3 -c 'import json,sys; print(json.dumps(open(sys.argv[1]).read().strip()))' "$notes_file")"
else
    notes='""'
fi

{
    printf '{\n'
    printf '  "version": "%s",\n' "$version"
    printf '  "notes": %s,\n' "$notes"
    printf '  "pub_date": "%s",\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '  "platforms": {\n'
    printf '    %s' "${platforms[0]}"
    for entry in "${platforms[@]:1}"; do
        printf ',\n    %s' "$entry"
    done
    printf '\n  }\n'
    printf '}\n'
} > "$out_dir/latest.json"

echo "wrote $out_dir/latest.json"
for entry in "${platforms[@]}"; do
    echo "  ${entry%%:*}"
done
