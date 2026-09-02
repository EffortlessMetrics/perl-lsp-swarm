//! Proof for the `publication_sync_manifest.v1` model and the read-only plan
//! (#7972). Every negative control here corresponds to a way a manifest can
//! look attractive while authorizing an unproven or dishonest publication.

use super::*;
use color_eyre::eyre::{bail, eyre};
use serde_json::json;

const SCHEMA: &str = include_str!("../../../../schemas/publication_sync_manifest.v1.schema.json");
const CLEAN: &str = include_str!("../../../../fixtures/publication_sync/clean_manifest.json");
const RECONCILIATION_FIXTURE: &[u8] =
    include_bytes!("../../../../fixtures/publication_sync/reconciliation_receipt.json");
const LIVE_RECEIPT_SCHEMA: &str =
    include_str!("../../../../schemas/publication_live_control_receipt.v1.schema.json");
const LIVE_RECEIPT_FIXTURE: &[u8] =
    include_bytes!("../../../../fixtures/publication_sync/live_control_receipt.json");
const RECONCILIATION_PATH: &str = "docs/swarm/source-syncs/0.18.0-reconciliation-receipt.json";

// ---------------------------------------------------------------------------
// Fixture plumbing
// ---------------------------------------------------------------------------

/// Deterministic content for every declared release input. Shared by the
/// in-memory loader and the on-disk end-to-end fixture so both agree with the
/// digests baked into `clean_manifest.json`.
fn fixture_input_bytes(path: &str) -> Option<Vec<u8>> {
    if path == RECONCILIATION_PATH {
        return Some(RECONCILIATION_FIXTURE.to_vec());
    }
    // Live-control receipts are typed documents, not synthesized filler: the
    // planner parses them, so the fixture must serve a real one per control.
    if let Some(control) = path
        .strip_prefix("docs/release/0.18.0/live/")
        .and_then(|name| name.strip_suffix("_receipt.json"))
    {
        return Some(live_receipt_bytes(control, |_| {}));
    }
    if path.starts_with("docs/release/0.18.0/") || path == "docs/swarm/sync-protocol.md" {
        return Some(format!("publication-sync fixture input: {path}\n").into_bytes());
    }
    None
}

/// A valid live-control receipt for `control`, after applying `mutate`. Tests
/// mutate one identity/result/freshness field at a time from this baseline.
fn live_receipt_bytes(control: &str, mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut receipt = json!({
        "schema_version": "publication_live_control_receipt.v1",
        "control": control,
        "repository": "EffortlessMetrics/perl-lsp",
        "release": "0.18.0",
        "result": "proven",
        "observed_at": "2026-08-30",
        "observation_method": format!("gh api repos/EffortlessMetrics/perl-lsp {control} readout"),
        "observation_authority": "release/ci publication observer",
        "observed_state": {
            "summary": format!("{control} observed for the 0.18.0 public-beta transaction")
        }
    });
    mutate(&mut receipt);
    let mut rendered = serde_json::to_string_pretty(&receipt).unwrap_or_default();
    rendered.push('\n');
    rendered.into_bytes()
}

/// Re-point `branch_rules` at a mutated receipt and re-declare its digest, so
/// the byte-identity layer still passes and only the semantic layer is on trial.
fn plan_with_mutated_receipt(mutate: impl FnOnce(&mut Value)) -> Result<Receipt> {
    let bytes = live_receipt_bytes("branch_rules", mutate);
    let digest = sha256_digest(&bytes);
    MUTATED.with(|cell| *cell.borrow_mut() = Some(bytes));

    let mut document = clean_value()?;
    document["live_controls"]["branch_rules"]["evidence"][0]["reference"] = json!(MUTATED_PATH);
    document["live_controls"]["branch_rules"]["evidence"][0]["digest"] = json!(digest);

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let manifest_digest = canonical_digest(&document)?;
    let outcome = evaluate_with_surface(
        &manifest,
        &manifest_digest,
        Path::new("."),
        mutating_loader,
        &test_product_surface(),
    );
    MUTATED.with(|cell| *cell.borrow_mut() = None);
    outcome
}

const MUTATED_PATH: &str = "docs/release/0.18.0/live/mutated_receipt.json";

thread_local! {
    static MUTATED: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
}

fn mutating_loader(_repo_root: &Path, path: &str) -> Result<Vec<u8>, LoadFailure> {
    if path == MUTATED_PATH {
        return MUTATED.with(|cell| cell.borrow().clone()).ok_or(LoadFailure::Missing);
    }
    fixture_input_bytes(path).ok_or(LoadFailure::Missing)
}

fn test_loader(_repo_root: &Path, path: &str) -> Result<Vec<u8>, LoadFailure> {
    fixture_input_bytes(path).ok_or(LoadFailure::Missing)
}

/// The product surface the row validator consults. Tests drive the classifier
/// directly rather than materializing the repository's allowlist, except where
/// the allowlist itself is the subject.
fn test_product_surface() -> ProductSurface {
    ProductSurface::from_entries_for_test(vec![
        ("clients/sublime/LSP-perllsp/plugin.py", "production"),
        ("clients/lite-xl/compose.lua", "production"),
        ("vscode-extension/package.json", "production"),
        ("clients/lite-xl/leaves/base/init.lua", "test"),
    ])
}

fn clean_value() -> Result<Value> {
    serde_json::from_str(CLEAN).context("parsing the clean manifest fixture")
}

fn plan_value(document: &Value) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())
        .context("parsing the mutated fixture as publication_sync_manifest.v1")?;
    let digest = canonical_digest(document)?;
    evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &test_product_surface())
}

/// Evaluate against a materialized repository root, using the real on-disk
/// loader so filesystem-shaped rules (directory rows, crate roots) are exercised.
fn plan_on_disk(document: &Value, root: &Path) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(document)?;
    evaluate_with_surface(&manifest, &digest, root, load_input, &test_product_surface())
}

fn rows_mut(document: &mut Value) -> Result<&mut Vec<Value>> {
    document
        .get_mut("paths")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("fixture has no paths array"))
}

fn inputs_mut(document: &mut Value) -> Result<&mut Vec<Value>> {
    document
        .get_mut("inputs")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("fixture has no inputs array"))
}

fn assert_finding(receipt: &Receipt, code: &str) -> Result<()> {
    if receipt.findings.iter().any(|finding| finding.code == code) {
        return Ok(());
    }
    let observed: Vec<&str> =
        receipt.findings.iter().map(|finding| finding.code.as_str()).collect();
    bail!("expected finding {code}; receipt carried {observed:?}")
}

fn assert_verdict(receipt: &Receipt, expected: Verdict) -> Result<()> {
    if receipt.verdict == expected {
        return Ok(());
    }
    bail!("expected verdict {expected} but the plan returned {}", receipt.verdict)
}

// ---------------------------------------------------------------------------
// Positive control
// ---------------------------------------------------------------------------

