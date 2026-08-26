//! Tests for the reachability fixture manifest checker (#10998): schema
//! acceptance, every fail-closed validation rule, self-fixture documents,
//! determinism under row shuffling, and the real repository manifest.

use super::model::{
    CompletenessClaim, CurrentnessExpectation, CurrentnessTransition, DIGEST_ALGORITHM,
    EnvelopeClass, FactClass, FixtureIdentity, FixtureRole, IndependenceClass, InstrumentStatus,
    InstrumentStatusKind, Limitation, MANIFEST_NAME, MANIFEST_RELATIVE_PATH, Manifest,
    OperationExpectation, OperationStage, Oracle, OracleType, ProfileExpectation, ProfileName,
    ProofCeiling, RaceBarrier, RaceBarrierKind, ResultIdentity, Row, RowControls, RowExpectations,
    SCHEMA_ID, SCHEMA_VERSION, SupportClass, TerminalOutcome, TrainIdentity, TransportExpectation,
    TransportRoute, VIEW_RELATIVE_PATH,
};
use super::*;
use sha2::Digest as ShaDigest;
use sha2::Sha256;
use std::path::PathBuf;

type TestResult<T = ()> = Result<T>;

const ALLOWED_ROOTS: &[&str] = &["crates", "test_corpus"];
const SAMPLE_FIXTURE_RELATIVE: &str = "crates/reachability_fixtures/sample.pl";
const SAMPLE_FIXTURE_BODY: &str = "sub demo { return 1; }\nprint demo();\n";

fn digest_of(body: &str) -> String {
    let normalized: Vec<u8> = body.bytes().filter(|byte| *byte != b'\r').collect();
    Sha256::digest(&normalized).iter().map(|byte| format!("{byte:02x}")).collect()
}

struct Workspace {
    tempdir: tempfile::TempDir,
}

impl Workspace {
    fn new() -> TestResult<Self> {
        let tempdir = tempfile::tempdir()?;
        let fixture_path = tempdir.path().join(SAMPLE_FIXTURE_RELATIVE);
        fs::create_dir_all(
            fixture_path
                .parent()
                .ok_or_else(|| color_eyre::eyre::eyre!("fixture path lacks a parent directory"))?,
        )?;
        fs::write(&fixture_path, SAMPLE_FIXTURE_BODY)?;
        fs::create_dir_all(tempdir.path().join("schemas"))?;
        fs::write(
            tempdir.path().join("schemas/analysis_reachability_fixture_manifest.v1.schema.json"),
            valid_schema_text(),
        )?;
        Ok(Self { tempdir })
    }

    fn root(&self) -> PathBuf {
        self.tempdir.path().to_path_buf()
    }

    fn write_manifest(&self, manifest: &Manifest) -> TestResult<()> {
        let path = self.root().join(MANIFEST_RELATIVE_PATH);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, serde_json::to_string_pretty(manifest)?)?;
        Ok(())
    }
}

fn train(family: &str) -> TrainIdentity {
    TrainIdentity {
        node: "N.denominator".to_string(),
        component: family.to_string(),
        family: family.to_string(),
        claim_profile: "reachability-v1".to_string(),
    }
}

fn oracle(oracle_type: OracleType) -> Oracle {
    Oracle {
        oracle_type,
        independence_class: if oracle_type.is_implementation_derived() {
            IndependenceClass::ObservedOnly
        } else {
            IndependenceClass::Independent
        },
        proof_ceiling: ProofCeiling::InternalSemantic,
    }
}

fn supported_limitation() -> Limitation {
    Limitation { support_class: SupportClass::Supported, exit_owner_issue: None }
}

fn default_currentness() -> CurrentnessExpectation {
    CurrentnessExpectation {
        proposition: "exact results carry generation-scoped identity".to_string(),
        transition: CurrentnessTransition::IrrelevantEditCompleteEquivalent,
        generation: Some("gen-1".to_string()),
    }
}

