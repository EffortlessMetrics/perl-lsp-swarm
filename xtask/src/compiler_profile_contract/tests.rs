//! Discriminating falsifiers for the maintained compiler-profile model
//! (#12186).
//!
//! Every test below is named after the falsifier it pins from the issue body.
//! An implementation that omits a closure or identity law, collapses an
//! independent axis, admits issue/workflow state as evidence, or introduces a
//! scalar readiness score fails at least one named test here.
//!
//! The four shape fixtures prove representability and closure only: they are
//! minimal in-memory shapes for the #12176 profile classes, not the checked
//! repository row inventory (owned by the successor initial-row inventory).

use std::collections::BTreeMap;

use super::model::{
    AllowedLimitation, ClaimCeiling, ClaimFamily, CompilerProfileDefinition, CompilerProfileId,
    CompilerProfileImport, CompilerProfileRow, CompilerProfileRowId, CompilerProfileVersion,
    CompletenessRequirement, CompletenessRule, EvidenceObservation, EvidenceRequirement,
    InvalidationInput, LegacyExitRequirement, LimitationPolicy, OwnerAndWakeEvent, OwnerToken,
    ProofClass, RowDisposition, SourceTier, SubjectSelector, WakeEvent, WorkClass, WorkObservation,
    WorkRequirement,
};
use super::{CompilerProfileContractError, ProfileDigest};

// ---------------------------------------------------------------------------
// Shape fixtures for the four #12176 profile classes
// ---------------------------------------------------------------------------

fn token_owner(
    owner: &str,
    wake_event: WakeEvent,
) -> Result<OwnerAndWakeEvent, CompilerProfileContractError> {
    Ok(OwnerAndWakeEvent { owner: OwnerToken::new(owner)?, wake_event })
}

fn evidence(
    classes: &[ProofClass],
    tiers: &[SourceTier],
) -> Result<EvidenceRequirement, CompilerProfileContractError> {
    EvidenceRequirement::new(classes.iter().copied().collect(), tiers.iter().copied().collect())
}

fn insert_row(
    rows: &mut BTreeMap<CompilerProfileRowId, CompilerProfileRow>,
    row: CompilerProfileRow,
) -> Result<(), CompilerProfileContractError> {
    if rows.insert(row.row_id.clone(), row).is_some() {
        return Err(CompilerProfileContractError::Schema {
            field: "fixture.rows".to_string(),
            message: "duplicate fixture row id".to_string(),
        });
    }
    Ok(())
}

fn insert_limitation(
    limitations: &mut BTreeMap<String, AllowedLimitation>,
    id: &str,
    boundary: &str,
    owner: &str,
    wake_event: WakeEvent,
) -> Result<(), CompilerProfileContractError> {
    if limitations
        .insert(
            id.to_string(),
            AllowedLimitation {
                boundary: boundary.to_string(),
                owner: token_owner(owner, wake_event)?,
            },
        )
        .is_some()
    {
        return Err(CompilerProfileContractError::Schema {
            field: "fixture.limitations".to_string(),
            message: "duplicate fixture limitation id".to_string(),
        });
    }
    Ok(())
}

/// A row skeleton carrying the neutral defaults; fixture rows override the
/// semantic fields with struct-update syntax.
fn shape_row(
    row_id: &str,
    statement: &str,
    disposition: RowDisposition,
    subject: SubjectSelector,
    owner: &str,
    wake_event: WakeEvent,
) -> Result<CompilerProfileRow, CompilerProfileContractError> {
    Ok(CompilerProfileRow {
        row_id: CompilerProfileRowId::new(row_id)?,
        statement: statement.to_string(),
        disposition,
        subject,
        evidence: EvidenceRequirement::new(
            [ProofClass::GeneralSemanticSupport].into(),
            [SourceTier::Source].into(),
        )?,
        completeness: CompletenessRequirement { rule: CompletenessRule::CurrentSubjectState },
        work: None,
        limitation_policy: LimitationPolicy::Unbounded,
        legacy_exit: None,
        owner: token_owner(owner, wake_event)?,
        invalidation: [InvalidationInput::SubjectChange].into(),
        claim_ceiling: ClaimCeiling::profile_evidence(),
    })
}

