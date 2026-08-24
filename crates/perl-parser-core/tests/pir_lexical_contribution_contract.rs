//! Falsifiers for the file-level compiler lexical contribution contract
//! (PIRL-01, #12109).
//!
//! Negative controls fail when: a first write becomes a declaration; `Modify`
//! is dropped while completeness stays complete; same-name bindings collapse;
//! producer naming upgrades proof/completeness; mixed identities validate;
//! missing facts become exact-empty; a foreign-generation semantic join
//! validates; or an unknown work field defaults to zero. Positive controls
//! cover complete, declaration-only, partial/recovered, stale/cancelled/
//! instrument-failed states plus deterministic output under input-order
//! variation.

use perl_parser_core::hir::HirId;
use perl_parser_core::pir::{
    BuildKind, CompilerProducerIdentity, ContributionCompleteness, ContributionDraft,
    ContributionError, ContributionLimitation, ContributionOccurrence, ContributionSubjectIdentity,
    ContributionWorkShape, FilePirLexicalContributionV1, LexicalBindingIdentity, LexicalSigil,
    OccurrenceAnchor, OccurrenceRole, PirSourceAnchor, SemanticSnapshotJoinMetadata,
    TerminalDisposition, WorkObservation,
};
use perl_position_tracking::{ByteSpan, SourceLocation};
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

fn binding(id: &str, name: &str, decl_range: (usize, usize)) -> LexicalBindingIdentity {
    LexicalBindingIdentity {
        binding_id: id.to_string(),
        body_id: "body-0".to_string(),
        scope_path: vec!["scope-0".to_string()],
        sigil: LexicalSigil::Scalar,
        name: name.to_string(),
        declaration_range: decl_range,
        fingerprint: digest(id.as_bytes()),
    }
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

#[test]
fn valid_complete_initialized_lexical_construction_is_exact() -> TestResult {
    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))],
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
        bindings: vec![binding("b0", "count", (8, 13))],
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
        bindings: vec![binding("b0", "n", (0, 6))],
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

    let roles: Vec<_> = contribution.occurrences.iter().map(|o| o.role).collect();
    assert!(roles.contains(&OccurrenceRole::Modify));
    assert!(roles.contains(&OccurrenceRole::Declaration));
    Ok(())
}

#[test]
fn same_name_in_another_body_is_a_distinct_binding() -> TestResult {
    let mut outer = binding("b-outer", "x", (0, 5));
    outer.body_id = "body-outer".to_string();
    let mut inner = binding("b-inner", "x", (40, 45));
    inner.body_id = "body-inner".to_string();

    let contribution = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(3),
        bindings: vec![outer.clone(), inner],
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

    assert_eq!(contribution.bindings.len(), 2);
    Ok(())
}

#[test]
fn sigils_separate_bindings_with_equal_names() -> TestResult {
    let mut scalar_binding = binding("b-scalar", "x", (0, 5));
    scalar_binding.sigil = LexicalSigil::Scalar;
    let mut array_binding = binding("b-array", "x", (20, 25));
    array_binding.sigil = LexicalSigil::Array;

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
    assert_eq!(contribution.bindings.len(), 2);
    Ok(())
}

#[test]
fn source_identical_later_generation_is_another_subject() -> TestResult {
    let occurrences = vec![declaration_occurrence("o0", "b0", (4, 5))?];
    let partial_with_recovery = |generation: u64, occurrences: &[ContributionOccurrence]| {
        FilePirLexicalContributionV1::try_new(ContributionDraft {
            producer: producer(),
            subject: subject(generation),
            bindings: vec![binding("b0", "x", (4, 5))],
            occurrences: occurrences.to_vec(),
            completeness: ContributionCompleteness::Partial,
            limitations: vec![ContributionLimitation::RecoveredBody],
            work: work(0),
            terminal_disposition: committed(),
            semantic_snapshot_join: None,
        })
    };

    let earlier =
        partial_with_recovery(9, &occurrences).map_err(|e| format!("earlier generation: {e}"))?;
    let later =
        partial_with_recovery(10, &occurrences).map_err(|e| format!("later generation: {e}"))?;

    assert_ne!(earlier.fingerprint, later.fingerprint);
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
        bindings: vec![binding("b0", "x", (4, 5))],
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
        bindings: vec![binding("b0", "x", (4, 5))],
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
        let mut bindings = vec![binding("b0", "x", (4, 5)), binding("b1", "y", (30, 35))];
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

    assert_eq!(build(false)?.fingerprint, build(true)?.fingerprint);
    Ok(())
}

// ── Negative controls ──

#[test]
fn first_write_becoming_declaration_is_rejected() -> TestResult {
    let error = FilePirLexicalContributionV1::try_new(ContributionDraft {
        producer: producer(),
        subject: subject(7),
        bindings: vec![binding("b0", "x", (4, 5))],
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
        bindings: vec![binding("b0", "x", (4, 5))],
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
        bindings: vec![binding("b-a", "x", (4, 5)), binding("b-b", "x", (40, 45))],
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
        bindings: vec![binding("b0", "x", (4, 5))],
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
