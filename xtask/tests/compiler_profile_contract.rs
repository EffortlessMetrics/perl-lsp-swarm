//! Falsifier-first contract tests for the maintained compiler profile domain
//! model (#12186).
//!
//! Falsifier ID map (F01-F15 are the issue's falsifiers verbatim in spirit;
//! F16-F19 are companion proofs required by the claim brief):
//!
//! - `f01_` a local lexical pass cannot stand in for a stronger profile
//! - `f02_` long-horizon compiler work is not a prerequisite of the bounded
//!   local profile
//! - `f03_` issue/PR/workflow state cannot enter the evidence model
//! - `f04_` parser proof cannot satisfy provider/edit/installed-host proof
//! - `f05_` fixture replay or oracle agreement cannot satisfy EIR mechanism
//!   evaluation
//! - `f06_` source-locked debt cannot be typed as general semantic support
//! - `f07_` source/exact-process/package/install/client stages do not collapse
//! - `f08_` an unsupported/not-proven required row cannot disappear by omission
//! - `f09_` zero-work execution cannot satisfy a required work row
//! - `f10_` cold/oracle work cannot be typed as production work avoided
//! - `f11_` an imported lower profile losing rows or limitations fails
//! - `f12_` row ordering changes nothing about the profile fingerprint
//! - `f13_` no scalar score or aggregate percentage exists in the API
//! - `f14_` ceiling/legacy-exit/owner/invalidation fields are mandatory
//! - `f15_` support/release authority cannot be inferred from a profile result
//! - `f16_` import preservation happy path binds exact identity and digest
//! - `f17_` every semantic field kind moves the fingerprint when changed
//! - `f18_` the four #12176 shape fixtures validate with stable digests
//! - `f19_` closed dispositions survive validation without becoming required
//!
//! House rule for this suite: no `unwrap`/`expect`/`panic!` anywhere, tests
//! return `Result<(), String>`, and failures carry appended context.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use xtask::compiler_profile::AllowedLimitation;
use xtask::compiler_profile::AxisProofSpec;
use xtask::compiler_profile::ClaimCeiling;
use xtask::compiler_profile::ClaimFamily;
use xtask::compiler_profile::CollaborationSurface;
use xtask::compiler_profile::CompatibilityAcceptance;
use xtask::compiler_profile::CompilerProfileDefinition;
use xtask::compiler_profile::CompilerProfileError;
use xtask::compiler_profile::CompilerProfileId;
use xtask::compiler_profile::CompilerProfileImport;
use xtask::compiler_profile::CompilerProfileRow;
use xtask::compiler_profile::CompilerProfileRowId;
use xtask::compiler_profile::CompilerProfileVersion;
use xtask::compiler_profile::CompletenessRequirement;
use xtask::compiler_profile::CompletenessRule;
use xtask::compiler_profile::ConditionalActivation;
use xtask::compiler_profile::CurrentnessRule;
use xtask::compiler_profile::EvidenceClass;
use xtask::compiler_profile::EvidenceRecord;
use xtask::compiler_profile::ExecutionStage;
use xtask::compiler_profile::ExternalProvenance;
use xtask::compiler_profile::InvalidationInput;
use xtask::compiler_profile::LegacyExitDimension;
use xtask::compiler_profile::LegacyExitRequirement;
use xtask::compiler_profile::OwnerAndWakeEvent;
use xtask::compiler_profile::ProfileContentDigest;
use xtask::compiler_profile::ProfileRegistry;
use xtask::compiler_profile::ProofAxis;
use xtask::compiler_profile::RowDisposition;
use xtask::compiler_profile::SemanticSupportLevel;
use xtask::compiler_profile::SourceTier;
use xtask::compiler_profile::SubjectArea;
use xtask::compiler_profile::SubjectSelector;
use xtask::compiler_profile::SupportClaim;
use xtask::compiler_profile::UpstreamObservation;
use xtask::compiler_profile::WakeEvent;
use xtask::compiler_profile::WorkContext;
use xtask::compiler_profile::WorkPerformed;
use xtask::compiler_profile::WorkRequirement;

// ---------------------------------------------------------------------------
// Result plumbing (no unwrap/expect/panic in this suite)
// ---------------------------------------------------------------------------

/// Converts any displayable error into `String`; callers append context.
fn ck<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|error| error.to_string())
}

/// Fails the test with `message` unless `condition` holds.
fn ensure(condition: bool, message: &str) -> Result<(), String> {
    if condition { Ok(()) } else { Err(message.to_string()) }
}

/// Extracts the error from a definition constructor, or fails the test.
fn expect_definition_err(
    result: Result<CompilerProfileDefinition, CompilerProfileError>,
) -> Result<CompilerProfileError, String> {
    match result {
        Ok(_) => Err("expected construction to fail, but it succeeded".to_string()),
        Err(error) => Ok(error),
    }
}

/// Extracts the error from a validation call, or fails the test.
fn expect_validation_err(
    result: Result<(), CompilerProfileError>,
) -> Result<CompilerProfileError, String> {
    match result {
        Ok(()) => Err("expected validation to fail, but it succeeded".to_string()),
        Err(error) => Ok(error),
    }
}

/// The variant name of a classified profile error, for precise assertions.
fn variant_of(error: &CompilerProfileError) -> &'static str {
    match error {
        CompilerProfileError::Identity { .. } => "Identity",
        CompilerProfileError::Structure { .. } => "Structure",
        CompilerProfileError::MissingRequiredEvidence { .. } => "MissingRequiredEvidence",
        CompilerProfileError::CrossSatisfaction { .. } => "CrossSatisfaction",
        CompilerProfileError::StageUnderflow { .. } => "StageUnderflow",
        CompilerProfileError::EvidenceTierBelowFloor { .. } => "EvidenceTierBelowFloor",
        CompilerProfileError::WorkMismatch { .. } => "WorkMismatch",
        CompilerProfileError::SupportOverstatement { .. } => "SupportOverstatement",
        CompilerProfileError::DispositionConflict { .. } => "DispositionConflict",
        CompilerProfileError::RejectedProvenance { .. } => "RejectedProvenance",
        CompilerProfileError::ImportResolution { .. } => "ImportResolution",
        CompilerProfileError::ImportPreservation { .. } => "ImportPreservation",
    }
}

// ---------------------------------------------------------------------------
// Shared builders
// ---------------------------------------------------------------------------

fn pid(value: &str) -> Result<CompilerProfileId, String> {
    ck(CompilerProfileId::new(value))
}

fn pver(value: &str) -> Result<CompilerProfileVersion, String> {
    ck(CompilerProfileVersion::new(value))
}

fn rid(value: &str) -> Result<CompilerProfileRowId, String> {
    ck(CompilerProfileRowId::new(value))
}

fn subject(area: SubjectArea, selector: &str) -> Result<SubjectSelector, String> {
    ck(SubjectSelector::new(area, selector))
}

fn owner() -> Result<OwnerAndWakeEvent, String> {
    ck(OwnerAndWakeEvent::new("#12186", WakeEvent::NextUpstreamRelease))
}

fn completeness() -> CompletenessRequirement {
    CompletenessRequirement {
        currentness: CurrentnessRule::FreshAtValidationTime,
        completeness: CompletenessRule::ExhaustiveAcrossSubjectSelector,
    }
}

fn facts_claim() -> Result<SupportClaim, String> {
    ck(SupportClaim::new(
        UpstreamObservation::ImportedCleanly,
        CompatibilityAcceptance::AcceptedUnchanged,
        SemanticSupportLevel::InternalFactsOnly,
    ))
}

fn gss_claim() -> Result<SupportClaim, String> {
    ck(SupportClaim::new(
        UpstreamObservation::ImportedCleanly,
        CompatibilityAcceptance::AcceptedUnchanged,
        SemanticSupportLevel::GeneralSemanticSupport,
    ))
}

fn unsupported_claim() -> Result<SupportClaim, String> {
    ck(SupportClaim::new(
        UpstreamObservation::Unobserved,
        CompatibilityAcceptance::NotYetAccepted,
        SemanticSupportLevel::Unsupported,
    ))
}

fn spec_for(
    axis_to_spec: ProofAxis,
    classes: &[EvidenceClass],
    min_tier: SourceTier,
    min_stage: ExecutionStage,
    work: WorkRequirement,
) -> Result<AxisProofSpec, String> {
    let acceptable = classes.iter().copied().collect::<BTreeSet<_>>();
    ck(AxisProofSpec::for_axis(axis_to_spec, acceptable, min_tier, min_stage, work))
}

#[allow(clippy::too_many_arguments)]
fn make_row(
    row_id: &str,
    area: SubjectArea,
    selector: &str,
    statement: &str,
    disposition: RowDisposition,
    support: SupportClaim,
    axes: BTreeMap<ProofAxis, AxisProofSpec>,
    limitations: &[AllowedLimitation],
    legacy_exit: LegacyExitRequirement,
    invalidation: InvalidationInput,
    ceiling: ClaimCeiling,
) -> Result<CompilerProfileRow, String> {
    CompilerProfileRow::new(
        rid(row_id)?,
        subject(area, selector)?,
        statement,
        disposition,
        support,
        axes,
        completeness(),
        limitations.iter().cloned().collect::<BTreeSet<_>>(),
        legacy_exit,
        invalidation,
        ceiling,
        owner()?,
    )
    .map_err(|error| error.to_string())
}

