# Leyline — shortcuts for the commands that come up often.
#
# Nothing here is required: every target is a thin wrapper over a `cargo`
# invocation or a script under `packaging/`, and all of them stay runnable by
# hand. The point is to keep the exact flags in one place — several of them
# (the golden bless variable, the RAW test opt-in, the Windows packaging
# environment) are easy to get subtly wrong from memory.
#
# `make check` is the gate to run before every commit: the GitHub CI replays
# the same three steps (see `docs/contributing.md`), but only after the push —
# this is what stands between a mistake and `main`.

# rustup installs into ~/.cargo/bin, which some login shells don't export.
# A no-op when cargo is already on PATH.
export PATH := $(HOME)/.cargo/bin:$(PATH)

.DEFAULT_GOAL := help
.PHONY: help check fmt fmt-check lint test test-raw bench golden golden-bless \
        run cli i18n windows appimage dmg clean

help: ## List the available targets
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN {FS = ":.*?## "} {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

# --- the pre-commit gate ----------------------------------------------------

check: fmt-check lint test ## Everything that must pass before a commit

fmt: ## Format the workspace
	cargo fmt --all

fmt-check: ## Fail if anything is unformatted
	cargo fmt --all --check

lint: ## Clippy over every target, warnings are errors
	cargo clippy --workspace --all-targets -- -D warnings

test: ## Run the whole test suite
	cargo test --workspace

# --- tests that need something extra ----------------------------------------

# Ignored by default because they need a real RAW file, which the repo does
# not (and should not) carry. Point LEYLINE_TEST_RAW at one, e.g.
# `make test-raw LEYLINE_TEST_RAW="/mnt/g/Mes images/IMG_1234.CR2"`.
test-raw: ## Run the ignored tests against a real RAW (needs LEYLINE_TEST_RAW)
	@test -n "$(LEYLINE_TEST_RAW)" || { \
		echo "error: set LEYLINE_TEST_RAW to a real RAW file"; exit 1; }
	@test -f "$(LEYLINE_TEST_RAW)" || { \
		echo "error: no such file: $(LEYLINE_TEST_RAW)"; exit 1; }
	LEYLINE_TEST_RAW="$(LEYLINE_TEST_RAW)" cargo test -p leyline-engine -- --ignored

bench: ## Criterion benchmarks (pipeline + import + export)
	cargo bench -p leyline-engine

# --- reference renders (docs/contributing.md) -------------------------------

golden: ## Check the pinned reference renders
	cargo test -p leyline-engine --lib stages::golden

# Additive only: it never overwrites an existing entry. A changed fingerprint
# on an already-pinned entry is a defect — frozen code was touched — and it is
# fixed in the code, not in the manifest.
golden-bless: ## Add missing entries to the reference-render manifest
	LEYLINE_BLESS_GOLDEN=1 cargo test -p leyline-engine --lib stages::golden

# --- running ----------------------------------------------------------------

run: ## Launch Studio (make run ARGS=/path/to/library)
	cargo run -p leyline-studio -- $(ARGS)

cli: ## Run the CLI (make cli ARGS="info /path/to/library")
	cargo run -p leyline-cli -- $(ARGS)

# --- translations (ADR 0019) ------------------------------------------------

# The catalogue goes stale silently: `slint-tr-extractor` records each
# string's file, line *and* the name of the component containing it, so moving
# a string between components detaches it from its translation without any
# warning. Re-run this after every UI slice, carry the existing `msgstr`s
# across by `msgid`, then check on screen with `LANG=fr_FR.UTF-8`.
i18n: ## Re-extract the translation template from the .slint sources
	slint-tr-extractor -o crates/leyline-studio/translations/leyline-studio.pot \
		$$(find crates/leyline-studio/ui -name '*.slint' | sort)
	@echo "reminder: report existing translations into translations/fr/LC_MESSAGES/*.po"

# --- packaging (ADR 0019) ---------------------------------------------------

windows: ## Cross-build the Windows NSIS installer (no tethering, see the script)
	bash packaging/windows/build-nsis.sh

appimage: ## Build the Linux AppImage
	bash packaging/linux/build-appimage.sh

dmg: ## Build the macOS .app + .dmg (macOS host only)
	bash packaging/macos/build-dmg.sh

clean: ## Remove build artefacts
	cargo clean
