//! Discriminating tests for the `native_neovim_train.v1` manifest check.
//!
//! Every shift-left rejection named on issue #11392 is proven against a real
//! mutation that must fail with exactly the named diagnostic, and the
//! canonical manifest plus shuffled control must pass cleanly.

use super::{MANIFEST_PATH, SHUFFLED_PATH, canonical_form, validate_document};
use serde_json::Value;
use std::path::Path;

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn load(rel: &str) -> Value {
    let bytes = std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|error| panic!("failed to read {rel}: {error}"));
    serde_json::from_str(&bytes).unwrap_or_else(|error| panic!("invalid JSON in {rel}: {error}"))
}

fn codes(doc: &Value) -> Vec<String> {
    validate_document(doc).iter().map(|violation| violation.code.clone()).collect()
}

#[test]
fn canonical_manifest_is_clean() {
    let doc = load(MANIFEST_PATH);
    assert!(
        validate_document(&doc).is_empty(),
        "the landed native Neovim train manifest must validate"
    );
}

#[test]
fn shuffled_control_canonizes_identically_and_validates() {
    let base = load(MANIFEST_PATH);
    let shuffled = load(SHUFFLED_PATH);
    assert_eq!(
        canonical_form(&base),
        canonical_form(&shuffled),
        "serialization and ordinary validation must stay deterministic under reordering"
    );
    assert!(validate_document(&shuffled).is_empty());
}

#[test]
fn every_expected_invalid_fixture_fails_with_named_code() {
    let expected: serde_json::Map<String, Value> =
        load(".spec/11392-native-neovim-train-graph/invalid/expected_errors.json")
            .as_object()
            .cloned()
            .expect("expected_errors.json must be an object");
    assert!(expected.len() >= 15, "all 15 rejection classes stay discriminated");
    for (filename, expected_code) in &expected {
        let expected_code = expected_code.as_str().expect("string reason code");
        let doc = load(&format!(".spec/11392-native-neovim-train-graph/invalid/{filename}"));
        let actual = codes(&doc);
        assert!(!actual.is_empty(), "invalid/{filename} unexpectedly validated cleanly");
        assert!(
            actual.iter().any(|code| code == expected_code),
            "invalid/{filename}: expected {expected_code}, got {actual:?}"
        );
    }
}

fn mutate_base(mutation: impl FnOnce(&mut Value)) -> Value {
    let mut doc = load(MANIFEST_PATH);
    mutation(&mut doc);
    doc
}

fn find_node<'a>(doc: &'a mut Value, node_id: &str) -> &'a mut Value {
    doc.get_mut("nodes")
        .and_then(Value::as_array_mut)
        .and_then(|nodes| {
            nodes
                .iter_mut()
                .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        })
        .expect("node exists in the base manifest")
}

#[test]
fn undeclared_selecting_authority_fails_closed() {
    let doc = mutate_base(|doc| {
        let gate = find_node(doc, "nv_atomic_release_dependents_gate")
            .get_mut("release_gates")
            .and_then(Value::as_array_mut)
            .and_then(|gates| gates.first_mut())
            .expect("gate row present");
        gate["selecting_authority"] = Value::String("undeclared_authority".to_string());
    });
    assert!(codes(&doc).contains(&"UNQUALIFIED_RELEASE_GATE".to_string()));
}

#[test]
fn selected_value_outside_allowed_values_fails_closed() {
    let doc = mutate_base(|doc| {
        let gate = find_node(doc, "nv_release_bounded_v0_18_envelope")
            .get_mut("release_gates")
            .and_then(Value::as_array_mut)
            .and_then(|gates| gates.first_mut())
            .expect("gate row present");
        gate["selected_value"] = Value::String("something_else_entirely".to_string());
    });
    assert!(codes(&doc).contains(&"UNQUALIFIED_RELEASE_GATE".to_string()));
}

#[test]
fn unknown_profile_member_fails() {
    let doc = mutate_base(|doc| {
        let profile =
            doc.get_mut("claim_profiles").and_then(Value::as_array_mut).expect("profiles array");
        for profile in profile.iter_mut() {
            if profile.get("id").and_then(Value::as_str) == Some("native_neovim_core") {
                profile["members"]
                    .as_array_mut()
                    .expect("members array")
                    .insert(0, Value::String("nv_missing_row_xyz".to_string()));
                return;
            }
        }
        panic!("core profile present");
    });
    assert!(codes(&doc).contains(&"UNKNOWN_PROFILE_MEMBER".to_string()));
}

#[test]
fn fan_in_composing_an_instrument_child_fails() {
    let doc = mutate_base(|doc| {
        let fan_in = find_node(doc, "nv_core_fanin_exact_subject_receipts")
            .get_mut("fan_in")
            .and_then(Value::as_object_mut)
            .expect("fan-in present");
        fan_in
            .get_mut("children")
            .and_then(Value::as_array_mut)
            .expect("children array")
            .push(Value::String("nv_host_toolchain_leaf".to_string()));
    });
    assert!(codes(&doc).contains(&"FAN_IN_INVALID_COMPOSITION".to_string()));
}

#[test]
fn duplicate_primary_issue_anchor_fails() {
    let doc = mutate_base(|doc| {
        // #8129 already anchors the release-decision controller; reusing that
        // anchor for the durable spec row must fail closed.
        let issue = find_node(doc, "nv_ctrl_release_decision")
            .get("issue")
            .cloned()
            .expect("controller has an issue");
        find_node(doc, "nv_spec_train_durable")["issue"] = issue;
    });
    assert!(codes(&doc).contains(&"DUPLICATE_PRIMARY_ISSUE".to_string()));
}

#[test]
fn duplicate_claim_profile_identity_fails() {
    let doc = mutate_base(|doc| {
        let profiles =
            doc.get_mut("claim_profiles").and_then(Value::as_array_mut).expect("profiles array");
        let clone = profiles.first().cloned().expect("at least one profile");
        profiles.push(clone);
    });
    assert!(codes(&doc).contains(&"DUPLICATE_PROFILE_IDENTITY".to_string()));
}

#[test]
fn internal_class_targeting_an_external_authority_fails() {
    let doc = mutate_base(|doc| {
        let deps = find_node(doc, "nv_core_slice_attach_root_effects")
            .get_mut("dependencies")
            .and_then(Value::as_array_mut)
            .expect("deps present");
        deps[0]["target"] = Value::String("ext_mason_registry".to_string());
    });
    assert!(codes(&doc).contains(&"INTERNAL_TARGET_NAMESPACE".to_string()));
}

/// The gate command itself applies the JSON Schema plus graph semantics end
/// to end; this keeps the automated surface equivalent to `run()`, not merely
/// the in-process semantic layer.
#[test]
fn gate_command_run_is_green_on_the_landed_tree() {
    super::run().expect("check-native-neovim-train must stay green on the landed tree");
}
