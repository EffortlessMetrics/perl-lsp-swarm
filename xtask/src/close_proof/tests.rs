//! Discriminating proof for the close-proof schema train (#10380).
//!
//! Corruption rows prove that mis-typed or semantically broken documents fail
//! validation while valid rows pass, and that packets bind current contract
//! identity. The committed regression corpus under `.ci/close-proof-contract/`
//! must verify end to end.

use super::contract::IssueContract;
use super::corpus::{FixtureDocument, ManifestEntry, load_corpus_manifest, verify_corpus_at};
use super::model::{
    ChildDispositionRecord, ChildState, ClaimStatement, CloseMode, ClosePacket, ControlOutcome,
    DenominatorRow, PacketBinding, ProofLevel, RowDispositionValue,
};
use super::{
    CloseProofError, IssueCloseOutcome, IssueKind, PrScopeOutcome, content_digest_hex, corpus_root,
    is_repository_id, is_stable_token, validate_packet_against_contract, verify_corpus,
};

const CORPUS_FIXTURE_COUNT: usize = 15;

// New additive identity protocol. These witnesses remain unchanged between
// the disclosed row-only RED scaffold and full-material GREEN implementation.
#[cfg(test)]
mod full_identity {
    use super::super::{
        CONTRACT_IDENTITY_SCHEMA_V1, ChildRelationDeclaration, ContractIdentityContext,
        ContractIdentityEnvelope, FULL_PACKET_BINDING_SCHEMA_V1, FullBindingMatch, FullBoundPacket,
        IssueRef, NegativeControlRow, RulingDeclaration, SourceIdentity,
        validate_packet_full_binding,
    };
    use super::*;

    fn source(identity: &str, seed: u64) -> SourceIdentity {
        SourceIdentity { identity: identity.to_string(), digest: leaf_digest(seed) }
    }

    fn subject() -> Result<(ContractIdentityEnvelope, FullBoundPacket), CloseProofError> {
        let mut contract = leaf_contract()?;
        contract.denominator.push(DenominatorRow {
            row_id: "neighbor.parses".to_string(),
            statement: "The neighboring valid form still parses.".to_string(),
            required_proof_level: ProofLevel::Mechanism,
        });
        contract.identity.denominator_digest =
            super::super::compute_denominator_digest(&contract.denominator)?;
        contract.negative_controls = vec![
            NegativeControlRow {
                control_id: "nc.reject".to_string(),
                guards_row_id: "single-row.defect.fixed".to_string(),
                description: "Reject malformed input.".to_string(),
            },
            NegativeControlRow {
                control_id: "nc.neighbor".to_string(),
                guards_row_id: "neighbor.parses".to_string(),
                description: "Preserve valid neighbors.".to_string(),
            },
        ];
        contract.mandatory_children = vec![
            IssueRef { repository: contract.repository.clone(), number: 9000500 },
            IssueRef { repository: contract.repository.clone(), number: 9000501 },
        ];
        contract.transfer_policy.permitted = true;
        contract.transfer_policy.conditions =
            vec!["Exact open owner.".to_string(), "Reviewed transfer.".to_string()];
        contract.domain_evidence_refs = vec!["domain.v1".to_string(), "domain.v2".to_string()];
        let context = ContractIdentityContext {
            child_relations: contract
                .mandatory_children
                .iter()
                .map(|child| ChildRelationDeclaration {
                    child: child.clone(),
                    relation_class: "mandatory-phase".to_string(),
                })
                .collect(),
            permitted_transferred_rows: contract
                .row_ids()
                .into_iter()
                .map(str::to_string)
                .collect(),
            linked_authorities: vec![source("linked-a", 10), source("linked-b", 11)],
            accepted_rulings: vec![
                RulingDeclaration {
                    ruling_type: "reviewed-ruling".to_string(),
                    source: source("ruling-a", 20),
                    review: source("review-a", 21),
                    supersedes: vec![source("old-a", 22), source("old-b", 23)],
                    changed_propositions: vec![
                        "Require current proof.".to_string(),
                        "Retain the neighbor.".to_string(),
                    ],
                },
                RulingDeclaration {
                    ruling_type: "reviewed-ruling".to_string(),
                    source: source("ruling-b", 24),
                    review: source("review-b", 25),
                    supersedes: vec![],
                    changed_propositions: vec!["Retain children.".to_string()],
                },
            ],
            compiler_generation: source("compiler-1", 30),
            schema_generation: source("schema-1", 31),
            policy_generation: source("policy-1", 32),
            limitations: vec![
                "Authority unverified.".to_string(),
                "Evidence not admitted.".to_string(),
            ],
            adoption_record: Some(source("candidate-adoption", 33)),
        };
        let mut envelope = ContractIdentityEnvelope {
            schema_version: CONTRACT_IDENTITY_SCHEMA_V1.to_string(),
            contract,
            context,
            full_contract_digest: String::new(),
        };
        reseal(&mut envelope)?;
        let mut packet = passing_packet(&envelope.contract)?;
        packet.row_dispositions.insert(
            "neighbor.parses".to_string(),
            RowDispositionValue::NotProven {
                reason: "No independently admitted neighbor evidence.".to_string(),
            },
        );
        for control in &envelope.contract.negative_controls {
            packet
                .negative_control_dispositions
                .insert(control.control_id.clone(), ControlOutcome::Verified);
        }
        packet.child_dispositions = envelope
            .contract
            .mandatory_children
            .iter()
            .map(|child| ChildDispositionRecord {
                child: child.clone(),
                state: ChildState::StillOpen,
            })
            .collect();
        let bound = FullBoundPacket {
            schema_version: FULL_PACKET_BINDING_SCHEMA_V1.to_string(),
            packet,
            full_contract_digest: envelope.full_contract_digest.clone(),
        };
        assert_eq!(
            validate_packet_full_binding(&bound, &envelope)?,
            FullBindingMatch::RepresentationOnly
        );
        Ok((envelope, bound))
    }

    fn reseal(envelope: &mut ContractIdentityEnvelope) -> Result<(), CloseProofError> {
        envelope.contract.identity.denominator_digest =
            super::super::compute_denominator_digest(&envelope.contract.denominator)?;
        envelope.full_contract_digest = envelope.compute_full_contract_digest()?;
        envelope.validate()
    }

    fn assert_movements(
        base: &ContractIdentityEnvelope,
        packet: &FullBoundPacket,
        changes: Vec<(&str, ContractIdentityEnvelope)>,
    ) -> Result<(), CloseProofError> {
        let mut missed = Vec::new();
        for (name, mut current) in changes {
            reseal(&mut current)?;
            assert_ne!(current, *base, "mutation did not change material: {name}");
            let changed = current.full_contract_digest != base.full_contract_digest;
            let refused = matches!(
                validate_packet_full_binding(packet, &current),
                Err(CloseProofError::Identity { .. })
            );
            eprintln!("FULL_IDENTITY {name}: identity_changed={changed} stale_refused={refused}");
            if !changed || !refused {
                missed.push(name);
            }
            assert_eq!(
                validate_packet_full_binding(packet, base)?,
                FullBindingMatch::RepresentationOnly,
                "restoration control: {name}"
            );
        }
        assert!(missed.is_empty(), "full identity omitted load-bearing inputs: {missed:?}");
        Ok(())
    }

    macro_rules! movements {
        ($base:ident; $($name:literal => $change:expr),+ $(,)?) => {{
            let mut changes = Vec::new();
            $(let mut value = $base.clone(); ($change)(&mut value); changes.push(($name, value));)+
            changes
        }};
    }

