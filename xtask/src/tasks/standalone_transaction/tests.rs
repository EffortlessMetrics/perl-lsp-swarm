//! Focused falsifier matrix for the standalone install transaction model
//! (#10243 child 1 / #11099): construction validation, serde fail-closed
//! boundaries, DAG composition rules, fan-in mutation controls, determinism,
//! and privacy.
//!
//! Panic/unwrap/expect are denied workspace-wide; tests fail through a
//! distinct nonzero exit instead.

use super::fixtures::{green_chain, linux_gnu_target, release_archive_subject, succeeded_receipt};
use super::*;

fn fail(message: &str) -> ! {
    eprintln!("test failure: {message}");
    std::process::exit(101)
}

fn must<T>(outcome: ContractResult<T>, what: &str) -> T {
    match outcome {
        Ok(value) => value,
        Err(error) => fail(&format!("{what}: {error}")),
    }
}

fn wire<T: Serialize>(value: &T) -> String {
    match serde_json::to_string(value) {
        Ok(text) => text,
        Err(error) => fail(&format!("serialization failed: {error}")),
    }
}

fn json_of<T: Serialize>(value: &T) -> JsonValue {
    match serde_json::to_value(value) {
        Ok(json) => json,
        Err(error) => fail(&format!("serialization failed: {error}")),
    }
}

fn parse_of<T: for<'de> Deserialize<'de>>(text: &str) -> T {
    match serde_json::from_str(text) {
        Ok(value) => value,
        Err(error) => fail(&format!("own wire must reparse: {error}")),
    }
}