fn limitation(id: &str, description: &str) -> Result<AllowedLimitation, String> {
    ck(AllowedLimitation::new(id, description))
}

fn ev(
    record_id: &str,
    row_id: &str,
    record_axis: ProofAxis,
    class: EvidenceClass,
    tier: SourceTier,
    stage: ExecutionStage,
    work: WorkPerformed,
) -> Result<EvidenceRecord, String> {
    ck(EvidenceRecord::from_domain_artifact(
        record_id,
        rid(row_id)?,
        record_axis,
        class,
        tier,
        stage,
        work,
        "in-memory://fixture",
    ))
}

fn ax(family: ClaimFamily, stage: ExecutionStage) -> ProofAxis {
    ProofAxis::new(family, stage)
}

/// One-required-row profile scaffold whose axis spec, support claim, and
/// evidence are chosen by the caller. Construction may legitimately fail;
/// the nested result lets tests target whichever layer must refuse.
struct AttackScaffold {
    family: ClaimFamily,
    stage: ExecutionStage,
    spec_classes: Vec<EvidenceClass>,
    spec_min_tier: SourceTier,
    spec_min_stage: ExecutionStage,
    spec_work: WorkRequirement,
    support: SupportClaim,
    evidence_class: EvidenceClass,
    evidence_tier: SourceTier,
    evidence_stage: ExecutionStage,
    evidence_work: WorkPerformed,
    include_evidence: bool,
    include_row: bool,
}

impl AttackScaffold {
    fn build(&self) -> Result<Result<CompilerProfileDefinition, CompilerProfileError>, String> {
        let attack_axis = ax(self.family, self.stage);
        let mut records: BTreeSet<EvidenceRecord> = BTreeSet::new();
        let mut rows = BTreeMap::new();
        if self.include_row {
            let spec = spec_for(
                attack_axis,
                &self.spec_classes,
                self.spec_min_tier,
                self.spec_min_stage,
                self.spec_work,
            )?;
            rows.insert(
                rid("attack-row")?,
                make_row(
                    "attack-row",
                    SubjectArea::RuntimeBoundary,
                    "attack_subject",
                    "Attack scaffold proposition.",
                    RowDisposition::Required,
                    self.support.clone(),
                    BTreeMap::from([(attack_axis, spec)]),
                    &[],
                    LegacyExitRequirement::none(),
                    InvalidationInput::NoneDeclared,
                    ClaimCeiling::RepositoryInternalOnly,
                )?,
            );
        }
        if self.include_evidence {
            records.insert(ev(
                "ev-attack",
                "attack-row",
                attack_axis,
                self.evidence_class,
                self.evidence_tier,
                self.evidence_stage,
                self.evidence_work,
            )?);
        }
        Ok(CompilerProfileDefinition::new(
            pid("compiler_attack_scaffold")?,
            pver("v1")?,
            "attack scaffold",
            BTreeSet::new(),
            rows,
            records,
            BTreeSet::new(),
        ))
    }
}

