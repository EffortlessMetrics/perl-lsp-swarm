//! Falsifiers for the file-level compiler lexical contribution contract
//! (PIRL-01, #12109).
//!
//! Negative controls fail when: a first write becomes a declaration; `Modify`
//! is dropped while completeness stays complete; same-name bindings collapse;
//! binding ids repeat; occurrence anchors are not source-backed; a binding
//! fingerprint survives mutation of its identity fields; an occurrence anchor
//! id does not trace back to the canonical #12191 derivation; a complete but
//! superseded/withdrawn record counts as exact; producer naming upgrades
//! proof/completeness; mixed identities validate; missing facts become
//! exact-empty; a foreign-generation semantic join validates; or an unknown
//! work field defaults to zero. Positive controls cover complete,
//! declaration-only, partial/recovered, stale/cancelled/instrument-failed
//! states, every source-backed anchor kind, canonical anchor identity
//! preservation, accessor-only envelope reads, and deterministic fingerprints
//! over the unsigned canonical serialization under input-order and
//! limitation-order variation.

use perl_parser_core::hir::HirId;
use perl_parser_core::pir::{
    BuildKind, CompilerProducerIdentity, ContributionCompleteness, ContributionDraft,
    ContributionError, ContributionLimitation, ContributionOccurrence, ContributionSubjectIdentity,
    ContributionWorkShape, FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION,
    FilePirLexicalContributionV1, LexicalBindingIdentity, LexicalSigil, OccurrenceAnchor,
    OccurrenceRole, PirAnchorKind, PirSourceAnchor, SemanticSnapshotJoinMetadata,
    TerminalDisposition, WorkObservation,
};
use perl_position_tracking::{ByteSpan, SourceLocation};
use perl_semantic_facts::AnchorId;
use perl_source_identity::ContentDigest;

type TestResult<T = ()> = Result<T, String>;

fn digest(seed: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(seed)
}

fn anchor(range: (usize, usize)) -> TestResult<OccurrenceAnchor> {
    let span = ByteSpan { start: range.0, end: range.1 };
    let location: SourceLocation = span;
    let pir_anchor = PirSourceAnchor::explicit(location, HirId::from_index(0));
    OccurrenceAnchor::from_pir_anchor(&pir_anchor)
        .ok_or_else(|| "explicit anchors must snapshot".to_string())
}

fn subject(generation: u64) -> ContributionSubjectIdentity {
    ContributionSubjectIdentity {
        full_source_digest: digest(b"source"),
        parser_input_digest: digest(b"parser-input"),
        accepted_generation: generation,
        body_hir_identity: digest(b"body-hir"),
    }
}

fn producer() -> CompilerProducerIdentity {
    CompilerProducerIdentity {
        implementation: "perl-lsp-compiler-test".to_string(),
        pir_profile: "pir-v0".to_string(),
        producer: "lexical-builder".to_string(),
    }
}

fn binding(id: &str, name: &str, decl_range: (usize, usize)) -> TestResult<LexicalBindingIdentity> {
    binding_in_body(id, "body-0", LexicalSigil::Scalar, name, decl_range)
}

fn binding_in_body(
    id: &str,
    body_id: &str,
    sigil: LexicalSigil,
    name: &str,
    decl_range: (usize, usize),
) -> TestResult<LexicalBindingIdentity> {
    LexicalBindingIdentity::new(
        id.to_string(),
        body_id.to_string(),
        vec!["scope-0".to_string()],
        sigil,
        name.to_string(),
        decl_range,
    )
    .map_err(|error| format!("test binding identity must derive its fingerprint: {error}"))
}

/// One Declaration occurrence exactly covering its binding's declared range.
fn declaration_occurrence(
    id: &str,
    binding_id: &str,
    decl_range: (usize, usize),
) -> TestResult<ContributionOccurrence> {
    Ok(ContributionOccurrence {
        occurrence_id: id.to_string(),
        binding_id: binding_id.to_string(),
        role: OccurrenceRole::Declaration,
        anchor: anchor(decl_range)?,
        operation_provenance: "LexicalWrite".to_string(),
    })
}

