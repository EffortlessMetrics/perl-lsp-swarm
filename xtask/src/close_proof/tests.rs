//! Discriminating proof for the close-proof schema train (#10380).
//!
//! Corruption rows prove that mis-typed or semantically broken documents fail
//! validation while valid rows pass, and that packets bind current contract
//! identity. The committed regression corpus under `.ci/close-proof-contract/`
//! must verify end to end.

use super::contract::IssueContract;
use super::corpus::{ManifestEntry, load_corpus_manifest, verify_corpus_at};
use super::model::{
    ChildDispositionRecord, ChildState, ClaimStatement, CloseMode, ClosePacket, ControlOutcome,
    DenominatorRow, PacketBinding, ProofLevel, RowDispositionValue,
};
use super::{
    CloseProofError, IssueCloseOutcome, IssueKind, PrScopeOutcome, content_digest_hex, corpus_root,
    is_repository_id, is_stable_token, validate_packet_against_contract, verify_corpus,
};

const CORPUS_FIXTURE_COUNT: usize = 15;

fn leaf_digest(seed: u64) -> String {
    format!("{seed:064x}")
}

fn leaf_contract() -> Result<IssueContract, CloseProofError> {
    IssueContract::minimal_leaf(
        "effortlessmetrics/perl-lsp-swarm",
        9000300,
        "fix(parser): reject one malformed delimiter",
        "single-row.defect.fixed",
        "The named defect is repaired and its neighboring form still parses.",
        ProofLevel::Mechanism,
        &leaf_digest(9000300),
    )
}

fn current_binding(contract: &IssueContract) -> PacketBinding {
    PacketBinding {
        contract_issue_body_digest: contract.identity.issue_body_digest.clone(),
        contract_denominator_digest: contract.identity.denominator_digest.clone(),
        accepted_ruling_digest: None,
    }
}

fn passing_packet(contract: &IssueContract) -> Result<ClosePacket, CloseProofError> {
    Ok(ClosePacket {
        schema_version: super::CLOSE_PACKET_SCHEMA_V1.to_string(),
        repository: contract.repository.clone(),
        issue_number: contract.issue_number,
        requested_close_mode: CloseMode::Completed,
        contract_binding: current_binding(contract),
        candidate_pr: Some(9000400),
        landed_subjects: Vec::new(),
        landing_content_proof: Vec::new(),
        established_claims: vec![ClaimStatement {
            statement: "The named defect repair is proven on current main.".to_string(),
            covers_rows: vec!["single-row.defect.fixed".to_string()],
        }],
        explicitly_not_established_claims: Vec::new(),
        row_dispositions: [(
            "single-row.defect.fixed".to_string(),
            RowDispositionValue::ProvenCurrentMain {
                evidence: super::EvidenceRef {
                    producer: "xtask-pr-close-proof".to_string(),
                    subject: "a".repeat(64),
                    content_digest: content_digest_hex(b"evidence-bytes"),
                    reference: "cargo xtask pr-close-proof --pr 9000400".to_string(),
                },
            },
        )]
        .into_iter()
        .collect(),
        negative_control_dispositions: Default::default(),
        child_dispositions: Vec::new(),
        duplicate_of: None,
        verdict: super::CloseVerdict {
            pr_scope: PrScopeOutcome::Pass,
            issue_close: IssueCloseOutcome::Valid,
            reasons: vec!["Every required row is proven on current main.".to_string()],
        },
    })
}

// ---------------------------------------------------------------------------
// Regression corpus integrity
// ---------------------------------------------------------------------------

#[test]
fn committed_corpus_verifies_end_to_end() -> Result<(), CloseProofError> {
    let verified = verify_corpus()?;
    assert_eq!(verified, CORPUS_FIXTURE_COUNT);
    Ok(())
}

#[test]
fn tampered_manifest_digest_fails_verification() -> Result<(), CloseProofError> {
    let mut manifest = load_corpus_manifest()?;
    let first = &mut manifest.fixtures[0];
    let mut tampered = first.sha256.clone();
    tampered.replace_range(0..1, if first.sha256.starts_with('0') { "1" } else { "0" });
    first.sha256 = tampered;
    let result = verify_corpus_at(&corpus_root(), &manifest);
    assert!(matches!(result, Err(CloseProofError::Corpus { .. })));
    Ok(())
}

#[test]
fn drifted_manifest_membership_fails_verification() -> Result<(), CloseProofError> {
    let mut manifest = load_corpus_manifest()?;
    manifest.fixtures.remove(0);
    let result = verify_corpus_at(&corpus_root(), &manifest);
    assert!(matches!(result, Err(CloseProofError::Corpus { .. })));
    Ok(())
}