/// `compiler_local_lexical.v1`: the bounded first production/compiler
/// transaction. No imports, no long-horizon axes.
fn local_lexical_shape() -> Result<CompilerProfileDefinition, CompilerProfileContractError> {
    let mut limitations = BTreeMap::new();
    insert_limitation(
        &mut limitations,
        "no.compiler.world",
        "No compiler world, cross-file authority, or project-graph claims.",
        "compiler.program",
        WakeEvent::WorldSnapshotMovement,
    )?;
    insert_limitation(
        &mut limitations,
        "no.eir.execution",
        "No EIR, bounded execution, curated gold, or real-Perl oracle claims.",
        "compiler.program",
        WakeEvent::InterfaceTransition,
    )?;
    insert_limitation(
        &mut limitations,
        "no.upstream.breadth",
        "No broad upstream target, all-provider, installed-client, or all-Perl claims.",
        "compiler.program",
        WakeEvent::UpstreamSeriesMovement,
    )?;

    let mut rows = BTreeMap::new();

    let upstream_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::ObservedUpstreamResult], &[SourceTier::Source])?,
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.upstream.breadth".to_string()].into(),
        },
        invalidation: [InvalidationInput::UpstreamSeriesMovement, InvalidationInput::SubjectChange]
            .into(),
        ..shape_row(
            "observed.upstream.selected",
            "Selected upstream base/comp/run parse+compile observation.",
            RowDisposition::Required,
            SubjectSelector::SelectedUpstreamSeries {
                series: "base-comp-run-selected".to_string(),
            },
            "compiler.program",
            WakeEvent::UpstreamSeriesMovement,
        )?
    };
    insert_row(&mut rows, upstream_row)?;

    let debt_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::AcceptedCompatibilityState, ProofClass::ReplacementCurrentness],
            &[SourceTier::Source],
        )?,
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.upstream.breadth".to_string()].into(),
        },
        legacy_exit: Some(LegacyExitRequirement {
            legacy_path: "legacy-reference-heuristics".to_string(),
            required_proof: [ProofClass::OldPathAbsence, ProofClass::RecurrenceProof].into(),
        }),
        invalidation: [InvalidationInput::SubjectChange, InvalidationInput::ReviewRulingChange]
            .into(),
        ..shape_row(
            "debt.retirement.accepted",
            "Accepted general semantic debt retirement with exact legacy exit.",
            RowDisposition::Required,
            SubjectSelector::AcceptedDebtLedger,
            "compiler.program",
            WakeEvent::ReviewRulingChange,
        )?
    };
    insert_row(&mut rows, debt_row)?;

    let facts_row = CompilerProfileRow {
        evidence: evidence(
            &[
                ProofClass::ParserFactProduction,
                ProofClass::SemanticFactProduction,
                ProofClass::PirFactProduction,
            ],
            &[SourceTier::Source],
        )?,
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.eir.execution".to_string()].into(),
        },
        ..shape_row(
            "facts.parser.semantic.pir",
            "Current accepted parser/semantic/PIR generation.",
            RowDisposition::Required,
            SubjectSelector::CompilerPipelineFacts,
            "facts.train",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, facts_row)?;

    let lexical_row = CompilerProfileRow {
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.compiler.world".to_string()].into(),
        },
        invalidation: [InvalidationInput::EditTouch, InvalidationInput::SubjectChange].into(),
        ..shape_row(
            "lexicals.same-file.initialized",
            "Same-file initialized lexical references.",
            RowDisposition::Required,
            SubjectSelector::SameFileInitializedLexicals,
            "facts.train",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, lexical_row)?;

    let rename_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::EditAuthorization], &[SourceTier::Source])?,
        completeness: CompletenessRequirement {
            rule: CompletenessRule::ExactDenominator {
                denominator_id: "same-occurrence-denominator".to_string(),
            },
        },
        legacy_exit: Some(LegacyExitRequirement {
            legacy_path: "legacy-reference-heuristics".to_string(),
            required_proof: [ProofClass::OldPathAbsence, ProofClass::RecurrenceProof].into(),
        }),
        invalidation: [InvalidationInput::EditTouch].into(),
        ..shape_row(
            "rename.same-file.complete-or-refuse",
            "Complete-or-refuse rename from the same occurrence denominator.",
            RowDisposition::Required,
            SubjectSelector::SameFileCompleteOrRefuseRename,
            "edit.authorization",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, rename_row)?;

    let stdio_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::TestReachability], &[SourceTier::ExactProcess])?,
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        work: Some(WorkRequirement::new(WorkClass::Production, 1)?),
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.upstream.breadth".to_string()].into(),
        },
        invalidation: [
            InvalidationInput::ToolchainOrClientUpgrade,
            InvalidationInput::SubjectChange,
        ]
        .into(),
        ..shape_row(
            "stdio.product.proof.exact-process",
            "Exact external perllsp stdio product proof.",
            RowDisposition::Required,
            SubjectSelector::ExactEditorProductSurface { product: "perllsp-stdio".to_string() },
            "product.proof",
            WakeEvent::ClientOrToolchainUpgrade,
        )?
    };
    insert_row(&mut rows, stdio_row)?;

    CompilerProfileDefinition::new(
        CompilerProfileId::new("compiler_local_lexical")?,
        CompilerProfileVersion::new("v1")?,
        "Bounded first production/compiler transaction.".to_string(),
        [].into(),
        rows,
        limitations,
    )
}

fn import_of(
    lower: &CompilerProfileDefinition,
) -> Result<CompilerProfileImport, CompilerProfileContractError> {
    Ok(CompilerProfileImport {
        profile_id: lower.profile_id.clone(),
        version: lower.version.clone(),
        digest: lower.semantic_fingerprint()?,
    })
}

/// `compiler_static_project.v1`: imports the exact local lexical profile and
/// adds project/world rows.
fn static_project_shape(
    local: &CompilerProfileDefinition,
) -> Result<CompilerProfileDefinition, CompilerProfileContractError> {
    let mut limitations = local.limitations.clone();
    insert_limitation(
        &mut limitations,
        "dynamic.runtime.ambient.classes",
        "Dynamic, runtime-only, ambient and project-execution classes remain typed limitations \
         unless separately admitted.",
        "compiler.program",
        WakeEvent::WorldSnapshotMovement,
    )?;

    let mut rows = local.rows.clone();

    let world_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::ProjectWorldCurrentness], &[SourceTier::Source])?,
        invalidation: [InvalidationInput::WorldSnapshotMovement, InvalidationInput::EditTouch]
            .into(),
        ..shape_row(
            "world.snapshot.current",
            "ProjectFactShard/ProjectModel and CompilerWorldSnapshot currentness.",
            RowDisposition::Required,
            SubjectSelector::ProjectWorldSnapshot,
            "project.world",
            WakeEvent::WorldSnapshotMovement,
        )?
    };
    insert_row(&mut rows, world_row)?;

    let graph_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::ProjectWorldCurrentness, ProofClass::CrossFileExternalBehavior],
            &[SourceTier::Source],
        )?,
        invalidation: [
            InvalidationInput::DependencyGraphChange,
            InvalidationInput::InterfaceTransition,
        ]
        .into(),
        ..shape_row(
            "dependency.graph.scc",
            "Compile-time dependency graph and SCC scheduling.",
            RowDisposition::Required,
            SubjectSelector::CompileTimeDependencyGraph,
            "project.world",
            WakeEvent::DependencyGraphChange,
        )?
    };
    insert_row(&mut rows, graph_row)?;

    let navigation_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::CrossFileExternalBehavior, ProofClass::ProviderConsumption],
            &[SourceTier::Source],
        )?,
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["dynamic.runtime.ambient.classes".to_string()].into(),
        },
        ..shape_row(
            "navigation.cross-file.admitted",
            "Admitted compiler-world-backed cross-file navigation.",
            RowDisposition::Conditional {
                condition: "compiler-world admission ruling is current".to_string(),
            },
            SubjectSelector::CrossFileNavigation,
            "provider.surface",
            WakeEvent::WorldSnapshotMovement,
        )?
    };
    insert_row(&mut rows, navigation_row)?;

    let refactor_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::CrossFileExternalBehavior, ProofClass::EditAuthorization],
            &[SourceTier::Source],
        )?,
        completeness: CompletenessRequirement {
            rule: CompletenessRule::ExactDenominator {
                denominator_id: "cross-file-occurrence-denominator".to_string(),
            },
        },
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["dynamic.runtime.ambient.classes".to_string()].into(),
        },
        ..shape_row(
            "refactor.cross-file.complete-or-refuse",
            "Admitted complete-or-refuse cross-file refactor.",
            RowDisposition::Conditional {
                condition: "compiler-world admission ruling is current".to_string(),
            },
            SubjectSelector::CrossFileRefactor,
            "edit.authorization",
            WakeEvent::WorldSnapshotMovement,
        )?
    };
    insert_row(&mut rows, refactor_row)?;

    let cold_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::ProjectWorldCurrentness], &[SourceTier::Source])?,
        completeness: CompletenessRequirement {
            rule: CompletenessRule::RepresentativeSample {
                sample_id: "representative-projects".to_string(),
            },
        },
        work: Some(WorkRequirement::new(WorkClass::OracleCold, 3)?),
        ..shape_row(
            "cold.equivalence.representative",
            "Representative-project cold-equivalence, lifecycle, work and cleanup proof.",
            RowDisposition::Required,
            SubjectSelector::ProjectWorldSnapshot,
            "project.world",
            WakeEvent::WorldSnapshotMovement,
        )?
    };
    insert_row(&mut rows, cold_row)?;

    CompilerProfileDefinition::new(
        CompilerProfileId::new("compiler_static_project")?,
        CompilerProfileVersion::new("v1")?,
        "Project/world static profile importing the exact local lexical profile.".to_string(),
        [import_of(local)?].into(),
        rows,
        limitations,
    )
}

