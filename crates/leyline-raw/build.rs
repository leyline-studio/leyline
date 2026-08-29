//! Finds LibRaw (reentrant flavor) via pkg-config and compiles the C shim.
//!
//! Linking is always dynamic: the LGPL-2.1 substitution obligation of ADR 0004
//! forbids static linking of LibRaw. The version floor is therefore a floor on
//! what may *link*, never on what runs: `leyline_raw::decoder_version()` reads
//! the library that actually answers, and a test pins it (ADR 0086).

fn main() {
    let libraw = pkg_config::Config::new()
        .atleast_version("0.21")
        .probe("libraw_r")
        .expect(
            "LibRaw (reentrant) 0.21 or newer not found. Install it first, \
             e.g. `apt install libraw-dev` or `dnf install LibRaw-devel` \
             (Windows: vcpkg install libraw). The floor is 0.21 because that \
             is what this tree is validated against: the decoder is a term of \
             the reproducibility promise, not a detail (ADR 0086).",
        );

    let mut shim = cc::Build::new();
    for include in &libraw.include_paths {
        shim.include(include);
    }
    shim.file("src/shim.c").compile("leyline_libraw_shim");

    println!("cargo:rerun-if-changed=src/shim.c");
}