#[test]
fn unlisted_on_disk_fixture_fails_verification() -> Result<(), CloseProofError> {
    let mut manifest = load_corpus_manifest()?;
    manifest.fixtures.push(ManifestEntry {
        file: "fixtures/not-really-on-disk.json".to_string(),
        sha256: leaf_digest(1),
    });
    let result = verify_corpus_at(&corpus_root(), &manifest);
    assert!(matches!(result, Err(CloseProofError::Corpus { .. })));
    Ok(())
}

// ---------------------------------------------------------------------------
// Contract validation discrimination
// ---------------------------------------------------------------------------

#[test]
fn valid_minimal_leaf_contract_passes() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    contract.validate()?;
    Ok(())
}

#[test]
fn unknown_top_level_field_is_rejected() -> Result<(), CloseProofError> {
    let json = leaf_contract()?.to_canonical_json()?;
    let poisoned = json.replace(
        "{\n  \"schema_version\"",
        "{\n  \"bogus_extra_field\": true,\n  \"schema_version\"",
    );
    assert!(matches!(IssueContract::from_json_str(&poisoned), Err(CloseProofError::Schema { .. })));
    Ok(())
}

#[test]
fn mistyped_issue_number_is_rejected() -> Result<(), CloseProofError> {
    let json = leaf_contract()?.to_canonical_json()?;
    let poisoned = json.replace("\"issue_number\": 9000300", "\"issue_number\": \"9000300\"");
    assert!(matches!(IssueContract::from_json_str(&poisoned), Err(CloseProofError::Schema { .. })));
    Ok(())
}

#[test]
fn wrong_schema_version_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.schema_version = "issue_contract.v2".to_string();
    assert!(matches!(
        contract.validate(),
        Err(CloseProofError::Schema { field, .. }) if field == "schema_version"
    ));
    Ok(())
}

#[test]
fn unstable_row_id_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.denominator[0].row_id = "Bad Row Id!".to_string();
    assert!(matches!(
        contract.validate(),
        Err(CloseProofError::Schema { field, .. }) if field == "denominator.row_id"
    ));
    Ok(())
}

#[test]
fn duplicate_row_ids_are_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.denominator.push(DenominatorRow {
        row_id: contract.denominator[0].row_id.clone(),
        statement: "A second row stealing the same stable id.".to_string(),
        required_proof_level: ProofLevel::Representation,
    });
    contract.identity.denominator_digest =
        super::compute_denominator_digest(&contract.denominator)?;
    assert!(matches!(contract.validate(), Err(CloseProofError::Coverage { .. })));
    Ok(())
}

#[test]
fn dangling_negative_control_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.negative_controls = vec![super::NegativeControlRow {
        control_id: "nc.dangling".to_string(),
        guards_row_id: "row-that-does-not-exist".to_string(),
        description: "Guards nothing.".to_string(),
    }];
    assert!(matches!(contract.validate(), Err(CloseProofError::Coverage { .. })));
    Ok(())
}

#[test]
fn forged_denominator_digest_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.identity.denominator_digest = leaf_digest(1);
    assert!(matches!(contract.validate(), Err(CloseProofError::Digest { .. })));
    Ok(())
}

#[test]
fn malformed_body_digest_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.identity.issue_body_digest = "not-a-digest".to_string();
    assert!(matches!(contract.validate(), Err(CloseProofError::Digest { .. })));
    Ok(())
}

#[test]
fn controller_without_children_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.kind = IssueKind::Controller;
    assert!(matches!(contract.validate(), Err(CloseProofError::Coverage { .. })));
    Ok(())
}

#[test]
fn permitted_transfer_requires_conditions() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.transfer_policy.permitted = true;
    contract.transfer_policy.conditions = Vec::new();
    assert!(matches!(contract.validate(), Err(CloseProofError::Coverage { .. })));
    Ok(())
}

// ---------------------------------------------------------------------------
// Packet-versus-contract discrimination
// ---------------------------------------------------------------------------

#[test]
fn current_packet_validates_against_its_contract() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let doc_packet = passing_packet(&contract)?;
    validate_packet_against_contract(&doc_packet, &contract)?;
    Ok(())
}

#[test]
fn moved_issue_body_invalidates_packet() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut stale = passing_packet(&contract)?;
    stale.contract_binding.contract_issue_body_digest = leaf_digest(777);
    assert!(matches!(
        validate_packet_against_contract(&stale, &contract),
        Err(CloseProofError::Identity { .. })
    ));
    Ok(())
}

