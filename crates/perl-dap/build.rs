//! Build script for `perl-dap`.
//!
//! Generates `dap_feature_catalog.rs` in `OUT_DIR` from `features.toml`.
#![allow(clippy::pedantic, clippy::panic)]
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::env;
use std::error::Error;
use std::path::Path;

mod catalog {
    #![allow(dead_code)]
    include!("build_catalog.rs");
}

use catalog::{generate_catalog_module_at, resolve_catalog_source};

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var("OUT_DIR")?;
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;

    println!("cargo:rerun-if-env-changed=FEATURES_TOML_OVERRIDE");
    let source = resolve_catalog_source(Path::new(&manifest_dir))?;
    println!("cargo:rerun-if-changed={}", source.path.display());

    generate_catalog_module_at(
        Path::new(&manifest_dir),
        Path::new(&out_dir),
        env::var("FEATURES_TOML_OVERRIDE").ok().map(Into::into),
    )
}
