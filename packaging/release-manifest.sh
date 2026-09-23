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
# it). The key carries a passphrase, asked once on the terminal; set
# `LEYLINE_SIGN_PASSWORD` instead to sign without a terminal.
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

# Asked once, never echoed, and handed over through the environment: an
# argument would sit in `ps` and in the shell history for as long as the
# signing runs. An empty answer means the key has no passphrase.
if [[ -z "${LEYLINE_SIGN_PASSWORD+set}" && -t 0 ]]; then
    read -rsp "passphrase for $key: " LEYLINE_SIGN_PASSWORD
    echo >&2
fi
if [[ -n "${LEYLINE_SIGN_PASSWORD:-}" ]]; then
    export CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD="$LEYLINE_SIGN_PASSWORD"
fi

# Signs one artifact and echoes the signature, which is what the manifest
# carries — the updater verifies the downloaded bytes against it before
# anything is installed. The key goes by its path, for the same reason as
# the passphrase.
sign() {
    local file="$1"
    cargo packager signer sign \
        --private-key "$key" \
        --quite \
        "$file" >/dev/null
    cat "$file.sig"
}

# Each platform contributes a line only if its artifact was actually built:
# a manifest that names a file nobody published sends the updater to a 404,
# and a platform left out simply reports "no update" there — which is the
# honest answer while, say, macOS has no build (ADR 0019, ADR 0077).
#
# Matched on the version being released, not just on the extension. `target/`
# keeps every artifact ever built, and a plain `ls … | head -1` picks them in
# alphabetical order — which is to say it picks the *oldest* version still
# lying around. The manifest would then carry a signature over the previous
# release's bytes while pointing at this release's URL, and every updater
# would refuse the download it just made. Found the day 0.1.0-alpha.4 was
# packaged next to 0.1.0-alpha.3.
platforms=()

# The one artifact of this version, or nothing. Two matches means `target/`
# holds something unexpected, and guessing between them is exactly the
# mistake above.
only_one() {
    local matches=()
    while IFS= read -r line; do
        [[ -n "$line" ]] && matches+=("$line")
    done < <(ls "$@" 2>/dev/null || true)
    case ${#matches[@]} in
        0) return 0 ;;
        1) printf '%s' "${matches[0]}" ;;
        *)
            printf 'several artifacts match %s:\n%s\n' "$*" "$(printf '  %s\n' "${matches[@]}")" >&2
            exit 1
            ;;
    esac
}

appimage="$(only_one "target/release/"*"_${version}_"*.AppImage)"
if [[ -n "$appimage" ]]; then
    signature="$(sign "$appimage")"
    platforms+=("$(printf '"linux-x86_64": {"url": "%s/releases/download/v%s/%s", "signature": "%s", "format": "appimage"}' \
        "$repository" "$version" "$(basename "$appimage")" "$signature")")
fi

nsis="$(only_one "target/x86_64-pc-windows-gnu/release/"*"_${version}_"*-setup.exe)"
if [[ -n "$nsis" ]]; then
    signature="$(sign "$nsis")"
    platforms+=("$(printf '"windows-x86_64": {"url": "%s/releases/download/v%s/%s", "signature": "%s", "format": "nsis"}' \
        "$repository" "$version" "$(basename "$nsis")" "$signature")")
fi

dmg="$(only_one "target/release/"*"_${version}_"*.dmg)"
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