#[test]
fn clean_manifest_plans_pass() -> Result<()> {
    let receipt = plan_value(&clean_value()?)?;
    if !receipt.findings.is_empty() {
        bail!("clean manifest produced findings: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)?;
    if receipt.rows.len() != 4 {
        bail!("clean manifest projected {} rows", receipt.rows.len());
    }
    if receipt.inputs.len() != 6 {
        bail!("clean manifest bound {} inputs", receipt.inputs.len());
    }
    Ok(())
}

#[test]
fn clean_fixture_conforms_to_the_published_schema() -> Result<()> {
    let schema: Value = serde_json::from_str(SCHEMA).context("parsing the manifest schema")?;
    let document = clean_value()?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    if !errors.is_empty() {
        bail!("clean fixture violates its own schema: {errors:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Closed vocabularies fail closed
// ---------------------------------------------------------------------------

#[test]
fn unknown_action_token_is_rejected_by_model_and_schema() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["action"] = json!("blend");

    if serde_json::from_value::<Manifest>(document.clone()).is_ok() {
        bail!("an unknown action token parsed into the typed model");
    }

    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;
    if validator.is_valid(&document) {
        bail!("an unknown action token passed the published schema");
    }
    Ok(())
}

#[test]
fn unknown_class_token_is_rejected_by_model_and_schema() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["class"] = json!("editorial_cleanup");

    if serde_json::from_value::<Manifest>(document.clone()).is_ok() {
        bail!("an unknown class token parsed into the typed model");
    }
    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;
    if validator.is_valid(&document) {
        bail!("an unknown class token passed the published schema");
    }
    Ok(())
}

#[test]
fn unknown_track_and_input_id_tokens_are_rejected() -> Result<()> {
    let mut track = clean_value()?;
    track["track"] = json!("internal-preview");
    if serde_json::from_value::<Manifest>(track).is_ok() {
        bail!("an unknown track token parsed into the typed model");
    }

    let mut input = clean_value()?;
    inputs_mut(&mut input)?[1]["id"] = json!("vibes_audit");
    if serde_json::from_value::<Manifest>(input).is_ok() {
        bail!("an unknown release-input id parsed into the typed model");
    }
    Ok(())
}

#[test]
fn unknown_field_fails_closed() -> Result<()> {
    let mut document = clean_value()?;
    document["publication_note"] = json!("a foreign manifest shape");
    if serde_json::from_value::<Manifest>(document).is_ok() {
        bail!("an unknown top-level field parsed into the typed model");
    }
    Ok(())
}

#[test]
fn a_non_take_swarm_basis_cannot_be_declared() -> Result<()> {
    let mut document = clean_value()?;
    document["default_action"] = json!("take_release");
    if serde_json::from_value::<Manifest>(document).is_ok() {
        bail!("a manifest declared a projection basis other than the prepared swarm tree");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Row-level negative controls
// ---------------------------------------------------------------------------

#[test]
fn duplicate_path_row_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    let rows = rows_mut(&mut document)?;
    let duplicate = rows.first().cloned().ok_or_else(|| eyre!("fixture has no rows"))?;
    rows.push(duplicate);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_duplicate_path")
}

#[test]
fn parent_and_child_rows_are_ambiguous() -> Result<()> {
    let mut document = clean_value()?;
    let rows = rows_mut(&mut document)?;
    let mut parent = rows.first().cloned().ok_or_else(|| eyre!("fixture has no rows"))?;
    parent["path"] = json!("docs/policy");
    parent["action"] = json!("translate");
    rows.push(parent);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_path_ambiguous")
}

#[test]
fn sibling_paths_sharing_a_prefix_are_not_ambiguous() -> Result<()> {
    // Negative control for the ambiguity rule itself: `docs/policy/A.md` and
    // `docs/policy/A.md.bak` share a textual prefix but neither contains the
    // other, so a naive `starts_with` check would produce a false positive.
    let mut document = clean_value()?;
    let rows = rows_mut(&mut document)?;
    let mut sibling = rows.first().cloned().ok_or_else(|| eyre!("fixture has no rows"))?;
    sibling["path"] = json!("docs/policy/NON_RUST_INVENTORY.md.bak");
    rows.push(sibling);

    let receipt = plan_value(&document)?;
    if receipt.findings.iter().any(|finding| finding.code == "row_path_ambiguous") {
        bail!("sibling paths sharing a prefix were reported as ambiguous");
    }
    Ok(())
}

#[test]
fn product_bearing_exclusion_is_blocked() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("crates/perl-parser/src/lexer.rs");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn preserving_release_over_a_test_path_is_blocked() -> Result<()> {
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["path"] = json!("tests/publication_projection.rs");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn translating_a_product_path_is_not_a_product_bearing_exclusion() -> Result<()> {
    // A translation still ships the swarm content; only operations that
    // withhold prepared work hide product divergence. Without this control the
    // exclusion rule could be satisfied by refusing every product path.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["path"] = json!("crates/perl-parser/src/lib.rs");

    let receipt = plan_value(&document)?;
    if receipt.findings.iter().any(|finding| finding.code == "row_product_bearing_exclusion") {
        bail!("a translation was reported as a product-bearing exclusion");
    }
    Ok(())
}

#[test]
fn missing_row_authority_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("   ");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_authority_missing")
}

#[test]
fn unresolvable_row_authority_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("because we always did it this way");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_authority_unresolved")
}

#[test]
fn dropping_a_path_may_not_also_project_it() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["expected_public_digest"] =
        json!("sha256:2000000000000000000000000000000000000000000000000000000000000009");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_expected_digest_inconsistent")
}

#[test]
fn a_translation_that_changes_nothing_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[0];
    let source = row["source_digest"].clone();
    row["expected_public_digest"] = source;

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_translation_is_identity")
}

#[test]
fn preserve_release_may_not_project_a_third_digest() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[2]["expected_public_digest"] =
        json!("sha256:3000000000000000000000000000000000000000000000000000000000000009");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_preserve_release_diverges")
}

#[test]
fn traversal_and_absolute_row_paths_are_rejected() -> Result<()> {
    for candidate in ["../perl-lsp/README.md", "/etc/passwd", "docs/../../escape.md"] {
        let mut document = clean_value()?;
        rows_mut(&mut document)?[0]["path"] = json!(candidate);
        let receipt = plan_value(&document)?;
        assert_verdict(&receipt, Verdict::NotProven)?;
        assert_finding(&receipt, "row_path_invalid")?;
    }
    Ok(())
}

