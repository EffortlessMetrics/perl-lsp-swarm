// Integration contract tests for the compiler-critical test topology
// (#12125). Every falsifier from the issue is pinned against synthetic
// cargo-metadata fixtures; one smoke test exercises the real tree.
// Bodies propagate results so no `expect`/`unwrap`/`panic!` appears anywhere.

use std::collections::{BTreeMap, BTreeSet};

use xtask::test_topology::discovery::{
    self, DiscoveredTarget, ManifestFacts, discover_from_metadata,
};
use xtask::test_topology::model::{
    CandidateProfileV1, CompileObligationV1, DefaultProfileStateV1, ExecutionClaimV1,
    FeatureSubjectV1, ProofRoleV1, TargetKindV1, TestTopologyRowV1,
};
use xtask::test_topology::{
    Cohort, TestTopologyInventoryV1, Violation, ensure_current, inventory_from_json, render_json,
    render_markdown, render_report, rows_into_inventory, validate_inventory,
};

const ROOT_A: &str = "Z:/ws";
const MANIFEST_A: &str = "Z:/ws/crates/perl-parser-core/Cargo.toml";
const WORKSPACE_MANIFEST_A: &str = "Z:/ws/crates/perl-workspace/Cargo.toml";
const LSP_MANIFEST_A: &str = "Z:/ws/crates/perl-lsp-rs/Cargo.toml";

/// Minimal cohort-package manifest declaring one explicit integration test.
const PARSER_MANIFEST: &str = r##"
[package]
name = "perl-parser-core"
version = "0.0.0"
edition = "2021"

[lib]
doctest = false

[[test]]
name = "core_parse_contract"
"##;

/// Manifest for a package with a feature-gated binary (zero cases under the
/// default profile) and a harness-free bench.
const WORKSPACE_MANIFEST: &str = r##"
[package]
name = "perl-workspace"
version = "0.0.0"
edition = "2021"

[lib]
doctest = false

[features]
default = []
memory-profiling = []

[[bin]]
name = "workspace_memory_profile"
required-features = ["memory-profiling"]

[[bench]]
name = "workspace_index_benchmark"
harness = false
required-features = ["workspace"]
"##;

/// Manifest for a provider-read package.
const LSP_MANIFEST: &str = r##"
[package]
name = "perl-lsp-rs"
version = "0.0.0"
edition = "2021"
"##;

fn parser_targets(root: &str, extra_test_name: Option<&str>) -> String {
    let mut tests = vec![json_target(
        "core_parse_contract",
        &["test"],
        &format!("{root}/crates/perl-parser-core/tests/core_parse_contract.rs"),
        &[],
    )];
    if let Some(extra) = extra_test_name {
        tests.push(json_target(
            extra,
            &["test"],
            &format!("{root}/crates/perl-parser-core/tests/{extra}.rs"),
            &[],
        ));
    }
    serde_json::json!({
        "packages": [{
            "name": "perl-parser-core",
            "manifest_path": format!("{root}/crates/perl-parser-core/Cargo.toml"),
            "targets": tests,
        }],
        "workspace_root": root,
    })
    .to_string()
}

fn workspace_targets(root: &str) -> String {
    serde_json::json!({
        "packages": [{
            "name": "perl-workspace",
            "manifest_path": format!("{root}/crates/perl-workspace/Cargo.toml"),
            "targets": [
                json_target(
                    "perl_workspace",
                    &["lib"],
                    &format!("{root}/crates/perl-workspace/src/lib.rs"),
                    &[],
                ),
                json_target(
                    "workspace_memory_profile",
                    &["bin"],
                    &format!("{root}/crates/perl-workspace/src/bin/workspace_memory_profile.rs"),
                    &["memory-profiling"],
                ),
                json_target(
                    "workspace_index_benchmark",
                    &["bench"],
                    &format!("{root}/crates/perl-workspace/benches/workspace_index_benchmark.rs"),
                    &["workspace"],
                ),
                json_target(
                    "workspace_freshness_probe",
                    &["test"],
                    &format!("{root}/crates/perl-workspace/tests/workspace_freshness_probe.rs"),
                    &[],
                ),
            ],
        }],
        "workspace_root": root,
    })
    .to_string()
}

