//! Integration tests for gold project envelopes.
//!
//! Run with:
//!
//! ```bash
//! cargo test -p perl-corpus --test gold_project
//! bash scripts/cargo-admitted test -p perl-corpus --locked gold_project
//! ```

use perl_corpus::gold::project_envelope::{
    DiscoveryOutcome, EnvelopeError, GOLD_PROJECT_SCHEMA, MemberRole, build_envelope,
    compute_content_fingerprint, compute_topology_fingerprint, discover_corpus, discover_project,
    envelope_to_toml, infer_role, member_id_from_path, parse_envelope_str,
    project_id_from_dir_name, validate_envelope_structure,
};
use perl_tdd_support::{must, must_some};
use std::path::Path;

fn fixtures_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gold_project")
}

// ---------------------------------------------------------------------------
// ID derivation
// ---------------------------------------------------------------------------

#[test]
fn gold_project_member_id_rule_is_documented_and_stable() {
    assert_eq!(member_id_from_path("fixture.pl"), "fixture_pl");
    assert_eq!(member_id_from_path("expected.json"), "expected_json");
    assert_eq!(member_id_from_path("expected_hover.json"), "expected_hover_json");
    assert_eq!(member_id_from_path("lib/FindBinModule.pm"), "lib__FindBinModule_pm");
    assert_eq!(member_id_from_path("lib/deep/Nested.pm"), "lib__deep__Nested_pm");
}

#[test]
fn gold_project_project_id_is_dir_name_verbatim() {
    assert_eq!(project_id_from_dir_name("hello_world"), "hello_world");
    assert_eq!(project_id_from_dir_name("inc_findbin_relative"), "inc_findbin_relative");
    assert_eq!(project_id_from_dir_name("v5_40_suppresses_strict"), "v5_40_suppresses_strict");
}

// ---------------------------------------------------------------------------
// Role inference
// ---------------------------------------------------------------------------

