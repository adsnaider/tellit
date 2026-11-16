//! Ensures the `crates/tellit-ui/dist` directory exists before `rust-embed` compiles.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=crates/tellit-ui/dist");

    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .expect("CARGO_MANIFEST_DIR must be set by Cargo");
    let dist_dir = manifest_dir.join("crates").join("tellit-ui").join("dist");
    if !dist_dir.exists() {
        std::fs::create_dir_all(&dist_dir)
            .expect("failed to create crates/tellit-ui/dist directory for rust-embed");
    }
}
