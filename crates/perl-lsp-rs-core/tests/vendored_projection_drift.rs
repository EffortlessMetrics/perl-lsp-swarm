//! Vendored catalog projection drift guard (#7029).
//!
//! Root `features.toml` is the single human-edited authority. Each crate-local
//! `features_sot.toml` is a deterministic byte projection of that authority so
//! standalone/packaged builds resolve an identical catalog. This test fails
//! when a vendored copy drifts from the authority; regenerate with
//! `cargo xtask features regen-vendored`.

use std::path::{Path, PathBuf};

use perl_tdd_support::{must, must_some};

const VENDORED_PATHS: &[&str] = &[
    "crates/perl-lsp-rs/features_sot.toml",
    "crates/perl-lsp-rs-core/features_sot.toml",
    "crates/perl-parser/features_sot.toml",
    "crates/perl-dap/features_sot.toml",
];

fn workspace_root() -> PathBuf {
    must_some(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).map(Path::to_path_buf))
}

fn read_bytes(path: &Path) -> Vec<u8> {
    must(
        std::fs::read(path)
            .map_err(|error| format!("cannot read catalog projection {}: {error}", path.display())),
    )
}

#[test]
fn every_vendored_projection_is_byte_identical_to_the_authority() {
    let root = workspace_root();
    let authority = read_bytes(&root.join("features.toml"));

    let mut drifted = Vec::new();
    for relative in VENDORED_PATHS {
        let path = root.join(relative);
        let vendored = read_bytes(&path);
        if vendored != authority {
            drifted.push((*relative).to_string());
        }
    }

    assert!(
        drifted.is_empty(),
        "vendored catalog projections drifted from the root authority (#7029): {drifted:?}; \
         regenerate with `cargo xtask features regen-vendored`"
    );
}

#[test]
fn authority_header_declares_projection_relationship() {
    // The authority must state that crate-local features_sot.toml files are
    // generated projections, so a future editor cannot reintroduce a rival
    // "single source of truth" header (#7029 negative control).
    let root = workspace_root();
    let text = must(
        String::from_utf8(read_bytes(&root.join("features.toml")))
            .map_err(|error| format!("catalog is valid UTF-8: {error}")),
    );
    assert!(
        text.contains("GENERATED PROJECTIONS"),
        "authority header must declare the vendored-projection relationship (#7029)"
    );
}
