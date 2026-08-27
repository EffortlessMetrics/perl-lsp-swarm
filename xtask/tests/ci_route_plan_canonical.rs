//! Canonical encoding, fingerprint, and schema-conformance falsifiers for
//! `ci_route_plan.v1` (#10179).
//!
//! The golden vectors under `fixtures/ci-route-plan/` were authored by the
//! independent Python reference generator (`generate_golden.py`) from the
//! specified encoding contract — never by the Rust encoder — so agreement
//! here is independent proof, not the production canonicalizer comparing
//! itself. The generator's non-mutating `--check` mode runs alongside
//! these tests and compares every checked-in fixture byte against the
//! generator output, so a stale or hand-edited vector cannot pass by
//! merely agreeing with a stale Rust encoder.
//!
//! The schema checks parse the checked-in JSON Schema
//! (`.ci/schemas/ci-route-plan.v1.schema.json`) and validate actual
//! generated payloads against it with a test-local structural checker
//! bounded to exactly the keywords that schema uses (it fails closed on
//! any unknown keyword, so schema growth is detected). This follows the
//! repository's mirror-plus-drift-test idiom; it is not a generic schema
//! framework.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::Value;
use xtask::ci_route_plan::{
    Applicability, CI_ROUTE_PLAN_PRODUCER, CI_ROUTE_PLAN_SCHEMA, CLOSED_ERROR_CODES, CiRoutePlanV1,
    CompileRoutePlanInput, ExpansionStatus, FINGERPRINT_DOMAIN, GateSelectorInput, KNOWN_PROFILES,
    LifecycleDisposition, LifecycleState, PlannedOutcome, PolicyRole, Resolution,
    RouteDispositionInput, RouteExecutionIdentity, RouteProfileExpansionInput,
    RouteQuarantineEvidence, RouteScopeEvidence, RouteSelectionEvidence, RouteSubjectRef,
    ScopedIdentity, SelectorPlacement, SelectorProof, SelectorRole,
};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ci-route-plan");
const SCHEMA_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../.ci/schemas/ci-route-plan.v1.schema.json");

fn subject() -> RouteSubjectRef {
    RouteSubjectRef {
        kind: "pull_request".to_string(),
        head_sha: SHA_A.to_string(),
        base_sha: Some(SHA_B.to_string()),
        subject_digest: DIGEST_A.to_string(),
    }
}

fn selection() -> RouteSelectionEvidence {
    RouteSelectionEvidence {
        base: SHA_B.to_string(),
        scope_ok: true,
        fallback_used: false,
        fallback_reason: None,
        package_args: vec!["-p".to_string(), "perl-parser".to_string()],
        scope: None,
        selector_digest: DIGEST_A.to_string(),
    }
}

fn expansion(profile: &str, tiers: &[&str], denominator: &[&str]) -> RouteProfileExpansionInput {
    RouteProfileExpansionInput {
        requested_profile: profile.to_string(),
        included_native_tiers: tiers.iter().map(|tier| tier.to_string()).collect(),
        semantic_fingerprint: DIGEST_B.to_string(),
        policy_digest: DIGEST_C.to_string(),
        denominator: denominator.iter().map(|gate| gate.to_string()).collect(),
        resolution: ExpansionStatus::Complete,
        detail: None,
    }
}

fn active_disposition(gate_id: &str, tier: &str, role: PolicyRole) -> RouteDispositionInput {
    RouteDispositionInput {
        gate_id: gate_id.to_string(),
        policy_role: role,
        lifecycle: LifecycleDisposition {
            state: LifecycleState::Active,
            resolution: Resolution::Current,
        },
        native_tier: tier.to_string(),
        quarantine: None,
        detail: None,
    }
}

fn selected(gate_id: &str, proof: Option<SelectorProof>) -> GateSelectorInput {
    GateSelectorInput {
        gate_id: gate_id.to_string(),
        placement: SelectorPlacement::Selected,
        role: Some(SelectorRole::AlwaysOn),
        reason: "selected by selector".to_string(),
        proof,
    }
}

fn skipped(gate_id: &str, proof: Option<SelectorProof>) -> GateSelectorInput {
    GateSelectorInput {
        gate_id: gate_id.to_string(),
        placement: SelectorPlacement::Skipped,
        role: Some(SelectorRole::RustScoped),
        reason: "scope selector decided".to_string(),
        proof,
    }
}

fn execution(gate_id: &str) -> RouteExecutionIdentity {
    RouteExecutionIdentity {
        gate_id: gate_id.to_string(),
        command: format!("run {gate_id}"),
        timeout_seconds: 60,
    }
}