fn base_intent() -> StandaloneInstallIntent {
    StandaloneInstallIntent {
        schema_version: INTENT_SCHEMA_VERSION.to_string(),
        transaction_id: "tx-11099-test".to_string(),
        attempt_id: "attempt-1".to_string(),
        operation: InstallOperation::Install,
        route: RouteMode::FirstPartyPosix,
        mode: InstallMode::ReleaseArchive,
        selector: ReleaseSelector::exact("v0.18.0"),
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

fn archive_dag(unit: ProductUnit) -> StageDag {
    use StageId::*;
    StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: InstallMode::ReleaseArchive,
        product_unit: unit,
        nodes: vec![
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
    }
}

fn source_dag(unit: ProductUnit) -> StageDag {
    use StageId::*;
    StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: InstallMode::ExactRegistrySource,
        product_unit: unit,
        nodes: vec![
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
    }
}

fn local_dag() -> StageDag {
    use StageId::*;
    StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: InstallMode::ExplicitLocalDevelopment,
        product_unit: ProductUnit::ServerOnly,
        nodes: vec![
            node(ResolveSubject, Applicability::Required, &[]),
            node(SourceBuild, Applicability::Required, &[ResolveSubject]),
        ],
    }
}

fn node(stage: StageId, applicability: Applicability, predecessors: &[StageId]) -> StageNode {
    StageNode { stage_id: stage, applicability, predecessors: predecessors.to_vec() }
}

fn resolved_archive(
    intent: &StandaloneInstallIntent,
) -> (ResolvedStandaloneInstallSubject, String) {
    let candidate =
        ResolvedStandaloneInstallSubject::ReleaseArchive(release_archive_subject(intent));
    let subject = must(resolve_subject(intent, candidate), "canonical intent resolves");
    let digest = must(subject.validate(), "canonical subject validates");
    (subject, digest)
}

fn folded(
    intent: &StandaloneInstallIntent,
    dag: &StageDag,
    subject: &ResolvedStandaloneInstallSubject,
    subject_digest: &str,
    mutate: impl FnOnce(&mut Vec<StageReceipt>),
) -> ContractResult<TerminalStandaloneInstallOutcome> {
    let mut receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, subject, subject_digest, dag)?;
    mutate(&mut receipts);
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

#[track_caller]
fn expect_violation(
    outcome: ContractResult<TerminalStandaloneInstallOutcome>,
    code: ContractViolation,
) {
    match outcome {
        Ok(outcome) => fail(&format!(
            "expected {} but fold produced {} at {}",
            code.as_str(),
            outcome.result.as_str(),
            outcome.terminal_stage.as_str()
        )),
        Err(error) => assert_eq!(error.code(), code, "expected {}, got {error}", code.as_str()),
    }
}

#[track_caller]
fn expect_code<T>(outcome: ContractResult<T>, code: ContractViolation) {
    match outcome {
        Ok(_) => fail(&format!("expected {} but validation passed", code.as_str())),
        Err(error) => assert_eq!(error.code(), code, "expected {}, got {error}", code.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Construction validation matrix
// ---------------------------------------------------------------------------

#[test]
fn canonical_intent_and_subject_validate_and_bind_distinct_domains() {
    let intent = base_intent();
    let intent_digest = must(intent.validate(), "intent validates");
    assert_eq!(intent_digest.len(), 64, "intent digest is 64-hex");
    assert!(intent.authorizes_artifact_work());
    let (_, subject_digest) = resolved_archive(&intent);
    assert_ne!(intent_digest, subject_digest, "domains must separate identities");
}

#[test]
fn every_field_of_the_intent_is_digest_load_bearing() {
    let base = base_intent();
    let baseline = must(base.validate(), "baseline");
    let mutations: Vec<(&str, StandaloneInstallIntent)> = vec![
        (
            "transaction_id",
            StandaloneInstallIntent { transaction_id: "tx-other".into(), ..base.clone() },
        ),
        ("attempt_id", StandaloneInstallIntent { attempt_id: "attempt-2".into(), ..base.clone() }),
        (
            "operation",
            StandaloneInstallIntent { operation: InstallOperation::Repair, ..base.clone() },
        ),
        (
            "route",
            StandaloneInstallIntent { route: RouteMode::FirstPartyPowershell, ..base.clone() },
        ),
        (
            "mode",
            StandaloneInstallIntent { mode: InstallMode::ExactRegistrySource, ..base.clone() },
        ),
        (
            "selector",
            StandaloneInstallIntent { selector: ReleaseSelector::exact("v0.19.0"), ..base.clone() },
        ),
        (
            "target",
            StandaloneInstallIntent {
                target: TargetIdentity {
                    platform: Platform::Windows,
                    triple: "x86_64-pc-windows-msvc".into(),
                    libc: LibcDisposition::Msvc,
                },
                ..base.clone()
            },
        ),
        (
            "product_unit",
            StandaloneInstallIntent {
                requested_product_unit: ProductUnit::ServerOnly,
                ..base.clone()
            },
        ),
        (
            "destination_role",
            StandaloneInstallIntent {
                destination_role: DestinationRole::SystemShared,
                ..base.clone()
            },
        ),
        (
            "path_policy",
            StandaloneInstallIntent { path_policy: PathPolicy::SessionOnly, ..base.clone() },
        ),
        (
            "fallback_policy",
            StandaloneInstallIntent {
                fallback_policy: FallbackPolicy::ArchiveToSourceAllowed,
                ..base.clone()
            },
        ),
        (
            "config_digest",
            StandaloneInstallIntent { trusted_config_digest: "bb".repeat(32), ..base.clone() },
        ),
        (
            "policy_version",
            StandaloneInstallIntent { policy_version: "policy-v2".into(), ..base.clone() },
        ),
        ("contract_generation", StandaloneInstallIntent { contract_generation: 2, ..base.clone() }),
        (
            "target_override",
            StandaloneInstallIntent {
                target_override: Some(TargetOverride {
                    triple: "aarch64-pc-windows-msvc".into(),
                    authority: "operator-request".into(),
                }),
                ..base
            },
        ),
    ];
    for (field, mutated) in mutations {
        // Mutations may break semantic validity; identity sensitivity only
        // requires the digest to differ whenever the mutated form validates.
        if let Ok(digest) = mutated.validate() {
            assert_ne!(digest, baseline, "{field} failed to change the intent digest");
        }
    }
}

#[test]
fn malformed_intents_fail_closed() {
    let wrong_schema = StandaloneInstallIntent {
        schema_version: "standalone_install_intent.v2".into(),
        ..base_intent()
    };
    expect_code(wrong_schema.validate(), ContractViolation::UnknownSchemaVersion);

    let empty_transaction =
        StandaloneInstallIntent { transaction_id: String::new(), ..base_intent() };
    expect_code(empty_transaction.validate(), ContractViolation::MalformedDocument);

    let bad_config =
        StandaloneInstallIntent { trusted_config_digest: "xyz".into(), ..base_intent() };
    expect_code(bad_config.validate(), ContractViolation::MalformedDocument);

    let zero_generation = StandaloneInstallIntent { contract_generation: 0, ..base_intent() };
    expect_code(zero_generation.validate(), ContractViolation::MalformedDocument);

    let mut control_chars = base_intent();
    control_chars.policy_version = "policy\u{7}v1".into();
    expect_code(control_chars.validate(), ContractViolation::MalformedDocument);

    let archive_without_selector =
        StandaloneInstallIntent { selector: ReleaseSelector::not_applicable(), ..base_intent() };
    expect_code(archive_without_selector.validate(), ContractViolation::AmbiguousSelector);

    let latest_uninstall = StandaloneInstallIntent {
        operation: InstallOperation::Uninstall,
        selector: ReleaseSelector::latest_requested(),
        ..base_intent()
    };
    expect_code(latest_uninstall.validate(), ContractViolation::AmbiguousSelector);

    let exact_without_tag = StandaloneInstallIntent {
        selector: ReleaseSelector { kind: SelectorKind::Exact, tag: None },
        ..base_intent()
    };
    expect_code(exact_without_tag.validate(), ContractViolation::MalformedDocument);

    let latest_with_tag = StandaloneInstallIntent {
        selector: ReleaseSelector { kind: SelectorKind::LatestRequested, tag: Some("v1".into()) },
        ..base_intent()
    };
    expect_code(latest_with_tag.validate(), ContractViolation::MalformedDocument);
}

#[test]
fn glibc_never_applies_to_windows_or_macos_targets() {
    let mut intent = base_intent();
    intent.target = TargetIdentity {
        platform: Platform::Windows,
        triple: "x86_64-pc-windows-gnuish".into(),
        libc: LibcDisposition::Gnu,
    };
    expect_code(intent.validate(), ContractViolation::IncoherentTargetIdentity);

    intent.target = TargetIdentity {
        platform: Platform::Macos,
        triple: "aarch64-apple-darwin".into(),
        libc: LibcDisposition::Musl,
    };
    expect_code(intent.validate(), ContractViolation::IncoherentTargetIdentity);

    // NoneLibc is the macOS disposition only; Linux and Windows targets must
    // name their libc.
    for platform in [Platform::Linux, Platform::Windows] {
        intent.target = TargetIdentity {
            platform,
            triple: "x86_64-unknown-none".into(),
            libc: LibcDisposition::NoneLibc,
        };
        expect_code(intent.validate(), ContractViolation::IncoherentTargetIdentity);
    }
}

#[test]
fn unresolved_latest_selector_can_never_mint_a_subject() {
    let mut intent = base_intent();
    intent.selector = ReleaseSelector::latest_requested();
    assert!(!intent.authorizes_artifact_work());
    let candidate =
        ResolvedStandaloneInstallSubject::ReleaseArchive(release_archive_subject(&intent));
    let error = resolve_subject(&intent, candidate)
        .err()
        .unwrap_or_else(|| fail("latest_requested cannot authorize artifact work"));
    assert_eq!(error.code(), ContractViolation::AmbiguousSelector);
}

#[test]
fn resolver_seam_rejects_mode_selector_and_identity_drift() {
    let intent = base_intent();

    // Mode change: archive intent cannot yield a registry-source subject.
    expect_code(
        resolve_subject(&intent, registry_candidate(&intent)).map(|_| ()),
        ContractViolation::ModeMismatch,
    );

    // Tag drift: resolved tag disagrees with the exact selector.
    let mut drifted = release_archive_subject(&intent);
    drifted.tag = "v0.17.0".into();
    expect_code(
        resolve_subject(&intent, ResolvedStandaloneInstallSubject::ReleaseArchive(drifted))
            .map(|_| ()),
        ContractViolation::SelectorSubjectMismatch,
    );

    // Product-unit drift.
    let mut narrowed = release_archive_subject(&intent);
    narrowed.product_unit = ProductUnit::ServerOnly;
    narrowed.expected_members = canonical_members(ProductUnit::ServerOnly);
    expect_code(
        resolve_subject(&intent, ResolvedStandaloneInstallSubject::ReleaseArchive(narrowed))
            .map(|_| ()),
        ContractViolation::OutcomeConflict,
    );

    // Destination-role drift.
    let mut moved = release_archive_subject(&intent);
    moved.destination_role = DestinationRole::SystemShared;
    expect_code(
        resolve_subject(&intent, ResolvedStandaloneInstallSubject::ReleaseArchive(moved))
            .map(|_| ()),
        ContractViolation::OutcomeConflict,
    );
    // Target drift: the subject must stay on the intent's platform/triple/libc.
    let mut target_drift = release_archive_subject(&intent);
    target_drift.target.platform = Platform::Windows;
    target_drift.target.triple = "x86_64-pc-windows-msvc".into();
    target_drift.target.libc = LibcDisposition::Msvc;
    expect_code(
        resolve_subject(&intent, ResolvedStandaloneInstallSubject::ReleaseArchive(target_drift))
            .map(|_| ()),
        ContractViolation::OutcomeConflict,
    );
}

fn canonical_members(unit: ProductUnit) -> Vec<MemberIdentity> {
    match unit {
        ProductUnit::ServerOnly => vec![MemberIdentity {
            role: MemberRole::PerllspServer,
            artifact_name: "perllsp".into(),
        }],
        ProductUnit::ServerDapPair => vec![
            MemberIdentity { role: MemberRole::PerllspServer, artifact_name: "perllsp".into() },
            MemberIdentity {
                role: MemberRole::PerlDapAdapter,
                artifact_name: "perl-dap-adapter".into(),
            },
        ],
    }
}

/// Registry-source subjects bind the same product-unit/destination-role
/// coherence the archive arm enforces: a resolver cannot widen the requested
/// unit or move the install root while the mode stays exact.
#[test]
fn registry_source_resolver_drift_fails_coherence() {
    // Registry-source intent requesting the full pair; both mutations below
    // keep the mode exact and fail on coherence alone.
    let mut intent = base_intent();
    intent.mode = InstallMode::ExactRegistrySource;
    intent.selector = ReleaseSelector::not_applicable();

    // Product-unit drift: pair requested, server-only resolved.
    let narrowed = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.product_unit = ProductUnit::ServerOnly;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(resolve_subject(&intent, narrowed).map(|_| ()), ContractViolation::OutcomeConflict);

    // Destination-role drift.
    let moved = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.destination_role = DestinationRole::SystemShared;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(resolve_subject(&intent, moved).map(|_| ()), ContractViolation::OutcomeConflict);

    // The coherent registry candidate still resolves under a matching
    // server-only intent.
    let mut matching = intent.clone();
    matching.requested_product_unit = ProductUnit::ServerOnly;
    let coherent =
        must(resolve_subject(&matching, registry_candidate(&matching)), "coherent registry");
    assert_eq!(coherent.mode(), InstallMode::ExactRegistrySource);
}

fn registry_candidate(intent: &StandaloneInstallIntent) -> ResolvedStandaloneInstallSubject {
    ResolvedStandaloneInstallSubject::ExactRegistrySource(ExactRegistrySourceSubject {
        schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
        subject_id: "subject-source-test".into(),
        registry_id: "crates-io".into(),
        package: "perllsp".into(),
        version: "0.18.0".into(),
        lockfile_digest: Some("cc".repeat(32)),
        toolchain_policy_id: "toolchain-stable-v1".into(),
        target: intent.target.clone(),
        product_unit: ProductUnit::ServerOnly,
        executable_role: MemberRole::PerllspServer,
        destination_role: intent.destination_role,
    })
}

#[test]
fn malformed_subjects_fail_closed() {
    let intent = base_intent();
    let mutated = |mutate: &dyn Fn(&mut ReleaseArchiveSubject)| {
        let mut subject = release_archive_subject(&intent);
        mutate(&mut subject);
        ResolvedStandaloneInstallSubject::ReleaseArchive(subject)
    };

    let missing_topology = mutated(&|subject| subject.topology_digest = "nothex".into());
    expect_code(missing_topology.validate().map(|_| ()), ContractViolation::MalformedDocument);

    let pair_missing_dap =
        mutated(&|subject| subject.expected_members = canonical_members(ProductUnit::ServerOnly));
    expect_code(pair_missing_dap.validate().map(|_| ()), ContractViolation::SubjectIncomplete);

    let server_only_unit = mutated(&|subject| subject.product_unit = ProductUnit::ServerOnly);
    expect_code(server_only_unit.validate().map(|_| ()), ContractViolation::SubjectIncomplete);

    let duplicate_role = mutated(&|subject| {
        subject.expected_members.push(MemberIdentity {
            role: MemberRole::PerlDapAdapter,
            artifact_name: "perl-dap-adapter-again".into(),
        });
    });
    expect_code(duplicate_role.validate().map(|_| ()), ContractViolation::DuplicateMemberRole);

    let path_smuggled_member =
        mutated(&|subject| subject.expected_members[0].artifact_name = "/usr/bin/perllsp".into());
    expect_code(path_smuggled_member.validate().map(|_| ()), ContractViolation::MalformedDocument);

    let path_smuggled_archive =
        mutated(&|subject| subject.archive_name = "dist/perllsp.tar.gz".into());
    expect_code(path_smuggled_archive.validate().map(|_| ()), ContractViolation::MalformedDocument);

    let bad_repo = mutated(&|subject| subject.repository = "EffortlessMetrics".into());
    expect_code(bad_repo.validate().map(|_| ()), ContractViolation::MalformedDocument);

    let wrong_subject_schema =
        mutated(&|subject| subject.schema_version = "standalone_install_subject.v2".into());
    expect_code(
        wrong_subject_schema.validate().map(|_| ()),
        ContractViolation::UnknownSchemaVersion,
    );

    // Registry-source subjects validate their own closed shape.
    let good_registry = registry_candidate(&intent);
    assert!(good_registry.validate().is_ok(), "registry candidate validates");

    let bad_lockfile = match good_registry.clone() {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.lockfile_digest = Some("zzz".into());
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(bad_lockfile.validate().map(|_| ()), ContractViolation::MalformedDocument);

    let smuggled_package = match good_registry {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.package = "../etc/perllsp".into();
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(smuggled_package.validate().map(|_| ()), ContractViolation::MalformedDocument);

    // One artifact cannot satisfy two roles: the pair requires distinct
    // server and adapter binaries.
    let shared_artifact = mutated(&|subject| {
        subject.expected_members = vec![
            MemberIdentity { role: MemberRole::PerllspServer, artifact_name: "perllsp".into() },
            MemberIdentity { role: MemberRole::PerlDapAdapter, artifact_name: "perllsp".into() },
        ];
    });
    expect_code(shared_artifact.validate().map(|_| ()), ContractViolation::SubjectIncomplete);

    // A server-only registry subject must name the server executable, and a
    // single registry executable identity can never certify the pair.
    let adapter_only = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.executable_role = MemberRole::PerlDapAdapter;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(adapter_only.validate().map(|_| ()), ContractViolation::SubjectIncomplete);

    let underspecified_pair = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.product_unit = ProductUnit::ServerDapPair;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(underspecified_pair.validate().map(|_| ()), ContractViolation::SubjectIncomplete);
}

#[test]
fn fallback_requires_explicit_admission_and_creates_a_new_branch() {
    let forbidden_intent = base_intent();
    let (_, failed_digest) = resolved_archive(&forbidden_intent);
    let new_subject = registry_candidate(&forbidden_intent);
    expect_code(
        fallback_branch(&forbidden_intent, &failed_digest, "attempt-2", new_subject.clone())
            .map(|_| ()),
        ContractViolation::FallbackNotAllowed,
    );

    let mut admitted = base_intent();
    admitted.fallback_policy = FallbackPolicy::ArchiveToSourceAllowed;
    // The registry fallback candidate is server-only; the intent must request
    // that unit because a fallback branch stays bound to the trusted intent.
    admitted.requested_product_unit = ProductUnit::ServerOnly;
    expect_code(
        fallback_branch(&admitted, &failed_digest, "attempt-1", new_subject).map(|_| ()),
        ContractViolation::AttemptMismatch,
    );

    let retry_candidate = registry_candidate(&admitted);
    let branch = must(
        fallback_branch(&admitted, &failed_digest, "attempt-2", retry_candidate),
        "admitted fallback creates a branch",
    );
    assert_eq!(branch.prior_subject_digest, failed_digest);
    assert_eq!(branch.subject.mode(), InstallMode::ExactRegistrySource);
    let new_digest = must(branch.subject.validate(), "fallback subject validates");
    assert_ne!(new_digest, failed_digest, "fallback resolves a NEW subject");

    // Non-archive-to-source transitions are never modelled as fallback.
    expect_code(
        fallback_branch(
            &admitted,
            &failed_digest,
            "attempt-2",
            ResolvedStandaloneInstallSubject::ExplicitLocalDevelopment(LocalDevelopmentSubject {
                schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
                subject_id: "localdev".into(),
                description: "non-authoritative".into(),
                destination_role: DestinationRole::UserLocal,
            }),
        )
        .map(|_| ()),
        ContractViolation::FallbackNotAllowed,
    );
}

/// A fallback branch admits only the archive→registry-source mode
/// transition: product unit, destination, and target identity stay bound to
/// the trusted intent, or a fallback would install different software in a
/// different place.
#[test]
fn fallback_branch_cannot_drift_from_the_intent() {
    let mut intent = base_intent();
    intent.fallback_policy = FallbackPolicy::ArchiveToSourceAllowed;
    intent.requested_product_unit = ProductUnit::ServerOnly;
    let failed_digest = "ee".repeat(32);

    // Positive control: the coherent branch is admitted.
    must(
        fallback_branch(&intent, &failed_digest, "attempt-2", registry_candidate(&intent)),
        "coherent fallback branch",
    );

    // Product-unit drift.
    let widened = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.product_unit = ProductUnit::ServerDapPair;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(
        fallback_branch(&intent, &failed_digest, "attempt-2", widened).map(|_| ()),
        ContractViolation::OutcomeConflict,
    );

    // Destination drift.
    let moved = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.destination_role = DestinationRole::SystemShared;
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(
        fallback_branch(&intent, &failed_digest, "attempt-2", moved).map(|_| ()),
        ContractViolation::OutcomeConflict,
    );

    // Target drift.
    let retargeted = match registry_candidate(&intent) {
        ResolvedStandaloneInstallSubject::ExactRegistrySource(mut subject) => {
            subject.target = TargetIdentity {
                platform: Platform::Windows,
                triple: "x86_64-pc-windows-msvc".into(),
                libc: LibcDisposition::Msvc,
            };
            ResolvedStandaloneInstallSubject::ExactRegistrySource(subject)
        }
        other => other,
    };
    expect_code(
        fallback_branch(&intent, &failed_digest, "attempt-2", retargeted).map(|_| ()),
        ContractViolation::OutcomeConflict,
    );
}

// ---------------------------------------------------------------------------
// Serde boundary
// ---------------------------------------------------------------------------

#[test]
fn unknown_fields_are_rejected_on_every_closed_type() {
    let mut value: JsonValue = parse_of(wire(&base_intent()).as_str());
    value["sneaky_field"] = serde_json::json!(1);
    let parsed: std::result::Result<StandaloneInstallIntent, _> =
        serde_json::from_str(value.to_string().as_str());
    assert!(parsed.is_err(), "deny_unknown_fields must reject sneaky intent fields");

    let (_, subject_digest) = resolved_archive(&base_intent());
    let receipt = succeeded_receipt("tx", "attempt-1", &subject_digest, StageId::Transport, &[]);
    let mut value: JsonValue = parse_of(wire(&receipt).as_str());
    // A producer-declared completeness flag does not even exist on the type.
    value["complete"] = serde_json::json!(true);
    let parsed: std::result::Result<StageReceipt, _> =
        serde_json::from_str(value.to_string().as_str());
    assert!(parsed.is_err(), "producer completeness fields must be rejected");

    let dag = archive_dag(ProductUnit::ServerDapPair);
    let mut value = json_of(&dag);
    value["extra"] = serde_json::json!("junk");
    let parsed: std::result::Result<StageDag, _> = serde_json::from_value(value);
    assert!(parsed.is_err(), "deny_unknown_fields must reject sneaky DAG fields");

    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    let outcome = must(
        fold_terminal_outcome(FanInInput {
            dag: &dag,
            operation: intent.operation,
            mode: intent.mode,
            transaction_id: &intent.transaction_id,
            attempt_id: &intent.attempt_id,
            subject: &subject,
            subject_digest: &subject_digest,
            receipts: &receipts,
        }),
        "fold",
    );
    let mut value = json_of(&outcome);
    value["installed_ok"] = serde_json::json!(true);
    let parsed: std::result::Result<TerminalStandaloneInstallOutcome, _> =
        serde_json::from_value(value);
    assert!(parsed.is_err(), "deny_unknown_fields must reject sneaky outcome fields");
}

#[test]
fn unknown_enum_values_fail_at_the_serde_boundary() {
    let mut value: JsonValue = parse_of(wire(&base_intent()).as_str());
    value["mode"] = serde_json::json!("mystery_mode");
    let parsed: std::result::Result<StandaloneInstallIntent, _> = serde_json::from_value(value);
    assert!(parsed.is_err(), "closed vocabularies reject unknown values");

    assert_eq!(StageId::parse("checksum_integrity"), Some(StageId::ChecksumIntegrity));
    assert_eq!(StageId::parse("definitely_not_a_stage"), None, "unknown stages fail closed");
    assert_eq!(TerminalResult::parse("installed"), Some(TerminalResult::Installed));
}

#[test]
fn full_packet_round_trip_is_byte_stable_under_key_permutation() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    let outcome = must(
        fold_terminal_outcome(FanInInput {
            dag: &dag,
            operation: intent.operation,
            mode: intent.mode,
            transaction_id: &intent.transaction_id,
            attempt_id: &intent.attempt_id,
            subject: &subject,
            subject_digest: &subject_digest,
            receipts: &receipts,
        }),
        "fold",
    );

    fn reversed_order_json(value: &JsonValue) -> String {
        match value {
            JsonValue::Object(map) => {
                let members: Vec<String> = map
                    .iter()
                    .rev()
                    .map(|(key, item)| {
                        format!(
                            "{}:{}",
                            serde_json::to_string(key).unwrap_or_default(),
                            reversed_order_json(item)
                        )
                    })
                    .collect();
                format!("{{{}}}", members.join(","))
            }
            JsonValue::Array(items) => {
                let items: Vec<String> = items.iter().map(reversed_order_json).collect();
                format!("[{}]", items.join(","))
            }
            other => serde_json::to_string(other).unwrap_or_default(),
        }
    }

    for document in [json_of(&intent), json_of(&receipts[0]), json_of(&dag), json_of(&outcome)] {
        let first = canonical_json(&document);
        let permuted_text = reversed_order_json(&document);
        let permuted: JsonValue = match serde_json::from_str(permuted_text.as_str()) {
            Ok(parsed) => parsed,
            Err(error) => fail(&format!("permutation must reparse: {error}")),
        };
        assert_eq!(
            first,
            canonical_json(&permuted),
            "canonical bytes must not depend on input key order"
        );
    }

    let reparsed: TerminalStandaloneInstallOutcome =
        match serde_json::from_str(wire(&outcome).as_str()) {
            Ok(parsed) => parsed,
            Err(error) => fail(&format!("round trip failed: {error}")),
        };
    assert_eq!(reparsed, outcome);
}

// ---------------------------------------------------------------------------
// DAG validation falsifiers
// ---------------------------------------------------------------------------

#[test]
fn dag_structure_falsifiers() {
    use StageId::*;

    let mut duplicate = archive_dag(ProductUnit::ServerDapPair);
    duplicate.nodes.push(node(Transport, Applicability::Required, &[ResolveSubject]));
    expect_code(duplicate.validate(), ContractViolation::DuplicateStageNode);

    let mut unknown_predecessor = archive_dag(ProductUnit::ServerDapPair);
    unknown_predecessor.nodes[1].predecessors.push(Uninstall);
    expect_code(unknown_predecessor.validate(), ContractViolation::UnknownPredecessor);

    // Reversed DAG: the complete canonical floor declared in reverse order,
    // so promotion is cited before it appears.
    let mut reversed = archive_dag(ProductUnit::ServerOnly);
    reversed.nodes.reverse();
    expect_code(reversed.validate(), ContractViolation::CyclicStageGraph);

    // Direct cycle on top of the complete floor: transport and checksum cite
    // each other.
    let mut cyclic = archive_dag(ProductUnit::ServerOnly);
    cyclic.nodes[1].predecessors.push(ChecksumIntegrity);
    expect_code(cyclic.validate(), ContractViolation::CyclicStageGraph);

    // Source builds are forbidden outright in archive mode: the stage cannot
    // even be declared, in either applicability.
    let mut source_build_in_archive_mode = archive_dag(ProductUnit::ServerOnly);
    source_build_in_archive_mode.nodes.push(node(
        SourceBuild,
        Applicability::NotApplicable,
        &[ResolveSubject],
    ));
    expect_code(
        source_build_in_archive_mode.validate(),
        ContractViolation::UnauthorizedStageApplicability,
    );

    // Archive staging cannot appear in exact-source mode at all.
    let mut archive_stage_in_source_mode = source_dag(ProductUnit::ServerOnly);
    archive_stage_in_source_mode.nodes.push(node(
        ArchiveManifestAndStaging,
        Applicability::Required,
        &[ResolveSubject],
    ));
    expect_code(
        archive_stage_in_source_mode.validate(),
        ContractViolation::UnauthorizedStageApplicability,
    );

    // Local development cannot declare promotion stages at all.
    let mut local_with_promotion = local_dag();
    local_with_promotion.nodes.push(node(Promotion, Applicability::Required, &[ResolveSubject]));
    expect_code(local_with_promotion.validate(), ContractViolation::UnauthorizedStageApplicability);

    let mut wrong_schema = source_dag(ProductUnit::ServerOnly);
    wrong_schema.schema_version = "standalone_stage_dag.v2".into();
    expect_code(wrong_schema.validate(), ContractViolation::UnknownSchemaVersion);
}

/// Canonical composition floor (#11099): promotion is mandatory outside local
/// development, and PATH persistence / fresh-process observation / installed
/// transition can only follow the promoted candidate. Each mutation below is
/// structurally self-consistent and passes the per-stage authorization map;
/// only the floor rejects it.
#[test]
fn canonical_dag_floor_rejects_promotion_less_and_unordered_graphs() {
    use StageId::*;

    // A promotion-less archive DAG is structurally valid but can never reach
    // installed: the floor must reject it outright.
    let promotionless = StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: InstallMode::ReleaseArchive,
        product_unit: ProductUnit::ServerDapPair,
        nodes: vec![
            node(ResolveSubject, Applicability::Required, &[]),
            node(Transport, Applicability::Required, &[ResolveSubject]),
            node(ChecksumIntegrity, Applicability::Required, &[Transport]),
            node(ArchiveManifestAndStaging, Applicability::Required, &[ChecksumIntegrity]),
            node(ExecutableObservation, Applicability::Required, &[ArchiveManifestAndStaging]),
            node(PathPersistence, Applicability::Required, &[ExecutableObservation]),
            node(FreshProcessObservation, Applicability::Required, &[ExecutableObservation]),
            node(
                InstalledTransition,
                Applicability::Required,
                &[PathPersistence, FreshProcessObservation],
            ),
        ],
    };
    expect_code(promotionless.validate(), ContractViolation::MissingRequiredStage);

    // Declaring promotion as skippable is equally fatal: no transaction may
    // authorize an install claim without promoting.
    let mut skipped_promotion = archive_dag(ProductUnit::ServerDapPair);
    if let Some(promotion) =
        skipped_promotion.nodes.iter_mut().find(|node| node.stage_id == StageId::Promotion)
    {
        promotion.applicability = Applicability::NotApplicable;
    }
    expect_code(skipped_promotion.validate(), ContractViolation::MissingRequiredStage);

    // Promotion with no predecessors opens a transaction from nothing.
    let mut rootless_promotion = archive_dag(ProductUnit::ServerDapPair);
    if let Some(promotion) =
        rootless_promotion.nodes.iter_mut().find(|node| node.stage_id == StageId::Promotion)
    {
        promotion.predecessors.clear();
    }
    expect_code(rootless_promotion.validate(), ContractViolation::PredecessorMismatch);

    // Post-promotion stages with empty predecessor sets: PATH persistence,
    // fresh-process observation, and the installed transition would fold
    // green without any promoted candidate behind them.
    let mut unordered = archive_dag(ProductUnit::ServerDapPair);
    for stage in [StageId::PathPersistence, StageId::FreshProcessObservation] {
        if let Some(node) = unordered.nodes.iter_mut().find(|node| node.stage_id == stage) {
            node.predecessors.clear();
        }
    }
    // InstalledTransition still cites the two mutated stages, so the graph
    // stays connected; only the promotion edge requirement is violated.
    expect_code(unordered.validate(), ContractViolation::PredecessorMismatch);

    // The same floor binds exact-registry-source mode: dropping promotion's
    // downstream stages cannot yield an installed transition either. The
    // dangling citation is repaired too, so the graph stays structurally
    // self-consistent and only the floor rejects it.
    let mut truncated_source = source_dag(ProductUnit::ServerOnly);
    truncated_source.nodes.retain(|node| node.stage_id != StageId::FreshProcessObservation);
    if let Some(transition) =
        truncated_source.nodes.iter_mut().find(|node| node.stage_id == StageId::InstalledTransition)
    {
        transition
            .predecessors
            .retain(|predecessor| *predecessor != StageId::FreshProcessObservation);
    }
    expect_code(truncated_source.validate(), ContractViolation::MissingRequiredStage);

    // Local development remains exempt: its non-authoritative graph never
    // promotes and still validates.
    must(local_dag().validate(), "local development graph stays exempt from the floor");
}

// ---------------------------------------------------------------------------
// Fan-in composition falsifiers
// ---------------------------------------------------------------------------

#[test]
fn mixed_subject_receipt_is_rejected_even_with_wellformed_bytes() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let other_subject = "ff".repeat(32);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.get_mut(1) {
                // Transport claims success against another well-formed
                // subject identity; matching shapes do not merge subjects.
                receipt.subject_digest = other_subject;
            }
        }),
        ContractViolation::SubjectDigestMismatch,
    );
}

