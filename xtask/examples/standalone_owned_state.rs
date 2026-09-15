//! Checked contract validator for standalone installer-owned state (#11470).
#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

const MANIFEST_SCHEMA_VERSION: &str = "standalone_owned_state.v1";
const PLAN_SCHEMA_VERSION: &str = "standalone_removal_plan.v1";
const RESULT_SCHEMA_VERSION: &str = "standalone_uninstall_result.v1";

macro_rules! closed_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        $(#[$meta])*
        pub enum $name { $($variant),+ }
        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
    };
}

closed_enum!(LinkPolicy { ResolvedPhysical => "resolved_physical", RefusedSubstitution => "refused_substitution" });
closed_enum!(InstallerKind { PosixInstaller => "posix_installer", PowershellInstaller => "powershell_installer", CargoBinstall => "cargo_binstall", ManualArchive => "manual_archive" });
closed_enum!(OwnershipClass {
    OwnedRequired => "owned_required",
    OwnedRemovable => "owned_removable",
    OwnedRetainedForRollback => "owned_retained_for_rollback",
    ForeignOrUserOwned => "foreign_or_user_owned",
    PackageManagerOrOtherRoute => "package_manager_or_other_route",
    UnknownNotSafeToDelete => "unknown_not_safe_to_delete",
    MalformedOrInstrumentFailed => "malformed_or_instrument_failed",
    RunningOrActive => "running_or_active"
});
closed_enum!(Role {
    ImmutableCandidateDir => "immutable_candidate_dir",
    CandidateManifest => "candidate_manifest",
    SelectionMarker => "selection_marker",
    InstallationReceipt => "installation_receipt",
    AttemptRecord => "attempt_record",
    PathMarker => "path_marker",
    ProfileMarker => "profile_marker",
    RegistryMarker => "registry_marker",
    TransactionLog => "transaction_log",
    UnownedFileObserved => "unowned_file_observed",
    ExternalRouteState => "external_route_state"
});
closed_enum!(RetentionDisposition {
    RetainRequired => "retain_required",
    RetainForRollback => "retain_for_rollback",
    Removable => "removable",
    ForeignPreserve => "foreign_preserve",
    ExternalRoutePreserve => "external_route_preserve",
    BlockedPendingRevalidation => "blocked_pending_revalidation"
});
closed_enum!(EntryIdentityKind { Sha256Content => "sha256_content", DirectoryTreeDigest => "directory_tree_digest", Unavailable => "unavailable" });
closed_enum!(ProcessRefKind { Pid => "pid", Pidfile => "pidfile", Socket => "socket", Lockfile => "lockfile", Unknown => "unknown" });
closed_enum!(ObservedState { Present => "present", Absent => "absent" });
closed_enum!(OperationKind { Install => "install", Upgrade => "upgrade", Rollback => "rollback", Repair => "repair", UninstallAttempt => "uninstall_attempt" });
closed_enum!(TransactionOutcome { Completed => "completed", Failed => "failed", Interrupted => "interrupted", Orphaned => "orphaned", NotProven => "not_proven" });
closed_enum!(RedactionPolicy { None => "none", SecretsAndEnvironment => "secrets_and_environment", PathsRedacted => "paths_redacted" });
closed_enum!(LifecyclePolicy { FullRemovalSelected => "full_removal_selected", RetainRollbackSelected => "retain_rollback_selected" });
closed_enum!(RunningProcessPolicy { AbortOnRunning => "abort_on_running", WaitExternalThenAbort => "wait_external_then_abort", RequireManualConfirmation => "require_manual_confirmation" });
closed_enum!(PathCleanupMode { ExactOwnedEntriesOnly => "exact_owned_entries_only", Skipped => "skipped" });
closed_enum!(ActionKind { RemoveExact => "remove_exact", RemoveMarker => "remove_marker", Preserve => "preserve", Revalidate => "revalidate" });
closed_enum!(FailureStage { Verify => "verify", Remove => "remove", MarkerCleanup => "marker_cleanup", Postcondition => "postcondition" });
closed_enum!(ActivationState {
    ConditionalActivationNotSelected => "conditional_activation_not_selected",
    ConditionalActivationSelected => "conditional_activation_selected"
});
closed_enum!(UninstallOutcome {
    Removed => "removed",
    AlreadyAbsentOwnedState => "already_absent_owned_state",
    PartialFailure => "partial_failure",
    BlockedRunning => "blocked_running",
    BlockedUnknownOrForeign => "blocked_unknown_or_foreign",
    RootOrManifestMismatch => "root_or_manifest_mismatch",
    PathCleanupFailed => "path_cleanup_failed",
    Cancelled => "cancelled",
    InstrumentFailure => "instrument_failure",
    NotProven => "not_proven",
    NotApplicable => "not_applicable"
});

#[derive(Debug)]
pub struct ContractError(String);

impl ContractError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for ContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ContractError {}

type ContractResult<T> = std::result::Result<T, ContractError>;

fn err<T>(message: impl Into<String>) -> ContractResult<T> {
    Err(ContractError::new(message))
}

fn require_nonempty(value: &str, field: &str) -> ContractResult<()> {
    if value.trim().is_empty() {
        return err(format!("{field}: must not be empty"));
    }
    Ok(())
}

const ROOT_METACHARACTERS: [char; 6] = ['*', '?', '"', '<', '>', '|'];

fn reject_root_segment(segment: &str, path: &str, field: &str) -> ContractResult<()> {
    if segment.is_empty() {
        return err(format!(
            "{field}: empty segment or trailing separator makes `{path}` a non-canonical equivalent spelling of the same root"
        ));
    }
    if segment == "." || segment == ".." {
        return err(format!(
            "{field}: dot or parent segment escapes the bounded root identity; traversal spellings are invalid (`{path}`)"
        ));
    }
    Ok(())
}

fn validate_absolute_path(path: &str, field: &str) -> ContractResult<()> {
    require_nonempty(path, field)?;
    for metacharacter in ROOT_METACHARACTERS {
        if path.contains(metacharacter) {
            return err(format!(
                "{field}: glob or device metacharacter `{metacharacter}` makes the root identity unbounded; one physical representation only"
            ));
        }
    }
    if let Some(rest) = path.strip_prefix('/') {
        if rest.contains('\\') {
            return err(format!(
                "{field}: posix form must use the forward-slash separator consistently; found `{path}`"
            ));
        }
        for segment in rest.split('/') {
            reject_root_segment(segment, path, field)?;
        }
        return Ok(());
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 4 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\' {
        let rest = &path[3..];
        if rest.contains('/') {
            return err(format!(
                "{field}: drive form must use the backslash separator consistently; found `{path}`"
            ));
        }
        for segment in rest.split('\\') {
            reject_root_segment(segment, path, field)?;
        }
        return Ok(());
    }
    if path.starts_with("\\\\") && path.len() > 2 {
        let rest = &path[2..];
        if rest.contains('/') {
            return err(format!(
                "{field}: unc form must use the backslash separator consistently; found `{path}`"
            ));
        }
        let segments: Vec<&str> = rest.split('\\').collect();
        if segments.len() < 2 {
            return err(format!(
                "{field}: unc form requires host and share segments; found `{path}`"
            ));
        }
        for segment in segments {
            reject_root_segment(segment, path, field)?;
        }
        return Ok(());
    }
    err(format!(
        "{field}: root identity must be one canonical absolute path (posix `/seg`, drive `X:\\seg`, or unc `\\\\host\\share`) without dot, parent, or empty segments; found `{path}`"
    ))
}

