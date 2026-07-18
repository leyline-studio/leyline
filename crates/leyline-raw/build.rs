//! Finds LibRaw (reentrant flavor) via pkg-config and compiles the C shim.
//!
//! Linking is always dynamic: the LGPL-2.1 substitution obligation of ADR 0004
//! forbids static linking of LibRaw.

fn main() {
    let libraw = pkg_config::Config::new()
        .atleast_version("0.19")
        .probe("libraw_r")
        .expect(
            "LibRaw (reentrant) not found. Install it first, e.g. \
             `apt install libraw-dev` or `dnf install LibRaw-devel` \
             (Windows: vcpkg install libraw).",
        );

    let mut shim = cc::Build::new();
    for include in &libraw.include_paths {
        shim.include(include);
    }
    shim.file("src/shim.c").compile("leyline_libraw_shim");

    println!("cargo:rerun-if-changed=src/shim.c");
}