#[test]
fn stale_attempt_receipt_cannot_compose() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.get_mut(2) {
                receipt.attempt_id = "attempt-0-stale".into();
            }
        }),
        ContractViolation::AttemptMismatch,
    );
}

#[test]
fn foreign_transaction_receipt_cannot_compose() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.first_mut() {
                receipt.transaction_id = "tx-someone-else".into();
            }
        }),
        ContractViolation::TransactionMismatch,
    );
}

#[test]
fn duplicate_stage_results_are_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            // Insert the duplicate adjacent to its original: appending it to
            // the tail would trip the out-of-order rule first and stop
            // discriminating the duplicate-result rule under test.
            let clone = receipts[3].clone();
            receipts.insert(4, clone);
        }),
        ContractViolation::DuplicateStageResult,
    );
}

#[test]
fn fabricated_predecessor_chain_is_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            // Checksum cites a well-formed digest from some other chain.
            if let Some(receipt) = receipts.get_mut(2) {
                receipt.predecessor_receipt_digests = vec!["ab".repeat(32)];
            }
        }),
        ContractViolation::PredecessorMismatch,
    );
}

#[test]
fn missing_required_stage_evidence_fails_closed() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);

    // Removing a terminal required stage leaves a silent coverage hole.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            receipts.retain(|receipt| receipt.stage_id != StageId::InstalledTransition);
        }),
        ContractViolation::MissingRequiredStage,
    );

    // Removing a mid-chain stage surfaces first as a broken citation chain:
    // every dependent cites evidence that no longer exists.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            receipts.retain(|receipt| receipt.stage_id != StageId::Promotion);
        }),
        ContractViolation::PredecessorMismatch,
    );
}

