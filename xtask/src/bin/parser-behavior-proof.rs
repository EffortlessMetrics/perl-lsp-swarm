//! Validate parser behavior proofs against the exact test bodies that carry them.
//!
//! The registry binds each behavior-defining expectation to a governing concept,
//! a production entrypoint/decision, a positive test, and at least one nearby
//! negative control. Collection contracts reject partial consumption such as
//! `.first()` when the declared cardinality is `all`.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "parser-behavior-proof")]
#[command(about = "Validate parser behavior proof and expectation-consumption contracts")]
struct Args {
    /// Repository root. Defaults to the parent of the xtask crate.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Behavior-proof policy.
    #[arg(long, default_value = "policy/parser-behavior-proofs.toml")]
    policy: PathBuf,

    /// Optional deterministic JSON receipt path.
    #[arg(long)]
    receipt: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Policy {
    schema_version: u32,
    policy: String,
    #[serde(default)]
    proof: Vec<BehaviorProof>,
    #[serde(default)]
    collection: Vec<CollectionProof>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BehaviorProof {
    id: String,
    concept_id: String,
    governing_contract: String,
    entrypoint: String,
    required_decision: String,
    source_file: PathBuf,
    positive_test: String,
    source_shape: String,
    expected_outcome: String,
    #[serde(default)]
    positive_markers: Vec<String>,
    claim_boundary: String,
    #[serde(default)]
    negative: Vec<NegativeProof>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NegativeProof {
    test: String,
    reason: String,
    #[serde(default)]
    markers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CollectionProof {
    id: String,
    source_file: PathBuf,
    consumer_test: String,
    collection: String,
    cardinality: String,
    #[serde(default)]
    required_markers: Vec<String>,
    #[serde(default)]
    forbidden_markers: Vec<String>,
    claim_boundary: String,
}

/// One row of the review map #6908 asks for: concept → contract → source file →
/// exercised decision → positive proof → negative controls.
#[derive(Clone, Debug, Serialize)]
struct ProofResult {
    id: String,
    concept_id: String,
    governing_contract: String,
    entrypoint: String,
    source_file: String,
    positive_test: String,
    required_decision: String,
    negative_tests: Vec<String>,
    passed: bool,
}

#[derive(Clone, Debug, Serialize)]
struct CollectionResult {
    id: String,
    source_file: String,
    consumer_test: String,
    collection: String,
    cardinality: String,
    passed: bool,
}

#[derive(Clone, Debug, Serialize)]
struct Finding {
    level: &'static str,
    code: &'static str,
    proof_id: String,
    source_file: Option<String>,
    test: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct Receipt {
    schema_version: &'static str,
    receipt_kind: &'static str,
    policy_path: String,
    passed: bool,
    behavior_proof_count: usize,
    collection_proof_count: usize,
    error_count: usize,
    warning_count: usize,
    proofs: Vec<ProofResult>,
    collections: Vec<CollectionResult>,
    findings: Vec<Finding>,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();
    let root = args.root.unwrap_or_else(default_root);
    let policy_path = root.join(&args.policy);
    let content = fs::read_to_string(&policy_path)
        .with_context(|| format!("reading {}", policy_path.display()))?;
    let policy: Policy =
        toml::from_str(&content).with_context(|| format!("parsing {}", policy_path.display()))?;
    let receipt = validate_policy(&root, &args.policy, &policy)?;

    for finding in &receipt.findings {
        let command = if finding.level == "error" { "error" } else { "warning" };
        let file = finding.source_file.as_deref().unwrap_or("policy/parser-behavior-proofs.toml");
        let test = finding.test.as_deref().map(|name| format!(", test {name}")).unwrap_or_default();
        eprintln!(
            "::{command} file={file}::[{}] {}{}: {}",
            finding.code, finding.proof_id, test, finding.message
        );
    }

    if let Some(receipt_path) = args.receipt {
        let destination =
            if receipt_path.is_absolute() { receipt_path } else { root.join(receipt_path) };
        write_receipt(&destination, &receipt)?;
        println!("Parser behavior-proof receipt written: {}", destination.display());
    }

    if !receipt.passed {
        bail!("parser behavior-proof validation failed with {} error(s)", receipt.error_count);
    }

    println!(
        "Parser behavior-proof validation passed ({} behavior proof(s), {} collection proof(s))",
        receipt.behavior_proof_count, receipt.collection_proof_count
    );
    Ok(())
}

fn default_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn validate_policy(root: &Path, policy_path: &Path, policy: &Policy) -> Result<Receipt> {
    let mut findings = Vec::new();
    let mut source_cache = BTreeMap::new();
    let mut seen_ids = BTreeSet::new();
    let mut seen_tests = BTreeSet::new();
    let decision_pattern = Regex::new(r"^[a-z0-9_]+$").ok();

    if policy.schema_version != 1 {
        findings.push(global_error(
            "UNSUPPORTED_SCHEMA",
            format!("schema_version must be 1, got {}", policy.schema_version),
        ));
    }
    if policy.policy != "parser-behavior-proofs" {
        findings.push(global_error(
            "POLICY_ID_MISMATCH",
            format!("policy must be 'parser-behavior-proofs', got '{}'", policy.policy),
        ));
    }
    if policy.proof.is_empty() {
        findings.push(global_error(
            "NO_BEHAVIOR_PROOFS",
            "at least one behavior proof is required".to_string(),
        ));
    }

    let mut proofs = Vec::new();
    for proof in &policy.proof {
        let before = findings.len();
        validate_behavior_proof(
            root,
            proof,
            decision_pattern.as_ref(),
            &mut seen_ids,
            &mut seen_tests,
            &mut source_cache,
            &mut findings,
        );
        proofs.push(ProofResult {
            id: proof.id.clone(),
            concept_id: proof.concept_id.clone(),
            governing_contract: proof.governing_contract.clone(),
            entrypoint: proof.entrypoint.clone(),
            source_file: normalize_path(&proof.source_file),
            positive_test: proof.positive_test.clone(),
            required_decision: proof.required_decision.clone(),
            negative_tests: proof.negative.iter().map(|negative| negative.test.clone()).collect(),
            passed: findings.len() == before,
        });
    }

    let mut collections = Vec::new();
    for collection in &policy.collection {
        let before = findings.len();
        validate_collection_proof(
            root,
            collection,
            &mut seen_ids,
            &mut seen_tests,
            &mut source_cache,
            &mut findings,
        );
        collections.push(CollectionResult {
            id: collection.id.clone(),
            source_file: normalize_path(&collection.source_file),
            consumer_test: collection.consumer_test.clone(),
            collection: collection.collection.clone(),
            cardinality: collection.cardinality.clone(),
            passed: findings.len() == before,
        });
    }

    findings.sort_by(|left, right| {
        (left.level, &left.proof_id, &left.source_file, &left.test, left.code, &left.message).cmp(
            &(
                right.level,
                &right.proof_id,
                &right.source_file,
                &right.test,
                right.code,
                &right.message,
            ),
        )
    });
    let error_count = findings.iter().filter(|finding| finding.level == "error").count();
    let warning_count = findings.iter().filter(|finding| finding.level == "warning").count();

    Ok(Receipt {
        schema_version: "parser_behavior_proof.v1",
        receipt_kind: "parser_behavior_proof",
        policy_path: normalize_path(policy_path),
        passed: error_count == 0,
        behavior_proof_count: proofs.len(),
        collection_proof_count: collections.len(),
        error_count,
        warning_count,
        proofs,
        collections,
        findings,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_behavior_proof(
    root: &Path,
    proof: &BehaviorProof,
    decision_pattern: Option<&Regex>,
    seen_ids: &mut BTreeSet<String>,
    seen_tests: &mut BTreeSet<String>,
    source_cache: &mut BTreeMap<PathBuf, String>,
    findings: &mut Vec<Finding>,
) {
    validate_common_id(&proof.id, seen_ids, findings);
    validate_nonempty_field(proof, "concept_id", &proof.concept_id, findings);
    validate_nonempty_field(proof, "governing_contract", &proof.governing_contract, findings);
    validate_nonempty_field(proof, "entrypoint", &proof.entrypoint, findings);
    validate_nonempty_field(proof, "source_shape", &proof.source_shape, findings);
    validate_nonempty_field(proof, "expected_outcome", &proof.expected_outcome, findings);
    validate_nonempty_field(proof, "claim_boundary", &proof.claim_boundary, findings);

    match decision_pattern {
        Some(pattern) if !pattern.is_match(&proof.required_decision) => {
            findings.push(proof_error(
                proof,
                "INVALID_DECISION_ID",
                format!("required_decision '{}' must match ^[a-z0-9_]+$", proof.required_decision),
            ));
        }
        Some(_) => {}
        None => findings.push(proof_error(
            proof,
            "DECISION_ID_NOT_VALIDATED",
            "decision-id pattern failed to compile, so the id could not be validated".to_string(),
        )),
    }
    if proof.positive_markers.is_empty() {
        findings.push(proof_error(
            proof,
            "MISSING_POSITIVE_MARKERS",
            "positive_markers must prove source shape, path/decision, and typed outcome"
                .to_string(),
        ));
    }
    if proof.negative.is_empty() {
        findings.push(proof_error(
            proof,
            "MISSING_NEGATIVE_CONTROL",
            "every behavior-defining proof requires at least one negative control".to_string(),
        ));
    }

    if !seen_tests.insert(proof.positive_test.clone()) {
        findings.push(proof_error(
            proof,
            "DUPLICATE_TEST_OWNER",
            format!("test '{}' is already owned by another proof", proof.positive_test),
        ));
    }

    let Some(source) = read_source(root, &proof.source_file, source_cache, &proof.id, findings)
    else {
        return;
    };
    let body = match find_test_body(source, &proof.positive_test) {
        Ok(body) => body,
        Err(error) => {
            findings.push(test_error(
                proof,
                &proof.positive_test,
                error.code("POSITIVE_TEST_MISSING"),
                format!("positive proof: {}", error.describe(&proof.positive_test)),
            ));
            return;
        }
    };

    require_marker(proof, &proof.positive_test, &body, &proof.source_shape, findings);
    for marker in &proof.positive_markers {
        require_marker(proof, &proof.positive_test, &body, marker, findings);
    }
    validate_entrypoint_markers(proof, &body, findings);
    validate_decision_markers(proof, &body, findings);

    for negative in &proof.negative {
        if negative.test == proof.positive_test {
            findings.push(test_error(
                proof,
                &negative.test,
                "NEGATIVE_EQUALS_POSITIVE",
                "negative test must be distinct from the positive proof".to_string(),
            ));
        }
        if !seen_tests.insert(negative.test.clone()) {
            findings.push(test_error(
                proof,
                &negative.test,
                "DUPLICATE_TEST_OWNER",
                "negative test is already owned by another proof".to_string(),
            ));
        }
        if negative.reason.trim().is_empty() {
            findings.push(test_error(
                proof,
                &negative.test,
                "NEGATIVE_REASON_MISSING",
                "negative control requires a reason".to_string(),
            ));
        }
        if negative.markers.is_empty() {
            findings.push(test_error(
                proof,
                &negative.test,
                "NEGATIVE_MARKERS_MISSING",
                "negative control requires at least one discriminating marker".to_string(),
            ));
        }
        let negative_body = match find_test_body(source, &negative.test) {
            Ok(body) => body,
            Err(error) => {
                findings.push(test_error(
                    proof,
                    &negative.test,
                    error.code("NEGATIVE_TEST_MISSING"),
                    format!("negative control: {}", error.describe(&negative.test)),
                ));
                continue;
            }
        };
        for marker in &negative.markers {
            require_marker(proof, &negative.test, &negative_body, marker, findings);
        }
    }
}

fn validate_collection_proof(
    root: &Path,
    collection: &CollectionProof,
    seen_ids: &mut BTreeSet<String>,
    seen_tests: &mut BTreeSet<String>,
    source_cache: &mut BTreeMap<PathBuf, String>,
    findings: &mut Vec<Finding>,
) {
    validate_collection_id(collection, seen_ids, findings);
    if collection.collection.trim().is_empty() {
        findings.push(collection_error(
            collection,
            "COLLECTION_NAME_MISSING",
            "collection must name the consumed manifest field".to_string(),
        ));
    }
    if collection.cardinality != "all" {
        findings.push(collection_error(
            collection,
            "UNSUPPORTED_CARDINALITY",
            format!("cardinality must be 'all', got '{}'", collection.cardinality),
        ));
    }
    if collection.claim_boundary.trim().is_empty() {
        findings.push(collection_error(
            collection,
            "CLAIM_BOUNDARY_MISSING",
            "collection proof requires a claim boundary".to_string(),
        ));
    }
    if collection.required_markers.is_empty() {
        findings.push(collection_error(
            collection,
            "REQUIRED_MARKERS_MISSING",
            "collection proof requires markers showing complete iteration".to_string(),
        ));
    }
    if collection.forbidden_markers.is_empty() {
        findings.push(collection_error(
            collection,
            "FORBIDDEN_MARKERS_MISSING",
            "collection proof requires shortcuts such as .first() to be forbidden".to_string(),
        ));
    }
    if !seen_tests.insert(collection.consumer_test.clone()) {
        findings.push(collection_error(
            collection,
            "DUPLICATE_TEST_OWNER",
            format!("test '{}' is already owned by another proof", collection.consumer_test),
        ));
    }

    let Some(source) =
        read_source(root, &collection.source_file, source_cache, &collection.id, findings)
    else {
        return;
    };
    let body = match find_test_body(source, &collection.consumer_test) {
        Ok(body) => body,
        Err(error) => {
            findings.push(collection_test_error(
                collection,
                error.code("CONSUMER_TEST_MISSING"),
                format!("consumer test: {}", error.describe(&collection.consumer_test)),
            ));
            return;
        }
    };
    for marker in &collection.required_markers {
        if !body.contains(marker) {
            findings.push(collection_test_error(
                collection,
                "COLLECTION_REQUIRED_MARKER_MISSING",
                format!("consumer test does not contain required marker {marker:?}"),
            ));
        }
    }
    for marker in &collection.forbidden_markers {
        if body.contains(marker) {
            findings.push(collection_test_error(
                collection,
                "PARTIAL_COLLECTION_CONSUMPTION",
                format!("consumer test uses forbidden shortcut {marker:?} for cardinality=all"),
            ));
        }
    }
    let collection_marker = format!("fixture.{}", collection.collection);
    if !body.contains(&collection_marker) {
        findings.push(collection_test_error(
            collection,
            "COLLECTION_NOT_REFERENCED",
            format!("consumer test never references {collection_marker:?}"),
        ));
    }
}

fn validate_entrypoint_markers(proof: &BehaviorProof, body: &str, findings: &mut Vec<Finding>) {
    if proof.entrypoint == "Parser::parse" {
        for marker in ["Parser::new", ".parse()"] {
            if !body.contains(marker) {
                findings.push(test_error(
                    proof,
                    &proof.positive_test,
                    "PRODUCTION_ENTRYPOINT_NOT_EXERCISED",
                    format!("Parser::parse proof is missing marker {marker:?}"),
                ));
            }
        }
    }
}

fn validate_decision_markers(proof: &BehaviorProof, body: &str, findings: &mut Vec<Finding>) {
    let required: &[&str] = match proof.required_decision.as_str() {
        "unknown_lowercase_bareword_call" => {
            &["parser.decision_trace()", "ParserDecision::UnknownLowercaseBarewordCall"]
        }
        "unclosed_qw_recovery_boundary" => {
            &["Unclosed qw() delimiter: missing closing delimiter before end of file"]
        }
        _ => {
            findings.push(proof_error(
                proof,
                "UNKNOWN_DECISION_CONTRACT",
                format!(
                    "required_decision '{}' has no registered proof markers",
                    proof.required_decision
                ),
            ));
            return;
        }
    };
    for marker in required {
        if !body.contains(marker) {
            findings.push(test_error(
                proof,
                &proof.positive_test,
                "DECISION_NOT_PROVEN",
                format!(
                    "test does not prove decision '{}' with observed-route marker {marker:?}",
                    proof.required_decision
                ),
            ));
        }
    }
}

fn validate_common_id(id: &str, seen_ids: &mut BTreeSet<String>, findings: &mut Vec<Finding>) {
    if id.trim().is_empty() {
        findings.push(global_error("PROOF_ID_MISSING", "proof id must not be empty".to_string()));
    } else if !seen_ids.insert(id.to_string()) {
        findings.push(global_error(
            "DUPLICATE_PROOF_ID",
            format!("proof id '{id}' appears more than once"),
        ));
    }
}

fn validate_collection_id(
    collection: &CollectionProof,
    seen_ids: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) {
    if collection.id.trim().is_empty() {
        findings.push(collection_error(
            collection,
            "PROOF_ID_MISSING",
            "collection proof id must not be empty".to_string(),
        ));
    } else if !seen_ids.insert(collection.id.clone()) {
        findings.push(collection_error(
            collection,
            "DUPLICATE_PROOF_ID",
            format!("proof id '{}' appears more than once", collection.id),
        ));
    }
}

fn validate_nonempty_field(
    proof: &BehaviorProof,
    field: &'static str,
    value: &str,
    findings: &mut Vec<Finding>,
) {
    if value.trim().is_empty() {
        findings.push(proof_error(
            proof,
            "REQUIRED_FIELD_MISSING",
            format!("{field} must not be empty"),
        ));
    }
}

fn require_marker(
    proof: &BehaviorProof,
    test: &str,
    body: &str,
    marker: &str,
    findings: &mut Vec<Finding>,
) {
    if !body.contains(marker) {
        findings.push(test_error(
            proof,
            test,
            "TEST_MARKER_MISSING",
            format!("test body does not contain required marker {marker:?}"),
        ));
    }
}

fn read_source<'a>(
    root: &Path,
    relative: &Path,
    cache: &'a mut BTreeMap<PathBuf, String>,
    proof_id: &str,
    findings: &mut Vec<Finding>,
) -> Option<&'a str> {
    if !cache.contains_key(relative) {
        let path = root.join(relative);
        match fs::read_to_string(&path) {
            Ok(content) => {
                // Cache the comment-blanked form: every marker check downstream must
                // run against executable code, never against prose.
                cache.insert(relative.to_path_buf(), blank_comments(&content));
            }
            Err(error) => {
                findings.push(Finding {
                    level: "error",
                    code: "SOURCE_FILE_UNREADABLE",
                    proof_id: proof_id.to_string(),
                    source_file: Some(normalize_path(relative)),
                    test: None,
                    message: format!("unable to read {}: {error}", path.display()),
                });
                return None;
            }
        }
    }
    cache.get(relative).map(String::as_str)
}

/// Why a named test body could not be admitted as the owner of its markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestBodyError {
    /// No function with exactly this name is defined in the file.
    NotFound,
    /// The name is defined more than once, so marker ownership is ambiguous.
    Ambiguous(usize),
    /// A function with this name exists but does not carry `#[test]`.
    NotATest,
    /// The braces of the function body do not balance.
    Unbalanced,
}

impl TestBodyError {
    /// Finding code, using the caller's role-specific code for a plain absence so the
    /// existing `*_TEST_MISSING` receipt vocabulary stays stable.
    fn code(self, missing: &'static str) -> &'static str {
        match self {
            Self::NotFound => missing,
            Self::Ambiguous(_) => "TEST_NAME_AMBIGUOUS",
            Self::NotATest => "TEST_NOT_A_TEST_FUNCTION",
            Self::Unbalanced => "TEST_BODY_UNBALANCED",
        }
    }

    fn describe(self, test_name: &str) -> String {
        match self {
            Self::NotFound => format!("test function '{test_name}' was not found"),
            Self::Ambiguous(count) => {
                format!("'{test_name}' is defined {count} times, so marker ownership is ambiguous")
            }
            Self::NotATest => {
                format!("'{test_name}' exists but does not carry a #[test] attribute")
            }
            Self::Unbalanced => format!("body of '{test_name}' has unbalanced braces"),
        }
    }
}

/// Return the brace-balanced body of the `#[test]` function named `test_name`.
///
/// `code` must already have had its comments blanked by [`blank_comments`], so a
/// marker cannot be satisfied by prose. The name is matched on an exact identifier
/// boundary, the function must carry `#[test]`, and the body is bounded by its own
/// braces rather than by the next attribute, so a neighbouring helper cannot supply
/// the markers a proof claims.
fn find_test_body(code: &str, test_name: &str) -> Result<String, TestBodyError> {
    let sites = definition_sites(code, test_name);
    let site = match sites.as_slice() {
        [] => return Err(TestBodyError::NotFound),
        [only] => *only,
        many => return Err(TestBodyError::Ambiguous(many.len())),
    };
    if !is_test_function(code, site) {
        return Err(TestBodyError::NotATest);
    }
    let (open, end) = body_extent(code, site).ok_or(TestBodyError::Unbalanced)?;
    Ok(code.get(open..end).unwrap_or_default().to_string())
}

/// Byte offsets of every `fn <test_name>` definition, matched on an identifier
/// boundary so `fn foo` does not bind to `fn foo_bar`.
fn definition_sites(code: &str, test_name: &str) -> Vec<usize> {
    let needle = format!("fn {test_name}");
    let mut sites = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = code.get(cursor..).and_then(|rest| rest.find(&needle)) {
        let start = cursor + offset;
        let after = start + needle.len();
        let bounded = code
            .get(after..)
            .and_then(|rest| rest.chars().next())
            .is_none_or(|next| !next.is_alphanumeric() && next != '_');
        if bounded {
            sites.push(start);
        }
        cursor = after;
    }
    sites
}

/// Whether the definition at `site` is preceded by a `#[test]` attribute.
fn is_test_function(code: &str, site: usize) -> bool {
    let mut line_start = code.get(..site).and_then(|head| head.rfind('\n')).map_or(0, |at| at + 1);
    while line_start > 0 {
        let previous_end = line_start - 1;
        let previous_start =
            code.get(..previous_end).and_then(|head| head.rfind('\n')).map_or(0, |at| at + 1);
        let line = code.get(previous_start..previous_end).unwrap_or_default().trim();
        if line.contains("#[test]") {
            return true;
        }
        // Blank lines and other attributes may sit between `#[test]` and `fn`.
        // Comments are already blanked, so they read as blank lines here.
        if line.is_empty() || line.starts_with("#[") || line.starts_with("#!") {
            line_start = previous_start;
            continue;
        }
        return false;
    }
    false
}

/// Byte range of the brace-balanced body beginning at the first `{` at or after
/// `from`. Braces inside string and character literals are ignored, so a declared
/// Perl source shape such as `$options->{limit}` cannot unbalance the scan.
fn body_extent(code: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = code.as_bytes();
    let mut index = from;
    let open = loop {
        let byte = *bytes.get(index)?;
        match byte {
            b'{' => break index,
            b'"' => index = skip_quoted(bytes, index)?,
            b'r' if starts_raw_string(bytes, index) => index = skip_raw_string(bytes, index)?,
            b'\'' if is_char_literal(bytes, index) => index = skip_quoted(bytes, index)?,
            _ => index += 1,
        }
    };
    let mut depth = 0usize;
    index = open;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' => {
                depth = depth.checked_sub(1)?;
                index += 1;
                if depth == 0 {
                    return Some((open, index));
                }
            }
            b'"' => index = skip_quoted(bytes, index)?,
            b'r' if starts_raw_string(bytes, index) => index = skip_raw_string(bytes, index)?,
            b'\'' if is_char_literal(bytes, index) => index = skip_quoted(bytes, index)?,
            _ => index += 1,
        }
    }
    None
}

/// Replace the bytes of every comment with spaces, preserving length, newlines, and
/// every string literal. Markers must therefore appear in executable code; prose in a
/// doc comment or a trailing `//` note can no longer admit a proof.
fn blank_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = source.as_bytes().to_vec();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => index = skip_quoted(bytes, index).unwrap_or(bytes.len()),
            b'r' if starts_raw_string(bytes, index) => {
                index = skip_raw_string(bytes, index).unwrap_or(bytes.len());
            }
            b'\'' if is_char_literal(bytes, index) => {
                index = skip_quoted(bytes, index).unwrap_or(bytes.len());
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                while index < bytes.len() && bytes[index] != b'\n' {
                    out[index] = b' ';
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let mut depth = 1usize;
                let mut cursor = index + 2;
                out[index] = b' ';
                out[index + 1] = b' ';
                while cursor < bytes.len() && depth > 0 {
                    if bytes[cursor] == b'/' && bytes.get(cursor + 1) == Some(&b'*') {
                        depth += 1;
                    } else if bytes[cursor] == b'*' && bytes.get(cursor + 1) == Some(&b'/') {
                        depth -= 1;
                    } else {
                        if bytes[cursor] != b'\n' {
                            out[cursor] = b' ';
                        }
                        cursor += 1;
                        continue;
                    }
                    out[cursor] = b' ';
                    out[cursor + 1] = b' ';
                    cursor += 2;
                }
                index = cursor;
            }
            _ => index += 1,
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

/// Offset just past a `"..."` or `'...'` literal opening at `start`.
fn skip_quoted(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = *bytes.get(start)?;
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

/// Whether `r` at `start` opens a raw string rather than continuing an identifier.
fn starts_raw_string(bytes: &[u8], start: usize) -> bool {
    let preceded_by_identifier = start
        .checked_sub(1)
        .and_then(|before| bytes.get(before))
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    if preceded_by_identifier {
        return false;
    }
    let mut index = start + 1;
    while bytes.get(index) == Some(&b'#') {
        index += 1;
    }
    bytes.get(index) == Some(&b'"')
}

/// Offset just past a raw string literal opening at `start`.
fn skip_raw_string(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    let mut hashes = 0usize;
    while bytes.get(index) == Some(&b'#') {
        hashes += 1;
        index += 1;
    }
    if bytes.get(index) != Some(&b'"') {
        return None;
    }
    index += 1;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let closing =
                (0..hashes).take_while(|at| bytes.get(index + 1 + at) == Some(&b'#')).count();
            if closing == hashes {
                return Some(index + 1 + hashes);
            }
        }
        index += 1;
    }
    None
}

/// Distinguish a character literal from a lifetime such as `&'a str`.
fn is_char_literal(bytes: &[u8], start: usize) -> bool {
    match bytes.get(start + 1) {
        Some(b'\\') => true,
        Some(_) => bytes.get(start + 2) == Some(&b'\''),
        None => false,
    }
}

fn write_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let serialized = serde_json::to_string_pretty(receipt).context("serializing receipt")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, format!("{serialized}\n"))
        .with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn global_error(code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        proof_id: "policy".to_string(),
        source_file: None,
        test: None,
        message,
    }
}

fn proof_error(proof: &BehaviorProof, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        proof_id: proof.id.clone(),
        source_file: Some(normalize_path(&proof.source_file)),
        test: None,
        message,
    }
}

