//! Targeted inventory for #6886: production LSP-core diagnostic placement must
//! consume parser-owned anchors instead of reconstructing offsets from variant shape.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn diagnostics_provider_delegates_placement_to_parser_anchor() {
    let path = repo_root().join("crates/perl-lsp-rs-core/src/providers/diagnostics/diagnostics.rs");
    let content = std::fs::read_to_string(&path).expect("read diagnostics.rs");
    assert!(
        !content.contains("location().unwrap_or(0)"),
        "diagnostics.rs must not guess byte zero for unknown variants (#6886)"
    );
    assert!(
        content.contains("resolved_parse_diagnostic_byte_offset"),
        "diagnostics.rs must call the parser-owned anchor helper (#6886)"
    );
}