/// The fold composes evidence for exactly one installation shape: the settled
/// subject's mode and product unit must equal the DAG's, or evidence gathered
/// for one installation could certify another.
#[test]
fn fold_rejects_subject_dag_shape_mismatch() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);

    // Product mismatch: a server+ DAP pair subject folded against a
    // server-only DAG.
    let wrong_unit_dag = archive_dag(ProductUnit::ServerOnly);
    expect_violation(
        folded(&intent, &wrong_unit_dag, &subject, &subject_digest, |_| {}),
        ContractViolation::OutcomeConflict,
    );

    // Mode mismatch: a registry-source subject folded against an archive DAG.
    let mut source_intent = base_intent();
    source_intent.mode = InstallMode::ExactRegistrySource;
    source_intent.selector = ReleaseSelector::not_applicable();
    source_intent.requested_product_unit = ProductUnit::ServerOnly;
    let source_subject = must(
        resolve_subject(&source_intent, registry_candidate(&source_intent)),
        "registry subject resolves",
    );
    let source_digest = must(source_subject.validate(), "registry subject validates");
    let archive_dag = archive_dag(ProductUnit::ServerOnly);
    expect_violation(
        folded(&intent, &archive_dag, &source_subject, &source_digest, |_| {}),
        ContractViolation::ModeMismatch,
    );
}