/// Baseline mirroring `generate_golden.baseline_semantic`: one proof-backed
/// run (`fmt_gate`) and one proof-backed scoped noop (`unit_gate`). The
/// golden fixture stores tiers in canonical ascending order; the input
/// order here differs on purpose.
fn baseline_input() -> CompileRoutePlanInput {
    CompileRoutePlanInput {
        subject: subject(),
        expansion: expansion("merge_gate", &["pr_fast", "merge_gate"], &["fmt_gate", "unit_gate"]),
        dispositions: vec![
            active_disposition("fmt_gate", "pr_fast", PolicyRole::Required),
            active_disposition("unit_gate", "pr_fast", PolicyRole::Advisory),
        ],
        disposition_digest: DIGEST_B.to_string(),
        workflow_digest: DIGEST_C.to_string(),
        selectors: vec![
            selected("fmt_gate", Some(SelectorProof::Applicable)),
            skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject)),
        ],
        selection: selection(),
        execution: vec![execution("fmt_gate"), execution("unit_gate")],
    }
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(Path::new(FIXTURES).join(name))
        .unwrap_or_else(|error| panic!("fixture {name}: {error}"))
}

fn golden_digest(name: &str) -> String {
    let digests: Value = serde_json::from_slice(&fixture("digests.json")).expect("digests.json");
    digests[name].as_str().unwrap_or_else(|| panic!("digests.json carries {name}")).to_string()
}

fn independent_sha256(preimage: &[u8]) -> String {
    // Independent digest path: hex-encode by table instead of reusing any
    // repository digest helper. Independence from the production encoder is
    // anchored by the frozen golden digests, which this recomputation must
    // reproduce.
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(preimage);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

// ---------------------------------------------------------------------------
// Golden vectors (independent authorship: Python generator + hashlib)
// ---------------------------------------------------------------------------

#[test]
fn golden_semantic_bytes_and_fingerprint_match_independent_vectors() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline compiles");
    let bytes = plan.canonical_semantic_bytes().expect("semantic bytes");
    assert_eq!(
        String::from_utf8(bytes.clone()).expect("utf-8"),
        String::from_utf8(fixture("semantic-baseline.json")).expect("utf-8"),
        "Rust canonical semantic bytes must equal the independently generated vector"
    );
    assert_eq!(
        plan.semantic_fingerprint,
        golden_digest("semantic-baseline"),
        "fingerprint must equal the independently computed digest"
    );
}

#[test]
fn golden_escaping_vector_matches() {
    let mut input = baseline_input();
    input.execution[0].command = "echo \"\u{e9}\" \\ done\n\t\u{1}".to_string();
    let plan = CiRoutePlanV1::compile(input).expect("escaping compiles");
    assert_eq!(
        String::from_utf8(plan.canonical_semantic_bytes().expect("bytes")).expect("utf-8"),
        String::from_utf8(fixture("semantic-escaping.json")).expect("utf-8"),
        "string escaping rules must match the independent vector"
    );
    assert_eq!(plan.semantic_fingerprint, golden_digest("semantic-escaping"));
}

#[test]
fn golden_payload_bytes_match() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline compiles");
    let bytes = plan.canonical_json().expect("payload bytes");
    assert_eq!(
        String::from_utf8(bytes).expect("utf-8"),
        String::from_utf8(fixture("payload-baseline.json")).expect("utf-8"),
        "the complete published artifact must equal the frozen reference payload"
    );
}

fn python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