/// `compiler_bounded_execution.v1`: imports the exact static project profile
/// and adds reviewed bounded execution rows.
fn bounded_execution_shape(
    static_project: &CompilerProfileDefinition,
) -> Result<CompilerProfileDefinition, CompilerProfileContractError> {
    let mut limitations = static_project.limitations.clone();
    insert_limitation(
        &mut limitations,
        "no.arbitrary.execution",
        "No arbitrary project Perl or command execution in editor requests.",
        "bounded.execution",
        WakeEvent::SubjectRecurrence,
    )?;

    let mut rows = static_project.rows.clone();

    let gold_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::CuratedExpectation], &[SourceTier::Source])?,
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        work: Some(WorkRequirement::new(WorkClass::OracleCold, 2)?),
        ..shape_row(
            "curated.gold.independent",
            "Independently authored curated gold.",
            RowDisposition::Required,
            SubjectSelector::CuratedGoldExpectations,
            "gold.authors",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, gold_row)?;

    let oracle_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::RealPerlOracle], &[SourceTier::Source])?,
        work: Some(WorkRequirement::new(WorkClass::OracleCold, 5)?),
        ..shape_row(
            "oracle.real-perl.differential",
            "Hermetic real-Perl differential evidence.",
            RowDisposition::Required,
            SubjectSelector::RealPerlOracleRows,
            "oracle.owners",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, oracle_row)?;

    let eir_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::EirMechanism, ProofClass::EvaluatedWork],
            &[SourceTier::Source],
        )?,
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        work: Some(WorkRequirement::new(WorkClass::Production, 10)?),
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.arbitrary.execution".to_string()].into(),
        },
        ..shape_row(
            "eir.lowering.and.evaluation",
            "Verified EIR lowering and bounded evaluation for admitted effects.",
            RowDisposition::Required,
            SubjectSelector::EirAdmittedEffects,
            "eir.mechanism",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, eir_row)?;

    let tap_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::EirMechanism], &[SourceTier::Source])?,
        ..shape_row(
            "tap.rows.via.general.eir",
            "Selected upstream TAP rows migrated through general EIR semantics.",
            RowDisposition::Conditional {
                condition: "upstream TAP row selection is current".to_string(),
            },
            SubjectSelector::EirAdmittedEffects,
            "eir.mechanism",
            WakeEvent::UpstreamSeriesMovement,
        )?
    };
    insert_row(&mut rows, tap_row)?;

    let unsupported_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::GeneralSemanticSupport], &[SourceTier::Source])?,
        limitation_policy: LimitationPolicy::BoundedBy {
            limitation_ids: ["no.arbitrary.execution".to_string()].into(),
        },
        ..shape_row(
            "dynamic.boundaries.unsupported",
            "Unsupported dynamic/magic/XS/tie/ambient boundaries.",
            RowDisposition::Unsupported {
                reason: "not admitted under bounded execution; retire per construct".to_string(),
            },
            SubjectSelector::UnsupportedDynamicBoundaries,
            "bounded.execution",
            WakeEvent::SubjectRecurrence,
        )?
    };
    insert_row(&mut rows, unsupported_row)?;

    CompilerProfileDefinition::new(
        CompilerProfileId::new("compiler_bounded_execution")?,
        CompilerProfileVersion::new("v1")?,
        "Reviewed bounded compile-time execution/correctness mechanisms.".to_string(),
        [import_of(static_project)?].into(),
        rows,
        limitations,
    )
}