/// The published schema and the planner must agree on what a repository path
/// is. A schema that accepts a path the planner refuses would let a manifest
/// look contract-valid while being unplannable, and the reverse would let the
/// contract bless an escape the planner only happens to catch.
#[test]
fn schema_and_planner_agree_on_repository_paths() -> Result<()> {
    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;

    let cases = [
        ("README.md", true),
        ("docs/policy/FILE_POLICY.md", true),
        (".claude/settings.json", true),
        ("a", true),
        ("...", true),
        ("/etc/passwd", false),
        ("../perl-lsp/README.md", false),
        ("docs/../../escape.md", false),
        ("docs/./here.md", false),
        ("docs//here.md", false),
        ("trailing/", false),
        ("windows\\path.md", false),
        (".", false),
        ("..", false),
    ];

    for (candidate, expected) in cases {
        let mut document = clean_value()?;
        rows_mut(&mut document)?[0]["path"] = json!(candidate);
        let by_schema = validator.is_valid(&document);
        let by_planner = valid_repository_path(candidate);

        if by_planner != expected {
            bail!("planner returned {by_planner} for {candidate:?}, expected {expected}");
        }
        if by_schema != expected {
            bail!("schema returned {by_schema} for {candidate:?}, expected {expected}");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Input identity and reconciliation currentness
// ---------------------------------------------------------------------------

#[test]
fn input_digest_mismatch_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    inputs_mut(&mut document)?[3]["digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "input_digest_mismatch")
}

#[test]
fn a_missing_input_file_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    inputs_mut(&mut document)?[4]["path"] = json!("docs/release/0.19.0/public_claims.json");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "input_missing")
}

#[test]
fn a_missing_required_input_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    let inputs = inputs_mut(&mut document)?;
    inputs.retain(|input| input.get("id").and_then(Value::as_str) != Some("release_integrity"));

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "input_required_missing")
}

#[test]
fn a_stale_reconciliation_receipt_is_blocked() -> Result<()> {
    // The reconciliation receipt reconciled S=1111..., so a manifest that
    // projects a different prepared swarm commit is consuming stale evidence.
    let mut document = clean_value()?;
    document["prepared_swarm_sha"] = json!("9999999999999999999999999999999999999999");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "reconciliation_stale")
}

#[test]
fn a_stale_release_base_is_blocked() -> Result<()> {
    let mut document = clean_value()?;
    document["release_base_sha"] = json!("8888888888888888888888888888888888888888");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "reconciliation_stale")
}

#[test]
fn a_non_passing_reconciliation_receipt_cannot_be_consumed() -> Result<()> {
    // The loader is keyed on path, so redirect the reconciliation input at a
    // path whose fixture content carries a blocked verdict.
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["verdict"] = json!("blocked");
    let blocked = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&blocked), &mut state);
    let (verdict, findings) = state.finish();
    if verdict != Verdict::Blocked {
        bail!("a blocked reconciliation receipt produced verdict {verdict}");
    }
    if !findings.iter().any(|finding| finding.code == "reconciliation_not_passing") {
        bail!("a blocked reconciliation receipt produced {findings:?}");
    }
    Ok(())
}

#[test]
fn an_unreadable_reconciliation_receipt_is_not_proven() -> Result<()> {
    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(b"{\"verdict\": \"pass\"}"), &mut state);
    let (verdict, findings) = state.finish();
    if verdict != Verdict::NotProven {
        bail!("an unreadable reconciliation receipt produced verdict {verdict}");
    }
    if !findings.iter().any(|finding| finding.code == "reconciliation_unreadable") {
        bail!("an unreadable reconciliation receipt produced {findings:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Invariants, live controls and declared blockers
// ---------------------------------------------------------------------------

#[test]
fn a_blocked_required_invariant_blocks_the_plan() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"][0]["result"] = json!("blocked");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "invariant_blocked")
}

#[test]
fn an_unproven_invariant_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"][1]["result"] = json!("not_proven");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "invariant_not_proven")
}

#[test]
fn an_invariant_cannot_pass_without_evidence() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"][2]["evidence"] = json!([]);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "invariant_unevidenced_pass")
}

#[test]
fn a_source_only_live_control_claim_is_not_proven() -> Result<()> {
    // Checked-in policy proves what the repository says, never what the live
    // control plane currently enforces.
    let mut document = clean_value()?;
    document["live_controls"]["branch_rules"]["evidence"] = json!([
        { "kind": "repository_source", "reference": ".github/workflows/publication-sync-contract.yml" },
        { "kind": "review_ruling", "reference": "#7647" }
    ]);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_control_source_only")
}

#[test]
fn a_blocked_live_control_blocks_the_plan() -> Result<()> {
    let mut document = clean_value()?;
    document["live_controls"]["environments"] =
        json!({ "result": "blocked", "max_observation_age_days": 14, "evidence": [] });

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "live_control_blocked")
}

#[test]
fn a_declared_blocker_blocks_the_plan() -> Result<()> {
    let mut document = clean_value()?;
    document["blockers"] = json!([
        { "code": "quality_exception_expired", "message": "waiver review_after has passed", "owner": "release/ci" }
    ]);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "manifest_declared_blocker")
}

#[test]
fn not_proven_dominates_blocked() -> Result<()> {
    // A manifest we cannot verify must not be reported as a known hard stop:
    // the two states carry different remediation meaning.
    let mut document = clean_value()?;
    document["blockers"] = json!([
        { "code": "declared", "message": "a declared hard stop", "owner": "release/ci" }
    ]);
    document["invariants"][0]["result"] = json!("not_proven");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "manifest_declared_blocker")?;
    assert_finding(&receipt, "invariant_not_proven")
}

#[test]
fn a_degenerate_identity_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    let swarm = document["prepared_swarm_sha"].clone();
    document["release_base_sha"] = swarm;

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "manifest_identity_degenerate")
}

// ---------------------------------------------------------------------------
// Determinism and canonical digest
// ---------------------------------------------------------------------------

#[test]
fn receipt_collections_are_deterministically_ordered() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?.reverse();
    inputs_mut(&mut document)?.reverse();
    document
        .get_mut("invariants")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("fixture has no invariants"))?
        .reverse();

    let receipt = plan_value(&document)?;
    if !receipt.rows.windows(2).all(|pair| pair[0].path <= pair[1].path) {
        bail!("receipt rows were not sorted");
    }
    if !receipt.inputs.windows(2).all(|pair| pair[0].id <= pair[1].id) {
        bail!("receipt inputs were not sorted");
    }
    if !receipt.invariants.windows(2).all(|pair| pair[0].id <= pair[1].id) {
        bail!("receipt invariants were not sorted");
    }
    if !receipt.live_controls.windows(2).all(|pair| pair[0].control <= pair[1].control) {
        bail!("receipt live controls were not sorted");
    }
    Ok(())
}

#[test]
fn findings_are_sorted_and_deduplicated() -> Result<()> {
    let mut state = PlanState::default();
    state.not_proven("zulu", "last", "release/ci");
    state.not_proven("alpha", "first", "release/ci");
    state.not_proven("alpha", "first", "release/ci");
    let (_, findings) = state.finish();

    let codes: Vec<&str> = findings.iter().map(|finding| finding.code.as_str()).collect();
    if codes != ["alpha", "zulu"] {
        bail!("findings were not sorted and deduplicated: {codes:?}");
    }
    Ok(())
}