fn occurrence(
    id: &str,
    binding_id: &str,
    role: OccurrenceRole,
    range: (usize, usize),
) -> TestResult<ContributionOccurrence> {
    let provenance = match role {
        OccurrenceRole::Read => "LexicalRead",
        OccurrenceRole::Write => "LexicalWrite",
        OccurrenceRole::Modify => "LexicalModify",
        OccurrenceRole::Declaration => "LexicalWrite",
    };
    Ok(ContributionOccurrence {
        occurrence_id: id.to_string(),
        binding_id: binding_id.to_string(),
        role,
        anchor: anchor(range)?,
        operation_provenance: provenance.to_string(),
    })
}

fn work(losses: u64) -> ContributionWorkShape {
    ContributionWorkShape {
        body_hir_inputs_consumed: WorkObservation::Observed(1),
        pir_bodies_lowered: WorkObservation::Observed(1),
        verifier_work: WorkObservation::Observed(3),
        lexical_operations_visited: WorkObservation::Observed(4),
        anchors_accepted: WorkObservation::Observed(4 - losses),
        anchors_rejected: WorkObservation::Observed(losses),
        unsupported_or_dynamic_operations: if losses > 0 {
            WorkObservation::Observed(losses)
        } else {
            WorkObservation::NotApplicable
        },
        build_kind: BuildKind::NewBuild,
    }
}

fn committed() -> TerminalDisposition {
    TerminalDisposition::Committed
}

/// Partial draft base for provenance/canonicalization falsifiers.
fn partial_draft(
    bindings: Vec<LexicalBindingIdentity>,
    occurrences: Vec<ContributionOccurrence>,
    limitations: Vec<ContributionLimitation>,
) -> ContributionDraft {
    ContributionDraft {
        producer: producer(),
        subject: subject(21),
        bindings,
        occurrences,
        completeness: ContributionCompleteness::Partial,
        limitations,
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    }
}

#[test]
fn valid_complete_initialized_lexical_construction_is_exact() -> TestResult {
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![
            declaration_occurrence("o0", "b0", (4, 5))?,
            occurrence("o1", "b0", OccurrenceRole::Read, (10, 11))?,
        ],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| {
        format!("complete initialized lexical contribution must construct: {error}")
    })?;

    assert!(contribution.is_exact());
    Ok(())
}

#[test]
fn valid_declaration_only_construction_is_exact_without_reads() -> TestResult {
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(1),
        bindings: vec![binding("b0", "count", (8, 13))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (8, 13))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| format!("declaration-only complete contribution must construct: {error}"))?;

    assert!(contribution.is_exact());
    Ok(())
}

#[test]
fn read_write_and_modify_roles_stay_distinct() -> TestResult {
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(2),
        bindings: vec![binding("b0", "n", (0, 6))?],
        occurrences: vec![
            declaration_occurrence("o0", "b0", (0, 6))?,
            occurrence("o1", "b0", OccurrenceRole::Modify, (12, 17))?,
        ],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| format!("modify-bearing contribution must construct: {error}"))?;

    let roles: Vec<_> = contribution.occurrences().iter().map(|o| o.role).collect();
    assert!(roles.contains(&OccurrenceRole::Modify));
    assert!(roles.contains(&OccurrenceRole::Declaration));
    Ok(())
}

#[test]
fn same_name_in_another_body_is_a_distinct_binding() -> TestResult {
    let outer = binding_in_body("b-outer", "body-outer", LexicalSigil::Scalar, "x", (0, 5))?;
    let inner = binding_in_body("b-inner", "body-inner", LexicalSigil::Scalar, "x", (40, 45))?;

    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(3),
        bindings: vec![outer, inner],
        occurrences: vec![
            declaration_occurrence("o0", "b-outer", (0, 5))?,
            declaration_occurrence("o1", "b-inner", (40, 45))?,
        ],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| format!("same display name in another body must stay distinct: {error}"))?;

    assert_eq!(contribution.bindings().len(), 2);
    Ok(())
}