impl Default for AttackScaffold {
    fn default() -> Self {
        Self {
            family: ClaimFamily::CompilerInternalFacts,
            stage: ExecutionStage::SourceTree,
            spec_classes: vec![EvidenceClass::ParserFactProduction],
            spec_min_tier: SourceTier::Vendored,
            spec_min_stage: ExecutionStage::SourceTree,
            spec_work: WorkRequirement::none(),
            support: facts_claim().unwrap_or_else(|_error| SupportClaim {
                observed_upstream: UpstreamObservation::Unobserved,
                accepted_compatibility: CompatibilityAcceptance::NotYetAccepted,
                semantic_support: SemanticSupportLevel::Unsupported,
            }),
            evidence_class: EvidenceClass::ParserFactProduction,
            evidence_tier: SourceTier::RepositoryOwned,
            evidence_stage: ExecutionStage::SourceTree,
            evidence_work: WorkPerformed::zero_execution(),
            include_evidence: true,
            include_row: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Shape fixtures (#12176 profile classes; representability only)
// ---------------------------------------------------------------------------

/// Reverses insertion order of any sized iterator when `reverse` is set.
trait RevIf: IntoIterator {
    fn rev_if(self, reverse: bool) -> std::vec::IntoIter<Self::Item>;
}

impl<T: IntoIterator> RevIf for T {
    fn rev_if(self, reverse: bool) -> std::vec::IntoIter<Self::Item> {
        let collected: Vec<Self::Item> = self.into_iter().collect();
        if reverse {
            collected.into_iter().rev().collect::<Vec<_>>().into_iter()
        } else {
            collected.into_iter()
        }
    }
}

fn single_row_map(row: CompilerProfileRow) -> BTreeMap<CompilerProfileRowId, CompilerProfileRow> {
    let mut map = BTreeMap::new();
    map.insert(row.row_id.clone(), row);
    map
}

fn single_evidence_set(record: EvidenceRecord) -> BTreeSet<EvidenceRecord> {
    let mut set = BTreeSet::new();
    set.insert(record);
    set
}

/// `compiler_local_lexical.v1`: lexical parity plus a typed unsupported row.
fn local_lexical(reverse: bool) -> Result<CompilerProfileDefinition, String> {
    let lex_axis = ax(ClaimFamily::CompilerInternalFacts, ExecutionStage::SourceTree);
    let lex_spec = spec_for(
        lex_axis,
        &[EvidenceClass::ParserFactProduction, EvidenceClass::LocalLexicalPass],
        SourceTier::Vendored,
        ExecutionStage::SourceTree,
        WorkRequirement::none(),
    )?;
    let lex_row = make_row(
        "lex-tokenization-parity",
        SubjectArea::SyntaxConstruct,
        "tokenize_perl_source",
        "Lexical tokenization matches expected token streams for local files.",
        RowDisposition::Required,
        facts_claim()?,
        BTreeMap::from([(lex_axis, lex_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::ToolchainChange,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let rename_row = make_row(
        "lex-semantic-rename",
        SubjectArea::ProviderAction,
        "rename_symbol",
        "Semantic rename is out of scope for the lexical profile.",
        ck(RowDisposition::unsupported("rename needs cross-procedure semantic facts"))?,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let lex_record = ev(
        "ev-lex-1",
        "lex-tokenization-parity",
        lex_axis,
        EvidenceClass::ParserFactProduction,
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkPerformed::zero_execution(),
    )?;
    let mut rows = BTreeMap::new();
    for row in [lex_row, rename_row].into_iter().rev_if(reverse) {
        rows.insert(row.row_id.clone(), row);
    }
    let mut evidence = BTreeSet::new();
    for record in [lex_record].into_iter().rev_if(reverse) {
        evidence.insert(record);
    }
    CompilerProfileDefinition::new(
        pid("compiler_local_lexical")?,
        pver("v1")?,
        "local lexical editing claims only",
        BTreeSet::new(),
        rows,
        evidence,
        BTreeSet::new(),
    )
    .map_err(|error| error.to_string())
}

/// `compiler_static_project.v1`: project/world shape carrying every closed
/// disposition state.
fn static_project(reverse: bool) -> Result<CompilerProfileDefinition, String> {
    let parser_axis = ax(ClaimFamily::CompilerInternalFacts, ExecutionStage::SourceTree);
    let world_axis = ax(ClaimFamily::ProjectWorldCurrentness, ExecutionStage::ExactProcess);
    let cross_axis = ax(ClaimFamily::CrossFileExternalBehavior, ExecutionStage::ExactProcess);

    let parser_spec = spec_for(
        parser_axis,
        &[EvidenceClass::ParserFactProduction],
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkRequirement::none(),
    )?;
    let world_spec = spec_for(
        world_axis,
        &[EvidenceClass::WorkspaceWorldObservation],
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkRequirement::at_least(WorkContext::ProductionPath, 1),
    )?;
    let cross_spec = spec_for(
        cross_axis,
        &[EvidenceClass::CrossFileBehaviorObservation],
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkRequirement::none(),
    )?;

    let parser_row = make_row(
        "sp-parser-facts",
        SubjectArea::SyntaxConstruct,
        "parse_project_files",
        "Project files parse into complete syntax trees.",
        RowDisposition::Required,
        facts_claim()?,
        BTreeMap::from([(parser_axis, parser_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::ToolchainChange,
        ClaimCeiling::ContributorDocumentation,
    )?;
    let world_row = make_row(
        "sp-world-currentness",
        SubjectArea::WorkspaceScenario,
        "index_matches_disk",
        "The workspace index reflects current disk state.",
        RowDisposition::Required,
        ck(SupportClaim::new(
            UpstreamObservation::AppliedWithPatches,
            CompatibilityAcceptance::AcceptedWithDocumentedDeviations,
            SemanticSupportLevel::FactsConsumedByProviders,
        ))?,
        BTreeMap::from([(world_axis, world_spec)]),
        &[limitation("sp-limit-index-lag", "Index refresh may lag rapid saves.")?],
        LegacyExitRequirement::none(),
        InvalidationInput::WorkspaceConfigurationChange,
        ClaimCeiling::ContributorDocumentation,
    )?;
    let conditional_row = make_row(
        "sp-cross-file-nav",
        SubjectArea::ProviderAction,
        "cross_file_navigation",
        "Cross-file navigation works while the feature flag is enabled.",
        RowDisposition::Conditional(ConditionalActivation::WhenWorkspaceFeatureEnabled(
            "cross-file-nav".to_string(),
        )),
        facts_claim()?,
        BTreeMap::from([(cross_axis, cross_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::WorkspaceConfigurationChange,
        ClaimCeiling::ContributorDocumentation,
    )?;
    let optional_row = make_row(
        "sp-provider-rename",
        SubjectArea::ProviderAction,
        "rename_symbol",
        "Rename is offered but not obligated in this profile.",
        RowDisposition::Optional,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let na_row = make_row(
        "sp-packaging-na",
        SubjectArea::PackagingUnit,
        "extension_bundle",
        "Packaging does not apply to this source-side profile.",
        ck(RowDisposition::not_applicable("profile covers source-tree behavior only"))?,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;

    let parser_record = ev(
        "ev-sp-parser",
        "sp-parser-facts",
        parser_axis,
        EvidenceClass::ParserFactProduction,
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkPerformed::zero_execution(),
    )?;
    let world_record = ev(
        "ev-sp-world",
        "sp-world-currentness",
        world_axis,
        EvidenceClass::WorkspaceWorldObservation,
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkPerformed::new(WorkContext::ProductionPath, 2),
    )?;
    let cross_record = ev(
        "ev-sp-cross",
        "sp-cross-file-nav",
        cross_axis,
        EvidenceClass::CrossFileBehaviorObservation,
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkPerformed::zero_execution(),
    )?;

    let ordered_rows = [parser_row, world_row, conditional_row, optional_row, na_row];
    let ordered_evidence = [parser_record, world_record, cross_record];
    let mut rows = BTreeMap::new();
    for row in ordered_rows.into_iter().rev_if(reverse) {
        rows.insert(row.row_id.clone(), row);
    }
    let mut evidence = BTreeSet::new();
    for record in ordered_evidence.into_iter().rev_if(reverse) {
        evidence.insert(record);
    }
    let limitations =
        BTreeSet::from([limitation("sp-limit-index-lag", "Index refresh may lag rapid saves.")?]);
    CompilerProfileDefinition::new(
        pid("compiler_static_project")?,
        pver("v1")?,
        "static project-wide claims inside one workspace",
        BTreeSet::new(),
        rows,
        evidence,
        limitations,
    )
    .map_err(|error| error.to_string())
}

/// `compiler_bounded_execution.v1`: bounded execution plus EIR mechanism.
fn bounded_execution(reverse: bool) -> Result<CompilerProfileDefinition, String> {
    let bounded_axis = ax(ClaimFamily::ExecutionBoundedness, ExecutionStage::ExactProcess);
    let eir_axis = ax(ClaimFamily::EirMechanism, ExecutionStage::ExactProcess);
    let bounded_spec = spec_for(
        bounded_axis,
        &[EvidenceClass::CuratedFixtureReplay, EvidenceClass::RealPerlOracleAgreement],
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkRequirement::none(),
    )?;
    let eir_spec = spec_for(
        eir_axis,
        &[EvidenceClass::EirMechanismEvaluation],
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkRequirement::at_least(WorkContext::ProductionPath, 2),
    )?;
    let bounded_row = make_row(
        "be-bounded-execution",
        SubjectArea::RuntimeBoundary,
        "analysis_time_bounded",
        "Analysis time stays bounded on curated workloads.",
        RowDisposition::Required,
        ck(SupportClaim::new(
            UpstreamObservation::AppliedWithPatches,
            CompatibilityAcceptance::AcceptedWithDocumentedDeviations,
            SemanticSupportLevel::FactsConsumedByProviders,
        ))?,
        BTreeMap::from([(bounded_axis, bounded_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::ToolchainChange,
        ClaimCeiling::ContributorDocumentation,
    )?;
    let eir_row = make_row(
        "be-eir-mechanism",
        SubjectArea::RuntimeBoundary,
        "eir_mechanism_evaluated",
        "The EIR evaluation mechanism is exercised on evaluated work.",
        RowDisposition::Required,
        facts_claim()?,
        BTreeMap::from([(eir_axis, eir_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::ExplicitReviewRequest,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let unbounded_row = make_row(
        "be-unbounded-host-features",
        SubjectArea::RuntimeBoundary,
        "host_unbounded_features",
        "Unbounded host features are not covered by this profile.",
        ck(RowDisposition::unsupported("no bound can be proven for host-driven features"))?,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let bounded_record = ev(
        "ev-be-bounded",
        "be-bounded-execution",
        bounded_axis,
        EvidenceClass::CuratedFixtureReplay,
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkPerformed::zero_execution(),
    )?;
    let eir_record = ev(
        "ev-be-eir",
        "be-eir-mechanism",
        eir_axis,
        EvidenceClass::EirMechanismEvaluation,
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkPerformed::new(WorkContext::ProductionPath, 3),
    )?;
    let ordered_rows = [bounded_row, eir_row, unbounded_row];
    let ordered_evidence = [eir_record, bounded_record];
    let mut rows = BTreeMap::new();
    for row in ordered_rows.into_iter().rev_if(reverse) {
        rows.insert(row.row_id.clone(), row);
    }
    let mut evidence = BTreeSet::new();
    for record in ordered_evidence.into_iter().rev_if(reverse) {
        evidence.insert(record);
    }
    CompilerProfileDefinition::new(
        pid("compiler_bounded_execution")?,
        pver("v1")?,
        "bounded execution claims over evaluated workloads",
        BTreeSet::new(),
        rows,
        evidence,
        BTreeSet::new(),
    )
    .map_err(|error| error.to_string())
}

/// `compiler_maintained_code_intelligence.v1`: exact imports of both lower
/// profiles plus installed-host/client/general-support obligations.
fn maintained_code_intelligence(reverse: bool) -> Result<CompilerProfileDefinition, String> {
    let lower_project = static_project(false)?;
    let lower_bounded = bounded_execution(false)?;
    let import_project = ck(CompilerProfileImport::new(
        lower_project.profile_id.clone(),
        lower_project.version.clone(),
        ProfileContentDigest::from_fingerprint(lower_project.semantic_fingerprint()),
    ))?;
    let import_bounded = ck(CompilerProfileImport::new(
        lower_bounded.profile_id.clone(),
        lower_bounded.version.clone(),
        ProfileContentDigest::from_fingerprint(lower_bounded.semantic_fingerprint()),
    ))?;

    let edit_axis = ax(ClaimFamily::EditAuthorization, ExecutionStage::InstalledHost);
    let client_axis = ax(ClaimFamily::ProviderConsumption, ExecutionStage::ActualClient);
    let gss_edit_axis = ax(ClaimFamily::EditAuthorization, ExecutionStage::PackagedArtifact);
    let gss_provider_axis = ax(ClaimFamily::ProviderConsumption, ExecutionStage::PackagedArtifact);

    let edit_spec = spec_for(
        edit_axis,
        &[EvidenceClass::EditAuthorizationProbe, EvidenceClass::InstalledHostVerification],
        SourceTier::RepositoryOwned,
        ExecutionStage::InstalledHost,
        WorkRequirement::at_least(WorkContext::ProductionPath, 1),
    )?;
    let client_spec = spec_for(
        client_axis,
        &[EvidenceClass::InstalledHostVerification],
        SourceTier::RepositoryOwned,
        ExecutionStage::ActualClient,
        WorkRequirement::at_least(WorkContext::ProductionPath, 1),
    )?;
    let gss_edit_spec = spec_for(
        gss_edit_axis,
        &[EvidenceClass::EditAuthorizationProbe],
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkRequirement::none(),
    )?;
    let gss_provider_spec = spec_for(
        gss_provider_axis,
        &[EvidenceClass::ProviderBehaviorProbe],
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkRequirement::none(),
    )?;

    let consumed_claim = ck(SupportClaim::new(
        UpstreamObservation::ImportedCleanly,
        CompatibilityAcceptance::AcceptedUnchanged,
        SemanticSupportLevel::FactsConsumedByProviders,
    ))?;
    let edit_row = make_row(
        "mci-edit-auth",
        SubjectArea::ProviderAction,
        "edit_authorization_installed_host",
        "Edit authorization behaves correctly on installed hosts.",
        RowDisposition::Required,
        consumed_claim.clone(),
        BTreeMap::from([(edit_axis, edit_spec)]),
        &[],
        LegacyExitRequirement::required(BTreeSet::from([
            LegacyExitDimension::ReplacementCurrentness,
        ]))
        .map_err(|error| error.to_string())?,
        InvalidationInput::ProductReleaseCut,
        ClaimCeiling::DocumentedProductBehavior,
    )?;
    let client_row = make_row(
        "mci-client-provider",
        SubjectArea::ProviderAction,
        "provider_behavior_actual_client",
        "Provider responses hold up under an actual client session.",
        RowDisposition::Required,
        consumed_claim,
        BTreeMap::from([(client_axis, client_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::ProductReleaseCut,
        ClaimCeiling::DocumentedProductBehavior,
    )?;
    let gss_row = make_row(
        "mci-general-support",
        SubjectArea::DocumentationSurface,
        "documented_semantic_support",
        "General semantic support is documented for consumers and edits.",
        RowDisposition::Required,
        gss_claim()?,
        BTreeMap::from([(gss_edit_axis, gss_edit_spec), (gss_provider_axis, gss_provider_spec)]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::UpstreamRelease,
        ClaimCeiling::DocumentedProductBehavior,
    )?;

    let mut rows: BTreeMap<CompilerProfileRowId, CompilerProfileRow> = BTreeMap::new();
    let lower_rows: Vec<&CompilerProfileRow> =
        lower_project.rows.values().chain(lower_bounded.rows.values()).collect();
    for row in lower_rows.into_iter().rev_if(reverse) {
        rows.insert(row.row_id.clone(), row.clone());
    }
    for row in [edit_row, client_row, gss_row].into_iter().rev_if(reverse) {
        rows.insert(row.row_id.clone(), row);
    }

    let mut evidence: BTreeSet<EvidenceRecord> = BTreeSet::new();
    let lower_evidence: Vec<&EvidenceRecord> =
        lower_project.evidence.iter().chain(lower_bounded.evidence.iter()).collect();
    for record in lower_evidence.into_iter().rev_if(reverse) {
        evidence.insert(record.clone());
    }
    let own_records = [
        ev(
            "ev-mci-edit",
            "mci-edit-auth",
            edit_axis,
            EvidenceClass::EditAuthorizationProbe,
            SourceTier::RepositoryOwned,
            ExecutionStage::InstalledHost,
            WorkPerformed::new(WorkContext::ProductionPath, 2),
        )?,
        ev(
            "ev-mci-client",
            "mci-client-provider",
            client_axis,
            EvidenceClass::InstalledHostVerification,
            SourceTier::RepositoryOwned,
            ExecutionStage::ActualClient,
            WorkPerformed::new(WorkContext::ProductionPath, 1),
        )?,
        ev(
            "ev-mci-gss-edit",
            "mci-general-support",
            gss_edit_axis,
            EvidenceClass::EditAuthorizationProbe,
            SourceTier::RepositoryOwned,
            ExecutionStage::PackagedArtifact,
            WorkPerformed::zero_execution(),
        )?,
        ev(
            "ev-mci-gss-provider",
            "mci-general-support",
            gss_provider_axis,
            EvidenceClass::ProviderBehaviorProbe,
            SourceTier::RepositoryOwned,
            ExecutionStage::PackagedArtifact,
            WorkPerformed::zero_execution(),
        )?,
    ];
    for record in own_records.into_iter().rev_if(reverse) {
        evidence.insert(record);
    }

    let mut limitations = lower_project.limitations.clone();
    limitations.insert(limitation(
        "mci-limit-no-perf-claims",
        "This profile licenses no performance or resource claims.",
    )?);

    CompilerProfileDefinition::new(
        pid("compiler_maintained_code_intelligence")?,
        pver("v1")?,
        "maintained end-to-end code intelligence claims",
        BTreeSet::from([import_project, import_bounded]),
        rows,
        evidence,
        limitations,
    )
    .map_err(|error| error.to_string())
}

/// Registry resolving both lower profiles for closure checks.
fn registry_with<'a>(entries: [&'a CompilerProfileDefinition; 2]) -> ProfileRegistry<'a> {
    let mut registry = ProfileRegistry::new();
    for entry in entries {
        registry.insert((entry.profile_id.clone(), entry.version.clone()), entry);
    }
    registry
}

/// An importer preserving every row, evidence record, and limitation of
/// `base` verbatim while binding its exact content digest.
fn preserved_importer(
    base: &CompilerProfileDefinition,
) -> Result<CompilerProfileDefinition, String> {
    let import = ck(CompilerProfileImport::new(
        base.profile_id.clone(),
        base.version.clone(),
        ProfileContentDigest::from_fingerprint(base.semantic_fingerprint()),
    ))?;
    let extra_row = make_row(
        "importer-extra-note",
        SubjectArea::DocumentationSurface,
        "importer_note",
        "Importer-local note carried alongside imported rows.",
        RowDisposition::Optional,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let mut rows = base.rows.clone();
    rows.insert(extra_row.row_id.clone(), extra_row);
    CompilerProfileDefinition::new(
        pid("compiler_importer_check")?,
        pver("v1")?,
        "preservation check importer",
        BTreeSet::from([import]),
        rows,
        base.evidence.clone(),
        base.limitations.clone(),
    )
    .map_err(|error| error.to_string())
}

fn sole_import_of(profile: &CompilerProfileDefinition) -> Result<CompilerProfileImport, String> {
    profile.imports.iter().next().cloned().ok_or_else(|| "expected exactly one import".to_string())
}

// ---------------------------------------------------------------------------
// Falsifier tests
// ---------------------------------------------------------------------------

#[test]
fn compiler_profile_contract_f01_local_lexical_pass_cannot_stand_in_for_stronger_profile()
-> Result<(), String> {
    // Constructor layer: a lexical-pass accept list is unrepresentable for an
    // edit-authorization axis.
    let rejected = AxisProofSpec::for_axis(
        ax(ClaimFamily::EditAuthorization, ExecutionStage::InstalledHost),
        BTreeSet::from([EvidenceClass::LocalLexicalPass]),
        SourceTier::RepositoryOwned,
        ExecutionStage::InstalledHost,
        WorkRequirement::none(),
    );
    let spec_error = match rejected {
        Ok(_) => return Err("lexical pass accepted for an edit-authorization axis".to_string()),
        Err(error) => error,
    };
    ensure(
        variant_of(&spec_error) == "CrossSatisfaction",
        &format!("expected CrossSatisfaction, got {spec_error}"),
    )?;

    // Evidence layer: an edit-authorization axis backed only by a local
    // lexical pass is refused — at whichever layer the mismatch surfaces.
    let scaffold = AttackScaffold {
        family: ClaimFamily::EditAuthorization,
        stage: ExecutionStage::InstalledHost,
        evidence_class: EvidenceClass::LocalLexicalPass,
        ..AttackScaffold::default()
    };
    let outcome = match scaffold.build() {
        Ok(inner) => inner,
        Err(spec_layer_message) => {
            // The mismatch surfaced at spec construction instead.
            return ensure(
                spec_layer_message.contains("cross-satisfaction"),
                &format!("expected a cross-satisfaction refusal, got {spec_layer_message}"),
            );
        }
    };
    match outcome {
        Err(construction_error) => ensure(
            variant_of(&construction_error) == "CrossSatisfaction",
            &format!("expected CrossSatisfaction, got {construction_error}"),
        ),
        Ok(definition) => {
            let validation_error = expect_validation_err(definition.validate())?;
            ensure(
                variant_of(&validation_error) == "CrossSatisfaction",
                &format!("expected CrossSatisfaction, got {validation_error}"),
            )
        }
    }?;

    // Closure layer: substituting compiler_local_lexical for
    // compiler_static_project in the maintained closure fails resolution.
    let maintained = maintained_code_intelligence(false)?;
    let bounded = bounded_execution(false)?;
    let stand_in = local_lexical(false)?;
    let wrong_registry = registry_with([&stand_in, &bounded]);
    let closure_error = expect_validation_err(maintained.validate_closure(&wrong_registry))?;
    ensure(
        variant_of(&closure_error) == "ImportResolution",
        &format!("expected ImportResolution, got {closure_error}"),
    )
}

#[test]
fn compiler_profile_contract_f02_long_horizon_work_is_not_a_prerequisite_of_bounded_local_profile()
-> Result<(), String> {
    for profile in [local_lexical(false)?, bounded_execution(false)?] {
        ck(profile.validate())?;
        for row in profile.rows.values() {
            ensure(
                row.axis_specs.keys().all(|axis| axis.family != ClaimFamily::PerformanceResource),
                "the model injected a long-horizon performance obligation into a bounded \
                 local profile",
            )?;
            ensure(
                !row.legacy_exit.demands_exit(),
                "the model injected a legacy-exit obligation into a bounded local profile",
            )?;
        }
    }
    // Scoped weakness is expressible through closed states (an unsupported
    // semantic row) instead of forcing global compiler completion rows.
    let local = local_lexical(false)?;
    let rename = local.row(&rid("lex-semantic-rename")?).ok_or("missing lex-semantic-rename")?;
    ensure(
        !rename.disposition.permits_axes() && rename.axis_specs.is_empty(),
        "unsupported row must stay obligation-free",
    )?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f03_issue_pr_workflow_state_cannot_enter_the_evidence_model()
-> Result<(), String> {
    for surface in [
        CollaborationSurface::Issue,
        CollaborationSurface::PullRequest,
        CollaborationSurface::WorkflowRun,
    ] {
        let offered = EvidenceRecord::finish(
            "ev-collab",
            rid("some-row")?,
            ax(ClaimFamily::CompilerInternalFacts, ExecutionStage::SourceTree),
            EvidenceClass::ParserFactProduction,
            SourceTier::RepositoryOwned,
            ExecutionStage::SourceTree,
            WorkPerformed::zero_execution(),
            ExternalProvenance::CollaborationSurfaceState {
                surface,
                identifier: "#12186".to_string(),
            },
        );
        let error = match offered {
            Ok(_) => return Err("collaboration state was accepted as evidence".to_string()),
            Err(error) => error,
        };
        ensure(
            variant_of(&error) == "RejectedProvenance",
            &format!("expected RejectedProvenance for {surface:?}, got {error}"),
        )?;
    }
    // Control: profile-domain artifacts remain representable.
    let domain_evidence = ev(
        "ev-domain",
        "some-row",
        ax(ClaimFamily::CompilerInternalFacts, ExecutionStage::SourceTree),
        EvidenceClass::ParserFactProduction,
        SourceTier::RepositoryOwned,
        ExecutionStage::SourceTree,
        WorkPerformed::zero_execution(),
    )?;
    ensure(
        matches!(domain_evidence.provenance, ExternalProvenance::ProfileDomainArtifacts { .. }),
        "domain-artifact provenance should be preserved verbatim",
    )
}

#[test]
fn compiler_profile_contract_f04_parser_proof_cannot_satisfy_provider_edit_or_installed_host_axes()
-> Result<(), String> {
    for family in [
        ClaimFamily::ProviderConsumption,
        ClaimFamily::EditAuthorization,
        ClaimFamily::ProjectWorldCurrentness,
    ] {
        let stage = if family == ClaimFamily::ProviderConsumption {
            ExecutionStage::ExactProcess
        } else {
            ExecutionStage::InstalledHost
        };
        let rejected = AxisProofSpec::for_axis(
            ax(family, stage),
            BTreeSet::from([EvidenceClass::ParserFactProduction]),
            SourceTier::RepositoryOwned,
            stage,
            WorkRequirement::none(),
        );
        ensure(
            rejected.is_err(),
            "parser-fact class must never be acceptable outside parser families",
        )?;
    }

    // Validator layer: provider axis with a legitimate accept list but
    // parser-class evidence is refused.
    let scaffold = AttackScaffold {
        family: ClaimFamily::ProviderConsumption,
        stage: ExecutionStage::ExactProcess,
        spec_classes: vec![EvidenceClass::ProviderBehaviorProbe],
        spec_min_stage: ExecutionStage::ExactProcess,
        evidence_class: EvidenceClass::ParserFactProduction,
        evidence_stage: ExecutionStage::ExactProcess,
        ..AttackScaffold::default()
    };
    let attack = expect_definition_err(scaffold.build()?)?;
    ensure(
        variant_of(&attack) == "CrossSatisfaction",
        &format!("expected CrossSatisfaction, got {attack}"),
    )?;

    // Stage layer: installed-host proof is not satisfied by exact-process
    // observation even with the right evidence class and family.
    let underflow = AttackScaffold {
        family: ClaimFamily::ProviderConsumption,
        stage: ExecutionStage::InstalledHost,
        spec_classes: vec![EvidenceClass::ProviderBehaviorProbe],
        spec_min_stage: ExecutionStage::InstalledHost,
        evidence_class: EvidenceClass::ProviderBehaviorProbe,
        evidence_stage: ExecutionStage::ExactProcess,
        ..AttackScaffold::default()
    };
    let stage_attack = expect_definition_err(underflow.build()?)?;
    ensure(
        variant_of(&stage_attack) == "StageUnderflow",
        &format!("expected StageUnderflow, got {stage_attack}"),
    )
}

#[test]
fn compiler_profile_contract_f05_fixture_replay_and_oracle_agreement_cannot_satisfy_eir_mechanism()
-> Result<(), String> {
    for class in [EvidenceClass::CuratedFixtureReplay, EvidenceClass::RealPerlOracleAgreement] {
        let rejected = AxisProofSpec::for_axis(
            ax(ClaimFamily::EirMechanism, ExecutionStage::ExactProcess),
            BTreeSet::from([class]),
            SourceTier::RepositoryOwned,
            ExecutionStage::ExactProcess,
            WorkRequirement::none(),
        );
        ensure(rejected.is_err(), "{class:?} must never be acceptable for the EIR mechanism")?;
    }
    // Control: oracle agreement still supports execution-boundedness axes, so
    // the refusal above is EIR-specific rather than a blanket ban.
    let control = AttackScaffold {
        family: ClaimFamily::ExecutionBoundedness,
        stage: ExecutionStage::ExactProcess,
        spec_classes: vec![EvidenceClass::RealPerlOracleAgreement],
        spec_min_stage: ExecutionStage::ExactProcess,
        evidence_class: EvidenceClass::RealPerlOracleAgreement,
        evidence_stage: ExecutionStage::ExactProcess,
        ..AttackScaffold::default()
    };
    ck(control.build()?)?.validate().map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f06_source_locked_debt_cannot_be_typed_as_general_semantic_support()
-> Result<(), String> {
    // Constructor layer: general support over unobserved upstream material is
    // ambiguous strengthening and is refused outright.
    let ambiguous = SupportClaim::new(
        UpstreamObservation::Unobserved,
        CompatibilityAcceptance::NotYetAccepted,
        SemanticSupportLevel::GeneralSemanticSupport,
    );
    ensure(ambiguous.is_err(), "GSS over Unobserved/NotYetAccepted must be refused")?;

    // Validator layer: a GSS row backed only by source-stage observations is
    // refused during validation even though construction floors passed.
    let scaffold = AttackScaffold {
        family: ClaimFamily::EditAuthorization,
        stage: ExecutionStage::PackagedArtifact,
        spec_classes: vec![EvidenceClass::EditAuthorizationProbe],
        spec_min_stage: ExecutionStage::SourceTree,
        support: gss_claim()?,
        evidence_class: EvidenceClass::EditAuthorizationProbe,
        evidence_stage: ExecutionStage::SourceTree,
        ..AttackScaffold::default()
    };
    let built = ck(scaffold.build()?)?;
    let error = expect_validation_err(built.validate())?;
    ensure(
        variant_of(&error) == "SupportOverstatement",
        &format!("expected SupportOverstatement, got {error}"),
    )?;
    ensure(
        error.to_string().contains("source-stage"),
        "error should name source-stage-only evidence",
    )
}

#[test]
fn compiler_profile_contract_f07_source_process_package_install_client_stages_do_not_collapse()
-> Result<(), String> {
    // Stage variants are pairwise distinct and ordered.
    let stages = [
        ExecutionStage::SourceTree,
        ExecutionStage::ExactProcess,
        ExecutionStage::PackagedArtifact,
        ExecutionStage::InstalledHost,
        ExecutionStage::ActualClient,
    ];
    for (index, stage) in stages.iter().enumerate() {
        for weaker in stages.iter().take(index) {
            ensure(stage != weaker, "distinct stages compared equal")?;
            ensure(stage.at_least(*weaker), "stage ordering broke")?;
            ensure(!weaker.at_least(*stage), "stage ordering broke symmetrically")?;
        }
    }

    // Per-axis law: satisfying ProviderConsumption@ExactProcess does not
    // satisfy ProviderConsumption@InstalledHost; each demands its own record.
    let satisfied_axis_spec = spec_for(
        ax(ClaimFamily::ProviderConsumption, ExecutionStage::ExactProcess),
        &[EvidenceClass::ProviderBehaviorProbe],
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkRequirement::at_least(WorkContext::ProductionPath, 1),
    )?;
    let installed_axis = ax(ClaimFamily::ProviderConsumption, ExecutionStage::InstalledHost);
    let installed_axis_spec = spec_for(
        installed_axis,
        &[EvidenceClass::ProviderBehaviorProbe, EvidenceClass::InstalledHostVerification],
        SourceTier::RepositoryOwned,
        ExecutionStage::InstalledHost,
        WorkRequirement::at_least(WorkContext::ProductionPath, 1),
    )?;
    let exact_row = make_row(
        "prov-exact",
        SubjectArea::ProviderAction,
        "provider_exact_process",
        "Providers consume facts inside the exact process.",
        RowDisposition::Required,
        ck(SupportClaim::new(
            UpstreamObservation::ImportedCleanly,
            CompatibilityAcceptance::AcceptedUnchanged,
            SemanticSupportLevel::FactsConsumedByProviders,
        ))?,
        BTreeMap::from([
            (
                ax(ClaimFamily::ProviderConsumption, ExecutionStage::ExactProcess),
                satisfied_axis_spec,
            ),
            (installed_axis, installed_axis_spec),
        ]),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::RepositoryInternalOnly,
    )?;
    let exact_record = ev(
        "ev-prov-exact",
        "prov-exact",
        ax(ClaimFamily::ProviderConsumption, ExecutionStage::ExactProcess),
        EvidenceClass::ProviderBehaviorProbe,
        SourceTier::RepositoryOwned,
        ExecutionStage::ExactProcess,
        WorkPerformed::new(WorkContext::ProductionPath, 2),
    )?;
    let partial = CompilerProfileDefinition::new(
        pid("compiler_stage_collapse_probe")?,
        pver("v1")?,
        "per-axis independence probe",
        BTreeSet::new(),
        single_row_map(exact_row),
        single_evidence_set(exact_record),
        BTreeSet::new(),
    )
    .map_err(|error| error.to_string())?;
    let error = expect_validation_err(partial.validate())?;
    ensure(
        variant_of(&error) == "MissingRequiredEvidence",
        &format!("expected MissingRequiredEvidence, got {error}"),
    )?;
    ensure(error.to_string().contains("InstalledHost"), "unsatisfied axis should name its stage")?;

    // Floor law: exact-process evidence cannot stand in where installed-host
    // observation is demanded.
    let underflow = AttackScaffold {
        family: ClaimFamily::ProviderConsumption,
        stage: ExecutionStage::InstalledHost,
        spec_classes: vec![
            EvidenceClass::ProviderBehaviorProbe,
            EvidenceClass::InstalledHostVerification,
        ],
        spec_min_stage: ExecutionStage::InstalledHost,
        evidence_class: EvidenceClass::ProviderBehaviorProbe,
        evidence_stage: ExecutionStage::ExactProcess,
        evidence_work: WorkPerformed::new(WorkContext::ProductionPath, 5),
        ..AttackScaffold::default()
    };
    let floor_error = expect_definition_err(underflow.build()?)?;
    ensure(
        variant_of(&floor_error) == "StageUnderflow",
        &format!("expected StageUnderflow, got {floor_error}"),
    )?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f08_unsupported_or_unproven_required_row_cannot_vanish_by_omission()
-> Result<(), String> {
    // A required row whose declared axis carries no evidence fails validation;
    // it can neither pass silently nor disappear behind its declaration.
    let mut scaffold = AttackScaffold::default();
    scaffold.include_evidence = false;
    let unproven = ck(scaffold.build()?)?;
    let error = expect_validation_err(unproven.validate())?;
    ensure(
        variant_of(&error) == "MissingRequiredEvidence",
        &format!("expected MissingRequiredEvidence, got {error}"),
    )?;
    ensure(error.to_string().contains("attack-row"), "error should name the unproven row")?;

    // Omitting the row expresses absence of the obligation, never
    // satisfaction: the resulting profile validates but is a different
    // semantic identity with fewer obligations.
    let mut absent = AttackScaffold::default();
    absent.include_row = false;
    absent.include_evidence = false;
    let without_row = ck(absent.build()?)?;
    ck(without_row.validate())?;
    ensure(
        without_row.semantic_fingerprint() != unproven.semantic_fingerprint(),
        "omitting a required row must change semantic identity",
    )?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f09_zero_work_execution_cannot_satisfy_a_required_work_row()
-> Result<(), String> {
    for units in [0u32, 1] {
        let scaffold = AttackScaffold {
            family: ClaimFamily::EirMechanism,
            stage: ExecutionStage::ExactProcess,
            spec_classes: vec![EvidenceClass::EirMechanismEvaluation],
            spec_min_stage: ExecutionStage::ExactProcess,
            spec_work: WorkRequirement::at_least(WorkContext::ProductionPath, 2),
            evidence_class: EvidenceClass::EirMechanismEvaluation,
            evidence_stage: ExecutionStage::ExactProcess,
            evidence_work: WorkPerformed::new(WorkContext::ProductionPath, units),
            ..AttackScaffold::default()
        };
        let error = expect_definition_err(scaffold.build()?)?;
        ensure(
            variant_of(&error) == "WorkMismatch" && error.to_string().contains("minimum"),
            &format!("expected WorkMismatch about minimum work, got {error}"),
        )?;
    }
    // Control: exactly the required minimum satisfies the row.
    let sufficient = AttackScaffold {
        family: ClaimFamily::EirMechanism,
        stage: ExecutionStage::ExactProcess,
        spec_classes: vec![EvidenceClass::EirMechanismEvaluation],
        spec_min_stage: ExecutionStage::ExactProcess,
        spec_work: WorkRequirement::at_least(WorkContext::ProductionPath, 2),
        evidence_class: EvidenceClass::EirMechanismEvaluation,
        evidence_stage: ExecutionStage::ExactProcess,
        evidence_work: WorkPerformed::new(WorkContext::ProductionPath, 2),
        ..AttackScaffold::default()
    };
    let built = ck(sufficient.build()?)?;
    ck(built.validate())
}

#[test]
fn compiler_profile_contract_f10_cold_oracle_work_cannot_be_typed_as_production_work_avoided()
-> Result<(), String> {
    for context in [WorkContext::ColdStart, WorkContext::OracleHarness] {
        let scaffold = AttackScaffold {
            family: ClaimFamily::EirMechanism,
            stage: ExecutionStage::ExactProcess,
            spec_classes: vec![EvidenceClass::EirMechanismEvaluation],
            spec_min_stage: ExecutionStage::ExactProcess,
            spec_work: WorkRequirement::at_least(WorkContext::ProductionPath, 2),
            evidence_class: EvidenceClass::EirMechanismEvaluation,
            evidence_stage: ExecutionStage::ExactProcess,
            evidence_work: WorkPerformed::new(context, 9),
            ..AttackScaffold::default()
        };
        let error = expect_definition_err(scaffold.build()?)?;
        ensure(
            variant_of(&error) == "WorkMismatch" && error.to_string().contains("context"),
            &format!("cold/oracle work must be refused for production-work rows, got {error}"),
        )?;
    }
    Ok(())
}

#[test]
fn compiler_profile_contract_f11_imported_lower_profile_losing_row_or_limitation_fails_validation()
-> Result<(), String> {
    let base = static_project(false)?;

    // Lost imported row (with its evidence so pairing checks do not fire first).
    let mut lost_row = preserved_importer(&base)?;
    lost_row.rows.remove(&rid("sp-parser-facts")?);
    lost_row.evidence.retain(|record| record.row_id.as_str() != "sp-parser-facts");
    let lost_error =
        expect_validation_err(lost_row.validate_closure(&registry_with([&base, &base])))?;
    ensure(
        variant_of(&lost_error) == "ImportPreservation"
            && lost_error.to_string().contains("sp-parser-facts")
            && lost_error.to_string().contains("disappeared"),
        &format!("expected disappearance preservation failure, got {lost_error}"),
    )?;

    // Altered imported row.
    let mut altered = preserved_importer(&base)?;
    let world_id = rid("sp-world-currentness")?;
    if let Some(row) = altered.rows.get_mut(&world_id) {
        row.statement.push_str(" (weakened)");
    }
    let altered_error =
        expect_validation_err(altered.validate_closure(&registry_with([&base, &base])))?;
    ensure(
        variant_of(&altered_error) == "ImportPreservation"
            && altered_error.to_string().contains("altered"),
        &format!("expected alteration preservation failure, got {altered_error}"),
    )?;

    // Dropped imported limitation.
    let mut dropped = preserved_importer(&base)?;
    dropped.limitations.clear();
    let dropped_error =
        expect_validation_err(dropped.validate_closure(&registry_with([&base, &base])))?;
    ensure(
        variant_of(&dropped_error) == "ImportPreservation"
            && dropped_error.to_string().contains("dropped"),
        &format!("expected dropped-limitation failure, got {dropped_error}"),
    )?;

    // Digest drift between importer binding and resolved content.
    let mut drifted = preserved_importer(&base)?;
    let stale_binding = ck(CompilerProfileImport::new(
        base.profile_id.clone(),
        base.version.clone(),
        ProfileContentDigest::from_fingerprint(0x0123_4567_89ab_cdef),
    ))?;
    drifted.imports = BTreeSet::from([stale_binding]);
    let drift_error =
        expect_validation_err(drifted.validate_closure(&registry_with([&base, &base])))?;
    ensure(
        variant_of(&drift_error) == "ImportResolution"
            && drift_error.to_string().contains("digest"),
        &format!("expected digest resolution failure, got {drift_error}"),
    )?;

    // Imported profile entirely missing from the registry.
    let empty_registry = ProfileRegistry::new();
    let importer = preserved_importer(&base)?;
    let absent_error = expect_validation_err(importer.validate_closure(&empty_registry))?;
    ensure(
        variant_of(&absent_error) == "ImportResolution"
            && absent_error.to_string().contains("absent"),
        &format!("expected absent-import failure, got {absent_error}"),
    )
}

#[test]
fn compiler_profile_contract_f12_row_ordering_does_not_change_the_semantic_fingerprint()
-> Result<(), String> {
    for (name, build) in [
        ("local_lexical", local_lexical as fn(bool) -> Result<CompilerProfileDefinition, String>),
        ("static_project", static_project as fn(bool) -> Result<CompilerProfileDefinition, String>),
        (
            "bounded_execution",
            bounded_execution as fn(bool) -> Result<CompilerProfileDefinition, String>,
        ),
        (
            "maintained_code_intelligence",
            maintained_code_intelligence as fn(bool) -> Result<CompilerProfileDefinition, String>,
        ),
    ] {
        let forward = build(false)?;
        let reversed = build(true)?;
        ensure(
            forward.semantic_fingerprint() == reversed.semantic_fingerprint(),
            &format!("{name}: insertion order changed the semantic fingerprint"),
        )?;
        ensure(
            forward.content_digest_hex() == reversed.content_digest_hex(),
            &format!("{name}: insertion order changed the content digest"),
        )?;
    }
    Ok(())
}

#[test]
fn compiler_profile_contract_f13_no_scalar_score_or_aggregate_percentage_exists_in_the_api()
-> Result<(), String> {
    let module_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("compiler_profile");
    let forbidden_score_terms =
        ["score", "readiness", "percent", "aggregate", "weighted", "rating"];
    let mut public_functions = Vec::new();
    for file in MODULE_FILES {
        let source = fs::read_to_string(module_dir.join(file))
            .map_err(|error| format!("read {}: {error}", module_dir.join(file).display()))?;
        public_functions.extend(public_function_names(&source));
    }
    let mut violations = Vec::new();
    for name in &public_functions {
        for term in forbidden_score_terms {
            if name.contains(term) {
                violations.push(format!("{name} contains forbidden scoring term {term:?}"));
            }
        }
    }
    ensure(
        violations.is_empty(),
        &format!("scoring API found in compiler_profile: {violations:?}"),
    )?;

    // Positive surface documentation: every required model type is exported
    // from the module root, and nothing else claims to summarize readiness.
    let mod_rs = fs::read_to_string(module_dir.join("mod.rs"))
        .map_err(|error| format!("read mod.rs: {error}"))?;
    for expected in REQUIRED_EXPORTS {
        ensure(
            mod_rs.contains(expected),
            &format!("module root must export {expected} for the successor inventory"),
        )?;
    }
    Ok(())
}

#[test]
fn compiler_profile_contract_f14_ceiling_legacy_exit_owner_and_invalidation_are_mandatory_on_rows()
-> Result<(), String> {
    // Owner references are validated at construction.
    for bad in ["", "owner", "#", "#abc"] {
        let rejected = OwnerAndWakeEvent::new(bad, WakeEvent::NoScheduledWake);
        ensure(rejected.is_err(), "owner reference {bad:?} must be refused")?;
    }
    let accepted = ck(OwnerAndWakeEvent::new("#1234", WakeEvent::ScheduledReview))?;
    ensure(accepted.owner_issue == "#1234", "accepted owner reference should round-trip")?;

    // Legacy exit demands an explicit, non-empty dimension set.
    ensure(
        LegacyExitRequirement::required(BTreeSet::new()).is_err(),
        "empty legacy-exit dimension set must be refused",
    )?;
    let demanded =
        ck(LegacyExitRequirement::required(BTreeSet::from([LegacyExitDimension::OldPathAbsence])))?;
    ensure(demanded.demands_exit(), "a dimensioned exit requirement demands exit")?;

    // Limitations are validated tokens.
    ensure(AllowedLimitation::new("bad id", "desc").is_err(), "limitation ids are stable tokens")?;

    // The row constructor takes ceiling/legacy/invalidation/owner explicitly;
    // absence is unrepresentable, and the values survive on the row verbatim.
    let row = make_row(
        "mandatory-row",
        SubjectArea::DocumentationSurface,
        "mandatory_fields",
        "Every mandatory field is explicit.",
        RowDisposition::Optional,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::UpstreamRelease,
        ClaimCeiling::PublicSupportStatement,
    )?;
    ensure(
        row.invalidation_input == InvalidationInput::UpstreamRelease,
        "invalidation input must round-trip",
    )?;
    ensure(
        row.claim_ceiling == ClaimCeiling::PublicSupportStatement,
        "claim ceiling must round-trip",
    )?;
    ensure(!row.legacy_exit.demands_exit(), "explicit none() stays not-applicable")?;
    ensure(row.owner.owner_issue == "#12186", "owner must round-trip")?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f15_support_release_authority_cannot_be_inferred_from_a_profile_result()
-> Result<(), String> {
    // No public function in the module derives authorization, release, or
    // publication decisions; validity returns a bare closure result.
    let module_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("compiler_profile");
    let forbidden_authority_terms =
        ["authoriz", "release_", "publish", "approv", "grant_", "decision"];
    let mut violations = Vec::new();
    for file in MODULE_FILES {
        let source = fs::read_to_string(module_dir.join(file))
            .map_err(|error| format!("read {}: {error}", module_dir.join(file).display()))?;
        for name in public_function_names(&source) {
            for term in forbidden_authority_terms {
                if name.contains(term) {
                    violations.push(format!("{name} contains authority term {term:?}"));
                }
            }
        }
    }
    ensure(
        violations.is_empty(),
        &format!("authority-inference API found in compiler_profile: {violations:?}"),
    )?;

    // Typed boundary control: even a fully valid profile carrying the highest
    // claim ceiling yields only Ok(()) — never an authorization value.
    let row = make_row(
        "ceiling-row",
        SubjectArea::DocumentationSurface,
        "public_ceiling_row",
        "A valid profile can carry a public support statement ceiling.",
        RowDisposition::Optional,
        unsupported_claim()?,
        BTreeMap::new(),
        &[],
        LegacyExitRequirement::none(),
        InvalidationInput::NoneDeclared,
        ClaimCeiling::PublicSupportStatement,
    )?;
    let profile = CompilerProfileDefinition::new(
        pid("compiler_ceiling_probe")?,
        pver("v1")?,
        "ceiling boundary probe",
        BTreeSet::new(),
        single_row_map(row),
        BTreeSet::new(),
        BTreeSet::new(),
    )
    .map_err(|error| error.to_string())?;
    ensure(profile.validate().is_ok(), "the ceiling probe profile should validate")?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f16_import_preservation_happy_path_binds_exact_identity_and_digest()
-> Result<(), String> {
    let base = static_project(false)?;
    let importer = preserved_importer(&base)?;
    let import = sole_import_of(&importer)?;
    ensure(
        import.imported_profile == base.profile_id && import.imported_version == base.version,
        "import must bind exact lower-profile identity and version",
    )?;
    ensure(
        import.content_digest.as_str() == base.content_digest_hex(),
        "import must bind the exact current content digest",
    )?;
    ensure(
        importer.row_count() == base.row_count() + 1,
        "preserved rows plus one importer-local row expected",
    )?;
    let registry = registry_with([&base, &base]);
    ck(importer.validate_closure(&registry))?;

    // Transitive closure: the maintained fixture closes over both lower
    // profiles at once.
    let project = static_project(false)?;
    let bounded = bounded_execution(false)?;
    let maintained = maintained_code_intelligence(false)?;
    ck(maintained.validate_closure(&registry_with([&project, &bounded])))?;

    // Digest binding is live: touching the dependency breaks old bindings.
    let mut touched = static_project(false)?;
    if let Some(row) = touched.rows.get_mut(&rid("sp-parser-facts")?) {
        row.statement.push_str(" (touched)");
    }
    let stale = preserved_importer(&touched)?;
    let error = expect_validation_err(stale.validate_closure(&registry_with([&base, &base])))?;
    ensure(
        variant_of(&error) == "ImportResolution",
        &format!("stale digest binding must fail resolution, got {error}"),
    )?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f17_every_semantic_field_kind_change_moves_the_fingerprint()
-> Result<(), String> {
    let base = static_project(false)?;
    let baseline = base.semantic_fingerprint();
    ensure(
        base.clone().semantic_fingerprint() == baseline,
        "fingerprint of an unchanged clone must be identical",
    )?;

    let parser_row_id = rid("sp-parser-facts")?;
    let world_record_id = "ev-sp-world".to_string();
    let mutations: Vec<(
        &'static str,
        Box<dyn Fn(&mut CompilerProfileDefinition) -> Result<(), String>>,
    )> = vec![
        (
            "profile_id",
            Box::new(|def: &mut CompilerProfileDefinition| {
                def.profile_id = pid("compiler_static_project_alt")?;
                Ok(())
            }),
        ),
        (
            "version",
            Box::new(|def: &mut CompilerProfileDefinition| {
                def.version = pver("v2")?;
                Ok(())
            }),
        ),
        (
            "purpose",
            Box::new(|def: &mut CompilerProfileDefinition| {
                def.purpose.push_str(" plus scope");
                Ok(())
            }),
        ),
        (
            "imports",
            Box::new(|def: &mut CompilerProfileDefinition| {
                let import = ck(CompilerProfileImport::new(
                    pid("compiler_local_lexical")?,
                    pver("v1")?,
                    ProfileContentDigest::from_fingerprint(7),
                ))?;
                def.imports.insert(import);
                Ok(())
            }),
        ),
        (
            "row.statement",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&parser_row_id)
                    .ok_or("missing sp-parser-faces")?
                    .statement
                    .push_str(" altered");
                Ok(())
            }),
        ),
        (
            "row.subject",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .subject
                    .selector = "parse_other_files".to_string();
                Ok(())
            }),
        ),
        (
            "row.disposition",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .disposition = RowDisposition::Optional;
                Ok(())
            }),
        ),
        (
            "row.support_claim",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .support_claim
                    .semantic_support = SemanticSupportLevel::FactsConsumedByProviders;
                Ok(())
            }),
        ),
        (
            "axis.acceptable_classes",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let row =
                    def.rows.get_mut(&rid("sp-parser-facts")?).ok_or("missing sp-parser-facts")?;
                for spec in row.axis_specs.values_mut() {
                    spec.acceptable_classes.insert(EvidenceClass::RealPerlOracleAgreement);
                }
                Ok(())
            }),
        ),
        (
            "spec.min_tier",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let row =
                    def.rows.get_mut(&rid("sp-parser-facts")?).ok_or("missing sp-parser-facts")?;
                for spec in row.axis_specs.values_mut() {
                    spec.min_tier = SourceTier::Vendored;
                }
                Ok(())
            }),
        ),
        (
            "spec.min_stage",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let row =
                    def.rows.get_mut(&rid("sp-parser-facts")?).ok_or("missing sp-parser-facts")?;
                for spec in row.axis_specs.values_mut() {
                    spec.min_stage = ExecutionStage::ExactProcess;
                }
                Ok(())
            }),
        ),
        (
            "spec.work.minimum_units",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let row = def
                    .rows
                    .get_mut(&rid("sp-world-currentness")?)
                    .ok_or("missing sp-world-currentness")?;
                for spec in row.axis_specs.values_mut() {
                    spec.work.minimum_units += 1;
                }
                Ok(())
            }),
        ),
        (
            "completeness.currentness",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .completeness
                    .currentness = CurrentnessRule::PinnedToContentDigest;
                Ok(())
            }),
        ),
        (
            "completeness.completeness",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .completeness
                    .completeness = CompletenessRule::SinglePointCheck;
                Ok(())
            }),
        ),
        (
            "row.limitations",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .limitations
                    .insert(limitation("extra-row-limit", "An added row limitation.")?);
                Ok(())
            }),
        ),
        (
            "legacy_exit",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .legacy_exit = LegacyExitRequirement::required(BTreeSet::from([
                    LegacyExitDimension::RecurrenceGuard,
                ]))
                .map_err(|error| error.to_string())?;
                Ok(())
            }),
        ),
        (
            "invalidation_input",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .invalidation_input = InvalidationInput::UpstreamRelease;
                Ok(())
            }),
        ),
        (
            "claim_ceiling",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .claim_ceiling = ClaimCeiling::DocumentedProductBehavior;
                Ok(())
            }),
        ),
        (
            "owner.issue",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .owner
                    .owner_issue = "#999999".to_string();
                Ok(())
            }),
        ),
        (
            "owner.wake_event",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                def.rows
                    .get_mut(&rid("sp-parser-facts")?)
                    .ok_or("missing sp-parser-facts")?
                    .owner
                    .wake_event = WakeEvent::NextProductRelease;
                Ok(())
            }),
        ),
        (
            "evidence.class",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let record = def
                    .evidence
                    .iter()
                    .find(|r| r.record_id == "ev-sp-parser")
                    .cloned()
                    .ok_or("missing ev-sp-parser")?;
                def.evidence.remove(&record);
                let mut changed = record;
                changed.class = EvidenceClass::LocalLexicalPass;
                def.evidence.insert(changed);
                Ok(())
            }),
        ),
        (
            "evidence.stage",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let record = def
                    .evidence
                    .iter()
                    .find(|r| r.record_id == world_record_id)
                    .cloned()
                    .ok_or("missing world record")?;
                def.evidence.remove(&record);
                let mut changed = record;
                changed.stage_observed = ExecutionStage::InstalledHost;
                def.evidence.insert(changed);
                Ok(())
            }),
        ),
        (
            "evidence.tier",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let record = def
                    .evidence
                    .iter()
                    .find(|r| r.record_id == "ev-sp-cross")
                    .cloned()
                    .ok_or("missing ev-sp-cross")?;
                def.evidence.remove(&record);
                let mut changed = record;
                changed.tier = SourceTier::Vendored;
                def.evidence.insert(changed);
                Ok(())
            }),
        ),
        (
            "evidence.work.units",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let record = def
                    .evidence
                    .iter()
                    .find(|r| r.record_id == "ev-sp-cross")
                    .cloned()
                    .ok_or("missing ev-sp-cross")?;
                def.evidence.remove(&record);
                let mut changed = record;
                changed.work.units = 9;
                def.evidence.insert(changed);
                Ok(())
            }),
        ),
        (
            "evidence.provenance.reference",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let record = def
                    .evidence
                    .iter()
                    .find(|r| r.record_id == "ev-sp-cross")
                    .cloned()
                    .ok_or("missing ev-sp-cross")?;
                def.evidence.remove(&record);
                let mut changed = record;
                changed.provenance = ExternalProvenance::ProfileDomainArtifacts {
                    reference: "in-memory://other".to_string(),
                };
                def.evidence.insert(changed);
                Ok(())
            }),
        ),
        (
            "profile.limitation.description",
            Box::new(move |def: &mut CompilerProfileDefinition| {
                let removed =
                    limitation("sp-limit-index-lag", "Index refresh may lag rapid saves.")?;
                def.limitations.remove(&removed);
                def.limitations.insert(limitation(
                    "sp-limit-index-lag",
                    "Index refresh may lag rapid saves considerably.",
                )?);
                Ok(())
            }),
        ),
    ];

    let mut applied = 0;
    for (label, mutate) in &mutations {
        let mut variant = base.clone();
        mutate(&mut variant)?;
        ensure(
            variant.semantic_fingerprint() != baseline,
            &format!("mutation '{label}' did not move the fingerprint"),
        )?;
        applied += 1;
    }
    ensure(applied >= 24, "expected full mutation coverage over every field kind")?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f18_four_shape_fixtures_validate_with_stable_content_digests()
