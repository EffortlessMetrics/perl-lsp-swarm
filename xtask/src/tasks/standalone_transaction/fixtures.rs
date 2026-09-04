//! Canonical pipelines exercised by the CLI `check` surface. Compiled
//! unconditionally (not under `cfg(test)`) so the full vocabulary stays
//! constructed and the smoke path proves the model end to end without any
//! live configuration, network, or platform mechanics.

use super::*;

/// Run every canonical pipeline; returns the number verified.
pub(super) fn run_canonical_pipelines() -> ContractResult<usize> {
    archive_pair_green_pipeline()?;
    registry_source_green_pipeline()?;
    local_development_non_authoritative_pipeline()?;
    uninstall_green_pipeline()?;
    latest_requested_never_resolves_probe()?;
    fallback_branch_probe()?;
    Ok(6)
}

pub(super) fn linux_gnu_target() -> TargetIdentity {
    TargetIdentity {
        platform: Platform::Linux,
        triple: "x86_64-unknown-linux-gnu".to_string(),
        libc: LibcDisposition::Gnu,
    }
}

fn base_intent(mode: InstallMode, operation: InstallOperation) -> StandaloneInstallIntent {
    StandaloneInstallIntent {
        schema_version: INTENT_SCHEMA_VERSION.to_string(),
        transaction_id: "tx-11099-canonical".to_string(),
        attempt_id: "attempt-1".to_string(),
        operation,
        route: RouteMode::FirstPartyPosix,
        mode,
        selector: if mode == InstallMode::ReleaseArchive {
            ReleaseSelector::exact("v0.18.0")
        } else {
            ReleaseSelector::not_applicable()
        },
        target: linux_gnu_target(),
        target_override: None,
        requested_product_unit: ProductUnit::ServerDapPair,
        destination_role: DestinationRole::UserLocal,
        path_policy: PathPolicy::Persist,
        fallback_policy: FallbackPolicy::Forbidden,
        trusted_config_digest: "aa".repeat(32),
        policy_version: "policy-v1".to_string(),
        contract_generation: 1,
    }
}

pub(super) fn release_archive_subject(intent: &StandaloneInstallIntent) -> ReleaseArchiveSubject {
    ReleaseArchiveSubject {
        schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
        subject_id: "subject-archive-canonical".to_string(),
        repository: "EffortlessMetrics/perl-lsp-swarm".to_string(),
        tag: "v0.18.0".to_string(),
        topology_id: "topology-v018".to_string(),
        topology_digest: "bb".repeat(32),
        topology_row: "linux-gnu-x86_64".to_string(),
        target: intent.target.clone(),
        archive_format: ArchiveFormat::TarGz,
        archive_name: "perllsp-v0.18.0-x86_64-linux-gnu.tar.gz".to_string(),
        expected_members: vec![
            MemberIdentity {
                role: MemberRole::PerllspServer,
                artifact_name: "perllsp".to_string(),
            },
            MemberIdentity {
                role: MemberRole::PerlDapAdapter,
                artifact_name: "perl-dap-adapter".to_string(),
            },
        ],
        product_unit: intent.requested_product_unit,
        integrity_policy_id: "integrity-v1".to_string(),
        provenance_policy_id: None,
        destination_role: intent.destination_role,
    }
}

fn registry_source_subject(intent: &StandaloneInstallIntent) -> ExactRegistrySourceSubject {
    ExactRegistrySourceSubject {
        schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
        subject_id: "subject-registry-canonical".to_string(),
        registry_id: "crates-io".to_string(),
        package: "perllsp".to_string(),
        version: "0.18.0".to_string(),
        lockfile_digest: Some("cc".repeat(32)),
        toolchain_policy_id: "toolchain-stable-v1".to_string(),
        target: intent.target.clone(),
        product_unit: intent.requested_product_unit,
        executable_role: MemberRole::PerllspServer,
        destination_role: intent.destination_role,
    }
}