#[test]
fn the_plan_is_reproducible_from_identical_inputs() -> Result<()> {
    let document = clean_value()?;
    let first = serde_json::to_string(&plan_value(&document)?)?;
    let second = serde_json::to_string(&plan_value(&document)?)?;
    if first != second {
        bail!("two plans over identical inputs differed");
    }
    Ok(())
}

#[test]
fn the_manifest_digest_ignores_formatting_and_key_order() -> Result<()> {
    let document = clean_value()?;
    let reordered: Value = serde_json::from_str(&serde_json::to_string_pretty(&document)?)?;
    if canonical_digest(&document)? != canonical_digest(&reordered)? {
        bail!("re-serializing the same document changed its digest");
    }

    let a = json!({ "left": 1, "right": 2 });
    let b = json!({ "right": 2, "left": 1 });
    if canonical_digest(&a)? != canonical_digest(&b)? {
        bail!("key order changed the canonical digest");
    }
    Ok(())
}

#[test]
fn moving_a_value_between_fields_changes_the_manifest_digest() -> Result<()> {
    // The falsifier for a weak digest: swapping two rows' `reason` strings
    // leaves the multiset of leaf values identical and changes only where each
    // value sits. A digest over concatenated or sorted leaves would not move.
    let document = clean_value()?;
    let mut moved = document.clone();
    {
        let rows = rows_mut(&mut moved)?;
        let first = rows
            .first()
            .and_then(|row| row.get("reason"))
            .cloned()
            .ok_or_else(|| eyre!("fixture row 0 has no reason"))?;
        let second = rows
            .get(1)
            .and_then(|row| row.get("reason"))
            .cloned()
            .ok_or_else(|| eyre!("fixture row 1 has no reason"))?;
        rows[0]["reason"] = second;
        rows[1]["reason"] = first;
    }

    if canonical_digest(&document)? == canonical_digest(&moved)? {
        bail!("moving a value between rows did not change the manifest digest");
    }
    Ok(())
}

#[test]
fn reordering_rows_changes_the_manifest_digest() -> Result<()> {
    // Array order is projection order, so it is part of the identity even
    // though object key order is not.
    let document = clean_value()?;
    let mut reversed = document.clone();
    rows_mut(&mut reversed)?.reverse();

    if canonical_digest(&document)? == canonical_digest(&reversed)? {
        bail!("reordering projection rows did not change the manifest digest");
    }
    Ok(())
}

#[test]
fn canonical_bytes_are_stable_and_compact() -> Result<()> {
    let document = clean_value()?;
    let mut bytes = String::new();
    canonical_json(&document, &mut bytes)?;
    let mut again = String::new();
    canonical_json(&document, &mut again)?;
    if bytes != again {
        bail!("canonical serialization was not stable");
    }
    if bytes.contains('\n') || bytes.contains(": ") {
        bail!("canonical serialization carried insignificant whitespace");
    }
    Ok(())
}

#[test]
fn digest_and_object_name_validators_reject_near_misses() -> Result<()> {
    let bad_digests = [
        "sha256:ABCDEF0000000000000000000000000000000000000000000000000000000000",
        "sha256:00",
        "sha1:0000000000000000000000000000000000000000",
        "0000000000000000000000000000000000000000000000000000000000000000",
    ];
    for candidate in bad_digests {
        if is_sha256_digest(candidate) {
            bail!("{candidate} was accepted as a sha256 digest");
        }
    }
    if !is_sha256_digest("sha256:0000000000000000000000000000000000000000000000000000000000000000")
    {
        bail!("a well-formed digest was rejected");
    }

    for candidate in ["", "1111", "11111111111111111111111111111111111111111", "ZZZZ"] {
        if is_object_name(candidate) {
            bail!("{candidate} was accepted as an object name");
        }
    }
    if !is_object_name("1111111111111111111111111111111111111111") {
        bail!("a well-formed object name was rejected");
    }
    Ok(())
}

#[test]
fn repository_path_validator_rejects_escapes() -> Result<()> {
    for candidate in ["", "/abs", "a//b", "../up", "a/../b", "a/./b", "a\\b", "trailing/"] {
        if valid_repository_path(candidate) {
            bail!("{candidate} was accepted as a repository-relative path");
        }
    }
    for candidate in ["README.md", "docs/policy/FILE_POLICY.md", ".claude/settings.json"] {
        if !valid_repository_path(candidate) {
            bail!("{candidate} was rejected as a repository-relative path");
        }
    }
    Ok(())
}