-> Result<(), String> {
    let local = local_lexical(false)?;
    let project = static_project(false)?;
    let bounded = bounded_execution(false)?;
    let maintained = maintained_code_intelligence(false)?;
    for (name, profile) in [
        ("compiler_local_lexical.v1", &local),
        ("compiler_static_project.v1", &project),
        ("compiler_bounded_execution.v1", &bounded),
        ("compiler_maintained_code_intelligence.v1", &maintained),
    ] {
        ck(profile.validate())?;
        let digest = profile.content_digest_hex();
        ensure(digest.len() == 16, &format!("{name}: digest must be 16 characters"))?;
        ensure(
            digest.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            &format!("{name}: digest must be lowercase hex"),
        )?;
    }
    let digests = [
        local.content_digest_hex(),
        project.content_digest_hex(),
        bounded.content_digest_hex(),
        maintained.content_digest_hex(),
    ];
    let unique = digests.iter().collect::<BTreeSet<_>>();
    ensure(unique.len() == digests.len(), "distinct profiles must have distinct fingerprints")?;

    // The maintained shape's imports bind exactly its freshly built deps.
    let import_ids: Vec<String> = maintained
        .imports
        .iter()
        .map(|import| {
            format!("{}.{}", import.imported_profile.as_str(), import.imported_version.as_str())
        })
        .collect();
    ensure(
        import_ids == ["compiler_bounded_execution.v1", "compiler_static_project.v1"],
        &format!("unexpected import set: {import_ids:?}"),
    )?;
    Ok(())
}