/// `compiler_maintained_code_intelligence.v1`: composes the lower profiles and
/// adds exact maintained product rows.
fn maintained_shape(
    bounded: &CompilerProfileDefinition,
) -> Result<CompilerProfileDefinition, CompilerProfileContractError> {
    let limitations = bounded.limitations.clone();
    let mut rows = bounded.rows.clone();

    let denominator_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::TestReachability],
            &[
                SourceTier::Source,
                SourceTier::ExactProcess,
                SourceTier::Packaged,
                SourceTier::InstalledHost,
                SourceTier::ActualClient,
            ],
        )?,
        completeness: CompletenessRequirement {
            rule: CompletenessRule::ExactDenominator {
                denominator_id: "maintained-target-denominator".to_string(),
            },
        },
        ..shape_row(
            "maintained.denominator.exact",
            "Maintained Perl/compiler/upstream target denominator.",
            RowDisposition::Required,
            SubjectSelector::MaintainedTargetDenominator,
            "maintained.program",
            WakeEvent::UpstreamSeriesMovement,
        )?
    };
    insert_row(&mut rows, denominator_row)?;

    let journey_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::TestReachability],
            &[SourceTier::Packaged, SourceTier::ExactProcess],
        )?,
        work: Some(WorkRequirement::new(WorkClass::Production, 25)?),
        ..shape_row(
            "release.journey.packaged",
            "Exact release-shaped package and process journey.",
            RowDisposition::Required,
            SubjectSelector::ReleaseProcessJourney,
            "release.owners",
            WakeEvent::ClientOrToolchainUpgrade,
        )?
    };
    insert_row(&mut rows, journey_row)?;

    let client_row = CompilerProfileRow {
        evidence: evidence(
            &[ProofClass::TestReachability, ProofClass::ProviderConsumption],
            &[SourceTier::InstalledHost, SourceTier::ActualClient],
        )?,
        completeness: CompletenessRequirement {
            rule: CompletenessRule::ExactDenominator {
                denominator_id: "installed-client-denominator".to_string(),
            },
        },
        ..shape_row(
            "installed.client.selected",
            "One manifest-selected installed client/plugin/platform denominator.",
            RowDisposition::Required,
            SubjectSelector::MaintainedTargetDenominator,
            "client.compatibility",
            WakeEvent::ClientOrToolchainUpgrade,
        )?
    };
    insert_row(&mut rows, client_row)?;

    let performance_row = CompilerProfileRow {
        evidence: evidence(&[ProofClass::PerformanceResourceResult], &[SourceTier::Source])?,
        work: Some(WorkRequirement::new(WorkClass::PerformanceResource, 100)?),
        completeness: CompletenessRequirement { rule: CompletenessRule::ExhaustiveCoverage },
        invalidation: [InvalidationInput::SubjectChange, InvalidationInput::WorldSnapshotMovement]
            .into(),
        ..shape_row(
            "performance.envelope.bounded",
            "Finite correctness-bound performance/resource envelopes.",
            RowDisposition::Required,
            SubjectSelector::PerformanceResourceEnvelope,
            "performance.owners",
            WakeEvent::WorldSnapshotMovement,
        )?
    };
    insert_row(&mut rows, performance_row)?;

    CompilerProfileDefinition::new(
        CompilerProfileId::new("compiler_maintained_code_intelligence")?,
        CompilerProfileVersion::new("v1")?,
        "Composed maintained code-intelligence profile.".to_string(),
        [import_of(bounded)?].into(),
        rows,
        limitations,
    )
}

type ShapeChain = (
    CompilerProfileDefinition,
    CompilerProfileDefinition,
    CompilerProfileDefinition,
    CompilerProfileDefinition,
);

/// All four shape fixtures in chain order.
fn shape_chain() -> Result<ShapeChain, CompilerProfileContractError> {
    let local = local_lexical_shape()?;
    let static_project = static_project_shape(&local)?;
    let bounded = bounded_execution_shape(&static_project)?;
    let maintained = maintained_shape(&bounded)?;
    Ok((local, static_project, bounded, maintained))
}

fn all_shapes() -> Result<Vec<CompilerProfileDefinition>, CompilerProfileContractError> {
    let (local, static_project, bounded, maintained) = shape_chain()?;
    Ok(vec![local, static_project, bounded, maintained])
}

fn row_id_of(value: &str) -> Result<CompilerProfileRowId, CompilerProfileContractError> {
    CompilerProfileRowId::new(value)
}

fn alternate_digest(seed: u8) -> Result<ProfileDigest, CompilerProfileContractError> {
    let hex: String = std::iter::repeat_n(format!("{seed:02x}"), 32).collect();
    ProfileDigest::from_hex(&hex)
}

// ---------------------------------------------------------------------------
// Representability, closure, and round-trip
// ---------------------------------------------------------------------------