fn json_target(name: &str, kinds: &[&str], src: &str, required: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "kind": kinds,
        "crate_types": [],
        "name": name,
        "src_path": src,
        "edition": "2021",
        "doc": true,
        "doctest": false,
        "test": true,
        "required-features": required,
    })
}

fn manifests(pairs: &[(&str, &str)]) -> anyhow::Result<BTreeMap<String, ManifestFacts>> {
    let mut map = BTreeMap::new();
    for (key, text) in pairs {
        map.insert(
            (*key).to_string(),
            discovery::parse_manifest_facts(text)
                .map_err(|error| anyhow::anyhow!("fixture manifest {key}: {error:#}"))?,
        );
    }
    Ok(map)
}

fn expect_failure(
    result: Result<(), anyhow::Error>,
    what: &'static str,
) -> anyhow::Result<anyhow::Error> {
    result.err().ok_or_else(|| anyhow::anyhow!("{what}"))
}

fn find_row<'a>(rows: &'a [DiscoveredTarget], target_id: &str) -> anyhow::Result<DiscoveredTarget> {
    rows.iter()
        .find(|target| target.target_id == target_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("discovery should contain {target_id}"))
}

fn violations_text(violations: &[Violation]) -> String {
    violations.iter().map(|violation| violation.to_string()).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Falsifier 1: new compiler-critical integration target without a topology row.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_new_integration_target_without_row_fails_check() -> anyhow::Result<()> {
    let manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let baseline = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests)?;
    let grown = discover_from_metadata(&parser_targets(ROOT_A, Some("new_cut_proof")), &manifests)?;
    let committed = rows_into_inventory(baseline)?;
    let findings = validate_inventory(&committed, &grown);
    let rendered = violations_text(&findings);
    assert!(
        rendered.contains("missing topology row"),
        "expected a missing-row finding, got:\n{rendered}"
    );
    assert!(
        rendered.contains("perl-parser-core/new_cut_proof/integration-test"),
        "finding should name the uncovered subject, got:\n{rendered}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 2: changed required feature subject without changed identity.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_feature_subject_drift_without_identity_change_detected()
-> anyhow::Result<()> {
    let mut manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let before = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests)?;
    let committed = rows_into_inventory(before)?;

    let drifted_manifest = PARSER_MANIFEST.replace(
        "[[test]]\nname = \"core_parse_contract\"",
        "[[test]]\nname = \"core_parse_contract\"\nrequired-features = [\"lsp-compat\"]",
    );
    manifests.insert(MANIFEST_A.to_string(), discovery::parse_manifest_facts(&drifted_manifest)?);
    let after = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests)?;
    let drifted = find_row(&after, "perl-parser-core/core_parse_contract/integration-test")?;
    assert_eq!(
        drifted.required_features,
        vec!["lsp-compat".to_string()],
        "manifest cross-check must surface the changed subject"
    );

    let findings = validate_inventory(&committed, &after);
    let rendered = violations_text(&findings);
    assert!(
        rendered.contains("required feature subject drift"),
        "expected feature-subject drift finding, got:\n{rendered}"
    );
    assert!(
        rendered.contains("subject fingerprint drift"),
        "identity stayed fixed so the fingerprint must move, got:\n{rendered}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 3: integration target represented as a library-test module.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_integration_target_as_library_module_rejected() -> anyhow::Result<()> {
    let manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let discovered = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests)?;

    // Layer 1 — incoherent flip: same identity, module kind. The closed
    // schema itself names this kind confusion instead of accepting it.
    let mut flipped = rows_into_inventory(discovered.clone())?;
    for row in &mut flipped.rows {
        if row.target_id == "perl-parser-core/core_parse_contract/integration-test" {
            row.target_kind = TargetKindV1::UnitTestModule;
        }
    }
    let findings = validate_inventory(&flipped, &discovered);
    assert!(
        violations_text(&findings).contains("kind confusion"),
        "an integration target relabeled as a library-test module must be rejected, got:\n{}",
        violations_text(&findings)
    );

    // Layer 2 — coherent relabel: identity, kind, and path agree with each
    // other but disagree with the live tree; the checker reports both sides.
    let mut committed = rows_into_inventory(discovered.clone())?;
    for row in &mut committed.rows {
        if row.target_id == "perl-parser-core/core_parse_contract/integration-test" {
            row.target_kind = TargetKindV1::UnitTestModule;
            row.target_id = "perl-parser-core/core_parse_contract/unit-test-module".to_string();
            row.path = "crates/perl-parser-core/src/lib.rs".to_string();
        }
    }
    let findings = validate_inventory(&committed, &discovered);
    let rendered = violations_text(&findings);
    assert!(
        rendered.contains("missing topology row")
            && rendered.contains("perl-parser-core/core_parse_contract/integration-test"),
        "the live integration subject must be reported missing, got:\n{rendered}"
    );
    assert!(
        rendered.contains("stale topology row")
            && rendered.contains("perl-parser-core/core_parse_contract/unit-test-module"),
        "the invented module subject must be reported stale, got:\n{rendered}"
    );

    // Constructor-level rejection: a tests/ path may never be a module row.
    let mut confused =
        find_row(&discovered, "perl-parser-core/core_parse_contract/integration-test")?
            .topology_row()?;
    confused.path = "crates/perl-parser-core/tests/core_parse_contract.rs".to_string();
    confused.target_kind = TargetKindV1::UnitTestModule;
    confused.target_id = "perl-parser-core/core_parse_contract/unit-test-module".to_string();
    let error = expect_failure(
        confused.validate(),
        "tests/ rows represented as module subjects must be rejected at construction",
    )?;
    assert!(format!("{error:#}").contains("kind confusion"), "unexpected error: {error:#}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 4: `cargo check --all-targets` compile evidence claimed as execution.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_compile_evidence_claimed_as_execution_refused() -> anyhow::Result<()> {
    let manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let discovered = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests)?;
    let mut committed = rows_into_inventory(discovered)?;
    for row in &mut committed.rows {
        row.execution_claim = ExecutionClaimV1 {
            claimed: true,
            evidence_ref: Some("cargo check --all-targets exit 0".to_string()),
        };
    }
    let error = expect_failure(
        committed.validate(),
        "claimed execution (even citing check-all-targets success) must be refused",
    )?;
    let rendered = format!("{error:#}");
    assert!(
        rendered.contains("execution claim refused"),
        "expected execution refusal, got:\n{rendered}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 5: feature-gated target compiling to zero under the default
// profile must still appear explicitly.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_feature_gated_zero_target_stays_explicit() -> anyhow::Result<()> {
    let manifests = manifests(&[(WORKSPACE_MANIFEST_A, WORKSPACE_MANIFEST)])?;
    let discovered = discover_from_metadata(&workspace_targets(ROOT_A), &manifests)?;
    let gated = find_row(&discovered, "perl-workspace/workspace_memory_profile/binary")?;
    assert_eq!(gated.required_features, vec!["memory-profiling".to_string()]);
    let row = gated.topology_row()?;
    assert_eq!(
        row.feature_subject.default_profile_state,
        DefaultProfileStateV1::FeatureGated,
        "feature-gated-zero subjects must not be collapsed into included-by-default"
    );
    assert_eq!(
        row.compile_obligation,
        CompileObligationV1::ExplicitFeatureBuildRequired,
        "compile obligations stay separate and explicit"
    );
    assert!(
        row.feature_subject.authority_refs.iter().any(|reference| reference == "#3790"),
        "gated subjects must reference the supported-combination authority"
    );
    let committed = rows_into_inventory(discovered.clone())?;
    assert!(
        committed.rows.iter().any(|candidate| candidate.target_id == gated.target_id),
        "the gated subject must remain an explicit row"
    );

    // Omission is a loud missing-row failure, never silent disappearance.
    let mut thinned_rows = committed.rows.clone();
    thinned_rows.retain(|candidate| candidate.target_id != gated.target_id);
    let thinned = TestTopologyInventoryV1::new(
        "compiler-critical",
        "fixture",
        &["#8437".to_string()],
        thinned_rows,
    )?;
    let findings = validate_inventory(&thinned, &discovered);
    assert!(
        violations_text(&findings).contains("missing topology row"),
        "omitting a gated subject must fail the check"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: duplicate target under path alias / different workspace root.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_duplicate_identity_across_roots_detected() -> anyhow::Result<()> {
    // Canonical identity is root-independent: identical layouts under two
    // roots produce identical ids, relative paths, and fingerprints.
    let manifests_a = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let root_b = "Y:/elsewhere";
    let manifest_b = "Y:/elsewhere/crates/perl-parser-core/Cargo.toml";
    let manifests_b = manifests(&[(manifest_b, PARSER_MANIFEST)])?;
    let a = discover_from_metadata(&parser_targets(ROOT_A, None), &manifests_a)?;
    let b = discover_from_metadata(&parser_targets(root_b, None), &manifests_b)?;
    assert_eq!(a.len(), b.len(), "both roots must yield the same cohort size");
    for (left, right) in a.iter().zip(b.iter()) {
        assert_eq!(left.target_id, right.target_id, "identity must survive root changes");
        assert_eq!(left.path, right.path, "stored paths are root-relative");
        assert_eq!(
            left.topology_row()?.subject_fingerprint,
            right.topology_row()?.subject_fingerprint,
            "fingerprints must survive root changes"
        );
    }

    // Hand-forged duplicate identities (path aliases) fail schema validation.
    let duplicated_inventory = duplicated_identity_fixture();
    let error = expect_failure(
        inventory_from_json(&duplicated_inventory).map(|_| ()),
        "duplicate canonical identity under path aliases must be rejected",
    )?;
    assert!(
        format!("{error:#}").contains("duplicate canonical identity"),
        "unexpected error: {error:#}"
    );
    Ok(())
}

fn duplicated_identity_fixture() -> String {
    let row_template = |path: &str, fingerprint: &str| {
        format!(
            r##"{{
                "target_id": "perl-parser-core/core_parse_contract/integration-test",
                "package_id": "perl-parser-core",
                "cargo_target_name": "core_parse_contract",
                "path": "{path}",
                "target_kind": "integration_test",
                "harness": true,
                "doctest": null,
                "feature_subject": {{"required": [], "default_profile_state": "included_by_default", "forbidden_under": [], "authority_refs": []}},
                "proof_role": "infrastructure",
                "controller_refs": ["#8437"],
                "candidate_profiles": ["pr_focused"],
                "minimum_nonzero_work": 1,
                "canonical_source_identity": {{"manifest_path": "crates/perl-parser-core/Cargo.toml", "source_path": "{path}"}},
                "compile_obligation": "included_in_check_all_targets",
                "execution_claim": {{}},
                "review_condition": "r",
                "retirement_condition": "r",
                "subject_fingerprint": "{fingerprint}"
            }}"##
        )
    };
    let first = row_template("crates/perl-parser-core/tests/core_parse_contract.rs", "x");
    let second = row_template("crates/perl-parser-core/tests/../tests/core_parse_contract.rs", "y");
    format!(
        r##"{{
            "schema_id": "test_topology_inventory.v1",
            "schema_version": 1,
            "cohort": "compiler-critical",
            "generated_by": "fixture",
            "regenerate_command": "fixture",
            "feature_authorities": ["#3790", "#8121"],
            "controllers": ["#8437"],
            "rows": [{first},{second}]
        }}"##
    )
}