#[test]
fn sigils_separate_bindings_with_equal_names() -> TestResult {
    let scalar_binding = binding_in_body("b-scalar", "body-0", LexicalSigil::Scalar, "x", (0, 5))?;
    let array_binding = binding_in_body("b-array", "body-0", LexicalSigil::Array, "x", (20, 25))?;

    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(4),
        bindings: vec![scalar_binding, array_binding],
        occurrences: vec![
            declaration_occurrence("o0", "b-scalar", (0, 5))?,
            declaration_occurrence("o1", "b-array", (20, 25))?,
        ],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| format!("$x and @x must be distinct bindings: {error}"))?;
    assert_eq!(contribution.bindings().len(), 2);
    Ok(())
}

#[test]
fn source_identical_later_generation_is_another_subject() -> TestResult {
    let occurrences = vec![declaration_occurrence("o0", "b0", (4, 5))?];
    let partial_with_recovery = |generation: u64,
                                 occurrences: &[ContributionOccurrence]|
     -> TestResult<FilePirLexicalContributionV1> {
        FilePirLexicalContributionV1::try_new(ContributionDraft {
            producer: producer(),
            subject: subject(generation),
            bindings: vec![binding("b0", "x", (4, 5))?],
            occurrences: occurrences.to_vec(),
            completeness: ContributionCompleteness::Partial,
            limitations: vec![ContributionLimitation::RecoveredBody],
            work: work(0),
            terminal_disposition: committed(),
            semantic_snapshot_join: None,
        })
        .map_err(|error| format!("construction failed: {error}"))
    };

    let earlier =
        partial_with_recovery(9, &occurrences).map_err(|e| format!("earlier generation: {e}"))?;
    let later =
        partial_with_recovery(10, &occurrences).map_err(|e| format!("later generation: {e}"))?;

    assert_ne!(earlier.fingerprint(), later.fingerprint());
    assert!(!earlier.is_exact());
    Ok(())
}

#[test]
fn matching_semantic_snapshot_join_is_accepted_as_metadata_only() -> TestResult {
    let mut joined_subject = subject(5);
    joined_subject.parser_input_digest = digest(b"joined-input");
    let join = SemanticSnapshotJoinMetadata {
        snapshot_digest: digest(b"snapshot"),
        generation: 5,
        parser_input_digest: digest(b"joined-input"),
    };

    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: joined_subject,
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
        completeness: ContributionCompleteness::Unavailable,
        limitations: Vec::new(),
        work: work(1),
        terminal_disposition: committed(),
        semantic_snapshot_join: Some(join),
    })
    .map_err(|error| format!("matching join metadata must be accepted: {error}"))?;

    // Semantic metadata never upgrades compiler completeness.
    assert!(!contribution.is_exact());
    Ok(())
}

#[test]
fn foreign_generation_semantic_join_is_rejected() -> TestResult {
    let join = SemanticSnapshotJoinMetadata {
        snapshot_digest: digest(b"snapshot"),
        generation: 99,
        parser_input_digest: digest(b"parser-input"),
    };

    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(5),
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: Some(join),
    });

    assert_eq!(
        error.err().ok_or("a foreign-generation join must be rejected")?,
        ContributionError::ForeignSemanticJoin
    );
    Ok(())
}