#[test]
fn shape_fixtures_validate_close_and_round_trip() -> Result<(), CompilerProfileContractError> {
    let (local, static_project, bounded, maintained) = shape_chain()?;
    local.validate_closure(&[])?;
    static_project.validate_closure(&[&local])?;
    bounded.validate_closure(&[&static_project])?;
    maintained.validate_closure(&[&local, &static_project, &bounded])?;

    for shape in all_shapes()? {
        let canonical = shape.to_canonical_json()?;
        let round_tripped = CompilerProfileDefinition::from_json_str(&canonical)?;
        assert_eq!(round_tripped, shape, "canonical JSON must round-trip losslessly");
        assert_eq!(
            round_tripped.semantic_fingerprint()?,
            shape.semantic_fingerprint()?,
            "fingerprints must be deterministic"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifiers from the issue body
// ---------------------------------------------------------------------------

#[test]
fn f01_local_lexical_pass_cannot_stand_in_for_a_stronger_profile()
-> Result<(), CompilerProfileContractError> {
    let (local, static_project, _bounded, maintained) = shape_chain()?;

    // Distinct identities: no two profile shapes share a fingerprint.
    let shapes = all_shapes()?;
    let mut fingerprints = Vec::new();
    for shape in &shapes {
        fingerprints.push(shape.semantic_fingerprint()?.as_str().to_string());
    }
    let unique: std::collections::BTreeSet<&String> = fingerprints.iter().collect();
    assert_eq!(
        unique.len(),
        fingerprints.len(),
        "each profile shape must have a distinct fingerprint"
    );

    // Local lexical evidence cannot satisfy the static project's added rows.
    let local_observations: Vec<EvidenceObservation> = local
        .rows
        .values()
        .filter_map(|row| {
            let class = row.evidence.required_classes.iter().next().copied()?;
            let tier = row.evidence.required_tiers.iter().next().copied()?;
            Some(EvidenceObservation { class, tier })
        })
        .collect();
    let world_row = static_project.rows.get(&row_id_of("world.snapshot.current")?).cloned().ok_or(
        CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: "missing world.snapshot.current".to_string(),
        },
    )?;
    assert!(
        !world_row.evidence.is_satisfied_by(&local_observations),
        "local lexical evidence must not satisfy project/world currentness"
    );

    // A stronger identity is a different profile: its fingerprint is not the
    // bounded local profile's.
    assert_ne!(
        maintained.semantic_fingerprint()?,
        local.semantic_fingerprint()?,
        "stronger profile identity must differ from the bounded local profile"
    );
    Ok(())
}

#[test]
fn f02_bounded_local_profile_requires_no_long_horizon_work()
-> Result<(), CompilerProfileContractError> {
    let local = local_lexical_shape()?;
    local.validate()?;
    local.validate_closure(&[])?;

    let long_horizon_subjects = [
        SubjectSelector::CuratedGoldExpectations,
        SubjectSelector::RealPerlOracleRows,
        SubjectSelector::EirAdmittedEffects,
        SubjectSelector::MaintainedTargetDenominator,
        SubjectSelector::ReleaseProcessJourney,
        SubjectSelector::PerformanceResourceEnvelope,
        SubjectSelector::ProjectWorldSnapshot,
        SubjectSelector::CompileTimeDependencyGraph,
    ];
    let long_horizon_classes = [
        ProofClass::EirMechanism,
        ProofClass::EvaluatedWork,
        ProofClass::CuratedExpectation,
        ProofClass::RealPerlOracle,
        ProofClass::ProjectWorldCurrentness,
        ProofClass::CrossFileExternalBehavior,
        ProofClass::PerformanceResourceResult,
    ];
    for row in local.rows.values() {
        assert!(
            !long_horizon_subjects.contains(&row.subject),
            "local lexical profile must not carry long-horizon subjects"
        );
        for class in &row.evidence.required_classes {
            assert!(
                !long_horizon_classes.contains(class),
                "local lexical profile must not require long-horizon proof classes"
            );
        }
    }
    assert!(local.imports.is_empty(), "the bounded local profile imports nothing");
    Ok(())
}

#[test]
fn f03_issue_pr_workflow_state_is_absent_from_the_evidence_model()
-> Result<(), CompilerProfileContractError> {
    let forbidden_tokens = [
        "issue",
        "pull_request",
        "pullrequest",
        "workflow",
        "check_run",
        "merge_state",
        "ci_status",
    ];

    for class in ProofClass::ALL {
        let name = class.as_str();
        for token in forbidden_tokens {
            assert!(!name.contains(token), "ProofClass `{name}` smuggles workflow state");
        }
    }
    for tier in SourceTier::ALL {
        let name = tier.as_str();
        for token in forbidden_tokens {
            assert!(!name.contains(token), "SourceTier `{name}` smuggles workflow state");
        }
    }

    // No fixture serialization carries workflow-shaped fields.
    for shape in all_shapes()? {
        let json = shape.to_canonical_json()?.to_lowercase();
        for token in forbidden_tokens {
            assert!(!json.contains(token), "profile serialization must not carry `{token}` state");
        }
    }

    // An observation is exactly a proof class plus a source tier.
    let observation =
        EvidenceObservation { class: ProofClass::TestReachability, tier: SourceTier::Source };
    let serialized = serde_json::to_string(&observation).map_err(|error| {
        CompilerProfileContractError::Schema {
            field: "observation".to_string(),
            message: error.to_string(),
        }
    })?;
    assert_eq!(serialized, r#"{"class":"test_reachability","tier":"source"}"#);
    Ok(())
}

#[test]
fn f04_parser_proof_cannot_satisfy_provider_edit_or_installed_host_proof()
-> Result<(), CompilerProfileContractError> {
    let provider_requirement = evidence(&[ProofClass::ProviderConsumption], &[SourceTier::Source])?;
    assert!(!provider_requirement.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::ParserFactProduction,
        tier: SourceTier::Source,
    }]));
    assert!(!provider_requirement.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::EditAuthorization,
        tier: SourceTier::Source,
    }]));
    assert!(provider_requirement.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::ProviderConsumption,
        tier: SourceTier::Source,
    }]));

    let installed_requirement =
        evidence(&[ProofClass::TestReachability], &[SourceTier::InstalledHost])?;
    assert!(
        !installed_requirement.is_satisfied_by(&[EvidenceObservation {
            class: ProofClass::TestReachability,
            tier: SourceTier::Source,
        }]),
        "source-stage proof must not satisfy installed-host proof"
    );
    assert!(installed_requirement.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::TestReachability,
        tier: SourceTier::InstalledHost,
    }]));

    // Several axes are conjunctive: one axis never covers the other.
    let multi_axis = evidence(
        &[ProofClass::ProviderConsumption, ProofClass::EditAuthorization],
        &[SourceTier::Source],
    )?;
    assert!(!multi_axis.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::ProviderConsumption,
        tier: SourceTier::Source,
    }]));
    assert!(multi_axis.is_satisfied_by(&[
        EvidenceObservation { class: ProofClass::ProviderConsumption, tier: SourceTier::Source },
        EvidenceObservation { class: ProofClass::EditAuthorization, tier: SourceTier::Source },
    ]));

    // Empty axes are not constructible: no requirement can be vacuously closed.
    assert!(EvidenceRequirement::new([].into(), [SourceTier::Source].into()).is_err());
    assert!(EvidenceRequirement::new([ProofClass::ProviderConsumption].into(), [].into()).is_err());
    Ok(())
}

#[test]
fn f05_fixture_replay_or_oracle_agreement_cannot_satisfy_eir_mechanism_or_evaluation()
-> Result<(), CompilerProfileContractError> {
    let eir_requirement =
        evidence(&[ProofClass::EirMechanism, ProofClass::EvaluatedWork], &[SourceTier::Source])?;
    let replay_and_oracle = [
        EvidenceObservation { class: ProofClass::CuratedExpectation, tier: SourceTier::Source },
        EvidenceObservation { class: ProofClass::RealPerlOracle, tier: SourceTier::Source },
    ];
    assert!(
        !eir_requirement.is_satisfied_by(&replay_and_oracle),
        "fixture replay and oracle agreement must not satisfy EIR mechanism/evaluation"
    );
    assert!(!eir_requirement.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::EirMechanism,
        tier: SourceTier::Source,
    }]));
    assert!(eir_requirement.is_satisfied_by(&[
        EvidenceObservation { class: ProofClass::EirMechanism, tier: SourceTier::Source },
        EvidenceObservation { class: ProofClass::EvaluatedWork, tier: SourceTier::Source },
    ]));
    Ok(())
}

