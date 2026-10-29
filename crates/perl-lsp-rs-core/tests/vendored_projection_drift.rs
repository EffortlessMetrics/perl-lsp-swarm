//! Vendored catalog projection drift guard (#7029 / #9199).
//!
//! Root `features.toml` is the single human-edited authority. Each crate-local
//! `features_sot.toml` is a deterministic byte projection of that authority so
//! standalone/packaged builds resolve an identical catalog. This live-tree test
//! fails when any crate-local fallback drifts; regenerate with
//! `cargo xtask features regen-vendored`. Fixture-level negative controls live
//! in `xtask/tests/vendored_catalog_projections.rs`.

use std::path::{Path, PathBuf};

use perl_tdd_support::{must, must_some};

fn workspace_root() -> PathBuf {
    must_some(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).map(Path::to_path_buf))
}

fn read_bytes(path: &Path) -> Vec<u8> {
    must(
        std::fs::read(path)
            .map_err(|error| format!("cannot read catalog projection {}: {error}", path.display())),
    )
}

fn crate_local_fallbacks(root: &Path) -> Vec<PathBuf> {
    let crates = root.join("crates");
    let entries = must(
        std::fs::read_dir(&crates)
            .map_err(|error| format!("cannot read {}: {error}", crates.display())),
    );
    let mut paths = Vec::new();
    for entry in entries {
        let entry = must(entry.map_err(|error| format!("cannot read crates/ entry: {error}")));
        let file_type = must(
            entry
                .file_type()
                .map_err(|error| format!("cannot stat {}: {error}", entry.path().display())),
        );
        if !file_type.is_dir() {
            continue;
        }
        let candidate = entry.path().join("features_sot.toml");
        if candidate.is_file() {
            paths.push(candidate);
        }
    }
    paths.sort();
    paths
}

#[test]
fn every_vendored_projection_is_byte_identical_to_the_authority() {
    let root = workspace_root();
    let authority = read_bytes(&root.join("features.toml"));
    let projections = crate_local_fallbacks(&root);
    assert!(
        !projections.is_empty(),
        "workspace must retain crate-local features_sot.toml fallbacks (#9199)"
    );

    let mut drifted = Vec::new();
    for path in &projections {
        let vendored = read_bytes(path);
        if vendored != authority {
            drifted.push(path.display().to_string());
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