#[test]
fn stale_cancelled_and_instrument_failed_states_stay_non_exact() -> TestResult {
    for completeness in [
        ContributionCompleteness::StaleOrSuperseded,
        ContributionCompleteness::Cancelled,
        ContributionCompleteness::InstrumentFailure,
        ContributionCompleteness::BudgetExhausted,
        ContributionCompleteness::InvalidSubject,
    ] {
        let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
            producer: producer(),
            subject: subject(6),
            bindings: Vec::new(),
            occurrences: Vec::new(),
            completeness,
            limitations: Vec::new(),
            work: work(1),
            terminal_disposition: committed(),
            semantic_snapshot_join: None,
        })
        .map_err(|error| format!("{completeness:?} state must construct: {error}"))?;
        assert!(!contribution.is_exact(), "{completeness:?} must never count as exact");
    }
    Ok(())
}

#[test]
fn deterministic_fingerprint_under_input_order_variation() -> TestResult {
    let build = |reverse: bool| -> TestResult<FilePirLexicalContributionV1> {
        let mut bindings = vec![binding("b0", "x", (4, 5))?, binding("b1", "y", (30, 35))?];
        let mut occurrences = vec![
            declaration_occurrence("o0", "b0", (4, 5))?,
            declaration_occurrence("o1", "b1", (30, 35))?,
            occurrence("o2", "b1", OccurrenceRole::Write, (50, 55))?,
        ];
        if reverse {
            bindings.reverse();
            occurrences.reverse();
        }
        FilePirLexicalContributionV1::try_new(ContributionDraft {
            producer: producer(),
            subject: subject(11),
            bindings,
            occurrences,
            completeness: ContributionCompleteness::Complete,
            limitations: Vec::new(),
            work: work(0),
            terminal_disposition: committed(),
            semantic_snapshot_join: None,
        })
        .map_err(|error| format!("order-varied construction must succeed: {error}"))
    };

    assert_eq!(build(false)?.fingerprint(), build(true)?.fingerprint());
    Ok(())
}

// ── Negative controls ──

#[test]
fn first_write_becoming_declaration_is_rejected() -> TestResult {
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))?],
        // A write at (10,15) mislabeled as the binding's Declaration even
        // though the binding's declared range is (4,5).
        occurrences: vec![occurrence("o0", "b0", OccurrenceRole::Declaration, (10, 15))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    });

    assert!(
        matches!(
            error.err().ok_or("relabeling a first write must be rejected")?,
            ContributionError::InvalidDeclarationAnchor { .. }
        ),
        "relabeling a first write as the declaration must fail"
    );
    Ok(())
}

#[test]
fn modify_dropped_while_claiming_complete_is_rejected() -> TestResult {
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        // Observed unsupported/dynamic operations mean Modify-class facts were
        // dropped; claiming Complete on top is invalid.
        work: ContributionWorkShape {
            unsupported_or_dynamic_operations: WorkObservation::Observed(2),
            ..work(0)
        },
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    });

    assert!(matches!(
        error.err().ok_or("dropped Modify with Complete claim must fail")?,
        ContributionError::IncompleteButClaimedComplete { .. }
    ));
    Ok(())
}

#[test]
fn same_name_bindings_collapse_is_rejected() -> TestResult {
    // Two bindings identical on (body, scope, sigil, name) collapse onto one
    // identity even with different ids.
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b-a", "x", (4, 5))?, binding("b-b", "x", (40, 45))?],
        occurrences: vec![
            declaration_occurrence("o0", "b-a", (4, 5))?,
            declaration_occurrence("o1", "b-b", (40, 45))?,
        ],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    });

    assert!(matches!(
        error.err().ok_or("collapsed same-name bindings must be rejected")?,
        ContributionError::CollapsedBindingIdentity { .. }
    ));
    Ok(())
}

#[test]
fn producer_name_cannot_upgrade_completeness() -> TestResult {
    let inflated = CompilerProducerIdentity {
        implementation: "super-compiler-9000".to_string(),
        pir_profile: "pir-v0".to_string(),
        producer: "trusted-producer".to_string(),
    };
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: inflated,
        subject: subject(7),
        bindings: Vec::new(),
        occurrences: Vec::new(),
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        // Loss observed: no producer name can repair that.
        work: work(3),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    });

    assert!(matches!(
        error.err().ok_or("producer naming must never upgrade proof/completeness")?,
        ContributionError::IncompleteButClaimedComplete { .. }
    ));
    Ok(())
}