    #[test]
    fn red_full_identity_binds_issue_and_policy_mutation_pairs() -> Result<(), CloseProofError> {
        let (base, packet) = subject()?;
        let changes = movements!(base;
            "repository" => |e: &mut ContractIdentityEnvelope| e.contract.repository = "other/repository".to_string(),
            "issue-number" => |e: &mut ContractIdentityEnvelope| e.contract.issue_number += 1,
            "title" => |e: &mut ContractIdentityEnvelope| e.contract.title.push_str(" with stronger scope"),
            "kind" => |e: &mut ContractIdentityEnvelope| e.contract.kind = IssueKind::Installed,
            "issue-proof" => |e: &mut ContractIdentityEnvelope| e.contract.required_proof_level = ProofLevel::Public,
            "mode-removal" => |e: &mut ContractIdentityEnvelope| e.contract.allowed_close_modes.retain(|m| *m != CloseMode::NotPlanned),
            "mode-addition" => |e: &mut ContractIdentityEnvelope| e.contract.allowed_close_modes.push(CloseMode::ControllerComplete),
            "transfer-disabled" => |e: &mut ContractIdentityEnvelope| { e.contract.transfer_policy.permitted = false; e.context.permitted_transferred_rows.clear(); },
            "transfer-condition" => |e: &mut ContractIdentityEnvelope| e.contract.transfer_policy.conditions[0] = "Exact independently adopted open owner.".to_string(),
            "transfer-condition-tail" => |e: &mut ContractIdentityEnvelope| e.contract.transfer_policy.conditions[1] = "Review by the independent owner.".to_string(),
            "transfer-condition-addition" => |e: &mut ContractIdentityEnvelope| e.contract.transfer_policy.conditions.push("Require adoption provenance.".to_string()),
            "transfer-condition-removal" => |e: &mut ContractIdentityEnvelope| { e.contract.transfer_policy.conditions.pop(); },
            "transfer-row-removal" => |e: &mut ContractIdentityEnvelope| { e.context.permitted_transferred_rows.pop(); },
            "domain-reference" => |e: &mut ContractIdentityEnvelope| e.contract.domain_evidence_refs[0] = "domain.v3".to_string(),
            "domain-reference-tail" => |e: &mut ContractIdentityEnvelope| e.contract.domain_evidence_refs[1] = "domain.v4".to_string(),
            "domain-reference-addition" => |e: &mut ContractIdentityEnvelope| e.contract.domain_evidence_refs.push("domain.v5".to_string()),
            "domain-reference-removal" => |e: &mut ContractIdentityEnvelope| { e.contract.domain_evidence_refs.pop(); },
            "body-digest" => |e: &mut ContractIdentityEnvelope| e.contract.identity.issue_body_digest = leaf_digest(99),
            "limitations" => |e: &mut ContractIdentityEnvelope| e.context.limitations[0] = "Authority only partially observed.".to_string(),
            "limitations-tail" => |e: &mut ContractIdentityEnvelope| e.context.limitations[1] = "Evidence missing from independent owner.".to_string(),
            "limitations-addition" => |e: &mut ContractIdentityEnvelope| e.context.limitations.push("No semantic verdict derived.".to_string()),
            "limitations-removal" => |e: &mut ContractIdentityEnvelope| { e.context.limitations.pop(); },
            "adoption-identity" => |e: &mut ContractIdentityEnvelope| e.context.adoption_record = Some(source("self-authored-adoption", 33)),
            "adoption-digest" => |e: &mut ContractIdentityEnvelope| e.context.adoption_record = Some(source("candidate-adoption", 34)),
            "adoption-removal" => |e: &mut ContractIdentityEnvelope| e.context.adoption_record = None,
        );
        assert_movements(&base, &packet, changes)
    }

    #[test]
    fn red_full_identity_binds_row_control_and_child_mutation_pairs() -> Result<(), CloseProofError>
    {
        let (base, packet) = subject()?;
        let changes = movements!(base;
            "row-statement" => |e: &mut ContractIdentityEnvelope| e.contract.denominator[0].statement.push_str(" on the installed route"),
            "row-proof" => |e: &mut ContractIdentityEnvelope| e.contract.denominator[0].required_proof_level = ProofLevel::Installed,
            "row-addition" => |e: &mut ContractIdentityEnvelope| e.contract.denominator.push(DenominatorRow { row_id: "new.row".to_string(), statement: "A new explicit proposition.".to_string(), required_proof_level: ProofLevel::Mechanism }),
            "row-removal" => |e: &mut ContractIdentityEnvelope| { e.contract.denominator.pop(); e.contract.negative_controls.pop(); e.context.permitted_transferred_rows.retain(|r| r != "neighbor.parses"); },
            "row-same-count-replacement" => |e: &mut ContractIdentityEnvelope| { e.contract.denominator[0].row_id = "replacement".to_string(); e.contract.negative_controls[0].guards_row_id = "replacement".to_string(); for id in &mut e.context.permitted_transferred_rows { if *id == "single-row.defect.fixed" { *id = "replacement".to_string(); } } },
            "control-description" => |e: &mut ContractIdentityEnvelope| e.contract.negative_controls[0].description.push_str(" at the public boundary"),
            "control-association" => |e: &mut ContractIdentityEnvelope| e.contract.negative_controls[0].guards_row_id = "neighbor.parses".to_string(),
            "control-same-count-replacement" => |e: &mut ContractIdentityEnvelope| e.contract.negative_controls[0].control_id = "nc.replacement".to_string(),
            "control-removal" => |e: &mut ContractIdentityEnvelope| { e.contract.negative_controls.pop(); },
            "control-addition" => |e: &mut ContractIdentityEnvelope| { let mut c = e.contract.negative_controls[0].clone(); c.control_id = "nc.addition".to_string(); e.contract.negative_controls.push(c); },
            "child-relation-class" => |e: &mut ContractIdentityEnvelope| e.context.child_relations[0].relation_class = "mandatory-terminal".to_string(),
            "child-same-count-replacement" => |e: &mut ContractIdentityEnvelope| { e.contract.mandatory_children[0].number += 2; e.context.child_relations[0].child.number += 2; },
            "child-repository" => |e: &mut ContractIdentityEnvelope| { e.contract.mandatory_children[0].repository = "other/child".to_string(); e.context.child_relations[0].child.repository = "other/child".to_string(); },
            "child-removal" => |e: &mut ContractIdentityEnvelope| { e.contract.mandatory_children.pop(); e.context.child_relations.pop(); },
            "child-addition" => |e: &mut ContractIdentityEnvelope| { let c = IssueRef { repository: e.contract.repository.clone(), number: 9000503 }; e.contract.mandatory_children.push(c.clone()); e.context.child_relations.push(ChildRelationDeclaration { child: c, relation_class: "mandatory-phase".to_string() }); },
        );
        assert_movements(&base, &packet, changes)
    }