fn dag_for(intent: &StandaloneInstallIntent) -> StageDag {
    use StageId::*;
    let mut nodes: Vec<StageNode> = match intent.mode {
        InstallMode::ReleaseArchive => vec![
            node(ResolveSubject, Applicability::Required, &[]),
            node(Transport, Applicability::Required, &[ResolveSubject]),
            node(ChecksumIntegrity, Applicability::Required, &[Transport]),
            node(Provenance, Applicability::NotApplicable, &[ChecksumIntegrity]),
            node(ArchiveManifestAndStaging, Applicability::Required, &[ChecksumIntegrity]),
            node(ExecutableObservation, Applicability::Required, &[ArchiveManifestAndStaging]),
            node(Promotion, Applicability::Required, &[ExecutableObservation]),
            node(PathPersistence, Applicability::Required, &[Promotion]),
            node(FreshProcessObservation, Applicability::Required, &[Promotion]),
            node(
                InstalledTransition,
                Applicability::Required,
                &[PathPersistence, FreshProcessObservation],
            ),
        ],
        InstallMode::ExactRegistrySource => vec![
            node(ResolveSubject, Applicability::Required, &[]),
            node(SourceBuild, Applicability::Required, &[ResolveSubject]),
            node(Promotion, Applicability::Required, &[SourceBuild]),
            node(PathPersistence, Applicability::Required, &[Promotion]),
            node(FreshProcessObservation, Applicability::Required, &[Promotion]),
            node(
                InstalledTransition,
                Applicability::Required,
                &[PathPersistence, FreshProcessObservation],
            ),
        ],
        // Local development is non-authoritative end to end: only subject
        // resolution and a local build may even be described.
        InstallMode::ExplicitLocalDevelopment => vec![
            node(ResolveSubject, Applicability::Required, &[]),
            node(SourceBuild, Applicability::Required, &[ResolveSubject]),
        ],
    };
    if intent.operation == InstallOperation::Uninstall {
        nodes.push(node(
            StageId::Uninstall,
            Applicability::Required,
            &[StageId::InstalledTransition],
        ));
    }
    StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: intent.mode,
        product_unit: intent.requested_product_unit,
        nodes,
    }
}

fn node(stage: StageId, applicability: Applicability, predecessors: &[StageId]) -> StageNode {
    StageNode { stage_id: stage, applicability, predecessors: predecessors.to_vec() }
}

/// Build the canonical green receipt chain for a DAG: succeeded receipts with
/// recomputed predecessor digests and subject-bound policy identities,
/// explicit not_applicable evidence for authorized skips.
pub(super) fn green_chain(
    transaction: &str,
    attempt: &str,
    subject: &ResolvedStandaloneInstallSubject,
    subject_digest: &str,
    dag: &StageDag,
) -> ContractResult<Vec<StageReceipt>> {
    let mut chain: Vec<(StageId, String)> = Vec::new();
    let mut receipts = Vec::new();
    for node in dag.nodes.iter() {
        if node.applicability == Applicability::NotApplicable {
            let mut skip =
                succeeded_receipt(transaction, attempt, subject_digest, node.stage_id, &[]);
            skip.result = StageResult::NotApplicable;
            let (integrity, provenance, toolchain) =
                expected_receipt_policies(subject, node.stage_id);
            skip.integrity_policy_id = integrity;
            skip.provenance_policy_id = provenance;
            skip.toolchain_policy_id = toolchain;
            let digest = skip.validate()?;
            chain.push((node.stage_id, digest.clone()));
            receipts.push(skip);
            continue;
        }
        let predecessors: Vec<String> = node
            .predecessors
            .iter()
            .map(|stage| {
                chain
                    .iter()
                    .find(|(done, _)| done == stage)
                    .map(|(_, digest)| digest.clone())
                    .ok_or_else(|| {
                        ContractError::new(
                            ContractViolation::PredecessorMismatch,
                            format!(
                                "fixture stage {} arrived before predecessor {}",
                                node.stage_id.as_str(),
                                stage.as_str()
                            ),
                        )
                    })
            })
            .collect::<ContractResult<Vec<_>>>()?;
        let mut receipt =
            succeeded_receipt(transaction, attempt, subject_digest, node.stage_id, &predecessors);
        let (integrity, provenance, toolchain) = expected_receipt_policies(subject, node.stage_id);
        receipt.integrity_policy_id = integrity;
        receipt.provenance_policy_id = provenance;
        receipt.toolchain_policy_id = toolchain;
        let digest = receipt.validate()?;
        chain.push((node.stage_id, digest));
        receipts.push(receipt);
    }
    Ok(receipts)
}