#[test]
fn unknown_work_field_is_not_numeric_zero() -> TestResult {
    // An Unavailable loss indicator cannot back a Complete claim: unknown is
    // not zero.
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: ContributionWorkShape { anchors_rejected: WorkObservation::Unavailable, ..work(0) },
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    });

    assert!(matches!(
        error.err().ok_or("unknown loss observation must not default to zero")?,
        ContributionError::IncompleteButClaimedComplete { .. }
    ));
    Ok(())
}

#[test]
fn occurrence_anchors_carry_exact_ranges_not_optional_presence() -> TestResult {
    // Structural missing-anchor control (#12109): an occurrence anchor always
    // carries a concrete byte range — there is no representation for an
    // unanchored fact, so missing anchors can only be recorded as
    // ContributionLimitation::MissingAnchor, never as silent empty facts.
    let pir_anchor =
        PirSourceAnchor::explicit(SourceLocation { start: 12, end: 17 }, HirId::from_index(0));
    assert_eq!(
        pir_anchor.kind,
        perl_parser_core::pir::PirAnchorKind::ExplicitSource,
        "the explicit constructor keeps its provenance class"
    );
    let snapshot =
        OccurrenceAnchor::from_pir_anchor(&pir_anchor).ok_or("explicit anchors must snapshot")?;
    assert_eq!(snapshot.range, (12, 17));
    Ok(())
}

// ── Accepted review repairs (#12180 discussion_r3846438509 … r3846439040) ──

#[test]
fn duplicate_binding_ids_are_rejected_before_lookup_construction() -> TestResult {
    // Two logically distinct bindings reuse one binding_id. They do not
    // collapse on (body, scope, sigil, name), so only an explicit id seen-set
    // can catch them; the lookup collect must never silently keep the last.
    let error = FilePirLexicalContributionV1::try_new(partial_draft(
        vec![binding("dup", "x", (4, 5))?, binding("dup", "y", (40, 45))?],
        Vec::new(),
        Vec::new(),
    ));

    assert_eq!(
        error.err().ok_or("duplicate binding ids must be rejected")?,
        ContributionError::DuplicateBindingId { binding_id: "dup".to_string() }
    );
    Ok(())
}

#[test]
fn non_source_backed_anchor_kinds_are_rejected() -> TestResult {
    let hand_anchored = |occurrence_id: &str,
                         anchor_kind: PirAnchorKind|
     -> TestResult<ContributionError> {
        let occurrence = ContributionOccurrence {
            occurrence_id: occurrence_id.to_string(),
            binding_id: "b0".to_string(),
            role: OccurrenceRole::Read,
            anchor: OccurrenceAnchor {
                anchor_kind,
                range: (20, 25),
                anchor_id: AnchorId(20),
                hir_item_index: Some(0),
            },
            operation_provenance: "LexicalRead".to_string(),
        };
        FilePirLexicalContributionV1::try_new(partial_draft(
            vec![binding("b0", "x", (4, 5))?],
            vec![occurrence],
            Vec::new(),
        ))
        .err()
        .ok_or_else(|| format!("anchor kind {} must not pass as source-backed", anchor_kind.name()))
    };

    for anchor_kind in
        [PirAnchorKind::GeneratedNoSource, PirAnchorKind::AmbientInput, PirAnchorKind::Unknown]
    {
        assert_eq!(
            hand_anchored("o-unanchored", anchor_kind)?,
            ContributionError::UnanchoredOccurrence { occurrence_id: "o-unanchored".to_string() },
            "anchor kind {} must be rejected as non-source-backed",
            anchor_kind.name()
        );
    }
    Ok(())
}