    #[test]
    fn red_full_identity_binds_ruling_authority_and_generation_mutation_pairs()
    -> Result<(), CloseProofError> {
        let (base, packet) = subject()?;
        let changes = movements!(base;
            "linked-identity" => |e: &mut ContractIdentityEnvelope| e.context.linked_authorities[0].identity = "linked-c".to_string(),
            "linked-digest" => |e: &mut ContractIdentityEnvelope| e.context.linked_authorities[0].digest = leaf_digest(40),
            "linked-removal" => |e: &mut ContractIdentityEnvelope| { e.context.linked_authorities.pop(); },
            "linked-addition" => |e: &mut ContractIdentityEnvelope| e.context.linked_authorities.push(source("linked-c", 40)),
            "ruling-type" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].ruling_type = "reviewed-exception".to_string(),
            "ruling-source-identity" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].source.identity = "ruling-c".to_string(),
            "ruling-source-digest" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].source.digest = leaf_digest(41),
            "ruling-review-identity" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].review.identity = "review-c".to_string(),
            "ruling-review-digest" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].review.digest = leaf_digest(42),
            "later-ruling-source-digest" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[1].source.digest = leaf_digest(50),
            "later-ruling-review-identity" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[1].review.identity = "later-review".to_string(),
            "later-ruling-review-digest" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[1].review.digest = leaf_digest(51),
            "later-ruling-proposition" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[1].changed_propositions[0] = "Retain all mandatory children.".to_string(),
            "later-ruling-supersedes" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[1].supersedes.push(source("older-later-ruling", 52)),
            "ruling-superseded-identity" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].supersedes[0].identity = "old-c".to_string(),
            "ruling-superseded-digest" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].supersedes[0].digest = leaf_digest(43),
            "ruling-superseded-tail" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].supersedes[1].digest = leaf_digest(48),
            "ruling-superseded-addition" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].supersedes.push(source("old-c", 49)),
            "ruling-superseded-removal" => |e: &mut ContractIdentityEnvelope| { e.context.accepted_rulings[0].supersedes.pop(); },
            "ruling-proposition" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].changed_propositions[0] = "Require independently accepted current proof.".to_string(),
            "ruling-proposition-tail" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].changed_propositions[1] = "Retain the valid installed neighbor.".to_string(),
            "ruling-proposition-addition" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].changed_propositions.push("Retain every negative control.".to_string()),
            "ruling-proposition-removal" => |e: &mut ContractIdentityEnvelope| { e.context.accepted_rulings[0].changed_propositions.pop(); },
            "ruling-removal" => |e: &mut ContractIdentityEnvelope| { e.context.accepted_rulings.pop(); },
            "ruling-addition" => |e: &mut ContractIdentityEnvelope| { let mut r = e.context.accepted_rulings[0].clone(); r.source = source("ruling-c", 44); e.context.accepted_rulings.push(r); },
            "compiler-identity" => |e: &mut ContractIdentityEnvelope| e.context.compiler_generation.identity = "compiler-2".to_string(),
            "compiler-digest" => |e: &mut ContractIdentityEnvelope| e.context.compiler_generation.digest = leaf_digest(45),
            "schema-generation-identity" => |e: &mut ContractIdentityEnvelope| e.context.schema_generation.identity = "schema-2".to_string(),
            "schema-generation-digest" => |e: &mut ContractIdentityEnvelope| e.context.schema_generation.digest = leaf_digest(46),
            "policy-identity" => |e: &mut ContractIdentityEnvelope| e.context.policy_generation.identity = "policy-2".to_string(),
            "policy-digest" => |e: &mut ContractIdentityEnvelope| e.context.policy_generation.digest = leaf_digest(47),
            "legacy-ruling-projection" => |e: &mut ContractIdentityEnvelope| e.contract.identity.accepted_ruling = Some(super::super::RulingIdentity { identity: "ruling-a".to_string(), digest: leaf_digest(20) }),
        );
        assert_movements(&base, &packet, changes)
    }

    #[test]
    fn control_full_identity_unordered_permutations_and_round_trips() -> Result<(), CloseProofError>
    {
        let (base, packet) = subject()?;
        let mut equivalent = base.clone();
        equivalent.contract.allowed_close_modes.reverse();
        equivalent.contract.denominator.reverse();
        equivalent.contract.negative_controls.reverse();
        equivalent.contract.mandatory_children.reverse();
        equivalent.contract.transfer_policy.conditions.reverse();
        equivalent.contract.domain_evidence_refs.reverse();
        equivalent.context.child_relations.reverse();
        equivalent.context.permitted_transferred_rows.reverse();
        equivalent.context.linked_authorities.reverse();
        equivalent.context.accepted_rulings.reverse();
        for ruling in &mut equivalent.context.accepted_rulings {
            ruling.supersedes.reverse();
            ruling.changed_propositions.reverse();
        }
        equivalent.context.limitations.reverse();
        reseal(&mut equivalent)?;
        assert_eq!(equivalent.full_contract_digest, base.full_contract_digest);
        assert_eq!(equivalent.to_canonical_json()?, base.to_canonical_json()?);
        assert_eq!(
            validate_packet_full_binding(&packet, &equivalent)?,
            FullBindingMatch::RepresentationOnly
        );
        let decoded = ContractIdentityEnvelope::from_json_str(&equivalent.to_canonical_json()?)?;
        decoded.validate()?;
        let decoded_packet = FullBoundPacket::from_json_str(&packet.to_canonical_json()?)?;
        assert_eq!(
            validate_packet_full_binding(&decoded_packet, &decoded)?,
            FullBindingMatch::RepresentationOnly
        );
        Ok(())
    }

    #[test]
    fn control_full_identity_rejects_duplicate_semantic_sets_and_dangling_context()
    -> Result<(), CloseProofError> {
        let (base, _) = subject()?;
        let mut changes = movements!(base;
            "duplicate-condition" => |e: &mut ContractIdentityEnvelope| e.contract.transfer_policy.conditions.push(e.contract.transfer_policy.conditions[0].clone()),
            "duplicate-domain" => |e: &mut ContractIdentityEnvelope| e.contract.domain_evidence_refs.push(e.contract.domain_evidence_refs[0].clone()),
            "duplicate-transfer-row" => |e: &mut ContractIdentityEnvelope| e.context.permitted_transferred_rows.push(e.context.permitted_transferred_rows[0].clone()),
            "duplicate-child-relation" => |e: &mut ContractIdentityEnvelope| { let mut c = e.context.child_relations[0].clone(); c.relation_class = "conflicting".to_string(); e.context.child_relations.push(c); },
            "duplicate-linked-identity" => |e: &mut ContractIdentityEnvelope| { let mut c = e.context.linked_authorities[0].clone(); c.digest = leaf_digest(80); e.context.linked_authorities.push(c); },
            "duplicate-ruling-identity" => |e: &mut ContractIdentityEnvelope| { let mut c = e.context.accepted_rulings[0].clone(); c.source.digest = leaf_digest(81); e.context.accepted_rulings.push(c); },
            "duplicate-superseded-identity" => |e: &mut ContractIdentityEnvelope| { let mut c = e.context.accepted_rulings[0].supersedes[0].clone(); c.digest = leaf_digest(82); e.context.accepted_rulings[0].supersedes.push(c); },
            "duplicate-proposition" => |e: &mut ContractIdentityEnvelope| { let c = e.context.accepted_rulings[0].changed_propositions[0].clone(); e.context.accepted_rulings[0].changed_propositions.push(c); },
            "duplicate-limitation" => |e: &mut ContractIdentityEnvelope| e.context.limitations.push(e.context.limitations[0].clone()),
            "unknown-transfer-row" => |e: &mut ContractIdentityEnvelope| e.context.permitted_transferred_rows.push("unknown".to_string()),
            "missing-child-relation" => |e: &mut ContractIdentityEnvelope| { e.context.child_relations.pop(); },
            "unknown-child-relation" => |e: &mut ContractIdentityEnvelope| e.context.child_relations[0].child.number += 100,
            "legacy-ruling-conflict" => |e: &mut ContractIdentityEnvelope| e.contract.identity.accepted_ruling = Some(super::super::RulingIdentity { identity: "ruling-a".to_string(), digest: leaf_digest(83) }),
            "disabled-transfer-rows" => |e: &mut ContractIdentityEnvelope| e.contract.transfer_policy.permitted = false,
            "malformed-generation-digest" => |e: &mut ContractIdentityEnvelope| e.context.policy_generation.digest = "BAD".to_string(),
            "blank-source" => |e: &mut ContractIdentityEnvelope| e.context.linked_authorities[0].identity.clear(),
            "ambiguous-source-whitespace" => |e: &mut ContractIdentityEnvelope| e.context.compiler_generation.identity.push(' '),
            "empty-ruling-propositions" => |e: &mut ContractIdentityEnvelope| e.context.accepted_rulings[0].changed_propositions.clear(),
        );
        for (name, e) in changes.drain(..) {
            assert!(
                e.compute_full_contract_digest().is_err(),
                "ambiguous/dangling material accepted: {name}"
            );
        }
        Ok(())
    }

    #[test]
    fn control_full_identity_strict_raw_intake_and_explicit_metadata() -> Result<(), CloseProofError>
    {
        let (base, packet) = subject()?;
        let raw = base.to_canonical_json()?;
        let poisoned = raw.replacen("\"context\": {", "\"context\": {\"injected\": true,", 1);
        assert!(matches!(
            ContractIdentityEnvelope::from_json_str(&poisoned),
            Err(CloseProofError::Schema { .. })
        ));
        let duplicate = raw.replacen(
            "\"policy_generation\": {",
            "\"policy_generation\": {\"identity\": \"shadow\",",
            1,
        );
        assert!(
            matches!(ContractIdentityEnvelope::from_json_str(&duplicate), Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate"))
        );
        let escaped =
            duplicate.replacen("\"identity\": \"shadow\"", "\"\\u0069dentity\": \"shadow\"", 1);
        assert!(
            matches!(ContractIdentityEnvelope::from_json_str(&escaped), Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate"))
        );
        let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            CloseProofError::Schema { field: "test".to_string(), message: e.to_string() }
        })?;
        for field in [
            "child_relations",
            "permitted_transferred_rows",
            "linked_authorities",
            "accepted_rulings",
            "compiler_generation",
            "schema_generation",
            "policy_generation",
            "limitations",
            "adoption_record",
        ] {
            let mut missing = value.clone();
            if let Some(context) =
                missing.get_mut("context").and_then(serde_json::Value::as_object_mut)
            {
                context.remove(field);
            }
            assert!(
                matches!(
                    ContractIdentityEnvelope::from_json_str(&missing.to_string()),
                    Err(CloseProofError::Schema { .. })
                ),
                "missing context field accepted: {field}"
            );
        }
        value["context"]["accepted_rulings"][0]["injected"] = serde_json::Value::Bool(true);
        assert!(matches!(
            ContractIdentityEnvelope::from_json_str(&value.to_string()),
            Err(CloseProofError::Schema { .. })
        ));
        let raw_packet = packet.to_canonical_json()?;
        let payload = raw_packet.replacen(
            "\"state\": \"still_open\"",
            "\"state\": \"still_open\", \"injected\": true",
            1,
        );
        assert!(matches!(
            FullBoundPacket::from_json_str(&payload),
            Err(CloseProofError::Schema { .. })
        ));
        let duplicate_packet = raw_packet.replacen(
            "\"full_contract_digest\":",
            &format!(
                "\"full_contract_digest\": \"{}\", \"full_contract_digest\":",
                leaf_digest(88)
            ),
            1,
        );
        assert!(
            matches!(FullBoundPacket::from_json_str(&duplicate_packet), Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate"))
        );
        assert!(
            ContractIdentityEnvelope::from_json_str(&base.contract.to_canonical_json()?).is_err()
        );
        assert!(FullBoundPacket::from_json_str(&packet.packet.to_canonical_json()?).is_err());
        Ok(())
    }

    #[test]
    fn control_full_identity_forged_digests_versions_and_representation_ceiling()
    -> Result<(), CloseProofError> {
        let (base, packet) = subject()?;
        let mut forged = base.clone();
        forged.full_contract_digest = leaf_digest(999);
        assert!(matches!(forged.validate(), Err(CloseProofError::Digest { .. })));
        let mut wrong = base.clone();
        wrong.schema_version = "issue_contract_identity.v2".to_string();
        assert!(matches!(wrong.validate(), Err(CloseProofError::Schema { .. })));
        let mut wrong_packet = packet.clone();
        wrong_packet.schema_version = "issue_close_proof_binding.v2".to_string();
        assert!(matches!(
            validate_packet_full_binding(&wrong_packet, &base),
            Err(CloseProofError::Schema { .. })
        ));
        wrong_packet.schema_version = FULL_PACKET_BINDING_SCHEMA_V1.to_string();
        wrong_packet.full_contract_digest = "BAD".to_string();
        assert!(matches!(
            validate_packet_full_binding(&wrong_packet, &base),
            Err(CloseProofError::Digest { .. })
        ));
        // Self-authored adoption and a supplied Valid verdict already occur in
        // the subject. A matching recomputation still cannot establish trust.
        assert_eq!(
            validate_packet_full_binding(&packet, &base)?,
            FullBindingMatch::RepresentationOnly
        );
        let mut phase = packet.clone();
        phase.packet.requested_close_mode = CloseMode::PhaseCompleteIssueRemainsOpen;
        assert_eq!(phase.packet.verdict.issue_close, IssueCloseOutcome::Valid);
        assert_eq!(
            validate_packet_full_binding(&phase, &base)?,
            FullBindingMatch::RepresentationOnly
        );
        Ok(())
    }

    #[test]
    fn red_candidate_recomputation_cannot_replace_separately_supplied_current_binding()
    -> Result<(), CloseProofError> {
        let (current, packet) = subject()?;
        let mut candidate = current.clone();
        candidate.contract.required_proof_level = ProofLevel::Representation;
        candidate.context.adoption_record = Some(source("candidate-claims-adoption", 900));
        reseal(&mut candidate)?;
        let mut rebound = packet.clone();
        rebound.full_contract_digest = candidate.full_contract_digest.clone();
        assert_eq!(
            validate_packet_full_binding(&rebound, &candidate)?,
            FullBindingMatch::RepresentationOnly
        );
        assert!(matches!(
            validate_packet_full_binding(&rebound, &current),
            Err(CloseProofError::Identity { .. })
        ));
        Ok(())
    }
}