#[test]
fn compiler_profile_contract_f19_closed_dispositions_survive_validation_without_becoming_required()
-> Result<(), String> {
    let profile = static_project(false)?;
    ck(profile.validate())?;

    let conditional_id = rid("sp-cross-file-nav")?;
    let optional_id = rid("sp-provider-rename")?;
    let na_id = rid("sp-packaging-na")?;

    let conditional = profile.row(&conditional_id).ok_or("missing conditional row")?;
    ensure(
        matches!(conditional.disposition, RowDisposition::Conditional(_)),
        "conditional disposition must survive validation unchanged",
    )?;
    let optional = profile.row(&optional_id).ok_or("missing optional row")?;
    ensure(
        matches!(optional.disposition, RowDisposition::Optional),
        "optional disposition must survive validation unchanged",
    )?;
    let na = profile.row(&na_id).ok_or("missing not-applicable row")?;
    ensure(
        matches!(na.disposition, RowDisposition::NotApplicable { .. }),
        "not-applicable disposition must survive validation unchanged",
    )?;

    // Conditional rows do not become conjunctive: dropping their evidence
    // leaves the profile valid, while the disposition itself stays typed.
    let mut without_conditional_evidence = profile.clone();
    without_conditional_evidence.evidence.retain(|record| record.row_id != conditional_id);
    ck(without_conditional_evidence.validate())?;
    let still_conditional =
        without_conditional_evidence.row(&conditional_id).ok_or("conditional row vanished")?;
    ensure(
        matches!(still_conditional.disposition, RowDisposition::Conditional(_)),
        "conditional disposition mutated after evidence removal",
    )?;

    // Unsupported rows keep their closed state and reason.
    let local = local_lexical(false)?;
    let unsupported = local.row(&rid("lex-semantic-rename")?).ok_or("missing unsupported row")?;
    match &unsupported.disposition {
        RowDisposition::Unsupported { reason } => {
            ensure(!reason.is_empty(), "unsupported reason must be preserved")?;
        }
        other => return Err(format!("expected Unsupported disposition, got {other:?}")),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// API-surface scan support
// ---------------------------------------------------------------------------

const MODULE_FILES: [&str; 6] =
    ["mod.rs", "identity.rs", "dimensions.rs", "requirements.rs", "rows.rs", "profile.rs"];

/// Public `pub fn` names declared in a module source file. Trait-impl methods
/// and private helpers carry no `pub` marker and are excluded by design.
fn public_function_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let after_pub = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("pub const fn ") {
            rest
        } else {
            continue;
        };
        let name: String = after_pub
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names
}

/// The complete exported type vocabulary required by #12186.
const REQUIRED_EXPORTS: [&str; 37] = [
    "CompilerProfileId",
    "CompilerProfileVersion",
    "CompilerProfileRowId",
    "ProfileContentDigest",
    "CompilerProfileDefinition",
    "CompilerProfileImport",
    "ProfileRegistry",
    "CompilerProfileRow",
    "RowDisposition",
    "ConditionalActivation",
    "AxisProofSpec",
    "ClaimFamily",
    "ProofAxis",
    "EvidenceClass",
    "SourceTier",
    "ExecutionStage",
    "SubjectArea",
    "SubjectSelector",
    "SupportClaim",
    "UpstreamObservation",
    "CompatibilityAcceptance",
    "SemanticSupportLevel",
    "WorkContext",
    "WorkPerformed",
    "WorkRequirement",
    "CompletenessRequirement",
    "CompletenessRule",
    "CurrentnessRule",
    "AllowedLimitation",
    "LegacyExitDimension",
    "LegacyExitRequirement",
    "ClaimCeiling",
    "InvalidationInput",
    "WakeEvent",
    "OwnerAndWakeEvent",
    "CollaborationSurface",
    "ExternalProvenance",
];
