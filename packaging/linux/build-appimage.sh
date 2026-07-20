#!/usr/bin/env bash
# Build the Leyline Studio AppImage (ADR 0019 — docs/adr/0019-distribution-i18n.md).
#
# Run from anywhere; always builds from the repo root so the relative
# paths in `crates/leyline-studio/Cargo.toml`'s
# `[package.metadata.packager]` resolve correctly.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo packager --release -p leyline-studio -f appimage "$@"
