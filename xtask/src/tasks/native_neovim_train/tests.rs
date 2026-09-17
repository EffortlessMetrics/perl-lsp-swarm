//! Discriminating tests for the `native_neovim_train.v1` manifest check.
//!
//! Every shift-left rejection named on issue #11392 is proven against a real
//! mutation that must fail with exactly the named diagnostic, and the
//! canonical manifest plus shuffled control must pass cleanly.

use super::{MANIFEST_PATH, SHUFFLED_PATH, canonical_form, validate_document};
use perl_tdd_support::{must_some_with, must_with};
use serde_json::Value;
use std::path::PathBuf;

#[track_caller]
fn repo_root() -> PathBuf {
    must_with(crate::utils::project_root(), "workspace project root")
}

#[track_caller]
fn parse_json(rel: &str, bytes: &str) -> Value {
    must_with(serde_json::from_str(bytes), format_args!("invalid JSON in {rel}"))
}

#[track_caller]
fn load(rel: &str) -> Value {
    let bytes = must_with(
        std::fs::read_to_string(repo_root().join(rel)),
        format_args!("failed to read {rel}"),
    );
    parse_json(rel, &bytes)
}

#[track_caller]
fn json_object(value: Value, context: &'static str) -> serde_json::Map<String, Value> {
    must_some_with(value.as_object().cloned(), context)
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
    let expected = json_object(
        load(".spec/11392-native-neovim-train-graph/invalid/expected_errors.json"),
        "expected_errors.json must be an object",
    );
    assert!(expected.len() >= 15, "all 15 rejection classes stay discriminated");
    for (filename, expected_code) in &expected {
        let expected_code = must_some_with(expected_code.as_str(), "string reason code");
        let doc = load(&format!(".spec/11392-native-neovim-train-graph/invalid/{filename}"));
        let actual = codes(&doc);
        assert!(!actual.is_empty(), "invalid/{filename} unexpectedly validated cleanly");
        assert!(
            actual.iter().any(|code| code == expected_code),
            "invalid/{filename}: expected {expected_code}, got {actual:?}"
        );
    }
}

#[track_caller]
fn mutate_base(mutation: impl FnOnce(&mut Value)) -> Value {
    let mut doc = load(MANIFEST_PATH);
    mutation(&mut doc);
    doc
}

#[track_caller]
fn find_node<'a>(doc: &'a mut Value, node_id: &str) -> &'a mut Value {
    must_some_with(
        doc.get_mut("nodes").and_then(Value::as_array_mut).and_then(|nodes| {
            nodes
                .iter_mut()
                .find(|node| node.get("node_id").and_then(Value::as_str) == Some(node_id))
        }),
        "node exists in the base manifest",
    )
}

#[track_caller]
fn first_release_gate<'a>(doc: &'a mut Value, node_id: &str) -> &'a mut Value {
    must_some_with(
        find_node(doc, node_id)
            .get_mut("release_gates")
            .and_then(Value::as_array_mut)
            .and_then(|gates| gates.first_mut()),
        "gate row present",
    )
}

#[track_caller]
fn claim_profiles(doc: &mut Value) -> &mut Vec<Value> {
    must_some_with(doc.get_mut("claim_profiles").and_then(Value::as_array_mut), "profiles array")
}

#[test]
fn undeclared_selecting_authority_fails_closed() {
    let doc = mutate_base(|doc| {
        first_release_gate(doc, "nv_atomic_release_dependents_gate")["selecting_authority"] =
            Value::String("undeclared_authority".to_string());
    });
    assert!(codes(&doc).contains(&"UNQUALIFIED_RELEASE_GATE".to_string()));
}

#[test]
fn selected_value_outside_allowed_values_fails_closed() {
    let doc = mutate_base(|doc| {
        first_release_gate(doc, "nv_release_bounded_v0_18_envelope")["selected_value"] =
            Value::String("something_else_entirely".to_string());
    });
    assert!(codes(&doc).contains(&"UNQUALIFIED_RELEASE_GATE".to_string()));
}

#[test]
fn unknown_profile_member_fails() {
    let doc = mutate_base(|doc| {
        let core = must_some_with(
            claim_profiles(doc).iter_mut().find(|profile| {
                profile.get("id").and_then(Value::as_str) == Some("native_neovim_core")
            }),
            "core profile present",
        );
        must_some_with(core.get_mut("members").and_then(Value::as_array_mut), "members array")
            .insert(0, Value::String("nv_missing_row_xyz".to_string()));
    });
    assert!(codes(&doc).contains(&"UNKNOWN_PROFILE_MEMBER".to_string()));
}

