//! Compiles the Slint UI description into Rust at build time.

fn main() {
    slint_build::compile("ui/studio.slint").expect("ui/studio.slint must compile");
}
