//! Deterministic fake receipts and false-green controls for `readiness_rehearsal.v1`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::model::{
    ArchiveMember, Artifact, ArtifactKind, Availability, Cleanup, CleanupDisposition, FIXTURE_DIR,
    InstalledKind, InstalledSubject, Instrument, InstrumentKind, Lockfiles, Mutation, MutationKind,
    Outcome, PathRole, ReasonCode, RehearsalError, RehearsalReceipt, Repository, SCHEMA_VERSION,
    SourceKind, Stage, Status, TreeStatus,
};
use super::validate::validate_receipt;

pub(crate) enum Expect {
    Accept { status: Status },
    Reject { reason: ReasonCode },
}

pub(crate) struct FixtureCase {
    pub(crate) file: &'static str,
    pub(crate) expect: Expect,
    builder: fn() -> Result<RehearsalReceipt, RehearsalError>,
}

pub(crate) fn fixture_cases() -> Vec<FixtureCase> {
    vec![
        accept("valid_pass.json", Status::Pass, valid_pass),
        accept("valid_failed.json", Status::Failed, valid_failed),
        accept("valid_limited.json", Status::Limited, valid_limited),
        accept("valid_not_proven.json", Status::NotProven, valid_not_proven),
        reject("wrong_binary.json", ReasonCode::WrongBinaryPathRole, wrong_binary),
        reject("mixed_source_sha.json", ReasonCode::MixedSourceSha, mixed_source_sha),
        reject("mixed_version.json", ReasonCode::MixedVersion, mixed_version),
        reject(
            "instrument_sha_identity.json",
            ReasonCode::IdentityKindMismatch,
            instrument_sha_identity,
        ),
        reject(
            "missing_archive_member.json",
            ReasonCode::MissingArchiveMember,
            missing_archive_member,
        ),
        reject(
            "duplicate_archive_member.json",
            ReasonCode::DuplicateArchiveMember,
            duplicate_archive_member,
        ),
        reject(
            "colliding_archive_member.json",
            ReasonCode::CollidingArchiveMember,
            colliding_archive_member,
        ),
        reject(
            "checksum_invalid_member.json",
            ReasonCode::ChecksumInvalidMember,
            checksum_invalid_member,
        ),
        reject("vsix_server_mismatch.json", ReasonCode::VsixServerMismatch, vsix_server_mismatch),
        reject(
            "unavailable_instrument_pass.json",
            ReasonCode::UnavailableInstrumentPass,
            unavailable_instrument_pass,
        ),
        reject("cancelled_stage_pass.json", ReasonCode::NonTerminalStagePass, cancelled_stage_pass),
        reject("timed_out_stage_pass.json", ReasonCode::NonTerminalStagePass, timed_out_stage_pass),
        reject("skipped_stage_pass.json", ReasonCode::NonTerminalStagePass, skipped_stage_pass),
        reject("malformed_stage_pass.json", ReasonCode::NonTerminalStagePass, malformed_stage_pass),
        reject("stale_stage_pass.json", ReasonCode::NonTerminalStagePass, stale_stage_pass),
        reject(
            "failed_cleanup_hidden.json",
            ReasonCode::FailedCleanupHidden,
            failed_cleanup_hidden,
        ),
        reject("mutation_tag.json", ReasonCode::MutationAttempted, mutation_tag),
        reject("privacy_home_path.json", ReasonCode::PrivacyLeak, privacy_home_path),
        reject("privacy_credential.json", ReasonCode::PrivacyLeak, privacy_credential),
        reject("privacy_unbounded_log.json", ReasonCode::PrivacyLeak, privacy_unbounded_log),
        reject("compensating_stage.json", ReasonCode::CompensatingStage, compensating_stage),
        reject(
            "published_channels_non_empty.json",
            ReasonCode::PublishedChannelsNonEmpty,
            published_channels_non_empty,
        ),
        reject("release_cut_true.json", ReasonCode::ReleaseCutTrue, release_cut_true),
        reject(
            "receipt_digest_mismatch.json",
            ReasonCode::ReceiptDigestMismatch,
            receipt_digest_mismatch,
        ),
    ]
}