#[test]
fn moved_ruling_invalidates_packet() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    let doc_packet = passing_packet(&contract)?;
    contract.identity.accepted_ruling = Some(super::RulingIdentity {
        identity: "https://github.com/effortlessmetrics/perl-lsp-swarm/issues/9000300#ruling"
            .to_string(),
        digest: leaf_digest(4242),
    });
    assert!(matches!(
        validate_packet_against_contract(&doc_packet, &contract),
        Err(CloseProofError::Identity { .. })
    ));
    Ok(())
}

#[test]
fn matching_ruling_keeps_packet_current() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.identity.accepted_ruling = Some(super::RulingIdentity {
        identity: "https://github.com/effortlessmetrics/perl-lsp-swarm/issues/9000300#ruling"
            .to_string(),
        digest: leaf_digest(4242),
    });
    let mut doc_packet = passing_packet(&contract)?;
    doc_packet.contract_binding.accepted_ruling_digest = Some(leaf_digest(4242));
    validate_packet_against_contract(&doc_packet, &contract)?;
    Ok(())
}

#[test]
fn silently_dropped_row_is_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut dropping = passing_packet(&contract)?;
    dropping.row_dispositions.clear();
    assert!(matches!(
        validate_packet_against_contract(&dropping, &contract),
        Err(CloseProofError::Coverage { message }) if message.contains("silently dropped")
    ));
    Ok(())
}

#[test]
fn unknown_row_disposition_is_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut extra = passing_packet(&contract)?;
    extra.row_dispositions.insert(
        "row.not-in-contract".to_string(),
        RowDispositionValue::NotProven { reason: "invented".to_string() },
    );
    assert!(matches!(
        validate_packet_against_contract(&extra, &contract),
        Err(CloseProofError::Coverage { message }) if message.contains("unknown rows")
    ));
    Ok(())
}

#[test]
fn missing_child_coverage_is_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.kind = IssueKind::MultiPhase;
    contract.mandatory_children =
        vec![super::IssueRef { repository: contract.repository.clone(), number: 9000301 }];
    contract.validate()?;
    let mut incomplete = passing_packet(&contract)?;
    incomplete.child_dispositions = Vec::new();
    assert!(matches!(
        validate_packet_against_contract(&incomplete, &contract),
        Err(CloseProofError::Coverage { message }) if message.contains("child")
    ));
    Ok(())
}

#[test]
fn covered_child_satisfies_controller_shape() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.kind = IssueKind::Controller;
    contract.mandatory_children =
        vec![super::IssueRef { repository: contract.repository.clone(), number: 9000301 }];
    contract.validate()?;
    let mut complete = passing_packet(&contract)?;
    complete.child_dispositions = vec![ChildDispositionRecord {
        child: super::IssueRef { repository: contract.repository.clone(), number: 9000301 },
        state: ChildState::ClosedByPacket { packet_subject: "PR #9000401".to_string() },
    }];
    complete.validate_shape()?;
    validate_packet_against_contract(&complete, &contract)?;
    Ok(())
}

#[test]
fn true_duplicate_without_target_is_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut duplicate = passing_packet(&contract)?;
    duplicate.requested_close_mode = CloseMode::TrueDuplicate;
    assert!(matches!(
        validate_packet_against_contract(&duplicate, &contract),
        Err(CloseProofError::Coverage { message }) if message.contains("duplicate target")
    ));
    Ok(())
}

#[test]
fn transfer_without_destination_identity_is_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut transferring = passing_packet(&contract)?;
    transferring.row_dispositions.insert(
        "single-row.defect.fixed".to_string(),
        RowDispositionValue::TransferredToOpenOwner {
            proposition: "the identical defect proposition".to_string(),
            destination_repository: contract.repository.clone(),
            destination_issue: 9000303,
            destination_contract_identity: "not-a-digest".to_string(),
            rationale: "same governing proposition survives in the open owner".to_string(),
        },
    );
    assert!(matches!(
        validate_packet_against_contract(&transferring, &contract),
        Err(CloseProofError::Digest { .. })
    ));
    Ok(())
}

#[test]
fn claim_covering_unknown_row_is_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut claiming = passing_packet(&contract)?;
    claiming.established_claims = vec![ClaimStatement {
        statement: "Claims a row this contract does not own.".to_string(),
        covers_rows: vec!["ghost.row.id".to_string()],
    }];
    assert!(matches!(
        validate_packet_against_contract(&claiming, &contract),
        Err(CloseProofError::Coverage { message }) if message.contains("unknown row")
    ));
    Ok(())
}