#[test]
fn path_prefix_detection_is_segment_aware() -> Result<()> {
    if !is_path_prefix("docs", "docs/policy/FILE_POLICY.md") {
        bail!("a genuine parent path was not detected");
    }
    if is_path_prefix("docs", "docsite/index.md") {
        bail!("a textual prefix was treated as a parent directory");
    }
    if is_path_prefix("docs", "docs") {
        bail!("a path was treated as its own parent");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// End-to-end: the command is read-only and writes a receipt either way
// ---------------------------------------------------------------------------

/// Materializes a repository root carrying exactly the manifest's declared
/// inputs, then runs the real `plan` entry point against it.
fn materialize_repo(document: &Value) -> Result<(tempfile::TempDir, PathBuf, PathBuf)> {
    let root = tempfile::tempdir().context("creating a fixture repository root")?;

    let write = |relative: &str, bytes: Vec<u8>| -> Result<()> {
        let destination = root.path().join(relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, bytes)?;
        Ok(())
    };

    // Every repository artifact the planner resolves: release inputs, the live
    // control receipts, and any document authority a row cites.
    let mut materialized: BTreeSet<String> = BTreeSet::new();
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    for input in &manifest.inputs {
        materialized.insert(input.path.clone());
    }
    for control in [
        &manifest.live_controls.branch_rules,
        &manifest.live_controls.environments,
        &manifest.live_controls.quality_exceptions,
    ] {
        for evidence in &control.evidence {
            if evidence.kind == EvidenceKind::LiveReceipt {
                materialized.insert(evidence.reference.clone());
            }
        }
    }
    for row in &manifest.paths {
        if valid_repository_path(&row.authority_ref) && row.authority_ref.contains('/') {
            materialized.insert(row.authority_ref.clone());
        }
    }
    for relative in materialized {
        if let Some(bytes) = fixture_input_bytes(&relative) {
            write(&relative, bytes)?;
        }
    }

    // The product-surface authority. A fixture root without it is a legitimate
    // `not_proven`, so the end-to-end positive control must provide one.
    write("policy/non-rust-allowlist.toml", FIXTURE_ALLOWLIST.as_bytes().to_vec())?;

    let manifest_path = root.path().join("publication_sync_manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(document)?)?;
    let receipt_path = root.path().join("receipts/publication-sync-plan.json");
    Ok((root, manifest_path, receipt_path))
}

/// A minimal product-surface ledger in the real allowlist's shape.
const FIXTURE_ALLOWLIST: &str = r#"
schema_version = 1
policy = "non-rust-allowlist"

[[allow]]
id = "fixture-sublime-plugin"
path = "clients/sublime/LSP-perllsp/plugin.py"
kind = "editor_extension"
language = "python"
surface = "editor"
classification = "production"
owner = "editor/sublime"
reason = "Fixture product surface row."
created = "2026-09-02"
review_after = "2026-12-02"
"#;

#[test]
fn plan_writes_a_pass_receipt_and_leaves_the_tree_alone() -> Result<()> {
    let document = clean_value()?;
    let (root, manifest, receipt) = materialize_repo(&document)?;
    let before = fs::read(&manifest)?;

    plan(PlanConfig {
        manifest: manifest.clone(),
        repo_root: root.path().to_path_buf(),
        receipt: receipt.clone(),
    })?;

    let written: Value = serde_json::from_slice(&fs::read(&receipt)?)?;
    if written.get("verdict").and_then(Value::as_str) != Some("pass") {
        bail!("the end-to-end plan did not pass: {written}");
    }
    if written.get("manifest_digest").and_then(Value::as_str) != Some(&canonical_digest(&document)?)
    {
        bail!("the receipt reported a different manifest digest");
    }
    if fs::read(&manifest)? != before {
        bail!("planning mutated the manifest it read");
    }
    Ok(())
}

#[test]
fn plan_fails_loudly_but_still_writes_the_receipt() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"][0]["result"] = json!("blocked");
    let (root, manifest, receipt) = materialize_repo(&document)?;

    let outcome = plan(PlanConfig {
        manifest,
        repo_root: root.path().to_path_buf(),
        receipt: receipt.clone(),
    });
    if outcome.is_ok() {
        bail!("a blocked manifest returned success");
    }
    let written: Value = serde_json::from_slice(&fs::read(&receipt)?)?;
    if written.get("verdict").and_then(Value::as_str) != Some("blocked") {
        bail!("the blocked receipt was not written: {written}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Product-surface exclusion: every displacing action, over the whole surface
// ---------------------------------------------------------------------------

/// Rewrite row 0 into a displacing row at `path` and plan it.
fn plan_displacing_row(path: &str, action: &str, class: &str) -> Result<Receipt> {
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[0];
    row["path"] = json!(path);
    row["action"] = json!(action);
    row["class"] = json!(class);
    row["source_digest"] =
        json!("sha256:5000000000000000000000000000000000000000000000000000000000000001");
    row["release_base_digest"] =
        json!("sha256:5000000000000000000000000000000000000000000000000000000000000002");
    row["expected_public_digest"] = match action {
        "drop_swarm_only" => Value::Null,
        "preserve_release" => {
            json!("sha256:5000000000000000000000000000000000000000000000000000000000000002")
        }
        _ => json!("sha256:5000000000000000000000000000000000000000000000000000000000000003"),
    };
    plan_value(&document)
}

#[test]
fn every_displacing_action_is_blocked_on_product_paths() -> Result<()> {
    // `regenerate` substitutes published bytes that do not derive from S, so it
    // hides product work exactly as dropping or preserving does. Omitting it
    // from the rule let a manifest relabel an arbitrary substitution on real
    // source as "regenerate" and plan clean.
    for action in ["drop_swarm_only", "preserve_release", "regenerate"] {
        let receipt = plan_displacing_row("crates/perl-parser/src/lexer.rs", action, "generated")?;
        assert_verdict(&receipt, Verdict::Blocked)?;
        assert_finding(&receipt, "row_product_bearing_exclusion")?;
    }
    Ok(())
}

#[test]
fn product_paths_the_source_heuristic_cannot_see_are_still_protected() -> Result<()> {
    // None of these carry a product segment or a source extension, so the
    // extension heuristic alone reports them as safe to withhold. The
    // repository classifies all of them as product or test work.
    for path in [
        "clients/sublime/LSP-perllsp/plugin.py",
        "clients/lite-xl/compose.lua",
        "vscode-extension/package.json",
        "clients/lite-xl/leaves/base/init.lua",
    ] {
        if is_product_or_test_path(path) {
            bail!("{path} is already caught by the source heuristic; pick a sharper case");
        }
        let receipt = plan_displacing_row(path, "drop_swarm_only", "governance")?;
        assert_verdict(&receipt, Verdict::Blocked)?;
        assert_finding(&receipt, "row_product_bearing_exclusion")?;
    }
    Ok(())
}

#[test]
fn a_translation_of_product_code_must_carry_a_destination_context_class() -> Result<()> {
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[0];
    row["path"] = json!("clients/lite-xl/compose.lua");
    row["class"] = json!("public_claim");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_translation_class_invalid")
}

#[test]
fn a_destination_context_translation_of_product_code_is_allowed() -> Result<()> {
    // The opposite-direction control: #6356 contemplates translating source
    // comments that ship through installers, so this must stay legal or the
    // rule would be satisfied by refusing every product path.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["path"] = json!("clients/lite-xl/compose.lua");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Pass)?;
    if !receipt.findings.is_empty() {
        bail!("a destination-context translation was rejected: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn an_unavailable_product_surface_is_not_proven() -> Result<()> {
    // Without the ledger the planner cannot tell product work from publication
    // context, so a withholding row must not pass on the source heuristic alone.
    let document = clean_value()?;
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let surface = ProductSurface { entries: Vec::new(), available: false };
    let receipt = evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &surface)?;

    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_product_bearing_unverifiable")
}

// ---------------------------------------------------------------------------
// Live-control receipts are resolved, not trusted by label
// ---------------------------------------------------------------------------

#[test]
fn a_live_receipt_that_does_not_exist_cannot_prove_a_control() -> Result<()> {
    let mut document = clean_value()?;
    document["live_controls"]["branch_rules"]["evidence"][0]["reference"] =
        json!("docs/release/9.9.9/never_written.json");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_missing")
}

#[test]
fn a_live_receipt_whose_bytes_changed_cannot_prove_a_control() -> Result<()> {
    let mut document = clean_value()?;
    document["live_controls"]["environments"]["evidence"][0]["digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_digest_mismatch")
}

#[test]
fn a_live_receipt_without_a_digest_cannot_prove_a_control() -> Result<()> {
    let mut document = clean_value()?;
    document["live_controls"]["quality_exceptions"]["evidence"][0]["digest"] = Value::Null;

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_undigested")
}

#[test]
fn unresolved_evidence_roles_may_not_carry_a_digest() -> Result<()> {
    let mut document = clean_value()?;
    document["live_controls"]["quality_exceptions"]["evidence"][1]["digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "evidence_digest_unexpected")
}

// ---------------------------------------------------------------------------
// Authority, invariants and reconciliation identity
// ---------------------------------------------------------------------------

#[test]
fn a_document_authority_that_does_not_exist_is_not_proven() -> Result<()> {
    // An authority a reviewer cannot open is not an authority.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["authority_ref"] = json!("docs/does-not-exist.md");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_authority_missing")
}

#[test]
fn an_omitted_required_invariant_is_not_proven() -> Result<()> {
    let mut document = clean_value()?;
    let invariants = document
        .get_mut("invariants")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| eyre!("fixture has no invariants"))?;
    invariants.retain(|invariant| {
        invariant.get("id").and_then(Value::as_str) != Some("governance_time_state")
    });

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "invariant_required_missing")
}

#[test]
fn an_invented_invariant_id_is_rejected_by_model_and_schema() -> Result<()> {
    // A manifest must not be able to swap a required invariant for a
    // comfortable invented one.
    let mut document = clean_value()?;
    document["invariants"][0]["id"] = json!("vibes_are_good");

    if serde_json::from_value::<Manifest>(document.clone()).is_ok() {
        bail!("an invented invariant id parsed into the typed model");
    }
    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;
    if validator.is_valid(&document) {
        bail!("an invented invariant id passed the published schema");
    }
    Ok(())
}

#[test]
fn an_invariant_cannot_cite_an_undeclared_release_input() -> Result<()> {
    let mut document = clean_value()?;
    let inputs = inputs_mut(&mut document)?;
    inputs.retain(|input| input.get("id").and_then(Value::as_str) != Some("public_claims"));

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "invariant_source_undeclared")
}

#[test]
fn a_reconciliation_receipt_from_another_version_cannot_be_consumed() -> Result<()> {
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["schema_version"] = json!(1);
    let foreign = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&foreign), &mut state);
    let (verdict, findings) = state.finish();
    if verdict != Verdict::NotProven {
        bail!("a v1 reconciliation receipt produced verdict {verdict}");
    }
    if !findings.iter().any(|f| f.code == "reconciliation_schema_version_unknown") {
        bail!("a v1 reconciliation receipt produced {findings:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The published schema is the admission boundary, not serde
// ---------------------------------------------------------------------------

#[test]
fn an_omitted_required_digest_key_is_rejected_by_the_schema() -> Result<()> {
    // `Option<String>` tolerates an absent key, so serde alone would accept a
    // row that never states whether the path exists in R. The schema requires
    // the key, and the command enforces the schema.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]
        .as_object_mut()
        .ok_or_else(|| eyre!("row 0 is not an object"))?
        .remove("release_base_digest");

    if serde_json::from_value::<Manifest>(document.clone()).is_err() {
        bail!("serde rejected the omitted key, so this control proves nothing about the schema");
    }

    let raw = serde_json::to_vec(&document)?;
    let receipt = build_receipt(&raw, Path::new("."))
        .unwrap_or_else(|failure| Receipt::unevaluated(failure.manifest_digest, failure.finding));
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "manifest_schema_violation")
}

#[test]
fn an_empty_invariant_list_is_rejected_by_the_schema() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"] = json!([]);

    if serde_json::from_value::<Manifest>(document.clone()).is_err() {
        bail!("serde rejected the empty list, so this control proves nothing about the schema");
    }

    let raw = serde_json::to_vec(&document)?;
    let receipt = build_receipt(&raw, Path::new("."))
        .unwrap_or_else(|failure| Receipt::unevaluated(failure.manifest_digest, failure.finding));
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "manifest_schema_violation")
}

#[test]
fn an_unparsable_manifest_still_produces_a_receipt() -> Result<()> {
    let receipt = build_receipt(b"{ not json", Path::new("."))
        .unwrap_or_else(|failure| Receipt::unevaluated(failure.manifest_digest, failure.finding));
    assert_verdict(&receipt, Verdict::NotProven)?;
    if receipt.manifest_digest.is_some() {
        bail!("an unparsable manifest reported a digest");
    }
    assert_finding(&receipt, "manifest_unparsable")
}

#[test]
fn the_receipt_carries_no_local_filesystem_path() -> Result<()> {
    // Identity is the canonical digest. A receipt naming the caller's path
    // would make byte-identical reproduction depend on where the file sits.
    let document = clean_value()?;
    let receipt = plan_value(&document)?;
    let rendered = serde_json::to_string(&receipt)?;
    for marker in ["fixture.json", "/home/", "/tmp/", "publication_sync_manifest.json"] {
        if rendered.contains(marker) {
            bail!("the receipt embedded the local path marker {marker}");
        }
    }
    Ok(())
}

#[test]
fn rust_build_manifests_cannot_be_displaced() -> Result<()> {
    // `Cargo.toml`/`Cargo.lock` define the product's crates and pinned
    // dependencies. Neither the source-extension heuristic (no `toml` rule) nor
    // the non-Rust ledger (which structurally excludes Rust-family files) can
    // see them, so displacing them would otherwise plan clean.
    for path in ["Cargo.toml", "Cargo.lock", "crates/perl-parser/Cargo.toml"] {
        if is_product_or_test_path(path) {
            bail!("{path} is already caught by the source heuristic; pick a sharper case");
        }
        for action in ["drop_swarm_only", "preserve_release", "regenerate"] {
            let receipt = plan_displacing_row(path, action, "generated")?;
            assert_verdict(&receipt, Verdict::Blocked)?;
            assert_finding(&receipt, "row_product_bearing_exclusion")?;
        }
    }
    Ok(())
}

#[test]
fn invariant_evidence_must_resolve() -> Result<()> {
    // A non-empty evidence array is not evidence. An invented repository
    // citation must not carry a required invariant.
    let mut document = clean_value()?;
    // Outside the fixture loader's synthesizing prefix, so this genuinely
    // resolves to nothing.
    document["invariants"][0]["evidence"][0]["reference"] =
        json!("docs/release/9.9.9/never_written.json");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "evidence_missing")
}

#[test]
fn an_invariant_review_ruling_must_be_an_issue_reference() -> Result<()> {
    let mut document = clean_value()?;
    document["invariants"][2]["evidence"][0]["reference"] = json!("we talked about it");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "evidence_ruling_unresolved")
}