fn positive_row(id: &str, opposite: Option<&str>) -> Row {
    Row {
        row_id: id.to_string(),
        fixture: FixtureIdentity {
            id: "sample".to_string(),
            path: SAMPLE_FIXTURE_RELATIVE.to_string(),
            digest_sha256_lf: digest_of(SAMPLE_FIXTURE_BODY),
            role: FixtureRole::Positive,
        },
        subjects: vec!["subject://demo".to_string()],
        source_roles: vec!["entry-script".to_string()],
        train: train("A_local_flow"),
        prerequisites: vec![],
        controls: RowControls { opposite: opposite.map(str::to_string), near_neighbour: None },
        expectations: RowExpectations::facts_with_currentness(
            "return terminates the callable body",
            FactClass::ExactValueOrEdge,
            default_currentness(),
        ),
        terminal: TerminalOutcome::CompleteNonempty,
        result_identity: Some(ResultIdentity {
            identity: "result://demo".to_string(),
            completeness: CompletenessClaim::SemanticComplete,
        }),
        authority_reference: None,
        limitation: supported_limitation(),
        oracle: oracle(OracleType::IndependentExpectedAuthority),
        race_barrier: None,
        instrument: None,
        owner_issue: None,
    }
}

fn control_row(id: &str) -> Row {
    let mut row = positive_row(id, None);
    row.fixture.role = FixtureRole::ControlOpposite;
    row.terminal = TerminalOutcome::DynamicOrUnsupported;
    row.result_identity = None;
    row.expectations = RowExpectations::facts_only(
        "same-spelling user subs never inherit exact flow",
        FactClass::AbsentFactNonEdge,
    );
    row
}

/// Builds a manifest whose denominator declares, for every family, the exact
/// stage/terminal slots its rows instantiate (or named deferrals covering
/// every vocabulary slot the document leaves uninstantiated), mirroring how
/// the canonical manifest keeps coverage mechanically cross-checked.
fn minimal_manifest(rows: Vec<Row>) -> Manifest {
    let mut covered_stages: Vec<&'static str> = Vec::new();
    let mut covered_terminals: Vec<&'static str> = Vec::new();
    for row in &rows {
        let terminal_token = row.terminal.wire_name();
        if !covered_terminals.contains(&terminal_token) {
            covered_terminals.push(terminal_token);
        }
        if let Some(operation) = &row.expectations.operation
            && !covered_stages.contains(&operation.stage.wire_name())
        {
            covered_stages.push(operation.stage.wire_name());
        }
    }

    let unit_deferral = |coverage: String| model::DeferredCoverage {
        coverage,
        owner_issue: 11004,
        reason: "unit-test deferral placeholder".to_string(),
    };

    let mut denominator = Vec::new();
    for family in model::FAMILIES {
        let family_rows: Vec<&Row> =
            rows.iter().filter(|row| row.train.family == **family).collect();
        let mut required_coverage = Vec::new();
        for row in &family_rows {
            let terminal_token = format!("terminal:{}", row.terminal.wire_name());
            if !required_coverage.contains(&terminal_token) {
                required_coverage.push(terminal_token);
            }
            if let Some(operation) = &row.expectations.operation {
                let stage_token = format!("stage:{}", operation.stage.wire_name());
                if !required_coverage.contains(&stage_token) {
                    required_coverage.push(stage_token);
                }
            }
        }
        let deferred_coverage = if family_rows.is_empty() {
            model::OperationStage::ALL
                .iter()
                .filter(|stage| !covered_stages.contains(&stage.wire_name()))
                .map(|stage| unit_deferral(format!("stage:{}", stage.wire_name())))
                .chain(
                    model::TerminalOutcome::ALL
                        .iter()
                        .filter(|terminal| !covered_terminals.contains(&terminal.wire_name()))
                        .map(|terminal| {
                            unit_deferral(format!("terminal:{}", terminal.wire_name()))
                        }),
                )
                .collect()
        } else {
            Vec::new()
        };
        denominator.push(model::FamilyDenominator {
            family: (*family).to_string(),
            required_coverage,
            deferred_coverage,
        });
    }
    Manifest {
        schema: SCHEMA_ID.to_string(),
        schema_version: SCHEMA_VERSION,
        manifest: MANIFEST_NAME.to_string(),
        owner_issue: 10998,
        status: "declaration-only".to_string(),
        claim_boundary: "Declaration only; no analysis execution, no semantic proof execution, \
no exact-process proof execution, no product behavior selection, no claim promotion; \
generated views derive from this manifest."
            .to_string(),
        digest_algorithm: DIGEST_ALGORITHM.to_string(),
        allowed_fixture_roots: ALLOWED_ROOTS.iter().map(|root| (*root).to_string()).collect(),
        authorities: vec!["#8062".to_string(), "#8149".to_string()],
        contracts: Default::default(),
        proof_owners: model::FAMILIES.iter().map(|family| ((*family).to_string(), 11004)).collect(),
        declared_row_count: rows.len() as u64,
        denominator,
        rows,
    }
}