// ---------------------------------------------------------------------------
// Falsifier 7: provider/refactor test classified as compile-only infrastructure.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_provider_read_misclassified_as_infrastructure_rejected()
-> anyhow::Result<()> {
    let hover_doc = serde_json::json!({
        "packages": [{
            "name": "perl-lsp-rs",
            "manifest_path": LSP_MANIFEST_A,
            "targets": [json_target(
                "lsp_hover_tests",
                &["test"],
                &format!("{ROOT_A}/crates/perl-lsp-rs/tests/lsp_hover_tests.rs"),
                &[],
            )],
        }],
        "workspace_root": ROOT_A,
    })
    .to_string();
    let manifests = manifests(&[(LSP_MANIFEST_A, LSP_MANIFEST)])?;
    let discovered = discover_from_metadata(&hover_doc, &manifests)?;
    let classified =
        find_row(&discovered, "perl-lsp-rs/lsp_hover_tests/integration-test")?.topology_row()?;
    assert_eq!(
        classified.proof_role,
        ProofRoleV1::ProviderRead,
        "hover subjects are provider-read proofs"
    );

    let mut committed = rows_into_inventory(discovered.clone())?;
    for row in &mut committed.rows {
        row.proof_role = ProofRoleV1::Infrastructure;
    }
    let findings = validate_inventory(&committed, &discovered);
    let rendered = violations_text(&findings);
    assert!(
        rendered.contains("proof role misassignment"),
        "misassigned provider-read rows must be rejected, got:\n{rendered}"
    );
    assert!(
        rendered.contains("provider_read") && rendered.contains("infrastructure"),
        "finding should name both roles, got:\n{rendered}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 8: output depends on metadata/filesystem ordering.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_output_bytes_are_ordering_independent() -> anyhow::Result<()> {
    let forward = mixed_cohort_doc();
    let reversed = serde_json::json!({
        "packages": forward_packages_reversed(),
        "workspace_root": ROOT_A,
    })
    .to_string();
    let manifests_forward =
        manifests(&[(MANIFEST_A, PARSER_MANIFEST), (WORKSPACE_MANIFEST_A, WORKSPACE_MANIFEST)])?;
    let manifests_reversed =
        manifests(&[(WORKSPACE_MANIFEST_A, WORKSPACE_MANIFEST), (MANIFEST_A, PARSER_MANIFEST)])?;
    let left = rows_into_inventory(discover_from_metadata(&forward, &manifests_forward)?)?;
    let right = rows_into_inventory(discover_from_metadata(&reversed, &manifests_reversed)?)?;
    assert_eq!(
        render_json(&left)?,
        render_json(&right)?,
        "JSON projection must be byte-identical regardless of input ordering"
    );
    assert_eq!(
        render_markdown(&left),
        render_markdown(&right),
        "Markdown projection must be byte-identical regardless of input ordering"
    );
    assert_eq!(
        render_report(&left),
        render_report(&right),
        "report projection must be identical regardless of input ordering"
    );
    Ok(())
}

fn mixed_cohort_doc() -> String {
    serde_json::json!({
        "packages": [
            {"name": "perl-workspace", "manifest_path": WORKSPACE_MANIFEST_A, "targets": [
                json_target("perl_workspace", &["lib"], &format!("{ROOT_A}/crates/perl-workspace/src/lib.rs"), &[]),
                json_target("workspace_memory_profile", &["bin"], &format!("{ROOT_A}/crates/perl-workspace/src/bin/workspace_memory_profile.rs"), &["memory-profiling"]),
            ]},
            {"name": "perl-parser-core", "manifest_path": MANIFEST_A, "targets": [
                json_target("core_parse_contract", &["test"], &format!("{ROOT_A}/crates/perl-parser-core/tests/core_parse_contract.rs"), &[]),
            ]},
        ],
        "workspace_root": ROOT_A,
    })
    .to_string()
}

fn forward_packages_reversed() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "perl-parser-core",
            "manifest_path": MANIFEST_A,
            "targets": [json_target(
                "core_parse_contract",
                &["test"],
                &format!("{ROOT_A}/crates/perl-parser-core/tests/core_parse_contract.rs"),
                &[],
            )],
        }),
        serde_json::json!({
            "name": "perl-workspace",
            "manifest_path": WORKSPACE_MANIFEST_A,
            "targets": [
                json_target("workspace_memory_profile", &["bin"], &format!("{ROOT_A}/crates/perl-workspace/src/bin/workspace_memory_profile.rs"), &["memory-profiling"]),
                json_target("perl_workspace", &["lib"], &format!("{ROOT_A}/crates/perl-workspace/src/lib.rs"), &[]),
            ],
        }),
    ]
}

