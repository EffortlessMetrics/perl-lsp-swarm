//! Checked contract validator for standalone PATH persistence and fresh-process
//! command resolution (#11521).
//!
//! PATH carries three distinct propositions that the install surfaces have
//! historically collapsed into one:
//!
//! ```text
//! selected candidate has an exact executable path
//! != profile or registry state was changed correctly
//! != a genuinely new ordinary process resolves that exact candidate
//! ```
//!
//! This validator publishes the three documents that keep them apart —
//! `standalone_path_plan.v1` (policy and mutation intent),
//! `standalone_path_persistence.v1` (what durable state changed), and
//! `standalone_fresh_process.v1` (what a genuinely fresh process resolved) —
//! and enforces the laws that stop one from being manufactured from another.
//!
//! This binary validates documents. It mutates no profile, no registry, no
//! environment, and no PATH, and it launches no process.
#![allow(clippy::print_stdout)]

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::PathBuf;

const PLAN_SCHEMA_VERSION: &str = "standalone_path_plan.v1";
const PERSISTENCE_SCHEMA_VERSION: &str = "standalone_path_persistence.v1";
const FRESH_PROCESS_SCHEMA_VERSION: &str = "standalone_fresh_process.v1";
const REQUIRED_REDACTION_POLICY: &str = "bounded_typed_fields_only";

macro_rules! closed_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

// ── plan vocabulary ─────────────────────────────────────────────────────────

closed_enum!(ConfirmationState {
    ConfirmedCurrent => "confirmed_current",
    Unconfirmed => "unconfirmed",
    Superseded => "superseded",
    NotProven => "not_proven",
});

closed_enum!(Platform {
    Posix => "posix",
    WindowsNative => "windows_native",
    Wsl => "wsl",
});

closed_enum!(ShellFamily {
    PosixSh => "posix_sh",
    Bash => "bash",
    Zsh => "zsh",
    Fish => "fish",
    Powershell => "powershell",
    Cmd => "cmd",
});

closed_enum!(InstallRootRole {
    UserInstallRoot => "user_install_root",
    SystemInstallRoot => "system_install_root",
    PackageManagerRoot => "package_manager_root",
});

closed_enum!(PathPolicy {
    InstallerOwnedEntry => "installer_owned_entry",
    PackageManagerOwned => "package_manager_owned",
    ManualInstructionOnly => "manual_instruction_only",
    NoPathChange => "no_path_change",
});

closed_enum!(MutationOwner {
    Installer => "installer",
    PackageManager => "package_manager",
    UserManual => "user_manual",
    None => "none",
});

closed_enum!(MutationScope {
    User => "user",
    SystemElevated => "system_elevated",
    ShellSpecific => "shell_specific",
    SessionOnly => "session_only",
});

closed_enum!(OwnedEntryKind {
    ProfileLine => "profile_line",
    PathDFragment => "path_d_fragment",
    RegistryUserPath => "registry_user_path",
    None => "none",
});

closed_enum!(NewSessionRequirement {
    None => "none",
    NewShell => "new_shell",
    LogoutLogin => "logout_login",
});

closed_enum!(PriorStateIdentity {
    Sha256Content => "sha256_content",
    Unavailable => "unavailable",
});

// ── persistence result vocabulary ───────────────────────────────────────────

closed_enum!(PersistenceResult {
    AlreadyVisibleNoChange => "already_visible_no_change",
    InstallerPersisted => "installer_persisted",
    PackageManagerOwned => "package_manager_owned",
    ManualActionRequired => "manual_action_required",
    NewSessionRequired => "new_session_required",
    UnsupportedScope => "unsupported_scope",
    PermissionOrLockFailure => "permission_or_lock_failure",
    ConflictOrWrongExistingEntry => "conflict_or_wrong_existing_entry",
    Cancelled => "cancelled",
    InstrumentFailure => "instrument_failure",
    NotProven => "not_proven",
});

closed_enum!(ConflictReason {
    WrongDirectory => "wrong_directory",
    DuplicateEntry => "duplicate_entry",
    UserEditedOwnedEntry => "user_edited_owned_entry",
    ForeignInstallerEntry => "foreign_installer_entry",
    AliasOrCaseVariant => "alias_or_case_variant",
    UnreadablePriorState => "unreadable_prior_state",
});

closed_enum!(EntryState {
    AlreadyPresent => "already_present",
    Added => "added",
    Absent => "absent",
    Conflicting => "conflicting",
    Unknown => "unknown",
});

// ── fresh-process result vocabulary ─────────────────────────────────────────

closed_enum!(FreshProcessResult {
    PathVisibleImmediately => "path_visible_immediately",
    PathVisibleAfterDocumentedNewSession => "path_visible_after_documented_new_session",
    PackageManagerOwnedPath => "package_manager_owned_path",
    ManualPathActionRequired => "manual_path_action_required",
    WrongAmbientBinary => "wrong_ambient_binary",
    NotFound => "not_found",
    AmbiguousResolution => "ambiguous_resolution",
    UnsupportedEnvironment => "unsupported_environment",
    InstrumentFailure => "instrument_failure",
    NotProven => "not_proven",
});

closed_enum!(SessionOrigin {
    NewLoginSession => "new_login_session",
    NewShellProcess => "new_shell_process",
    InstallerChildProcess => "installer_child_process",
    CurrentProcess => "current_process",
});

closed_enum!(EnvironmentSource {
    AmbientSession => "ambient_session",
    InstallerMutatedCurrentProcess => "installer_mutated_current_process",
    HarnessInjectedPath => "harness_injected_path",
});

closed_enum!(LookupMethod {
    CommandLookup => "command_lookup",
    AbsolutePathInvocation => "absolute_path_invocation",
    NotAttempted => "not_attempted",
});

closed_enum!(StartupDisposition {
    Healthy => "healthy",
    Failed => "failed",
    NotRequired => "not_required",
    NotObserved => "not_observed",
});

impl SessionOrigin {
    /// A genuinely new ordinary session. `installer_child_process` is not one:
    /// it inherits the installer's environment.
    const fn is_fresh_session(self) -> bool {
        matches!(self, Self::NewLoginSession | Self::NewShellProcess)
    }
}

impl FreshProcessResult {
    const fn claims_visibility(self) -> bool {
        matches!(self, Self::PathVisibleImmediately | Self::PathVisibleAfterDocumentedNewSession)
    }
}

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

// ── shared hygiene ──────────────────────────────────────────────────────────

fn require_nonempty(value: &str, field: &str) -> ContractResult<()> {
    if value.trim().is_empty() {
        return err(format!("{field}: must not be empty"));
    }
    Ok(())
}