fn validate_relative_path(path: &str, field: &str) -> ContractResult<()> {
    if path.trim().is_empty() {
        return err(format!("{field}: must not be empty"));
    }
    if path.contains('\\') {
        return err(format!(
            "{field}: backslash is not a portable separator; `{path}` is unbounded identity"
        ));
    }
    if path.starts_with('/') {
        return err(format!("{field}: must be relative to the install root; found `{path}`"));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return err(format!(
            "{field}: drive-qualified path escapes the bounded root; found `{path}`"
        ));
    }
    for forbidden in ['*', '?', '[', ']', '{', '}', '"', '<', '>', '|'] {
        if path.contains(forbidden) {
            return err(format!(
                "{field}: glob or shell metacharacter `{forbidden}` makes the target unbounded; exact entries only"
            ));
        }
    }
    for segment in path.split('/') {
        if segment.trim().is_empty() || segment == "." || segment == ".." {
            return err(format!(
                "{field}: empty, `.`, or `..` segment escapes the exact-entry boundary; found `{path}`"
            ));
        }
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &str) -> ContractResult<()> {
    let hex_ok =
        value.len() == 64 && value.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if !hex_ok {
        return err(format!("{field}: expected exactly 64 lowercase hexadecimal characters"));
    }
    Ok(())
}

fn retention_for_class(ownership_class: OwnershipClass) -> RetentionDisposition {
    match ownership_class {
        OwnershipClass::OwnedRequired => RetentionDisposition::RetainRequired,
        OwnershipClass::OwnedRemovable => RetentionDisposition::Removable,
        OwnershipClass::OwnedRetainedForRollback => RetentionDisposition::RetainForRollback,
        OwnershipClass::ForeignOrUserOwned => RetentionDisposition::ForeignPreserve,
        OwnershipClass::PackageManagerOrOtherRoute => RetentionDisposition::ExternalRoutePreserve,
        OwnershipClass::UnknownNotSafeToDelete
        | OwnershipClass::MalformedOrInstrumentFailed
        | OwnershipClass::RunningOrActive => RetentionDisposition::BlockedPendingRevalidation,
    }
}

fn is_marker_role(role: Role) -> bool {
    matches!(role, Role::PathMarker | Role::ProfileMarker | Role::RegistryMarker)
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedStateManifest {
    pub schema_version: String,
    pub claim_boundary: String,
    pub install_root: InstallRoot,
    pub installer: InstallerIdentity,
    pub current_selection: Option<CandidateIdentity>,
    pub previous_candidates: Vec<CandidateIdentity>,
    pub other_roots_observed: Vec<OtherRoot>,
    pub entries: Vec<Entry>,
    pub transactions: Vec<Transaction>,
    pub enumeration: Enumeration,
    pub redaction: Redaction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRoot {
    pub absolute_path: String,
    pub identity_digest_sha256: String,
    pub link_policy: LinkPolicy,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallerIdentity {
    pub kind: InstallerKind,
    pub schema_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateIdentity {
    pub candidate_id: String,
    pub version: String,
    pub artifact_sha256: String,
    pub selection_receipt_path: String,
    pub retained_for_rollback: bool,
}

closed_enum!(OtherRootClassification {
    ForeignOrUserOwned => "foreign_or_user_owned",
    PackageManagerOrOtherRoute => "package_manager_or_other_route"
});

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OtherRoot {
    pub absolute_path: String,
    pub classification: OtherRootClassification,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub role: Role,
    pub relative_path: String,
    pub observed: ObservedState,
    pub ownership_class: OwnershipClass,
    pub identity: EntryIdentity,
    pub process_refs: Vec<ProcessRef>,
    pub user_modified: bool,
    pub retention: RetentionDisposition,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryIdentity {
    pub kind: EntryIdentityKind,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessRef {
    pub kind: ProcessRefKind,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub attempt_id: String,
    pub operation: OperationKind,
    pub outcome: TransactionOutcome,
    pub record_relative_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enumeration {
    pub complete: bool,
    pub instrument: String,
    pub incomplete_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Redaction {
    pub policy: RedactionPolicy,
    pub redacted_fields: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub claim_boundary: String,
    pub bound_subject: BoundSubject,
    pub lifecycle_policy: LifecyclePolicy,
    pub running_process_policy: RunningProcessPolicy,
    pub path_cleanup: PathCleanup,
    pub actions: Vec<Action>,
    pub postconditions: Postconditions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundSubject {
    pub install_root_absolute_path: String,
    pub install_root_digest_sha256: String,
    pub manifest_sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PathCleanup {
    pub mode: PathCleanupMode,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub relative_path: String,
    pub action: ActionKind,
    pub order_index: u64,
    pub verified_identity_sha256: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Postconditions {
    pub fresh_process_proof_required: bool,
    pub verify_entries_absent: Vec<String>,
    pub verify_preserved: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UninstallResult {
    pub schema_version: String,
    pub result: UninstallOutcome,
    pub claim_boundary: String,
    pub plan_id: String,
    pub bound_manifest_sha256: String,
    pub removed_entries: Vec<String>,
    pub preserved_entries: Vec<String>,
    pub failed_entries: Vec<FailedEntry>,
    pub complete_evidence: bool,
    pub activation_state: ActivationState,
    pub retryable: bool,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FailedEntry {
    pub relative_path: String,
    pub stage: FailureStage,
    pub detail: String,
}

pub fn validate_manifest(manifest: &OwnedStateManifest) -> ContractResult<()> {
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return err(format!(
            "manifest.schema_version: expected `{MANIFEST_SCHEMA_VERSION}`, found `{}`",
            manifest.schema_version
        ));
    }
    require_nonempty(&manifest.claim_boundary, "manifest.claim_boundary")?;

    validate_absolute_path(
        &manifest.install_root.absolute_path,
        "manifest.install_root.absolute_path",
    )?;
    validate_sha256(
        &manifest.install_root.identity_digest_sha256,
        "manifest.install_root.identity_digest_sha256",
    )?;
    require_nonempty(&manifest.installer.schema_version, "manifest.installer.schema_version")?;

    let mut candidate_ids: BTreeSet<&str> = BTreeSet::new();
    if let Some(current) = &manifest.current_selection {
        validate_candidate(current, "manifest.current_selection", &mut candidate_ids)?;
        if current.retained_for_rollback {
            return err(
                "manifest.current_selection.retained_for_rollback: the selected candidate is not rollback-retained state",
            );
        }
    }
    for previous in &manifest.previous_candidates {
        validate_candidate(previous, "manifest.previous_candidates[]", &mut candidate_ids)?;
    }

    let mut root_paths: BTreeSet<&str> = BTreeSet::new();
    for other in &manifest.other_roots_observed {
        validate_absolute_path(
            &other.absolute_path,
            "manifest.other_roots_observed[].absolute_path",
        )?;
        if !root_paths.insert(other.absolute_path.as_str()) {
            return err(format!(
                "manifest.other_roots_observed: duplicate observation of `{}`",
                other.absolute_path
            ));
        }
    }

    let mut seen_paths: BTreeSet<&str> = BTreeSet::new();
    for entry in &manifest.entries {
        validate_entry(entry)?;
        if !seen_paths.insert(entry.relative_path.as_str()) {
            return err(format!(
                "manifest.entries: relative path `{}` appears more than once; one observed entry is one row",
                entry.relative_path
            ));
        }
    }

    let mut attempt_ids: BTreeSet<&str> = BTreeSet::new();
    for transaction in &manifest.transactions {
        require_nonempty(&transaction.attempt_id, "manifest.transactions[].attempt_id")?;
        if !attempt_ids.insert(transaction.attempt_id.as_str()) {
            return err(format!(
                "manifest.transactions: duplicate attempt_id `{}`",
                transaction.attempt_id
            ));
        }
        if let Some(record) = &transaction.record_relative_path {
            validate_relative_path(record, "manifest.transactions[].record_relative_path")?;
        }
    }

    require_nonempty(&manifest.enumeration.instrument, "manifest.enumeration.instrument")?;
    if !manifest.enumeration.complete && manifest.enumeration.incomplete_reason.is_none() {
        return err(
            "manifest.enumeration.incomplete_reason: incomplete enumeration must say why; absence from incomplete enumeration is not ownership or safe absence",
        );
    }

    if manifest.redaction.policy == RedactionPolicy::PathsRedacted
        && manifest.redaction.redacted_fields.is_empty()
    {
        return err(
            "manifest.redaction.redacted_fields: paths_redacted policy must name the redacted surfaces",
        );
    }
    for field in &manifest.redaction.redacted_fields {
        require_nonempty(field, "manifest.redaction.redacted_fields[]")?;
    }
    Ok(())
}

fn validate_candidate<'a>(
    candidate: &'a CandidateIdentity,
    field: &str,
    seen_ids: &mut BTreeSet<&'a str>,
) -> ContractResult<()> {
    require_nonempty(&candidate.candidate_id, &format!("{field}.candidate_id"))?;
    if !seen_ids.insert(candidate.candidate_id.as_str()) {
        return err(format!(
            "{field}.candidate_id: `{}` is recorded twice across current and previous candidates",
            candidate.candidate_id
        ));
    }
    require_nonempty(&candidate.version, &format!("{field}.version"))?;
    validate_sha256(&candidate.artifact_sha256, &format!("{field}.artifact_sha256"))?;
    validate_relative_path(
        &candidate.selection_receipt_path,
        &format!("{field}.selection_receipt_path"),
    )
}

fn validate_entry(entry: &Entry) -> ContractResult<()> {
    validate_relative_path(&entry.relative_path, "entry.relative_path")?;

    let running_refs_empty = entry.process_refs.is_empty();
    match entry.ownership_class {
        OwnershipClass::RunningOrActive if running_refs_empty => {
            return err(format!(
                "entry `{}`: running_or_active requires at least one process reference; ambiguous running-state is not removable state",
                entry.relative_path
            ));
        }
        OwnershipClass::RunningOrActive => {}
        _ if !running_refs_empty => {
            return err(format!(
                "entry `{}`: process references on a `{}` row make running-state ambiguous; reclassify as running_or_active before planning",
                entry.relative_path,
                entry.ownership_class.as_str()
            ));
        }
        _ => {}
    }

    let expected_retention = retention_for_class(entry.ownership_class);
    if entry.retention != expected_retention {
        return err(format!(
            "entry `{}`: retention `{}` disagrees with ownership class `{}`; the disposition vocabulary is total and fixed",
            entry.relative_path,
            entry.retention.as_str(),
            entry.ownership_class.as_str()
        ));
    }

    if entry.user_modified && entry.ownership_class != OwnershipClass::ForeignOrUserOwned {
        return err(format!(
            "entry `{}`: a user-edited marker is foreign_or_user_owned, not `{}`",
            entry.relative_path,
            entry.ownership_class.as_str()
        ));
    }

    match (&entry.observed, entry.identity.kind, entry.identity.sha256.as_deref()) {
        (ObservedState::Absent, EntryIdentityKind::Unavailable, None) => {}
        (ObservedState::Absent, _, _) => {
            return err(format!(
                "entry `{}`: an absent entry records identity unavailable; stale digests must not survive absence",
                entry.relative_path
            ));
        }
        (_, EntryIdentityKind::Unavailable, Some(_)) => {
            return err(format!(
                "entry `{}`: unavailable identity must not carry a digest",
                entry.relative_path
            ));
        }
        (ObservedState::Present, EntryIdentityKind::Unavailable, None)
            if matches!(
                entry.ownership_class,
                OwnershipClass::UnknownNotSafeToDelete
                    | OwnershipClass::MalformedOrInstrumentFailed
            ) => {}
        (ObservedState::Present, EntryIdentityKind::Unavailable, None) => {
            return err(format!(
                "entry `{}`: present owned state claims no digest; filename, version, executable bit, or familiar path is not ownership",
                entry.relative_path
            ));
        }
        (ObservedState::Present, EntryIdentityKind::Sha256Content, Some(digest))
        | (ObservedState::Present, EntryIdentityKind::DirectoryTreeDigest, Some(digest)) => {
            validate_sha256(digest, "entry.identity.sha256")?;
        }
        (ObservedState::Present, _, None) => {
            return err(format!(
                "entry `{}`: digest-backed identity kinds require a digest when present",
                entry.relative_path
            ));
        }
    }
    Ok(())
}

pub fn validate_plan_against_current_manifest(
    plan: &RemovalPlan,
    manifest: &OwnedStateManifest,
    current_manifest_sha256: &str,
) -> ContractResult<()> {
    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return err(format!(
            "plan.schema_version: expected `{PLAN_SCHEMA_VERSION}`, found `{}`",
            plan.schema_version
        ));
    }
    require_nonempty(&plan.plan_id, "plan.plan_id")?;
    require_nonempty(&plan.claim_boundary, "plan.claim_boundary")?;

    if plan.bound_subject.install_root_absolute_path != manifest.install_root.absolute_path
        || plan.bound_subject.install_root_digest_sha256
            != manifest.install_root.identity_digest_sha256
    {
        return err(
            "plan.bound_subject: root or manifest identity moved after planning; revalidate or refuse (root_or_manifest_mismatch)",
        );
    }
    if plan.bound_subject.manifest_sha256 != current_manifest_sha256 {
        return err(format!(
            "plan.bound_subject.manifest_sha256: planned against `{}` but current manifest is `{current_manifest_sha256}`; changed state requires revalidation (root_or_manifest_mismatch)",
            plan.bound_subject.manifest_sha256
        ));
    }

    let entries_by_path: BTreeMap<&str, &Entry> =
        manifest.entries.iter().map(|entry| (entry.relative_path.as_str(), entry)).collect();

    let mut planned_paths: BTreeSet<&str> = BTreeSet::new();
    let mut destructive_paths: BTreeSet<&str> = BTreeSet::new();
    let mut preserved_paths: BTreeSet<&str> = BTreeSet::new();
    for action in &plan.actions {
        require_nonempty(&action.reason, "plan.actions[].reason")?;
        let entry = entries_by_path.get(action.relative_path.as_str()).ok_or_else(|| {
            ContractError::new(format!(
                "plan.actions: path `{}` is absent from the bound manifest; plans are total over manifest rows only",
                action.relative_path
            ))
        })?;
        if !planned_paths.insert(action.relative_path.as_str()) {
            return err(format!(
                "plan.actions: path `{}` receives more than one disposition; every manifest row gets exactly one",
                action.relative_path
            ));
        }
        match action.action {
            ActionKind::RemoveExact | ActionKind::RemoveMarker => {
                validate_destructive_action(action, entry, &plan.lifecycle_policy, manifest)?;
                let is_marker_action = action.action == ActionKind::RemoveMarker;
                if is_marker_action != is_marker_role(entry.role) {
                    return err(format!(
                        "plan.actions: `{}` uses `{}` but its manifest role is `{}`",
                        action.relative_path,
                        action.action.as_str(),
                        entry.role.as_str()
                    ));
                }
                destructive_paths.insert(action.relative_path.as_str());
            }
            ActionKind::Preserve => {
                preserved_paths.insert(action.relative_path.as_str());
            }
            ActionKind::Revalidate => {}
        }
    }

    if planned_paths.len() != manifest.entries.len() {
        return err(format!(
            "plan.actions: removal plan is not total over the manifest; {} of {} entries received a disposition",
            planned_paths.len(),
            manifest.entries.len()
        ));
    }

    for (position, action) in plan.actions.iter().enumerate() {
        if action.order_index != position as u64 {
            return err(format!(
                "plan.actions: order_index must be the canonical execution sequence 0..N matching array order exactly; action `{}` carries index {} at position {position}; duplicates, gaps, permutations, and reversal are invalid",
                action.relative_path, action.order_index
            ));
        }
    }

    match plan.path_cleanup.mode {
        PathCleanupMode::Skipped if plan.path_cleanup.entries.is_empty() => {}
        PathCleanupMode::Skipped => {
            return err("plan.path_cleanup: skipped mode must carry no entries");
        }
        PathCleanupMode::ExactOwnedEntriesOnly => {
            if plan.path_cleanup.entries.is_empty() {
                return err(
                    "plan.path_cleanup: exact_owned_entries_only requires the exact owned marker entries",
                );
            }
            for marker in &plan.path_cleanup.entries {
                let entry = entries_by_path.get(marker.as_str()).ok_or_else(|| {
                    ContractError::new(format!(
                        "plan.path_cleanup: `{}` is not a manifest row; PATH cleanup composes with exact ownership only (#11468/#11469)",
                        marker
                    ))
                })?;
                if !is_marker_role(entry.role) {
                    return err(format!(
                        "plan.path_cleanup: `{}` is role `{}`, not a PATH/profile/registry marker",
                        marker,
                        entry.role.as_str()
                    ));
                }
                let paired = plan.actions.iter().any(|action| {
                    action.relative_path == *marker && action.action == ActionKind::RemoveMarker
                });
                if !paired {
                    return err(format!(
                        "plan.path_cleanup: `{}` has no remove_marker action; cleanup and actions must compose exactly",
                        marker
                    ));
                }
            }
        }
    }

    if !plan.postconditions.fresh_process_proof_required {
        return err(
            "plan.postconditions.fresh_process_proof_required: hosted fresh-process proof is mandatory after uninstall",
        );
    }
    let absent_requested = collect_exact_paths(
        &plan.postconditions.verify_entries_absent,
        "plan.postconditions.verify_entries_absent",
    )?;
    if absent_requested != destructive_paths {
        return err(
            "plan.postconditions.verify_entries_absent: must list exactly the destructively removed entries",
        );
    }
    let preserved_requested = collect_exact_paths(
        &plan.postconditions.verify_preserved,
        "plan.postconditions.verify_preserved",
    )?;
    if !absent_requested.is_disjoint(&preserved_requested) {
        return err(
            "plan.postconditions: verify_entries_absent and verify_preserved must not overlap; a path cannot be both removed and preserved postconditions",
        );
    }
    if preserved_requested != preserved_paths {
        return err(
            "plan.postconditions.verify_preserved: must equal the plan's preserved population exactly (every preserve disposition, nothing else); revalidate rows belong to neither postcondition list until they are revalidated",
        );
    }
    Ok(())
}

fn collect_exact_paths<'a>(paths: &'a [String], field: &str) -> ContractResult<BTreeSet<&'a str>> {
    let mut exact = BTreeSet::new();
    for path in paths {
        validate_relative_path(path, field)?;
        if !exact.insert(path.as_str()) {
            return err(format!(
                "{field}: duplicate postcondition path `{path}`; postcondition populations are exact sets"
            ));
        }
    }
    Ok(exact)
}

fn validate_destructive_action(
    action: &Action,
    entry: &Entry,
    lifecycle_policy: &LifecyclePolicy,
    manifest: &OwnedStateManifest,
) -> ContractResult<()> {
    if !manifest.enumeration.complete {
        return err(format!(
            "plan.actions: destructive action on `{}` refused; enumeration was incomplete so ownership and absence are not proven",
            action.relative_path
        ));
    }
    if entry.observed != ObservedState::Present {
        return err(format!(
            "plan.actions: destructive action on `{}` refused; the manifest records it absent (already_absent_owned_state)",
            action.relative_path
        ));
    }
    if entry.ownership_class == OwnershipClass::RunningOrActive {
        return err(format!(
            "plan.actions: destructive action on `{}` refused; running or active state blocks removal (blocked_running)",
            action.relative_path
        ));
    }
    if !entry.process_refs.is_empty() {
        return err(format!(
            "plan.actions: destructive action on `{}` refused; live process references block removal (blocked_running)",
            action.relative_path
        ));
    }
    if !matches!(
        entry.ownership_class,
        OwnershipClass::OwnedRequired
            | OwnershipClass::OwnedRemovable
            | OwnershipClass::OwnedRetainedForRollback
    ) {
        return err(format!(
            "plan.actions: destructive action on `{}` refused; ownership class `{}` is never deleted by name, path, age, count, or familiarity",
            action.relative_path,
            entry.ownership_class.as_str()
        ));
    }
    if entry.ownership_class == OwnershipClass::OwnedRetainedForRollback
        && *lifecycle_policy != LifecyclePolicy::FullRemovalSelected
    {
        return err(format!(
            "plan.actions: `{}` is rollback-retained; removing it requires full_removal_selected while retain_rollback_selected preserves it (rollback and uninstall are distinct)",
            action.relative_path
        ));
    }
    let verified = action.verified_identity_sha256.as_deref().ok_or_else(|| {
        ContractError::new(format!(
            "plan.actions: destructive action on `{}` requires verified_identity_sha256; exact currentness binds destruction",
            action.relative_path
        ))
    })?;
    let recorded = entry.identity.sha256.as_deref().unwrap_or_default();
    if verified != recorded {
        return err(format!(
            "plan.actions: verified identity for `{}` does not match the manifest record; exact currentness failed",
            action.relative_path
        ));
    }
    Ok(())
}

pub fn validate_result(outcome: &UninstallResult) -> ContractResult<()> {
    if outcome.schema_version != RESULT_SCHEMA_VERSION {
        return err(format!(
            "result.schema_version: expected `{RESULT_SCHEMA_VERSION}`, found `{}`",
            outcome.schema_version
        ));
    }
    require_nonempty(&outcome.claim_boundary, "result.claim_boundary")?;
    require_nonempty(&outcome.plan_id, "result.plan_id")?;
    validate_sha256(&outcome.bound_manifest_sha256, "result.bound_manifest_sha256")?;
    for limitation in &outcome.limitations {
        require_nonempty(limitation, "result.limitations[]")?;
    }

    let failures_present = !outcome.failed_entries.is_empty();
    match outcome.result {
        UninstallOutcome::PartialFailure if failures_present => {}
        UninstallOutcome::PartialFailure => {
            return err(
                "result: partial_failure requires at least one failed_entry; partial failure stays explicit",
            );
        }
        UninstallOutcome::PathCleanupFailed => {
            if !failures_present {
                return err(
                    "result: path_cleanup_failed requires failures at the marker_cleanup stage",
                );
            }
            if !outcome
                .failed_entries
                .iter()
                .all(|failure| failure.stage == FailureStage::MarkerCleanup)
            {
                return err(
                    "result: path_cleanup_failed failures must all be marker_cleanup stage",
                );
            }
            if outcome.removed_entries.is_empty() {
                return err(
                    "result: path_cleanup_failed implies owned state was removed before PATH cleanup failed",
                );
            }
        }
        UninstallOutcome::AlreadyAbsentOwnedState => {
            if failures_present || !outcome.removed_entries.is_empty() {
                return err("result: already_absent_owned_state removes nothing and fails nothing");
            }
            if !outcome.preserved_entries.is_empty() {
                return err(
                    "result: already_absent_owned_state verifies absence only; it must not claim preserved rows",
                );
            }
            if !outcome.complete_evidence {
                return err(
                    "result: already_absent_owned_state requires complete_evidence; missing manifest is not automatically clean absence",
                );
            }
        }
        UninstallOutcome::Removed => {
            if failures_present {
                return err(
                    "result: partial failure must never become success; removed results carry no failed entries",
                );
            }
            if outcome.removed_entries.is_empty() {
                return err("result: removed requires at least one removed entry");
            }
            if !outcome.complete_evidence {
                return err(
                    "result: removed requires complete_evidence covering enumeration and postconditions",
                );
            }
            if outcome.retryable {
                return err(
                    "result: completed removal is not retryable; idempotent rerun reports already_absent_owned_state",
                );
            }
        }
        UninstallOutcome::NotApplicable => {
            if outcome.activation_state != ActivationState::ConditionalActivationSelected {
                return err(
                    "result: not_applicable requires #11417 conditional activation selection; issue existence alone never activates the lifecycle claim",
                );
            }
            if !outcome.preserved_entries.is_empty() {
                return err("result: not_applicable ran nothing; it must not claim preserved rows");
            }
        }
        UninstallOutcome::BlockedRunning
        | UninstallOutcome::BlockedUnknownOrForeign
        | UninstallOutcome::RootOrManifestMismatch
        | UninstallOutcome::Cancelled
        | UninstallOutcome::InstrumentFailure
        | UninstallOutcome::NotProven => {
            if !outcome.removed_entries.is_empty() {
                return err(format!(
                    "result: `{}` must not report removed entries; blocked and unproven outcomes delete nothing",
                    outcome.result.as_str()
                ));
            }
            if !outcome.preserved_entries.is_empty() {
                return err(format!(
                    "result: `{}` executed nothing; preserved_entries must be empty because no postcondition was evaluated",
                    outcome.result.as_str()
                ));
            }
            if failures_present {
                return err(format!(
                    "result: `{}` carries no partial-failure entries; use partial_failure",
                    outcome.result.as_str()
                ));
            }
        }
    }

    for failure in &outcome.failed_entries {
        require_nonempty(&failure.detail, "result.failed_entries[].detail")?;
        validate_relative_path(&failure.relative_path, "result.failed_entries[].relative_path")?;
    }
    for path in outcome.removed_entries.iter().chain(outcome.preserved_entries.iter()) {
        validate_relative_path(path, "result.removed/preserved_entries[]")?;
    }

    let mut removed_paths: BTreeSet<&str> = BTreeSet::new();
    for path in &outcome.removed_entries {
        if !removed_paths.insert(path.as_str()) {
            return err(format!(
                "result.removed_entries: duplicate entry `{path}`; reported populations are exact sets"
            ));
        }
    }
    let mut preserved_reported: BTreeSet<&str> = BTreeSet::new();
    for path in &outcome.preserved_entries {
        if !preserved_reported.insert(path.as_str()) {
            return err(format!(
                "result.preserved_entries: duplicate entry `{path}`; reported populations are exact sets"
            ));
        }
    }
    let mut failed_paths: BTreeSet<&str> = BTreeSet::new();
    for failure in &outcome.failed_entries {
        if !failed_paths.insert(failure.relative_path.as_str()) {
            return err(format!(
                "result.failed_entries: duplicate failure for `{}`; report one failure per entry",
                failure.relative_path
            ));
        }
    }
    if let Some(path) = removed_paths.intersection(&preserved_reported).next() {
        return err(format!(
            "result: entry `{path}` is reported both removed and preserved; the populations must not overlap"
        ));
    }
    if let Some(path) = removed_paths.intersection(&failed_paths).next() {
        return err(format!(
            "result: entry `{path}` is reported both removed and failed; a row is either removed or an explicit failure, never both"
        ));
    }
    if let Some(path) = preserved_reported.intersection(&failed_paths).next() {
        return err(format!(
            "result: entry `{path}` is reported both preserved and failed; failures name the work that did not settle, and stay in failed_entries only"
        ));
    }
    Ok(())
}

/// Binds one uninstall result to the exact validated plan and current manifest
/// observation (#11470): plan identity, manifest identity, and per-row
/// reconciliation between what was reported and what the plan admitted.
pub fn validate_result_against_plan(
    outcome: &UninstallResult,
    plan: &RemovalPlan,
    manifest: &OwnedStateManifest,
    current_manifest_sha256: &str,
) -> ContractResult<()> {
    if outcome.plan_id != plan.plan_id {
        return err(format!(
            "result.plan_id: binds plan `{}` but was reported against plan `{}`; results bind the exact validated plan",
            outcome.plan_id, plan.plan_id
        ));
    }
    if outcome.bound_manifest_sha256 != plan.bound_subject.manifest_sha256 {
        return err(
            "result.bound_manifest_sha256: the result and its plan bind different manifest digests; the report and its plan disagree",
        );
    }
    if plan.bound_subject.manifest_sha256 != current_manifest_sha256
        || plan.bound_subject.install_root_absolute_path != manifest.install_root.absolute_path
        || plan.bound_subject.install_root_digest_sha256
            != manifest.install_root.identity_digest_sha256
    {
        return err(
            "result: the plan's subject moved since planning; revalidation refuses with root_or_manifest_mismatch before any result may bind",
        );
    }

    let mut destructive: BTreeMap<&str, ActionKind> = BTreeMap::new();
    let mut preserve_dispositions: BTreeSet<&str> = BTreeSet::new();
    for action in &plan.actions {
        match action.action {
            ActionKind::RemoveExact | ActionKind::RemoveMarker => {
                destructive.insert(action.relative_path.as_str(), action.action);
            }
            ActionKind::Preserve => {
                preserve_dispositions.insert(action.relative_path.as_str());
            }
            ActionKind::Revalidate => {}
        }
    }

    let removed: BTreeSet<&str> = outcome.removed_entries.iter().map(String::as_str).collect();
    let preserved_reported: BTreeSet<&str> =
        outcome.preserved_entries.iter().map(String::as_str).collect();
    let failed_paths: BTreeSet<&str> =
        outcome.failed_entries.iter().map(|failure| failure.relative_path.as_str()).collect();

    for path in &removed {
        if !destructive.contains_key(path) {
            return err(format!(
                "result.removed_entries: `{path}` reconciles to no destructive action in the bound plan; removed rows must be planned destructive work (remove_exact/remove_marker)"
            ));
        }
    }
    for path in &failed_paths {
        if !destructive.contains_key(path) && !preserve_dispositions.contains(path) {
            return err(format!(
                "result.failed_entries: `{path}` is not a planned path in the bound plan; failures reconcile to admitted actions only"
            ));
        }
    }

    match outcome.result {
        UninstallOutcome::Removed
        | UninstallOutcome::PartialFailure
        | UninstallOutcome::PathCleanupFailed => {
            if preserved_reported != preserve_dispositions {
                return err(
                    "result.preserved_entries: must equal the bound plan's preserve disposition population exactly for an executed outcome",
                );
            }
            for path in destructive.keys() {
                if !removed.contains(path) && !failed_paths.contains(path) {
                    return err(format!(
                        "result: planned destructive action `{path}` appears in neither removed nor failed entries; partial state must stay explicit"
                    ));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let members: Vec<String> = keys
                .iter()
                .map(|key| {
                    let rendered_key =
                        serde_json::to_string(key).unwrap_or_else(|_| format!("\"{key}\""));
                    format!("{rendered_key}:{}", canonical_json(&map[key.as_str()]))
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

#[derive(Debug, Parser)]
#[command(name = "standalone-owned-state")]
#[command(
    about = "Validate standalone owned-state manifests, pure removal plans, and uninstall results (#11470); validates documents and deletes nothing"
)]
struct Args {
    /// standalone_owned_state.v1 document to validate.
    #[arg(long)]
    manifest: Option<PathBuf>,
    /// standalone_removal_plan.v1 document; requires --manifest.
    #[arg(long)]
    plan: Option<PathBuf>,
    /// standalone_uninstall_result.v1 document; with --plan, binds against the exact validated plan.
    #[arg(long)]
    result: Option<PathBuf>,
    /// Print the manifest in canonical key-sorted serialization after validation.
    #[arg(long)]
    print_canonical: bool,
}

fn load_json(path: &std::path::Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn load_and_validate_manifest(path: &std::path::Path) -> Result<(OwnedStateManifest, String)> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let manifest: OwnedStateManifest =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    validate_manifest(&manifest).with_context(|| "validating owned-state manifest")?;
    Ok((manifest, sha256_hex(&bytes)))
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    if args.manifest.is_none() && args.plan.is_none() && args.result.is_none() {
        bail!("nothing to validate: pass --manifest, --plan (with --manifest), or --result");
    }

    if let Some(manifest_path) = &args.manifest {
        let (_, digest) = load_and_validate_manifest(manifest_path)?;
        println!("standalone-owned-state: manifest valid ({})", &digest[..16]);
        if args.print_canonical {
            let value = load_json(manifest_path)?;
            println!("{}", canonical_json(&value));
        }
    }

    let mut validated_plan: Option<RemovalPlan> = None;
    if let Some(plan_path) = &args.plan {
        let manifest_path = args
            .manifest
            .as_ref()
            .ok_or_else(|| color_eyre::eyre::eyre!("--plan requires --manifest to bind against"))?;
        let (manifest, digest) = load_and_validate_manifest(manifest_path)?;
        let plan_bytes =
            fs::read(plan_path).with_context(|| format!("reading {}", plan_path.display()))?;
        let plan: RemovalPlan = serde_json::from_slice(&plan_bytes)
            .with_context(|| format!("parsing {}", plan_path.display()))?;
        validate_plan_against_current_manifest(&plan, &manifest, &digest)
            .with_context(|| "validating removal plan against current manifest")?;
        println!("standalone-owned-state: plan valid ({})", plan.plan_id);
        validated_plan = Some(plan);
    }

    if let Some(result_path) = &args.result {
        let result_bytes =
            fs::read(result_path).with_context(|| format!("reading {}", result_path.display()))?;
        let outcome: UninstallResult = serde_json::from_slice(&result_bytes)
            .with_context(|| format!("parsing {}", result_path.display()))?;
        validate_result(&outcome).with_context(|| "validating uninstall result")?;
        match (&validated_plan, args.manifest.as_ref()) {
            (Some(plan), Some(manifest_path)) => {
                let (manifest, digest) = load_and_validate_manifest(manifest_path)?;
                validate_result_against_plan(&outcome, plan, &manifest, &digest)
                    .with_context(|| "validating uninstall result against its bound plan")?;
            }
            (_, Some(manifest_path)) => {
                let (_, digest) = load_and_validate_manifest(manifest_path)?;
                if outcome.bound_manifest_sha256 != digest {
                    bail!(
                        "result binds manifest {} but current manifest is {}; the claimed state moved",
                        &outcome.bound_manifest_sha256[..16],
                        &digest[..16]
                    );
                }
            }
            _ => {}
        }
        println!("standalone-owned-state: result valid ({})", outcome.result.as_str());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ActionKind, ActivationState, ContractResult, FailureStage, LifecyclePolicy,
        OwnedStateManifest, RemovalPlan, UninstallOutcome, UninstallResult, canonical_json,
        sha256_hex, validate_manifest, validate_plan_against_current_manifest, validate_result,
        validate_result_against_plan,
    };
    use color_eyre::eyre::{Result, bail, ensure};
    use serde_json::Value;

    const MANIFEST_DIR: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/experience/install_owned_state");

    fn canonical_manifest_text() -> &'static str {
        include_str!(
            "../../fixtures/experience/install_owned_state/manifest_canonical_full_install.json"
        )
    }

    fn parse_manifest(text: &str) -> ContractResult<OwnedStateManifest> {
        serde_json::from_str(text)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))
    }

    fn parse_plan(text: &str) -> ContractResult<RemovalPlan> {
        serde_json::from_str(text)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))
    }

    fn parse_result(text: &str) -> ContractResult<UninstallResult> {
        serde_json::from_str(text)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))
    }

    fn fixture_text(name: &str) -> Result<String> {
        std::fs::read_to_string(format!("{MANIFEST_DIR}/{name}"))
            .map_err(|error| color_eyre::eyre::eyre!("fixture {name} unreadable: {error}"))
    }

    fn load_fixture_text(name: &str) -> ContractResult<String> {
        std::fs::read_to_string(format!("{MANIFEST_DIR}/{name}")).map_err(|error| {
            super::ContractError::new(format!("fixture {name} unreadable: {error}"))
        })
    }

    fn mutated_document<T: serde::de::DeserializeOwned>(
        name: &str,
        mutate: impl FnOnce(&mut Value),
    ) -> ContractResult<T> {
        let mut value: Value = serde_json::from_str(&load_fixture_text(name)?)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
        mutate(&mut value);
        serde_json::from_value(value)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))
    }

    fn canonical_digest() -> Result<String> {
        Ok(sha256_hex(canonical_manifest_text().as_bytes()))
    }

    fn running_digest() -> Result<String> {
        Ok(sha256_hex(fixture_text("manifest_running_current.json")?.as_bytes()))
    }

    #[track_caller]
    fn expect_rejected(outcome: ContractResult<()>, needle: &str) -> Result<()> {
        match outcome {
            Ok(()) => bail!("expected rejection mentioning {needle:?}, but validation passed"),
            Err(error) => {
                let rendered = error.to_string();
                ensure!(
                    rendered.contains(needle),
                    "expected rejection mentioning {needle:?}, got: {rendered}"
                );
                Ok(())
            }
        }
    }

    fn mutated_canonical(mutate: impl FnOnce(&mut Value)) -> ContractResult<OwnedStateManifest> {
        let mut value: Value = serde_json::from_str(canonical_manifest_text())
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
        mutate(&mut value);
        serde_json::from_value(value)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))
    }

    fn entry_mut<'a>(value: &'a mut Value, relative_path: &str) -> Option<&'a mut Value> {
        value["entries"]
            .as_array_mut()?
            .iter_mut()
            .find(|entry| entry["relative_path"] == serde_json::json!(relative_path))
    }

    const D_A1: &str = "d0a1000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn canonical_and_scenario_manifests_validate() -> Result<()> {
        for name in [
            "manifest_canonical_full_install.json",
            "manifest_running_current.json",
            "manifest_symlink_substitution.json",
            "manifest_instrument_failed.json",
            "manifest_partial_deletion_retry.json",
            "manifest_user_edited_path.json",
        ] {
            let manifest = parse_manifest(&fixture_text(name)?)?;
            validate_manifest(&manifest).map_err(|error| {
                color_eyre::eyre::eyre!("fixture {name} must validate: {error}")
            })?;
        }
        Ok(())
    }

    #[test]
    fn invalid_manifest_fixtures_fail_for_named_reasons() -> Result<()> {
        let unknown_role_text = fixture_text("manifest_invalid_unknown_role.json")?;
        let unknown_role = parse_manifest(&unknown_role_text);
        match unknown_role {
            Ok(_) => bail!("unknown role must not parse as valid manifest"),
            Err(error) => ensure!(
                error.to_string().contains("unknown variant"),
                "unknown-role rejection must name the variant problem: {error}"
            ),
        }

        let unbounded = parse_manifest(&fixture_text("manifest_invalid_unbounded_identity.json")?)?;
        expect_rejected(validate_manifest(&unbounded), "unbounded")?;

        let ambiguous = parse_manifest(&fixture_text("manifest_invalid_ambiguous_running.json")?)?;
        expect_rejected(validate_manifest(&ambiguous), "ambiguous")?;
        Ok(())
    }

    #[test]
    fn malformed_manifest_mutations_fail_closed() -> Result<()> {
        let bad_version = mutated_canonical(|value| {
            value["schema_version"] = serde_json::json!("standalone_owned_state.v2");
        })?;
        expect_rejected(validate_manifest(&bad_version), "schema_version")?;

        let absolute_entry = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, ".perllsp-path-marker") {
                entry["relative_path"] = serde_json::json!("/etc/perllsp-marker");
            }
        })?;
        expect_rejected(validate_manifest(&absolute_entry), "relative to the install root")?;

        let parent_escape = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, ".perllsp-profile-marker") {
                entry["relative_path"] = serde_json::json!("../../home");
            }
        })?;
        expect_rejected(validate_manifest(&parent_escape), "exact-entry boundary")?;

        let stale_digest = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, "current") {
                entry["identity"]["sha256"] = serde_json::json!("ZZZ");
            }
        })?;
        expect_rejected(validate_manifest(&stale_digest), "lowercase hexadecimal")?;

        let running_without_refs = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, "candidates/v0.18.0-x86_64-unknown-linux-gnu") {
                entry["ownership_class"] = serde_json::json!("running_or_active");
                entry["retention"] = serde_json::json!("blocked_pending_revalidation");
                entry["process_refs"] = serde_json::json!([]);
            }
        })?;
        expect_rejected(
            validate_manifest(&running_without_refs),
            "requires at least one process reference",
        )?;

        let removable_with_refs = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, ".perllsp-path-marker") {
                entry["process_refs"] = serde_json::json!([{"kind": "pid", "value": "9"}]);
            }
        })?;
        expect_rejected(validate_manifest(&removable_with_refs), "ambiguous")?;

        let duplicate_row = mutated_canonical(|value| {
            if let Some(entries) = value["entries"].as_array_mut() {
                let clone = entries[0].clone();
                entries.push(clone);
            }
        })?;
        expect_rejected(validate_manifest(&duplicate_row), "more than once")?;

        let retention_drift = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, "notes.txt") {
                entry["retention"] = serde_json::json!("removable");
            }
        })?;
        expect_rejected(validate_manifest(&retention_drift), "disagrees with ownership class")?;

        let user_edited_owned = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, ".perllsp-path-marker") {
                entry["user_modified"] = serde_json::json!(true);
            }
        })?;
        expect_rejected(validate_manifest(&user_edited_owned), "foreign_or_user_owned")?;

        let incomplete_without_reason = mutated_canonical(|value| {
            value["enumeration"]["complete"] = serde_json::json!(false);
            value["enumeration"]["incomplete_reason"] = serde_json::Value::Null;
        })?;
        expect_rejected(
            validate_manifest(&incomplete_without_reason),
            "not ownership or safe absence",
        )?;

        let absent_row_keeps_digest = mutated_canonical(|value| {
            if let Some(entry) = entry_mut(value, "current") {
                entry["observed"] = serde_json::json!("absent");
            }
        })?;
        expect_rejected(validate_manifest(&absent_row_keeps_digest), "absent entry records")?;

        let redacted_without_surfaces = mutated_canonical(|value| {
            value["redaction"]["policy"] = serde_json::json!("paths_redacted");
        })?;
        expect_rejected(
            validate_manifest(&redacted_without_surfaces),
            "must name the redacted surfaces",
        )?;

        let colliding_candidates = mutated_canonical(|value| {
            value["previous_candidates"][0]["candidate_id"] =
                value["current_selection"]["candidate_id"].clone();
        })?;
        expect_rejected(validate_manifest(&colliding_candidates), "recorded twice")?;

        Ok(())
    }

    #[test]
    fn plan_fixtures_bind_and_validate_against_current_manifests() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let full = parse_plan(&fixture_text("plan_full_removal.json")?)?;
        validate_plan_against_current_manifest(&full, &canonical, &canonical_digest()?)
            .map_err(|error| color_eyre::eyre::eyre!("full-removal plan must validate: {error}"))?;

        let retained = parse_plan(&fixture_text("plan_rollback_retained.json")?)?;
        validate_plan_against_current_manifest(&retained, &canonical, &canonical_digest()?)
            .map_err(|error| {
                color_eyre::eyre::eyre!("rollback-retaining plan must validate: {error}")
            })?;

        let running = parse_manifest(&fixture_text("manifest_running_current.json")?)?;
        let blocked = parse_plan(&fixture_text("plan_blocked_running_all_preserve.json")?)?;
        validate_plan_against_current_manifest(&blocked, &running, &running_digest()?).map_err(
            |error| color_eyre::eyre::eyre!("blocked-running plan must validate: {error}"),
        )?;
        Ok(())
    }

    #[test]
    fn stale_binding_is_refused() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let stale = parse_plan(&fixture_text("plan_invalid_stale_binding.json")?)?;
        expect_rejected(
            validate_plan_against_current_manifest(&stale, &canonical, &canonical_digest()?),
            "root_or_manifest_mismatch",
        )?;
        Ok(())
    }

    #[test]
    fn plan_totality_holds_in_both_directions() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;

        let dropped: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["actions"].as_array_mut().map(Vec::pop);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&dropped, &canonical, &canonical_digest()?),
            "not total over the manifest",
        )?;

        let foreign_target: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            if let Some(actions) = value["actions"].as_array_mut() {
                actions.push(serde_json::json!({
                    "relative_path": "outside-the-root.txt",
                    "action": "remove_exact",
                    "order_index": 99,
                    "reason": "name familiarity is not ownership"
                }));
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(
                &foreign_target,
                &canonical,
                &canonical_digest()?,
            ),
            "absent from the bound manifest",
        )?;
        Ok(())
    }

    #[test]
    fn destructive_actions_require_exact_owned_currentness() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;

        let delete_user_file: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            for action in value["actions"].as_array_mut().into_iter().flatten() {
                if action["relative_path"] == serde_json::json!("notes.txt") {
                    action["action"] = serde_json::json!("remove_exact");
                    action["verified_identity_sha256"] = serde_json::json!(
                        "d0ac000000000000000000000000000000000000000000000000000000000000"
                    );
                }
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(
                &delete_user_file,
                &canonical,
                &canonical_digest()?,
            ),
            "never deleted by name, path, age, count, or familiarity",
        )?;

        let wrong_digest: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            for action in value["actions"].as_array_mut().into_iter().flatten() {
                if action["relative_path"] == serde_json::json!("current") {
                    action["verified_identity_sha256"] = serde_json::json!(D_A1);
                }
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&wrong_digest, &canonical, &canonical_digest()?),
            "exact currentness failed",
        )?;

        let missing_verification: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            for action in value["actions"].as_array_mut().into_iter().flatten() {
                if action["relative_path"] == serde_json::json!("current") {
                    action.as_object_mut().map(|object| object.remove("verified_identity_sha256"));
                }
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(
                &missing_verification,
                &canonical,
                &canonical_digest()?,
            ),
            "requires verified_identity_sha256",
        )?;

        Ok(())
    }

    #[test]
    fn rollback_retained_rows_need_full_removal_policy() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let mut retained = parse_plan(&fixture_text("plan_rollback_retained.json")?)?;
        retained.lifecycle_policy = LifecyclePolicy::FullRemovalSelected;
        validate_plan_against_current_manifest(&retained, &canonical, &canonical_digest()?)
            .map_err(|error| {
                color_eyre::eyre::eyre!("full removal may drop rollback rows: {error}")
            })?;

        let mut keep_policy_removes_prev = parse_plan(&fixture_text("plan_full_removal.json")?)?;
        keep_policy_removes_prev.lifecycle_policy = LifecyclePolicy::RetainRollbackSelected;
        expect_rejected(
            validate_plan_against_current_manifest(
                &keep_policy_removes_prev,
                &canonical,
                &canonical_digest()?,
            ),
            "rollback and uninstall are distinct",
        )?;
        Ok(())
    }

    #[test]
    fn running_manifest_blocks_every_destructive_action() -> Result<()> {
        let running = parse_manifest(&fixture_text("manifest_running_current.json")?)?;

        let aggressive: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_blocked_running_all_preserve.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            for action in value["actions"].as_array_mut().into_iter().flatten() {
                if action["relative_path"]
                    == serde_json::json!("candidates/v0.18.0-x86_64-unknown-linux-gnu")
                {
                    action["action"] = serde_json::json!("remove_exact");
                    action["verified_identity_sha256"] = serde_json::json!(D_A1);
                }
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&aggressive, &running, &running_digest()?),
            "blocked_running",
        )?;
        Ok(())
    }

    #[test]
    fn incomplete_enumeration_blocks_destruction() -> Result<()> {
        let instrument_failed = parse_manifest(&fixture_text("manifest_instrument_failed.json")?)?;
        let mut any_plan: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["bound_subject"]["manifest_sha256"] = serde_json::json!(sha256_hex(
                fixture_text("manifest_instrument_failed.json")?.as_bytes()
            ));
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        any_plan.bound_subject.install_root_absolute_path =
            instrument_failed.install_root.absolute_path.clone();
        expect_rejected(
            validate_plan_against_current_manifest(
                &any_plan,
                &instrument_failed,
                &sha256_hex(fixture_text("manifest_instrument_failed.json")?.as_bytes()),
            ),
            "enumeration was incomplete",
        )?;
        Ok(())
    }

    #[test]
    fn path_cleanup_composes_only_with_exact_owned_markers() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let running = parse_manifest(&fixture_text("manifest_running_current.json")?)?;

        let cleanup_foreign: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["path_cleanup"]["entries"] = serde_json::json!(["notes.txt"]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(
                &cleanup_foreign,
                &canonical,
                &canonical_digest()?,
            ),
            "not a PATH/profile/registry marker",
        )?;

        let skipped_with_entries: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_blocked_running_all_preserve.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["path_cleanup"]["entries"] = serde_json::json!([".perllsp-path-marker"]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(
                &skipped_with_entries,
                &running,
                &running_digest()?,
            ),
            "skipped mode must carry no entries",
        )?;
        Ok(())
    }

    #[test]
    fn postconditions_cannot_drop_fresh_process_proof() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let weakened: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["postconditions"]["fresh_process_proof_required"] = serde_json::json!(false);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&weakened, &canonical, &canonical_digest()?),
            "fresh-process proof is mandatory",
        )?;

        let uncovered: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["postconditions"]["verify_entries_absent"] = serde_json::json!(["current"]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&uncovered, &canonical, &canonical_digest()?),
            "exactly the destructively removed entries",
        )?;
        Ok(())
    }

    #[test]
    fn result_fixtures_are_coherent() -> Result<()> {
        let partial = parse_result(&fixture_text("result_partial_failure_retryable.json")?)?;
        validate_result(&partial)
            .map_err(|error| color_eyre::eyre::eyre!("partial fixture must validate: {error}"))?;
        let already_absent =
            parse_result(&fixture_text("result_already_absent_complete_evidence.json")?)?;
        validate_result(&already_absent).map_err(|error| {
            color_eyre::eyre::eyre!("already-absent fixture must validate: {error}")
        })?;
        Ok(())
    }

    #[test]
    fn result_vocabulary_fails_closed_on_contradictions() -> Result<()> {
        let base_text = fixture_text("result_partial_failure_retryable.json")?;

        let silent_success: UninstallResult = {
            let mut value: Value = serde_json::from_str(&base_text)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["result"] = serde_json::json!("removed");
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(validate_result(&silent_success), "must never become success")?;

        let evidence_free_absence: UninstallResult = {
            let mut value: Value = serde_json::from_str(&base_text)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["result"] = serde_json::json!("already_absent_owned_state");
            value["failed_entries"] = serde_json::json!([]);
            value["removed_entries"] = serde_json::json!([]);
            value["preserved_entries"] = serde_json::json!([]);
            value["complete_evidence"] = serde_json::json!(false);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_result(&evidence_free_absence),
            "missing manifest is not automatically clean absence",
        )?;

        let premature_not_applicable: UninstallResult = {
            let mut value: Value = serde_json::from_str(&base_text)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["result"] = serde_json::json!("not_applicable");
            value["failed_entries"] = serde_json::json!([]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_result(&premature_not_applicable),
            "#11417 conditional activation",
        )?;

        let partial_without_failures: UninstallResult = {
            let mut value: Value = serde_json::from_str(&base_text)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["failed_entries"] = serde_json::json!([]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_result(&partial_without_failures),
            "partial failure stays explicit",
        )?;

        let blocked_but_removed: UninstallResult = {
            let mut value: Value = serde_json::from_str(&base_text)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["result"] = serde_json::json!("blocked_running");
            value["failed_entries"] = serde_json::json!([]);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(validate_result(&blocked_but_removed), "delete nothing")?;

        let retryable_completed: UninstallResult = {
            let mut value: Value = serde_json::from_str(&fixture_text(
                "result_already_absent_complete_evidence.json",
            )?)
            .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            value["result"] = serde_json::json!("removed");
            value["removed_entries"] = serde_json::json!(["current"]);
            value["retryable"] = serde_json::json!(true);
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(validate_result(&retryable_completed), "already_absent_owned_state")?;

        Ok(())
    }

    #[test]
    fn deterministic_serialization_is_stable_and_key_sorted() -> Result<()> {
        let first: Value = serde_json::from_str(canonical_manifest_text())
            .map_err(|error| color_eyre::eyre::eyre!("fixture parse: {error}"))?;
        let second: Value = serde_json::from_str(&serde_json::to_string(&first)?)?;
        let rendered_once = canonical_json(&first);
        let rendered_twice = canonical_json(&second);
        ensure!(rendered_once == rendered_twice, "canonical serialization must be deterministic");
        let claim = rendered_once.find("\"claim_boundary\":");
        let schema = rendered_once.find("\"schema_version\":");
        match (claim, schema) {
            (Some(c), Some(s)) if c < s => {}
            other => bail!("canonical output must sort keys; got ordering {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn action_kind_marker_split_matches_roles() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;
        let swapped: RemovalPlan = {
            let mut value: Value =
                serde_json::from_str(&fixture_text("plan_full_removal.json")?)
                    .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?;
            for action in value["actions"].as_array_mut().into_iter().flatten() {
                if action["relative_path"] == serde_json::json!(".perllsp-path-marker") {
                    action["action"] = serde_json::json!("remove_exact");
                }
            }
            serde_json::from_value(value)
                .map_err(|error| super::ContractError::new(format!("parse error: {error}")))?
        };
        expect_rejected(
            validate_plan_against_current_manifest(&swapped, &canonical, &canonical_digest()?),
            "but its manifest role is",
        )?;

        assert_ne!(ActionKind::RemoveExact.as_str(), ActionKind::RemoveMarker.as_str());
        assert_eq!(
            ActivationState::ConditionalActivationSelected.as_str(),
            "conditional_activation_selected"
        );
        assert_eq!(
            UninstallOutcome::AlreadyAbsentOwnedState.as_str(),
            "already_absent_owned_state"
        );
        assert_eq!(FailureStage::MarkerCleanup.as_str(), "marker_cleanup");
        assert_eq!(LifecyclePolicy::RetainRollbackSelected.as_str(), "retain_rollback_selected");
        Ok(())
    }

    #[test]
    fn canonical_absolute_roots_are_the_only_accepted_identity() -> Result<()> {
        expect_rejected(
            validate_manifest(&parse_manifest(&fixture_text(
                "manifest_invalid_traversing_root.json",
            )?)?),
            "traversal spellings are invalid",
        )?;

        let traversing = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] =
                    serde_json::json!("/home/alice/.local/share/../etc/perllsp");
            },
        )?;
        expect_rejected(validate_manifest(&traversing), "traversal spellings are invalid")?;

        let double_slash = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] =
                    serde_json::json!("//home/alice/.local/share/perllsp");
            },
        )?;
        expect_rejected(validate_manifest(&double_slash), "non-canonical equivalent spelling")?;

        let trailing_separator = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] =
                    serde_json::json!("/home/alice/.local/share/perllsp/");
            },
        )?;
        expect_rejected(
            validate_manifest(&trailing_separator),
            "non-canonical equivalent spelling",
        )?;

        let drive_traversal = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] =
                    serde_json::json!("C:\\perllsp\\..\\Windows");
            },
        )?;
        expect_rejected(validate_manifest(&drive_traversal), "traversal spellings are invalid")?;

        let forward_slash_drive = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] = serde_json::json!("C:/perllsp");
            },
        )?;
        expect_rejected(validate_manifest(&forward_slash_drive), "one canonical absolute path")?;

        let device_path = mutated_document::<OwnedStateManifest>(
            "manifest_canonical_full_install.json",
            |value| {
                value["install_root"]["absolute_path"] = serde_json::json!("\\\\?\\C:\\perllsp");
            },
        )?;
        expect_rejected(validate_manifest(&device_path), "metacharacter")?;

        for canonical_root in
            ["/opt/perllsp", "C:\\Users\\alice\\.perllsp", "\\\\file.corp\\share\\perllsp"]
        {
            let valid = mutated_document::<OwnedStateManifest>(
                "manifest_canonical_full_install.json",
                |value| value["install_root"]["absolute_path"] = serde_json::json!(canonical_root),
            )?;
            validate_manifest(&valid).map_err(|error| {
                color_eyre::eyre::eyre!("canonical root `{canonical_root}` must validate: {error}")
            })?;
        }
        Ok(())
    }

    #[test]
    fn order_index_is_the_enforced_canonical_sequence() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;

        let duplicate_index: RemovalPlan = mutated_document("plan_full_removal.json", |value| {
            if let Some(actions) = value["actions"].as_array_mut() {
                actions[5]["order_index"] = serde_json::json!(4);
            }
        })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &duplicate_index,
                &canonical,
                &canonical_digest()?,
            ),
            "canonical execution sequence",
        )?;

        let permutation: RemovalPlan = mutated_document("plan_full_removal.json", |value| {
            if let Some(actions) = value["actions"].as_array_mut() {
                let moved = actions[2]["order_index"].take();
                actions[2]["order_index"] = actions[3]["order_index"].clone();
                actions[3]["order_index"] = moved;
            }
        })?;
        expect_rejected(
            validate_plan_against_current_manifest(&permutation, &canonical, &canonical_digest()?),
            "canonical execution sequence",
        )?;

        let gap: RemovalPlan = mutated_document("plan_full_removal.json", |value| {
            if let Some(actions) = value["actions"].as_array_mut() {
                let last = actions.len() - 1;
                actions[last]["order_index"] = serde_json::json!(99);
            }
        })?;
        expect_rejected(
            validate_plan_against_current_manifest(&gap, &canonical, &canonical_digest()?),
            "canonical execution sequence",
        )?;

        let reversed: RemovalPlan = mutated_document("plan_full_removal.json", |value| {
            if let Some(actions) = value["actions"].as_array_mut() {
                actions.reverse();
            }
        })?;
        expect_rejected(
            validate_plan_against_current_manifest(&reversed, &canonical, &canonical_digest()?),
            "canonical execution sequence",
        )?;
        Ok(())
    }

    #[test]
    fn verify_preserved_must_equal_the_preserved_population_exactly() -> Result<()> {
        let canonical = parse_manifest(canonical_manifest_text())?;

        let extra_unplanned_row: RemovalPlan =
            mutated_document("plan_full_removal.json", |value| {
                value["postconditions"]["verify_preserved"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("stowaway.txt")));
            })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &extra_unplanned_row,
                &canonical,
                &canonical_digest()?,
            ),
            "preserved population exactly",
        )?;

        let traversal_injection: RemovalPlan =
            mutated_document("plan_full_removal.json", |value| {
                value["postconditions"]["verify_preserved"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("../../outside")));
            })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &traversal_injection,
                &canonical,
                &canonical_digest()?,
            ),
            "exact-entry boundary",
        )?;

        let duplicate_entry: RemovalPlan = mutated_document("plan_full_removal.json", |value| {
            value["postconditions"]["verify_preserved"]
                .as_array_mut()
                .map(|paths| paths.push(serde_json::json!("notes.txt")));
        })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &duplicate_entry,
                &canonical,
                &canonical_digest()?,
            ),
            "duplicate postcondition path",
        )?;

        let overlap_with_absent: RemovalPlan =
            mutated_document("plan_full_removal.json", |value| {
                value["postconditions"]["verify_preserved"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("current")));
            })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &overlap_with_absent,
                &canonical,
                &canonical_digest()?,
            ),
            "must not overlap",
        )?;

        let running = parse_manifest(&fixture_text("manifest_running_current.json")?)?;
        let revalidate_row_claimed_preserved: RemovalPlan =
            mutated_document("plan_blocked_running_all_preserve.json", |value| {
                value["postconditions"]["verify_preserved"].as_array_mut().map(|paths| {
                    paths.push(serde_json::json!("candidates/v0.18.0-x86_64-unknown-linux-gnu"))
                });
            })?;
        expect_rejected(
            validate_plan_against_current_manifest(
                &revalidate_row_claimed_preserved,
                &running,
                &running_digest()?,
            ),
            "neither postcondition list",
        )?;
        Ok(())
    }

    #[test]
    fn result_population_sets_fail_closed() -> Result<()> {
        let duplicate_removed: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["removed_entries"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("current")));
            })?;
        expect_rejected(validate_result(&duplicate_removed), "exact sets")?;

        let duplicate_failure: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                let failure = value["failed_entries"][0].clone();
                value["failed_entries"].as_array_mut().map(|entries| entries.push(failure));
            })?;
        expect_rejected(validate_result(&duplicate_failure), "one failure per entry")?;

        let removed_and_preserved: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["preserved_entries"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("receipts/current.json")));
            })?;
        expect_rejected(validate_result(&removed_and_preserved), "must not overlap")?;

        let failed_and_still_present: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["preserved_entries"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!(".perllsp-path-marker")));
            })?;
        expect_rejected(validate_result(&failed_and_still_present), "stay in failed_entries only")?;

        let blocked_claims_preserved: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["result"] = serde_json::json!("blocked_running");
                value["removed_entries"] = serde_json::json!([]);
                value["failed_entries"] = serde_json::json!([]);
            })?;
        expect_rejected(
            validate_result(&blocked_claims_preserved),
            "preserved_entries must be empty",
        )?;

        let absent_claims_preserved: UninstallResult =
            mutated_document("result_already_absent_complete_evidence.json", |value| {
                value["preserved_entries"] = serde_json::json!(["notes.txt"]);
            })?;
        expect_rejected(validate_result(&absent_claims_preserved), "claim preserved rows")?;

        Ok(())
    }

    #[test]
    fn results_bind_the_exact_validated_plan_and_manifest() -> Result<()> {
        let manifest = parse_manifest(canonical_manifest_text())?;
        let digest = canonical_digest()?;
        let plan = parse_plan(&fixture_text("plan_full_removal.json")?)?;
        let outcome = parse_result(&fixture_text("result_partial_failure_retryable.json")?)?;
        validate_result_against_plan(&outcome, &plan, &manifest, &digest).map_err(|error| {
            color_eyre::eyre::eyre!("coherent partial result must bind its plan: {error}")
        })?;

        let foreign_plan_id =
            UninstallResult { plan_id: "some-other-plan".into(), ..outcome.clone() };
        expect_rejected(
            validate_result_against_plan(&foreign_plan_id, &plan, &manifest, &digest),
            "bind the exact validated plan",
        )?;

        let foreign_manifest_binding =
            UninstallResult { bound_manifest_sha256: "f".repeat(64), ..outcome.clone() };
        expect_rejected(
            validate_result_against_plan(&foreign_manifest_binding, &plan, &manifest, &digest),
            "report and its plan disagree",
        )?;

        let foreign_removed: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["removed_entries"]
                    .as_array_mut()
                    .map(|paths| paths.push(serde_json::json!("outside-the-root.txt")));
            })?;
        expect_rejected(
            validate_result_against_plan(&foreign_removed, &plan, &manifest, &digest),
            "no destructive action in the bound plan",
        )?;

        let unplanned_failure: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                value["failed_entries"][0]["relative_path"] =
                    serde_json::json!("outside-the-root.txt");
            })?;
        expect_rejected(
            validate_result_against_plan(&unplanned_failure, &plan, &manifest, &digest),
            "failures reconcile to admitted actions only",
        )?;

        let missing_destructive_coverage: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                if let Some(paths) = value["removed_entries"].as_array_mut() {
                    paths.retain(|path| path != "receipts/current.json");
                }
            })?;
        expect_rejected(
            validate_result_against_plan(&missing_destructive_coverage, &plan, &manifest, &digest),
            "partial state must stay explicit",
        )?;

        let preserve_drift: UninstallResult =
            mutated_document("result_partial_failure_retryable.json", |value| {
                if let Some(paths) = value["preserved_entries"].as_array_mut() {
                    paths.retain(|path| path != "notes.txt");
                }
            })?;
        expect_rejected(
            validate_result_against_plan(&preserve_drift, &plan, &manifest, &digest),
            "preserve disposition population",
        )?;

        let stale_plan = parse_plan(&fixture_text("plan_invalid_stale_binding.json")?)?;
        let outcome_matching_stale_subject = UninstallResult {
            plan_id: stale_plan.plan_id.clone(),
            bound_manifest_sha256: "f".repeat(64),
            ..outcome.clone()
        };
        expect_rejected(
            validate_result_against_plan(
                &outcome_matching_stale_subject,
                &stale_plan,
                &manifest,
                &digest,
            ),
            "root_or_manifest_mismatch",
        )?;
        Ok(())
    }
}