#[test]
fn empty_verdict_reasons_are_rejected() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut silent = passing_packet(&contract)?;
    silent.verdict.reasons = Vec::new();
    assert!(matches!(
        validate_packet_against_contract(&silent, &contract),
        Err(CloseProofError::Schema { field, .. }) if field == "verdict.reasons"
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Independent result surfaces and vocabulary semantics
// ---------------------------------------------------------------------------

#[test]
fn pr_scope_pass_and_issue_close_failure_coexist() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let mut mixed = passing_packet(&contract)?;
    mixed.verdict.pr_scope = PrScopeOutcome::Pass;
    mixed.verdict.issue_close = IssueCloseOutcome::Invalid;
    mixed.row_dispositions.insert(
        "single-row.defect.fixed".to_string(),
        RowDispositionValue::NotProven { reason: "bounded slice only".to_string() },
    );
    mixed.verdict.reasons =
        vec!["The bounded slice passes PR scope while the issue close stays invalid.".to_string()];
    validate_packet_against_contract(&mixed, &contract)?;
    Ok(())
}

#[test]
fn disposition_vocabulary_separates_completion_from_not_proven() {
    assert!(
        RowDispositionValue::ProvenCurrentMain {
            evidence: super::EvidenceRef {
                producer: "p".to_string(),
                subject: "s".to_string(),
                content_digest: content_digest_hex(b"x"),
                reference: "r".to_string(),
            },
        }
        .satisfies_completion()
    );
    assert!(
        !RowDispositionValue::NotProven { reason: "unbounded".to_string() }.satisfies_completion()
    );
    assert!(!RowDispositionValue::Contradicted { reason: "c".to_string() }.satisfies_completion());
    assert!(!RowDispositionValue::Stale { reason: "s".to_string() }.satisfies_completion());
}

#[test]
fn proof_levels_never_satisfy_a_stronger_requirement() {
    assert!(ProofLevel::Public.satisfies(ProofLevel::Installed));
    assert!(ProofLevel::Installed.satisfies(ProofLevel::AuthorizedBehavior));
    assert!(ProofLevel::Mechanism.satisfies(ProofLevel::Representation));
    assert!(!ProofLevel::Representation.satisfies(ProofLevel::Public));
    assert!(!ProofLevel::Mechanism.satisfies(ProofLevel::ConnectedRoute));
    assert!(!ProofLevel::AuthorizedBehavior.satisfies(ProofLevel::Cohort));
}

#[test]
fn token_and_repository_gates_discriminate() {
    assert!(is_stable_token("settings.inventory.complete"));
    assert!(is_stable_token("row-1"));
    assert!(!is_stable_token(""));
    assert!(!is_stable_token("Has Space"));
    assert!(!is_stable_token("-leading"));
    assert!(is_repository_id("effortlessmetrics/perl-lsp-swarm"));
    assert!(!is_repository_id("EffortlessMetrics/perl-lsp-swarm"));
    assert!(!is_repository_id("owner/name/extra"));
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

#[test]
fn second_serialization_generation_produces_no_diff() -> Result<(), CloseProofError> {
    let contract = leaf_contract()?;
    let first = contract.to_canonical_json()?;
    let reparsed = IssueContract::from_json_str(&first)?;
    let second = reparsed.to_canonical_json()?;
    assert_eq!(first, second);

    let doc_packet = passing_packet(&contract)?;
    let packet_first = doc_packet.to_canonical_json()?;
    let packet_second = ClosePacket::from_json_str(&packet_first)?.to_canonical_json()?;
    assert_eq!(packet_first, packet_second);
    Ok(())
}

#[test]
fn control_outcome_reasons_must_be_exact() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.negative_controls = vec![super::NegativeControlRow {
        control_id: "nc.must-hold".to_string(),
        guards_row_id: "single-row.defect.fixed".to_string(),
        description: "The repaired delimiter stays rejected.".to_string(),
    }];
    contract.validate()?;
    let mut controlled = passing_packet(&contract)?;
    controlled
        .negative_control_dispositions
        .insert("nc.must-hold".to_string(), ControlOutcome::Failed { reason: String::new() });
    assert!(matches!(
        validate_packet_against_contract(&controlled, &contract),
        Err(CloseProofError::Schema { field, .. }) if field.contains("reason")
    ));
    Ok(())
}
