//! Contract tests for PLSP-ADR-0009 / issue #14304 inventory and regressions.

use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask manifest has a parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    fs::read_to_string(project_root().join(relative))
        .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"))
}

#[test]
fn adr_documents_independent_evidence_principle() {
    let adr = read("docs/adr/PLSP-ADR-0009-safety-bearing-boolean-evidence.md");
    for needle in [
        "Independent evidence path",
        "Documented exemption",
        "Review gate",
        "Known instances (inventory)",
        "WP-1",
        "WP-2",
        "WP-3",
        "CO-1",
        "CO-2",
        "CO-3",
        "CO-4",
        "CO-5",
    ] {
        assert!(adr.contains(needle), "ADR missing section or instance {needle}");
    }
}

#[test]
fn adr_inventory_anchors_file_paths_and_line_refs() {
    let adr = read("docs/adr/PLSP-ADR-0009-safety-bearing-boolean-evidence.md");
    for anchor in [
        "writer_preflight/decision.rs:619",
        "writer_preflight/observation.rs:142",
        "tasks/writer_admission.rs:854",
        "authority_transfer_closeout.rs:296",
        "authority_transfer_closeout.rs:1046",
        "authority_transfer_closeout.rs:630",
    ] {
        assert!(
            adr.contains(anchor),
            "ADR inventory missing location anchor {anchor}"
        );
    }
}

#[test]
fn xtask_agents_md_points_at_adr_and_review_gate() {
    let agents = read("xtask/AGENTS.md");
    assert!(agents.contains("PLSP-ADR-0009"));
    assert!(agents.contains("Review obligation"));
    assert!(agents.contains("resume_or_mutate_with_unregistered_candidate_refuses"));
}

#[test]
fn working_tree_disposition_has_no_pass_permitting_default() {
    let observation = read("xtask/src/writer_preflight/observation.rs");
    let struct_marker = "pub struct WorkingTreeDisposition";
    let start = observation.find(struct_marker).expect("WorkingTreeDisposition struct");
    let prefix = &observation[..start];
    let derive_start =
        prefix.rfind("#[derive").expect("WorkingTreeDisposition should have a derive attribute");
    let derive_block = &observation[derive_start..start];
    assert!(
        !derive_block.contains("Default"),
        "WorkingTreeDisposition must not derive Default: {derive_block}"
    );
    assert!(
        observation.contains("There is deliberately **no `Default`**"),
        "type must document the no-Default rationale (#14304)"
    );
    assert!(
        !observation.contains("impl Default for WorkingTreeDisposition"),
        "WorkingTreeDisposition must not implement Default manually"
    );
}

#[test]
fn writer_preflight_regression_pins_empty_worktree_map() {
    let tests = read("xtask/src/writer_preflight/tests.rs");
    assert!(
        tests.contains("fn resume_or_mutate_with_unregistered_candidate_refuses"),
        "missing P1 regression for resume/mutate + empty worktree map"
    );
}

#[test]
fn closeout_regression_pins_executed_against_head_bind() {
    let closeout = read("xtask/src/authority_transfer_closeout.rs");
    assert!(
        closeout.contains("fn controls_executed_elsewhere_cannot_claim_subject_alignment"),
        "missing regression binding subject_matches_candidate to candidate head"
    );
    assert!(
        closeout.contains("executed_against_sha != request.candidate.head_sha"),
        "evaluate must cross-check control head against candidate head"
    );
}

#[test]
fn adr_readme_indexes_plsp_adr_0009() {
    let readme = read("docs/adr/README.md");
    assert!(readme.contains("PLSP-ADR-0009"), "ADR index must list PLSP-ADR-0009");
}
