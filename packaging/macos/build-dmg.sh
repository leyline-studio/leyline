#!/usr/bin/env bash
# Build the Leyline Studio .app bundle + .dmg (ADR 0019 —
# docs/adr/0019-distribution-i18n.md). Must run on macOS — cargo-packager
# shells out to macOS-only tools (hdiutil, etc.) to build the dmg. Not
# runnable/verified from the Linux dev environment this ADR slice was
# implemented in; config-only until built on a real macOS host.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo packager --release -p leyline-studio -f dmg "$@"
