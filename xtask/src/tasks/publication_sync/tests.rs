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
    // The fixture's own projection rows. A real plan runs in a checkout of the
    // prepared swarm tree, so a displacing row resolves to a regular file
    // there; the harness has to model that or every row is unresolvable.
    if FIXTURE_ROW_PATHS.contains(&path) {
        return Some(format!("publication-sync fixture row: {path}\n").into_bytes());
    }
    // Real repository files that tests point rows at. Every row's source must
    // resolve, and a real plan runs in a checkout of `S` where these exist, so
    // the harness has to model that or a product-protection test would fail for
    // absence rather than for the rule it is actually about.
    if FIXTURE_PRESENT_PATHS.contains(&path) {
        return Some(format!("publication-sync checkout file: {path}\n").into_bytes());
    }
    None
}

/// Paths a checkout of the prepared swarm tree contains as regular files,
/// beyond the fixture's own rows.
const FIXTURE_PRESENT_PATHS: [&str; 18] = [
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/config.toml",
    "vendored/crate/.cargo/config.toml",
    "deploy/config.toml",
    "crates/perl-parser/Cargo.toml",
    "crates/perl-parser/src/lexer.rs",
    "crates/perl-parser/src/lib.rs",
    "clients/lite-xl/compose.lua",
    "clients/lite-xl/leaves/base/init.lua",
    "clients/sublime/LSP-perllsp/plugin.py",
    "vscode-extension/package.json",
    "install.sh",
    "install.ps1",
    "tests/publication_projection.rs",
    "docs/release-notes.md",
    "schemas/some_contract.v1.schema.json",
    "docs/policy/NON_RUST_INVENTORY.md.bak",
];

/// Paths that exist in that checkout as directories. The loader reports these
/// the way the real one does, so subtree rules can be exercised without
/// materializing a tree.
const FIXTURE_DIRECTORY_PATHS: [&str; 8] = [
    "clients",
    "clients/lite-xl",
    "clients/sublime",
    "vscode-extension",
    "crates/perl-parser",
    "docs/policy",
    "some/bundle",
    "target/receipts",
];

/// Row paths the clean fixture projects, materialized so the planner can
/// establish that each names a regular file rather than a subtree.
const FIXTURE_ROW_PATHS: [&str; 4] = [
    "README.md",
    ".claude/settings.json",
    "RELEASE_HISTORY.md",
    "docs/policy/NON_RUST_INVENTORY.md",
];

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
        fixture_checkout,
        fixture_tree,
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
    if FIXTURE_DIRECTORY_PATHS.contains(&path) {
        return Err(LoadFailure::Unreadable(NOT_A_REGULAR_FILE.to_string()));
    }
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
    evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &test_product_surface(),
        fixture_checkout,
        fixture_tree,
    )
}

/// Evaluate against a materialized repository root, using the real on-disk
/// loader so filesystem-shaped rules (directory rows, crate roots) are exercised.
fn plan_on_disk(document: &Value, root: &Path) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(document)?;
    evaluate_with_surface(
        &manifest,
        &digest,
        root,
        load_input,
        &test_product_surface(),
        fixture_checkout,
        fixture_tree,
    )
}

/// Drive the full `evaluate` entry point, including the checkout binding.
fn plan_with_checkout(document: &Value, checkout: CheckoutResolver) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(document)?;
    evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &test_product_surface(),
        checkout,
        fixture_tree,
    )
}

/// Evaluate the clean fixture against a caller-supplied product surface.
fn plan_with_surface(document: &Value, surface: &ProductSurface) -> Result<Receipt> {
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(document)?;
    evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        surface,
        fixture_checkout,
        fixture_tree,
    )
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

    // Resolve the checkout to the mutated commit, so the checkout binding is
    // satisfied and only reconciliation staleness is on trial. Without this the
    // dominating `checkout_not_prepared_swarm` finding would mask the rule.
    fn mutated_checkout(_repo_root: &Path) -> Option<CheckoutFacts> {
        Some(clean_checkout("9999999999999999999999999999999999999999"))
    }

    let receipt = plan_with_checkout(&document, mutated_checkout)?;
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
fn a_minimal_document_is_not_a_reconciliation_receipt() -> Result<()> {
    // A declared digest binds the bytes the manifest points at; it does not
    // establish that those bytes came from `sync-divergence`. A manifest author
    // who forges the receipt declares the forgery's digest too, so the byte
    // layer cannot be what refuses this.
    //
    // Reading only `schema_version`, `verdict` and two commits admitted the
    // four-field document below, which no `sync-divergence` run can emit: the
    // producer always writes three subjects with fixed roles, a ledger, a
    // population digest and six commit arrays. This is the same hole the
    // live-control contract closed one round earlier — there the fix was a
    // typed `publication_live_control_receipt.v1` naming its own subject.
    let forged = serde_json::to_vec(&json!({
        "schema_version": 2,
        "verdict": "pass",
        "subjects": {
            "source": {"commit": "1111111111111111111111111111111111111111"},
            "target": {"commit": "2222222222222222222222222222222222222222"}
        }
    }))?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&forged), &mut state);
    let (verdict, findings) = state.finish();
    if verdict == Verdict::Pass {
        bail!("a four-field document authorized a plan: {findings:?}");
    }
    if !findings.iter().any(|finding| finding.code == "reconciliation_unreadable") {
        bail!("a forged minimal receipt produced {findings:?}");
    }
    Ok(())
}

#[test]
fn every_identifying_field_is_independently_required() -> Result<()> {
    // The four-field control above is refused by *any one* of the added
    // requirements, so on its own it cannot show that each is load-bearing —
    // dropping two of the three would leave it passing. Delete one field at a
    // time from the real fixture instead, so each requirement has its own
    // falsifier.
    //
    // `boundary` earns its place separately: `sync_divergence` resolves three
    // subjects and the planner reads only two, so nothing else in this module
    // would notice its absence.
    for pointer in ["subjects", "ledger", "population_digest"] {
        let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
        if receipt_value.get(pointer).is_none() {
            bail!("fixture has no {pointer}; this control is mis-aimed");
        }
        receipt_value
            .as_object_mut()
            .ok_or_else(|| eyre!("reconciliation fixture is not an object"))?
            .remove(pointer);
        let stripped = serde_json::to_vec(&receipt_value)?;

        let manifest: Manifest = serde_json::from_str(CLEAN)?;
        let mut state = PlanState::default();
        validate_reconciliation(&manifest, Some(&stripped), &mut state);
        let (verdict, findings) = state.finish();
        if verdict == Verdict::Pass {
            bail!("a receipt with no {pointer} authorized a plan: {findings:?}");
        }
        if !findings.iter().any(|finding| finding.code == "reconciliation_unreadable") {
            bail!("a receipt with no {pointer} produced {findings:?}");
        }
    }

    // And the third subject specifically, with the other two left intact.
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["subjects"]
        .as_object_mut()
        .ok_or_else(|| eyre!("reconciliation fixture subjects is not an object"))?
        .remove("boundary");
    let stripped = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&stripped), &mut state);
    let (verdict, findings) = state.finish();
    if verdict == Verdict::Pass {
        bail!("a receipt with no boundary subject authorized a plan: {findings:?}");
    }
    if !findings.iter().any(|finding| finding.code == "reconciliation_unreadable") {
        bail!("a receipt with no boundary subject produced {findings:?}");
    }
    Ok(())
}