#[test]
fn the_receipt_exports_every_cited_issue_reference() -> Result<()> {
    // Planning is offline and deterministic, so it cannot resolve `#NNNN`
    // against GitHub. Exporting the references hands that check to a consumer
    // that does have network access, instead of silently dropping it.
    let receipt = plan_value(&clean_value()?)?;
    if receipt.cited_issue_references != vec!["#6216", "#6355", "#6356"] {
        bail!("unexpected cited issues: {:?}", receipt.cited_issue_references);
    }

    // Row authorities and review rulings are both collected, deduplicated and
    // sorted; a repository-path authority is not an issue reference.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("#9999");
    let receipt = plan_value(&document)?;
    if !receipt.cited_issue_references.contains(&"#9999".to_string()) {
        bail!("a row authority issue was not exported: {:?}", receipt.cited_issue_references);
    }
    if receipt.cited_issue_references.iter().any(|r| r.contains('/')) {
        bail!("a repository-path authority leaked into the issue list");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Live-control receipts: typed, scoped, passing and fresh
//
// Every case below starts from a valid receipt and mutates exactly one field,
// keeping the declared digest correct. The byte-identity layer therefore always
// passes and only the semantic layer is on trial — which is the point: a digest
// proves the file did not change, never that it observed anything.
// ---------------------------------------------------------------------------

#[test]
fn a_valid_live_receipt_baseline_proves_its_control() -> Result<()> {
    // The positive control. Without it every mutation below could "pass" by
    // failing for an unrelated reason.
    let receipt = plan_with_mutated_receipt(|_| {})?;
    if !receipt.findings.is_empty() {
        bail!("the unmutated receipt produced findings: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

#[test]
fn a_receipt_that_is_not_a_live_control_observation_is_not_proven() -> Result<()> {
    // The original hole: arbitrary repository bytes, correctly hashed.
    let bytes = b"just some file that happens to live in the repository\n".to_vec();
    let digest = sha256_digest(&bytes);
    let mut document = clean_value()?;
    document["live_controls"]["branch_rules"]["evidence"][0]["reference"] =
        json!("docs/release/0.18.0/prepared_topology.json");
    document["live_controls"]["branch_rules"]["evidence"][0]["digest"] = json!(digest);

    // Hash the real fixture bytes so the digest layer genuinely passes.
    let actual = fixture_input_bytes("docs/release/0.18.0/prepared_topology.json")
        .ok_or_else(|| eyre!("fixture missing"))?;
    document["live_controls"]["branch_rules"]["evidence"][0]["digest"] =
        json!(sha256_digest(&actual));

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_unreadable")
}

#[test]
fn a_receipt_for_another_control_cannot_prove_this_one() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| r["control"] = json!("environments"))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_control_mismatch")
}

#[test]
fn a_receipt_for_another_repository_cannot_prove_this_release() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| {
        r["repository"] = json!("EffortlessMetrics/some-other-repo")
    })?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_repository_mismatch")
}

#[test]
fn a_receipt_for_another_release_cannot_prove_this_one() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| r["release"] = json!("0.17.0"))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_release_mismatch")
}