// Adversarial qualification under #10414/#10415. These tests exercise the
// existing Rust intake, never a second decoder or a semantic evaluator.
// The red_* names retain the sealed pre-fix falsifiers, now required GREEN.
// Diagnostics preserve representation-only boundaries, never an evaluator.
#[cfg(test)]
mod qualification {
    use super::*;

    fn replace_once(raw: &str, from: &str, to: &str) -> String {
        assert_eq!(raw.matches(from).count(), 1, "mutation anchor must be unique: {from}");
        raw.replacen(from, to, 1)
    }

    fn rich_subject() -> Result<(IssueContract, ClosePacket), CloseProofError> {
        let mut contract = leaf_contract()?;
        contract.denominator.push(DenominatorRow {
            row_id: "neighbor.parses".to_string(),
            statement: "The neighboring valid form still parses.".to_string(),
            required_proof_level: ProofLevel::Mechanism,
        });
        contract.identity.denominator_digest =
            super::super::compute_denominator_digest(&contract.denominator)?;
        contract.negative_controls.push(super::super::NegativeControlRow {
            control_id: "nc.repair".to_string(),
            guards_row_id: "single-row.defect.fixed".to_string(),
            description: "The malformed delimiter is rejected.".to_string(),
        });
        contract.mandatory_children.push(super::super::IssueRef {
            repository: contract.repository.clone(),
            number: 9000500,
        });
        let mut packet = passing_packet(&contract)?;
        packet.row_dispositions.insert(
            "neighbor.parses".to_string(),
            RowDispositionValue::NotProven { reason: "Neighbor remains untested.".to_string() },
        );
        packet
            .negative_control_dispositions
            .insert("nc.repair".to_string(), ControlOutcome::Verified);
        packet.child_dispositions.push(ChildDispositionRecord {
            child: contract.mandatory_children[0].clone(),
            state: ChildState::ClosedByPacket { packet_subject: "child-packet".to_string() },
        });
        validate_packet_against_contract(&packet, &contract)?;
        Ok((contract, packet))
    }