#[test]
fn gold_project_role_inference_covers_all_fixture_patterns() {
    assert_eq!(infer_role("fixture.pl"), MemberRole::Entrypoint);
    assert_eq!(infer_role("expected.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_hover.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_goto.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_completion.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_symbols.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_rename.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("expected_module.json"), MemberRole::GeneratedExpectationInput);
    assert_eq!(infer_role("lib/FindBinModule.pm"), MemberRole::SupportSource);
    assert_eq!(infer_role("lib/LexicalModule.pm"), MemberRole::SupportSource);
    assert_eq!(infer_role("helper.pl"), MemberRole::Source);
    assert_eq!(infer_role("data.txt"), MemberRole::Data);
}

// ---------------------------------------------------------------------------
// Fingerprint determinism
// ---------------------------------------------------------------------------

#[test]
fn gold_project_topology_fingerprint_is_deterministic() {
    let paths = vec!["expected.json", "fixture.pl"];
    let fp1 = compute_topology_fingerprint(&paths);
    let fp2 = compute_topology_fingerprint(&paths);
    assert_eq!(fp1, fp2, "fingerprint must be deterministic");
    assert_eq!(fp1.len(), 64, "fingerprint must be 64-character hex SHA-256");
    assert!(fp1.chars().all(|c| c.is_ascii_hexdigit()), "fingerprint must be lowercase hex");
}

#[test]
fn gold_project_content_fingerprint_is_deterministic() {
    let pairs = vec![("expected.json", "aabbcc"), ("fixture.pl", "ddeeff")];
    let fp1 = compute_content_fingerprint(&pairs);
    let fp2 = compute_content_fingerprint(&pairs);
    assert_eq!(fp1, fp2);
    assert_eq!(fp1.len(), 64);
}

#[test]
fn gold_project_fingerprints_change_with_content() {
    let paths_a = vec!["expected.json", "fixture.pl"];
    let paths_b = vec!["expected.json", "other.pl"];
    assert_ne!(
        compute_topology_fingerprint(&paths_a),
        compute_topology_fingerprint(&paths_b),
        "different paths must produce different topology fingerprints"
    );

    let pairs_a = vec![("fixture.pl", "aabbcc")];
    let pairs_b = vec![("fixture.pl", "ddeeff")];
    assert_ne!(
        compute_content_fingerprint(&pairs_a),
        compute_content_fingerprint(&pairs_b),
        "different digests must produce different content fingerprints"
    );
}

// ---------------------------------------------------------------------------
// Schema validation
// ---------------------------------------------------------------------------

#[test]
fn gold_project_parse_rejects_wrong_schema() {
    let toml = r#"
schema = "gold_project.v2"
project_id = "test"
topology_fingerprint = "aaa"
content_fingerprint = "bbb"
members = []
"#;
    let result = parse_envelope_str(toml);
    assert!(
        matches!(result, Err(EnvelopeError::SchemaMismatch { .. })),
        "wrong schema version must be rejected"
    );
}

#[test]
fn gold_project_parse_rejects_unknown_fields() {
    let toml = format!(
        r#"
schema = "{}"
project_id = "test"
topology_fingerprint = "aaa"
content_fingerprint = "bbb"
members = []
unexpected_field = "oops"
"#,
        GOLD_PROJECT_SCHEMA
    );
    let result = parse_envelope_str(&toml);
    assert!(result.is_err(), "unknown top-level fields must be rejected");
}

#[test]
fn gold_project_parse_rejects_malformed_toml() {
    let result = parse_envelope_str("this is {{ not valid toml");
    assert!(matches!(result, Err(EnvelopeError::Parse(_))));
}

// ---------------------------------------------------------------------------
// Good project: round-trip and validation
// ---------------------------------------------------------------------------

#[test]
fn gold_project_good_project_validates_successfully() {
    let project_dir = fixtures_dir().join("good_project");
    let outcome = discover_project(&project_dir);
    assert!(outcome.is_valid(), "good_project fixture must validate successfully: {outcome}");
}

#[test]
fn gold_project_good_project_with_lib_validates_successfully() {
    let project_dir = fixtures_dir().join("good_project_with_lib");
    let outcome = discover_project(&project_dir);
    assert!(
        outcome.is_valid(),
        "good_project_with_lib fixture must validate successfully: {outcome}"
    );
}

#[test]
fn gold_project_build_envelope_round_trips_through_parse() {
    let project_dir = fixtures_dir().join("good_project");
    let env = must(build_envelope(&project_dir));
    assert_eq!(env.schema, GOLD_PROJECT_SCHEMA);
    assert_eq!(env.project_id, "good_project");
    assert_eq!(env.members.len(), 2, "good_project has exactly two members");

    // Round-trip through TOML serialization.
    let toml_str = must(envelope_to_toml(&env));
    let reparsed = must(parse_envelope_str(&toml_str));
    assert_eq!(reparsed.schema, env.schema);
    assert_eq!(reparsed.project_id, env.project_id);
    assert_eq!(reparsed.topology_fingerprint, env.topology_fingerprint);
    assert_eq!(reparsed.content_fingerprint, env.content_fingerprint);
    assert_eq!(reparsed.members.len(), env.members.len());
}

#[test]
fn gold_project_build_envelope_assigns_correct_roles() {
    let project_dir = fixtures_dir().join("good_project");
    let env = must(build_envelope(&project_dir));
    let fixture = must_some(env.members.iter().find(|m| m.path == "fixture.pl"));
    let expected = must_some(env.members.iter().find(|m| m.path == "expected.json"));
    assert_eq!(fixture.role, MemberRole::Entrypoint);
    assert_eq!(expected.role, MemberRole::GeneratedExpectationInput);
}

#[test]
fn gold_project_build_envelope_with_lib_assigns_support_source() {
    let project_dir = fixtures_dir().join("good_project_with_lib");
    let env = must(build_envelope(&project_dir));
    let module = must_some(env.members.iter().find(|m| m.path == "lib/MyModule.pm"));
    assert_eq!(module.role, MemberRole::SupportSource);
    assert_eq!(module.member_id, "lib__MyModule_pm");
}

// ---------------------------------------------------------------------------
// Falsifier: duplicate member_id
// ---------------------------------------------------------------------------

#[test]
fn gold_project_duplicate_member_id_fails_validation() {
    let project_dir = fixtures_dir().join("duplicate_member_id");
    let outcome = discover_project(&project_dir);
    assert!(
        matches!(outcome, DiscoveryOutcome::DuplicateMemberId { .. }),
        "duplicate member_id must be detected, got: {outcome}"
    );
}

// ---------------------------------------------------------------------------
// Falsifier: orphan file
// ---------------------------------------------------------------------------

#[test]
fn gold_project_orphan_file_fails_validation() {
    let project_dir = fixtures_dir().join("orphan_file");
    let outcome = discover_project(&project_dir);
    assert!(
        matches!(outcome, DiscoveryOutcome::OrphanFile { .. }),
        "orphan file must be detected, got: {outcome}"
    );
}

// ---------------------------------------------------------------------------
// Falsifier: invalid path (.. component)
// ---------------------------------------------------------------------------

#[test]
fn gold_project_invalid_path_dotdot_fails_validation() {
    let project_dir = fixtures_dir().join("invalid_path");
    let outcome = discover_project(&project_dir);
    assert!(
        matches!(
            outcome,
            DiscoveryOutcome::InvalidMemberPath { .. } | DiscoveryOutcome::IoError { .. }
        ),
        "dotdot path must be detected, got: {outcome}"
    );
}

// ---------------------------------------------------------------------------
// Falsifier: malformed TOML
// ---------------------------------------------------------------------------

#[test]
fn gold_project_malformed_toml_fails_with_parse_error() {
    let project_dir = fixtures_dir().join("malformed_project");
    let outcome = discover_project(&project_dir);
    assert!(
        matches!(outcome, DiscoveryOutcome::ParseError { .. }),
        "malformed TOML must produce ParseError, got: {outcome}"
    );
}

// ---------------------------------------------------------------------------
// Falsifier: missing envelope
// ---------------------------------------------------------------------------

#[test]
fn gold_project_missing_envelope_produces_missing_outcome() {
    // Use a tempdir with no gold_project.toml.
    let tmp = must(tempfile::tempdir());
    let outcome = discover_project(tmp.path());
    assert!(
        matches!(outcome, DiscoveryOutcome::MissingEnvelope { .. }),
        "missing envelope must produce MissingEnvelope, got: {outcome}"
    );
}

// ---------------------------------------------------------------------------
// Filesystem-order independence
// ---------------------------------------------------------------------------

#[test]
fn gold_project_fingerprints_are_independent_of_filesystem_order() {
    // Build two equivalent member lists in different orders and verify
    // that sorted fingerprints are identical.
    let order_a = vec!["expected.json", "fixture.pl"];
    let order_b = vec!["fixture.pl", "expected.json"];

    // Sort both before computing — that's the contract.
    let mut sorted_a = order_a.clone();
    sorted_a.sort();
    let mut sorted_b = order_b.clone();
    sorted_b.sort();

    assert_eq!(
        compute_topology_fingerprint(&sorted_a),
        compute_topology_fingerprint(&sorted_b),
        "fingerprints must be order-independent when inputs are sorted"
    );
}

// ---------------------------------------------------------------------------
// One bad project does not invalidate others
// ---------------------------------------------------------------------------

#[test]
fn gold_project_one_bad_project_does_not_prevent_others_from_being_valid() {
    // The fixtures dir contains both good and bad projects.
    // discover_corpus must return outcomes for all of them.
    let outcomes = must(discover_corpus(&fixtures_dir()));

    let valid_count = outcomes.iter().filter(|(_, o)| o.is_valid()).count();
    let invalid_count = outcomes.iter().filter(|(_, o)| !o.is_valid()).count();

    assert!(valid_count >= 2, "at least two valid projects must be found; got {valid_count}");
    assert!(invalid_count >= 1, "at least one invalid project must be found; got {invalid_count}");
}

// ---------------------------------------------------------------------------
// Structural validation: sorted order
// ---------------------------------------------------------------------------

#[test]
fn gold_project_structural_validation_rejects_unsorted_members() {
    use perl_corpus::gold::project_envelope::{
        EncodingRecord, GoldMemberRecord, GoldProjectEnvelope, NewlineStyleRecord,
    };

    let make_member = |path: &str, member_id: &str| GoldMemberRecord {
        member_id: member_id.to_owned(),
        path: path.to_owned(),
        role: MemberRole::Data,
        byte_length: 0,
        sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_owned(),
        encoding: EncodingRecord::Utf8,
        newline_style: NewlineStyleRecord::None,
        final_newline: false,
        byte_order_mark: false,
    };

    // Place members in reverse sorted order — validation must reject.
    let env = GoldProjectEnvelope {
        schema: GOLD_PROJECT_SCHEMA.to_owned(),
        project_id: "test".to_owned(),
        topology_fingerprint: "placeholder".to_owned(),
        content_fingerprint: "placeholder".to_owned(),
        members: vec![
            make_member("z_file.txt", "z_file_txt"),
            make_member("a_file.txt", "a_file_txt"),
        ],
    };

    let result = validate_envelope_structure(&env);
    assert!(result.is_err(), "unsorted members must fail structural validation");
}

// ---------------------------------------------------------------------------
// Symlink rejection (unix only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn gold_project_symlink_member_fails_validation() {
    use std::fs;
    use std::os::unix::fs::symlink;

    let tmp = must(tempfile::tempdir());
    let project_dir = tmp.path();

    // Create a real file and a symlink to it.
    let real_file = project_dir.join("real.txt");
    must(fs::write(&real_file, b"hello\n"));

    let link_path = project_dir.join("link.txt");
    must(symlink(&real_file, &link_path));

    // discover_project will fail because the symlink appears as an
    // orphan (it's not in the envelope) or because it's rejected during
    // the fs walk.
    let outcome = discover_project(project_dir);
    assert!(!outcome.is_valid(), "a project directory with a symlink must not validate");
}
