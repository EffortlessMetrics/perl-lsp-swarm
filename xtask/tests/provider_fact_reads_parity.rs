//! Cross-implementation parity for the provider fact-read projection (#6815).
//!
//! `docs/project/status/provider_fact_reads.md` has two independent renderers:
//! `scripts/ci/validate_provider_fact_reads.py::render_status`, which gates pull
//! requests, and `xtask/src/tasks/update_status/provider_fact_reads.rs`, which owns the
//! `update-status` write path. Each hand-maintains the same header boilerplate, table
//! headers, and claim-boundary bullets in a different language.
//!
//! Nothing previously compared them. The Python renderer is pinned to the committed
//! document by the policy workflow, but the Rust renderer only runs post-merge under
//! `update-status --write`, where the provider projection is regenerated and then
//! discarded — it is not among the canonical files that job commits. So a divergence
//! between the two would surface only as a later, unrelated pull request failing the
//! policy gate on a document it never touched.
//!
//! Pinning the Rust renderer to the same committed bytes closes that: with both
//! renderers checked against one artifact, they cannot silently disagree.

use std::path::PathBuf;
use std::process::Command;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest directory always has a repository parent")
        .to_path_buf()
}

#[test]
fn rust_generator_agrees_with_committed_provider_projection() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .current_dir(repository_root())
        .args(["update-status", "--check", "--only", "provider-facts"])
        .output()
        .expect("xtask binary should be runnable from the integration test");

    assert!(
        output.status.success(),
        "the Rust provider fact-read renderer no longer reproduces the committed \
         docs/project/status/provider_fact_reads.md, so it has drifted from the Python \
         renderer that gates pull requests.\n\
         Run `cargo xtask update-status --write --only provider-facts` and confirm \
         `python3 scripts/ci/validate_provider_fact_reads.py` still accepts the result.\n\
         --- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