#[test]
fn accepts_minimal_valid_manifest() -> TestResult {
    let workspace = Workspace::new()?;
    let manifest = minimal_manifest(vec![
        positive_row("a1-positive", Some("a2-opposite")),
        control_row("a2-opposite"),
    ]);
    workspace.write_manifest(&manifest)?;

    let violations = validate_document(&workspace.root(), &manifest);
    assert!(violations.is_empty(), "unexpected violations: {violations:?}");
    Ok(())
}

#[test]
fn rejects_duplicate_row_ids() -> TestResult {
    let manifest = minimal_manifest(vec![
        positive_row("same-id", Some("other")),
        control_row("other"),
        control_row("same-id"),
    ]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("duplicate row id")),
        "missing duplicate violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_unstable_fixture_identity() -> TestResult {
    let mut second = control_row("second");
    second.fixture.id = "sample".to_string();
    second.fixture.digest_sha256_lf = "0".repeat(64);
    let manifest = minimal_manifest(vec![positive_row("first", Some("second")), second]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("unstable fixture identity")),
        "missing instability violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_promoted_positive_without_opposite_control() -> TestResult {
    let manifest = minimal_manifest(vec![positive_row("lone-positive", None)]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("promoted positive row lacks an opposite")),
        "missing control violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_unknown_control_links() -> TestResult {
    let manifest = minimal_manifest(vec![positive_row("dangling", Some("does-not-exist"))]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("links unknown row")),
        "missing dangling-control violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_fixture_paths_outside_declared_roots() -> TestResult {
    let mut row = positive_row("escaped", None);
    row.controls.opposite = Some("escaped".to_string());
    row.fixture.path = "docs/specs/escape.pl".to_string();
    row.fixture.digest_sha256_lf = digest_of(SAMPLE_FIXTURE_BODY);
    let manifest = minimal_manifest(vec![row]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("escapes owned fixture roots without disposition")),
        "missing escape violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_digest_drift_against_referenced_bytes() -> TestResult {
    let mut row = positive_row("drifted", Some("drifted"));
    row.fixture.digest_sha256_lf = "0".repeat(64);
    let manifest = minimal_manifest(vec![row]);
    let workspace = Workspace::new()?;
    workspace.write_manifest(&manifest)?;
    let violations = validate_document(&workspace.root(), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("fixture digest drift")),
        "missing drift violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_non_success_terminal_carrying_result_identity() -> TestResult {
    let mut row = positive_row("stale-with-identity", Some("control"));
    row.terminal = TerminalOutcome::Stale;
    row.expectations = RowExpectations::currentness_only(CurrentnessExpectation {
        proposition: "stale tier must not be served as current".to_string(),
        transition: CurrentnessTransition::FailedRecomputationNeverUnchanged,
        generation: Some("gen-7".to_string()),
    });
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("must not carry result identity")),
        "missing identity violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_complete_terminal_without_result_identity() -> TestResult {
    let mut row = positive_row("complete-no-identity", Some("control"));
    row.result_identity = None;
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("requires named result identity authority")),
        "missing identity requirement: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_bounded_view_and_incomplete_semantic_collapse() -> TestResult {
    let mut row = positive_row("collapsed", Some("control"));
    row.terminal = TerminalOutcome::IncompleteSemanticNeverBoundedComplete;
    row.limitation.support_class = SupportClass::Partial;
    row.limitation.exit_owner_issue = Some(11006);
    row.expectations = RowExpectations::operation_only(OperationExpectation {
        proposition: "one row claims both bounded completeness and incompleteness".to_string(),
        stage: OperationStage::SemanticProof,
        work_dimensions: vec![],
        checkpoints: vec![],
        terminal_outcome: TerminalOutcome::BoundedViewComplete,
    });
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("collapse in one row")),
        "missing collapse violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_profile_without_work_dimensions_or_unsafe_partial() -> TestResult {
    let mut missing_dimensions = positive_row("profile-missing-dimensions", Some("unsafe-partial"));
    missing_dimensions.expectations = RowExpectations::profile_only(ProfileExpectation {
        proposition: "workspace-full profile must disposition dimensions".to_string(),
        profile: ProfileName::WorkspaceFull,
        required_work_dimensions: vec![],
        partial_support_advertised: false,
        envelope_class: EnvelopeClass::WithinAdmittedEnvelope,
    });

    let mut unsafe_partial = control_row("unsafe-partial");
    unsafe_partial.fixture.id = "sample-partial".to_string();
    unsafe_partial.expectations = RowExpectations::profile_only(ProfileExpectation {
        proposition: "partial stream advertised before safe commit proof".to_string(),
        profile: ProfileName::WorkspacePartial,
        required_work_dimensions: vec!["visited-components".to_string()],
        partial_support_advertised: true,
        envelope_class: EnvelopeClass::WithinAdmittedEnvelope,
    });

    let manifest = minimal_manifest(vec![missing_dimensions, unsafe_partial]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("omits required work dimensions")),
        "missing dimension violation: {violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.contains("advertises unsafe partial support")),
        "missing unsafe partial violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_push_race_without_client_visible_expectation() -> TestResult {
    let mut row = positive_row("push-race-blind", Some("control"));
    row.race_barrier = Some(RaceBarrier {
        kind: RaceBarrierKind::MidPushBatch,
        position: "after-second-contributor-push".to_string(),
    });
    row.expectations = RowExpectations::transport_only(TransportExpectation {
        proposition: "mid-batch supersession without client expectation".to_string(),
        route: TransportRoute::PublishDiagnostics,
        client_visible_expectation: None,
    });
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("lacks an exact client-visible/currentness expectation")),
        "missing race violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_implementation_derived_oracle_on_promoted_rows() -> TestResult {
    let mut row = positive_row("observed-positive", Some("control"));
    row.oracle = oracle(OracleType::ObservedOutputRetainedOnly);
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains(
            "implementation-derived observed output cannot serve as the expected oracle"
        )),
        "missing observed-oracle violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_unowned_rows_and_missing_exit_owners() -> TestResult {
    let mut unowned = positive_row("unowned", Some("exit-missing"));
    unowned.owner_issue = Some(9999);

    let mut exit_missing = control_row("exit-missing");
    exit_missing.limitation.support_class = SupportClass::UnsupportedOpenWorld;

    let manifest = minimal_manifest(vec![unowned, exit_missing]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("not a declared proof-owner issue")),
        "missing owner violation: {violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.contains("requires a named exit owner issue")),
        "missing exit-owner violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_terminal_instrument_failure_without_not_proven_disposition() -> TestResult {
    let mut row = positive_row("instrument-blank", Some("control"));
    row.terminal = TerminalOutcome::InstrumentFailure;
    row.instrument = Some(InstrumentStatus {
        status: InstrumentStatusKind::Missing,
        disposition: "assumed zero".to_string(),
    });
    let manifest = minimal_manifest(vec![row, control_row("control")]);
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations
            .iter()
            .any(|v| v.contains("requires present instrumentation or an explicit not_proven")),
        "missing instrument violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_family_with_zero_rows_and_empty_deferral() -> TestResult {
    // One W-family row; every other family — including A_local_flow — ends up
    // with neither rows nor deferrals once placeholders are stripped.
    let mut row = control_row("w-only");
    row.train = train("W_workspace_facts");
    let mut manifest = minimal_manifest(vec![row]);
    for entry in &mut manifest.denominator {
        entry.deferred_coverage.clear();
        entry.required_coverage.clear();
    }
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations
            .iter()
            .any(|v| v
                .contains("family \"A_local_flow\" claims denominator coverage without any row")),
        "missing empty-family violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_required_coverage_declared_without_rows() -> TestResult {
    let mut row = control_row("w-only");
    row.train = train("W_workspace_facts");
    let mut manifest = minimal_manifest(vec![row]);
    for entry in &mut manifest.denominator {
        if entry.family == "A_local_flow" {
            // Strip the placeholder deferrals so the required slot resolves
            // neither to a row nor to a named deferral.
            entry.deferred_coverage.clear();
            entry.required_coverage = vec!["terminal:checked_near_overflow".to_string()];
        }
    }
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| {
            v.contains("declares required_coverage")
                && v.contains("terminal:checked_near_overflow")
                && v.contains("without any denominator row")
        }),
        "missing required-coverage violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn rejects_vocabulary_slot_without_row_or_named_deferral() -> TestResult {
    // Remove the named deferrals for two uninstantiated slots; the
    // completeness pass must fail closed on each of them.
    let mut manifest = minimal_manifest(vec![
        positive_row("a1-positive", Some("a2-opposite")),
        control_row("a2-opposite"),
    ]);
    for entry in &mut manifest.denominator {
        entry.deferred_coverage.retain(|slot| {
            slot.coverage != "terminal:cancelled_before_start"
                && slot.coverage != "terminal:checked_near_overflow"
        });
    }
    let violations = validate_document(Path::new("/unused"), &manifest);
    assert!(
        violations.iter().any(|v| v.contains("CancelledBeforeStart (cancelled_before_start) has no denominator row and no named deferral")),
        "missing stage/terminal completeness violation: {violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.contains("CheckedNearOverflow (checked_near_overflow) has no denominator row and no named deferral")),
        "missing terminal completeness violation: {violations:?}"
    );
    Ok(())
}