    fn duplicate_map_entry(
        packet: &ClosePacket,
        field: &str,
        key: &str,
        value: &str,
    ) -> Result<String, CloseProofError> {
        let raw = packet.to_canonical_json()?;
        let anchor = format!("\"{field}\": {{");
        Ok(replace_once(&raw, &anchor, &format!("{anchor}\n    \"{key}\": {value},")))
    }

    fn require_schema_rejections(cases: Vec<(&str, String)>) {
        let mut accepted = Vec::new();
        for (name, raw) in cases {
            match ClosePacket::from_json_str(&raw) {
                Err(CloseProofError::Schema { field, message }) => {
                    assert!(message.contains("duplicate"), "wrong refusal for {name}: {message}");
                    eprintln!("QUALIFICATION reject {name}: {field}: {message}");
                }
                other => {
                    eprintln!("QUALIFICATION expected Schema, observed {name}: {other:?}");
                    accepted.push(name);
                }
            }
        }
        assert!(accepted.is_empty(), "authoritative intake accepted ambiguous cases: {accepted:?}");
    }

    fn is_unknown_field_refusal<T>(result: &Result<T, serde_json::Error>) -> bool {
        match result {
            Ok(_) => false,
            Err(error) => {
                let message = error.to_string();
                assert!(
                    error.is_data()
                        && message.contains("unknown field")
                        && message.contains("injected"),
                    "wrong payload refusal: {message}"
                );
                true
            }
        }
    }

    fn require_clean_wire_round_trip<T>(raw: &str)
    where
        T: serde::de::DeserializeOwned + serde::Serialize,
    {
        let parsed = serde_json::from_str::<T>(raw);
        assert!(parsed.is_ok(), "clean variant must parse: {raw}");
        let encoded = parsed.and_then(serde_json::to_value);
        assert!(encoded.is_ok(), "clean variant must serialize: {raw}");
        assert_eq!(encoded.ok(), serde_json::from_str::<serde_json::Value>(raw).ok());
    }

    fn fixture_subject() -> Result<FixtureDocument, CloseProofError> {
        let (contract, packet) = rich_subject()?;
        Ok(FixtureDocument {
            schema_version: super::super::FIXTURE_SCHEMA_V1.to_string(),
            provenance: super::super::FixtureProvenance {
                captured_at: "2026-10-05T00:00:00Z".to_string(),
                sources: Vec::new(),
                subject_shas: Vec::new(),
                boundary: "Synthetic representation-only falsifier; no admitted semantic evidence."
                    .to_string(),
            },
            contract,
            cases: vec![super::super::FixtureCase {
                case_id: "self-asserted.verdict".to_string(),
                description: "Unproven neighbor row remains.".to_string(),
                expected_pr_scope: packet.verdict.pr_scope,
                expected_issue_close: packet.verdict.issue_close,
                packet,
            }],
        })
    }

    #[test]
    fn red_raw_map_duplicates_must_be_rejected_before_value_conversion()
    -> Result<(), CloseProofError> {
        let (_, mut packet) = rich_subject()?;
        let proven =
            super::super::canonical_json(&packet.row_dispositions["single-row.defect.fixed"])?;
        let not_proven = r#"{"disposition":"not_proven","reason":"earlier unproven result"}"#;
        let forward = duplicate_map_entry(
            &packet,
            "row_dispositions",
            "single-row.defect.fixed",
            not_proven,
        )?;
        let escaped = duplicate_map_entry(
            &packet,
            "row_dispositions",
            r"single-row.defect.\u0066ixed",
            not_proven,
        )?;
        packet.row_dispositions.insert(
            "single-row.defect.fixed".to_string(),
            RowDispositionValue::NotProven { reason: "later unproven result".to_string() },
        );
        let reverse =
            duplicate_map_entry(&packet, "row_dispositions", "single-row.defect.fixed", &proven)?;
        let control_forward = duplicate_map_entry(
            &packet,
            "negative_control_dispositions",
            "nc.repair",
            r#"{"state":"failed","reason":"hidden failure"}"#,
        )?;
        packet.negative_control_dispositions.insert(
            "nc.repair".to_string(),
            ControlOutcome::Failed { reason: "later failure".to_string() },
        );
        let control_reverse = duplicate_map_entry(
            &packet,
            "negative_control_dispositions",
            "nc.repair",
            r#"{"state":"verified"}"#,
        )?;
        let same_value = duplicate_map_entry(
            &packet,
            "row_dispositions",
            "single-row.defect.fixed",
            &super::super::canonical_json(&packet.row_dispositions["single-row.defect.fixed"])?,
        )?;
        require_schema_rejections(vec![
            ("row-unproven-then-proven", forward),
            ("row-proven-then-unproven", reverse),
            ("escaped-equivalent-row-key", escaped),
            ("control-failed-then-verified", control_forward),
            ("control-verified-then-failed", control_reverse),
            ("repeated-row-key", same_value),
        ]);
        Ok(())
    }

    #[test]
    fn control_raw_maps_refuse_hidden_contradictions_before_normalization()
    -> Result<(), CloseProofError> {
        let (_, packet) = rich_subject()?;
        let raw = duplicate_map_entry(
            &packet,
            "row_dispositions",
            "single-row.defect.fixed",
            r#"{"disposition":"contradicted","reason":"discarded contradiction"}"#,
        )?;
        assert!(matches!(
            ClosePacket::from_json_str(&raw),
            Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate field")
        ));
        let raw = duplicate_map_entry(
            &packet,
            "negative_control_dispositions",
            "nc.repair",
            r#"{"state":"failed","reason":"discarded control"}"#,
        )?;
        assert!(matches!(
            ClosePacket::from_json_str(&raw),
            Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate field")
        ));
        Ok(())
    }

    #[test]
    fn control_all_public_document_intakes_refuse_raw_duplicates() -> Result<(), CloseProofError> {
        let fixture = fixture_subject()?;
        let contract_raw = fixture.contract.to_canonical_json()?;
        IssueContract::from_json_str(&contract_raw)?.validate()?;
        let packet_raw = fixture.cases[0].packet.to_canonical_json()?;
        let packet = ClosePacket::from_json_str(&packet_raw)?;
        validate_packet_against_contract(&packet, &fixture.contract)?;
        let fixture_raw = fixture.to_canonical_json()?;
        FixtureDocument::from_json_str(&fixture_raw)?.verify()?;
        let manifest_raw = super::super::canonical_json(&load_corpus_manifest()?)?;
        super::super::CorpusManifest::from_json_str(&manifest_raw)?;

        let contract_raw = replace_once(
            &contract_raw,
            "\"issue_number\": 9000300",
            "\"issue_number\": 9000300, \"issue_number\": 9000300",
        );
        let packet_raw = duplicate_map_entry(
            &packet,
            "row_dispositions",
            "single-row.defect.fixed",
            r#"{"disposition":"not_proven","reason":"hidden result"}"#,
        )?;
        // Reach a free-key map nested inside a fixture, not merely a duplicate
        // typed field that serde would already refuse without the wire pass.
        let fixture_raw = replace_once(
            &fixture_raw,
            "\"row_dispositions\": {",
            "\"row_dispositions\": {\"single-row.defect.fixed\": {\"disposition\":\"not_proven\",\"reason\":\"hidden result\"},",
        );
        // Executable wrong-route control: direct typed serde intake still
        // loses a repeated free-map key. The public fixture parser must refuse
        // the original raw bytes before that loss can occur.
        assert!(serde_json::from_str::<FixtureDocument>(&fixture_raw).is_ok());
        let manifest_raw = replace_once(
            &manifest_raw,
            "\"corpus_id\":",
            "\"corpus_id\": \"hidden-corpus\", \"corpus_id\":",
        );
        for (field, result) in [
            ("issue_contract", IssueContract::from_json_str(&contract_raw).map(|_| ())),
            ("close_packet", ClosePacket::from_json_str(&packet_raw).map(|_| ())),
            (
                "close_proof_contract_fixture",
                FixtureDocument::from_json_str(&fixture_raw).map(|_| ()),
            ),
            (
                "corpus_manifest",
                super::super::CorpusManifest::from_json_str(&manifest_raw).map(|_| ()),
            ),
        ] {
            assert!(
                matches!(result, Err(CloseProofError::Schema { field: actual, message })
                    if actual == field && message.contains("duplicate field")),
                "expected duplicate-key Schema at {field}"
            );
        }
        Ok(())
    }