/// Identifiers correlate documents; they are not a place to put observations.
/// Leaving them free text let a complete PATH or a home directory ride into a
/// durable receipt through `candidate_id` while every path-shaped field around
/// them was checked, which is exactly what `bounded_typed_fields_only` denies.
fn validate_identifier(value: &str, field: &str) -> ContractResult<()> {
    require_nonempty(value, field)?;
    if value.len() > 128 {
        return err(format!(
            "{field}: an identifier is a bounded token of at most 128 characters, not a place to carry observed values"
        ));
    }
    let mut characters = value.chars();
    let starts_well = characters.next().is_some_and(|first| first.is_ascii_alphanumeric());
    let rest_ok =
        characters.all(|character| character.is_ascii_alphanumeric() || "._-".contains(character));
    if !starts_well || !rest_ok {
        return err(format!(
            "{field}: an identifier must be an ASCII token of letters, digits, `.`, `_`, or `-` starting alphanumeric; free text here would carry complete PATH, profile, or home values into a durable receipt (`{value}`)"
        ));
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

const DOS_DEVICE_BASENAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters through which path text could reach a shell, a script, the
/// registry, or an environment expansion. Path fields in these documents are
/// literal exact identities; nothing in them is ever expanded.
const EXPANSION_METACHARACTERS: [char; 13] =
    ['$', '%', '`', ';', '&', '|', '<', '>', '"', '\'', '*', '?', '~'];

fn reject_expansion(value: &str, field: &str) -> ContractResult<()> {
    for metacharacter in EXPANSION_METACHARACTERS {
        if value.contains(metacharacter) {
            return err(format!(
                "{field}: `{metacharacter}` is shell, script, registry, or environment expansion syntax; path text is a literal exact identity and is never expanded (`{value}`)"
            ));
        }
    }
    if value.chars().any(|character| character.is_ascii_control() || character == '\u{7f}') {
        return err(format!(
            "{field}: ASCII control characters make the identity spelling ambiguous; one exact representation only (`{value}`)"
        ));
    }
    Ok(())
}

/// A durable receipt records the exact installer-owned identity, never the
/// complete PATH, profile, or home value it lives inside. A list separator is
/// the observable signature of a whole PATH having been pasted in.
fn reject_path_list_or_private_value(value: &str, field: &str) -> ContractResult<()> {
    if value.contains(';') {
        return err(format!(
            "{field}: `;` is the Windows PATH list separator; durable receipts carry the exact owned entry, never the complete PATH value"
        ));
    }
    let colon_count = value.matches(':').count();
    let drive_prefixed = value.len() >= 2
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':';
    let allowed_colons = usize::from(drive_prefixed);
    if colon_count > allowed_colons {
        return err(format!(
            "{field}: `:` beyond a drive prefix is the POSIX PATH list separator; durable receipts carry the exact owned entry, never the complete PATH value"
        ));
    }
    Ok(())
}

fn reject_root_segment(segment: &str, path: &str, field: &str) -> ContractResult<()> {
    if segment.is_empty() {
        return err(format!(
            "{field}: empty segment or trailing separator makes `{path}` a non-canonical equivalent spelling of the same path"
        ));
    }
    if segment == "." || segment == ".." {
        return err(format!(
            "{field}: dot or parent segment escapes the bounded identity; traversal spellings are invalid (`{path}`)"
        ));
    }
    if segment.ends_with('.') || segment.ends_with(' ') {
        return err(format!(
            "{field}: trailing dot or space is an elidable Windows spelling of a different name; one exact representation only (`{path}`)"
        ));
    }
    let basename = segment.split('.').next().unwrap_or(segment).to_ascii_uppercase();
    if DOS_DEVICE_BASENAMES.contains(&basename.as_str()) {
        return err(format!(
            "{field}: DOS device basename `{basename}` names a reserved device, not an executable identity (`{path}`)"
        ));
    }
    Ok(())
}

/// One canonical absolute spelling: posix `/seg`, drive `X:\seg`, or UNC
/// `\\host\share`. Case, alias, traversal, and separator variants are refused
/// so that a duplicate entry cannot be spelled into looking distinct.
fn validate_absolute_path(path: &str, field: &str) -> ContractResult<()> {
    require_nonempty(path, field)?;
    reject_expansion(path, field)?;
    reject_path_list_or_private_value(path, field)?;

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
        for segment in &segments {
            reject_root_segment(segment, path, field)?;
        }
        return Ok(());
    }

    err(format!(
        "{field}: must be one canonical absolute path (posix `/seg`, drive `X:\\seg`, or unc `\\\\host\\share`) without dot, parent, or empty segments; found `{path}`"
    ))
}

/// The command a user types. A bare name only: anything with a separator is a
/// path invocation, and a path invocation can never prove PATH discoverability.
fn validate_command_name(name: &str, field: &str) -> ContractResult<()> {
    require_nonempty(name, field)?;
    reject_expansion(name, field)?;
    reject_path_list_or_private_value(name, field)?;
    if name.contains('/') || name.contains('\\') {
        return err(format!(
            "{field}: a command name with a path separator is a path invocation, not a PATH lookup; found `{name}`"
        ));
    }
    if name != name.trim() {
        return err(format!("{field}: surrounding whitespace is not part of the command name"));
    }
    Ok(())
}

fn path_separator(path: &str) -> char {
    if path.starts_with('/') { '/' } else { '\\' }
}

/// `child` must lie strictly beneath `root`, so that a foreign binary cannot be
/// laundered into the receipt as the installed candidate.
fn is_under_root(child: &str, root: &str) -> bool {
    let separator = path_separator(root);
    if path_separator(child) != separator {
        return false;
    }
    let trimmed_root = root.trim_end_matches(separator);
    // `trimmed_root.len()` is a byte offset into `child` and need not fall on a
    // character boundary, so take the prefix through the checked accessor rather
    // than indexing. A Unicode path is something to judge, never something to
    // panic on; `get` yielding Some also makes the split below boundary-safe.
    let Some(prefix) = child.get(..trimmed_root.len()) else {
        return false;
    };
    // Windows and UNC paths are case-insensitive, so two spellings of one
    // directory are one directory; refusing them would reject a legitimate
    // document rather than catch a laundering attempt. POSIX stays exact.
    let prefix_matches = if separator == '\\' {
        prefix.eq_ignore_ascii_case(trimmed_root)
    } else {
        prefix == trimmed_root
    };
    if !prefix_matches {
        return false;
    }
    let rest = &child[trimmed_root.len()..];
    rest.starts_with(separator) && rest.len() > 1
}

// ── documents ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub candidate_id: String,
    pub selection_generation: u64,
    pub confirmation_state: ConfirmationState,
    pub executable_path: String,
    pub executable_sha256: String,
    pub command_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRoot {
    pub path: String,
    pub role: InstallRootRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub platform: Platform,
    #[serde(default)]
    pub shell_family: Option<ShellFamily>,
    pub install_root: InstallRoot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedEntry {
    pub entry_kind: OwnedEntryKind,
    /// The exact location the installer owns (a profile file, a `path.d`
    /// fragment, or the user PATH registry value). Absent when nothing is owned.
    #[serde(default)]
    pub entry_location: Option<String>,
    /// The single canonical directory this entry places on PATH.
    #[serde(default)]
    pub entry_value: Option<String>,
    /// The marker that makes the entry recognizable as installer-owned, so that
    /// rollback and uninstall remove exactly this and nothing else.
    #[serde(default)]
    pub marker: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPolicyPlan {
    pub policy: PathPolicy,
    pub mutation_owner: MutationOwner,
    pub scope: MutationScope,
    pub owned_entry: OwnedEntry,
    pub new_session_requirement: NewSessionRequirement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriorState {
    pub identity_kind: PriorStateIdentity,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPlan {
    pub schema_version: String,
    pub plan_id: String,
    pub transaction_id: String,
    pub subject: Subject,
    pub environment: Environment,
    pub path_policy: PathPolicyPlan,
    pub prior_state: PriorState,
    pub redaction_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictingEntry {
    pub entry_kind: OwnedEntryKind,
    pub entry_location: String,
    pub reason: ConflictReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceReceipt {
    pub schema_version: String,
    pub bound_plan_id: String,
    pub bound_plan_sha256: String,
    pub result: PersistenceResult,
    /// Whether durable state outside this process was written.
    pub mutation_performed: bool,
    pub entry_state: EntryState,
    pub observed_scope: MutationScope,
    /// Set when the installer changed only its own process environment. This is
    /// never persistence evidence.
    pub current_process_env_mutated: bool,
    pub new_session_required: bool,
    /// Count of installer-owned entries observed after the operation. Anything
    /// other than exactly one means the entry is not canonical.
    pub owned_entries_observed: u32,
    #[serde(default)]
    pub conflicting_entry: Option<ConflictingEntry>,
    pub instrument_complete: bool,
    pub redaction_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservedEnvironment {
    pub platform: Platform,
    #[serde(default)]
    pub shell_family: Option<ShellFamily>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionFacts {
    pub origin: SessionOrigin,
    pub environment_source: EnvironmentSource,
    /// Whether the harness placed the install directory on PATH for this
    /// observation. A prepared PATH cannot prove that the installer persisted one.
    pub path_prepared_by_harness: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedExecutable {
    pub path: String,
    pub sha256: String,
    pub matches_candidate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompetingCandidate {
    pub path: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupFacts {
    pub method: LookupMethod,
    #[serde(default)]
    pub resolved: Option<ResolvedExecutable>,
    /// Every candidate for the command name observed on the session PATH, as an
    /// exact deterministic set: strictly ascending by path, no duplicates.
    pub competing_candidates: Vec<CompetingCandidate>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshProcessObservation {
    pub schema_version: String,
    pub bound_plan_id: String,
    pub bound_plan_sha256: String,
    #[serde(default)]
    pub bound_persistence_result: Option<PersistenceResult>,
    pub observed_environment: ObservedEnvironment,
    pub session: SessionFacts,
    pub lookup: LookupFacts,
    pub startup_disposition: StartupDisposition,
    pub result: FreshProcessResult,
    pub instrument_complete: bool,
    pub redaction_policy: String,
}

// ── plan laws ───────────────────────────────────────────────────────────────

fn shell_belongs_to(platform: Platform, shell: ShellFamily) -> bool {
    match platform {
        Platform::WindowsNative => matches!(shell, ShellFamily::Powershell | ShellFamily::Cmd),
        Platform::Posix | Platform::Wsl => matches!(
            shell,
            ShellFamily::PosixSh | ShellFamily::Bash | ShellFamily::Zsh | ShellFamily::Fish
        ),
    }
}

fn entry_kind_belongs_to(platform: Platform, kind: OwnedEntryKind) -> bool {
    match kind {
        OwnedEntryKind::None => true,
        OwnedEntryKind::RegistryUserPath => matches!(platform, Platform::WindowsNative),
        OwnedEntryKind::ProfileLine | OwnedEntryKind::PathDFragment => {
            matches!(platform, Platform::Posix | Platform::Wsl)
        }
    }
}

pub fn validate_plan(plan: &PathPlan) -> ContractResult<()> {
    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return err(format!(
            "schema_version: expected `{PLAN_SCHEMA_VERSION}`, found `{}`",
            plan.schema_version
        ));
    }
    if plan.redaction_policy != REQUIRED_REDACTION_POLICY {
        return err(format!(
            "redaction_policy: must be `{REQUIRED_REDACTION_POLICY}`; complete PATH, profile, registry, and home values never enter a durable receipt"
        ));
    }
    validate_identifier(&plan.plan_id, "plan_id")?;
    validate_identifier(&plan.transaction_id, "transaction_id")?;
    validate_identifier(&plan.subject.candidate_id, "subject.candidate_id")?;

    let environment = &plan.environment;
    let subject = &plan.subject;
    let policy = &plan.path_policy;

    validate_absolute_path(&environment.install_root.path, "environment.install_root.path")?;
    validate_absolute_path(&subject.executable_path, "subject.executable_path")?;
    validate_sha256(&subject.executable_sha256, "subject.executable_sha256")?;
    validate_command_name(&subject.command_name, "subject.command_name")?;

    if !is_under_root(&subject.executable_path, &environment.install_root.path) {
        return err(format!(
            "subject.executable_path: `{}` is not under install root `{}`; a binary outside the root is not this installer's candidate",
            subject.executable_path, environment.install_root.path
        ));
    }

    let separator = path_separator(&subject.executable_path);
    let file_name =
        subject.executable_path.rsplit(separator).next().unwrap_or(&subject.executable_path);
    // Command lookup is platform-specific: native Windows appends an executable
    // extension and matches case-insensitively, while POSIX and WSL resolve the
    // exact byte sequence. Normalizing the same way on every platform would
    // accept `perllsp.exe` as the POSIX command `perllsp`, which no POSIX lookup
    // would ever resolve.
    let names_the_command = match environment.platform {
        Platform::WindowsNative => {
            // `len() - 4` is a byte offset, not a character offset: for a name
            // whose tail is multi-byte UTF-8 it can land inside a codepoint, and
            // indexing there would panic. A malformed-looking name must produce
            // the typed refusal below, never a crash, so take the tail through
            // the checked accessor — `get` yields None on a non-boundary, and a
            // boundary that did yield Some is safe to split on.
            let stem = file_name
                .len()
                .checked_sub(4)
                .and_then(|cut| file_name.get(cut..).map(|tail| (cut, tail)))
                .filter(|(_, tail)| tail.eq_ignore_ascii_case(".exe"))
                .map_or(file_name, |(cut, _)| &file_name[..cut]);
            stem.eq_ignore_ascii_case(&subject.command_name)
        }
        Platform::Posix | Platform::Wsl => file_name == subject.command_name,
    };
    if !names_the_command {
        return err(format!(
            "subject.command_name: `{}` does not name the executable `{file_name}` under `{}` lookup rules; the command a user types must be the candidate that is looked up",
            subject.command_name,
            environment.platform.as_str()
        ));
    }

    if let Some(shell) = environment.shell_family
        && !shell_belongs_to(environment.platform, shell)
    {
        return err(format!(
            "environment.shell_family: `{}` does not exist on platform `{}`; native Windows, WSL, and POSIX are distinct execution environments",
            shell.as_str(),
            environment.platform.as_str()
        ));
    }

    if !entry_kind_belongs_to(environment.platform, policy.owned_entry.entry_kind) {
        return err(format!(
            "path_policy.owned_entry.entry_kind: `{}` is not a persistence mechanism on platform `{}`",
            policy.owned_entry.entry_kind.as_str(),
            environment.platform.as_str()
        ));
    }

    // The confirmation gate: a candidate that is not the confirmed current
    // selection may never reach installer-owned PATH mutation.
    if policy.mutation_owner == MutationOwner::Installer
        && subject.confirmation_state != ConfirmationState::ConfirmedCurrent
    {
        return err(format!(
            "path_policy.mutation_owner: installer-owned PATH mutation requires `confirmed_current`; subject is `{}`",
            subject.confirmation_state.as_str()
        ));
    }

    let expected_owner = match policy.policy {
        PathPolicy::InstallerOwnedEntry => MutationOwner::Installer,
        PathPolicy::PackageManagerOwned => MutationOwner::PackageManager,
        PathPolicy::ManualInstructionOnly => MutationOwner::UserManual,
        PathPolicy::NoPathChange => MutationOwner::None,
    };
    if policy.mutation_owner != expected_owner {
        return err(format!(
            "path_policy.mutation_owner: policy `{}` is owned by `{}`, not `{}`",
            policy.policy.as_str(),
            expected_owner.as_str(),
            policy.mutation_owner.as_str()
        ));
    }

    if policy.mutation_owner == MutationOwner::Installer {
        if policy.owned_entry.entry_kind == OwnedEntryKind::None {
            return err(
                "path_policy.owned_entry: installer-owned persistence must name exactly one canonical owned entry",
            );
        }
    } else if policy.owned_entry.entry_kind != OwnedEntryKind::None {
        return err(format!(
            "path_policy.owned_entry: mutation owner `{}` owns no entry, yet `{}` is declared; ownership cannot be laundered",
            policy.mutation_owner.as_str(),
            policy.owned_entry.entry_kind.as_str()
        ));
    }

    validate_owned_entry(&policy.owned_entry, &environment.install_root)?;

    // A session-only change lives and dies with one process; it is never durable
    // installer-owned persistence.
    if policy.scope == MutationScope::SessionOnly
        && matches!(policy.mutation_owner, MutationOwner::Installer | MutationOwner::PackageManager)
    {
        return err(format!(
            "path_policy.scope: `session_only` cannot carry mutation owner `{}`; a current-process change is not durable persistence",
            policy.mutation_owner.as_str()
        ));
    }
    if policy.scope == MutationScope::SystemElevated
        && environment.install_root.role != InstallRootRole::SystemInstallRoot
    {
        return err(
            "path_policy.scope: `system_elevated` requires a system install root; user scope is never silently widened",
        );
    }
    if policy.scope == MutationScope::ShellSpecific && environment.shell_family.is_none() {
        return err(
            "path_policy.scope: `shell_specific` requires the exact shell family; one shell's evidence never satisfies all shells",
        );
    }

    if policy.policy == PathPolicy::NoPathChange
        && policy.new_session_requirement != NewSessionRequirement::None
    {
        return err(
            "path_policy.new_session_requirement: `no_path_change` changes nothing, so no new session can be required",
        );
    }

    match plan.prior_state.identity_kind {
        PriorStateIdentity::Sha256Content => match plan.prior_state.sha256.as_deref() {
            Some(digest) => validate_sha256(digest, "prior_state.sha256")?,
            None => {
                return err(
                    "prior_state.sha256: `sha256_content` identity requires the digest it claims",
                );
            }
        },
        PriorStateIdentity::Unavailable => {
            if plan.prior_state.sha256.is_some() {
                return err(
                    "prior_state.sha256: `unavailable` identity must carry no digest; ambiguous prior state stays ambiguous",
                );
            }
        }
    }

    Ok(())
}

fn validate_owned_entry(entry: &OwnedEntry, install_root: &InstallRoot) -> ContractResult<()> {
    if entry.entry_kind == OwnedEntryKind::None {
        if entry.entry_location.is_some() || entry.entry_value.is_some() || entry.marker.is_some() {
            return err(
                "path_policy.owned_entry: entry_kind `none` must carry no location, value, or marker",
            );
        }
        return Ok(());
    }

    let Some(location) = entry.entry_location.as_deref() else {
        return err(
            "path_policy.owned_entry.entry_location: an owned entry must name where it lives",
        );
    };
    let Some(value) = entry.entry_value.as_deref() else {
        return err(
            "path_policy.owned_entry.entry_value: an owned entry must name the single directory it places on PATH",
        );
    };
    let Some(marker) = entry.marker.as_deref() else {
        return err(
            "path_policy.owned_entry.marker: an owned entry must be recognizable, so rollback and uninstall remove exactly it and nothing else",
        );
    };

    if entry.entry_kind == OwnedEntryKind::RegistryUserPath {
        require_nonempty(location, "path_policy.owned_entry.entry_location")?;
        reject_expansion(location, "path_policy.owned_entry.entry_location")?;
    } else {
        validate_absolute_path(location, "path_policy.owned_entry.entry_location")?;
    }
    validate_absolute_path(value, "path_policy.owned_entry.entry_value")?;
    require_nonempty(marker, "path_policy.owned_entry.marker")?;
    reject_expansion(marker, "path_policy.owned_entry.marker")?;
    reject_path_list_or_private_value(marker, "path_policy.owned_entry.marker")?;

    // The entry may name a stable directory rather than the versioned one, but it
    // must lie inside what this installer owns: PATH is never pointed at a
    // directory outside the install root.
    if !is_under_root(value, &install_root.path) {
        return err(format!(
            "path_policy.owned_entry.entry_value: `{value}` is not under install root `{}`; an installer-owned entry never places a directory it does not own on PATH",
            install_root.path
        ));
    }

    Ok(())
}

// ── persistence laws ────────────────────────────────────────────────────────

impl PersistenceResult {
    /// Outcomes under which nothing durable was written, so no owned entry can
    /// have appeared.
    const fn wrote_nothing(self) -> bool {
        matches!(
            self,
            Self::AlreadyVisibleNoChange
                | Self::PackageManagerOwned
                | Self::ManualActionRequired
                | Self::UnsupportedScope
                | Self::PermissionOrLockFailure
                | Self::ConflictOrWrongExistingEntry
                | Self::Cancelled
                | Self::InstrumentFailure
                | Self::NotProven
        )
    }

    /// Outcomes that assert the command is on PATH for a future process.
    const fn claims_visible_entry(self) -> bool {
        matches!(self, Self::AlreadyVisibleNoChange | Self::InstallerPersisted)
    }
}

pub fn validate_persistence(receipt: &PersistenceReceipt) -> ContractResult<()> {
    if receipt.schema_version != PERSISTENCE_SCHEMA_VERSION {
        return err(format!(
            "schema_version: expected `{PERSISTENCE_SCHEMA_VERSION}`, found `{}`",
            receipt.schema_version
        ));
    }
    if receipt.redaction_policy != REQUIRED_REDACTION_POLICY {
        return err(format!(
            "redaction_policy: must be `{REQUIRED_REDACTION_POLICY}`; complete PATH, profile, registry, and home values never enter a durable receipt"
        ));
    }
    validate_identifier(&receipt.bound_plan_id, "bound_plan_id")?;
    validate_sha256(&receipt.bound_plan_sha256, "bound_plan_sha256")?;

    let result = receipt.result;

    if receipt.instrument_complete {
        if result == PersistenceResult::InstrumentFailure {
            return err("result: `instrument_failure` cannot be reported by a complete instrument");
        }
    } else if !matches!(result, PersistenceResult::InstrumentFailure | PersistenceResult::NotProven)
    {
        return err(format!(
            "result: an incomplete instrument cannot conclude `{}`; missing evidence is `instrument_failure` or `not_proven`",
            result.as_str()
        ));
    }

    if result.wrote_nothing() && receipt.mutation_performed {
        return err(format!(
            "mutation_performed: `{}` asserts that no durable state was written",
            result.as_str()
        ));
    }
    if result.wrote_nothing() && receipt.entry_state == EntryState::Added {
        return err(format!("entry_state: `{}` cannot have added an owned entry", result.as_str()));
    }

    match result {
        PersistenceResult::InstallerPersisted => {
            if !receipt.mutation_performed {
                return err(
                    "mutation_performed: `installer_persisted` must have written durable state",
                );
            }
            if receipt.entry_state != EntryState::Added {
                return err(format!(
                    "entry_state: `installer_persisted` requires `added`, found `{}`",
                    receipt.entry_state.as_str()
                ));
            }
        }
        PersistenceResult::AlreadyVisibleNoChange
            if receipt.entry_state != EntryState::AlreadyPresent =>
        {
            return err(format!(
                "entry_state: `already_visible_no_change` requires `already_present`, found `{}`",
                receipt.entry_state.as_str()
            ));
        }
        PersistenceResult::ConflictOrWrongExistingEntry => {
            if receipt.conflicting_entry.is_none() {
                return err(
                    "conflicting_entry: `conflict_or_wrong_existing_entry` must name the exact conflicting state",
                );
            }
            if receipt.entry_state != EntryState::Conflicting {
                return err(format!(
                    "entry_state: `conflict_or_wrong_existing_entry` requires `conflicting`, found `{}`",
                    receipt.entry_state.as_str()
                ));
            }
        }
        PersistenceResult::NewSessionRequired if !receipt.new_session_required => {
            return err(
                "new_session_required: the `new_session_required` outcome must set the flag it names",
            );
        }
        _ => {}
    }

    // An entry cannot appear without a durable write, under any outcome word.
    if receipt.entry_state == EntryState::Added && !receipt.mutation_performed {
        return err(
            "entry_state: `added` requires a durable write; an entry cannot appear without one",
        );
    }

    if result != PersistenceResult::ConflictOrWrongExistingEntry
        && receipt.conflicting_entry.is_some()
    {
        return err(format!(
            "conflicting_entry: an observed conflict cannot be reported under `{}`",
            result.as_str()
        ));
    }

    if let Some(conflict) = &receipt.conflicting_entry {
        if conflict.entry_kind == OwnedEntryKind::RegistryUserPath {
            reject_expansion(&conflict.entry_location, "conflicting_entry.entry_location")?;
            require_nonempty(&conflict.entry_location, "conflicting_entry.entry_location")?;
        } else {
            validate_absolute_path(&conflict.entry_location, "conflicting_entry.entry_location")?;
        }
    }

    // A current-process environment edit is not durable persistence, and a
    // session-only scope cannot produce an entry a later process would see.
    if receipt.observed_scope == MutationScope::SessionOnly && result.claims_visible_entry() {
        return err(format!(
            "result: `{}` cannot be observed at `session_only` scope; a current-process change is not durable persistence",
            result.as_str()
        ));
    }
    if receipt.current_process_env_mutated
        && !receipt.mutation_performed
        && result.claims_visible_entry()
    {
        return err(format!(
            "result: `{}` rests only on a current-process environment edit; that is not evidence of durable persistence",
            result.as_str()
        ));
    }

    // Exactly one canonical installer-owned entry, or none.
    match receipt.entry_state {
        EntryState::Added | EntryState::AlreadyPresent => {
            if receipt.owned_entries_observed != 1 {
                return err(format!(
                    "owned_entries_observed: entry_state `{}` requires exactly one canonical owned entry, found {}; duplicate, case, or alias spellings are not idempotent",
                    receipt.entry_state.as_str(),
                    receipt.owned_entries_observed
                ));
            }
        }
        EntryState::Absent => {
            if receipt.owned_entries_observed != 0 {
                return err(format!(
                    "owned_entries_observed: entry_state `absent` requires zero owned entries, found {}",
                    receipt.owned_entries_observed
                ));
            }
        }
        EntryState::Conflicting | EntryState::Unknown => {}
    }

    Ok(())
}

pub fn validate_persistence_against_plan(
    receipt: &PersistenceReceipt,
    plan: &PathPlan,
    plan_digest: &str,
) -> ContractResult<()> {
    validate_persistence(receipt)?;

    if receipt.bound_plan_id != plan.plan_id {
        return err(format!(
            "bound_plan_id: receipt binds `{}` but the plan is `{}`",
            receipt.bound_plan_id, plan.plan_id
        ));
    }
    if receipt.bound_plan_sha256 != plan_digest {
        return err(format!(
            "bound_plan_sha256: receipt binds plan {} but the current plan is {}; the policy it was produced under moved",
            &receipt.bound_plan_sha256[..16],
            &plan_digest[..16]
        ));
    }

    let policy = &plan.path_policy;
    let expected_owner = match receipt.result {
        PersistenceResult::InstallerPersisted => Some(MutationOwner::Installer),
        PersistenceResult::PackageManagerOwned => Some(MutationOwner::PackageManager),
        PersistenceResult::ManualActionRequired => Some(MutationOwner::UserManual),
        _ => None,
    };
    if let Some(owner) = expected_owner
        && policy.mutation_owner != owner
    {
        return err(format!(
            "result: `{}` requires plan mutation owner `{}`, but the plan selected `{}`",
            receipt.result.as_str(),
            owner.as_str(),
            policy.mutation_owner.as_str()
        ));
    }

    if receipt.entry_state == EntryState::Added
        && policy.owned_entry.entry_kind == OwnedEntryKind::None
    {
        return err(
            "entry_state: `added` claims an owned entry the plan never authorized; the installer owns no entry under this policy",
        );
    }

    if receipt.new_session_required && policy.new_session_requirement == NewSessionRequirement::None
    {
        return err(
            "new_session_required: the plan documents no new-session boundary, so one cannot be reported",
        );
    }

    let scope_is_reportable = !matches!(
        receipt.result,
        PersistenceResult::UnsupportedScope
            | PersistenceResult::InstrumentFailure
            | PersistenceResult::NotProven
    );
    if scope_is_reportable && receipt.observed_scope != policy.scope {
        return err(format!(
            "observed_scope: observed `{}` but the plan selected `{}`; user, system, shell, and session scopes are distinct and never silently widened",
            receipt.observed_scope.as_str(),
            policy.scope.as_str()
        ));
    }

    Ok(())
}

// ── fresh-process laws ──────────────────────────────────────────────────────

pub fn validate_fresh_process(observation: &FreshProcessObservation) -> ContractResult<()> {
    if observation.schema_version != FRESH_PROCESS_SCHEMA_VERSION {
        return err(format!(
            "schema_version: expected `{FRESH_PROCESS_SCHEMA_VERSION}`, found `{}`",
            observation.schema_version
        ));
    }
    if observation.redaction_policy != REQUIRED_REDACTION_POLICY {
        return err(format!(
            "redaction_policy: must be `{REQUIRED_REDACTION_POLICY}`; complete PATH, profile, registry, and home values never enter a durable receipt"
        ));
    }
    validate_identifier(&observation.bound_plan_id, "bound_plan_id")?;
    validate_sha256(&observation.bound_plan_sha256, "bound_plan_sha256")?;

    let session = &observation.session;
    let lookup = &observation.lookup;
    let result = observation.result;

    if let Some(shell) = observation.observed_environment.shell_family
        && !shell_belongs_to(observation.observed_environment.platform, shell)
    {
        return err(format!(
            "observed_environment.shell_family: `{}` does not exist on platform `{}`",
            shell.as_str(),
            observation.observed_environment.platform.as_str()
        ));
    }

    if observation.instrument_complete {
        if result == FreshProcessResult::InstrumentFailure {
            return err("result: `instrument_failure` cannot be reported by a complete instrument");
        }
    } else if !matches!(
        result,
        FreshProcessResult::InstrumentFailure | FreshProcessResult::NotProven
    ) {
        return err(format!(
            "result: an incomplete instrument cannot conclude `{}`; missing evidence is `instrument_failure` or `not_proven`",
            result.as_str()
        ));
    }

    // The exact-set law: competing candidates are strictly ascending and unique,
    // so the same observation always serializes the same way.
    let mut previous: Option<&str> = None;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (index, candidate) in lookup.competing_candidates.iter().enumerate() {
        validate_absolute_path(
            &candidate.path,
            &format!("lookup.competing_candidates[{index}].path"),
        )?;
        if let Some(digest) = candidate.sha256.as_deref() {
            validate_sha256(digest, &format!("lookup.competing_candidates[{index}].sha256"))?;
        }
        if let Some(prior) = previous
            && candidate.path.as_str() <= prior
        {
            return err(format!(
                "lookup.competing_candidates: must be a strictly ascending unique set; `{}` follows `{prior}`",
                candidate.path
            ));
        }
        seen.insert(candidate.path.as_str());
        previous = Some(candidate.path.as_str());
    }

    if let Some(resolved) = &lookup.resolved {
        validate_absolute_path(&resolved.path, "lookup.resolved.path")?;
        validate_sha256(&resolved.sha256, "lookup.resolved.sha256")?;
        if !seen.contains(resolved.path.as_str()) {
            return err(format!(
                "lookup.resolved.path: `{}` is not among the observed candidates; the winner must be part of the observed set",
                resolved.path
            ));
        }
        // One document, one identity for the binary that won: membership by path
        // alone would let the observed set and the winner disagree about what
        // that path contains.
        if let Some(row) =
            lookup.competing_candidates.iter().find(|candidate| candidate.path == resolved.path)
            && let Some(row_digest) = row.sha256.as_deref()
            && row_digest != resolved.sha256
        {
            return err(format!(
                "lookup.resolved.sha256: the winner at `{}` is recorded as {} but the observed candidate set records {} for the same path; one observation carries one identity",
                resolved.path,
                &resolved.sha256[..16],
                &row_digest[..16]
            ));
        }
    }

    // A path invocation exercises a path, not a lookup.
    if lookup.method == LookupMethod::AbsolutePathInvocation && result.claims_visibility() {
        return err(format!(
            "result: `{}` cannot rest on an absolute-path invocation; invoking a known path proves nothing about command discoverability",
            result.as_str()
        ));
    }
    if lookup.method == LookupMethod::NotAttempted
        && !matches!(
            result,
            FreshProcessResult::UnsupportedEnvironment
                | FreshProcessResult::ManualPathActionRequired
                | FreshProcessResult::PackageManagerOwnedPath
                | FreshProcessResult::InstrumentFailure
                | FreshProcessResult::NotProven
        )
    {
        return err(format!("result: `{}` requires an attempted lookup", result.as_str()));
    }
    if result == FreshProcessResult::UnsupportedEnvironment
        && lookup.method != LookupMethod::NotAttempted
    {
        return err("lookup.method: `unsupported_environment` means the lookup was not attempted");
    }

    // A manufactured environment is not a fresh one.
    if result.claims_visibility() {
        if !session.origin.is_fresh_session() {
            return err(format!(
                "result: `{}` requires a genuinely new session; `{}` inherits or is the installer's own process",
                result.as_str(),
                session.origin.as_str()
            ));
        }
        if session.environment_source != EnvironmentSource::AmbientSession {
            return err(format!(
                "result: `{}` requires an ambient session environment; `{}` was supplied to the observation",
                result.as_str(),
                session.environment_source.as_str()
            ));
        }
        if session.path_prepared_by_harness {
            return err(format!(
                "result: `{}` cannot rest on a PATH the harness prepared; the installer's own persistence is what is under test",
                result.as_str()
            ));
        }
        if lookup.method != LookupMethod::CommandLookup {
            return err(format!("result: `{}` requires a command lookup", result.as_str()));
        }
        if lookup.ambiguous {
            return err(format!(
                "result: `{}` cannot be concluded from an ambiguous resolution",
                result.as_str()
            ));
        }
        match &lookup.resolved {
            Some(resolved) if resolved.matches_candidate => {}
            Some(_) => {
                return err(format!(
                    "result: `{}` resolved a binary that is not the selected candidate",
                    result.as_str()
                ));
            }
            None => {
                return err(format!(
                    "result: `{}` must name the executable that was resolved",
                    result.as_str()
                ));
            }
        }
    }

    // A wrong ambient binary stays visible; the selected candidate existing
    // somewhere does not make the lookup correct.
    if let Some(resolved) = &lookup.resolved
        && !resolved.matches_candidate
        && !matches!(
            result,
            FreshProcessResult::WrongAmbientBinary
                | FreshProcessResult::AmbiguousResolution
                | FreshProcessResult::InstrumentFailure
                | FreshProcessResult::NotProven
        )
    {
        return err(format!(
            "result: the command resolved a binary that is not the candidate, which is `wrong_ambient_binary`, not `{}`",
            result.as_str()
        ));
    }
    if result == FreshProcessResult::WrongAmbientBinary {
        match &lookup.resolved {
            Some(resolved) if !resolved.matches_candidate => {}
            Some(_) => {
                return err(
                    "result: `wrong_ambient_binary` cannot have resolved the selected candidate",
                );
            }
            None => {
                return err(
                    "lookup.resolved: `wrong_ambient_binary` must observe the exact binary that won",
                );
            }
        }
    }

    if lookup.ambiguous && result != FreshProcessResult::AmbiguousResolution {
        return err(format!(
            "result: an ambiguous resolution cannot be reported as `{}`",
            result.as_str()
        ));
    }
    if result == FreshProcessResult::AmbiguousResolution {
        if !lookup.ambiguous {
            return err("lookup.ambiguous: `ambiguous_resolution` must set the flag it names");
        }
        if lookup.competing_candidates.len() < 2 {
            return err(format!(
                "lookup.competing_candidates: `ambiguous_resolution` requires at least two observed candidates, found {}",
                lookup.competing_candidates.len()
            ));
        }
    }

    if result == FreshProcessResult::NotFound {
        if lookup.resolved.is_some() {
            return err("result: `not_found` cannot have resolved an executable");
        }
        if !lookup.competing_candidates.is_empty() {
            return err("result: `not_found` cannot have observed candidates for the command name");
        }
    }

    if observation.startup_disposition == StartupDisposition::Failed && result.claims_visibility() {
        return err(format!(
            "result: `{}` cannot be concluded while startup failed; discoverability and health stay separate but a failed start is not a clean visibility claim",
            result.as_str()
        ));
    }

    Ok(())
}

pub fn validate_fresh_process_against_plan(
    observation: &FreshProcessObservation,
    plan: &PathPlan,
    plan_digest: &str,
) -> ContractResult<()> {
    validate_fresh_process(observation)?;

    if observation.bound_plan_id != plan.plan_id {
        return err(format!(
            "bound_plan_id: observation binds `{}` but the plan is `{}`",
            observation.bound_plan_id, plan.plan_id
        ));
    }
    if observation.bound_plan_sha256 != plan_digest {
        return err(format!(
            "bound_plan_sha256: observation binds plan {} but the current plan is {}; the selection moved before the observation, so it proves nothing about the current candidate",
            &observation.bound_plan_sha256[..16],
            &plan_digest[..16]
        ));
    }

    // Native Windows, WSL, and POSIX are distinct execution environments; one
    // never stands in for another.
    let environment_is_reportable = !matches!(
        observation.result,
        FreshProcessResult::UnsupportedEnvironment
            | FreshProcessResult::InstrumentFailure
            | FreshProcessResult::NotProven
    );
    if environment_is_reportable
        && observation.observed_environment.platform != plan.environment.platform
    {
        return err(format!(
            "observed_environment.platform: observed `{}` but the plan targets `{}`; native Windows, WSL, and POSIX evidence are not interchangeable",
            observation.observed_environment.platform.as_str(),
            plan.environment.platform.as_str()
        ));
    }
    if environment_is_reportable
        && plan.path_policy.scope == MutationScope::ShellSpecific
        && observation.observed_environment.shell_family != plan.environment.shell_family
    {
        return err(
            "observed_environment.shell_family: a shell-specific policy is only proven in the exact shell it targets",
        );
    }

    if let Some(resolved) = &observation.lookup.resolved {
        let digest_matches = resolved.sha256 == plan.subject.executable_sha256;
        if resolved.matches_candidate != digest_matches {
            return err(format!(
                "lookup.resolved.matches_candidate: claims `{}` but the resolved digest {} the plan's candidate digest",
                resolved.matches_candidate,
                if digest_matches { "equals" } else { "differs from" }
            ));
        }
        if digest_matches && resolved.path != plan.subject.executable_path {
            return err(format!(
                "lookup.resolved.path: `{}` carries the candidate digest but is not the candidate's exact path `{}`",
                resolved.path, plan.subject.executable_path
            ));
        }
    }

    match observation.result {
        FreshProcessResult::PathVisibleImmediately
            if plan.path_policy.new_session_requirement != NewSessionRequirement::None =>
        {
            return err(format!(
                "result: the plan documents a `{}` boundary, so visibility is `path_visible_after_documented_new_session`, not immediate",
                plan.path_policy.new_session_requirement.as_str()
            ));
        }
        FreshProcessResult::PathVisibleAfterDocumentedNewSession
            if plan.path_policy.new_session_requirement == NewSessionRequirement::None =>
        {
            return err(
                "result: `path_visible_after_documented_new_session` requires the plan to document the new-session mechanism it tested",
            );
        }
        // A logout/login boundary is strictly stronger than a new shell: only a
        // new login session exercises it. A new shell would report success
        // without ever crossing the boundary the plan selected.
        FreshProcessResult::PathVisibleAfterDocumentedNewSession
            if plan.path_policy.new_session_requirement == NewSessionRequirement::LogoutLogin
                && observation.session.origin != SessionOrigin::NewLoginSession =>
        {
            return err(format!(
                "session.origin: the plan documents a `logout_login` boundary, which only `new_login_session` exercises; `{}` never crossed it",
                observation.session.origin.as_str()
            ));
        }
        FreshProcessResult::PackageManagerOwnedPath
            if plan.path_policy.mutation_owner != MutationOwner::PackageManager =>
        {
            return err(
                "result: `package_manager_owned_path` requires a package-manager-owned plan",
            );
        }
        FreshProcessResult::ManualPathActionRequired
            if plan.path_policy.mutation_owner != MutationOwner::UserManual =>
        {
            return err("result: `manual_path_action_required` requires a manual-instruction plan");
        }
        _ => {}
    }

    Ok(())
}

/// Persistence and discoverability are separate propositions, but they cannot
/// contradict each other about the same plan.
pub fn validate_fresh_process_against_persistence(
    observation: &FreshProcessObservation,
    receipt: &PersistenceReceipt,
) -> ContractResult<()> {
    if observation.bound_plan_id != receipt.bound_plan_id
        || observation.bound_plan_sha256 != receipt.bound_plan_sha256
    {
        return err(
            "bound_plan: the observation and the persistence receipt describe different plans",
        );
    }
    let Some(declared) = observation.bound_persistence_result else {
        return err(
            "bound_persistence_result: an observation paired with a persistence receipt must declare which outcome it followed",
        );
    };
    if declared != receipt.result {
        return err(format!(
            "bound_persistence_result: observation followed `{}` but the receipt reports `{}`",
            declared.as_str(),
            receipt.result.as_str()
        ));
    }
    if observation.result.claims_visibility()
        && !matches!(
            receipt.result,
            PersistenceResult::InstallerPersisted
                | PersistenceResult::AlreadyVisibleNoChange
                | PersistenceResult::PackageManagerOwned
                | PersistenceResult::NewSessionRequired
        )
    {
        return err(format!(
            "result: `{}` contradicts persistence outcome `{}`; no durable entry was established for a later process to find",
            observation.result.as_str(),
            receipt.result.as_str()
        ));
    }
    Ok(())
}

// ── serialization and CLI ───────────────────────────────────────────────────

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
#[command(name = "standalone-path-persistence")]
#[command(
    about = "Validate standalone PATH plans, persistence receipts, and fresh-process observations (#11521); validates documents and mutates no PATH, profile, registry, or environment"
)]
struct Args {
    /// standalone_path_plan.v1 document to validate.
    #[arg(long)]
    plan: Option<PathBuf>,
    /// standalone_path_persistence.v1 document; with --plan, binds against the exact validated plan.
    #[arg(long)]
    persistence: Option<PathBuf>,
    /// standalone_fresh_process.v1 document; with --plan, binds against the exact validated plan.
    #[arg(long)]
    fresh_process: Option<PathBuf>,
    /// Print the plan in canonical key-sorted serialization after validation.
    #[arg(long)]
    print_canonical: bool,
}

fn load_json(path: &std::path::Path) -> Result<Value> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

fn load_and_validate_plan(path: &std::path::Path) -> Result<(PathPlan, String)> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let plan: PathPlan =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))?;
    validate_plan(&plan).with_context(|| "validating PATH plan")?;
    Ok((plan, sha256_hex(&bytes)))
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::parse();

    if args.plan.is_none() && args.persistence.is_none() && args.fresh_process.is_none() {
        bail!("nothing to validate: pass --plan, --persistence, or --fresh-process");
    }

    let validated_plan = match &args.plan {
        Some(plan_path) => {
            let (plan, digest) = load_and_validate_plan(plan_path)?;
            println!("standalone-path-persistence: plan valid ({})", &digest[..16]);
            if args.print_canonical {
                let value = load_json(plan_path)?;
                println!("{}", canonical_json(&value));
            }
            Some((plan, digest))
        }
        None => None,
    };

    let mut observed_without_persistence = false;
    let mut validated_persistence: Option<PersistenceReceipt> = None;
    if let Some(receipt_path) = &args.persistence {
        let bytes = fs::read(receipt_path)
            .with_context(|| format!("reading {}", receipt_path.display()))?;
        let receipt: PersistenceReceipt = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {}", receipt_path.display()))?;
        match &validated_plan {
            Some((plan, digest)) => {
                validate_persistence_against_plan(&receipt, plan, digest)
                    .with_context(|| "validating persistence receipt against its bound plan")?;
            }
            None => {
                validate_persistence(&receipt).with_context(|| "validating persistence receipt")?;
            }
        }
        println!("standalone-path-persistence: persistence valid ({})", receipt.result.as_str());
        validated_persistence = Some(receipt);
    }

    if let Some(observation_path) = &args.fresh_process {
        let bytes = fs::read(observation_path)
            .with_context(|| format!("reading {}", observation_path.display()))?;
        let observation: FreshProcessObservation = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {}", observation_path.display()))?;
        match &validated_plan {
            Some((plan, digest)) => {
                validate_fresh_process_against_plan(&observation, plan, digest).with_context(
                    || "validating fresh-process observation against its bound plan",
                )?;
            }
            None => {
                validate_fresh_process(&observation)
                    .with_context(|| "validating fresh-process observation")?;
            }
        }
        if let Some(receipt) = &validated_persistence {
            validate_fresh_process_against_persistence(&observation, receipt).with_context(
                || "validating fresh-process observation against its persistence receipt",
            )?;
        }
        println!(
            "standalone-path-persistence: fresh-process valid ({})",
            observation.result.as_str()
        );
        observed_without_persistence =
            validated_persistence.is_none() && observation.bound_persistence_result.is_some();
    }

    // "valid" must not be read as "fully checked". The cross-document laws only
    // fire when the documents they relate are supplied together, and no JSON
    // Schema can express them, so a partial invocation states its own scope.
    let mut unverified: Vec<&str> = Vec::new();
    if validated_plan.is_none() && (args.persistence.is_some() || args.fresh_process.is_some()) {
        unverified.push(
            "plan binding (bound_plan_id, bound_plan_sha256) — no --plan supplied, so the declared subject was not checked against a real plan",
        );
    }
    if observed_without_persistence {
        unverified.push(
            "persistence agreement (bound_persistence_result) — no --persistence supplied, so the outcome this observation claims to follow was not checked",
        );
    }
    for skipped in &unverified {
        println!("standalone-path-persistence: NOT VERIFIED: {skipped}");
    }
    if !unverified.is_empty() {
        println!(
            "standalone-path-persistence: {} cross-document law(s) were not evaluated by this invocation",
            unverified.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ContractError, ContractResult, FreshProcessObservation, PathPlan, PersistenceReceipt,
        canonical_json, sha256_hex, validate_fresh_process,
        validate_fresh_process_against_persistence, validate_fresh_process_against_plan,
        validate_persistence, validate_persistence_against_plan, validate_plan,
    };
    use color_eyre::eyre::{Result, bail, ensure};
    use serde_json::{Value, json};

    const FIXTURES: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/experience/install_path_persistence");

    fn fixture_text(name: &str) -> Result<String> {
        std::fs::read_to_string(format!("{FIXTURES}/{name}"))
            .map_err(|error| color_eyre::eyre::eyre!("fixture {name} unreadable: {error}"))
    }

    fn digest_of(name: &str) -> Result<String> {
        Ok(sha256_hex(fixture_text(name)?.as_bytes()))
    }

    fn parse<T: serde::de::DeserializeOwned>(name: &str) -> Result<T> {
        let text = fixture_text(name)?;
        serde_json::from_str(&text)
            .map_err(|error| color_eyre::eyre::eyre!("fixture {name} does not parse: {error}"))
    }

    /// Load a fixture, mutate one field, and re-type it. A document that no
    /// longer types is itself a rejection, so the battery reports that too.
    fn mutated<T: serde::de::DeserializeOwned>(
        name: &str,
        mutate: impl FnOnce(&mut Value),
    ) -> ContractResult<T> {
        let text = std::fs::read_to_string(format!("{FIXTURES}/{name}"))
            .map_err(|error| ContractError::new(format!("fixture {name} unreadable: {error}")))?;
        let mut value: Value = serde_json::from_str(&text)
            .map_err(|error| ContractError::new(format!("parse error: {error}")))?;
        mutate(&mut value);
        serde_json::from_value(value)
            .map_err(|error| ContractError::new(format!("parse error: {error}")))
    }

    #[track_caller]
    fn expect_rejected<T>(outcome: ContractResult<T>, needle: &str) -> Result<()> {
        match outcome {
            Ok(_) => bail!("expected rejection mentioning {needle:?}, but validation passed"),
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

    const POSITIVE_PLANS: [&str; 5] = [
        "plan_posix_installer_user_scope.json",
        "plan_windows_registry_user_path.json",
        "plan_package_manager_owned.json",
        "plan_manual_instruction_only.json",
        "plan_wsl_already_visible.json",
    ];

    /// Every persistence receipt and fresh-process observation, with the plan it
    /// binds. Keeping the pairing here is what lets the battery cross-validate.
    const BOUND_PERSISTENCE: [(&str, &str); 8] = [
        ("persistence_installer_persisted.json", "plan_posix_installer_user_scope.json"),
        ("persistence_already_visible_no_change.json", "plan_wsl_already_visible.json"),
        ("persistence_conflict_wrong_existing_entry.json", "plan_posix_installer_user_scope.json"),
        ("persistence_permission_or_lock_failure.json", "plan_windows_registry_user_path.json"),
        ("persistence_instrument_failure.json", "plan_windows_registry_user_path.json"),
        ("persistence_package_manager_owned.json", "plan_package_manager_owned.json"),
        ("persistence_manual_action_required.json", "plan_manual_instruction_only.json"),
        ("persistence_conflict_user_edited_entry.json", "plan_posix_installer_user_scope.json"),
    ];

    const BOUND_OBSERVATIONS: [(&str, &str); 6] = [
        ("fresh_visible_after_documented_new_session.json", "plan_posix_installer_user_scope.json"),
        ("fresh_visible_immediately.json", "plan_wsl_already_visible.json"),
        ("fresh_wrong_ambient_binary.json", "plan_posix_installer_user_scope.json"),
        ("fresh_ambiguous_resolution.json", "plan_posix_installer_user_scope.json"),
        ("fresh_not_found.json", "plan_manual_instruction_only.json"),
        ("fresh_package_manager_owned_path.json", "plan_package_manager_owned.json"),
    ];

    // ── positive controls ───────────────────────────────────────────────────

    #[test]
    fn every_committed_plan_validates() -> Result<()> {
        for name in POSITIVE_PLANS {
            let plan: PathPlan = parse(name)?;
            validate_plan(&plan).map_err(|error| {
                color_eyre::eyre::eyre!("plan fixture {name} must validate: {error}")
            })?;
        }
        Ok(())
    }

    #[test]
    fn every_committed_persistence_receipt_validates_against_its_plan() -> Result<()> {
        for (receipt_name, plan_name) in BOUND_PERSISTENCE {
            let receipt: PersistenceReceipt = parse(receipt_name)?;
            let plan: PathPlan = parse(plan_name)?;
            validate_persistence_against_plan(&receipt, &plan, &digest_of(plan_name)?).map_err(
                |error| {
                    color_eyre::eyre::eyre!(
                        "{receipt_name} must validate against {plan_name}: {error}"
                    )
                },
            )?;
        }
        Ok(())
    }

    #[test]
    fn every_committed_observation_validates_against_its_plan() -> Result<()> {
        for (observation_name, plan_name) in BOUND_OBSERVATIONS {
            let observation: FreshProcessObservation = parse(observation_name)?;
            let plan: PathPlan = parse(plan_name)?;
            validate_fresh_process_against_plan(&observation, &plan, &digest_of(plan_name)?)
                .map_err(|error| {
                    color_eyre::eyre::eyre!(
                        "{observation_name} must validate against {plan_name}: {error}"
                    )
                })?;
        }
        Ok(())
    }

    #[test]
    fn a_paired_observation_and_receipt_agree_about_the_same_plan() -> Result<()> {
        let observation: FreshProcessObservation =
            parse("fresh_visible_after_documented_new_session.json")?;
        let receipt: PersistenceReceipt = parse("persistence_installer_persisted.json")?;
        validate_fresh_process_against_persistence(&observation, &receipt)
            .map_err(|error| color_eyre::eyre::eyre!("paired documents must agree: {error}"))?;
        Ok(())
    }

    // ── negative controls ───────────────────────────────────────────────────

    /// Issue negative control 1: an exact candidate path is not PATH success.
    #[test]
    fn a_valid_candidate_does_not_make_persistence_true() -> Result<()> {
        let receipt: PersistenceReceipt =
            parse("persistence_invalid_persisted_without_mutation.json")?;
        expect_rejected(validate_persistence(&receipt), "must have written durable state")
    }

    /// Issue negative control 2: a profile or registry write is not a
    /// fresh-process observation, and an outcome that wrote nothing cannot back
    /// a visibility claim about the same plan.
    #[test]
    fn a_write_that_did_not_happen_cannot_back_a_visibility_claim() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_visible_after_documented_new_session.json", |value| {
                value["bound_persistence_result"] = json!("manual_action_required");
            })?;
        let receipt: PersistenceReceipt =
            mutated("persistence_installer_persisted.json", |value| {
                value["result"] = json!("manual_action_required");
                value["mutation_performed"] = json!(false);
                value["entry_state"] = json!("absent");
                value["owned_entries_observed"] = json!(0);
                value["current_process_env_mutated"] = json!(false);
            })?;
        validate_persistence(&receipt)
            .map_err(|error| color_eyre::eyre::eyre!("control receipt must be valid: {error}"))?;
        expect_rejected(
            validate_fresh_process_against_persistence(&observation, &receipt),
            "contradicts persistence outcome",
        )
    }

    /// Two documents about two different plans never compose into one claim.
    #[test]
    fn documents_bound_to_different_plans_do_not_compose() -> Result<()> {
        let observation: FreshProcessObservation =
            parse("fresh_visible_after_documented_new_session.json")?;
        let receipt: PersistenceReceipt = parse("persistence_manual_action_required.json")?;
        expect_rejected(
            validate_fresh_process_against_persistence(&observation, &receipt),
            "describe different plans",
        )
    }

    /// Issue negative control 3a: a current-process prepend is not a fresh process.
    #[test]
    fn a_current_process_prepend_cannot_prove_discoverability() -> Result<()> {
        let observation: FreshProcessObservation =
            parse("fresh_invalid_current_process_prepend.json")?;
        expect_rejected(validate_fresh_process(&observation), "requires a genuinely new session")
    }

    /// Issue negative control 3b: an absolute-path launch is not a PATH lookup.
    #[test]
    fn an_absolute_path_launch_cannot_prove_discoverability() -> Result<()> {
        let observation: FreshProcessObservation =
            parse("fresh_invalid_absolute_path_launch.json")?;
        expect_rejected(
            validate_fresh_process(&observation),
            "proves nothing about command discoverability",
        )
    }

    /// Issue negative control 3c: a harness that prepared PATH is testing itself.
    #[test]
    fn a_harness_prepared_path_cannot_prove_discoverability() -> Result<()> {
        let observation: FreshProcessObservation =
            parse("fresh_invalid_harness_prepared_path.json")?;
        expect_rejected(
            validate_fresh_process(&observation),
            "cannot rest on a PATH the harness prepared",
        )
    }

    /// Issue negative control 4: a wrong ambient binary is not ignorable because
    /// the selected candidate exists somewhere.
    #[test]
    fn a_wrong_ambient_binary_cannot_be_reported_as_success() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_wrong_ambient_binary.json", |value| {
                value["result"] = json!("path_visible_immediately");
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "resolved a binary that is not the selected candidate",
        )
    }

    /// The same law from the other direction: a resolved non-candidate cannot be
    /// laundered into any non-wrong-ambient outcome.
    #[test]
    fn a_resolved_non_candidate_is_wrong_ambient_not_not_found() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_wrong_ambient_binary.json", |value| {
                value["result"] = json!("not_found");
            })?;
        expect_rejected(validate_fresh_process(&observation), "which is `wrong_ambient_binary`")
    }

    /// Issue negative control 5: Windows evidence does not satisfy WSL or POSIX,
    /// and one shell does not satisfy all shells.
    #[test]
    fn one_platform_does_not_satisfy_another() -> Result<()> {
        let plan: PathPlan = parse("plan_wsl_already_visible.json")?;
        let digest = digest_of("plan_wsl_already_visible.json")?;
        let observation: FreshProcessObservation =
            mutated("fresh_visible_immediately.json", |value| {
                value["observed_environment"]["platform"] = json!("windows_native");
                value["observed_environment"]["shell_family"] = json!("powershell");
            })?;
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &digest),
            "not interchangeable",
        )
    }

    #[test]
    fn one_shell_does_not_satisfy_a_shell_specific_policy() -> Result<()> {
        let plan: PathPlan = parse("plan_manual_instruction_only.json")?;
        let digest = digest_of("plan_manual_instruction_only.json")?;
        let observation: FreshProcessObservation = mutated("fresh_not_found.json", |value| {
            value["observed_environment"]["shell_family"] = json!("zsh");
        })?;
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &digest),
            "only proven in the exact shell it targets",
        )
    }

    /// Issue negative control 6: a duplicate, case, or alias entry is not the one
    /// canonical installer-owned entry.
    #[test]
    fn a_duplicate_owned_entry_is_not_idempotent_success() -> Result<()> {
        let receipt: PersistenceReceipt =
            mutated("persistence_installer_persisted.json", |value| {
                value["owned_entries_observed"] = json!(2);
            })?;
        expect_rejected(
            validate_persistence(&receipt),
            "requires exactly one canonical owned entry",
        )
    }

    /// Issue negative control 7: user and system scope are never silently widened.
    #[test]
    fn an_observed_scope_cannot_widen_the_planned_scope() -> Result<()> {
        let plan: PathPlan = parse("plan_posix_installer_user_scope.json")?;
        let digest = digest_of("plan_posix_installer_user_scope.json")?;
        let receipt: PersistenceReceipt =
            mutated("persistence_installer_persisted.json", |value| {
                value["observed_scope"] = json!("system_elevated");
            })?;
        expect_rejected(
            validate_persistence_against_plan(&receipt, &plan, &digest),
            "never silently widened",
        )
    }

    /// Issue negative control 8: a non-installer owner owns no entry, so it cannot
    /// declare one or report having added one.
    #[test]
    fn a_non_installer_owner_cannot_declare_an_owned_entry() -> Result<()> {
        let plan: ContractResult<PathPlan> = mutated("plan_package_manager_owned.json", |value| {
            value["path_policy"]["owned_entry"] = json!({
                "entry_kind": "profile_line",
                "entry_location": "/home/operator/.profile",
                "entry_value": "/opt/homebrew/Cellar/perl-lsp/bin",
                "marker": "perl-lsp-installer-owned-path-entry"
            });
        });
        expect_rejected(plan.and_then(|plan| validate_plan(&plan)), "cannot be laundered")
    }

    #[test]
    fn an_added_entry_requires_a_plan_that_authorized_one() -> Result<()> {
        let plan: PathPlan = parse("plan_package_manager_owned.json")?;
        let digest = digest_of("plan_package_manager_owned.json")?;
        let receipt: PersistenceReceipt =
            mutated("persistence_package_manager_owned.json", |value| {
                value["result"] = json!("installer_persisted");
                value["mutation_performed"] = json!(true);
                value["entry_state"] = json!("added");
            })?;
        expect_rejected(
            validate_persistence_against_plan(&receipt, &plan, &digest),
            "requires plan mutation owner",
        )
    }

    /// Issue negative control 9: complete PATH, profile, and home values never
    /// enter a durable receipt.
    #[test]
    fn a_complete_path_value_cannot_enter_a_durable_document() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["path_policy"]["owned_entry"]["entry_value"] =
                    json!("/home/operator/.local/share/perl-lsp/bin:/usr/bin:/bin");
            });
        expect_rejected(plan.and_then(|plan| validate_plan(&plan)), "never the complete PATH value")
    }

    #[test]
    fn a_home_expansion_cannot_enter_a_durable_document() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["path_policy"]["owned_entry"]["entry_location"] = json!("~/.profile");
            });
        expect_rejected(
            plan.and_then(|plan| validate_plan(&plan)),
            "is shell, script, registry, or environment expansion syntax",
        )
    }

    #[test]
    fn shell_expansion_in_path_text_cannot_reach_a_profile() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["path_policy"]["owned_entry"]["entry_value"] =
                    json!("/home/operator/$(id -u)/bin");
            });
        expect_rejected(
            plan.and_then(|plan| validate_plan(&plan)),
            "is shell, script, registry, or environment expansion syntax",
        )
    }

    /// Issue negative control 10, proven against this binary's own source: this
    /// contract lane mutates no platform state.
    #[test]
    fn this_validator_performs_no_platform_mutation() -> Result<()> {
        let source = include_str!("standalone_path_persistence.rs");
        let forbidden = [
            concat!("fs::", "write"),
            concat!("fs::", "remove_file"),
            concat!("fs::", "create_dir"),
            concat!("fs::", "OpenOptions"),
            concat!("set_", "var"),
            concat!("Command", "::new"),
            concat!("std::process::", "Command"),
        ];
        for needle in forbidden {
            ensure!(
                !source.contains(needle),
                "this contract validates documents only; `{needle}` would mutate or launch platform state"
            );
        }
        Ok(())
    }

    // ── remaining issue fixture cases ───────────────────────────────────────

    /// Fixture case: the confirmation gate is a hard predecessor to mutation.
    #[test]
    fn an_unconfirmed_candidate_cannot_reach_installer_owned_mutation() -> Result<()> {
        let plan: PathPlan = parse("plan_invalid_unconfirmed_candidate.json")?;
        expect_rejected(validate_plan(&plan), "requires `confirmed_current`")
    }

    /// Fixture case: a session-only change is never durable installer persistence.
    #[test]
    fn a_session_only_scope_cannot_be_installer_owned() -> Result<()> {
        let plan: PathPlan = parse("plan_invalid_session_only_installer.json")?;
        expect_rejected(validate_plan(&plan), "not durable persistence")
    }

    #[test]
    fn a_session_only_observation_cannot_claim_a_visible_entry() -> Result<()> {
        let receipt: PersistenceReceipt = parse("persistence_invalid_session_only_visible.json")?;
        expect_rejected(validate_persistence(&receipt), "not durable persistence")
    }

    /// Fixture case: PATH is never pointed at a directory the installer does not own.
    #[test]
    fn an_owned_entry_outside_the_install_root_is_refused() -> Result<()> {
        let plan: PathPlan = parse("plan_invalid_entry_outside_root.json")?;
        expect_rejected(validate_plan(&plan), "is not under install root")
    }

    /// Fixture case: the candidate selection changed before the observation, so
    /// the observation proves nothing about the current candidate.
    #[test]
    fn a_selection_that_moved_invalidates_the_observation() -> Result<()> {
        let plan: PathPlan = parse("plan_posix_installer_user_scope.json")?;
        let observation: FreshProcessObservation =
            parse("fresh_visible_after_documented_new_session.json")?;
        let stale = "f".repeat(64);
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &stale),
            "the selection moved before the observation",
        )
    }

    #[test]
    fn a_plan_that_moved_invalidates_the_persistence_receipt() -> Result<()> {
        let plan: PathPlan = parse("plan_posix_installer_user_scope.json")?;
        let receipt: PersistenceReceipt = parse("persistence_installer_persisted.json")?;
        let stale = "f".repeat(64);
        expect_rejected(
            validate_persistence_against_plan(&receipt, &plan, &stale),
            "the policy it was produced under moved",
        )
    }

    /// Fixture case: a failed instrument is never represented clean, in either
    /// direction.
    #[test]
    fn an_incomplete_instrument_cannot_conclude_a_real_outcome() -> Result<()> {
        let receipt: PersistenceReceipt =
            mutated("persistence_instrument_failure.json", |value| {
                value["result"] = json!("already_visible_no_change");
                value["entry_state"] = json!("already_present");
                value["owned_entries_observed"] = json!(1);
            })?;
        expect_rejected(
            validate_persistence(&receipt),
            "an incomplete instrument cannot conclude",
        )?;

        let observation: FreshProcessObservation =
            mutated("fresh_visible_immediately.json", |value| {
                value["instrument_complete"] = json!(false);
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "an incomplete instrument cannot conclude",
        )
    }

    #[test]
    fn a_complete_instrument_cannot_report_instrument_failure() -> Result<()> {
        let receipt: PersistenceReceipt =
            mutated("persistence_instrument_failure.json", |value| {
                value["instrument_complete"] = json!(true);
            })?;
        expect_rejected(
            validate_persistence(&receipt),
            "cannot be reported by a complete instrument",
        )
    }

    /// Fixture case: ambiguity stays visible rather than resolving itself into a
    /// convenient winner.
    #[test]
    fn an_ambiguous_resolution_cannot_be_reported_as_a_clean_outcome() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_ambiguous_resolution.json", |value| {
                value["result"] = json!("not_found");
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "an ambiguous resolution cannot be reported as",
        )
    }

    #[test]
    fn an_ambiguous_outcome_requires_at_least_two_observed_candidates() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_ambiguous_resolution.json", |value| {
                value["lookup"]["competing_candidates"] = json!([
                    {"path": "/usr/local/bin/perllsp", "sha256": null}
                ]);
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "requires at least two observed candidates",
        )
    }

    /// Fixture case: `not_found` cannot hide candidates it did observe.
    #[test]
    fn not_found_cannot_hide_observed_candidates() -> Result<()> {
        let observation: FreshProcessObservation = mutated("fresh_not_found.json", |value| {
            value["lookup"]["competing_candidates"] =
                json!([{"path": "/usr/local/bin/perllsp", "sha256": null}]);
        })?;
        expect_rejected(validate_fresh_process(&observation), "cannot have observed candidates")
    }

    /// Fixture case: deterministic serialization. The observed candidate set is
    /// an exact ascending set, so one observation has one spelling.
    #[test]
    fn the_observed_candidate_set_is_exact_and_deterministic() -> Result<()> {
        let unsorted: ContractResult<FreshProcessObservation> =
            mutated("fresh_ambiguous_resolution.json", |value| {
                let candidates = value["lookup"]["competing_candidates"].clone();
                let mut items = candidates.as_array().cloned().unwrap_or_default();
                items.reverse();
                value["lookup"]["competing_candidates"] = Value::Array(items);
            });
        expect_rejected(
            unsorted.and_then(|observation| validate_fresh_process(&observation)),
            "strictly ascending unique set",
        )?;

        let duplicated: ContractResult<FreshProcessObservation> =
            mutated("fresh_ambiguous_resolution.json", |value| {
                value["lookup"]["competing_candidates"] = json!([
                    {"path": "/usr/local/bin/perllsp", "sha256": null},
                    {"path": "/usr/local/bin/perllsp", "sha256": null}
                ]);
            });
        expect_rejected(
            duplicated.and_then(|observation| validate_fresh_process(&observation)),
            "strictly ascending unique set",
        )
    }

    #[test]
    fn a_resolved_executable_must_belong_to_the_observed_candidate_set() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_wrong_ambient_binary.json", |value| {
                value["lookup"]["resolved"]["path"] = json!("/opt/elsewhere/perllsp");
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "is not among the observed candidates",
        )
    }

    #[test]
    fn canonical_serialization_is_key_order_independent() -> Result<()> {
        let text = fixture_text("plan_posix_installer_user_scope.json")?;
        let parsed: Value = serde_json::from_str(&text)?;
        let reordered: Value = serde_json::from_str(&serde_json::to_string(&parsed)?)?;
        ensure!(
            canonical_json(&parsed) == canonical_json(&reordered),
            "canonical serialization must not depend on key insertion order"
        );
        Ok(())
    }

    /// The candidate digest, not the claim, decides whether the resolved binary
    /// is the candidate.
    #[test]
    fn a_matches_candidate_claim_must_agree_with_the_digest() -> Result<()> {
        let plan: PathPlan = parse("plan_posix_installer_user_scope.json")?;
        let digest = digest_of("plan_posix_installer_user_scope.json")?;
        let observation: FreshProcessObservation =
            mutated("fresh_wrong_ambient_binary.json", |value| {
                value["lookup"]["resolved"]["matches_candidate"] = json!(true);
                value["result"] = json!("path_visible_after_documented_new_session");
            })?;
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &digest),
            "differs from",
        )
    }

    /// A documented new-session boundary is reported, not elided into immediacy.
    #[test]
    fn a_documented_new_session_boundary_is_not_reported_as_immediate() -> Result<()> {
        let plan: PathPlan = parse("plan_posix_installer_user_scope.json")?;
        let digest = digest_of("plan_posix_installer_user_scope.json")?;
        let observation: FreshProcessObservation =
            mutated("fresh_visible_after_documented_new_session.json", |value| {
                value["result"] = json!("path_visible_immediately");
            })?;
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &digest),
            "not immediate",
        )
    }

    #[test]
    fn an_added_entry_requires_a_durable_write_under_any_outcome_word() -> Result<()> {
        let receipt: PersistenceReceipt =
            mutated("persistence_installer_persisted.json", |value| {
                value["result"] = json!("new_session_required");
                value["mutation_performed"] = json!(false);
            })?;
        expect_rejected(validate_persistence(&receipt), "requires a durable write")
    }

    #[test]
    fn a_conflict_must_name_the_state_it_conflicts_with() -> Result<()> {
        let receipt: PersistenceReceipt =
            mutated("persistence_conflict_wrong_existing_entry.json", |value| {
                value["conflicting_entry"] = Value::Null;
            })?;
        expect_rejected(validate_persistence(&receipt), "must name the exact conflicting state")
    }

    #[test]
    fn a_new_session_boundary_the_plan_never_documented_cannot_be_reported() -> Result<()> {
        let plan: PathPlan = parse("plan_wsl_already_visible.json")?;
        let digest = digest_of("plan_wsl_already_visible.json")?;
        let receipt: PersistenceReceipt =
            mutated("persistence_already_visible_no_change.json", |value| {
                value["new_session_required"] = json!(true);
            })?;
        expect_rejected(
            validate_persistence_against_plan(&receipt, &plan, &digest),
            "documents no new-session boundary",
        )
    }

    #[test]
    fn a_command_name_with_a_separator_is_not_a_path_lookup() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["subject"]["command_name"] = json!("bin/perllsp");
            });
        expect_rejected(
            plan.and_then(|plan| validate_plan(&plan)),
            "is a path invocation, not a PATH lookup",
        )
    }

    #[test]
    fn a_candidate_outside_the_install_root_is_not_this_installers_candidate() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["subject"]["executable_path"] = json!("/usr/local/bin/perllsp");
            });
        expect_rejected(plan.and_then(|plan| validate_plan(&plan)), "is not under install root")
    }

    #[test]
    fn a_persistence_mechanism_foreign_to_the_platform_is_refused() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["path_policy"]["owned_entry"]["entry_kind"] = json!("registry_user_path");
            });
        expect_rejected(
            plan.and_then(|plan| validate_plan(&plan)),
            "is not a persistence mechanism on platform",
        )
    }

    /// Anti-vacuity: each committed invalid fixture must become valid when, and
    /// only when, the single field it violates is repaired. Without this, a
    /// negative control could be passing for an unrelated reason.
    #[test]
    fn every_invalid_fixture_is_valid_once_its_one_violation_is_repaired() -> Result<()> {
        let plan_repairs: [(&str, fn(&mut Value)); 3] = [
            ("plan_invalid_unconfirmed_candidate.json", |value| {
                value["subject"]["confirmation_state"] = json!("confirmed_current");
            }),
            ("plan_invalid_session_only_installer.json", |value| {
                value["path_policy"]["scope"] = json!("user");
            }),
            ("plan_invalid_entry_outside_root.json", |value| {
                value["path_policy"]["owned_entry"]["entry_value"] =
                    json!("/home/operator/.local/share/perl-lsp/bin");
            }),
        ];
        for (name, repair) in plan_repairs {
            let plan: PathPlan = mutated(name, repair)
                .map_err(|error| color_eyre::eyre::eyre!("{name} must re-type: {error}"))?;
            validate_plan(&plan).map_err(|error| {
                color_eyre::eyre::eyre!(
                    "{name} must be valid once its one violation is repaired, so the negative control is not passing for an unrelated reason: {error}"
                )
            })?;
        }

        let receipt_repairs: [(&str, fn(&mut Value)); 2] = [
            ("persistence_invalid_persisted_without_mutation.json", |value| {
                value["mutation_performed"] = json!(true);
            }),
            ("persistence_invalid_session_only_visible.json", |value| {
                value["observed_scope"] = json!("user");
                value["current_process_env_mutated"] = json!(false);
            }),
        ];
        for (name, repair) in receipt_repairs {
            let receipt: PersistenceReceipt = mutated(name, repair)
                .map_err(|error| color_eyre::eyre::eyre!("{name} must re-type: {error}"))?;
            validate_persistence(&receipt).map_err(|error| {
                color_eyre::eyre::eyre!(
                    "{name} must be valid once its one violation is repaired: {error}"
                )
            })?;
        }

        let observation_repairs: [(&str, fn(&mut Value)); 3] = [
            ("fresh_invalid_absolute_path_launch.json", |value| {
                value["lookup"]["method"] = json!("command_lookup");
            }),
            ("fresh_invalid_harness_prepared_path.json", |value| {
                value["session"]["path_prepared_by_harness"] = json!(false);
            }),
            ("fresh_invalid_current_process_prepend.json", |value| {
                value["session"]["origin"] = json!("new_shell_process");
                value["session"]["environment_source"] = json!("ambient_session");
            }),
        ];
        for (name, repair) in observation_repairs {
            let observation: FreshProcessObservation = mutated(name, repair)
                .map_err(|error| color_eyre::eyre::eyre!("{name} must re-type: {error}"))?;
            validate_fresh_process(&observation).map_err(|error| {
                color_eyre::eyre::eyre!(
                    "{name} must be valid once its one violation is repaired: {error}"
                )
            })?;
        }

        Ok(())
    }

    /// Issue fixture case: a user-edited managed marker. The installer owns the
    /// entry, but the user changed it, so it is a conflict to report rather than
    /// state to silently overwrite.
    #[test]
    fn a_user_edited_owned_entry_is_a_conflict_not_an_overwrite() -> Result<()> {
        let receipt: PersistenceReceipt = parse("persistence_conflict_user_edited_entry.json")?;
        validate_persistence(&receipt)
            .map_err(|error| color_eyre::eyre::eyre!("the fixture must validate: {error}"))?;
        ensure!(!receipt.mutation_performed, "a user-edited owned entry must not be overwritten");

        // The same observation cannot be laundered into a clean outcome.
        let overwritten: PersistenceReceipt =
            mutated("persistence_conflict_user_edited_entry.json", |value| {
                value["result"] = json!("installer_persisted");
                value["mutation_performed"] = json!(true);
                value["entry_state"] = json!("added");
            })?;
        expect_rejected(
            validate_persistence(&overwritten),
            "an observed conflict cannot be reported under",
        )
    }

    /// An identifier is a correlation token, not a smuggling channel. Every
    /// path-shaped field was checked while these four were free text, so a
    /// complete PATH rode through the full validator untouched.
    #[test]
    fn an_identifier_cannot_carry_a_complete_path_or_a_private_value() -> Result<()> {
        let leaks = [
            "leaked-PATH=/usr/local/bin:/usr/bin:/home/operator/.ssh",
            "/home/operator/.aws/credentials",
            "$HOME/.secret",
            "id with spaces",
        ];
        for leak in leaks {
            let plan: ContractResult<PathPlan> =
                mutated("plan_posix_installer_user_scope.json", |value| {
                    value["subject"]["candidate_id"] = json!(leak);
                });
            expect_rejected(
                plan.and_then(|plan| validate_plan(&plan)),
                "an identifier must be an ASCII token",
            )?;
        }

        let long: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["plan_id"] = json!("a".repeat(129));
            });
        expect_rejected(
            long.and_then(|plan| validate_plan(&plan)),
            "bounded token of at most 128 characters",
        )?;

        let receipt: ContractResult<PersistenceReceipt> =
            mutated("persistence_installer_persisted.json", |value| {
                value["bound_plan_id"] = json!("/etc/paths:/usr/bin");
            });
        expect_rejected(
            receipt.and_then(|receipt| validate_persistence(&receipt)),
            "an identifier must be an ASCII token",
        )
    }

    /// Windows and UNC paths are case-insensitive, so two spellings of one
    /// directory are one directory. Refusing them rejects a legitimate document
    /// rather than catching a laundering attempt.
    #[test]
    fn windows_root_containment_is_case_insensitive() -> Result<()> {
        let plan: PathPlan = mutated("plan_windows_registry_user_path.json", |value| {
            value["environment"]["install_root"]["path"] =
                json!("C:\\users\\operator\\appdata\\local\\perl-lsp");
        })?;
        validate_plan(&plan).map_err(|error| {
            color_eyre::eyre::eyre!("windows paths are case-insensitive: {error}")
        })?;

        // POSIX stays exact: case is meaningful there.
        let posix: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["environment"]["install_root"]["path"] =
                    json!("/home/operator/.local/share/PERL-LSP");
            });
        expect_rejected(posix.and_then(|plan| validate_plan(&plan)), "is not under install root")
    }

    // ── review findings (PR #16014, Codex) ──────────────────────────────────

    /// Command lookup is platform-specific. A POSIX lookup for `perllsp` cannot
    /// resolve a file named `perllsp.exe`, so the plan must not accept one.
    #[test]
    fn an_exe_suffix_is_not_stripped_under_posix_lookup_rules() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["subject"]["executable_path"] =
                    json!("/home/operator/.local/share/perl-lsp/versions/0.18.0/bin/perllsp.exe");
            });
        expect_rejected(plan.and_then(|plan| validate_plan(&plan)), "under `posix` lookup rules")
    }

    /// Native Windows lookup is case-insensitive, so a `.EXE` spelling names the
    /// same command and must be accepted.
    #[test]
    fn windows_lookup_matches_the_command_case_insensitively() -> Result<()> {
        let plan: PathPlan = mutated("plan_windows_registry_user_path.json", |value| {
            value["subject"]["executable_path"] = json!(
                "C:\\Users\\operator\\AppData\\Local\\perl-lsp\\versions\\0.18.0\\bin\\PerlLSP.EXE"
            );
        })?;
        validate_plan(&plan).map_err(|error| {
            color_eyre::eyre::eyre!("windows lookup is case-insensitive: {error}")
        })?;
        Ok(())
    }

    /// A multi-byte tail must reach the typed refusal, not a panic. `len() - 4`
    /// can land inside a codepoint, and path validation accepts non-ASCII, so
    /// this is reachable from a well-formed document.
    #[test]
    fn a_unicode_filename_tail_refuses_instead_of_panicking() -> Result<()> {
        // "\u{1F600}x" is five bytes; len - 4 = 1 lands on a continuation byte.
        let plan: ContractResult<PathPlan> =
            mutated("plan_windows_registry_user_path.json", |value| {
                value["subject"]["executable_path"] =
                    json!("C:\\Users\\operator\\AppData\\Local\\perl-lsp\\\u{1F600}x");
            });
        expect_rejected(
            plan.and_then(|plan| validate_plan(&plan)),
            "does not name the executable",
        )?;

        // Same tail, and now the command name matches it exactly: still no panic,
        // and the document is accepted on its merits rather than by accident.
        let matching: PathPlan = mutated("plan_windows_registry_user_path.json", |value| {
            value["subject"]["executable_path"] =
                json!("C:\\Users\\operator\\AppData\\Local\\perl-lsp\\\u{1F600}x");
            value["subject"]["command_name"] = json!("\u{1F600}x");
        })?;
        validate_plan(&matching).map_err(|error| {
            color_eyre::eyre::eyre!("a multi-byte name is not invalid: {error}")
        })?;
        Ok(())
    }

    /// The same byte-offset class in install-root containment. A root whose byte
    /// length lands inside a multi-byte segment of the candidate path must be
    /// judged, not panicked on.
    #[test]
    fn unicode_root_containment_refuses_instead_of_panicking() -> Result<()> {
        // root "C:\\a" is 4 bytes; the candidate's 4th byte is inside "\u{1F600}".
        let plan: ContractResult<PathPlan> =
            mutated("plan_windows_registry_user_path.json", |value| {
                value["environment"]["install_root"]["path"] = json!("C:\\a");
                value["subject"]["executable_path"] = json!("C:\\\u{1F600}\\perllsp.exe");
            });
        expect_rejected(plan.and_then(|plan| validate_plan(&plan)), "is not under install root")?;

        // A Unicode segment genuinely under the root is still accepted, so the
        // guard does not turn every non-ASCII path into a refusal.
        let nested: PathPlan = mutated("plan_windows_registry_user_path.json", |value| {
            value["subject"]["executable_path"] = json!(
                "C:\\Users\\operator\\AppData\\Local\\perl-lsp\\\u{65E5}\u{672C}\\perllsp.exe"
            );
        })?;
        validate_plan(&nested).map_err(|error| {
            color_eyre::eyre::eyre!("a Unicode segment under the root is valid: {error}")
        })?;
        Ok(())
    }

    /// A genuine Unicode name ending in `.exe` still strips correctly: the
    /// boundary guard must not break the case it was protecting.
    #[test]
    fn a_unicode_name_ending_in_exe_still_strips_on_windows() -> Result<()> {
        let plan: PathPlan = mutated("plan_windows_registry_user_path.json", |value| {
            value["subject"]["executable_path"] =
                json!("C:\\Users\\operator\\AppData\\Local\\perl-lsp\\\u{65E5}\u{672C}.exe");
            value["subject"]["command_name"] = json!("\u{65E5}\u{672C}");
        })?;
        validate_plan(&plan).map_err(|error| {
            color_eyre::eyre::eyre!("a Unicode stem before `.exe` must still strip: {error}")
        })?;
        Ok(())
    }

    /// One observation carries one identity for the binary that won.
    #[test]
    fn the_winner_and_the_candidate_set_cannot_disagree_about_one_path() -> Result<()> {
        let observation: FreshProcessObservation =
            mutated("fresh_wrong_ambient_binary.json", |value| {
                value["lookup"]["competing_candidates"][1]["sha256"] = json!("a".repeat(64));
            })?;
        expect_rejected(
            validate_fresh_process(&observation),
            "one observation carries one identity",
        )
    }

    /// A logout/login boundary is strictly stronger than a new shell, and only a
    /// new login session exercises it.
    #[test]
    fn a_logout_login_boundary_is_not_proven_by_a_new_shell() -> Result<()> {
        let plan: PathPlan = parse("plan_windows_registry_user_path.json")?;
        let digest = digest_of("plan_windows_registry_user_path.json")?;
        let observation: FreshProcessObservation = mutated(
            "fresh_visible_after_documented_new_session.json",
            |value| {
                value["bound_plan_id"] = json!("path-plan-windows-registry-user");
                value["bound_plan_sha256"] = json!(digest_of_windows_plan());
                value["observed_environment"] = json!({
                    "platform": "windows_native",
                    "shell_family": "powershell"
                });
                value["session"]["origin"] = json!("new_shell_process");
                value["lookup"]["resolved"] = json!({
                    "path": "C:\\Users\\operator\\AppData\\Local\\perl-lsp\\versions\\0.18.0\\bin\\perllsp.exe",
                    "sha256": "e4".to_string() + &"0".repeat(62),
                    "matches_candidate": true
                });
                value["lookup"]["competing_candidates"] = json!([{
                    "path": "C:\\Users\\operator\\AppData\\Local\\perl-lsp\\versions\\0.18.0\\bin\\perllsp.exe",
                    "sha256": "e4".to_string() + &"0".repeat(62)
                }]);
            },
        )?;
        expect_rejected(
            validate_fresh_process_against_plan(&observation, &plan, &digest),
            "only `new_login_session` exercises",
        )
    }

    fn digest_of_windows_plan() -> String {
        digest_of("plan_windows_registry_user_path.json").unwrap_or_default()
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() -> Result<()> {
        let plan: ContractResult<PathPlan> =
            mutated("plan_posix_installer_user_scope.json", |value| {
                value["surprise"] = json!("unmodelled");
            });
        expect_rejected(plan, "unknown field")
    }

    #[test]
    fn an_unmodelled_result_word_is_refused() -> Result<()> {
        let receipt: ContractResult<PersistenceReceipt> =
            mutated("persistence_installer_persisted.json", |value| {
                value["result"] = json!("probably_fine");
            });
        expect_rejected(receipt, "unknown variant")
    }
}