fn accept(
    file: &'static str,
    status: Status,
    builder: fn() -> Result<RehearsalReceipt, RehearsalError>,
) -> FixtureCase {
    FixtureCase { file, expect: Expect::Accept { status }, builder }
}

fn reject(
    file: &'static str,
    reason: ReasonCode,
    builder: fn() -> Result<RehearsalReceipt, RehearsalError>,
) -> FixtureCase {
    FixtureCase { file, expect: Expect::Reject { reason }, builder }
}

pub(crate) fn build_case(case: &FixtureCase) -> Result<RehearsalReceipt, RehearsalError> {
    (case.builder)()
}

pub(crate) fn sync_fixture_files(root: &Path, update: bool) -> Result<(), RehearsalError> {
    let dir = root.join(FIXTURE_DIR);
    if update {
        fs::create_dir_all(&dir).map_err(|error| {
            RehearsalError::new(ReasonCode::MalformedDocument, error.to_string())
        })?;
    }
    for case in fixture_cases() {
        let receipt = build_case(&case)?;
        let encoded = format!(
            "{}\n",
            serde_json::to_string_pretty(&receipt).map_err(|error| {
                RehearsalError::new(ReasonCode::MalformedDocument, error.to_string())
            })?
        );
        let path = dir.join(case.file);
        if update {
            fs::write(&path, &encoded).map_err(|error| {
                RehearsalError::new(ReasonCode::MalformedDocument, error.to_string())
            })?;
            continue;
        }
        let on_disk = fs::read_to_string(&path).map_err(|error| {
            RehearsalError::new(
                ReasonCode::MalformedDocument,
                format!("{}: {error}", path.display()),
            )
        })?;
        if on_disk != encoded {
            return Err(RehearsalError::new(
                ReasonCode::MalformedDocument,
                format!(
                    "{} drifted from the typed fixture builder; rerun with UPDATE_READINESS_REHEARSAL_FIXTURES=1",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn run_fixture_suite() -> Result<(), RehearsalError> {
    let mut failures = Vec::new();
    for case in fixture_cases() {
        if let Err(error) = run_one(&case) {
            failures.push(format!("{}: {error}", case.file));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(RehearsalError::new(
            ReasonCode::SchemaViolation,
            format!("fixture suite failed:\n{}", failures.join("\n")),
        ))
    }
}

fn run_one(case: &FixtureCase) -> Result<(), RehearsalError> {
    let receipt = build_case(case)?;
    match &case.expect {
        Expect::Accept { status } => {
            let accepted = validate_receipt(receipt)?;
            if accepted.status != *status {
                return Err(RehearsalError::new(
                    ReasonCode::StatusMismatch,
                    format!(
                        "{} accepted as {} want {}",
                        case.file,
                        accepted.status.as_str(),
                        status.as_str()
                    ),
                ));
            }
            Ok(())
        }
        Expect::Reject { reason } => match validate_receipt(receipt) {
            Ok(_) => Err(RehearsalError::new(
                ReasonCode::StatusMismatch,
                format!("{} expected {}", case.file, reason.as_str()),
            )),
            Err(error) if error.code == *reason => Ok(()),
            Err(error) => Err(RehearsalError::new(
                ReasonCode::StatusMismatch,
                format!("{} expected {}, got {}", case.file, reason.as_str(), error.code.as_str()),
            )),
        },
    }
}

fn sha(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn git(byte: u8) -> String {
    format!("{byte:02x}").repeat(20)
}

fn completed_instrument(role: &str) -> Instrument {
    Instrument {
        role: role.to_string(),
        identity: format!("xtask-{role}"),
        kind: InstrumentKind::Instrument,
        availability: Availability::Available,
        outcome: Outcome::Completed,
        exit_code: Some(0),
    }
}

fn stage(
    id: &str,
    mandatory: bool,
    status: Status,
    instrument: Instrument,
    artifacts: Vec<Artifact>,
    installed: Vec<InstalledSubject>,
) -> Stage {
    Stage {
        id: id.to_string(),
        mandatory,
        status,
        instrument,
        artifacts,
        installed,
        limitations: Vec::new(),
        owning_issues: vec!["#16785".to_string()],
    }
}

fn package_artifact() -> Artifact {
    Artifact {
        role: "package_crate".to_string(),
        name: "perl-lsp-0.0.0-rehearsal.crate".to_string(),
        kind: ArtifactKind::ArtifactBytes,
        digest: sha(0x11),
        size: 32,
        candidate_id: "rehearsal-fixture".to_string(),
        source_sha: git(0xaa),
        members: None,
        declared_server_identity: None,
    }
}

fn vsix_artifact(declared: &str) -> Artifact {
    Artifact {
        role: "vsix".to_string(),
        name: "perl-lsp-rs-rehearsal.vsix".to_string(),
        kind: ArtifactKind::ArtifactBytes,
        digest: sha(0x22),
        size: 64,
        candidate_id: "rehearsal-fixture".to_string(),
        source_sha: git(0xaa),
        members: None,
        declared_server_identity: Some(declared.to_string()),
    }
}

fn archive_artifact(members: Option<Vec<ArchiveMember>>) -> Artifact {
    Artifact {
        role: "archive".to_string(),
        name: "perllsp-rehearsal-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        kind: ArtifactKind::ArtifactBytes,
        digest: sha(0x33),
        size: 128,
        candidate_id: "rehearsal-fixture".to_string(),
        source_sha: git(0xaa),
        members,
        declared_server_identity: None,
    }
}

fn default_members() -> Vec<ArchiveMember> {
    vec![
        ArchiveMember {
            name: "perllsp".to_string(),
            digest: sha(0x44),
            size: 16,
            checksum: Some(sha(0x44)),
        },
        ArchiveMember {
            name: "perl-dap".to_string(),
            digest: sha(0x55),
            size: 16,
            checksum: Some(sha(0x55)),
        },
    ]
}

fn installed(role: &str, identity: &str, digest: String, path_role: PathRole) -> InstalledSubject {
    InstalledSubject {
        role: role.to_string(),
        kind: InstalledKind::InstalledSubject,
        identity: identity.to_string(),
        digest,
        path_role,
    }
}

fn base_receipt(stages: Vec<Stage>, status: Status) -> RehearsalReceipt {
    RehearsalReceipt {
        schema_version: SCHEMA_VERSION.to_string(),
        repository: Repository {
            head_sha: git(0xaa),
            kind: SourceKind::SourceRevision,
            tree_status: TreeStatus::Clean,
            lockfiles: Lockfiles { cargo_lock: sha(0x66), npm_lock: sha(0x77) },
            toolchains: BTreeMap::from([
                ("rustc".to_string(), "1.95.0".to_string()),
                ("cargo".to_string(), "1.95.0".to_string()),
            ]),
        },
        candidate_id: "rehearsal-fixture".to_string(),
        package_version: "0.0.0+rehearsal".to_string(),
        extension_version: "0.0.0+rehearsal".to_string(),
        stages,
        cleanup: Cleanup { disposition: CleanupDisposition::Cleaned, status: Status::Pass },
        mutations: Vec::new(),
        limitations: Vec::new(),
        owning_issues: vec!["#16785".to_string()],
        published_channels: Vec::new(),
        release_cut: false,
        status,
        receipt_digest: String::new(),
    }
}

fn pass_stages() -> Vec<Stage> {
    vec![
        stage(
            "identity_preflight",
            true,
            Status::Pass,
            completed_instrument("identity-preflight"),
            Vec::new(),
            Vec::new(),
        ),
        stage(
            "package_install",
            true,
            Status::Pass,
            completed_instrument("package-install"),
            vec![package_artifact()],
            vec![installed(
                "perllsp",
                "perllsp 0.0.0+rehearsal",
                sha(0x44),
                PathRole::StagedInstall,
            )],
        ),
        stage(
            "vsix_inspect",
            true,
            Status::Pass,
            completed_instrument("vsix-inspect"),
            vec![vsix_artifact("perllsp 0.0.0+rehearsal")],
            vec![installed(
                "bundled_server",
                "perllsp 0.0.0+rehearsal",
                sha(0x44),
                PathRole::VsixBundledServer,
            )],
        ),
        stage(
            "archive_members",
            true,
            Status::Pass,
            completed_instrument("archive-members"),
            vec![archive_artifact(Some(default_members()))],
            vec![installed(
                "perllsp",
                "perllsp 0.0.0+rehearsal",
                sha(0x44),
                PathRole::ArchiveExtracted,
            )],
        ),
        stage(
            "cleanup",
            true,
            Status::Pass,
            completed_instrument("cleanup"),
            Vec::new(),
            Vec::new(),
        ),
    ]
}

fn mutate_pass(
    edit: impl FnOnce(&mut RehearsalReceipt),
) -> Result<RehearsalReceipt, RehearsalError> {
    let mut receipt = base_receipt(pass_stages(), Status::Pass);
    edit(&mut receipt);
    receipt.seal()
}

fn valid_pass() -> Result<RehearsalReceipt, RehearsalError> {
    base_receipt(pass_stages(), Status::Pass).seal()
}

fn valid_failed() -> Result<RehearsalReceipt, RehearsalError> {
    let mut stages = pass_stages();
    if let Some(package) = stages.iter_mut().find(|stage| stage.id == "package_install") {
        package.status = Status::Failed;
        package.instrument.exit_code = Some(1);
        package.limitations = vec!["staged package install refused the fake crate".to_string()];
    }
    if let Some(vsix) = stages.iter_mut().find(|stage| stage.id == "vsix_inspect") {
        vsix.status = Status::NotProven;
        vsix.instrument.availability = Availability::NotProven;
        vsix.instrument.outcome = Outcome::Skipped;
        vsix.instrument.exit_code = None;
        vsix.artifacts.clear();
        vsix.installed.clear();
    }
    if let Some(archive) = stages.iter_mut().find(|stage| stage.id == "archive_members") {
        archive.status = Status::NotProven;
        archive.instrument.outcome = Outcome::Skipped;
        archive.instrument.exit_code = None;
        archive.artifacts.clear();
        archive.installed.clear();
    }
    base_receipt(stages, Status::Failed).seal()
}

fn valid_limited() -> Result<RehearsalReceipt, RehearsalError> {
    let mut stages = pass_stages();
    if let Some(vsix) = stages.iter_mut().find(|stage| stage.id == "vsix_inspect") {
        vsix.status = Status::Limited;
        vsix.instrument.availability = Availability::Unavailable;
        vsix.instrument.outcome = Outcome::Skipped;
        vsix.instrument.exit_code = None;
        vsix.limitations =
            vec!["vsix inspect instrument is unavailable in this fixture".to_string()];
        vsix.artifacts.clear();
        vsix.installed.clear();
    }
    base_receipt(stages, Status::Limited).seal()
}

fn valid_not_proven() -> Result<RehearsalReceipt, RehearsalError> {
    let mut stages = pass_stages();
    if let Some(archive) = stages.iter_mut().find(|stage| stage.id == "archive_members") {
        archive.status = Status::NotProven;
        archive.instrument.availability = Availability::NotProven;
        archive.instrument.outcome = Outcome::Stale;
        archive.instrument.exit_code = None;
        archive.limitations = vec!["archive instrument evidence is stale".to_string()];
        archive.artifacts.clear();
        archive.installed.clear();
    }
    base_receipt(stages, Status::NotProven).seal()
}

fn wrong_binary() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(package) = receipt.stages.iter_mut().find(|stage| stage.id == "package_install")
            && let Some(installed) = package.installed.first_mut()
        {
            installed.path_role = PathRole::WorkspaceTarget;
        }
    })
}

fn mixed_source_sha() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(package) = receipt.stages.iter_mut().find(|stage| stage.id == "package_install")
            && let Some(artifact) = package.artifacts.first_mut()
        {
            artifact.source_sha = git(0xbb);
        }
    })
}

fn mixed_version() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.extension_version = "0.1.0+other".to_string();
    })
}