/// A stage the settled subject binds policy-mandated evidence to can never
/// fold as not_applicable: DAG authorization alone cannot waive mandated
/// evidence (negative control: missing mandatory stage as skip).
#[test]
fn subject_mandated_evidence_cannot_be_skipped() {
    let intent = base_intent();
    let mut mandated = release_archive_subject(&intent);
    mandated.provenance_policy_id = Some("provenance-v1".into());
    let subject = must(
        resolve_subject(&intent, ResolvedStandaloneInstallSubject::ReleaseArchive(mandated)),
        "provenance-mandated subject resolves",
    );
    let subject_digest = must(subject.validate(), "subject validates");
    // The canonical DAG marks provenance skippable; the settled subject
    // mandates provenance evidence, so the skip can no longer fold green.
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |_| {}),
        ContractViolation::MissingRequiredStage,
    );
}

#[test]
fn required_stage_claiming_not_applicable_is_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.get_mut(4) {
                // Required stage claims a skip; a skip would also have to
                // cite no predecessors, so both shapes are wrong here.
                receipt.result = StageResult::NotApplicable;
            }
        }),
        ContractViolation::UnauthorizedStageApplicability,
    );
}

#[test]
fn skipped_stage_citing_predecessor_evidence_is_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            // The authorized provenance skip fabricates a predecessor
            // citation: skips consume nothing and must cite nothing.
            if let Some(receipt) = receipts.get_mut(3) {
                receipt.predecessor_receipt_digests = vec!["ab".repeat(32)];
            }
        }),
        ContractViolation::PredecessorMismatch,
    );
}