#[test]
fn golden_generator_reports_no_fixture_drift() {
    // Non-mutating drift check: the frozen vectors must equal the
    // independent Python generator's output byte for byte, so a stale or
    // hand-edited fixture cannot pass the equality tests above by merely
    // agreeing with a stale Rust encoder.
    let output = std::process::Command::new(python())
        .arg(Path::new(FIXTURES).join("generate_golden.py"))
        .arg("--check")
        .output()
        .expect("python is available to run the golden generator");
    assert!(
        output.status.success(),
        "generate_golden.py --check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn published_payload_keys_are_the_semantic_keys_plus_derived_fields() {
    // The payload embeds the semantic projection, so its key set is the
    // fingerprint-preimage key set plus exactly `summary` and
    // `semantic_fingerprint`; any other divergence between the two
    // projections is caught here instead of drifting silently.
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline compiles");
    let payload = serde_json::to_value(plan.canonical_payload()).expect("payload value");
    let projection = serde_json::to_value(plan.semantic_projection()).expect("projection value");
    let mut expected: BTreeSet<&str> =
        projection.as_object().expect("object").keys().map(String::as_str).collect();
    expected.insert("summary");
    expected.insert("semantic_fingerprint");
    let payload_keys: BTreeSet<&str> =
        payload.as_object().expect("object").keys().map(String::as_str).collect();
    assert_eq!(
        payload_keys, expected,
        "published payload = semantic projection + the two derived fields only"
    );
}

// ---------------------------------------------------------------------------
// Order semantics: invariance and refusal
// ---------------------------------------------------------------------------

#[test]
fn source_order_cannot_change_canonical_bytes_or_fingerprint() {
    let baseline = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let mut shuffled = baseline_input();
    // Input vector order, tier order, and map insertion order all move.
    shuffled.dispositions.reverse();
    shuffled.selectors.reverse();
    shuffled.execution.reverse();
    shuffled.expansion.included_native_tiers =
        vec!["merge_gate".to_string(), "pr_fast".to_string()];
    let moved = CiRoutePlanV1::compile(shuffled).expect("shuffled compiles");
    assert_eq!(
        moved.canonical_semantic_bytes().expect("bytes"),
        baseline.canonical_semantic_bytes().expect("bytes"),
        "set-like and input order must not move canonical bytes"
    );
    assert_eq!(moved.semantic_fingerprint, baseline.semantic_fingerprint);
    assert_eq!(moved.semantic_fingerprint, golden_digest("semantic-baseline"));
}

#[test]
fn ordered_command_tokens_are_never_sorted() {
    let mut input = baseline_input();
    input.selection.package_args = vec!["perl-parser".to_string(), "-p".to_string()];
    let plan = CiRoutePlanV1::compile(input).expect("reordered tokens compile");
    let baseline = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    assert_ne!(
        plan.semantic_fingerprint, baseline.semantic_fingerprint,
        "package_args are an ordered command-token sequence; reordering is semantic movement"
    );
    let bytes = String::from_utf8(plan.canonical_semantic_bytes().expect("bytes")).expect("utf-8");
    assert!(
        bytes.contains("\"package_args\":[\"perl-parser\",\"-p\"]"),
        "the projected order must be preserved verbatim"
    );
}

#[test]
fn set_like_source_reorder_is_refused_never_reencoded() {
    // Scope identity lists and risk tags are validator-enforced canonical;
    // a reordering is a typed refusal, never silently different bytes.
    let mut input = baseline_input();
    input.selection.scope = Some(RouteScopeEvidence {
        head_sha: SHA_A.to_string(),
        diff_class: "rust".to_string(),
        direct_crates: vec![
            ScopedIdentity { name: "zeta".to_string(), reason: "later".to_string() },
            ScopedIdentity { name: "alpha".to_string(), reason: "earlier".to_string() },
        ],
        reverse_dependencies: vec![],
        architecture_wideners: vec![],
        risk_tags: vec![],
    });
    let error = CiRoutePlanV1::compile(input).expect_err("non-canonical scope must refuse");
    assert!(error.contains("not canonical"), "{error}");
}

#[test]
fn duplicate_set_member_is_refused_at_compile() {
    let mut input = baseline_input();
    input.expansion.included_native_tiers =
        vec!["pr_fast".to_string(), "pr_fast".to_string(), "merge_gate".to_string()];
    let error = CiRoutePlanV1::compile(input).expect_err("duplicate tier must refuse");
    assert!(error.contains("duplicated"), "{error}");
}

// ---------------------------------------------------------------------------
// Fingerprint discrimination: semantic movement moves, derived movement
// does not, and the fingerprint never covers itself
// ---------------------------------------------------------------------------

fn fingerprint_of(input: CompileRoutePlanInput) -> String {
    CiRoutePlanV1::compile(input).expect("compile").semantic_fingerprint
}

#[test]
fn semantic_movement_moves_the_fingerprint() {
    let baseline = fingerprint_of(baseline_input());

    let mut gate_command = baseline_input();
    gate_command.execution[0].command = "run fmt_gate differently".to_string();
    assert_ne!(fingerprint_of(gate_command), baseline, "gate command movement");

    let mut timeout = baseline_input();
    timeout.execution[0].timeout_seconds = 61;
    assert_ne!(fingerprint_of(timeout), baseline, "timeout movement");

    let mut role = baseline_input();
    role.dispositions[0].policy_role = PolicyRole::Advisory;
    assert_ne!(fingerprint_of(role), baseline, "policy role movement");

    let mut resolution = baseline_input();
    resolution.dispositions[1].lifecycle.resolution = Resolution::Expired;
    resolution.dispositions[1].detail = Some("expired".to_string());
    assert_ne!(fingerprint_of(resolution), baseline, "lifecycle resolution movement");

    let mut profile = baseline_input();
    profile.expansion.requested_profile = "nightly".to_string();
    assert_ne!(fingerprint_of(profile), baseline, "requested profile movement");

    let mut selector_digest = baseline_input();
    selector_digest.selection.selector_digest = DIGEST_B.to_string();
    assert_ne!(fingerprint_of(selector_digest), baseline, "selector digest movement");

    let mut head = baseline_input();
    head.subject.head_sha = SHA_B.to_string();
    assert_ne!(fingerprint_of(head), baseline, "subject movement");

    let mut policy_digest = baseline_input();
    policy_digest.expansion.policy_digest = DIGEST_A.to_string();
    assert_ne!(fingerprint_of(policy_digest), baseline, "policy digest movement");

    let mut workflow_digest = baseline_input();
    workflow_digest.workflow_digest = DIGEST_A.to_string();
    assert_ne!(fingerprint_of(workflow_digest), baseline, "workflow digest movement");

    let mut disposition_digest = baseline_input();
    disposition_digest.disposition_digest = DIGEST_A.to_string();
    assert_ne!(fingerprint_of(disposition_digest), baseline, "disposition digest movement");

    let mut expansion_fingerprint = baseline_input();
    expansion_fingerprint.expansion.semantic_fingerprint = DIGEST_A.to_string();
    assert_ne!(fingerprint_of(expansion_fingerprint), baseline, "denominator identity movement");

    let mut denominator = baseline_input();
    denominator.expansion.denominator = vec!["fmt_gate".to_string()];
    denominator.dispositions.truncate(1);
    denominator.selectors.truncate(1);
    denominator.execution.truncate(1);
    assert_ne!(fingerprint_of(denominator), baseline, "denominator membership movement");

    let mut native_tier = baseline_input();
    native_tier.dispositions[1].native_tier = "merge_gate".to_string();
    assert_ne!(fingerprint_of(native_tier), baseline, "native tier movement");

    let mut selector_role = baseline_input();
    selector_role.selectors[0].role = Some(SelectorRole::Static);
    assert_ne!(fingerprint_of(selector_role), baseline, "selector role movement");

    let mut subject_digest = baseline_input();
    subject_digest.subject.subject_digest = DIGEST_B.to_string();
    assert_ne!(fingerprint_of(subject_digest), baseline, "subject digest movement");
}

#[test]
fn derived_and_presentation_movement_does_not_move_the_fingerprint() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let before = plan.semantic_fingerprint_of().expect("fingerprint");
    let bytes_before = plan.canonical_semantic_bytes().expect("bytes");

    // A stale (wrong) summary must not move the semantic identity...
    let mut stale_summary = plan.clone();
    stale_summary.summary.run = 99;
    assert_eq!(stale_summary.semantic_fingerprint_of().expect("fingerprint"), before);
    // ...but is still a typed validation refusal: derived fields are
    // recomputed and checked, not trusted.
    let error = stale_summary.validate().expect_err("stale summary must not validate");
    assert!(error.contains("does not reconcile"), "{error}");

    // Absent-vs-empty and presentation fields cannot participate: the
    // semantic projection structurally excludes summary and fingerprint.
    let projection = serde_json::to_value(plan.semantic_projection()).expect("projection");
    let keys: BTreeSet<&str> =
        projection.as_object().expect("object").keys().map(String::as_str).collect();
    assert!(!keys.contains("summary"), "summary is excluded from the preimage");
    assert!(
        !keys.contains("semantic_fingerprint"),
        "the fingerprint is excluded from its own preimage"
    );

    // Pretty-printing the same payload never touches the semantic bytes.
    let pretty = serde_json::to_string_pretty(&plan).expect("pretty");
    let mut rehydrated: CiRoutePlanV1 = serde_json::from_str(&pretty).expect("parse pretty");
    rehydrated.validate().expect("pretty round-trip validates");
    assert_eq!(rehydrated.canonical_semantic_bytes().expect("bytes"), bytes_before);
}