#[test]
fn source_backed_anchor_kinds_are_accepted() -> TestResult {
    for anchor_kind in [
        PirAnchorKind::ExplicitSource,
        PirAnchorKind::SourceBackedGenerated,
        PirAnchorKind::DynamicBoundary,
    ] {
        let occurrence = ContributionOccurrence {
            occurrence_id: "o-anchored".to_string(),
            binding_id: "b0".to_string(),
            role: OccurrenceRole::Read,
            anchor: OccurrenceAnchor {
                anchor_kind,
                range: (20, 25),
                anchor_id: AnchorId(20),
                hir_item_index: Some(0),
            },
            operation_provenance: "LexicalRead".to_string(),
        };
        let contribution = FilePirLexicalContributionV1::try_new(partial_draft(
            vec![binding("b0", "x", (4, 5))?],
            vec![occurrence],
            Vec::new(),
        ))
        .map_err(|error| {
            format!("{} is source-backed and must construct: {error}", anchor_kind.name())
        })?;
        assert_eq!(contribution.occurrences().len(), 1);
    }
    Ok(())
}

#[test]
fn fingerprint_covers_the_unsigned_serialization_exactly() -> TestResult {
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(31),
        bindings: vec![binding("b0", "x", (4, 5))?],
        occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
        completeness: ContributionCompleteness::Complete,
        limitations: Vec::new(),
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: None,
    })
    .map_err(|error| format!("complete contribution must construct: {error}"))?;

    let unsigned = contribution
        .unsigned_canonical_json()
        .map_err(|error| format!("unsigned serialization must succeed: {error}"))?;
    let unsigned_top_level =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&unsigned)
            .map_err(|error| format!("unsigned serialization must be a JSON object: {error}"))?;
    assert!(
        !unsigned_top_level.contains_key("fingerprint"),
        "the fingerprint input must omit the envelope's fingerprint field itself"
    );
    assert_eq!(
        contribution.fingerprint(),
        &ContentDigest::of_bytes(unsigned.as_bytes()),
        "stored fingerprint must hash exactly the unsigned canonical bytes"
    );

    let full = contribution
        .canonical_json()
        .map_err(|error| format!("canonical serialization must succeed: {error}"))?;
    let full_top_level = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&full)
        .map_err(|error| format!("canonical serialization must be a JSON object: {error}"))?;
    assert!(full_top_level.contains_key("fingerprint"), "durable envelopes still carry the digest");
    Ok(())
}

#[test]
fn envelope_state_reads_back_through_accessors_only() -> TestResult {
    let join = SemanticSnapshotJoinMetadata {
        snapshot_digest: digest(b"snapshot"),
        generation: 41,
        parser_input_digest: digest(b"parser-input"),
    };
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(41),
        bindings: vec![binding("b0", "x", (4, 5))?, binding("b1", "y", (30, 35))?],
        occurrences: vec![
            declaration_occurrence("o0", "b0", (4, 5))?,
            declaration_occurrence("o1", "b1", (30, 35))?,
        ],
        completeness: ContributionCompleteness::Partial,
        limitations: vec![ContributionLimitation::RecoveredBody],
        work: work(0),
        terminal_disposition: committed(),
        semantic_snapshot_join: Some(join),
    })
    .map_err(|error| format!("partial contribution must construct: {error}"))?;

    assert_eq!(contribution.schema_version(), FILE_PIR_LEXICAL_CONTRIBUTION_SCHEMA_VERSION);
    assert_eq!(contribution.producer(), &producer());
    assert_eq!(contribution.subject(), &subject(41));
    assert_eq!(contribution.completeness(), ContributionCompleteness::Partial);
    assert_eq!(contribution.limitations(), &[ContributionLimitation::RecoveredBody]);
    assert_eq!(contribution.work(), &work(0));
    assert_eq!(contribution.terminal_disposition(), &TerminalDisposition::Committed);
    assert!(contribution.semantic_snapshot_join().is_some());
    assert_eq!(contribution.bindings().len(), 2);
    assert_eq!(contribution.occurrences().len(), 2);
    assert!(!contribution.is_exact());
    Ok(())
}