/// Rewrite each receipt's predecessor citations from the mutated receipts
/// themselves, exactly as an honest composer would, so downstream tests
/// isolate the rule under test from citation drift.
fn rechain(dag: &StageDag, receipts: &mut [StageReceipt]) {
    let mut chain: Vec<(StageId, String)> = Vec::new();
    for receipt in receipts.iter_mut() {
        if receipt.result == StageResult::NotApplicable {
            // Skips consume nothing and cite nothing.
            receipt.predecessor_receipt_digests.clear();
        } else if let Some(node) = dag.nodes.iter().find(|node| node.stage_id == receipt.stage_id) {
            receipt.predecessor_receipt_digests = node
                .predecessors
                .iter()
                .filter_map(|stage| chain.iter().find(|(done, _)| done == stage))
                .map(|(_, digest)| digest.clone())
                .collect();
        } else {
            receipt.predecessor_receipt_digests.clear();
        }
        if let Ok(digest) = receipt.validate() {
            chain.push((receipt.stage_id, digest));
        }
    }
}

#[test]
fn success_after_cancelled_evidence_is_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.get_mut(4) {
                receipt.result = StageResult::Cancelled;
                receipt.reason = ReasonFamily::Cancelled;
                receipt.next_action = ActionClass::AbortInstall;
            }
            // Honest downstream citations: the violation under test is
            // success continuing after terminal evidence, not drift.
            rechain(&dag, receipts);
        }),
        ContractViolation::SuccessAfterTerminalEvidence,
    );
}

#[test]
fn producer_declared_completeness_is_never_authority() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts.get_mut(5) {
                // Claims success while its instrument only partially observed.
                receipt.instrument_completeness = InstrumentCompleteness::Partial;
            }
        }),
        ContractViolation::InstrumentIncompleteSuccess,
    );
}

#[test]
fn failure_blocks_downstream_and_names_the_reason() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let outcome = folded(&intent, &dag, &subject, &subject_digest, |receipts| {
        if let Some(receipt) = receipts.get_mut(2) {
            receipt.result = StageResult::Failed;
            receipt.reason = ReasonFamily::IntegrityFailed;
            receipt.next_action = ActionClass::VerifyEnvironmentThenRetry;
        }
        receipts.truncate(3);
    })
    .unwrap_or_else(|error| fail(&format!("terminal failure still folds: {error}")));
    assert_eq!(outcome.result, TerminalResult::Failed);
    assert_eq!(outcome.reason, ReasonFamily::IntegrityFailed);
    assert_eq!(outcome.terminal_stage, StageId::ChecksumIntegrity);
    assert_eq!(outcome.side_effect_ceiling, SideEffectCeiling::TransportArtifacts);
}

#[test]
fn timeout_and_not_proven_terminate_distinctly() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);

    let timed_out = folded(&intent, &dag, &subject, &subject_digest, |receipts| {
        receipts.truncate(3);
        if let Some(receipt) = receipts.last_mut() {
            receipt.result = StageResult::TimedOut;
            receipt.reason = ReasonFamily::Timeout;
            receipt.next_action = ActionClass::RetryNewAttempt;
        }
    })
    .unwrap_or_else(|error| fail(&format!("timeout folds: {error}")));
    assert_eq!(timed_out.result, TerminalResult::TimedOut);

    let unproven = folded(&intent, &dag, &subject, &subject_digest, |receipts| {
        receipts.truncate(6);
        if let Some(receipt) = receipts.last_mut() {
            receipt.result = StageResult::NotProven;
            receipt.reason = ReasonFamily::InstrumentFailure;
            receipt.next_action = ActionClass::RetryNewAttempt;
        }
    })
    .unwrap_or_else(|error| fail(&format!("not-proven folds: {error}")));
    assert_eq!(unproven.result, TerminalResult::NotProven);
    assert_ne!(timed_out.stage_set_digest, unproven.stage_set_digest);
}

#[test]
fn green_local_development_still_cannot_claim_installed() {
    let mut intent = base_intent();
    intent.mode = InstallMode::ExplicitLocalDevelopment;
    intent.selector = ReleaseSelector::not_applicable();
    let subject =
        ResolvedStandaloneInstallSubject::ExplicitLocalDevelopment(LocalDevelopmentSubject {
            schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
            subject_id: "subject-localdev-test".into(),
            description: "developer checkout".into(),
            destination_role: DestinationRole::UserLocal,
        });
    let resolved = must(resolve_subject(&intent, subject), "local dev resolves");
    let subject_digest = must(resolved.validate(), "local dev validates");
    let dag = local_dag();
    let outcome = folded(&intent, &dag, &resolved, &subject_digest, |_| {})
        .unwrap_or_else(|error| fail(&format!("green local dev folds: {error}")));
    assert_eq!(outcome.result, TerminalResult::NotProven);
    assert_eq!(outcome.reason, ReasonFamily::LocalDevelopmentNonAuthoritative);
}

/// Local development declares no product-unit identity, but its destination
/// role is real and stays bound to the intent like every other mode's.
#[test]
fn local_development_subject_cannot_drift_destination() {
    let mut intent = base_intent();
    intent.mode = InstallMode::ExplicitLocalDevelopment;
    intent.selector = ReleaseSelector::not_applicable();
    let subject =
        ResolvedStandaloneInstallSubject::ExplicitLocalDevelopment(LocalDevelopmentSubject {
            schema_version: SUBJECT_SCHEMA_VERSION.to_string(),
            subject_id: "subject-localdev-drift".into(),
            description: "developer checkout".into(),
            destination_role: DestinationRole::SystemShared,
        });
    expect_code(resolve_subject(&intent, subject).map(|_| ()), ContractViolation::OutcomeConflict);
}

#[test]
fn terminal_result_follows_operation_vocabulary() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    for (operation, expected) in [
        (InstallOperation::Repair, TerminalResult::Repaired),
        (InstallOperation::Update, TerminalResult::Updated),
        (InstallOperation::Rollback, TerminalResult::RolledBack),
    ] {
        let outcome = fold_terminal_outcome(FanInInput {
            dag: &dag,
            operation,
            mode: intent.mode,
            transaction_id: &intent.transaction_id,
            attempt_id: &intent.attempt_id,
            subject: &subject,
            subject_digest: &subject_digest,
            receipts: &receipts,
        })
        .unwrap_or_else(|error| fail(&format!("{}: {error}", operation.as_str())));
        assert_eq!(outcome.result, expected, "operation mapping drift for {}", operation.as_str());
        assert_eq!(outcome.side_effect_ceiling, SideEffectCeiling::InstalledClaim);
    }
}

// ---------------------------------------------------------------------------
// Receipt↔subject policy binding (#11099), schema mutations, absent stages,
// and candidate disposition
// ---------------------------------------------------------------------------

#[test]
fn receipt_policy_identities_must_match_the_settled_subject() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);

    // Integrity-bearing stage bound to another integrity policy.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) =
                receipts.iter_mut().find(|receipt| receipt.stage_id == StageId::ChecksumIntegrity)
            {
                receipt.integrity_policy_id = Some("integrity-rogue".into());
            }
        }),
        ContractViolation::PolicyIdentityMismatch,
    );

    // Integrity evidence missing where the subject demands it.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) = receipts
                .iter_mut()
                .find(|receipt| receipt.stage_id == StageId::ArchiveManifestAndStaging)
            {
                receipt.integrity_policy_id = None;
            }
        }),
        ContractViolation::PolicyIdentityMismatch,
    );

    // A policy identity smuggled onto a stage that bears none.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) =
                receipts.iter_mut().find(|receipt| receipt.stage_id == StageId::Promotion)
            {
                receipt.toolchain_policy_id = Some("toolchain-stable-v1".into());
            }
        }),
        ContractViolation::PolicyIdentityMismatch,
    );

    // Provenance receipts bind the subject's optional provenance policy;
    // claiming one when the subject authorizes none is drift.
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            if let Some(receipt) =
                receipts.iter_mut().find(|receipt| receipt.stage_id == StageId::Provenance)
            {
                receipt.provenance_policy_id = Some("provenance-unrequested".into());
            }
        }),
        ContractViolation::PolicyIdentityMismatch,
    );

    // Registry-source subjects anchor toolchain policies instead.
    let mut source_intent = base_intent();
    source_intent.mode = InstallMode::ExactRegistrySource;
    source_intent.selector = ReleaseSelector::not_applicable();
    source_intent.requested_product_unit = ProductUnit::ServerOnly;
    let source_subject = must(
        resolve_subject(&source_intent, registry_candidate(&source_intent)),
        "registry subject resolves",
    );
    let source_digest = must(source_subject.validate(), "registry subject validates");
    let source_dag = source_dag(ProductUnit::ServerOnly);
    expect_violation(
        folded(&source_intent, &source_dag, &source_subject, &source_digest, |receipts| {
            if let Some(receipt) =
                receipts.iter_mut().find(|receipt| receipt.stage_id == StageId::SourceBuild)
            {
                receipt.toolchain_policy_id = None;
            }
        }),
        ContractViolation::PolicyIdentityMismatch,
    );
}