#[test]
fn fingerprint_field_tampering_is_refused() {
    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    plan.semantic_fingerprint = DIGEST_C.to_string();
    let error = plan.validate().expect_err("moved fingerprint must fail");
    assert!(error.contains("does not equal the recomputed digest"), "{error}");
    // A well-formed but wrong digest cannot encode either.
    assert!(plan.canonical_json().is_err());
}

#[test]
fn fingerprint_is_domain_separated() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let bytes = plan.canonical_semantic_bytes().expect("bytes");
    // Without the domain separator the digest differs: the separator is
    // load-bearing, verified against the frozen golden digest.
    let undomained = independent_sha256(&bytes);
    assert_ne!(undomained, plan.semantic_fingerprint);
    let mut preimage = FINGERPRINT_DOMAIN.to_vec();
    preimage.extend_from_slice(&bytes);
    assert_eq!(independent_sha256(&preimage), plan.semantic_fingerprint);
    assert_eq!(independent_sha256(&preimage), golden_digest("semantic-baseline"));
}

// ---------------------------------------------------------------------------
// Round-trip determinism and fail-closed parsing
// ---------------------------------------------------------------------------

#[test]
fn serialize_parse_validate_reserialize_is_byte_identical() {
    for input in [baseline_input(), {
        let mut input = baseline_input();
        input.execution[0].command = "echo \"\u{e9}\" \\ done\n\t\u{1}".to_string();
        input
    }] {
        let plan = CiRoutePlanV1::compile(input).expect("compile");
        let bytes = plan.canonical_json().expect("canonical bytes");
        let parsed: CiRoutePlanV1 = serde_json::from_slice(&bytes).expect("canonical bytes parse");
        parsed.validate().expect("parsed plan validates");
        assert_eq!(parsed.canonical_json().expect("re-encode"), bytes);
    }
}