#[test]
fn f06_source_locked_debt_is_not_general_semantic_support()
-> Result<(), CompilerProfileContractError> {
    let general = evidence(&[ProofClass::GeneralSemanticSupport], &[SourceTier::Source])?;
    assert!(
        !general.is_satisfied_by(&[EvidenceObservation {
            class: ProofClass::AcceptedCompatibilityState,
            tier: SourceTier::Source,
        }]),
        "source-locked debt acceptance must not be typed as general semantic support"
    );
    assert!(
        !general.is_satisfied_by(&[EvidenceObservation {
            class: ProofClass::ObservedUpstreamResult,
            tier: SourceTier::Source,
        }]),
        "an observed upstream result is not general semantic support either"
    );

    let debt = evidence(&[ProofClass::AcceptedCompatibilityState], &[SourceTier::Source])?;
    assert!(
        !debt.is_satisfied_by(&[EvidenceObservation {
            class: ProofClass::GeneralSemanticSupport,
            tier: SourceTier::Source,
        }]),
        "general semantic support must not retire source-locked debt"
    );
    assert!(debt.is_satisfied_by(&[EvidenceObservation {
        class: ProofClass::AcceptedCompatibilityState,
        tier: SourceTier::Source,
    }]));
    Ok(())
}

#[test]
fn f07_source_process_package_install_client_stages_do_not_collapse()
-> Result<(), CompilerProfileContractError> {
    let mut names = Vec::new();
    for tier in SourceTier::ALL {
        names.push(tier.as_str());
    }
    let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), 5, "the five stages must stay distinct typed states");

    let requirement = evidence(&[ProofClass::TestReachability], &[SourceTier::ActualClient])?;
    for tier in [
        SourceTier::Source,
        SourceTier::ExactProcess,
        SourceTier::Packaged,
        SourceTier::InstalledHost,
    ] {
        assert!(
            !requirement.is_satisfied_by(&[EvidenceObservation {
                class: ProofClass::TestReachability,
                tier,
            }]),
            "evidence at `{}` must not satisfy an actual-client requirement",
            tier.as_str()
        );
    }
    Ok(())
}

#[test]
fn f08_unsupported_or_required_rows_cannot_disappear_by_omission()
-> Result<(), CompilerProfileContractError> {
    let (local, static_project, bounded, maintained) = shape_chain()?;

    // Dropping an imported required row breaks closure.
    let mut dropped = static_project.clone();
    dropped.rows.remove(&row_id_of("lexicals.same-file.initialized")?);
    let error =
        dropped.validate_closure(&[&local]).err().ok_or(CompilerProfileContractError::Schema {
            field: "test".to_string(),
            message: "dropping an imported row must fail closure".to_string(),
        })?;
    assert!(
        matches!(error, CompilerProfileContractError::Closure { .. }),
        "unexpected error: {error}"
    );

    // Dropping an imported Unsupported row (a closed typed state) also breaks
    // closure: omission is not a disposition.
    let mut dropped_unsupported = maintained.clone();
    dropped_unsupported.rows.remove(&row_id_of("dynamic.boundaries.unsupported")?);
    assert!(matches!(
        dropped_unsupported.validate_closure(&[&local, &static_project, &bounded]),
        Err(CompilerProfileContractError::Closure { .. })
    ));

    // Closed negative states must carry their reason/ruling: no silent states.
    let mut empty_reason = bounded.clone();
    if let Some(row) = empty_reason.rows.get_mut(&row_id_of("dynamic.boundaries.unsupported")?) {
        row.disposition = RowDisposition::Unsupported { reason: "  ".to_string() };
    }
    assert!(matches!(empty_reason.validate(), Err(CompilerProfileContractError::Schema { .. })));

    let mut empty_ruling = bounded.clone();
    if let Some(row) = empty_ruling.rows.get_mut(&row_id_of("navigation.cross-file.admitted")?) {
        row.disposition = RowDisposition::NotApplicable { ruling: String::new() };
    }
    assert!(matches!(empty_ruling.validate(), Err(CompilerProfileContractError::Schema { .. })));

    let mut empty_condition = static_project.clone();
    if let Some(row) = empty_condition.rows.get_mut(&row_id_of("navigation.cross-file.admitted")?) {
        row.disposition = RowDisposition::Conditional { condition: String::new() };
    }
    assert!(matches!(empty_condition.validate(), Err(CompilerProfileContractError::Schema { .. })));
    Ok(())
}

#[test]
fn f09_zero_work_cannot_satisfy_a_required_work_row() -> Result<(), CompilerProfileContractError> {
    assert!(
        WorkRequirement::new(WorkClass::Production, 0).is_err(),
        "a zero-floor work requirement must not be constructible"
    );
    let requirement = WorkRequirement::new(WorkClass::Production, 5)?;
    assert!(
        !requirement.is_satisfied_by(WorkObservation { class: WorkClass::Production, units: 0 })
    );
    assert!(
        !requirement.is_satisfied_by(WorkObservation { class: WorkClass::Production, units: 4 })
    );
    assert!(
        requirement.is_satisfied_by(WorkObservation { class: WorkClass::Production, units: 5 })
    );
    Ok(())
}

#[test]
fn f10_cold_or_oracle_work_is_not_production_work() -> Result<(), CompilerProfileContractError> {
    let requirement = WorkRequirement::new(WorkClass::Production, 5)?;
    assert!(
        !requirement.is_satisfied_by(WorkObservation { class: WorkClass::OracleCold, units: 100 }),
        "cold/oracle work must not be typed as production work"
    );
    assert!(
        !requirement.is_satisfied_by(WorkObservation { class: WorkClass::Correctness, units: 100 })
    );
    assert!(
        !requirement
            .is_satisfied_by(WorkObservation { class: WorkClass::PerformanceResource, units: 100 })
    );
    assert!(
        requirement.is_satisfied_by(WorkObservation { class: WorkClass::Production, units: 5 })
    );
    Ok(())
}