    #[test]
    fn control_wire_keys_cover_nested_arrays_escapes_and_valid_json_values()
    -> Result<(), CloseProofError> {
        let raw = r#"{"a":1,"nested":[{"a":2},{"a":3}],"text":"{\"a\":1,\"a\":2}","values":[null,true,false,-1,18446744073709551615,1.25],"escaped":{"\u0062":false}}"#;
        let parsed = super::super::wire::from_json_str::<serde_json::Value>(raw, "wire-control")?;
        assert_eq!(Some(parsed), serde_json::from_str::<serde_json::Value>(raw).ok());
        for raw in [
            r#"{"array":[{"key":1,"key":2}]}"#,
            r#"{"unused":{"a\"b":true,"a\u0022b":false}}"#,
            r#"{"parent":{"child":{"\u0061":null,"a":null}}}"#,
        ] {
            assert!(matches!(
                super::super::wire::from_json_str::<serde_json::Value>(raw, "wire-control"),
                Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate field")
            ));
            // A Value-first implementation would accept and normalize each
            // witness. This independent wrong route must stay distinguishable.
            assert!(serde_json::from_str::<serde_json::Value>(raw).is_ok());
        }
        for raw in ["{} {}", "[", r#"{"key":}"#] {
            assert!(matches!(
                super::super::wire::from_json_str::<serde_json::Value>(raw, "wire-control"),
                Err(CloseProofError::Schema { .. })
            ));
        }
        Ok(())
    }

    #[test]
    fn red_unknown_payload_fields_must_be_rejected_for_every_variant() -> Result<(), CloseProofError>
    {
        let (_, packet) = rich_subject()?;
        let row_variants = [
            r#"{"disposition":"proven_current_main","evidence":{"producer":"x","subject":"s","content_digest":"d","reference":"r","schema_version":"x.v1"},"injected":true}"#,
            r#"{"disposition":"not_applicable_by_reviewed_ruling","ruling_ref":"r","injected":true}"#,
            r#"{"disposition":"transferred_to_open_owner","proposition":"p","destination_repository":"o/r","destination_issue":1,"destination_contract_identity":"d","rationale":"r","injected":true}"#,
            r#"{"disposition":"removed_surface_with_proof","proof":{"producer":"x","subject":"s","content_digest":"d","reference":"r","schema_version":"x.v1"},"injected":true}"#,
            r#"{"disposition":"not_proven","reason":"r","injected":true}"#,
            r#"{"disposition":"contradicted","reason":"r","injected":true}"#,
            r#"{"disposition":"stale","reason":"r","injected":true}"#,
        ];
        // Parsing is deliberately separated from shape validation: placeholder
        // digests cannot turn an unknown-field test into a digest test.
        let mut missed = Vec::new();
        for raw in row_variants {
            let clean = replace_once(raw, ",\"injected\":true", "");
            require_clean_wire_round_trip::<RowDispositionValue>(&clean);
            let result = serde_json::from_str::<RowDispositionValue>(raw);
            if !is_unknown_field_refusal(&result) {
                missed.push(raw);
            }
            eprintln!("QUALIFICATION row payload {raw}: {result:?}");
        }
        for raw in [
            r#"{"state":"verified","injected":true}"#,
            r#"{"state":"failed","reason":"r","injected":true}"#,
            r#"{"state":"not_proven","reason":"r","injected":true}"#,
        ] {
            let clean = replace_once(raw, ",\"injected\":true", "");
            require_clean_wire_round_trip::<ControlOutcome>(&clean);
            let result = serde_json::from_str::<ControlOutcome>(raw);
            if !is_unknown_field_refusal(&result) {
                missed.push(raw);
            }
            eprintln!("QUALIFICATION control payload {raw}: {result:?}");
        }
        for raw in [
            r#"{"state":"still_open","injected":true}"#,
            r#"{"state":"closed_by_packet","packet_subject":"p","injected":true}"#,
            r#"{"state":"transferred_to_open_owner","proposition":"p","destination_repository":"o/r","destination_issue":1,"destination_contract_identity":"d","rationale":"r","injected":true}"#,
        ] {
            let clean = replace_once(raw, ",\"injected\":true", "");
            require_clean_wire_round_trip::<ChildState>(&clean);
            let result = serde_json::from_str::<ChildState>(raw);
            if !is_unknown_field_refusal(&result) {
                missed.push(raw);
            }
            eprintln!("QUALIFICATION child payload {raw}: {result:?}");
        }
        // Also reach the public packet parser with a valid otherwise-complete
        // subject rather than only deserializing enum components.
        let raw = packet.to_canonical_json()?;
        let poisoned = replace_once(
            &raw,
            "\"state\": \"verified\"",
            "\"state\": \"verified\", \"injected\": true",
        );
        if ClosePacket::from_json_str(&poisoned).is_ok() {
            missed.push("public packet control payload");
        }
        assert!(missed.is_empty(), "unknown variant fields were silently ignored: {missed:?}");
        Ok(())
    }

    #[test]
    fn control_unknown_tags_and_typed_struct_fields_are_schema_errors()
    -> Result<(), CloseProofError> {
        let (_, packet) = rich_subject()?;
        let raw = packet.to_canonical_json()?;
        for (from, to) in [
            ("\"disposition\": \"proven_current_main\"", "\"disposition\": \"future_success\""),
            ("\"state\": \"verified\"", "\"state\": \"future_success\""),
            ("\"state\": \"closed_by_packet\"", "\"state\": \"future_success\""),
            ("\"issue_number\": 9000300", "\"issue_number\": 9000300, \"issue_number\": 9000300"),
            (
                "\"producer\": \"xtask-landing-proof\"",
                "\"producer\": \"xtask-landing-proof\", \"producer\": \"xtask-landing-proof\"",
            ),
            (
                "\"producer\": \"xtask-landing-proof\"",
                "\"producer\": \"xtask-landing-proof\", \"injected\": true",
            ),
        ] {
            let poisoned = replace_once(&raw, from, to);
            assert!(
                matches!(
                    ClosePacket::from_json_str(&poisoned),
                    Err(CloseProofError::Schema { .. })
                ),
                "expected schema refusal: {to}"
            );
        }
        Ok(())
    }

    #[test]
    fn control_contract_and_packet_parsers_reject_ambiguous_typed_fields()
    -> Result<(), CloseProofError> {
        let (contract, packet) = rich_subject()?;
        let raw = contract.to_canonical_json()?;
        for (from, to) in [
            ("\"kind\": \"leaf\"", "\"kind\": \"future_kind\""),
            (
                "\"required_proof_level\": \"mechanism\",\n  \"allowed_close_modes\"",
                "\"required_proof_level\": \"unknown_level\",\n  \"allowed_close_modes\"",
            ),
            ("\"permitted\": false", "\"permitted\": false, \"permitted\": true"),
            ("\"permitted\": false", "\"permitted\": false, \"injected\": true"),
        ] {
            assert!(
                matches!(
                    IssueContract::from_json_str(&replace_once(&raw, from, to)),
                    Err(CloseProofError::Schema { .. })
                ),
                "expected contract Schema: {to}"
            );
        }
        let raw = packet.to_canonical_json()?;
        for (from, to) in [
            (
                "\"disposition\": \"proven_current_main\"",
                "\"disposition\": \"proven_current_main\", \"disposition\": \"proven_current_main\"",
            ),
            ("\"state\": \"verified\"", "\"state\": \"verified\", \"state\": \"verified\""),
        ] {
            assert!(
                matches!(
                    ClosePacket::from_json_str(&replace_once(&raw, from, to)),
                    Err(CloseProofError::Schema { message, .. }) if message.contains("duplicate field")
                ),
                "expected tag duplicate-field Schema: {to}"
            );
        }
        assert!(matches!(
            ClosePacket::from_json_str(&(raw + " {}")),
            Err(CloseProofError::Schema { .. })
        ));
        Ok(())
    }

