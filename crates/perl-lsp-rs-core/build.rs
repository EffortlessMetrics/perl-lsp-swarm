// Build script for generating feature-contract support artifacts.
// Wave Final PR B: pearl-feature-catalog absorbed into feature_catalog.rs.
// Build-time catalog logic is inlined via include!().
#![allow(clippy::pedantic, clippy::panic)]
// Build scripts are allowed to use eprintln!/println! for cargo directives and diagnostics.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::env;
use std::error::Error;
use std::path::Path;

// Import catalog helpers inlined from build_catalog.rs
// (separate file because build.rs is its own compilation unit)
mod catalog {
    #![allow(dead_code)] // DAP-specific helpers used only by perl-dap/build.rs
    include!("build_catalog.rs");
}

use catalog::generate_lsp_catalog_module_at;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var("OUT_DIR").map_err(
        |_| "OUT_DIR must be set by cargo during build - this is a build-time requirement",
    )?;

    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    println!("cargo:rerun-if-env-changed=FEATURES_TOML_OVERRIDE");

    let source = generate_lsp_catalog_module_at(
        Path::new(&manifest_dir),
        Path::new(&out_dir),
        env::var_os("FEATURES_TOML_OVERRIDE").map(Into::into),
    )
    .map_err(|error| format!("failed to load LSP feature catalog: {error}"))?;
    println!("cargo:rerun-if-changed={}", source.path.display());

    Ok(())
}
