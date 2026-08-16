#[path = "workspace_doctor_inventory/support.rs"]
mod support;

use std::fs;
use support::fixture_root;
use support::workspace_doctor_inventory::{
    Disposition, MutationPosture, ResultClass, build_inventory, canonical_rows, validate_inventory,
    validate_rows,
};

#[test]
fn new_doctor_check_cannot_be_omitted() {
    let temp = fixture_root();
    let path = temp.path().join("justfile");
    let text = fs::read_to_string(&path).expect("read justfile");
    fs::write(
        path,
        text.replace(
            "    echo \"$issues issues found, $fixed auto-fixed\"",
            "    # Check 8: New unclassified fact\n    echo \"new fact\"\n    echo \"$issues issues found, $fixed auto-fixed\"",
        ),
    )
    .expect("write justfile");
    assert!(build_inventory(temp.path()).is_err());
}

#[test]
fn new_active_mutation_cannot_be_omitted() {
    let temp = fixture_root();
    let path = temp.path().join("justfile");
    let text = fs::read_to_string(&path).expect("read justfile");
    fs::write(
        path,
        text.replace(
            "    echo \"$issues issues found, $fixed auto-fixed\"",
            "    git reset --hard HEAD\n    echo \"$issues issues found, $fixed auto-fixed\"",
        ),
    )
    .expect("write justfile");
    assert!(build_inventory(temp.path()).is_err());
}

#[test]
fn automatic_mutation_cannot_be_classified_read_only() {
    let mut rows = canonical_rows();
    let row = rows.iter_mut().find(|row| row.check_id == "core-bare").expect("core-bare row");
    row.current_mutation = MutationPosture::ReadOnly;
    assert!(validate_rows(&rows).is_err());
}

#[test]
fn required_block_cannot_be_downgraded() {
    let mut rows = canonical_rows();
    let row =
        rows.iter_mut().find(|row| row.check_id == "worktree-file-overlap").expect("overlap row");
    row.target_result = ResultClass::Advisory;
    assert!(validate_rows(&rows).is_err());
}

#[test]
fn unavailable_remote_cannot_be_clean() {
    let mut rows = canonical_rows();
    let row = rows
        .iter_mut()
        .find(|row| row.check_id == "default-base-unresolved")
        .expect("default base row");
    row.target_result = ResultClass::Advisory;
    assert!(validate_rows(&rows).is_err());
}

#[test]
fn one_fact_cannot_have_two_canonical_rows() {
    let mut rows = canonical_rows();
    let mut duplicate = rows[0].clone();
    duplicate.check_id = "repository-context-duplicate".to_string();
    rows.push(duplicate);
    assert!(validate_rows(&rows).is_err());
}

#[test]
fn behind_only_cannot_become_blocking() {
    let mut rows = canonical_rows();
    let row =
        rows.iter_mut().find(|row| row.check_id == "default-base-behind").expect("behind row");
    row.disposition = Disposition::RetainBlocking;
    assert!(validate_rows(&rows).is_err());
}

#[test]
fn missing_authority_marker_is_not_clean() {
    let temp = fixture_root();
    let path = temp.path().join("xtask/src/tasks/writer_admission.rs");
    let text = fs::read_to_string(&path).expect("read writer admission");
    fs::write(path, text.replace("check_writer_collision(snapshot)", ""))
        .expect("write writer admission");
    assert!(build_inventory(temp.path()).is_err());
}

#[test]
fn saved_inventory_becomes_stale_after_source_change() {
    let temp = fixture_root();
    let inventory = build_inventory(temp.path()).expect("build inventory");
    let path = temp.path().join("scripts/storage-doctor");
    let mut text = fs::read_to_string(&path).expect("read storage doctor");
    text.push_str("extra\n");
    fs::write(path, text).expect("write storage doctor");
    assert!(validate_inventory(temp.path(), &inventory).is_err());
}

#[test]
fn ready_must_compose_doctor_and_pr_fast() {
    let temp = fixture_root();
    let path = temp.path().join("justfile");
    let text = fs::read_to_string(&path).expect("read justfile");
    fs::write(path, text.replace("ready: doctor pr-fast", "ready: pr-fast"))
        .expect("write justfile");
    assert!(build_inventory(temp.path()).is_err());
}
