//! Pin the PR-title context split at the native Draft/Ready boundary.
//!
//! Draft metadata feedback may still execute, but only Ready/non-draft events
//! may emit the ruleset-required `validate-title` check name.

use std::fs;
use std::path::Path;

const WORKFLOW: &str = ".github/workflows/pr-title-check.yml";
const READY_TYPES: &str =
    "types: [opened, edited, reopened, synchronize, ready_for_review]";
const CONTEXT_SPLIT: &str =
    "name: ${{ github.event_name == 'pull_request_target' && github.event.pull_request.draft == true && 'validate-title-draft-advisory' || 'validate-title' }}";

fn source() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    fs::read_to_string(root.join(WORKFLOW)).expect("read PR title workflow")
}

fn findings(source: &str) -> Vec<&'static str> {
    let mut result = Vec::new();
    if !source.contains(READY_TYPES) {
        result.push("missing Ready transition");
    }
    if !source.contains(CONTEXT_SPLIT) {
        result.push("missing Draft/Ready context split");
    }
    if !source.contains("  pull_request_target:\n") {
        result.push("metadata workflow lost pull_request_target");
    }
    if !source.contains("  workflow_dispatch: {}") {
        result.push("manual authority disappeared");
    }
    result
}

#[test]
fn draft_title_feedback_cannot_impersonate_the_required_context() {
    let source = source();
    assert!(findings(&source).is_empty(), "{:?}", findings(&source));
    assert!(source.contains("validate-title-draft-advisory"));
    assert!(source.contains("'validate-title'"));
}

#[test]
fn contract_rejects_static_required_context_on_drafts() {
    let source = source();
    let changed = source.replace(CONTEXT_SPLIT, "name: validate-title");
    assert_ne!(changed, source, "context-name mutation must engage");
    assert!(
        findings(&changed).contains(&"missing Draft/Ready context split"),
        "static required name must be rejected"
    );
}

#[test]
fn contract_rejects_missing_ready_transition() {
    let source = source();
    let changed = source.replace(", ready_for_review", "");
    assert_ne!(changed, source, "Ready-event mutation must engage");
    assert!(
        findings(&changed).contains(&"missing Ready transition"),
        "missing Ready event must be rejected"
    );
}