    #[test]
    fn diagnostic_nonrow_contract_movement_is_not_bound_by_v1() -> Result<(), CloseProofError> {
        let (contract, packet) = rich_subject()?;
        let mut movements = Vec::new();
        let mut moved = contract.clone();
        moved.required_proof_level = ProofLevel::Public;
        movements.push(("top-level-proof", moved));
        let mut moved = contract.clone();
        moved.kind = IssueKind::Installed;
        movements.push(("issue-kind", moved));
        let mut moved = contract.clone();
        moved.allowed_close_modes.retain(|m| *m != CloseMode::NotPlanned);
        movements.push(("allowed-mode-set", moved));
        let mut moved = contract.clone();
        moved.transfer_policy.permitted = true;
        moved.transfer_policy.conditions.push("Only to an adopted exact owner.".to_string());
        movements.push(("transfer-policy", moved));
        let mut moved = contract.clone();
        moved.negative_controls[0].description =
            "Reject the malformed form at the external parser boundary.".to_string();
        movements.push(("control-meaning", moved));
        let mut moved = contract.clone();
        moved.negative_controls[0].guards_row_id = "neighbor.parses".to_string();
        movements.push(("control-row-association", moved));
        let mut moved = contract.clone();
        moved.domain_evidence_refs.push("adopted-domain-contract.v2".to_string());
        movements.push(("linked-domain-contract", moved));
        for (name, moved) in movements {
            moved.validate()?;
            assert_eq!(
                moved.identity, contract.identity,
                "demonstrate omitted input, not a forged row digest"
            );
            validate_packet_against_contract(&packet, &moved)?;
            eprintln!("QUALIFICATION v1 accepts nonrow contract movement: {name}");
        }
        // A future full-policy compiler must reseal moved contracts before
        // testing stale packet rejection. No such seam exists in v1: do not
        // fabricate its runtime result or treat a hand-edited digest failure
        // as authoritative contract-movement proof.
        Ok(())
    }

    #[test]
    fn control_body_rows_and_rulings_stale_packets_while_membership_has_coverage()
    -> Result<(), CloseProofError> {
        let (contract, packet) = rich_subject()?;
        let mut moved = contract.clone();
        moved.identity.issue_body_digest = leaf_digest(44);
        assert!(matches!(
            validate_packet_against_contract(&packet, &moved),
            Err(CloseProofError::Identity { .. })
        ));
        let mut moved = contract.clone();
        moved.denominator[0].row_id = "same-count.replacement".to_string();
        moved.negative_controls[0].guards_row_id = "same-count.replacement".to_string();
        moved.identity.denominator_digest =
            super::super::compute_denominator_digest(&moved.denominator)?;
        assert_eq!(moved.denominator.len(), contract.denominator.len());
        assert!(matches!(
            validate_packet_against_contract(&packet, &moved),
            Err(CloseProofError::Identity { .. })
        ));
        let mut moved = contract.clone();
        moved.identity.accepted_ruling = Some(super::super::RulingIdentity {
            identity: "accepted-comment-44".to_string(),
            digest: leaf_digest(44),
        });
        assert!(matches!(
            validate_packet_against_contract(&packet, &moved),
            Err(CloseProofError::Identity { .. })
        ));
        let mut moved = contract.clone();
        moved.mandatory_children[0].number += 1;
        assert_eq!(moved.identity, contract.identity);
        assert!(matches!(
            validate_packet_against_contract(&packet, &moved),
            Err(CloseProofError::Coverage { .. })
        ));
        let mut moved = contract.clone();
        moved.negative_controls[0].control_id = "nc.replacement".to_string();
        assert_eq!(moved.identity, contract.identity);
        assert!(matches!(
            validate_packet_against_contract(&packet, &moved),
            Err(CloseProofError::Coverage { .. })
        ));
        Ok(())
    }

    #[test]
    fn diagnostic_verdict_evidence_rank_and_phase_are_representation_only()
    -> Result<(), CloseProofError> {
        let (contract, mut packet) = rich_subject()?;
        // A claimed Valid verdict coexists with an unproven row. This layer
        // checks shape and coverage; it does not independently decide closure.
        assert_eq!(packet.verdict.issue_close, IssueCloseOutcome::Valid);
        assert!(matches!(
            packet.row_dispositions["neighbor.parses"],
            RowDispositionValue::NotProven { .. }
        ));
        validate_packet_against_contract(&packet, &contract)?;
        for verdict in
            [IssueCloseOutcome::Valid, IssueCloseOutcome::Invalid, IssueCloseOutcome::NotProven]
        {
            packet.verdict.issue_close = verdict;
            validate_packet_against_contract(&packet, &contract)?;
        }
        if let Some(RowDispositionValue::ProvenCurrentMain { evidence }) =
            packet.row_dispositions.get_mut("single-row.defect.fixed")
        {
            evidence.producer = "unregistered-wrong-property-producer".to_string();
            evidence.subject = "different-candidate-root-and-generation".to_string();
            evidence.schema_version = Some("unregistered.v99".to_string());
        }
        validate_packet_against_contract(&packet, &contract)?;
        assert!(ProofLevel::Public.satisfies(ProofLevel::Mechanism));
        // The ranked primitive has no row/domain capability argument. Its
        // true result cannot admit a producer for a different property.
        packet.requested_close_mode = CloseMode::PhaseCompleteIssueRemainsOpen;
        packet.verdict.issue_close = IssueCloseOutcome::Valid;
        packet.landed_subjects.clear();
        packet.landing_content_proof.clear();
        validate_packet_against_contract(&packet, &contract)?;
        assert_eq!(packet.requested_close_mode, CloseMode::PhaseCompleteIssueRemainsOpen);
        Ok(())
    }

    #[test]
    fn diagnostic_fixture_expectations_can_be_resealed_without_independent_evaluation()
    -> Result<(), CloseProofError> {
        let mut fixture = fixture_subject()?;
        fixture.cases[0].packet.verdict.issue_close = IssueCloseOutcome::Invalid;
        fixture.cases[0].expected_issue_close = IssueCloseOutcome::Invalid;
        fixture.verify()?;
        fixture.cases[0].packet.verdict.issue_close = IssueCloseOutcome::Valid;
        assert!(matches!(fixture.verify(), Err(CloseProofError::Corpus { .. })));
        fixture.cases[0].expected_issue_close = IssueCloseOutcome::Valid;
        fixture.verify()?;
        // Do not edit the historical corpus or promote its expectations into
        // an oracle: this disposable fixture establishes only the boundary.
        Ok(())
    }