fn instrument_sha_identity() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(preflight) =
            receipt.stages.iter_mut().find(|stage| stage.id == "identity_preflight")
        {
            preflight.instrument.identity = git(0xaa);
        }
    })
}

fn missing_archive_member() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(archive) = receipt.stages.iter_mut().find(|stage| stage.id == "archive_members")
            && let Some(artifact) = archive.artifacts.first_mut()
        {
            artifact.members = Some(Vec::new());
        }
    })
}

fn duplicate_archive_member() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(archive) = receipt.stages.iter_mut().find(|stage| stage.id == "archive_members")
            && let Some(artifact) = archive.artifacts.first_mut()
        {
            artifact.members = Some(vec![
                ArchiveMember {
                    name: "perllsp".to_string(),
                    digest: sha(0x44),
                    size: 16,
                    checksum: Some(sha(0x44)),
                },
                ArchiveMember {
                    name: "perllsp".to_string(),
                    digest: sha(0x55),
                    size: 16,
                    checksum: Some(sha(0x55)),
                },
            ]);
        }
    })
}

fn colliding_archive_member() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(archive) = receipt.stages.iter_mut().find(|stage| stage.id == "archive_members")
            && let Some(artifact) = archive.artifacts.first_mut()
        {
            artifact.members = Some(vec![
                ArchiveMember {
                    name: "perllsp".to_string(),
                    digest: sha(0x44),
                    size: 16,
                    checksum: Some(sha(0x44)),
                },
                ArchiveMember {
                    name: "perl-dap".to_string(),
                    digest: sha(0x44),
                    size: 16,
                    checksum: Some(sha(0x44)),
                },
            ]);
        }
    })
}

