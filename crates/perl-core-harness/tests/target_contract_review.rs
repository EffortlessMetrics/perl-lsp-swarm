//! Review falsifiers for target-selector decoding and legacy composite partitioning.

#[path = "../src/target_contracts/model.rs"]
mod model;
#[path = "../src/target_contracts/contract.rs"]
mod contract;
#[path = "../src/target_contracts/matrix.rs"]
mod matrix;
#[path = "../src/target_contracts/io.rs"]
mod io;

use model::{CompositeOverlapPolicy, TargetSelector};
use std::path::{Path, PathBuf};

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn selector_payloads_reject_unknown_fields() {
    let misspelled_path = r#"{"kind":"recursive_root","pth":"base"}"#;
    assert!(serde_json::from_str::<TargetSelector>(misspelled_path).is_err());

    let silently_extra_field =
        r#"{"kind":"recursive_root","path":"base","scope":"recursive"}"#;
    assert!(serde_json::from_str::<TargetSelector>(silently_extra_field).is_err());
}

#[test]
fn legacy_composites_reject_overlap_and_keep_op_hook_disjoint() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = io::read_matrix(&repo_file(
        ".ci/perl-core-harness/upstream-targets-5.42.2.v1",
    ))?;

    let find = |target_id: &str| {
        matrix
            .targets
            .iter()
            .find(|entry| entry.contract.target_id == target_id)
            .map(|entry| &entry.contract)
            .ok_or_else(|| format!("missing target contract {target_id}"))
    };

    let core = find("legacy_custom_core")?;
    let full = find("legacy_custom_full")?;
    let direct_op = find("component_op")?;
    let op_hook = find("component_op_hook")?;

    assert_eq!(
        core.composite_overlap_policy,
        Some(CompositeOverlapPolicy::RejectOverlap)
    );
    assert_eq!(
        full.composite_overlap_policy,
        Some(CompositeOverlapPolicy::RejectOverlap)
    );
    assert!(core.composite_members.iter().any(|member| member == "component_op"));
    assert!(
        core.composite_members
            .iter()
            .any(|member| member == "component_op_hook")
    );
    assert_eq!(
        direct_op.selectors,
        vec![TargetSelector::NonRecursiveGlob {
            pattern: "op/*.t".to_string(),
        }]
    );
    assert_eq!(
        op_hook.selectors,
        vec![TargetSelector::RecursiveRoot {
            path: "op/hook".to_string(),
        }]
    );
    Ok(())
}