#[test]
fn f11_imports_bind_exact_lower_identity_and_preserve_rows_limitations_and_ceilings()
-> Result<(), CompilerProfileContractError> {
    let (local, static_project, bounded, maintained) = shape_chain()?;
    let chain = [&local, &static_project, &bounded];

    // Exact lower identity: a stale or forged digest fails closed.
    let mut stale_digest = static_project.clone();
    let mut import = static_project.imports.iter().next().cloned().ok_or(
        CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: "static fixture must import the local profile".to_string(),
        },
    )?;
    import.digest = alternate_digest(7)?;
    stale_digest.imports = [import].into();
    assert!(
        matches!(
            stale_digest.validate_closure(&[&local]),
            Err(CompilerProfileContractError::Identity { .. })
        ),
        "stale import digest must fail with a typed identity error"
    );

    // Unresolvable identities fail closed.
    assert!(matches!(
        static_project.validate_closure(&[]),
        Err(CompilerProfileContractError::Identity { .. })
    ));

    // Preserving rows verbatim: modifying an imported row breaks closure.
    let mut modified_row = static_project.clone();
    if let Some(row) = modified_row.rows.get_mut(&row_id_of("lexicals.same-file.initialized")?) {
        row.statement = "weakened statement".to_string();
    }
    assert!(matches!(
        modified_row.validate_closure(&[&local]),
        Err(CompilerProfileContractError::Closure { .. })
    ));

    // Dropping or editing an imported limitation breaks closure.
    let mut dropped_limitation = static_project.clone();
    dropped_limitation.limitations.remove("no.compiler.world");
    assert!(matches!(
        dropped_limitation.validate_closure(&[&local]),
        Err(CompilerProfileContractError::Closure { .. })
    ));
    let mut edited_limitation = static_project.clone();
    if let Some(limitation) = edited_limitation.limitations.get_mut("no.compiler.world") {
        limitation.boundary = "narrowed boundary".to_string();
    }
    assert!(matches!(
        edited_limitation.validate_closure(&[&local]),
        Err(CompilerProfileContractError::Closure { .. })
    ));

    // Self-imports fail closed.
    let mut self_import = local.clone();
    self_import.imports = [import_of(&local)?].into();
    assert!(matches!(
        self_import.validate_closure(&[&local]),
        Err(CompilerProfileContractError::Identity { .. })
    ));

    // The full chain closes positively.
    maintained.validate_closure(&chain)?;
    Ok(())
}

#[test]
fn f12_row_ordering_does_not_change_the_fingerprint_but_semantic_fields_do()
-> Result<(), CompilerProfileContractError> {
    let (_local, static_project, _bounded, _maintained) = shape_chain()?;
    let baseline = static_project.semantic_fingerprint()?;

    // Rebuild with rows and limitations inserted in reverse order.
    let reversed_rows: BTreeMap<_, _> =
        static_project.rows.iter().rev().map(|(key, value)| (key.clone(), value.clone())).collect();
    let reversed_limitations: BTreeMap<_, _> = static_project
        .limitations
        .iter()
        .rev()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let reordered = CompilerProfileDefinition::new(
        static_project.profile_id.clone(),
        static_project.version.clone(),
        static_project.purpose.clone(),
        static_project.imports.clone(),
        reversed_rows,
        reversed_limitations,
    )?;
    assert_eq!(
        reordered.semantic_fingerprint()?,
        baseline,
        "insertion order must not change semantic identity"
    );
    assert_eq!(
        reordered.to_canonical_json()?,
        static_project.to_canonical_json()?,
        "canonical serialization must be insertion-order independent"
    );

    // Any semantic field change changes the fingerprint.
    type Mutation = (
        &'static str,
        Box<dyn FnOnce(&mut CompilerProfileDefinition) -> Result<(), CompilerProfileContractError>>,
    );
    let mutations: Vec<Mutation> = vec![
        (
            "purpose",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                definition.purpose = "changed purpose".to_string();
                Ok(())
            }),
        ),
        (
            "version",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                definition.version = CompilerProfileVersion::new("v2")?;
                Ok(())
            }),
        ),
        (
            "row.statement",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) = definition.rows.get_mut(&row_id_of("world.snapshot.current")?) {
                    row.statement = "changed statement".to_string();
                }
                Ok(())
            }),
        ),
        (
            "row.disposition",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) = definition.rows.get_mut(&row_id_of("world.snapshot.current")?) {
                    row.disposition = RowDisposition::Optional;
                }
                Ok(())
            }),
        ),
        (
            "row.subject",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) =
                    definition.rows.get_mut(&row_id_of("navigation.cross-file.admitted")?)
                {
                    row.subject = SubjectSelector::SameFileInitializedLexicals;
                }
                Ok(())
            }),
        ),
        (
            "row.evidence",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) = definition.rows.get_mut(&row_id_of("world.snapshot.current")?) {
                    row.evidence =
                        evidence(&[ProofClass::CrossFileExternalBehavior], &[SourceTier::Source])?;
                }
                Ok(())
            }),
        ),
        (
            "row.completeness",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) =
                    definition.rows.get_mut(&row_id_of("lexicals.same-file.initialized")?)
                {
                    row.completeness = CompletenessRequirement {
                        rule: CompletenessRule::RepresentativeSample {
                            sample_id: "sample".to_string(),
                        },
                    };
                }
                Ok(())
            }),
        ),
        (
            "row.work",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) =
                    definition.rows.get_mut(&row_id_of("cold.equivalence.representative")?)
                {
                    row.work = Some(WorkRequirement::new(WorkClass::OracleCold, 4)?);
                }
                Ok(())
            }),
        ),
        (
            "row.limitation_policy",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) =
                    definition.rows.get_mut(&row_id_of("navigation.cross-file.admitted")?)
                {
                    row.limitation_policy = LimitationPolicy::Unbounded;
                }
                Ok(())
            }),
        ),
        (
            "row.legacy_exit",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) =
                    definition.rows.get_mut(&row_id_of("rename.same-file.complete-or-refuse")?)
                    && let Some(exit) = row.legacy_exit.as_mut()
                {
                    exit.legacy_path = "other-legacy-path".to_string();
                }
                Ok(())
            }),
        ),
        (
            "row.owner",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) = definition.rows.get_mut(&row_id_of("world.snapshot.current")?) {
                    row.owner = token_owner("other.owner", WakeEvent::WorldSnapshotMovement)?;
                }
                Ok(())
            }),
        ),
        (
            "row.invalidation",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(row) = definition.rows.get_mut(&row_id_of("world.snapshot.current")?) {
                    row.invalidation.insert(InvalidationInput::RecurrenceObserved);
                }
                Ok(())
            }),
        ),
        (
            "limitation.boundary",
            Box::new(|definition: &mut CompilerProfileDefinition| {
                if let Some(limitation) = definition.limitations.get_mut("no.compiler.world") {
                    limitation.boundary = "changed boundary".to_string();
                }
                Ok(())
            }),
        ),
    ];
    for (name, mutate) in mutations {
        let mut mutated = static_project.clone();
        mutate(&mut mutated)?;
        assert_ne!(
            mutated.semantic_fingerprint()?,
            baseline,
            "changing `{name}` must change the profile identity"
        );
    }
    Ok(())
}