#[test]
fn a_non_passing_observation_cannot_prove_a_control() -> Result<()> {
    let blocked = plan_with_mutated_receipt(|r| r["result"] = json!("blocked"))?;
    assert_verdict(&blocked, Verdict::Blocked)?;
    assert_finding(&blocked, "live_receipt_observation_blocked")?;

    let unproven = plan_with_mutated_receipt(|r| r["result"] = json!("not_proven"))?;
    assert_verdict(&unproven, Verdict::NotProven)?;
    assert_finding(&unproven, "live_receipt_observation_not_proven")
}

#[test]
fn a_stale_observation_cannot_prove_a_control() -> Result<()> {
    // planned_at is 2026-09-02 and the horizon is 14 days, so 2026-08-01 is
    // 32 days old.
    let receipt = plan_with_mutated_receipt(|r| r["observed_at"] = json!("2026-08-01"))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_stale")
}

#[test]
fn an_observation_from_after_the_plan_cannot_prove_a_control() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| r["observed_at"] = json!("2026-09-30"))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_observed_after_plan")
}

#[test]
fn an_observation_exactly_on_the_horizon_still_proves_its_control() -> Result<()> {
    // Boundary control: 14 days before planned_at is inside a 14-day horizon.
    // Without this, an off-by-one in the comparison would go unnoticed.
    let receipt = plan_with_mutated_receipt(|r| r["observed_at"] = json!("2026-08-19"))?;
    if receipt.findings.iter().any(|f| f.code.starts_with("live_receipt")) {
        bail!("an observation exactly on the horizon was rejected: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

#[test]
fn a_receipt_from_another_schema_version_cannot_prove_a_control() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| {
        r["schema_version"] = json!("publication_live_control_receipt.v2");
    })?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_schema_version_unknown")
}

#[test]
fn a_receipt_recording_no_observed_state_is_not_proven() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| r["observed_state"] = json!({}))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_observation_empty")
}

#[test]
fn a_receipt_carrying_unknown_fields_is_not_proven() -> Result<()> {
    let receipt = plan_with_mutated_receipt(|r| r["extra_authority"] = json!("trust me"))?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "live_receipt_unreadable")
}

#[test]
fn live_receipt_evidence_cannot_be_used_for_an_invariant() -> Result<()> {
    // An invariant has no control identity to bind an observation to, so the
    // role is out of scope there rather than silently accepted.
    let mut document = clean_value()?;
    document["invariants"][0]["evidence"] = json!([
        { "kind": "live_receipt", "reference": "docs/release/0.18.0/live/branch_rules_receipt.json",
          "digest": "sha256:57af52318177ac5553cbc431557cc7fb7e410adbf401d3ae109e63208058d635" }
    ]);

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "evidence_live_receipt_out_of_scope")
}

#[test]
fn invariant_evidence_must_apply_to_its_own_declared_sources() -> Result<()> {
    // A document that exists but belongs to an input this invariant never
    // declared settled nothing here.
    let mut document = clean_value()?;
    document["invariants"][0]["evidence"][0]["reference"] =
        json!("docs/release/0.18.0/public_claims.json");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "invariant_evidence_inapplicable")
}

#[test]
fn an_expired_product_classification_is_not_proven_rather_than_ignored() -> Result<()> {
    // `file_policy` stops honouring an entry once it expires. Dropping it here
    // would quietly *weaken* a protective check, and honouring it forever would
    // assert authority nobody re-checked. Neither is honest, so fail closed.
    let surface = ProductSurface::from_expiring_entries_for_test(vec![(
        "clients/lite-xl/compose.lua",
        "production",
        Some("2026-01-01"),
    )]);
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[1];
    row["path"] = json!("clients/lite-xl/compose.lua");

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &surface)?;

    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_product_classification_expired")
}