fn checksum_invalid_member() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(archive) = receipt.stages.iter_mut().find(|stage| stage.id == "archive_members")
            && let Some(artifact) = archive.artifacts.first_mut()
        {
            artifact.members = Some(vec![ArchiveMember {
                name: "perllsp".to_string(),
                digest: sha(0x44),
                size: 16,
                checksum: Some(sha(0x99)),
            }]);
        }
    })
}

fn vsix_server_mismatch() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(vsix) = receipt.stages.iter_mut().find(|stage| stage.id == "vsix_inspect")
            && let Some(artifact) = vsix.artifacts.first_mut()
        {
            artifact.declared_server_identity = Some("perllsp 9.9.9+other".to_string());
        }
    })
}

fn unavailable_instrument_pass() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(vsix) = receipt.stages.iter_mut().find(|stage| stage.id == "vsix_inspect") {
            vsix.instrument.availability = Availability::Unavailable;
        }
    })
}

fn non_terminal_pass(outcome: Outcome) -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(package) = receipt.stages.iter_mut().find(|stage| stage.id == "package_install")
        {
            package.instrument.outcome = outcome;
            package.instrument.exit_code = None;
        }
    })
}

fn cancelled_stage_pass() -> Result<RehearsalReceipt, RehearsalError> {
    non_terminal_pass(Outcome::Cancelled)
}

