//! Compiler-critical test topology (#12125, parent controller #8437).
//!
//! Machine-owned inventory of every governed Cargo execution subject on the
//! compiler-convergence critical path: stable identities, package/path/kind
//! facts, required feature subjects referencing the #3790/#8121 authorities,
//! proof roles, candidate visibility profiles, and owner/controller
//! references — with compile obligations and execution claims represented
//! separately (the inventory never claims execution).
//!
//! Command surface:
//!
//! ```text
//! cargo xtask test-topology inventory --cohort compiler-critical
//! cargo xtask test-topology check      --cohort compiler-critical
//! cargo xtask test-topology report     --cohort compiler-critical
//! ```
//!
//! The committed JSON/Markdown inventories are generated artifacts registered
//! in `policy/generated-allowlist.toml` under
//! `generated-test-topology-compiler-critical-{json,md}`.

pub mod discovery;
pub mod model;
pub mod projections;
pub mod validation;

use std::path::Path;

use anyhow::Context;
use anyhow::bail;

pub use discovery::{Cohort, DiscoveredTarget, discover_from_metadata, discover_live};
pub use model::{
    ExecutionClaimV1, FeatureSubjectV1, ProofRoleV1, TargetKindV1, TestTopologyInventoryV1,
    TestTopologyRowV1, inventory_from_json,
};
pub use projections::{render_json, render_markdown, render_report};
pub use validation::{Violation, ensure_current, validate_inventory};

/// Committed JSON inventory path (workspace-relative, forward slashes).
pub const OUTPUT_JSON: &str = "docs/policy/TEST_TOPOLOGY_COMPILER_CRITICAL.json";

/// Committed Markdown inventory path (workspace-relative, forward slashes).
pub const OUTPUT_MD: &str = "docs/policy/TEST_TOPOLOGY_COMPILER_CRITICAL.md";

/// Controller set recorded in generated inventories.
const INVENTORY_CONTROLLERS: [&str; 4] = ["#12075", "#12078", "#12079", "#8437"];

/// Discovers the cohort and builds the canonical inventory object.
pub fn build_cohort_inventory(root: &Path) -> anyhow::Result<TestTopologyInventoryV1> {
    let discovered = discover_live(root)?;
    rows_into_inventory(discovered)
}

/// Converts discovered targets into a validated inventory.
pub fn rows_into_inventory(
    discovered: Vec<DiscoveredTarget>,
) -> anyhow::Result<TestTopologyInventoryV1> {
    let mut rows = Vec::with_capacity(discovered.len());
    for target in discovered {
        rows.push(
            target
                .topology_row()
                .with_context(|| format!("building topology row for {}", target.target_id))?,
        );
    }
    TestTopologyInventoryV1::new(
        Cohort::CompilerCritical.as_slug(),
        &format!(
            "cargo xtask test-topology inventory --cohort {}",
            Cohort::CompilerCritical.as_slug()
        ),
        &INVENTORY_CONTROLLERS.iter().map(|value| (*value).to_string()).collect::<Vec<_>>(),
        rows,
    )
}

/// `test-topology inventory`: regenerate and write the committed artifacts.
///
/// Returns a short human summary; printing belongs to the caller so this
/// library stays free of stdout writes.
pub fn run_inventory(root: &Path) -> anyhow::Result<String> {
    let inventory = build_cohort_inventory(root)?;
    let json = render_json(&inventory)?;
    let markdown = render_markdown(&inventory);
    write_artifact(root, OUTPUT_JSON, &json)?;
    write_artifact(root, OUTPUT_MD, &markdown)?;
    Ok(format!(
        "wrote {} ({} rows) and {} for cohort {}",
        OUTPUT_JSON,
        inventory.rows.len(),
        OUTPUT_MD,
        inventory.cohort,
    ))
}

/// `test-topology check`: validate the committed inventory against live
/// discovery. Returns the pass summary or an error listing every violation.
pub fn run_check(root: &Path) -> anyhow::Result<String> {
    let committed = load_committed_inventory(root)?;
    let discovered =
        discover_live(root).context("discovering live cohort subjects for the topology check")?;
    ensure_current(&committed, &discovered)?;
    ensure_markdown_projection(root, &committed)?;
    Ok(format!(
        "test-topology check passed: {} committed row(s) match live discovery for cohort {}",
        committed.rows.len(),
        committed.cohort,
    ))
}

