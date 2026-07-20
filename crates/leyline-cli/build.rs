//! Embeds the Windows `.exe` icon (see `windows-icon.rc`) when the *target*
//! is Windows; a no-op for every other target (same pattern as
//! `crates/leyline-studio/build.rs`).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // `.manifest_optional()`: the resource here is cosmetic (an icon),
        // not a manifest with security/entry-point implications, so a failed
        // embed shouldn't hard-fail the build the way `manifest_required()`
        // would — see `leyline-studio/build.rs` for the contrasting case.
        embed_resource::compile("windows-icon.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("failed to embed the Windows icon (windows-icon.rc)");
    }
}