// ---------------------------------------------------------------------------
// Falsifier 9: copied/redefined feature matrix instead of authority reference.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_copied_feature_matrix_rejected_in_favor_of_authorities()
-> anyhow::Result<()> {
    // Inventory-level: tampered authority list cannot replace #3790/#8121.
    let tampered = r##"{
        "schema_id": "test_topology_inventory.v1",
        "schema_version": 1,
        "cohort": "compiler-critical",
        "generated_by": "fixture",
        "regenerate_command": "fixture",
        "feature_authorities": [{"matrix": [["a","b"]]}],
        "controllers": ["#8437"],
        "rows": []
    }"##;
    let shape_error = expect_failure(
        inventory_from_json(&tampered).map(|_| ()),
        "non-authority feature payloads must be structurally rejected",
    )?;
    assert!(format!("{shape_error:#}").contains("rejected topology inventory"));

    let wrong_set = r##"{
        "schema_id": "test_topology_inventory.v1",
        "schema_version": 1,
        "cohort": "compiler-critical",
        "generated_by": "fixture",
        "regenerate_command": "fixture",
        "feature_authorities": ["#3790-copy", "#8121-mirror"],
        "controllers": ["#8437"],
        "rows": []
    }"##;
    let authority_error = expect_failure(
        inventory_from_json(&wrong_set).map(|_| ()),
        "copied authorities must be rejected",
    )?;
    assert!(
        format!("{authority_error:#}").contains("never redefine or copy"),
        "unexpected error: {authority_error:#}"
    );

    // Row-level: inline feature definitions have nowhere to live.
    let inline_matrix = inline_matrix_fixture();
    let row_error: anyhow::Error = serde_json::from_str::<TestTopologyRowV1>(&inline_matrix)
        .map(|_| ())
        .expect_err_value("inline matrices are not representable")?;
    assert!(format!("{row_error}").contains("unknown field"));

    let invented = FeatureSubjectV1::new(
        vec!["dap".to_string()],
        DefaultProfileStateV1::FeatureGated,
        Vec::new(),
        vec!["#3790-powerset-snapshot".to_string()],
    );
    let invented =
        expect_failure(invented.map(|_| ()), "invented authority references must be rejected")?;
    assert!(
        format!("{invented:#}").contains("unknown feature authority reference"),
        "unexpected error: {invented:#}"
    );
    Ok(())
}

