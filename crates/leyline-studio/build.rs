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

    embed_windows_manifest();
    emit_build_facts();
}

/// Records what this binary was built from, for the About dialog's Version
/// tab.
///
/// It is not decoration: a bug report that names a version alone cannot
/// distinguish two builds a week apart, and asking a user to go and find
/// their commit costs a round trip on every report. Everything here is
/// best-effort — a build from a source tarball has no git checkout, and
/// that is a normal way to build this project, not a failure.
fn emit_build_facts() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "inconnu".to_owned());
    println!("cargo:rustc-env=LEYLINE_COMMIT={commit}");

    // Re-run when HEAD moves, so the recorded commit cannot go stale.
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let target = std::env::var("TARGET").unwrap_or_else(|_| "inconnu".to_owned());
    println!("cargo:rustc-env=LEYLINE_TARGET={target}");

    let rustc = std::env::var("RUSTC")
        .ok()
        .and_then(|rustc| {
            std::process::Command::new(rustc)
                .arg("--version")
                .output()
                .ok()
        })
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "inconnu".to_owned());
    println!("cargo:rustc-env=LEYLINE_RUSTC={rustc}");
}

/// Embeds a DPI-aware application manifest (`windows-manifest.xml`, wired in
/// via `windows-manifest.rc`) into the .exe when the *target* is Windows.
///
/// Without this, an unmarked process is DPI-unaware by default: Windows
/// negotiates the initial window size/DPI itself before winit gets a chance
/// to call `SetProcessDpiAwarenessContext`, which produced the "thin
/// horizontal strip on first launch, fixed by any manual resize" symptom
/// seen after installing the cross-compiled NSIS build.
///
/// `embed_resource::compile` uses the `CARGO_CFG_TARGET_OS`/`TARGET` build
/// environment (not the host OS `build.rs` itself runs on) to decide what to
/// do, so this is correct under cross-compilation: it chains `windres` +
/// `ar` for the `x86_64-pc-windows-gnu` target used by
/// `packaging/windows/build-nsis.sh`, would use `RC.EXE` for an MSVC target
/// (not used by this project, see that script for why), and is a no-op for
/// every non-Windows target (the native Linux build and its AppImage
/// packaging are unaffected). `embed-resource` itself only requires
/// `windres`/`ar`, not `rc.exe`, matching the mingw-based cross-compile
/// toolchain this project uses instead of requiring a Windows host.
fn embed_windows_manifest() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("windows-manifest.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed the Windows DPI-awareness manifest (windows-manifest.rc)");
    }
}