#[test]
fn fingerprint_is_invariant_under_limitation_order_and_duplicates() -> TestResult {
    let build =
        |limitations: Vec<ContributionLimitation>| -> TestResult<FilePirLexicalContributionV1> {
            FilePirLexicalContributionV1::try_new(partial_draft(
                vec![binding("b0", "x", (4, 5))?],
                vec![declaration_occurrence("o0", "b0", (4, 5))?],
                limitations,
            ))
            .map_err(|error| format!("partial limitation draft must construct: {error}"))
        };

    let ordered = build(vec![
        ContributionLimitation::RecoveredBody,
        ContributionLimitation::DynamicOperation,
    ])?;
    let reversed = build(vec![
        ContributionLimitation::DynamicOperation,
        ContributionLimitation::RecoveredBody,
    ])?;
    let duplicated = build(vec![
        ContributionLimitation::RecoveredBody,
        ContributionLimitation::DynamicOperation,
        ContributionLimitation::RecoveredBody,
    ])?;

    assert_eq!(ordered.fingerprint(), reversed.fingerprint());
    assert_eq!(ordered.fingerprint(), duplicated.fingerprint());
    assert_eq!(
        ordered.limitations(),
        &[ContributionLimitation::RecoveredBody, ContributionLimitation::DynamicOperation],
        "canonical order follows enum declaration order and duplicates collapse"
    );
    Ok(())
}

// ── Accepted review repairs (#12180 discussion_r3848199806 … r3849868383) ──

#[test]
fn stale_binding_fingerprint_is_rejected() -> TestResult {
    // Mutating any binding-identity component while keeping the original
    // fingerprint must fail construction: the envelope never attests a false
    // binding identity (#12180 discussion_r3848199806).
    let original = binding("b0", "x", (4, 5))?;
    let mutated: Vec<LexicalBindingIdentity> = [
        LexicalBindingIdentity { body_id: "body-other".to_string(), ..original.clone() },
        LexicalBindingIdentity { scope_path: vec!["scope-1".to_string()], ..original.clone() },
        LexicalBindingIdentity { sigil: LexicalSigil::Array, ..original.clone() },
        LexicalBindingIdentity { name: "y".to_string(), ..original.clone() },
        LexicalBindingIdentity { declaration_range: (4, 9), ..original.clone() },
    ]
    .into();

    for binding in mutated {
        let error = FilePirLexicalContributionV1::try_new(partial_draft(
            vec![binding],
            Vec::new(),
            Vec::new(),
        ));
        assert_eq!(
            error.err().ok_or("a stale binding fingerprint must be rejected")?,
            ContributionError::BindingFingerprintMismatch { binding_id: "b0".to_string() },
            "mutated identity fields with a stale fingerprint must fail construction"
        );
    }

    // Re-deriving the fingerprint after the mutation validates again, and the
    // corrected binding changes the envelope fingerprint.
    let mut corrected = original.clone();
    corrected.name = "y".to_string();
    corrected.fingerprint = LexicalBindingIdentity::fingerprint_for(
        &corrected.body_id,
        &corrected.scope_path,
        corrected.sigil,
        &corrected.name,
        corrected.declaration_range,
    )
    .map_err(|error| format!("fingerprint re-derivation must succeed: {error}"))?;

    let baseline = FilePirLexicalContributionV1::try_new(partial_draft(
        vec![original],
        Vec::new(),
        Vec::new(),
    ))
    .map_err(|error| format!("original binding must construct: {error}"))?;
    let changed = FilePirLexicalContributionV1::try_new(partial_draft(
        vec![corrected],
        Vec::new(),
        Vec::new(),
    ))
    .map_err(|error| format!("re-derived binding must construct: {error}"))?;
    assert_ne!(
        baseline.fingerprint(),
        changed.fingerprint(),
        "a re-derived binding identity must change the envelope fingerprint"
    );
    Ok(())
}