fn inline_matrix_fixture() -> String {
    r##"{
        "target_id": "pkg/t/library",
        "package_id": "pkg",
        "cargo_target_name": "t",
        "path": "crates/pkg/src/lib.rs",
        "target_kind": "library",
        "harness": true,
        "doctest": true,
        "supported_feature_matrix": [["lsp-compat", "dap"]],
        "feature_subject": {"required": [], "default_profile_state": "included_by_default", "forbidden_under": [], "authority_refs": []},
        "proof_role": "infrastructure",
        "controller_refs": ["#8437"],
        "candidate_profiles": ["pr_focused"],
        "minimum_nonzero_work": 1,
        "canonical_source_identity": {"manifest_path": "c", "source_path": "s"},
        "compile_obligation": "included_in_check_all_targets",
        "execution_claim": {},
        "review_condition": "r",
        "retirement_condition": "r",
        "subject_fingerprint": "x"
    }"##.to_string()
}

trait ExpectErrValue<T> {
    fn expect_err_value(self, what: &'static str) -> anyhow::Result<anyhow::Error>;
}

impl<T> ExpectErrValue<T> for Result<T, anyhow::Error> {
    fn expect_err_value(self, what: &'static str) -> anyhow::Result<anyhow::Error> {
        self.err().ok_or_else(|| anyhow::anyhow!("{what}"))
    }
}

impl<T> ExpectErrValue<T> for std::result::Result<T, serde_json::Error> {
    fn expect_err_value(self, what: &'static str) -> anyhow::Result<anyhow::Error> {
        self.err().map(anyhow::Error::new).ok_or_else(|| anyhow::anyhow!("{what}"))
    }
}

// ---------------------------------------------------------------------------
// Falsifier 10: unknown target kind / proof role silently coerced.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_falsifier_unknown_kind_and_role_never_coerced() -> anyhow::Result<()> {
    for (field, value) in [("target_kind", "mystery_kind"), ("proof_role", "ordinary_compile_only")]
    {
        let payload = unknown_variant_fixture(field, value);
        let error = serde_json::from_str::<TestTopologyRowV1>(&payload)
            .map(|_| ())
            .expect_err_value("unknown enum values must be hard errors")?;
        assert!(
            format!("{error}").contains("unknown variant"),
            "{field}={value} must fail as unknown variant, got: {error}"
        );
    }