fn test_error(proof: &BehaviorProof, test: &str, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        proof_id: proof.id.clone(),
        source_file: Some(normalize_path(&proof.source_file)),
        test: Some(test.to_string()),
        message,
    }
}

fn collection_error(proof: &CollectionProof, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        proof_id: proof.id.clone(),
        source_file: Some(normalize_path(&proof.source_file)),
        test: None,
        message,
    }
}

fn collection_test_error(proof: &CollectionProof, code: &'static str, message: String) -> Finding {
    Finding {
        level: "error",
        code,
        proof_id: proof.id.clone(),
        source_file: Some(normalize_path(&proof.source_file)),
        test: Some(proof.consumer_test.clone()),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_source(temp: &TempDir, relative: &str, content: &str) -> Result<PathBuf> {
        let path = temp.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        Ok(PathBuf::from(relative))
    }

    fn valid_behavior(source_file: PathBuf) -> BehaviorProof {
        BehaviorProof {
            id: "call-proof".to_string(),
            concept_id: "parser.calls.unknown".to_string(),
            governing_contract: "#1".to_string(),
            entrypoint: "Parser::parse".to_string(),
            required_decision: "unknown_lowercase_bareword_call".to_string(),
            source_file,
            positive_test: "positive".to_string(),
            source_shape: "call $x 1;".to_string(),
            expected_outcome: "FunctionCall".to_string(),
            positive_markers: vec![
                "parser.decision_trace()".to_string(),
                "ParserDecision::UnknownLowercaseBarewordCall".to_string(),
                "FunctionCall".to_string(),
            ],
            claim_boundary: "bounded call proof".to_string(),
            negative: vec![NegativeProof {
                test: "negative".to_string(),
                reason: "parenthesized form is a different route".to_string(),
                markers: vec!["parenthesized control".to_string()],
            }],
        }
    }

    fn valid_policy(source_file: PathBuf) -> Policy {
        Policy {
            schema_version: 1,
            policy: "parser-behavior-proofs".to_string(),
            proof: vec![valid_behavior(source_file)],
            collection: Vec::new(),
        }
    }

    #[test]
    fn valid_behavior_and_negative_control_pass() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() {\n let mut parser = Parser::new(\"call $x 1;\");\n let _ = parser.parse();\n let _ = parser.decision_trace();\n let _ = ParserDecision::UnknownLowercaseBarewordCall;\n let _ = FunctionCall;\n}\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(receipt.passed, "findings: {:?}", receipt.findings);
        Ok(())
    }

    #[test]
    fn missing_positive_marker_fails() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() { let mut p = Parser::new(\"call $x 1;\"); let _ = p.parse(); }\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "TEST_MARKER_MISSING"),
            "expected TEST_MARKER_MISSING, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    #[test]
    fn missing_negative_test_fails() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() { let mut parser = Parser::new(\"call $x 1;\"); let _ = parser.parse(); let _ = parser.decision_trace(); let _ = ParserDecision::UnknownLowercaseBarewordCall; let _ = FunctionCall; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "NEGATIVE_TEST_MISSING"),
            "expected NEGATIVE_TEST_MISSING, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    #[test]
    fn collection_first_shortcut_fails_all_cardinality() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "collection.rs",
            "#[test]\nfn consume() { let _ = fixture.recovery_expectations.first(); }\n",
        )?;
        let policy = Policy {
            schema_version: 1,
            policy: "parser-behavior-proofs".to_string(),
            proof: Vec::new(),
            collection: vec![CollectionProof {
                id: "recovery-all".to_string(),
                source_file,
                consumer_test: "consume".to_string(),
                collection: "recovery_expectations".to_string(),
                cardinality: "all".to_string(),
                required_markers: vec!["fixture.recovery_expectations".to_string()],
                forbidden_markers: vec!["fixture.recovery_expectations.first()".to_string()],
                claim_boundary: "all recovery expectations".to_string(),
            }],
        };
        let receipt = validate_policy(temp.path(), Path::new("policy.toml"), &policy)?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "PARTIAL_COLLECTION_CONSUMPTION"),
            "expected PARTIAL_COLLECTION_CONSUMPTION, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    #[test]
    fn duplicate_proof_ids_fail() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() { let mut parser = Parser::new(\"call $x 1;\"); let _ = parser.parse(); let _ = parser.decision_trace(); let _ = ParserDecision::UnknownLowercaseBarewordCall; let _ = FunctionCall; }\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let mut policy = valid_policy(source_file.clone());
        policy.proof.push(valid_behavior(source_file));
        let receipt = validate_policy(temp.path(), Path::new("policy.toml"), &policy)?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "DUPLICATE_PROOF_ID"),
            "expected DUPLICATE_PROOF_ID, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    /// Falsifier: author-controlled prose must not admit a proof.
    #[test]
    fn markers_only_in_comments_do_not_admit_proof() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() {\n let mut parser = Parser::new(\"call $x 1;\");\n let _ = parser.parse();\n // parser.decision_trace() and ParserDecision::UnknownLowercaseBarewordCall\n /* FunctionCall */\n}\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "TEST_MARKER_MISSING"),
            "commented markers must not satisfy the proof, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    /// Falsifier: `fn positive` must not bind to `fn positive_helper`.
    #[test]
    fn prefix_named_function_does_not_own_markers() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "fn positive_helper() {\n let mut parser = Parser::new(\"call $x 1;\");\n let _ = parser.parse();\n let _ = parser.decision_trace();\n let _ = ParserDecision::UnknownLowercaseBarewordCall;\n let _ = FunctionCall;\n}\n#[test]\nfn positive() { let mut parser = Parser::new(\"call $x 1;\"); let _ = parser.parse(); }\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            !receipt.passed,
            "a prefix-named helper must not supply another test's markers, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    /// Falsifier: a plain function must not own a behavior proof.
    #[test]
    fn non_test_function_cannot_own_proof() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "fn positive() {\n let mut parser = Parser::new(\"call $x 1;\");\n let _ = parser.parse();\n let _ = parser.decision_trace();\n let _ = ParserDecision::UnknownLowercaseBarewordCall;\n let _ = FunctionCall;\n}\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "TEST_NOT_A_TEST_FUNCTION"),
            "expected TEST_NOT_A_TEST_FUNCTION, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    /// Falsifier: two definitions of one name leave marker ownership ambiguous.
    #[test]
    fn duplicate_test_definition_is_ambiguous() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() {\n let mut parser = Parser::new(\"call $x 1;\");\n let _ = parser.parse();\n let _ = parser.decision_trace();\n let _ = ParserDecision::UnknownLowercaseBarewordCall;\n let _ = FunctionCall;\n}\n#[test]\nfn positive() { }\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let receipt =
            validate_policy(temp.path(), Path::new("policy.toml"), &valid_policy(source_file))?;
        assert!(
            receipt.findings.iter().any(|finding| finding.code == "TEST_NAME_AMBIGUOUS"),
            "expected TEST_NAME_AMBIGUOUS, findings: {:?}",
            receipt.findings
        );
        Ok(())
    }

    /// A declared Perl source shape carries braces. Body extraction must treat them as
    /// string content, not as the end of the test.
    #[test]
    fn braces_inside_a_source_shape_do_not_truncate_the_body() -> Result<()> {
        let temp = TempDir::new().context("creating temp directory")?;
        let source_file = write_source(
            &temp,
            "tests.rs",
            "#[test]\nfn positive() {\n let mut parser = Parser::new(\"call $obj ($t // 'x'), $options->{limit};\");\n let _ = parser.parse();\n let _ = parser.decision_trace();\n let _ = ParserDecision::UnknownLowercaseBarewordCall;\n let _ = FunctionCall;\n}\n#[test]\nfn negative() { let _ = \"parenthesized control\"; }\n",
        )?;
        let mut policy = valid_policy(source_file);
        if let Some(proof) = policy.proof.first_mut() {
            proof.source_shape = "call $obj ($t // 'x'), $options->{limit};".to_string();
        }
        let receipt = validate_policy(temp.path(), Path::new("policy.toml"), &policy)?;
        assert!(receipt.passed, "findings: {:?}", receipt.findings);
        Ok(())
    }
}
