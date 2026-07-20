#!/usr/bin/env bash
# Build the Leyline Studio NSIS installer (ADR 0019 —
# docs/adr/0019-distribution-i18n.md). Must run on Windows (or a Windows
# cross-compilation host with NSIS installed) — cargo-packager shells out to
# `makensis`. Not runnable/verified from the Linux dev environment this
# ADR slice was implemented in; config-only until built on a real Windows
# host.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

cargo packager --release -p leyline-studio -f nsis "$@"