#[test]
fn coverage_view_is_identical_under_row_shuffling() -> TestResult {
    let mut rows = Vec::new();
    for index in 0..12 {
        let id = format!("row-{index:02}");
        if index % 3 == 0 {
            rows.push(positive_row(&id, None));
        } else {
            rows.push(control_row(&id));
        }
    }
    for pair in rows.chunks_mut(2) {
        if pair.len() == 2 && pair[0].fixture.role == FixtureRole::Positive {
            pair[0].controls.opposite = Some(pair[1].row_id.clone());
        }
    }
    let forward = minimal_manifest(rows.clone());
    rows.reverse();
    let shuffled = minimal_manifest(rows);

    assert_eq!(super::view::render(&forward), super::view::render(&shuffled));
    Ok(())
}

#[test]
fn generated_view_drift_is_detected() -> TestResult {
    let manifest = minimal_manifest(vec![
        positive_row("a1-positive", Some("a2-opposite")),
        control_row("a2-opposite"),
    ]);
    let workspace = Workspace::new()?;
    workspace.write_manifest(&manifest)?;

    // No view yet → missing-view violation.
    let mut violations = validate_document(&workspace.root(), &manifest);
    super::validate_generated_view(&workspace.root(), &manifest, &mut violations);
    assert!(
        violations.iter().any(|v| v.contains("missing generated view")),
        "missing view detection: {violations:?}"
    );

    // Regenerated bytes pass.
    fs::write(workspace.root().join(VIEW_RELATIVE_PATH), super::view::render(&manifest))?;
    let mut violations = Vec::new();
    super::validate_generated_view(&workspace.root(), &manifest, &mut violations);
    assert!(violations.is_empty());

    // Drifted bytes fail closed.
    fs::write(workspace.root().join(VIEW_RELATIVE_PATH), "drifted\n")?;
    let mut violations = Vec::new();
    super::validate_generated_view(&workspace.root(), &manifest, &mut violations);
    assert!(
        violations.iter().any(|v| v.contains("generated view drifted")),
        "missing drift detection: {violations:?}"
    );
    Ok(())
}