pub(super) fn succeeded_receipt(
    transaction: &str,
    attempt: &str,
    subject_digest: &str,
    stage: StageId,
    predecessors: &[String],
) -> StageReceipt {
    StageReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        transaction_id: transaction.to_string(),
        attempt_id: attempt.to_string(),
        subject_digest: subject_digest.to_string(),
        stage_id: stage,
        implementation_identity: format!("fixture/{}", stage.as_str()),
        integrity_policy_id: None,
        provenance_policy_id: None,
        toolchain_policy_id: None,
        predecessor_receipt_digests: predecessors.to_vec(),
        input_artifact_ids: Vec::new(),
        result: StageResult::Succeeded,
        reason: ReasonFamily::None,
        next_action: ActionClass::None,
        output_evidence_ids: Vec::new(),
        instrument_completeness: InstrumentCompleteness::Complete,
        redaction_disposition: RedactionDisposition::RedactedRolesOnly,
    }
}

fn fold_green(
    intent: &StandaloneInstallIntent,
    dag: &StageDag,
    subject: &ResolvedStandaloneInstallSubject,
    subject_digest: &str,
) -> ContractResult<TerminalStandaloneInstallOutcome> {
    let receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, subject, subject_digest, dag)?;
    fold_terminal_outcome(FanInInput {
        dag,
        operation: intent.operation,
        mode: intent.mode,
        transaction_id: &intent.transaction_id,
        attempt_id: &intent.attempt_id,
        subject,
        subject_digest,
        receipts: &receipts,
    })
}

fn archive_pair_green_pipeline() -> ContractResult<()> {
    let intent = base_intent(InstallMode::ReleaseArchive, InstallOperation::Install);
    let subject = resolve_subject(
        &intent,
        ResolvedStandaloneInstallSubject::ReleaseArchive(release_archive_subject(&intent)),
    )?;
    let subject_digest = subject.validate()?;
    let dag = dag_for(&intent);
    let outcome = fold_green(&intent, &dag, &subject, &subject_digest)?;
    if outcome.result == TerminalResult::Installed
        && outcome.candidate_disposition == CandidateDisposition::CurrentConfirmed
    {
        Ok(())
    } else {
        violation(
            ContractViolation::OutcomeConflict,
            format!("archive-pair pipeline must install cleanly, got {}", outcome.result.as_str()),
        )
    }
}

fn registry_source_green_pipeline() -> ContractResult<()> {
    let mut intent = base_intent(InstallMode::ExactRegistrySource, InstallOperation::Update);
    intent.requested_product_unit = ProductUnit::ServerOnly;
    let subject = resolve_subject(
        &intent,
        ResolvedStandaloneInstallSubject::ExactRegistrySource(registry_source_subject(&intent)),
    )?;
    let subject_digest = subject.validate()?;
    let dag = dag_for(&intent);
    let outcome = fold_green(&intent, &dag, &subject, &subject_digest)?;
    if outcome.result == TerminalResult::Updated
        && outcome.candidate_disposition == CandidateDisposition::CurrentConfirmed
    {
        Ok(())
    } else {
        violation(
            ContractViolation::OutcomeConflict,
            format!("registry-source update pipeline must update, got {}", outcome.result.as_str()),
        )
    }
}

