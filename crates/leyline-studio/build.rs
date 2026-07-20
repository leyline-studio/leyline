//! Compiles the Slint UI description into Rust at build time.
//!
//! Also bundles the gettext `.po` translations under `translations/`
//! (ADR 0019) straight into the binary — no system gettext, no `.mo`
//! compilation step, no runtime file dependency to ship alongside the
//! executable, matching the Local First constraint (`docs/vision.md`).
//! `translations/<lang>/LC_MESSAGES/leyline-studio.po` is the layout
//! `slint-tr-extractor` and `slint_build` both expect; English needs no
//! entry here since it is the source language `@tr(...)` is written in.

fn main() {
    let config =
        slint_build::CompilerConfiguration::new().with_bundled_translations("translations");
    slint_build::compile_with_config("ui/studio.slint", config)
        .expect("ui/studio.slint must compile");
}