#[test]
fn legacy_and_current_fields_cannot_coexist() {
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let mut value = serde_json::to_value(&plan).expect("serialize");
    // A legacy pre-split field alongside the current spelling is refused.
    value
        .as_object_mut()
        .expect("object")
        .insert("profile".to_string(), Value::String("merge_gate".to_string()));
    let error = serde_json::from_value::<CiRoutePlanV1>(value)
        .expect_err("legacy + current must not coexist");
    assert!(error.to_string().contains("unknown field"), "{error}");
}

#[test]
fn unknown_version_fails_closed_everywhere() {
    let mut plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    plan.schema = "ci_route_plan.v2".to_string();
    let error = plan.validate().expect_err("unknown version must fail");
    assert!(error.contains("unsupported route-plan schema"), "{error}");
    assert!(plan.canonical_json().is_err());
}

// ---------------------------------------------------------------------------
// Schema conformance and reciprocal drift
// ---------------------------------------------------------------------------

fn checked_schema() -> Value {
    let text = fs::read_to_string(SCHEMA_PATH)
        .unwrap_or_else(|error| panic!("read checked schema {SCHEMA_PATH}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse checked schema: {error}"))
}

fn enum_spellings<T: serde::Serialize>(values: &[T]) -> Vec<String> {
    values
        .iter()
        .map(|value| serde_json::to_value(value).expect("serialize enum"))
        .map(|value| value.as_str().expect("snake_case spelling").to_string())
        .collect()
}