fn local_development_non_authoritative_pipeline() -> ContractResult<()> {
    let intent = base_intent(InstallMode::ExplicitLocalDevelopment, InstallOperation::Repair);
    let candidate =
        ResolvedStandaloneInstallSubject::ExplicitLocalDevelopment(LocalDevelopmentSubject {
            schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
            subject_id: "subject-localdev-canonical".to_string(),
            description: "developer checkout build; never authoritative".to_string(),
            destination_role: intent.destination_role,
        });
    let subject = resolve_subject(&intent, candidate)?;
    let subject_digest = subject.validate()?;
    let dag = dag_for(&intent);
    let outcome = fold_green(&intent, &dag, &subject, &subject_digest)?;
    // Green evidence cannot make local development an install claim.
    if outcome.result == TerminalResult::NotProven
        && outcome.reason == ReasonFamily::LocalDevelopmentNonAuthoritative
    {
        Ok(())
    } else {
        violation(
            ContractViolation::OutcomeConflict,
            format!(
                "green local-development evidence must stay not_proven, got {}",
                outcome.result.as_str()
            ),
        )
    }
}

fn uninstall_green_pipeline() -> ContractResult<()> {
    let mut intent = base_intent(InstallMode::ExactRegistrySource, InstallOperation::Uninstall);
    intent.requested_product_unit = ProductUnit::ServerOnly;
    let subject = resolve_subject(
        &intent,
        ResolvedStandaloneInstallSubject::ExactRegistrySource(registry_source_subject(&intent)),
    )?;
    let subject_digest = subject.validate()?;
    let dag = dag_for(&intent);
    let outcome = fold_green(&intent, &dag, &subject, &subject_digest)?;
    if outcome.result == TerminalResult::Uninstalled
        && outcome.candidate_disposition == CandidateDisposition::NoneRemaining
    {
        Ok(())
    } else {
        violation(
            ContractViolation::OutcomeConflict,
            format!(
                "uninstall pipeline must terminate uninstalled, got {}",
                outcome.result.as_str()
            ),
        )
    }
}

/// Negative probe: a `latest_requested` intent can name itself but can never
/// mint a resolved subject through the sanctioned seam.
fn latest_requested_never_resolves_probe() -> ContractResult<()> {
    let mut intent = base_intent(InstallMode::ReleaseArchive, InstallOperation::Install);
    intent.selector = ReleaseSelector::latest_requested();
    match resolve_subject(
        &intent,
        ResolvedStandaloneInstallSubject::ReleaseArchive(release_archive_subject(&intent)),
    ) {
        Err(error) if error.code() == ContractViolation::AmbiguousSelector => Ok(()),
        Err(error) => violation(
            ContractViolation::AmbiguousSelector,
            format!("latest_requested must fail ambiguous_selector, got {error}"),
        ),
        Ok(_) => violation(
            ContractViolation::AmbiguousSelector,
            "latest_requested resolved a subject; the two-phase grammar is broken",
        ),
    }
}

/// Positive probe: an admitted fallback creates a genuinely new branch.
fn fallback_branch_probe() -> ContractResult<()> {
    let mut intent = base_intent(InstallMode::ReleaseArchive, InstallOperation::Install);
    intent.fallback_policy = FallbackPolicy::ArchiveToSourceAllowed;
    // The fallback subject below is server-only, so the intent must request
    // that unit: a fallback branch stays bound to the requested product.
    intent.requested_product_unit = ProductUnit::ServerOnly;
    let failed_digest = "ee".repeat(32);
    let new_subject =
        ResolvedStandaloneInstallSubject::ExactRegistrySource(ExactRegistrySourceSubject {
            schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
            subject_id: "subject-fallback-canonical".to_string(),
            registry_id: "crates-io".to_string(),
            package: "perllsp".to_string(),
            version: "0.18.0".to_string(),
            lockfile_digest: None,
            toolchain_policy_id: "toolchain-stable-v1".to_string(),
            target: intent.target.clone(),
            product_unit: ProductUnit::ServerOnly,
            executable_role: MemberRole::PerllspServer,
            destination_role: intent.destination_role,
        });
    let branch = fallback_branch(&intent, &failed_digest, "attempt-2", new_subject)?;
    if branch.prior_subject_digest == failed_digest
        && branch.subject.mode() == InstallMode::ExactRegistrySource
        && branch.new_attempt_id != intent.attempt_id
    {
        Ok(())
    } else {
        violation(
            ContractViolation::FallbackNotAllowed,
            "fallback branch must be a new subject on a new attempt",
        )
    }
}