#[test]
fn self_fixture_documents_fail_with_expected_codes() -> TestResult {
    let root = crate::utils::project_root()?;
    let invalid_dir = root.join("fixtures/analysis_reachability_denominator/self_fixtures/invalid");
    let expected: std::collections::BTreeMap<String, String> =
        serde_json::from_str(&fs::read_to_string(invalid_dir.join("expected_errors.json"))?)?;

    let mut checked = 0;
    for entry in fs::read_dir(&invalid_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") || name == "expected_errors.json" {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        let marker = expected
            .get(&name)
            .ok_or_else(|| color_eyre::eyre::eyre!("{name} lacks an expectation"))?;
        match parse_document(&text) {
            Err(error) => {
                assert!(
                    error.to_string().contains(marker.as_str()),
                    "{name}: parse error {:?} does not mention {marker:?}",
                    error.to_string()
                );
            }
            Ok(document) => {
                let violations = validate_document(&root, &document);
                assert!(
                    violations.iter().any(|violation| violation.contains(marker.as_str())),
                    "{name}: none of the violations mentions {marker:?}; got {violations:?}"
                );
            }
        }
        checked += 1;
    }
    assert!(checked >= 8, "self-fixture corpus unexpectedly small: {checked}");
    Ok(())
}

#[test]
fn real_repository_manifest_passes_validation() -> TestResult {
    let root = crate::utils::project_root()?;
    let stats = validate(&root)?;
    assert!(stats.rows >= 40, "denominator population too small: {}", stats.rows);
    assert_eq!(stats.families_covered, model::FAMILIES.len());
    Ok(())
}

fn valid_schema_text() -> &'static str {
    r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://effortlessmetrics.dev/perl-lsp/schemas/analysis_reachability_fixture_manifest.v1.schema.json",
  "properties": { "schema": { "const": "analysis_reachability_fixture_manifest.v1" } }
}"#
}