#[test]
fn complete_but_superseded_or_withdrawn_is_not_exact() -> TestResult {
    // A non-committed terminal disposition never authorizes an exact answer,
    // even when the completeness axis says Complete (#12180
    // discussion_r3848199812). Both records still construct: they are
    // representable terminal history, just never exact.
    let draft =
        |terminal_disposition: TerminalDisposition| -> TestResult<FilePirLexicalContributionV1> {
            FilePirLexicalContributionV1::try_new(ContributionDraft {
                producer: producer(),
                subject: subject(7),
                bindings: vec![binding("b0", "x", (4, 5))?],
                occurrences: vec![declaration_occurrence("o0", "b0", (4, 5))?],
                completeness: ContributionCompleteness::Complete,
                limitations: Vec::new(),
                work: work(0),
                terminal_disposition,
                semantic_snapshot_join: None,
            })
            .map_err(|error| format!("construction failed: {error}"))
        };

    let superseded =
        draft(TerminalDisposition::SupersededBy { successor_fingerprint: digest(b"successor") })
            .map_err(|error| format!("complete superseded record must construct: {error}"))?;
    assert!(!superseded.is_exact(), "Complete + SupersededBy must never be exact");

    let withdrawn = draft(TerminalDisposition::Withdrawn { reason: "replaced".to_string() })
        .map_err(|error| format!("complete withdrawn record must construct: {error}"))?;
    assert!(!withdrawn.is_exact(), "Complete + Withdrawn must never be exact");

    let committed = draft(committed())
        .map_err(|error| format!("complete committed record must construct: {error}"))?;
    assert!(committed.is_exact(), "Complete + Committed is the only exact combination");
    Ok(())
}

#[test]
fn occurrence_anchor_identity_traces_back_to_the_canonical_anchor() -> TestResult {
    // The envelope anchor projection is mechanically derived from the
    // canonical #12191 PirSourceAnchor: kind, range, anchor id, and hir_item
    // provenance all travel with the fact (#12180 discussion_r3849868378).
    let pir_anchor =
        PirSourceAnchor::explicit(SourceLocation { start: 12, end: 17 }, HirId::from_index(3));
    let snapshot =
        OccurrenceAnchor::from_pir_anchor(&pir_anchor).ok_or("explicit anchors must snapshot")?;
    assert_eq!(snapshot.anchor_kind, PirAnchorKind::ExplicitSource);
    assert_eq!(snapshot.range, (12, 17));
    assert_eq!(snapshot.anchor_id, pir_anchor.anchor_id.ok_or("explicit anchors carry an id")?);
    assert_eq!(snapshot.hir_item_index, Some(3));

    // An anchor id that does not match the canonical derivation for its range
    // cannot be attested.
    let occurrence = ContributionOccurrence {
        occurrence_id: "o-foreign".to_string(),
        binding_id: "b0".to_string(),
        role: OccurrenceRole::Read,
        anchor: OccurrenceAnchor {
            anchor_kind: PirAnchorKind::ExplicitSource,
            range: (20, 25),
            anchor_id: AnchorId(999),
            hir_item_index: Some(0),
        },
        operation_provenance: "LexicalRead".to_string(),
    };
    let error = FilePirLexicalContributionV1::try_new(partial_draft(
        vec![binding("b0", "x", (4, 5))?],
        vec![occurrence],
        Vec::new(),
    ));
    assert_eq!(
        error.err().ok_or("a foreign anchor id must be rejected")?,
        ContributionError::InconsistentAnchorIdentity { occurrence_id: "o-foreign".to_string() },
        "anchor ids must match the canonical derivation for their range"
    );
    Ok(())
}