#[test]
fn f13_no_weighted_or_aggregate_score_exists_in_the_model()
-> Result<(), CompilerProfileContractError> {
    let forbidden_tokens = ["score", "readiness", "percent", "weight", "aggregate", "percentage"];
    for shape in all_shapes()? {
        let json = shape.to_canonical_json()?.to_lowercase();
        for token in forbidden_tokens {
            assert!(
                !json.contains(&format!("\"{token}")),
                "the model must not serialize a `{token}` field"
            );
        }
    }

    // A row alone serializes no score fields either.
    let (_local, static_project, _bounded, _maintained) = shape_chain()?;
    let row = static_project.rows.get(&row_id_of("world.snapshot.current")?).cloned().ok_or(
        CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: "missing world.snapshot.current".to_string(),
        },
    )?;
    let row_json =
        serde_json::to_string(&row).map_err(|error| CompilerProfileContractError::Schema {
            field: "row".to_string(),
            message: error.to_string(),
        })?;
    let row_json = row_json.to_lowercase();
    for token in forbidden_tokens {
        assert!(!row_json.contains(&format!("\"{token}")), "rows must not carry `{token}` fields");
    }
    Ok(())
}

#[test]
fn f14_missing_or_malformed_row_fields_fail_closed_deserialization()
-> Result<(), CompilerProfileContractError> {
    let (_local, static_project, _bounded, _maintained) = shape_chain()?;
    let canonical = static_project.to_canonical_json()?;

    let remove_row_field = |field: &str| -> Result<String, CompilerProfileContractError> {
        let mut value: serde_json::Value = serde_json::from_str(&canonical).map_err(|error| {
            CompilerProfileContractError::Schema {
                field: "fixture".to_string(),
                message: error.to_string(),
            }
        })?;
        let rows = value.get_mut("rows").and_then(|rows| rows.as_object_mut()).ok_or(
            CompilerProfileContractError::Schema {
                field: "fixture".to_string(),
                message: "missing rows".to_string(),
            },
        )?;
        let first_row = rows.values_mut().next().ok_or(CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: "empty rows".to_string(),
        })?;
        let object = first_row.as_object_mut().ok_or(CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: "row is not an object".to_string(),
        })?;
        object.remove(field);
        serde_json::to_string(&value).map_err(|error| CompilerProfileContractError::Schema {
            field: "fixture".to_string(),
            message: error.to_string(),
        })
    };

    for field in
        ["claim_ceiling", "owner", "invalidation", "evidence", "completeness", "limitation_policy"]
    {
        let payload = remove_row_field(field)?;
        assert!(
            CompilerProfileDefinition::from_json_str(&payload).is_err(),
            "a row without `{field}` must fail closed"
        );
    }

    // Unknown fields are refused on structs.
    let with_unknown =
        canonical.replacen("\"purpose\"", "\"surprise_field\": 1,\n    \"purpose\"", 1);
    assert!(
        CompilerProfileDefinition::from_json_str(&with_unknown).is_err(),
        "unknown fields must be refused"
    );

    // Malformed disposition payloads are refused.
    let malformed = canonical.replacen(
        "\"disposition\": \"required\"",
        "\"disposition\": {\"disposition\": \"conditional\"}",
        1,
    );
    assert!(
        CompilerProfileDefinition::from_json_str(&malformed).is_err(),
        "a conditional without its condition must fail closed"
    );

    // Row ids must be stable tokens and match their map key.
    let bad_token = canonical.replacen("observed.upstream.selected", "Observed.Upstream", 1);
    assert!(
        CompilerProfileDefinition::from_json_str(&bad_token).is_err(),
        "unstable row tokens must fail closed"
    );
    Ok(())
}

#[test]
fn f15_support_release_authority_is_not_inferrable_from_a_profile_result()
-> Result<(), CompilerProfileContractError> {
    for family in [
        ClaimFamily::SupportAuthorization,
        ClaimFamily::ReleaseAuthorization,
        ClaimFamily::PublicationAuthorization,
    ] {
        let mut authorized = local_lexical_shape()?;
        if let Some(row) = authorized.rows.get_mut(&row_id_of("observed.upstream.selected")?) {
            row.claim_ceiling = ClaimCeiling::new(family);
        }
        assert!(
            matches!(
                authorized.validate(),
                Err(CompilerProfileContractError::Authorization { .. })
            ),
            "profile rows must not carry {family:?} ceilings"
        );
    }

    // Every fixture row's ceiling denies authorization families.
    for shape in all_shapes()? {
        for row in shape.rows.values() {
            assert!(row.claim_ceiling.permits(ClaimFamily::ProfileEvidence));
            assert!(!row.claim_ceiling.permits(ClaimFamily::SupportAuthorization));
            assert!(!row.claim_ceiling.permits(ClaimFamily::ReleaseAuthorization));
            assert!(!row.claim_ceiling.permits(ClaimFamily::PublicationAuthorization));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Exhaustive typed dispositions
// ---------------------------------------------------------------------------

#[test]
fn required_conditional_optional_unsupported_not_applicable_are_exhaustive()
-> Result<(), CompilerProfileContractError> {
    let mut expected = vec!["required", "conditional", "optional", "unsupported", "not_applicable"];
    expected.sort_unstable();
    let mut names = Vec::new();
    for disposition in RowDisposition::ALL {
        names.push(disposition.as_str());
    }
    names.sort_unstable();
    assert_eq!(
        names, expected,
        "the disposition vocabulary must be exactly the five closed states"
    );

    // Deserializing each state round-trips.
    for disposition in RowDisposition::ALL {
        let json = serde_json::to_string(&disposition).map_err(|error| {
            CompilerProfileContractError::Schema {
                field: "disposition".to_string(),
                message: error.to_string(),
            }
        })?;
        let parsed: RowDisposition =
            serde_json::from_str(&json).map_err(|error| CompilerProfileContractError::Schema {
                field: "disposition".to_string(),
                message: error.to_string(),
            })?;
        assert_eq!(parsed, disposition);
    }
    Ok(())
}