    #[test]
    fn diagnostic_corpus_inventory_is_schema_only_not_semantic_gold() -> Result<(), CloseProofError>
    {
        let manifest = load_corpus_manifest()?;
        assert_eq!(manifest.fixtures.len(), 15);
        let mut case_count = 0;
        let mut landing_row_refs = 0;
        let mut valid_phase_cases = 0;
        for entry in manifest.fixtures {
            let raw = super::super::corpus::read_file(&corpus_root().join(entry.file))?;
            let fixture = FixtureDocument::from_json_str(&raw)?;
            fixture.verify()?;
            assert!(fixture.provenance.boundary.contains("schema-level dispositions only"));
            for case in fixture.cases {
                case_count += 1;
                for disposition in case.packet.row_dispositions.values() {
                    if let RowDispositionValue::ProvenCurrentMain { evidence }
                    | RowDispositionValue::RemovedSurfaceWithProof { proof: evidence } =
                        disposition
                    {
                        assert_eq!(evidence.schema_version.as_deref(), Some("landing_proof.v1"));
                        landing_row_refs += 1;
                    }
                }
                if case.packet.requested_close_mode == CloseMode::PhaseCompleteIssueRemainsOpen
                    && case.expected_issue_close == IssueCloseOutcome::Valid
                {
                    valid_phase_cases += 1;
                }
            }
        }
        assert_eq!(case_count, 18);
        assert_eq!(landing_row_refs, 25);
        assert_eq!(valid_phase_cases, 2);
        eprintln!(
            "QUALIFICATION corpus: 15 structural fixtures, 18 cases, 25 landing row references, 2 valid nonterminal phase relations; no semantic gold admission"
        );
        Ok(())
    }
}

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
        accepted_ruling_identity: None,
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
                    producer: "xtask-landing-proof".to_string(),
                    subject: "a".repeat(64),
                    content_digest: content_digest_hex(b"evidence-bytes"),
                    reference:
                        "cargo xtask landing-proof --commit aaaa1111 --canonical-main origin/main"
                            .to_string(),
                    schema_version: Some(super::contract::LANDING_PROOF_ENVELOPE_V1.to_string()),
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
fn unauthorized_close_mode_is_rejected() -> Result<(), CloseProofError> {
    let fixture = FixtureDocument::from_json_str(include_str!(
        "../../../.ci/close-proof-contract/fixtures/invalid-controller-fanin-missing.json"
    ))?;
    let case = fixture
        .cases
        .iter()
        .find(|case| case.case_id == "pr-9001011-child-count-only")
        .ok_or_else(|| CloseProofError::Corpus {
            message: "controller fan-in regression case is missing".to_string(),
        })?;
    assert!(!fixture.contract.allowed_close_modes.contains(&CloseMode::ControllerComplete));

    let mut unauthorized = case.packet.clone();
    unauthorized.requested_close_mode = CloseMode::ControllerComplete;
    assert!(matches!(
        validate_packet_against_contract(&unauthorized, &fixture.contract),
        Err(CloseProofError::Coverage { message })
            if message.contains("requested close mode") && message.contains("not allowed")
    ));
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
    doc_packet.contract_binding.accepted_ruling_identity = Some(
        "https://github.com/effortlessmetrics/perl-lsp-swarm/issues/9000300#ruling".to_string(),
    );
    doc_packet.contract_binding.accepted_ruling_digest = Some(leaf_digest(4242));
    validate_packet_against_contract(&doc_packet, &contract)?;
    Ok(())
}

#[test]
fn moved_ruling_identity_invalidates_packet_even_with_matching_digest()
-> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    let ruling_identity =
        "https://github.com/effortlessmetrics/perl-lsp-swarm/issues/9000300#ruling";
    contract.identity.accepted_ruling = Some(super::RulingIdentity {
        identity: ruling_identity.to_string(),
        digest: leaf_digest(4242),
    });
    let mut doc_packet = passing_packet(&contract)?;
    doc_packet.contract_binding.accepted_ruling_identity = Some(ruling_identity.to_string());
    doc_packet.contract_binding.accepted_ruling_digest = Some(leaf_digest(4242));
    contract.identity.accepted_ruling = Some(super::RulingIdentity {
        identity: "https://github.com/effortlessmetrics/perl-lsp-swarm/issues/9000300#replacement"
            .to_string(),
        digest: leaf_digest(4242),
    });
    assert!(matches!(
        validate_packet_against_contract(&doc_packet, &contract),
        Err(CloseProofError::Identity { message }) if message.contains("accepted ruling moved")
    ));
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
fn conflicting_duplicate_child_dispositions_are_rejected() -> Result<(), CloseProofError> {
    let mut contract = leaf_contract()?;
    contract.kind = IssueKind::Controller;
    contract.mandatory_children =
        vec![super::IssueRef { repository: contract.repository.clone(), number: 9000301 }];
    contract.validate()?;
    let mut duplicate = passing_packet(&contract)?;
    duplicate.child_dispositions = vec![
        ChildDispositionRecord {
            child: super::IssueRef { repository: contract.repository.clone(), number: 9000301 },
            state: ChildState::ClosedByPacket { packet_subject: "PR #9000401".to_string() },
        },
        ChildDispositionRecord {
            child: super::IssueRef { repository: contract.repository.clone(), number: 9000301 },
            state: ChildState::StillOpen,
        },
    ];
    assert!(matches!(
        validate_packet_against_contract(&duplicate, &contract),
        Err(CloseProofError::Coverage { message })
            if message.contains("duplicate mandatory child disposition")
    ));
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
                schema_version: Some("p.v1".to_string()),
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

// ---------------------------------------------------------------------------
// Evidence-ref envelope discipline (#15386)
// ---------------------------------------------------------------------------

fn landing_evidence_ref(schema_version: Option<String>) -> super::EvidenceRef {
    super::EvidenceRef {
        producer: "xtask-landing-proof".to_string(),
        subject: "a".repeat(64),
        content_digest: content_digest_hex(b"evidence-bytes"),
        reference: "cargo xtask landing-proof --commit aaaa1111 --canonical-main origin/main"
            .to_string(),
        schema_version,
    }
}

#[test]
fn evidence_ref_refuses_missing_envelope_schema_version() {
    let missing = landing_evidence_ref(None);
    assert!(matches!(
        super::contract::validate_evidence_ref(&missing),
        Err(CloseProofError::Schema { field, .. }) if field == "evidence.schema_version"
    ));
}

#[test]
fn evidence_ref_refuses_wrong_envelope_version_for_known_producer() {
    let tampered = landing_evidence_ref(Some("landing_proof.v2".to_string()));
    assert!(matches!(
        super::contract::validate_evidence_ref(&tampered),
        Err(CloseProofError::Schema { field, .. }) if field == "evidence.schema_version"
    ));
}

#[test]
fn evidence_ref_accepts_pinned_envelope_for_known_producer() {
    let pinned = landing_evidence_ref(Some(super::contract::LANDING_PROOF_ENVELOPE_V1.to_string()));
    assert!(super::contract::validate_evidence_ref(&pinned).is_ok());
}

#[test]
fn evidence_ref_requires_declared_version_from_unknown_producers() {
    let mut undeclared =
        landing_evidence_ref(Some(super::contract::LANDING_PROOF_ENVELOPE_V1.to_string()));
    undeclared.producer = "domain-owner-evidence".to_string();
    super::contract::validate_evidence_ref(&undeclared)
        .expect("unknown producers may declare their own envelope vocabulary");
    undeclared.schema_version = None;
    assert!(matches!(
        super::contract::validate_evidence_ref(&undeclared),
        Err(CloseProofError::Schema { field, .. }) if field == "evidence.schema_version"
    ));
}

#[test]
fn evidence_ref_pins_corpus_producer_identity_too() {
    let mut corpus_producer =
        landing_evidence_ref(Some(super::contract::LANDING_PROOF_ENVELOPE_V1.to_string()));
    corpus_producer.producer = super::contract::PR_CLOSE_PROOF_PRODUCER.to_string();
    assert!(super::contract::validate_evidence_ref(&corpus_producer).is_ok());
    corpus_producer.schema_version = Some("landing_proof.v2".to_string());
    assert!(matches!(
        super::contract::validate_evidence_ref(&corpus_producer),
        Err(CloseProofError::Schema { field, .. }) if field == "evidence.schema_version"
    ));
}