#[test]
fn unknown_receipt_schema_version_fails_closed() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    expect_violation(
        folded(&intent, &dag, &subject, &subject_digest, |receipts| {
            // A future/foreign receipt schema can never compose into this
            // model's digest domains.
            if let Some(receipt) = receipts.first_mut() {
                receipt.schema_version = "standalone_stage_receipt.v2".into();
            }
        }),
        ContractViolation::UnknownReceiptSchema,
    );
}

#[test]
fn receipt_citing_a_stage_absent_from_the_dag_is_rejected() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let mut receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    // Well-formed receipt whose stage simply does not exist in this DAG.
    let intruder = succeeded_receipt(
        &intent.transaction_id,
        &intent.attempt_id,
        &subject_digest,
        StageId::Uninstall,
        &[],
    );
    assert!(intruder.validate().is_ok(), "the intruding receipt must be valid on its own terms");
    receipts.push(intruder);
    let error = fold_terminal_outcome(FanInInput {
        dag: &dag,
        operation: intent.operation,
        mode: intent.mode,
        transaction_id: &intent.transaction_id,
        attempt_id: &intent.attempt_id,
        subject: &subject,
        subject_digest: &subject_digest,
        receipts: &receipts,
    })
    .err()
    .unwrap_or_else(|| fail("an out-of-DAG receipt must fail the fold"));
    assert_eq!(error.code(), ContractViolation::UnauthorizedStageApplicability);
}

#[test]
fn empty_or_truncated_dags_cannot_authorize_success() {
    let intent = base_intent();
    let empty = StageDag {
        schema_version: DAG_SCHEMA_VERSION.to_string(),
        mode: intent.mode,
        product_unit: intent.requested_product_unit,
        nodes: Vec::new(),
    };
    expect_code(empty.validate(), ContractViolation::MissingRequiredStage);

    let mut truncated = archive_dag(intent.requested_product_unit);
    truncated.nodes.retain(|node| node.stage_id != StageId::Promotion);
    expect_code(truncated.validate(), ContractViolation::MissingRequiredStage);
}

#[test]
fn receipts_cannot_arrive_before_declared_predecessors() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let mut receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    let predecessor = receipts.remove(0);
    receipts.insert(1, predecessor);
    expect_code(
        fold_terminal_outcome(FanInInput {
            dag: &dag,
            operation: intent.operation,
            mode: intent.mode,
            transaction_id: &intent.transaction_id,
            attempt_id: &intent.attempt_id,
            subject: &subject,
            subject_digest: &subject_digest,
            receipts: &receipts,
        }),
        ContractViolation::PredecessorMismatch,
    );
}

#[test]
fn candidate_disposition_follows_the_operation_and_receipts() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);

    // Green install confirms the newly promoted current candidate.
    let installed = folded(&intent, &dag, &subject, &subject_digest, |_| {})
        .unwrap_or_else(|error| fail(&format!("green fold: {error}")));
    assert_eq!(installed.candidate_disposition, CandidateDisposition::CurrentConfirmed);

    // Green rollback restores the previous complete candidate.
    let mut rollback = intent.clone();
    rollback.operation = InstallOperation::Rollback;
    let rolled_back = folded(&rollback, &dag, &subject, &subject_digest, |_| {})
        .unwrap_or_else(|error| fail(&format!("rollback fold: {error}")));
    assert_eq!(rolled_back.result, TerminalResult::RolledBack);
    assert_eq!(rolled_back.candidate_disposition, CandidateDisposition::Unresolved);

    // Green uninstall leaves no candidate installed.
    let mut uninstall = base_intent();
    uninstall.mode = InstallMode::ExactRegistrySource;
    uninstall.selector = ReleaseSelector::not_applicable();
    uninstall.operation = InstallOperation::Uninstall;
    uninstall.requested_product_unit = ProductUnit::ServerOnly;
    let uninstalled_subject = must(
        resolve_subject(&uninstall, registry_candidate(&uninstall)),
        "uninstall subject resolves",
    );
    let uninstalled_digest = must(uninstalled_subject.validate(), "uninstall subject validates");
    let mut uninstall_dag = source_dag(ProductUnit::ServerOnly);
    uninstall_dag.nodes.push(node(
        StageId::Uninstall,
        Applicability::Required,
        &[StageId::InstalledTransition],
    ));
    let removed =
        folded(&uninstall, &uninstall_dag, &uninstalled_subject, &uninstalled_digest, |_| {})
            .unwrap_or_else(|error| fail(&format!("uninstall fold: {error}")));
    assert_eq!(removed.result, TerminalResult::Uninstalled);
    assert_eq!(removed.candidate_disposition, CandidateDisposition::NoneRemaining);

    // Terminal evidence before the installed transition stays unresolved.
    let blocked = folded(&intent, &dag, &subject, &subject_digest, |receipts| {
        if let Some(receipt) =
            receipts.iter_mut().find(|receipt| receipt.stage_id == StageId::PathPersistence)
        {
            receipt.result = StageResult::Failed;
            receipt.reason = ReasonFamily::ObservationFailed;
            receipt.next_action = ActionClass::AbortInstall;
        }
        receipts.retain(|receipt| {
            dag.position_of(receipt.stage_id) <= dag.position_of(StageId::PathPersistence)
        });
    })
    .unwrap_or_else(|error| fail(&format!("blocked fold: {error}")));
    assert_eq!(blocked.result, TerminalResult::Failed);
    assert_eq!(blocked.candidate_disposition, CandidateDisposition::Unresolved);
}

#[test]
fn folding_twice_produces_byte_identical_outcomes() {
    let intent = base_intent();
    let (subject, subject_digest) = resolved_archive(&intent);
    let dag = archive_dag(intent.requested_product_unit);
    let receipts =
        green_chain(&intent.transaction_id, &intent.attempt_id, &subject, &subject_digest, &dag)
            .unwrap_or_else(|error| fail(&format!("green chain: {error}")));
    let fold = || {
        fold_terminal_outcome(FanInInput {
            dag: &dag,
            operation: intent.operation,
            mode: intent.mode,
            transaction_id: &intent.transaction_id,
            attempt_id: &intent.attempt_id,
            subject: &subject,
            subject_digest: &subject_digest,
            receipts: &receipts,
        })
        .unwrap_or_else(|error| fail(&format!("fold: {error}")))
    };
    let first = fold();
    let second = fold();
    assert_eq!(first, second);
    let first_bytes = match canonical_bytes(&first) {
        Ok(bytes) => bytes,
        Err(error) => fail(&format!("canonical bytes: {error}")),
    };
    let second_bytes = match canonical_bytes(&second) {
        Ok(bytes) => bytes,
        Err(error) => fail(&format!("canonical bytes: {error}")),
    };
    assert_eq!(first_bytes, second_bytes, "second generation produced a diff");
}