fn timed_out_stage_pass() -> Result<RehearsalReceipt, RehearsalError> {
    non_terminal_pass(Outcome::TimedOut)
}

fn skipped_stage_pass() -> Result<RehearsalReceipt, RehearsalError> {
    non_terminal_pass(Outcome::Skipped)
}

fn malformed_stage_pass() -> Result<RehearsalReceipt, RehearsalError> {
    non_terminal_pass(Outcome::Malformed)
}

fn stale_stage_pass() -> Result<RehearsalReceipt, RehearsalError> {
    non_terminal_pass(Outcome::Stale)
}

fn failed_cleanup_hidden() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.cleanup.disposition = CleanupDisposition::Failed;
        receipt.cleanup.status = Status::Failed;
    })
}

fn mutation_tag() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.mutations.push(Mutation { kind: MutationKind::Tag, attempted: true });
    })
}

fn privacy_home_path() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.limitations.push("resolved from /home/runner/work/perl-lsp".to_string());
    })
}

fn privacy_credential() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.limitations.push("token=ghp_exampleleaknotarealsecret".to_string());
    })
}

fn privacy_unbounded_log() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.limitations.push("x".repeat(600));
    })
}

fn compensating_stage() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        if let Some(package) = receipt.stages.iter_mut().find(|stage| stage.id == "package_install")
        {
            package.status = Status::Failed;
            package.instrument.exit_code = Some(2);
        }
    })
}

fn published_channels_non_empty() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.published_channels.push("crates-io".to_string());
    })
}

fn release_cut_true() -> Result<RehearsalReceipt, RehearsalError> {
    mutate_pass(|receipt| {
        receipt.release_cut = true;
    })
}

fn receipt_digest_mismatch() -> Result<RehearsalReceipt, RehearsalError> {
    let mut receipt = valid_pass()?;
    receipt.receipt_digest = sha(0x00);
    Ok(receipt)
}