#[test]
fn a_reconciliation_receipt_must_carry_the_producers_subject_roles() -> Result<()> {
    // Structure alone is not provenance. A document can carry every field the
    // producer emits and still describe different subjects, so the three roles
    // are pinned to the constants `sync_divergence` writes. Only the role is
    // changed here; every other field stays exactly as the real fixture has it,
    // so nothing else can be what refuses it.
    for (subject, role) in [("source", "patch_equivalence_upstream"), ("target", "release_head")] {
        let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
        if receipt_value["subjects"][subject]["role"] != json!(role) {
            bail!("fixture role for {subject} is not {role}; this control is mis-aimed");
        }
        receipt_value["subjects"][subject]["role"] = json!("history_limit");
        let mislabelled = serde_json::to_vec(&receipt_value)?;

        let manifest: Manifest = serde_json::from_str(CLEAN)?;
        let mut state = PlanState::default();
        validate_reconciliation(&manifest, Some(&mislabelled), &mut state);
        let (verdict, findings) = state.finish();
        if verdict == Verdict::Pass {
            bail!("a receipt whose {subject} role was rewritten authorized a plan: {findings:?}");
        }
        if !findings.iter().any(|finding| finding.code == "reconciliation_subject_role_invalid") {
            bail!("a rewritten {subject} role produced {findings:?}");
        }
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
        materialized.insert(row.path.clone());
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

/// A checkout resolver that answers with the fixture's declared prepared swarm
/// commit, so the end-to-end tests exercise the planner rather than git.
/// What the prepared swarm commit contains, modelled consistently with
/// `test_loader`. The tree and the worktree agree in the fixture; tests that
/// need them to disagree — a sparse checkout — inject their own probe.
fn fixture_tree(_repo_root: &Path, _commit: &str, path: &str) -> Option<bool> {
    Some(
        FIXTURE_ROW_PATHS.contains(&path)
            || FIXTURE_PRESENT_PATHS.contains(&path)
            || FIXTURE_DIRECTORY_PATHS.contains(&path)
            || fixture_input_bytes(path).is_some(),
    )
}

fn fixture_checkout(_repo_root: &Path) -> Option<CheckoutFacts> {
    Some(clean_checkout("1111111111111111111111111111111111111111"))
}

/// A checkout at `head` whose worktree agrees with that commit everywhere.
fn clean_checkout(head: &str) -> CheckoutFacts {
    CheckoutFacts { head: head.to_string(), dirty: BTreeSet::new() }
}

/// A checkout at the prepared swarm commit whose worktree disagrees with it at
/// `dirty`.
fn checkout_dirty_at(dirty: &[&str]) -> CheckoutFacts {
    CheckoutFacts {
        head: "1111111111111111111111111111111111111111".to_string(),
        dirty: dirty.iter().map(|path| (*path).to_string()).collect(),
    }
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

    plan_with(
        PlanConfig {
            manifest: manifest.clone(),
            repo_root: root.path().to_path_buf(),
            receipt: receipt.clone(),
        },
        fixture_checkout,
        fixture_tree,
    )?;

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

    let outcome = plan_with(
        PlanConfig { manifest, repo_root: root.path().to_path_buf(), receipt: receipt.clone() },
        fixture_checkout,
        fixture_tree,
    );
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
fn a_release_lineage_translation_is_reported_as_displacing() -> Result<()> {
    // `translate` is excluded from the action's own displacement set, but
    // `release_lineage` says the content's lineage is release-owned, so the row
    // substitutes release content whatever the action is. Validation already
    // read the combination that way; the receipt recorded only the action half,
    // so a consumer reading the receipt saw `false` for a row the planner was
    // protecting as displacing.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["class"] = json!("release_lineage");

    // The contract really does admit the combination — otherwise the honest
    // repair would be to reject the row rather than to report it accurately.
    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator = jsonschema::validator_for(&schema)?;
    if !validator.is_valid(&document) {
        bail!("the published schema rejects a release_lineage translation");
    }

    // `README.md` is not product-bearing, so nothing else has an opinion here
    // and the receipt field is on trial alone.
    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Pass)?;

    // The serialized form, because that is what a consumer reads.
    let written = serde_json::to_value(&receipt)?;
    let row = written["rows"]
        .as_array()
        .and_then(|rows| {
            rows.iter().find(|row| row.get("path").and_then(Value::as_str) == Some("README.md"))
        })
        .ok_or_else(|| eyre!("the receipt carried no row for README.md: {written}"))?;
    if row.get("displaces_swarm_content").and_then(Value::as_bool) != Some(true) {
        bail!("a release_lineage translation was reported as not displacing: {row}");
    }
    Ok(())
}

#[test]
fn a_release_lineage_translation_of_product_code_is_excluded() -> Result<()> {
    // The validation half of the same rule, on a path where being displacing
    // has a consequence. `row_product_bearing_exclusion` is reachable only
    // through the displacement predicate, so asserting that exact code isolates
    // it from `row_product_translation_class_invalid`, which this row also
    // trips for an unrelated reason.
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[0];
    row["path"] = json!("clients/lite-xl/compose.lua");
    row["class"] = json!("release_lineage");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn an_unavailable_product_surface_is_not_proven() -> Result<()> {
    // Without the ledger the planner cannot tell product work from publication
    // context, so a withholding row must not pass on the source heuristic alone.
    let document = clean_value()?;
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let surface = ProductSurface { entries: Vec::new(), available: false };
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

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
    let receipt = build_receipt(&raw, Path::new("."), fixture_checkout, fixture_tree)
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
    let receipt = build_receipt(&raw, Path::new("."), fixture_checkout, fixture_tree)
        .unwrap_or_else(|failure| Receipt::unevaluated(failure.manifest_digest, failure.finding));
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "manifest_schema_violation")
}

#[test]
fn an_unparsable_manifest_still_produces_a_receipt() -> Result<()> {
    let receipt = build_receipt(b"{ not json", Path::new("."), fixture_checkout, fixture_tree)
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
fn cargo_configuration_cannot_be_displaced() -> Result<()> {
    // `.cargo/config.toml` carries `[build] rustflags`, `[target.*]` linker
    // selection and `[source]`/`[registries]` replacement, so displacing it
    // changes what is built exactly as displacing `Cargo.toml` does. It hid
    // from all three classifiers for three different reasons, so this control
    // asserts the two cheap ones really are blind to it before trusting the
    // rule it actually names.
    for path in [".cargo/config.toml", "vendored/crate/.cargo/config.toml"] {
        if is_product_or_test_path(path) {
            bail!("{path} is already caught by the source heuristic; pick a sharper case");
        }

        // Unlike the rest of the Rust family, this path IS in the non-Rust
        // ledger — and the ledger declines to protect it. `entry_is_protected`
        // keeps a `config` entry only on a shipped surface, and the real
        // `.cargo/**` entry declares `surface = "tooling"`, so the entry is
        // filtered out before `classify` consults it. Reproduce that exact row
        // shape and check the filter directly rather than through `classify`,
        // which now answers from the rule under test before it ever reaches the
        // ledger. If the ledger were what blocked this row, that rule would be
        // untested and this control would prove nothing.
        let ledger = ProductSurface::from_ledger_rows_for_test(vec![(path, "config", "tooling")]);
        if !ledger.entries.is_empty() {
            bail!("the ledger already protects {path}; this control proves nothing");
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
fn only_the_configuration_cargo_actually_reads_is_protected() -> Result<()> {
    // The opposite direction, pinning both halves of the match. Widening it to
    // the whole `.cargo/**` subtree the ledger entry covers would make the
    // first two undroppable; dropping the parent-directory guard and matching
    // the file name alone would protect the third, and `config.toml` is far too
    // common a name for that. Either is the over-protection failure the `config`
    // carve-out already had to repair once.
    for path in [".cargo/mutants.toml", ".cargo/config.local.toml.example", "deploy/config.toml"] {
        let receipt = plan_displacing_row(path, "drop_swarm_only", "generated")?;
        if receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
            bail!("{path} was protected as cargo build configuration: {:?}", receipt.findings);
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
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

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
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

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
        let receipt = evaluate_with_surface(
            &manifest,
            &digest,
            Path::new("."),
            test_loader,
            &surface,
            fixture_checkout,
            fixture_tree,
        )?;

        assert_verdict(&receipt, Verdict::Blocked)?;
        assert_finding(&receipt, "row_product_bearing_exclusion")?;
    }
    Ok(())
}

#[test]
fn documentation_on_a_shipped_surface_stays_displaceable() -> Result<()> {
    // Opposite direction: release notes are exactly what publication
    // legitimately translates, so the rule must not swallow prose.
    //
    // `config` used to be asserted displaceable here too. That was wrong and is
    // corrected in `a_shipped_configuration_file_cannot_be_displaced` — see the
    // reasoning there. Prose is translatable; functional configuration is not.
    let surface = ProductSurface::from_ledger_rows_for_test(vec![(
        "docs/release-notes.md",
        "documentation",
        "release",
    )]);
    for path in ["docs/release-notes.md"] {
        let mut document = clean_value()?;
        rows_mut(&mut document)?[1]["path"] = json!(path);
        let manifest: Manifest = serde_json::from_value(document.clone())?;
        let digest = canonical_digest(&document)?;
        let receipt = evaluate_with_surface(
            &manifest,
            &digest,
            Path::new("."),
            test_loader,
            &surface,
            fixture_checkout,
            fixture_tree,
        )?;

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

    let over_manifest = plan_with(
        PlanConfig {
            manifest: manifest_path.clone(),
            repo_root: root.path().to_path_buf(),
            receipt: manifest_path.clone(),
        },
        fixture_checkout,
        fixture_tree,
    );
    if over_manifest.is_ok() {
        bail!("the receipt was allowed to overwrite the manifest");
    }

    // A declared release input, reached through a non-canonical path.
    let evidence = root.path().join("docs/release/0.18.0/../0.18.0/public_claims.json");
    let over_evidence = plan_with(
        PlanConfig {
            manifest: manifest_path.clone(),
            repo_root: root.path().to_path_buf(),
            receipt: evidence,
        },
        fixture_checkout,
        fixture_tree,
    );
    if over_evidence.is_ok() {
        bail!("the receipt was allowed to overwrite a declared release input");
    }

    // The manifest is still intact and a normal destination still works.
    let receipt_path = root.path().join("receipts/plan.json");
    plan_with(
        PlanConfig {
            manifest: manifest_path,
            repo_root: root.path().to_path_buf(),
            receipt: receipt_path.clone(),
        },
        fixture_checkout,
        fixture_tree,
    )?;
    if !receipt_path.exists() {
        bail!("a non-aliasing receipt destination was refused");
    }
    Ok(())
}

#[test]
fn an_expired_descendant_classification_is_not_proven() -> Result<()> {
    // The subtree check must age its evidence exactly as the exact-match check
    // does. Returning ProductOrTest on a descendant without consulting expiry
    // reported a confident `blocked` from a classification nobody re-checked.
    let surface = ProductSurface::from_expiring_entries_for_test(vec![(
        "clients/lite-xl/compose.lua",
        "production",
        Some("2026-01-01"),
    )]);
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("clients");

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_product_classification_expired")
}

#[test]
fn an_unexpired_descendant_still_blocks_the_parent() -> Result<()> {
    // Opposite direction: aging the subtree check must not stop it protecting
    // parents whose descendants are still current.
    let surface = ProductSurface::from_expiring_entries_for_test(vec![(
        "clients/lite-xl/compose.lua",
        "production",
        Some("2027-01-01"),
    )]);
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("clients");

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn a_displacing_row_absent_from_the_checkout_is_not_proven() -> Result<()> {
    // Row digests are declarative and the planner resolves neither S nor R, so
    // a path missing here could be a file or an entire subtree in the declared
    // trees. The directory guard only sees materialized paths, so absence has
    // to fail closed rather than fall through it.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("not/in/this/checkout.json");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_source_absent")
}

#[test]
fn a_translating_row_absent_from_the_checkout_is_not_proven() -> Result<()> {
    // A translation rewrites bytes that exist in S for the destination context.
    // If the path exists nowhere, the row asserts a translation of nothing, and
    // a well-formed `source_digest` is not evidence that the content is there.
    //
    // An earlier version of this test asserted the opposite — that a
    // translating row need not resolve — because it was written to protect a
    // narrower property: the *subtree* rule must not fire on translations.
    // That property is still worth keeping and is checked below; it does not
    // license skipping existence.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["path"] = json!("not/in/this/checkout.md");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_source_absent")?;

    // The narrower property the old test was really defending: an absent
    // translation is not accused of displacing a directory.
    if receipt.findings.iter().any(|finding| finding.code == "row_displaces_directory") {
        bail!("a translating row was accused of displacing a directory: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn a_translating_row_on_a_directory_is_blocked() -> Result<()> {
    // A row declares one `source_digest`, which cannot describe a subtree, so a
    // directory is incoherent for a translation as much as for a displacement —
    // but it earns its own finding rather than the displacement one.
    // `some/bundle` rather than `docs/policy`: the latter is a path prefix of
    // row 3, so it would also raise the ambiguity finding and the verdict would
    // no longer be about this rule.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["path"] = json!("some/bundle");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_source_not_a_file")?;
    if receipt.findings.iter().any(|finding| finding.code == "row_displaces_directory") {
        bail!("a translating row was accused of displacing: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn a_translating_row_on_a_present_file_still_passes() -> Result<()> {
    // Opposite direction. The clean fixture's row 0 is a translation of a
    // materialized `README.md`; requiring existence must not reject it.
    let receipt = plan_value(&clean_value()?)?;
    if receipt.findings.iter().any(|finding| finding.code.starts_with("row_source_")) {
        bail!("a resolvable translation was refused: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

#[test]
fn a_root_level_document_can_be_a_row_authority() -> Result<()> {
    // `README.md` is a legitimate root-level authority; requiring a `/`
    // rejected it while the schema accepted it, so a valid manifest could not
    // pass the planner.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("README.md");

    let receipt = plan_value(&document)?;
    if receipt.findings.iter().any(|f| f.code.starts_with("row_authority")) {
        bail!("a root-level document authority was rejected: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn a_root_level_authority_that_does_not_exist_is_still_resolved() -> Result<()> {
    // Accepting root-level documents must not stop resolving them.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("NOT_THERE.md");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_authority_missing")
}

#[test]
fn prose_in_the_authority_field_is_still_unresolved() -> Result<()> {
    // Opposite direction: the extension discriminator must not turn a typed
    // sentence into a "document" whose absence reads as a missing file.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("because we always did it this way");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_authority_unresolved")
}

#[test]
fn a_checkout_that_is_not_the_prepared_swarm_commit_is_not_proven() -> Result<()> {
    // The product surface and every path shape come from this checkout. They
    // are only evidence about the projection if the checkout is S.
    fn other_commit(_root: &Path) -> Option<CheckoutFacts> {
        Some(clean_checkout("9999999999999999999999999999999999999999"))
    }
    let receipt = plan_with_checkout(&clean_value()?, other_commit)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "checkout_not_prepared_swarm")
}

#[test]
fn an_unresolvable_checkout_is_not_proven() -> Result<()> {
    fn unresolvable(_root: &Path) -> Option<CheckoutFacts> {
        None
    }
    let receipt = plan_with_checkout(&clean_value()?, unresolvable)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "checkout_unresolvable")
}

#[test]
fn the_prepared_swarm_checkout_plans_pass() -> Result<()> {
    // Opposite direction: the binding must not reject the correct checkout.
    let receipt = plan_with_checkout(&clean_value()?, fixture_checkout)?;
    if !receipt.findings.is_empty() {
        bail!("the prepared swarm checkout produced findings: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

#[test]
fn a_receipt_is_published_atomically() -> Result<()> {
    // A direct write truncates the destination in place, so a process that dies
    // partway leaves a half-receipt a consumer cannot distinguish from a real
    // one. Staging and renaming means the destination goes from the old bytes to
    // the complete new bytes with nothing observable in between.
    //
    // Content alone does not discriminate: `fs::write` also ends with valid
    // JSON at the path. The observable that separates the two is the inode —
    // a rename publishes a *new* file over the name, an in-place write keeps
    // the old one. So that is what this asserts.
    let document = clean_value()?;
    let (root, _, receipt_path) = materialize_repo(&document)?;
    fs::create_dir_all(root.path().join("receipts"))?;
    fs::write(&receipt_path, "{ truncated")?;
    let stale_identity = file_identity(&receipt_path)?;

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        root.path(),
        load_input,
        &test_product_surface(),
        fixture_checkout,
        fixture_tree,
    )?;
    write_receipt(&receipt_path, &receipt)?;

    let written: Value = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("the published receipt is not valid JSON")?;
    if written.get("verdict").is_none() {
        bail!("the published receipt has no verdict");
    }
    if file_identity(&receipt_path)? == stale_identity {
        bail!("the receipt reused the stale file, so it was written in place rather than renamed");
    }

    // The staging file must not survive as a second artifact in the directory.
    let published =
        receipt_path.file_name().ok_or_else(|| eyre!("receipt path has no file name"))?.to_owned();
    let directory = receipt_path.parent().ok_or_else(|| eyre!("receipt path has no parent"))?;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() != published {
            bail!("staging left {} behind next to the receipt", entry.path().display());
        }
    }

    // A temporary file is created 0600; the direct write this replaced produced
    // a normal readable artifact, and other tooling consumes the receipt.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(&receipt_path)?.permissions().mode() & 0o777;
        if mode != 0o644 {
            bail!("the published receipt is mode {mode:o}, not 644");
        }
    }
    Ok(())
}

/// The identity of the file currently at `path`, so a rename-based publish can
/// be told apart from an in-place rewrite.
#[cfg(unix)]
fn file_identity(path: &Path) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_identity(path: &Path) -> Result<Vec<u8>> {
    Ok(fs::read(path)?)
}

// ---------------------------------------------------------------------------
// Consumed-path coverage and worktree integrity

/// The alias guard and the worktree-integrity check must both see every path
/// evaluation can read. An earlier alias guard carried its own copy of the
/// authority rule and fell behind when that rule widened, so this pins the
/// three kinds of path that copy was missing.
#[test]
fn the_consumed_path_set_covers_every_path_evaluation_reads() -> Result<()> {
    let mut document = clean_value()?;
    rows_mut(&mut document)?[0]["authority_ref"] = json!("NOTICE.md");
    let manifest: Manifest = serde_json::from_value(document)?;
    let consumed = consumed_repository_paths(&manifest);

    for expected in [
        // The ledger the product surface is classified against.
        "policy/non-rust-allowlist.toml",
        // A declared release input.
        "docs/release/0.18.0/public_claims.json",
        // A row path — probed for shape when the row displaces.
        "README.md",
        // The crate-root probe `validate_rows` performs on that row path.
        "README.md/Cargo.toml",
        // A root-level authority document. The guard's old `contains('/')`
        // copy of the authority rule excluded exactly this.
        "NOTICE.md",
    ] {
        if !consumed.contains(expected) {
            bail!("consumed path set is missing {expected}: {consumed:?}");
        }
    }
    Ok(())
}

#[test]
fn the_receipt_cannot_be_written_over_a_row_path() -> Result<()> {
    // `validate_rows` probes each displacing row path, so a receipt written
    // there destroys a file the verdict was computed from.
    let document = clean_value()?;
    let (root, manifest_path, _) = materialize_repo(&document)?;

    let over_row = plan_with(
        PlanConfig {
            manifest: manifest_path,
            repo_root: root.path().to_path_buf(),
            receipt: root.path().join("README.md"),
        },
        fixture_checkout,
        fixture_tree,
    );
    if over_row.is_ok() {
        bail!("the receipt was allowed to overwrite a declared row path");
    }
    Ok(())
}

#[test]
fn a_consumed_path_that_differs_from_the_checkout_is_not_proven() -> Result<()> {
    // HEAD matching is not enough: the planner reads the worktree, so a
    // modified declared input is read as if it were part of S.
    fn dirty_input(_root: &Path) -> Option<CheckoutFacts> {
        Some(checkout_dirty_at(&["docs/release/0.18.0/public_claims.json"]))
    }

    let receipt = plan_with_checkout(&clean_value()?, dirty_input)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "consumed_path_not_at_checkout")
}

#[test]
fn a_dirty_descendant_of_a_row_directory_is_not_proven() -> Result<()> {
    // A row takes its whole subtree, so a changed descendant changes what the
    // row would carry even though the row path itself is unchanged.
    let mut document = clean_value()?;
    rows_mut(&mut document)?[3]["path"] = json!("docs/policy");

    // Assert the cheaper rule does not already catch this: the dirty path is
    // not itself a consumed path, so only the subtree rule can find it.
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let descendant = "docs/policy/UNRELATED_NOTE.md";
    if consumed_repository_paths(&manifest).contains(descendant) {
        bail!("{descendant} is directly consumed, so this control proves nothing about subtrees");
    }

    fn dirty_descendant(_root: &Path) -> Option<CheckoutFacts> {
        Some(checkout_dirty_at(&["docs/policy/UNRELATED_NOTE.md"]))
    }

    let receipt = plan_with_checkout(&document, dirty_descendant)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "consumed_path_not_at_checkout")
}

#[test]
fn a_dirty_path_the_plan_never_reads_does_not_block() -> Result<()> {
    // Opposite direction. A working tree is nearly always dirty somewhere —
    // the candidate manifest itself is normally uncommitted. Refusing on any
    // dirt at all would make the command unusable and the rule vacuous.
    fn dirty_elsewhere(_root: &Path) -> Option<CheckoutFacts> {
        Some(checkout_dirty_at(&["candidate/manifest.json", "target/scratch.txt"]))
    }

    let receipt = plan_with_checkout(&clean_value()?, dirty_elsewhere)?;
    if !receipt.findings.is_empty() {
        bail!("unrelated worktree changes produced findings: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

/// `git status --porcelain -z` shapes the planner has to read correctly.
///
/// Records are NUL-terminated and unquoted. The rename case is the one needing
/// lookahead: `-z` puts the origin in the following field with no status of its
/// own, so a parser written for the newline form would read it as a record and
/// lose both paths.
#[test]
fn status_parsing_recovers_modified_untracked_ignored_and_renamed_paths() -> Result<()> {
    let parsed = parse_status_paths(concat!(
        " M docs/how-to/PUBLICATION_SYNC.md\0",
        "?? fixtures/publication_sync/extra.json\0",
        "!! target/\0",
        "R  schemas/new.json\0schemas/old.json\0",
        // `-z` does not quote or escape, so a path with a tab and a space
        // arrives raw. The newline form would deliver this as
        // `"docs/we ird\tname.md"` with a literal backslash-t, matching no
        // declared path.
        "?? docs/we ird\tname.md\0",
    ));
    for expected in [
        "docs/how-to/PUBLICATION_SYNC.md",
        "fixtures/publication_sync/extra.json",
        "target/",
        "schemas/new.json",
        "schemas/old.json",
        "docs/we ird\tname.md",
    ] {
        if !parsed.contains(expected) {
            bail!("status parsing lost {expected:?}: {parsed:?}");
        }
    }
    // The rename origin must not also have been read as a record, which would
    // have produced a bogus path from its first three characters.
    if parsed.len() != 6 {
        bail!("status parsing produced unexpected entries: {parsed:?}");
    }
    Ok(())
}

#[test]
fn a_consumed_path_inside_an_ignored_directory_is_not_proven() -> Result<()> {
    // `--ignored=traditional` collapses a wholly ignored directory to one
    // entry, so the integrity check has to test ancestors as well as the path
    // itself. Without that, an ignored release input reads and hashes happily
    // while the receipt claims it came from the prepared commit.
    let mut document = clean_value()?;
    inputs_mut(&mut document)?[0]["path"] = json!("target/receipts/public_claims.json");

    // Assert the cheaper tests do not already catch this: the dirty entry is
    // neither the consumed path nor a descendant of it.
    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let consumed = consumed_repository_paths(&manifest);
    if consumed.contains("target/") {
        bail!("`target/` is itself consumed, so this control proves nothing about ancestors");
    }
    if consumed.iter().any(|path| path.starts_with("target/receipts/public_claims.json/")) {
        bail!("a descendant match would already catch this case");
    }

    fn ignored_directory(_root: &Path) -> Option<CheckoutFacts> {
        Some(checkout_dirty_at(&["target/"]))
    }

    let receipt = plan_with_checkout(&document, ignored_directory)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "consumed_path_not_at_checkout")
}

#[test]
fn the_receipt_cannot_be_written_through_a_symlink_to_an_input() -> Result<()> {
    // Comparing a canonical parent plus the raw file name compares a symlink by
    // its own name rather than by what it points at, so a destination linked to
    // a planning input would slip past the guard and be written through.
    let document = clean_value()?;
    let (root, manifest_path, _) = materialize_repo(&document)?;

    let link = root.path().join("receipts-plan.json");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&manifest_path, &link)?;
    #[cfg(not(unix))]
    return Ok(());

    #[cfg(unix)]
    {
        let through_link = plan_with(
            PlanConfig {
                manifest: manifest_path,
                repo_root: root.path().to_path_buf(),
                receipt: link,
            },
            fixture_checkout,
            fixture_tree,
        );
        if through_link.is_ok() {
            bail!("the receipt was allowed to be written through a symlink to the manifest");
        }
        Ok(())
    }
}

#[test]
fn a_receipt_destination_under_a_missing_directory_is_still_guarded() -> Result<()> {
    // `write_receipt` creates the destination directory, so a receipt path
    // whose parents do not exist yet is the ordinary case. Resolving only the
    // immediate parent failed there and skipped the whole alias check, which
    // made an unresolved parent the way around the guard.
    let document = clean_value()?;
    let (root, manifest_path, _) = materialize_repo(&document)?;

    // Deep, entirely absent destination: the guard must still run and still
    // allow this legitimate write.
    let fresh = root.path().join("a/b/c/plan.json");
    plan_with(
        PlanConfig {
            manifest: manifest_path.clone(),
            repo_root: root.path().to_path_buf(),
            receipt: fresh.clone(),
        },
        fixture_checkout,
        fixture_tree,
    )?;
    if !fresh.is_file() {
        bail!("the receipt was not written to a destination under missing directories");
    }

    // And the guard is genuinely live on such a path: aim it at the manifest
    // by way of a non-existent intermediate directory.
    let aliased = root
        .path()
        .join("a/b/../../")
        .join(manifest_path.file_name().ok_or_else(|| eyre!("manifest has no file name"))?);
    let over_manifest = plan_with(
        PlanConfig {
            manifest: manifest_path,
            repo_root: root.path().to_path_buf(),
            receipt: aliased,
        },
        fixture_checkout,
        fixture_tree,
    );
    if over_manifest.is_ok() {
        bail!("the receipt was allowed to overwrite the manifest through a traversing path");
    }
    Ok(())
}

/// `entry_governs` is a deliberate local copy of the ledger match rule, kept out
/// of `file_policy` so this claim does not churn a whole-tree policy surface.
/// A duplicated rule is a drift hazard, so pin the equivalence rather than
/// trusting that two hand-written copies stay in step: if either side learns
/// about glob parse errors, trailing slashes, or `retired` and the other does
/// not, this fails.
#[test]
fn the_local_ledger_matcher_agrees_with_the_shared_one() -> Result<()> {
    fn entry(path: Option<&str>, glob: Option<&str>) -> AllowEntry {
        AllowEntry {
            id: "differential".to_string(),
            glob: glob.map(str::to_string),
            path: path.map(str::to_string),
            kind: String::new(),
            language: String::new(),
            surface: String::new(),
            classification: "production".to_string(),
            owner: String::new(),
            reason: String::new(),
            covered_by: Vec::new(),
            created: String::new(),
            review_after: String::new(),
            expires: None,
            broad_glob_reason: None,
            retired: false,
        }
    }

    let entries = [
        entry(Some("install.sh"), None),
        entry(Some("clients/sublime/LSP-perllsp/plugin.py"), None),
        entry(None, Some("fixtures/publication_sync/*.json")),
        entry(None, Some("crates/**/*.rs")),
        // Neither field: the ledger shape forbids it, but both matchers have to
        // agree on what they do with it anyway.
        entry(None, None),
        // An unparsable glob. Both sides must decline rather than panic.
        entry(None, Some("[unterminated")),
        // Both fields set. The ledger shape forbids this (`glob` and `path` are
        // mutually exclusive) and `unused_entry_count` filters it out with an
        // xor, so neither matcher is reached with it today. Pinning it anyway
        // keeps the precedence explicit: if one side ever stops preferring
        // `path`, that is a divergence rather than a harmless difference.
        entry(Some("install.sh"), Some("crates/**/*.rs")),
    ];

    let candidates = [
        "install.sh",
        "install.ps1",
        "clients/sublime/LSP-perllsp/plugin.py",
        "clients/sublime/LSP-perllsp/",
        "fixtures/publication_sync/clean_manifest.json",
        "fixtures/publication_sync/nested/deep.json",
        "crates/perl-parser/src/lexer.rs",
        "crates/perl-parser/src",
        "README.md",
        "",
    ];

    let mut agreements = 0usize;
    let mut positives = 0usize;
    for entry in &entries {
        for candidate in candidates {
            let local = entry_governs(entry, candidate);
            let shared =
                file_policy::entry_matches_any_tracked_file(entry, &[candidate.to_string()]);
            if local != shared {
                bail!(
                    "matchers disagree on entry {:?}/{:?} for {candidate:?}: \
                     entry_governs={local}, entry_matches_any_tracked_file={shared}",
                    entry.path,
                    entry.glob
                );
            }
            agreements += 1;
            if local {
                positives += 1;
            }
        }
    }

    // Guard against a vacuous pass: two matchers that both always answer `false`
    // would agree on everything.
    if positives == 0 {
        bail!("no candidate matched any entry, so agreement proves nothing");
    }
    if agreements != entries.len() * candidates.len() {
        bail!("the differential matrix did not run in full");
    }
    Ok(())
}

/// The `--ignored` and `-z` flags are the kind of thing a later edit drops
/// while every unit test keeps passing, because both only change what Git hands
/// back. So drive the real `resolve_checkout` against a real repository.
#[test]
fn resolve_checkout_reports_ignored_and_unquoted_paths() -> Result<()> {
    let root = tempfile::tempdir()?;
    let run = |args: &[&str]| -> Result<()> {
        let status =
            std::process::Command::new("git").arg("-C").arg(root.path()).args(args).output()?;
        if !status.status.success() {
            bail!("git {args:?} failed: {}", String::from_utf8_lossy(&status.stderr));
        }
        Ok(())
    };

    run(&["init", "-q", "."])?;
    run(&["config", "user.email", "proof@example.invalid"])?;
    run(&["config", "user.name", "proof"])?;
    // Pin the quoting behaviour this test depends on. `core.quotePath` defaults
    // to true, but a developer or image with `core.quotePath=false` in global
    // config would make Git report the non-ASCII path below verbatim even
    // *without* `-z` — and the test would then keep passing with the flag
    // removed, which is the one thing it exists to catch.
    run(&["config", "core.quotePath", "true"])?;
    fs::write(root.path().join(".gitignore"), "target/\n")?;
    fs::create_dir_all(root.path().join("docs"))?;
    fs::write(root.path().join("docs/kept.md"), "kept\n")?;
    run(&["add", "-A"])?;
    run(&["commit", "-qm", "base"])?;

    // An ignored file that `load_input` would happily read and hash.
    fs::create_dir_all(root.path().join("target/receipts"))?;
    fs::write(root.path().join("target/receipts/live.json"), "{}\n")?;
    // A path Git quotes and C-escapes unless `-z` is passed. A control
    // character such as a tab is quoted unconditionally, but Windows
    // filesystems reject it, so this uses a non-ASCII name instead — and
    // non-ASCII quoting is governed by `core.quotePath`, which is configured
    // explicitly below rather than inherited.
    fs::write(root.path().join("docs/we ird-é.md"), "x")?;

    let facts = resolve_checkout(root.path()).ok_or_else(|| eyre!("checkout unresolvable"))?;
    if !is_object_name(&facts.head) {
        bail!("resolved head is not an object name: {}", facts.head);
    }

    // The ignored path must be covered, either named directly or by the
    // collapsed directory entry that stands for it.
    let ignored = "target/receipts/live.json";
    let covered = facts.dirty.iter().any(|entry| {
        entry == ignored || ignored.starts_with(&format!("{}/", entry.trim_end_matches('/')))
    });
    if !covered {
        bail!(
            "ignored consumed path {ignored} is invisible to the integrity check: {:?}",
            facts.dirty
        );
    }

    // Prove the premise instead of assuming it: without `-z`, Git must actually
    // quote this path. If it does not, `-z` is not what makes the assertion
    // below pass and this control proves nothing about the flag.
    let quoted = std::process::Command::new("git")
        .arg("-C")
        .arg(root.path())
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()?;
    let quoted = String::from_utf8_lossy(&quoted.stdout);
    if !quoted.contains("\\303\\251") && !quoted.contains('"') {
        bail!(
            "Git did not quote the awkward path without -z, so this test cannot detect its removal: {quoted:?}"
        );
    }

    // With `-z`, the same path must arrive raw rather than quoted and escaped.
    if !facts.dirty.contains("docs/we ird-é.md") {
        bail!("a path needing quoting was not reported verbatim: {:?}", facts.dirty);
    }

    // Opposite direction: a committed, unmodified file is not reported.
    if facts.dirty.contains("docs/kept.md") {
        bail!("a clean tracked file was reported dirty: {:?}", facts.dirty);
    }
    Ok(())
}

#[test]
fn a_release_only_file_can_be_preserved_without_a_source() -> Result<()> {
    // `preserve_release` keeps what the release repository already has, and the
    // contract lets such a row carry no `source_digest`: the schema constrains
    // only `expected_public_digest` and `release_base_digest` for this action.
    // That shape means the path is release-only, so its absence from S is the
    // declared state rather than a defect, and requiring existence here would
    // make a legitimate lineage addition unplannable.
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["action"] = json!("preserve_release");
    row["path"] = json!("not/in/this/checkout.md");
    row["source_digest"] = Value::Null;

    // The contract really does admit this shape — otherwise the planner would
    // be right to refuse and this test would be asserting against the schema.
    let schema: Value = serde_json::from_str(SCHEMA)?;
    let validator = jsonschema::validator_for(&schema)?;
    if !validator.is_valid(&document) {
        bail!("the published schema rejects a release-only preserve_release row");
    }

    let receipt = plan_value(&document)?;
    if receipt.findings.iter().any(|finding| finding.code == "row_source_absent") {
        bail!("a release-only preserved file was required to exist in S: {:?}", receipt.findings);
    }
    assert_verdict(&receipt, Verdict::Pass)
}

#[test]
fn a_release_only_claim_over_content_that_exists_in_s_is_blocked() -> Result<()> {
    // The complement, and the reason the exemption above is not a loophole.
    // Without this, any row could declare `preserve_release` with a null
    // source and skip the existence requirement entirely. A row claiming to be
    // release-only while the path is present in S understates what publication
    // displaces.
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["action"] = json!("preserve_release");
    row["path"] = json!("crates/perl-parser/src/lexer.rs");
    row["source_digest"] = Value::Null;

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_release_only_source_present")
}

#[test]
fn a_preserving_row_that_declares_a_source_must_still_resolve_it() -> Result<()> {
    // The exemption is keyed on the manifest's own declaration, not on the
    // action alone. A `preserve_release` row that *does* declare a
    // `source_digest` asserts the path is in S, so absence is still a defect.
    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["action"] = json!("preserve_release");
    row["path"] = json!("not/in/this/checkout.md");
    row["source_digest"] =
        json!("sha256:1111111111111111111111111111111111111111111111111111111111111111");

    let receipt = plan_value(&document)?;
    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_source_absent")
}

#[test]
fn a_sparse_checkout_cannot_make_content_look_release_only() -> Result<()> {
    // The permissive reading of absence is the one place the worktree cannot be
    // the witness. A sparse checkout omits tracked paths outside its cone while
    // HEAD matches and `git status` stays clean, so a row could claim to be
    // release-only over content that is really in the prepared commit.
    //
    // The tree says present, the worktree says absent — exactly the sparse
    // case — and the tree must win.
    fn sparse_tree(_root: &Path, _commit: &str, path: &str) -> Option<bool> {
        Some(path == "not/in/this/checkout.md")
    }

    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["action"] = json!("preserve_release");
    row["path"] = json!("not/in/this/checkout.md");
    row["source_digest"] = Value::Null;

    // The worktree really does report this path as absent, so the old
    // worktree-only rule would have accepted the row. Assert that, or this
    // control proves nothing about which witness is consulted.
    if test_loader(Path::new("."), "not/in/this/checkout.md").is_ok() {
        bail!(
            "the harness reports the path as present on disk; pick a path the loader cannot find"
        );
    }

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &test_product_surface(),
        fixture_checkout,
        sparse_tree,
    )?;

    assert_verdict(&receipt, Verdict::Blocked)?;
    assert_finding(&receipt, "row_release_only_source_present")
}

#[test]
fn an_unanswerable_tree_query_cannot_prove_a_release_only_row() -> Result<()> {
    // "I could not tell" must not read as "absent" on the one path where
    // absence is permission.
    fn unanswerable(_root: &Path, _commit: &str, _path: &str) -> Option<bool> {
        None
    }

    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[2];
    row["action"] = json!("preserve_release");
    row["path"] = json!("not/in/this/checkout.md");
    row["source_digest"] = Value::Null;

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &test_product_surface(),
        fixture_checkout,
        unanswerable,
    )?;

    assert_verdict(&receipt, Verdict::NotProven)?;
    assert_finding(&receipt, "row_release_only_unverifiable")
}

/// The tree probe is a Git question, so prove it against real Git rather than
/// only through the injected seam.
#[test]
fn resolve_tree_entry_answers_from_the_commit_not_the_worktree() -> Result<()> {
    let root = tempfile::tempdir()?;
    let run = |args: &[&str]| -> Result<()> {
        let out =
            std::process::Command::new("git").arg("-C").arg(root.path()).args(args).output()?;
        if !out.status.success() {
            bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    };
    run(&["init", "-q", "."])?;
    run(&["config", "user.email", "proof@example.invalid"])?;
    run(&["config", "user.name", "proof"])?;
    fs::create_dir_all(root.path().join("docs"))?;
    fs::write(root.path().join("docs/tracked.md"), "tracked\n")?;
    run(&["add", "-A"])?;
    run(&["commit", "-qm", "base"])?;

    let head = resolve_checkout(root.path()).ok_or_else(|| eyre!("no checkout"))?.head;

    if resolve_tree_entry(root.path(), &head, "docs/tracked.md") != Some(true) {
        bail!("a committed path was not found in its own commit tree");
    }
    if resolve_tree_entry(root.path(), &head, "docs/never-existed.md") != Some(false) {
        bail!("an uncommitted path was reported present");
    }

    // The sparse case, simulated the way it actually presents: the file is gone
    // from the worktree but still in the commit. The tree answer must not move.
    fs::remove_file(root.path().join("docs/tracked.md"))?;
    if resolve_tree_entry(root.path(), &head, "docs/tracked.md") != Some(true) {
        bail!("removing the file from the worktree changed the commit-tree answer");
    }

    // An unresolvable commit is `None`, not `false` — otherwise a broken
    // repository would read as "absent" and license the permissive path.
    if resolve_tree_entry(root.path(), "not-an-object-name", "docs/tracked.md").is_some() {
        bail!("a malformed commit produced an answer");
    }
    Ok(())
}

#[test]
fn a_shipped_configuration_file_cannot_be_displaced() -> Result<()> {
    // The ledger classifies functional configuration as `config`, and twenty-one
    // such entries sit on shipped surfaces: `crates/*/features_sot.toml` drives
    // LSP capability claims, `integrations/lsp4ij/**` is the editor integration
    // itself, `dist-workspace.toml` and `.docker/**` construct the release
    // artifact. Dropping any of them ships a broken product as surely as
    // dropping code, so `config` on a shipped surface is protected.
    //
    // This reverses an earlier assertion of mine. `documentation_on_a_shipped_
    // surface_stays_displaceable` used to claim config was displaceable too, on
    // the reasoning that "release notes and contracts are what publication
    // translates". The flaw is that protection only applies to *displacing*
    // actions — a config file whose `$id` names the swarm repository can still
    // be translated, as `a_destination_context_translation_of_product_code_is_
    // allowed` proves. Nothing legitimate needed it to be droppable.
    let surface = ProductSurface::from_ledger_rows_for_test(vec![
        ("crates/perl-lsp-rs/features_sot.toml", "config", "lsp"),
        ("dist-workspace.toml", "config", "release"),
    ]);

    for path in ["crates/perl-lsp-rs/features_sot.toml", "dist-workspace.toml"] {
        // Not already caught by a cheaper classifier, or this proves nothing
        // about the ledger rule.
        if is_product_or_test_path(path) || is_rust_build_manifest(path) {
            bail!("{path} is already caught as a string; pick a sharper case");
        }
        for action in ["drop_swarm_only", "preserve_release", "regenerate"] {
            let mut document = clean_value()?;
            let row = &mut rows_mut(&mut document)?[1];
            row["path"] = json!(path);
            row["action"] = json!(action);
            if action == "drop_swarm_only" {
                row["expected_public_digest"] = Value::Null;
            }

            let manifest: Manifest = serde_json::from_value(document.clone())?;
            let digest = canonical_digest(&document)?;
            let receipt = evaluate_with_surface(
                &manifest,
                &digest,
                Path::new("."),
                test_loader,
                &surface,
                fixture_checkout,
                fixture_tree,
            )?;

            if !receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
                bail!("{action} on shipped config {path} was allowed: {:?}", receipt.findings);
            }
        }
    }
    Ok(())
}

#[test]
fn shipped_configuration_may_still_be_translated() -> Result<()> {
    // The other direction, and the reason protecting config costs nothing
    // legitimate: publication still needs to rewrite destination context inside
    // these files — a schema `$id` naming the swarm repository, for instance.
    // Protection applies to displacement, not translation.
    let surface = ProductSurface::from_ledger_rows_for_test(vec![(
        "dist-workspace.toml",
        "config",
        "release",
    )]);

    let mut document = clean_value()?;
    let row = &mut rows_mut(&mut document)?[0];
    row["path"] = json!("dist-workspace.toml");
    row["action"] = json!("translate");
    row["class"] = json!("repository_context");

    let manifest: Manifest = serde_json::from_value(document.clone())?;
    let digest = canonical_digest(&document)?;
    let receipt = evaluate_with_surface(
        &manifest,
        &digest,
        Path::new("."),
        test_loader,
        &surface,
        fixture_checkout,
        fixture_tree,
    )?;

    if receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
        bail!("translating shipped config was refused: {:?}", receipt.findings);
    }
    Ok(())
}

/// The real mixed-content entry: `integrations/lsp4ij/perl-lsp/**`, classified
/// `config` on the `editor` surface. Its own `broad_glob_reason` in
/// `policy/non-rust-allowlist.toml` describes the directory as "a fixed
/// directory of sibling descriptors (template, settings, settings schema,
/// initialization options) plus its maintainer README".
///
/// Both halves of that sentence have to hold: the descriptors are protected,
/// the maintainer README is not.
#[test]
fn a_broad_config_glob_protects_its_descriptors_but_not_its_prose() -> Result<()> {
    let surface = ProductSurface::from_glob_rows_for_test(vec![(
        "integrations/lsp4ij/perl-lsp/**",
        "config",
        "editor",
    )]);

    let functional = "integrations/lsp4ij/perl-lsp/template.json";
    let prose = "integrations/lsp4ij/perl-lsp/README.md";

    // Neither is caught by a cheaper classifier, so both answers come from the
    // ledger rule under test.
    for path in [functional, prose] {
        if is_product_or_test_path(path) || is_rust_build_manifest(path) {
            bail!("{path} is already caught as a string; pick a sharper case");
        }
    }

    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!(functional);
    let receipt = plan_with_surface(&document, &surface)?;
    if !receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
        bail!("the functional descriptor was displaceable: {:?}", receipt.findings);
    }

    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!(prose);
    let receipt = plan_with_surface(&document, &surface)?;
    if receipt.findings.iter().any(|f| f.code == "row_product_bearing_exclusion") {
        bail!("the maintainer README was made undroppable: {:?}", receipt.findings);
    }
    Ok(())
}

#[test]
fn prose_inside_a_production_entry_stays_protected() -> Result<()> {
    // The carve-out is deliberately narrow. `production` and `test` are
    // statements about every file they cover, prose included; only the `config`
    // widening needed carving, so a `.md` under a production glob must not be
    // swept out with it.
    let surface = ProductSurface::from_glob_rows_for_test(vec![(
        "clients/sublime/**",
        "production",
        "editor",
    )]);

    let mut document = clean_value()?;
    rows_mut(&mut document)?[1]["path"] = json!("clients/sublime/README.md");
    let receipt = plan_with_surface(&document, &surface)?;
    assert_finding(&receipt, "row_product_bearing_exclusion")
}

#[test]
fn a_reconciliation_receipt_without_a_source_commit_is_not_proven() -> Result<()> {
    // A receipt that records no source commit never said which tree it
    // reconciled. Nothing is known to be *stale* — identity cannot be
    // established at all — so this is `not_proven`, not `blocked`.
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["subjects"]["source"]["commit"] = Value::Null;
    let mutated = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&mutated), &mut state);
    let (verdict, findings) = state.finish();

    if verdict != Verdict::NotProven {
        bail!("expected not_proven for an absent source commit, got {verdict:?}: {findings:?}");
    }
    if !findings.iter().any(|f| f.code == "reconciliation_identity_unresolved") {
        bail!("expected reconciliation_identity_unresolved: {findings:?}");
    }
    if findings.iter().any(|f| f.code == "reconciliation_stale") {
        bail!("an absent commit was reported as staleness: {findings:?}");
    }
    Ok(())
}

#[test]
fn a_reconciliation_receipt_without_a_target_commit_is_not_proven() -> Result<()> {
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["subjects"]["target"]["commit"] = Value::Null;
    let mutated = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&mutated), &mut state);
    let (verdict, findings) = state.finish();

    if verdict != Verdict::NotProven {
        bail!("expected not_proven for an absent target commit, got {verdict:?}: {findings:?}");
    }
    if !findings.iter().any(|f| f.code == "reconciliation_identity_unresolved") {
        bail!("expected reconciliation_identity_unresolved: {findings:?}");
    }
    Ok(())
}

#[test]
fn a_resolved_but_different_reconciliation_commit_is_still_blocked() -> Result<()> {
    // The opposite direction, and the reason the split is not just a rename: a
    // commit that resolves to something else *is* a concrete fact about a
    // different tree, and stays `blocked`.
    let mut receipt_value: Value = serde_json::from_slice(RECONCILIATION_FIXTURE)?;
    receipt_value["subjects"]["source"]["commit"] =
        json!("9999999999999999999999999999999999999999");
    let mutated = serde_json::to_vec(&receipt_value)?;

    let manifest: Manifest = serde_json::from_str(CLEAN)?;
    let mut state = PlanState::default();
    validate_reconciliation(&manifest, Some(&mutated), &mut state);
    let (verdict, findings) = state.finish();

    if verdict != Verdict::Blocked {
        bail!("expected blocked for a mismatched commit, got {verdict:?}: {findings:?}");
    }
    if !findings.iter().any(|f| f.code == "reconciliation_stale") {
        bail!("expected reconciliation_stale: {findings:?}");
    }
    Ok(())
}

/// A tracked symlink stays clean in `git status` while the file it points at is
/// modified. `load_input` follows the link and reads the target's bytes, so
/// testing only the declared path would let dirty content support a receipt
/// attributed to `prepared_swarm_sha`.
///
/// Real Git, because the whole mechanism is what Git reports about a link
/// versus its target — a synthetic dirty set could not show that the link is
/// reported clean.
#[cfg(unix)]
#[test]
fn a_modified_symlink_target_is_not_at_the_checkout() -> Result<()> {
    let root = tempfile::tempdir()?;
    let run = |args: &[&str]| -> Result<()> {
        let out =
            std::process::Command::new("git").arg("-C").arg(root.path()).args(args).output()?;
        if !out.status.success() {
            bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    };
    run(&["init", "-q", "."])?;
    run(&["config", "user.email", "proof@example.invalid"])?;
    run(&["config", "user.name", "proof"])?;

    fs::create_dir_all(root.path().join("docs"))?;
    fs::write(root.path().join("docs/real.md"), "committed\n")?;
    std::os::unix::fs::symlink("real.md", root.path().join("docs/link.md"))?;
    run(&["add", "-A"])?;
    run(&["commit", "-qm", "base"])?;

    // Modify the target, leaving the link itself untouched.
    fs::write(root.path().join("docs/real.md"), "tampered\n")?;

    let facts = resolve_checkout(root.path()).ok_or_else(|| eyre!("checkout unresolvable"))?;

    // The premise: Git reports the target dirty and says nothing about the link.
    // Without this the test could pass for the wrong reason.
    if !facts.dirty.contains("docs/real.md") {
        bail!("git did not report the modified target: {:?}", facts.dirty);
    }
    if facts.dirty.contains("docs/link.md") {
        bail!("git reported the symlink itself dirty, so this is not the case under test");
    }

    // A manifest that consumes the *link*, at the commit the checkout is on.
    let mut document = clean_value()?;
    document["prepared_swarm_sha"] = json!(facts.head);
    inputs_mut(&mut document)?[0]["path"] = json!("docs/link.md");
    let manifest: Manifest = serde_json::from_value(document)?;

    let mut state = PlanState::default();
    validate_worktree_integrity(&manifest, root.path(), &facts, &mut state);
    let (verdict, findings) = state.finish();

    if verdict != Verdict::NotProven
        || !findings.iter().any(|f| f.code == "consumed_path_not_at_checkout")
    {
        bail!("a modified symlink target did not invalidate the plan: {verdict:?} {findings:?}");
    }
    Ok(())
}

/// Opposite direction: a symlink whose target is unchanged must not be flagged,
/// or the rule would refuse every manifest that consumes a link.
#[cfg(unix)]
#[test]
fn a_symlink_to_an_unmodified_target_still_plans_clean() -> Result<()> {
    let root = tempfile::tempdir()?;
    let run = |args: &[&str]| -> Result<()> {
        let out =
            std::process::Command::new("git").arg("-C").arg(root.path()).args(args).output()?;
        if !out.status.success() {
            bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        }
        Ok(())
    };
    run(&["init", "-q", "."])?;
    run(&["config", "user.email", "proof@example.invalid"])?;
    run(&["config", "user.name", "proof"])?;

    fs::create_dir_all(root.path().join("docs"))?;
    fs::write(root.path().join("docs/real.md"), "committed\n")?;
    std::os::unix::fs::symlink("real.md", root.path().join("docs/link.md"))?;
    fs::write(root.path().join("unrelated.txt"), "x\n")?;
    run(&["add", "-A"])?;
    run(&["commit", "-qm", "base"])?;

    // Dirty something the plan never reads, so the dirty set is non-empty and
    // the check actually runs rather than short-circuiting on an empty set.
    fs::write(root.path().join("unrelated.txt"), "changed\n")?;

    let facts = resolve_checkout(root.path()).ok_or_else(|| eyre!("checkout unresolvable"))?;
    if facts.dirty.is_empty() {
        bail!("the dirty set is empty, so the integrity check short-circuits and proves nothing");
    }

    let mut document = clean_value()?;
    document["prepared_swarm_sha"] = json!(facts.head);
    inputs_mut(&mut document)?[0]["path"] = json!("docs/link.md");
    let manifest: Manifest = serde_json::from_value(document)?;

    let mut state = PlanState::default();
    validate_worktree_integrity(&manifest, root.path(), &facts, &mut state);
    let (_, findings) = state.finish();

    if findings.iter().any(|f| f.code == "consumed_path_not_at_checkout") {
        bail!("a symlink to an unmodified target was refused: {findings:?}");
    }
    Ok(())
}