#[test]
fn fan_in_composing_an_instrument_child_fails() {
    let doc = mutate_base(|doc| {
        let fan_in = must_some_with(
            find_node(doc, "nv_core_fanin_exact_subject_receipts")
                .get_mut("fan_in")
                .and_then(Value::as_object_mut),
            "fan-in present",
        );
        must_some_with(fan_in.get_mut("children").and_then(Value::as_array_mut), "children array")
            .push(Value::String("nv_host_toolchain_leaf".to_string()));
    });
    assert!(codes(&doc).contains(&"FAN_IN_INVALID_COMPOSITION".to_string()));
}

#[test]
fn duplicate_primary_issue_anchor_fails() {
    let doc = mutate_base(|doc| {
        // #8129 already anchors the release-decision controller; reusing that
        // anchor for the durable spec row must fail closed.
        let issue = must_some_with(
            find_node(doc, "nv_ctrl_release_decision").get("issue").cloned(),
            "controller has an issue",
        );
        find_node(doc, "nv_spec_train_durable")["issue"] = issue;
    });
    assert!(codes(&doc).contains(&"DUPLICATE_PRIMARY_ISSUE".to_string()));
}

#[test]
fn duplicate_claim_profile_identity_fails() {
    let doc = mutate_base(|doc| {
        let profiles = claim_profiles(doc);
        let clone = must_some_with(profiles.first().cloned(), "at least one profile");
        profiles.push(clone);
    });
    assert!(codes(&doc).contains(&"DUPLICATE_PROFILE_IDENTITY".to_string()));
}

#[test]
fn internal_class_targeting_an_external_authority_fails() {
    let doc = mutate_base(|doc| {
        let deps = must_some_with(
            find_node(doc, "nv_core_slice_attach_root_effects")
                .get_mut("dependencies")
                .and_then(Value::as_array_mut),
            "deps present",
        );
        must_some_with(deps.first_mut(), "first dependency")["target"] =
            Value::String("ext_mason_registry".to_string());
    });
    assert!(codes(&doc).contains(&"INTERNAL_TARGET_NAMESPACE".to_string()));
}

/// The gate command itself applies the JSON Schema plus graph semantics end
/// to end; this keeps the automated surface equivalent to `run()`, not merely
/// the in-process semantic layer.
#[test]
fn gate_command_run_is_green_on_the_landed_tree() {
    must_with(super::run(), "check-native-neovim-train must stay green on the landed tree");
}

#[test]
fn native_neovim_train_converted_must_wrappers_carry_track_caller() {
    let src = include_str!("tests.rs");
    let mut failures = Vec::new();
    for helper in [
        "fn repo_root(",
        "fn parse_json(",
        "fn load(",
        "fn json_object(",
        "fn mutate_base(",
        "fn find_node<",
        "fn first_release_gate<",
        "fn claim_profiles(",
    ] {
        let Some(idx) = src.find(helper) else {
            failures.push(format!("missing wrapper {helper}"));
            continue;
        };
        let preceding = src.get(..idx).unwrap_or("");
        let last_attr_line =
            preceding.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("");
        if last_attr_line.trim() != "#[track_caller]" {
            failures.push(format!(
                "{helper} is not immediately preceded by #[track_caller] (found {last_attr_line:?})"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[should_panic(expected = "must: failed to read does-not-exist.json:")]
fn native_neovim_train_converted_load_still_fails_when_the_file_is_missing() {
    let _ = load("does-not-exist.json");
}

#[test]
#[should_panic(expected = "must: invalid JSON in bogus.json:")]
fn native_neovim_train_converted_parse_json_still_fails_when_bytes_are_not_json() {
    let _ = parse_json("bogus.json", "not-json");
}

#[test]
#[should_panic(expected = "must_some: expected_errors.json must be an object:")]
fn native_neovim_train_converted_json_object_still_fails_when_value_is_not_an_object() {
    let _ = json_object(Value::Array(Vec::new()), "expected_errors.json must be an object");
}

#[test]
#[should_panic(expected = "must_some: node exists in the base manifest:")]
fn native_neovim_train_converted_find_node_still_fails_when_the_node_is_absent() {
    let mut doc = Value::Object(serde_json::Map::new());
    let _ = find_node(&mut doc, "missing");
}

#[test]
#[should_panic(expected = "must_some: gate row present:")]
fn native_neovim_train_converted_first_release_gate_still_fails_when_gates_are_absent() {
    let mut doc = serde_json::json!({
        "nodes": [{"node_id": "no_gates"}]
    });
    let _ = first_release_gate(&mut doc, "no_gates");
}

#[test]
#[should_panic(expected = "must_some: profiles array:")]
fn native_neovim_train_converted_claim_profiles_still_fails_when_profiles_are_absent() {
    let mut doc = Value::Object(serde_json::Map::new());
    let _ = claim_profiles(&mut doc);
}
