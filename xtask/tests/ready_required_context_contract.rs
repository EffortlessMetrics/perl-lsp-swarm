//! Pin the PR-title required context at the native Draft/Ready boundary.
//!
//! Draft metadata activity must not create `validate-title`, including as a
//! successful skipped job. The static required producer is requested only by
//! `ready_for_review`; manual dispatch remains a separate authority.

use std::fs;
use std::path::Path;

const WORKFLOW: &str = ".github/workflows/pr-title-check.yml";
const READY_TYPES: &str = "types: [ready_for_review]";
const MUTATED_TYPES: &str = "types: [ready_for_review, synchronize]";
const STATIC_CONTEXT: &str = "  validate-title:\n    name: validate-title";
const DYNAMIC_CONTEXT: &str = "  validate-title:\n    name: dynamic-validate-title";
const MISSING_READY: &str = "missing exact Ready-only transition";
const MISSING_STATIC: &str = "missing static required producer";

fn source() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    fs::read_to_string(root.join(WORKFLOW)).expect("read PR title workflow")
}

fn findings(source: &str) -> Vec<&'static str> {
    let mut result = Vec::new();
    if !source.contains(READY_TYPES) {
        result.push(MISSING_READY);
    }
    if !source.contains(STATIC_CONTEXT) {
        result.push(MISSING_STATIC);
    }
    if !source.contains("  pull_request_target:\n") {
        result.push("metadata workflow lost pull_request_target");
    }
    if !source.contains("  workflow_dispatch: {}") {
        result.push("manual authority disappeared");
    }
    if source.contains("github.event.pull_request.draft")
        || source.contains("validate-title-draft-advisory")
    {
        result.push("Draft job guard or pseudo-context remains");
    }
    result
}

#[test]
fn draft_activity_cannot_create_the_required_title_context() {
    assert!(findings(&source()).is_empty());
}

#[test]
fn contract_rejects_mutation_triggers() {
    let source = source();
    let changed = source.replace(READY_TYPES, MUTATED_TYPES);
    assert_ne!(changed, source);
    assert!(findings(&changed).contains(&MISSING_READY));
}

#[test]
fn contract_rejects_a_dynamic_or_renamed_required_context() {
    let source = source();
    let changed = source.replace(STATIC_CONTEXT, DYNAMIC_CONTEXT);
    assert_ne!(changed, source);
    assert!(findings(&changed).contains(&MISSING_STATIC));
}