    // Discovery refuses unknown metadata kinds instead of coercing them.
    let mystery = serde_json::json!({
        "packages": [{
            "name": "perl-parser-core",
            "manifest_path": MANIFEST_A,
            "targets": [json_target("odd", &["proc-macro"], &format!("{ROOT_A}/odd.rs"), &[])],
        }],
        "workspace_root": ROOT_A,
    })
    .to_string();
    let manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let error = expect_failure(
        discover_from_metadata(&mystery, &manifests).map(|_| ()),
        "unknown metadata kinds must fail discovery",
    )?;
    assert!(format!("{error:#}").contains("never coerced"));
    Ok(())
}

fn unknown_variant_fixture(field: &str, value: &str) -> String {
    format!(
        r##"{{
            "target_id": "pkg/t/library",
            "package_id": "pkg",
            "cargo_target_name": "t",
            "path": "crates/pkg/src/lib.rs",
            "{field}": "{value}",
            "harness": true,
            "doctest": true,
            "feature_subject": {{"required": [], "default_profile_state": "included_by_default", "forbidden_under": [], "authority_refs": []}},
            "controller_refs": ["#8437"],
            "candidate_profiles": ["pr_focused"],
            "minimum_nonzero_work": 1,
            "canonical_source_identity": {{"manifest_path": "c", "source_path": "s"}},
            "compile_obligation": "included_in_check_all_targets",
            "execution_claim": {{}},
            "review_condition": "r",
            "retirement_condition": "r",
            "subject_fingerprint": "x"
        }}"##
    )
}