#[test]
fn an_unexpired_classification_still_blocks() -> Result<()> {
    // Opposite direction: expiry handling must not turn every classification
    // into "unknown".
    let surface = ProductSurface::from_expiring_entries_for_test(vec![(
        "clients/lite-xl/compose.lua",
        "production",
        Some("2027-01-01"),
    )]);
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("clients/lite-xl/compose.lua");

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &surface)?;

    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn the_live_control_receipt_fixture_conforms_to_its_schema() -> Result<()> {
    let schema: Value = serde_json::from_str(LIVE_RECEIPT_SCHEMA)?;
    let document: Value = serde_json::from_slice(LIVE_RECEIPT_FIXTURE)?;
    let validator =
        jsonschema::validator_for(&schema).map_err(|error| eyre!("compiling schema: {error}"))?;
    let errors: Vec<String> = validator
        .iter_errors(&document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    if !errors.is_empty() {
        bail!("the live-control receipt fixture violates its schema: {errors:?}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// A row is subtree authority, so a directory row displaces everything under it
// ---------------------------------------------------------------------------

#[test]
fn a_displacing_row_cannot_take_a_product_subtree() -> Result<()> {
    // The exact parent-directory falsifier. `clients` and `vscode-extension`
    // match no classifier as strings — no product segment, no source
    // extension, not a Rust manifest, and the ledger holds file paths beneath
    // them, not the parents themselves — yet a row on either carries every
    // product file it contains.
    for parent in ["clients", "clients/lite-xl", "vscode-extension"] {
        if is_product_or_test_path(parent) || is_rust_build_manifest(parent) {
            bail!("{parent} is already caught as a string; pick a sharper case");
        }
        for action in ["drop_swarm_only", "preserve_release", "regenerate"] {
            let receipt = plan_displacing_row(parent, action, "governance")?;
            assert_verdict(&receipt, Verdict::Blocked)?;
            assert_finding(&receipt, "row_product_bearing_exclusion")?;
        }
    }
    Ok(())
}

#[test]
fn a_displacing_row_on_a_crate_root_is_blocked() -> Result<()> {
    // `crates/perl-parser` holds no ledger entry (Rust files are excluded from
    // the non-Rust allowlist by design), so the subtree check above cannot see
    // it. The crate manifest beneath it can.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("crates/perl-parser");
    let (root, _, _) = materialize_repo(&document)?;
    fs::create_dir_all(root.path().join("crates/perl-parser/src"))?;
    fs::write(root.path().join("crates/perl-parser/Cargo.toml"), "[package]\nname = \"x\"\n")?;

    let receipt = plan_on_disk(&document, root.path())?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn a_displacing_row_on_a_plain_directory_is_blocked() -> Result<()> {
    // No crate manifest and no ledger entry beneath it, but still a directory:
    // the projection would take every path under it sight unseen.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("some/bundle");
    let (root, _, _) = materialize_repo(&document)?;
    fs::create_dir_all(root.path().join("some/bundle"))?;
    fs::write(root.path().join("some/bundle/notes.md"), "notes\n")?;

    let receipt = plan_on_disk(&document, root.path())?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_displaces_directory")
}

#[test]
fn a_displacing_row_on_a_plain_file_is_not_a_directory_finding() -> Result<()> {
    // Opposite direction: the subtree rule must not fire on ordinary file rows,
    // or it would forbid every legitimate exclusion.
    let document = clean_value()?;
    let (root, _, _) = materialize_repo(&document)?;
    fs::create_dir_all(root.path().join(".claude"))?;
    fs::write(root.path().join(".claude/settings.json"), "{}\n")?;

    let receipt = plan_on_disk(&document, root.path())?;
    if receipt.findings.iter().any(|f| f.code == "row_displaces_directory") {
        bail!("an ordinary file row was reported as a directory: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn shipped_tooling_cannot_be_displaced_even_when_classified_tooling() -> Result<()> {
    // `install.sh` is classified `tooling`, not `production`, yet its own
    // ledger reason calls it the user-facing installer. Filtering on
    // production/test alone left both public installers droppable.
    let surface = ProductSurface::from_ledger_rows_for_test(vec![
        ("install.sh", "tooling", "release"),
        ("install.ps1", "tooling", "release"),
    ]);
    for path in ["install.sh", "install.ps1"] {
        if is_product_or_test_path(path) || is_rust_build_manifest(path) {
            bail!("{path} is already caught as a string; pick a sharper case");
        }
        let mut document = clean_value()?;
        rows_mut(&mut document)?[1]["path"] = json!(path);
        let manifest: Manifest = serde_json::from_value(document.clone())?;
        let digest = canonical_digest(&document)?;
        let receipt =
            evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &surface)?;

        assert_verdict(&receipt, Verdict::Blocked)?;
        assert_finding(&receipt, "row_product_bearing_exclusion")?;
    }
    Ok(())
}

#[test]
fn documentation_on_a_shipped_surface_stays_displaceable() -> Result<()> {
    // Opposite direction: release notes and contracts are exactly what
    // publication legitimately translates, so the widened rule must not
    // swallow documentation and config.
    let surface = ProductSurface::from_ledger_rows_for_test(vec![
        ("docs/release-notes.md", "documentation", "release"),
        ("schemas/some_contract.v1.schema.json", "config", "release"),
    ]);
    for path in ["docs/release-notes.md", "schemas/some_contract.v1.schema.json"] {
        let mut document = clean_value()?;
        rows_mut(&mut document)?[1]["path"] = json!(path);
        let manifest: Manifest = serde_json::from_value(document.clone())?;
        let digest = canonical_digest(&document)?;
        let receipt =
            evaluate_with_surface(&manifest, &digest, Path::new("."), test_loader, &surface)?;

        if receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
            bail!("{path} was treated as shipped product: {:?}", receipt.findings);
        }
    }
    Ok(())
}

#[test]
fn invariant_evidence_may_not_carry_an_unearned_digest() -> Result<()> {
    // The rule belongs to the evidence role, so it must hold under an invariant
    // exactly as it does under a live control.
    for kind_index in [0usize, 2usize] {
        let mut document = clean_value()?;
        document["invariants"][kind_index]["evidence"][0]["digest"] =
            json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");

        let receipt = plan_value(&document)?;
        assert_verdict(&receipt, Verdict::NotProven)?;
        assert_finding(&receipt, "evidence_digest_unexpected")?;
    }
    Ok(())
}

#[test]
fn the_receipt_cannot_be_written_over_a_planning_input() -> Result<()> {
    // Writing the receipt happens after validation, so an aliasing destination
    // would destroy the very inputs the verdict was computed from.
    let document = clean_value()?;
    let (root, manifest_path, _) = materialize_repo(&document)?;

    let over_manifest = plan(PlanConfig {
        manifest: manifest_path.clone(),
        repo_root: root.path().to_path_buf(),
        receipt: manifest_path.clone(),
    });
    if over_manifest.is_ok() {
        bail!("the receipt was allowed to overwrite the manifest");
    }

    // A declared release input, reached through a non-canonical path.
    let evidence = root.path().join("docs/release/0.18.0/../0.18.0/public_claims.json");
    let over_evidence = plan(PlanConfig {
        manifest: manifest_path.clone(),
        repo_root: root.path().to_path_buf(),
        receipt: evidence,
    });
    if over_evidence.is_ok() {
        bail!("the receipt was allowed to overwrite a declared release input");
    }

    // The manifest is still intact and a normal destination still works.
    let receipt_path = root.path().join("receipts/plan.json");
    plan(PlanConfig {
        manifest: manifest_path,
        repo_root: root.path().to_path_buf(),
        receipt: receipt_path.clone(),
    })?;
    if !receipt_path.exists() {
        bail!("a non-aliasing receipt destination was refused");
    }
    Ok(())
}