/// `test-topology report`: deterministic human summary of the committed
/// inventory.
pub fn run_report(root: &Path) -> anyhow::Result<String> {
    let committed = load_committed_inventory(root)?;
    Ok(render_report(&committed))
}

fn load_committed_inventory(root: &Path) -> anyhow::Result<TestTopologyInventoryV1> {
    let path = root.join(OUTPUT_JSON);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("reading committed inventory {}", path.display()))?;
    inventory_from_json(&text).with_context(|| format!("validating {}", path.display()))
}

/// Verifies the committed Markdown projection is a canonical re-render of the
/// committed JSON inventory, so readers never consume a stale, truncated, or
/// hand-edited human view after `check` reports success.
fn ensure_markdown_projection(
    root: &Path,
    inventory: &TestTopologyInventoryV1,
) -> anyhow::Result<()> {
    let path = root.join(OUTPUT_MD);
    let committed =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let expected = render_markdown(inventory);
    if committed != expected {
        bail!(
            "stale Markdown projection at {}: the committed file no longer matches the \
             canonical re-render of the JSON inventory; run `cargo xtask test-topology \
             inventory --cohort {}` to regenerate it",
            OUTPUT_MD,
            inventory.cohort
        );
    }
    Ok(())
}

fn write_artifact(root: &Path, relative: &str, contents: &str) -> anyhow::Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&path, contents).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Guard against accidental stdout coupling: runners return summaries.
    #[test]
    fn artifact_paths_are_workspace_relative_forward_slashed() {
        assert!(!OUTPUT_JSON.starts_with('/') && !OUTPUT_JSON.contains('\\'));
        assert!(!OUTPUT_MD.starts_with('/') && !OUTPUT_MD.contains('\\'));
    }

    #[test]
    fn missing_manifest_facts_are_a_loud_discovery_error() -> anyhow::Result<()> {
        let metadata = r#"{
            "packages": [{
                "name": "perl-parser-core",
                "manifest_path": "Z:/ws/crates/perl-parser-core/Cargo.toml",
                "targets": []
            }],
            "workspace_root": "Z:/ws"
        }"#;
        let error = discover_from_metadata(metadata, &BTreeMap::new())
            .err()
            .ok_or_else(|| anyhow::anyhow!("missing manifest facts must fail discovery"))?;
        assert!(
            format!("{error:#}").contains("manifest cross-check"),
            "unexpected error: {error:#}"
        );
        Ok(())
    }

    /// The Markdown projection is verified against a canonical re-render, so
    /// stale, truncated, or hand-edited artifacts fail the check loudly
    /// instead of silently passing while the JSON stays current.
    #[test]
    fn stale_or_edited_markdown_projection_fails_the_check() -> anyhow::Result<()> {
        let root = std::env::temp_dir()
            .join(format!("test-topology-md-projection-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        let result = (|| -> anyhow::Result<()> {
            std::fs::create_dir_all(root.join("docs").join("policy"))?;
            let inventory = TestTopologyInventoryV1::new(
                "compiler-critical",
                "cargo xtask test-topology inventory --cohort compiler-critical",
                &["#8437".to_string()],
                Vec::new(),
            )?;
            std::fs::write(root.join(OUTPUT_JSON), render_json(&inventory)?)?;
            std::fs::write(root.join(OUTPUT_MD), render_markdown(&inventory))?;
            ensure_markdown_projection(&root, &inventory)?;

            let truncated = "AUTO-GENERATED\n\nrows: (hand-truncated)\n";
            std::fs::write(root.join(OUTPUT_MD), truncated)?;
            let error = ensure_markdown_projection(&root, &inventory).err().ok_or_else(|| {
                anyhow::anyhow!("tampered Markdown projection must fail the check")
            })?;
            assert!(
                format!("{error:#}").contains("stale Markdown projection"),
                "unexpected error: {error:#}"
            );
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&root);
        result
    }
}
