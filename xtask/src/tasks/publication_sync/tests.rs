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
    if path.starts_with("docs/release/0.18.0/") {
        return Some(format!("publication-sync fixture input: {path}\n").into_bytes());
    }
    None
}

fn test_loader(_repo_root: &Path, path: &str) -> Option<Vec<u8>> {
    fixture_input_bytes(path)
}

fn clean_value() -> Result<Value> {
    serde_json::from_str(CLEAN).context("parsing the clean manifest fixture")
}

fn plan_value(document: &Value) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())
        .context("parsing the mutated fixture as publication_sync_manifest.v1")?;
    let digest = canonical_digest(document)?;
    evaluate(&manifest, &digest, Path::new("fixture.json"), Path::new("."), test_loader)
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
    assert_verdict(&receipt, Verdict::Pass)?;
    if !receipt.findings.is_empty() {
        bail!("clean manifest produced findings: {:?}", receipt.findings);
    }
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
    document["live_controls"]["environments"] = json!({ "result": "blocked", "evidence": [] });

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
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    for input in &manifest.inputs {
        let Some(bytes) = fixture_input_bytes(&input.path) else {
            continue;
        };
        let destination = root.path().join(&input.path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, bytes)?;
    }
    let manifest_path = root.path().join("publication_sync_manifest.json");
    fs::write(&manifest_path, serde_json::to_string_pretty(document)?)?;
    let receipt_path = root.path().join("receipts/publication-sync-plan.json");
    Ok((root, manifest_path, receipt_path))
}

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