// ---------------------------------------------------------------------------
// Falsifier 9b: invented authority references survive deserialization.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_invented_authority_reference_rejected_after_deserialization() -> anyhow::Result<()>
{
    // A committed row whose authority reference was replaced by an invented
    // nonempty value must fail inventory validation even though serde skips
    // the checked constructor: the checker re-validates every deserialized
    // row against the #3790/#8121 authority set.
    let tampered = r##"{
        "schema_id": "test_topology_inventory.v1",
        "schema_version": 1,
        "cohort": "compiler-critical",
        "generated_by": "fixture",
        "regenerate_command": "fixture",
        "feature_authorities": ["#3790", "#8121"],
        "controllers": ["#8437"],
        "rows": [{
            "target_id": "perl-workspace/memory_profile/integration-test",
            "package_id": "perl-workspace",
            "cargo_target_name": "memory_profile",
            "path": "crates/perl-workspace/tests/memory_profile.rs",
            "target_kind": "integration_test",
            "harness": true,
            "feature_subject": {"required": ["workspace_memory_profile"], "default_profile_state": "feature_gated", "forbidden_under": [], "authority_refs": ["#9999-invented"]},
            "proof_role": "infrastructure",
            "controller_refs": ["#8437"],
            "candidate_profiles": ["pr_focused"],
            "minimum_nonzero_work": 1,
            "canonical_source_identity": {"manifest_path": "c", "source_path": "s"},
            "compile_obligation": "explicit_feature_build_required",
            "execution_claim": {},
            "review_condition": "r",
            "retirement_condition": "r",
            "subject_fingerprint": "x"
        }]
    }"##;
    let error = expect_failure(
        inventory_from_json(tampered).map(|_| ()),
        "invented per-row authority references must be rejected after deserialization",
    )?;
    assert!(format!("{error:#}").contains("#9999-invented"), "unexpected error: {error:#}");
    Ok(())
}