#[test]
fn subject_digest_is_field_sensitive_over_every_identity_field() {
    let intent = base_intent();
    let base_subject = release_archive_subject(&intent);
    let baseline = must(
        ResolvedStandaloneInstallSubject::ReleaseArchive(base_subject.clone()).validate(),
        "baseline",
    );
    let mutations: Vec<(&str, ReleaseArchiveSubject)> = vec![
        (
            "repository",
            ReleaseArchiveSubject {
                repository: "OtherOwner/other-repo".into(),
                ..base_subject.clone()
            },
        ),
        ("tag", ReleaseArchiveSubject { tag: "v0.18.1".into(), ..base_subject.clone() }),
        (
            "topology_id",
            ReleaseArchiveSubject { topology_id: "topology-next".into(), ..base_subject.clone() },
        ),
        (
            "topology_digest",
            ReleaseArchiveSubject { topology_digest: "cd".repeat(32), ..base_subject.clone() },
        ),
        (
            "topology_row",
            ReleaseArchiveSubject { topology_row: "other-row".into(), ..base_subject.clone() },
        ),
        (
            "target",
            ReleaseArchiveSubject {
                target: TargetIdentity {
                    platform: Platform::Linux,
                    triple: "aarch64-unknown-linux-gnu".into(),
                    libc: LibcDisposition::Gnu,
                },
                ..base_subject.clone()
            },
        ),
        (
            "archive_name",
            ReleaseArchiveSubject {
                archive_name: "perllsp-v0.18.1.tar.gz".into(),
                ..base_subject.clone()
            },
        ),
        (
            "archive_format",
            ReleaseArchiveSubject { archive_format: ArchiveFormat::Zip, ..base_subject.clone() },
        ),
        (
            "members",
            ReleaseArchiveSubject {
                expected_members: vec![
                    MemberIdentity {
                        role: MemberRole::PerlDapAdapter,
                        artifact_name: "renamed-adapter".into(),
                    },
                    MemberIdentity {
                        role: MemberRole::PerllspServer,
                        artifact_name: "perllsp".into(),
                    },
                ],
                ..base_subject.clone()
            },
        ),
        (
            "product_unit",
            ReleaseArchiveSubject {
                product_unit: ProductUnit::ServerOnly,
                expected_members: canonical_members(ProductUnit::ServerOnly),
                ..base_subject.clone()
            },
        ),
        (
            "integrity_policy",
            ReleaseArchiveSubject {
                integrity_policy_id: "integrity-v2".into(),
                ..base_subject.clone()
            },
        ),
        (
            "provenance_policy",
            ReleaseArchiveSubject {
                provenance_policy_id: Some("provenance-v1".into()),
                ..base_subject.clone()
            },
        ),
        (
            "destination_role",
            ReleaseArchiveSubject {
                destination_role: DestinationRole::SystemShared,
                ..base_subject.clone()
            },
        ),
        (
            "subject_id",
            ReleaseArchiveSubject { subject_id: "subject-renamed".into(), ..base_subject },
        ),
    ];
    for (field, mutated) in mutations {
        let digest = ResolvedStandaloneInstallSubject::ReleaseArchive(mutated)
            .validate()
            .unwrap_or_else(|error| fail(&format!("{field}: {error}")));
        assert_ne!(digest, baseline, "{field} failed to change the subject digest");
    }
}

#[test]
fn digests_are_domain_separated_across_purposes() {
    let payload = br#"identical payload"#;
    let intent_digest = domain_digest(INTENT_DIGEST_DOMAIN, payload);
    let subject_digest = domain_digest(SUBJECT_DIGEST_DOMAIN, payload);
    let receipt_digest = domain_digest(RECEIPT_DIGEST_DOMAIN, payload);
    let stage_set_digest = domain_digest(STAGE_SET_DIGEST_DOMAIN, payload);
    assert_ne!(intent_digest, subject_digest);
    assert_ne!(intent_digest, receipt_digest);
    assert_ne!(subject_digest, receipt_digest);
    assert_ne!(receipt_digest, stage_set_digest);
}

#[test]
fn private_state_never_enters_durable_output() {
    for leaking in [
        "/usr/local/bin/perllsp",
        "C:\\Users\\dev\\perllsp",
        "$HOME/.perllsp",
        "${HOME}",
        "%USERPROFILE%\\perllsp",
        "PATH=/usr/bin:/bin",
        "Bearer abc123",
        "-----BEGIN PRIVATE KEY-----",
        "token=supersecret",
        "password=hunter2",
        "api_key=AKIAEXAMPLE",
        "Authorization: Basic dXNlcjpwYXNz",
        "/tmp/perllsp-stage",
    ] {
        assert!(redaction_finding(leaking).is_some(), "privacy scan missed {leaking:?}");
    }
    for benign in
        ["perllsp", "first_party_posix", "release_archive", "tx-11099-test", "fixture/transport"]
    {
        assert!(redaction_finding(benign).is_none(), "privacy scan false-positived on {benign:?}");
    }

    let intent = base_intent();
    let (_, subject_digest) = resolved_archive(&intent);
    let mut leaky = succeeded_receipt(
        &intent.transaction_id,
        &intent.attempt_id,
        &subject_digest,
        StageId::Transport,
        &[],
    );
    leaky.implementation_identity = "curl -o $HOME/payload".into();
    let error = leaky.validate().err().unwrap_or_else(|| fail("leak must fail closed"));
    assert_eq!(error.code(), ContractViolation::PrivateOutputLeakage);

    // Raw redaction disposition never validates as durable evidence.
    leaky.implementation_identity = "fixture/transport".into();
    leaky.redaction_disposition = RedactionDisposition::Raw;
    let error = leaky.validate().err().unwrap_or_else(|| fail("raw disposition must fail closed"));
    assert_eq!(error.code(), ContractViolation::PrivateOutputLeakage);
}

#[test]
fn wire_literals_are_the_single_spelling_authority() {
    for value in RouteMode::ALL {
        assert_eq!(RouteMode::parse(value.as_str()), Some(*value));
    }
    for value in InstallOperation::ALL {
        assert_eq!(InstallOperation::parse(value.as_str()), Some(*value));
    }
    for value in InstallMode::ALL {
        assert_eq!(InstallMode::parse(value.as_str()), Some(*value));
    }
    for value in SelectorKind::ALL {
        assert_eq!(SelectorKind::parse(value.as_str()), Some(*value));
    }
    for value in Platform::ALL {
        assert_eq!(Platform::parse(value.as_str()), Some(*value));
    }
    for value in LibcDisposition::ALL {
        assert_eq!(LibcDisposition::parse(value.as_str()), Some(*value));
    }
    for value in ProductUnit::ALL {
        assert_eq!(ProductUnit::parse(value.as_str()), Some(*value));
    }
    for value in MemberRole::ALL {
        assert_eq!(MemberRole::parse(value.as_str()), Some(*value));
    }
    for value in DestinationRole::ALL {
        assert_eq!(DestinationRole::parse(value.as_str()), Some(*value));
    }
    for value in PathPolicy::ALL {
        assert_eq!(PathPolicy::parse(value.as_str()), Some(*value));
    }
    for value in FallbackPolicy::ALL {
        assert_eq!(FallbackPolicy::parse(value.as_str()), Some(*value));
    }
    for value in ArchiveFormat::ALL {
        assert_eq!(ArchiveFormat::parse(value.as_str()), Some(*value));
    }
    for value in StageId::ALL {
        assert_eq!(StageId::parse(value.as_str()), Some(*value));
    }
    for value in StageResult::ALL {
        assert_eq!(StageResult::parse(value.as_str()), Some(*value));
    }
    for value in InstrumentCompleteness::ALL {
        assert_eq!(InstrumentCompleteness::parse(value.as_str()), Some(*value));
    }
    for value in RedactionDisposition::ALL {
        assert_eq!(RedactionDisposition::parse(value.as_str()), Some(*value));
    }
    for value in ActionClass::ALL {
        assert_eq!(ActionClass::parse(value.as_str()), Some(*value));
    }
    for value in ReasonFamily::ALL {
        assert_eq!(ReasonFamily::parse(value.as_str()), Some(*value));
    }
    for value in TerminalResult::ALL {
        assert_eq!(TerminalResult::parse(value.as_str()), Some(*value));
    }
    for value in SideEffectCeiling::ALL {
        assert_eq!(SideEffectCeiling::parse(value.as_str()), Some(*value));
    }
    for value in Applicability::ALL {
        assert_eq!(Applicability::parse(value.as_str()), Some(*value));
    }
    for value in ContractViolation::ALL {
        assert_eq!(ContractViolation::parse(value.as_str()), Some(*value));
    }
}