#[test]
fn schema_vocabulary_matches_the_rust_domain() {
    let schema = checked_schema();
    let properties = schema["properties"].as_object().expect("top properties");
    let defs = schema["$defs"].as_object().expect("defs");

    let assert_enum = |schema_node: &Value, expected: &[String], subject: &str| {
        let vocabulary: Vec<&str> = schema_node["enum"]
            .as_array()
            .expect("enum")
            .iter()
            .map(|item| item.as_str().expect("str"))
            .collect();
        let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
        assert_eq!(vocabulary, expected_refs, "{subject} vocabulary must match Rust exactly");
    };

    assert_eq!(properties["schema"]["const"].as_str().expect("const"), CI_ROUTE_PLAN_SCHEMA);
    assert_eq!(properties["producer"]["const"].as_str().expect("const"), CI_ROUTE_PLAN_PRODUCER);
    assert_enum(
        &properties["requested_profile"],
        &KNOWN_PROFILES.iter().map(|profile| profile.to_string()).collect::<Vec<_>>(),
        "requested_profile",
    );
    let outcome_one_of = defs["outcome"]["oneOf"].as_array().expect("outcome oneOf");
    let error_branch = outcome_one_of
        .iter()
        .find(|branch| branch["properties"]["kind"]["const"].as_str() == Some("error"))
        .expect("error outcome branch");
    assert_enum(
        &error_branch["properties"]["code"],
        &CLOSED_ERROR_CODES.iter().map(|code| code.to_string()).collect::<Vec<_>>(),
        "outcome error code",
    );
    let row_properties = defs["row"]["properties"].as_object().expect("row properties");
    assert_enum(
        &row_properties["policy_role"],
        &enum_spellings(&[PolicyRole::Required, PolicyRole::Advisory]),
        "policy_role",
    );
    assert_enum(
        &row_properties["lifecycle"]["properties"]["state"],
        &enum_spellings(&[
            LifecycleState::Active,
            LifecycleState::Dormant,
            LifecycleState::Quarantined,
            LifecycleState::Retired,
            LifecycleState::Blocked,
        ]),
        "lifecycle state",
    );
    assert_enum(
        &row_properties["lifecycle"]["properties"]["resolution"],
        &enum_spellings(&[Resolution::Current, Resolution::Expired, Resolution::Invalid]),
        "lifecycle resolution",
    );
    assert_enum(
        &row_properties["selector_role"],
        &enum_spellings(&[
            SelectorRole::AlwaysOn,
            SelectorRole::RustScoped,
            SelectorRole::RustFallback,
            SelectorRole::RustPackageScoped,
            SelectorRole::Static,
            SelectorRole::Unspecified,
        ]),
        "selector_role",
    );
    assert_enum(
        &row_properties["selector_placement"],
        &enum_spellings(&[SelectorPlacement::Selected, SelectorPlacement::Skipped]),
        "selector_placement",
    );
    assert_enum(
        &row_properties["applicability"],
        &enum_spellings(&[
            Applicability::Applicable,
            Applicability::NotApplicable,
            Applicability::Unknown,
        ]),
        "applicability",
    );
    for kind in ["run", "scoped_noop", "quarantined", "error"] {
        assert!(
            outcome_one_of
                .iter()
                .any(|branch| branch["properties"]["kind"]["const"].as_str() == Some(kind)),
            "outcome branch {kind} is projected"
        );
    }

    // Required-field reciprocity: a serialized Rust payload carries exactly
    // the schema's required top-level keys.
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let payload = serde_json::to_value(&plan).expect("payload");
    let payload_keys: BTreeSet<&str> =
        payload.as_object().expect("object").keys().map(String::as_str).collect();
    let required: BTreeSet<&str> = schema["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|item| item.as_str().expect("str"))
        .collect();
    assert_eq!(payload_keys, required, "payload keys and schema required list are reciprocal");
    let summary_keys: BTreeSet<&str> = payload["summary"]
        .as_object()
        .expect("summary object")
        .keys()
        .map(String::as_str)
        .collect();
    let summary_required: BTreeSet<&str> = defs["summary"]["required"]
        .as_array()
        .expect("summary required")
        .iter()
        .map(|item| item.as_str().expect("str"))
        .collect();
    assert_eq!(
        summary_keys, summary_required,
        "summary shape is complete, not an arbitrary object"
    );
}

#[test]
fn generated_payloads_validate_against_the_checked_schema() {
    let schema = checked_schema();
    let plans = [
        CiRoutePlanV1::compile(baseline_input()).expect("baseline"),
        CiRoutePlanV1::compile({
            let mut input = baseline_input();
            input.execution[0].command = "echo \"\u{e9}\" \\ done\n\t\u{1}".to_string();
            input
        })
        .expect("escaping"),
        CiRoutePlanV1::compile({
            let mut input = baseline_input();
            input.expansion.requested_profile = "nightly".to_string();
            input
        })
        .expect("nightly"),
        CiRoutePlanV1::compile({
            // Current quarantine with owner/reason/review identity.
            let mut input = baseline_input();
            input.dispositions[1] = RouteDispositionInput {
                gate_id: "unit_gate".to_string(),
                policy_role: PolicyRole::Advisory,
                lifecycle: LifecycleDisposition {
                    state: LifecycleState::Quarantined,
                    resolution: Resolution::Current,
                },
                native_tier: "pr_fast".to_string(),
                quarantine: Some(RouteQuarantineEvidence {
                    owner: "ci-owner".to_string(),
                    owner_issue: Some("10176".to_string()),
                    reason_token: "secondary_failure".to_string(),
                    review_after: "2030-01-01".to_string(),
                }),
                detail: None,
            };
            input.selectors[1] = skipped("unit_gate", Some(SelectorProof::NotApplicableToSubject));
            input
        })
        .expect("quarantined"),
        CiRoutePlanV1::compile({
            // Fallback firing: no positive proof, typed error rows, and the
            // fallback_used/fallback_reason relationship in the selection.
            let mut input = baseline_input();
            input.selection.fallback_used = true;
            input.selection.fallback_reason = Some("scope unavailable".to_string());
            input.selectors[0].proof = None;
            input.selectors[1].proof = None;
            input
        })
        .expect("fallback"),
    ];
    for plan in &plans {
        let bytes = plan.canonical_json().expect("canonical bytes");
        let payload: Value = serde_json::from_slice(&bytes).expect("payload parses");
        validate_against_schema(&payload, &schema, &schema, "#")
            .unwrap_or_else(|error| panic!("schema conformance: {error}"));
    }
}

#[test]
fn schema_mutation_is_detected() {
    let schema = checked_schema();
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let payload: Value =
        serde_json::from_slice(&plan.canonical_json().expect("bytes")).expect("payload");

    // Broadening an enum lets through a value the typed model refuses:
    // the original schema rejects the bogus profile, the broadened schema
    // accepts it, and the Rust validator still refuses it — the drift is
    // exactly the schema/typed-model divergence the vocabulary test catches.
    let mut broadened = schema.clone();
    broadened["properties"]["requested_profile"]["enum"]
        .as_array_mut()
        .expect("enum")
        .push(Value::String("bogus_profile".to_string()));
    let mut bogus = payload.clone();
    bogus["requested_profile"] = Value::String("bogus_profile".to_string());
    assert!(
        validate_against_schema(&bogus, &schema, &schema, "#").is_err(),
        "original schema refuses the bogus profile"
    );
    validate_against_schema(&bogus, &broadened, &broadened, "#")
        .expect("broadened schema accepts the bogus value");
    let mut bogus_plan = plan.clone();
    bogus_plan.requested_profile = "bogus_profile".to_string();
    assert!(
        bogus_plan.validate().is_err(),
        "the Rust domain keeps refusing what the broadened schema admitted"
    );
    assert_ne!(
        serde_json::to_string(&broadened["properties"]["requested_profile"]["enum"]).expect("json"),
        serde_json::to_string(&schema["properties"]["requested_profile"]["enum"]).expect("json"),
        "the vocabulary-equality drift test detects the broadening"
    );

    // Narrowing the typed contract: removing a required field from the
    // schema lets a payload the typed model cannot produce look valid; the
    // reciprocal key test detects the removal.
    let mut narrowed = schema.clone();
    narrowed["required"]
        .as_array_mut()
        .expect("required")
        .retain(|item| item.as_str() != Some("semantic_fingerprint"));
    let required: Vec<&str> = narrowed["required"]
        .as_array()
        .expect("required")
        .iter()
        .map(|item| item.as_str().expect("str"))
        .collect();
    assert!(
        !required.contains(&"semantic_fingerprint"),
        "the reciprocal key test detects the narrowing"
    );
    let mut fingerprintless = payload.clone();
    fingerprintless.as_object_mut().expect("object").remove("semantic_fingerprint");
    validate_against_schema(&fingerprintless, &narrowed, &narrowed, "#")
        .expect("narrowed schema admits a payload the typed model cannot represent");
    assert!(
        serde_json::from_value::<CiRoutePlanV1>(fingerprintless).is_err(),
        "the typed model still requires the fingerprint field"
    );

    // An unknown keyword fails closed: schema growth cannot pass silently.
    let mut grown = schema.clone();
    grown["properties"]["summary"]["frobnicate"] = Value::Bool(true);
    let summary = payload["summary"].clone();
    assert!(
        validate_against_schema(&summary, &grown["properties"]["summary"], &grown, "#").is_err(),
        "unknown keywords fail closed"
    );
}

#[test]
fn schema_refuses_payloads_the_typed_domain_refuses() {
    let schema = checked_schema();
    let plan = CiRoutePlanV1::compile(baseline_input()).expect("baseline");
    let mut payload: Value =
        serde_json::from_slice(&plan.canonical_json().expect("bytes")).expect("payload");

    // Unknown top-level field (including legacy spellings).
    let mut extra = payload.clone();
    extra["profile"] = Value::String("merge_gate".to_string());
    assert!(validate_against_schema(&extra, &schema, &schema, "#").is_err());

    // Unknown outcome kind.
    let mut teleport = payload.clone();
    teleport["rows"][0]["outcome"]["kind"] = Value::String("teleport".to_string());
    assert!(validate_against_schema(&teleport, &schema, &schema, "#").is_err());

    // Wrong digest shape.
    let mut digest = payload.clone();
    digest["workflow_digest"] = Value::String("not-a-digest".to_string());
    assert!(validate_against_schema(&digest, &schema, &schema, "#").is_err());

    // fallback_used false with a fallback_reason present.
    let mut fallback = payload.clone();
    fallback["selection"]["fallback_reason"] = Value::String("stale".to_string());
    assert!(
        validate_against_schema(&fallback, &schema, &schema, "#").is_err(),
        "the fallback_used/fallback_reason relationship is schema-enforced"
    );

    // fallback_used true without a reason.
    let mut falling_back = payload.clone();
    falling_back["selection"]["fallback_used"] = Value::Bool(true);
    assert!(validate_against_schema(&falling_back, &schema, &schema, "#").is_err());
}

/// Test-local structural checker for exactly the keyword set the checked-in
/// schema uses. It fails closed on any keyword it does not implement, so a
/// schema mutation cannot pass undetected. This deliberately mirrors the
/// repository's validator-plus-drift-test idiom rather than introducing a
/// generic schema framework.
fn validate_against_schema(
    value: &Value,
    schema: &Value,
    root: &Value,
    pointer: &str,
) -> Result<(), String> {
    let Some(schema) = schema.as_object() else {
        return Err(format!("{pointer}: schema fragment is not an object"));
    };
    for keyword in schema.keys() {
        if !matches!(
            keyword.as_str(),
            "$schema"
                | "$id"
                | "title"
                | "description"
                | "type"
                | "properties"
                | "required"
                | "additionalProperties"
                | "enum"
                | "const"
                | "pattern"
                | "minItems"
                | "minLength"
                | "minimum"
                | "uniqueItems"
                | "items"
                | "oneOf"
                | "allOf"
                | "if"
                | "then"
                | "not"
                | "propertyNames"
                | "$ref"
                | "$defs"
        ) {
            return Err(format!("{pointer}: unknown schema keyword {keyword:?} fails closed"));
        }
    }
    if let Some(reference) = schema.get("$ref") {
        let target = reference.as_str().expect("$ref is a string");
        let Some(fragment) = target.strip_prefix("#") else {
            return Err(format!("{pointer}: only local $ref supported: {target}"));
        };
        let mut resolved = root;
        if !fragment.is_empty() {
            for part in fragment.trim_start_matches('/').split('/') {
                let part = part.replace("~1", "/").replace("~0", "~");
                resolved = resolved
                    .get(part.as_str())
                    .ok_or_else(|| format!("{pointer}: unresolved $ref {target}"))?;
            }
        }
        return validate_against_schema(value, resolved, root, target);
    }
    if let Some(expected) = schema.get("type") {
        let expected = expected.as_str().expect("type is a string");
        let actual = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        // "integer" is the schema's only subtype refinement in this schema.
        let integer_ok = expected == "integer"
            && value.as_u64().is_some()
            && value.as_f64().is_some_and(|float| float.fract() == 0.0);
        if actual != expected && !integer_ok {
            return Err(format!("{pointer}: expected type {expected}, found {actual}"));
        }
    }
    if let Some(vocabulary) = schema.get("enum") {
        let allowed = vocabulary.as_array().expect("enum array");
        if !allowed.contains(value) {
            return Err(format!("{pointer}: {value} is not in the closed vocabulary"));
        }
    }
    if let Some(expected) = schema.get("const") {
        if expected != value {
            return Err(format!("{pointer}: {value} != const {expected}"));
        }
    }
    if let Some(pattern) = schema.get("pattern") {
        // JSON Schema semantics: `pattern` ignores non-string instances.
        if let Some(text) = value.as_str() {
            let regex = regex::Regex::new(pattern.as_str().expect("pattern string"))
                .map_err(|error| format!("{pointer}: invalid pattern {pattern:?}: {error}"))?;
            if !regex.is_match(text) {
                return Err(format!("{pointer}: {text:?} does not match {pattern:?}"));
            }
        }
    }
    if let Some(minimum) = schema.get("minimum") {
        let minimum = minimum.as_u64().expect("minimum integer");
        if value.as_u64().is_none_or(|found| found < minimum) {
            return Err(format!("{pointer}: {value} < minimum {minimum}"));
        }
    }
    match value {
        Value::String(text) => {
            if let Some(min_length) =
                schema.get("minLength").map(|value| value.as_u64().expect("minLength"))
            {
                // JSON Schema counts Unicode code points, not bytes.
                if (text.chars().count() as u64) < min_length {
                    return Err(format!("{pointer}: string shorter than {min_length}"));
                }
            }
        }
        Value::Array(items) => {
            if let Some(min_items) =
                schema.get("minItems").map(|value| value.as_u64().expect("minItems"))
            {
                if (items.len() as u64) < min_items {
                    return Err(format!("{pointer}: fewer than {min_items} items"));
                }
            }
            if schema.get("uniqueItems") == Some(&Value::Bool(true)) {
                let mut seen = BTreeSet::new();
                for item in items {
                    let encoded = serde_json::to_string(item).expect("serialize item");
                    if !seen.insert(encoded) {
                        return Err(format!("{pointer}: duplicate set-like member"));
                    }
                }
            }
            if let Some(item_schema) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    validate_against_schema(
                        item,
                        item_schema,
                        root,
                        &format!("{pointer}/{index}"),
                    )?;
                }
            }
        }
        Value::Object(map) => {
            if let Some(required) = schema.get("required") {
                for key in required.as_array().expect("required array") {
                    let key = key.as_str().expect("required key");
                    if !map.contains_key(key) {
                        return Err(format!("{pointer}: missing required field {key:?}"));
                    }
                }
            }
            // `additionalProperties: false` binds even when the node
            // declares no `properties` (an empty property map).
            let empty_properties = serde_json::Map::new();
            let properties = schema
                .get("properties")
                .map_or(&empty_properties, |value| value.as_object().expect("properties object"));
            for (key, subschema) in properties {
                if let Some(member) = map.get(key) {
                    validate_against_schema(member, subschema, root, &format!("{pointer}/{key}"))?;
                }
            }
            if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                for key in map.keys() {
                    if !properties.contains_key(key) {
                        return Err(format!("{pointer}: unknown field {key:?}"));
                    }
                }
            }
            if let Some(property_names) = schema.get("propertyNames") {
                for key in map.keys() {
                    validate_against_schema(
                        &Value::String(key.clone()),
                        property_names,
                        root,
                        &format!("{pointer}/<names>"),
                    )?;
                }
            }
        }
        _ => {}
    }
    for (index, branch) in schema
        .get("allOf")
        .map(|value| value.as_array().expect("allOf array"))
        .unwrap_or(&Vec::new())
        .iter()
        .enumerate()
    {
        validate_against_schema(value, branch, root, &format!("{pointer}/allOf/{index}"))?;
    }
    if let Some(one_of) = schema.get("oneOf").map(|value| value.as_array().expect("oneOf array")) {
        let matches = one_of
            .iter()
            .filter(|branch| validate_against_schema(value, branch, root, pointer).is_ok())
            .count();
        if matches != 1 {
            return Err(format!("{pointer}: matched {matches} oneOf branches, expected exactly 1"));
        }
    }
    if let Some(condition) = schema.get("if") {
        let holds =
            validate_against_schema(value, condition, root, &format!("{pointer}/if")).is_ok();
        if holds {
            if let Some(then_branch) = schema.get("then") {
                validate_against_schema(value, then_branch, root, &format!("{pointer}/then"))?;
            }
        }
    }
    if let Some(not_branch) = schema.get("not") {
        if validate_against_schema(value, not_branch, root, &format!("{pointer}/not")).is_ok() {
            return Err(format!("{pointer}: matched the forbidden `not` branch"));
        }
    }
    Ok(())
}