#[test]
fn test_topology_authority_references_move_the_subject_fingerprint() -> anyhow::Result<()> {
    let base = r##"{
        "target_id": "pkg/t/library",
        "package_id": "pkg",
        "cargo_target_name": "t",
        "path": "crates/pkg/src/lib.rs",
        "target_kind": "library",
        "harness": true,
        "doctest": true,
        "feature_subject": {"required": [], "default_profile_state": "included_by_default", "forbidden_under": [], "authority_refs": ["PLACEHOLDER"]},
        "proof_role": "infrastructure",
        "controller_refs": ["#8437"],
        "candidate_profiles": ["pr_focused"],
        "minimum_nonzero_work": 1,
        "canonical_source_identity": {"manifest_path": "c", "source_path": "s"},
        "compile_obligation": "included_in_check_all_targets",
        "execution_claim": {},
        "review_condition": "r",
        "retirement_condition": "r",
        "subject_fingerprint": "x"
    }"##;
    let mut row_a: TestTopologyRowV1 = serde_json::from_str(&base.replace("PLACEHOLDER", "#3790"))?;
    let mut row_b: TestTopologyRowV1 = serde_json::from_str(&base.replace("PLACEHOLDER", "#8121"))?;
    assert_ne!(
        row_a.compute_fingerprint(),
        row_b.compute_fingerprint(),
        "authority references are semantic subject facts; changing them must move the \
         fingerprint so corrupted boundaries cannot hide behind a stable identity"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Happy paths.
// ---------------------------------------------------------------------------

#[test]
fn test_topology_real_tree_inventory_round_trips_through_the_checker() -> anyhow::Result<()> {
    // Integration tests compile with CARGO_MANIFEST_DIR pointing at the
    // xtask package; its parent is the workspace root.
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask must live inside the workspace"))?
        .to_path_buf();
    let discovered = xtask::test_topology::discover_live(&root)?;
    assert!(
        discovered.len() >= 100,
        "compiler-critical cohort should cover hundreds of governed subjects, got {}",
        discovered.len()
    );
    let unique: BTreeSet<&str> =
        discovered.iter().map(|target| target.target_id.as_str()).collect();
    assert_eq!(unique.len(), discovered.len(), "canonical identities must be unique");

    let inventory = rows_into_inventory(discovered.clone())?;
    ensure_current(&inventory, &discovered)
        .map_err(|error| anyhow::anyhow!("fresh inventory must pass its own check: {error:#}"))?;

    // The serialized artifact round-trips through the closed schema.
    let json = render_json(&inventory)?;
    let reparsed = inventory_from_json(&json)?;
    assert_eq!(reparsed, inventory, "serialization round-trip preserves the inventory");
    ensure_current(&reparsed, &discovered)?;

    // Regeneration is byte-stable.
    let regenerated = rows_into_inventory(xtask::test_topology::discover_live(&root)?)?;
    assert_eq!(json, render_json(&regenerated)?, "same tree must give byte-identical JSON");

    // Cohort wiring sanity: seed packages and the xtask routing tests appear.
    assert!(
        unique.iter().any(|id| id.starts_with("perl-core-harness/")),
        "perl-core-harness subjects must be present"
    );
    assert!(
        unique.contains(&"xtask/gate_policy_profile_tests/integration-test"),
        "xtask gate-policy routing subjects must be present"
    );
    Ok(())
}

#[test]
fn test_topology_report_counts_match_inventory() -> anyhow::Result<()> {
    let manifests = manifests(&[(MANIFEST_A, PARSER_MANIFEST)])?;
    let discovered =
        discover_from_metadata(&parser_targets(ROOT_A, Some("second_case")), &manifests)?;
    let inventory = rows_into_inventory(discovered)?;
    let report = render_report(&inventory);

    let package_count =
        inventory.rows.iter().filter(|row| row.package_id == "perl-parser-core").count();
    assert!(
        report.contains(&format!("  perl-parser-core: {package_count}\n")),
        "report must state per-package counts matching the inventory\n{report}"
    );
    let pr_focused = inventory
        .rows
        .iter()
        .filter(|row| row.candidate_profiles.contains(&CandidateProfileV1::PrFocused))
        .count();
    assert!(
        report.contains(&format!("  pr_focused: {pr_focused}\n")),
        "report must state per-profile counts matching the inventory\n{report}"
    );
    let compiler_semantics = inventory
        .rows
        .iter()
        .filter(|row| matches!(row.proof_role, ProofRoleV1::CompilerSemantics))
        .count();
    assert!(
        report.contains(&format!("  compiler_semantics: {compiler_semantics}\n")),
        "report must state per-role counts matching the inventory\n{report}"
    );
    assert!(
        report.contains(&format!("rows={}", inventory.rows.len())),
        "report header must state the row total\n{report}"
    );
    Ok(())
}

#[test]
fn test_topology_cohort_surface_lists_evidence_based_extensions() {
    let packages: Vec<&str> = Cohort::CompilerCritical.packages().to_vec();
    for expected in [
        "perl-core-harness",
        "perl-core-harness-types",
        "perl-core-test-runner",
        "perl-parser-core",
        "perl-semantic-analyzer",
        "perl-workspace",
        "perl-lsp-rs-core",
        "perl-lsp-rs",
    ] {
        assert!(packages.contains(&expected), "seed package {expected} missing from cohort");
    }
    let extras = Cohort::CompilerCritical.extra_targets();
    assert!(
        extras.contains(&("xtask", "gate_policy_profile_tests")),
        "gate-policy routing subjects must ride with the cohort"
    );
}
