//! The pure plan validator.
//!
//! This is the only way to obtain a [`ValidatedProcessPlan`], and the
//! production supervisor port accepts nothing else. Validation performs no
//! I/O: it decides whether the identities and policies a plan already carries
//! are sufficient for the shape it claims.

use std::fmt;

use super::PROCESS_DOMAIN_SCHEMA_VERSION;
use super::encoding::PlanFingerprint;
use super::environment::{AmbientInheritance, CodeLoadingDisposition, EnvVarName};
use super::identity::{
    AuthorizationStrength, CwdPolicy, EvidenceFreshness, ExecutableResolution, ExecutionProfile,
    ResolutionProvenance,
};
use super::plan::{
    CancellationPolicy, DeadlinePolicy, MAX_CAPTURE_BUDGET_BYTES, ProcessPlan, PublicProjection,
    StdinPolicy, TerminationPolicy,
};

/// Program names that are shells rather than programs.
///
/// A plan whose executable is one of these plus an inline-command flag is a
/// shell invocation wearing structured argv, which the domain refuses.
const SHELL_PROGRAMS: &[&str] = &[
    "sh",
    "bash",
    "dash",
    "ash",
    "rbash",
    "zsh",
    "ksh",
    "csh",
    "tcsh",
    "fish",
    "busybox",
    "cmd",
    "powershell",
    "pwsh",
];

/// Executable suffixes stripped before matching a program against
/// [`SHELL_PROGRAMS`].
///
/// Listing `powershell` but not `powershell.exe` would let the same shell
/// through under the name it actually has on Windows, so the extension is
/// removed rather than enumerated.
const EXECUTABLE_SUFFIXES: &[&str] = &[".exe", ".com", ".bat", ".cmd"];

/// Flags that hand a shell an inline command string.
///
/// `/K` is here alongside `/C`: `cmd.exe /K "..."` runs the command string
/// exactly as `/C` does and merely keeps the interpreter alive afterwards, so
/// omitting it would leave the same door open under a different letter.
const INLINE_COMMAND_FLAGS: &[&str] =
    &["-c", "-command", "/c", "/C", "/k", "/K", "-Command", "-EncodedCommand", "--command"];

/// Shell options whose value is the *next* argument rather than part of the
/// same token.
///
/// These matter because the scan stops at the first operand: without knowing
/// that `errexit` in `bash -o errexit -c 'cmd'` is `-o`'s value and not the
/// script operand, the scan would stop there and never examine the `-c`.
const OPTIONS_TAKING_A_SEPARATE_VALUE: &[&str] = &["-o", "+o", "--rcfile", "--init-file"];

/// Programs that dispatch to an applet named by their first operand.
///
/// For these the operand that would otherwise end option parsing may be the
/// shell itself, so the scan continues past it.
const MULTI_CALL_PROGRAMS: &[&str] = &["busybox", "toybox"];

/// The authorization-evidence scheme version this build can read.
///
/// The reference itself stays opaque — the execution-authorization programme
/// owns what it means — but its scheme version is checked, because evidence
/// from an unknown scheme cannot be assumed to mean what this one means.
pub const SUPPORTED_AUTHORIZATION_SCHEME: super::identity::SchemaVersion =
    super::identity::SchemaVersion::new(1);

/// Prefixes of inline-command flags that carry the command in the same token
/// (`--command=...`), which an exact-match list alone would miss.
const INLINE_COMMAND_PREFIXES: &[&str] = &["--command=", "-Command:", "--command:"];

/// Why a plan may not be started.
///
/// A closed enum: rejection reasons are machine authority, so they are never
/// free-form prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanRejection {
    /// The plan declares a schema version this build does not implement.
    UnsupportedSchemaVersion {
        /// The version the plan declared.
        declared: u32,
        /// The version this build supports.
        supported: u32,
    },
    /// The plan's correlation identity is blank.
    MissingPlanIdentity,
    /// The plan's operation identity is blank.
    MissingOperationIdentity,
    /// The executable's logical name is blank.
    EmptyExecutableIdentity,
    /// The executable was never resolved to an exact path.
    UnresolvedExecutableIdentity,
    /// The executable would be resolved from the ambient environment at spawn.
    AmbientExecutableResolution,
    /// The resolved executable path is not absolute.
    NonAbsoluteExecutablePath,
    /// The plan invokes a shell with an inline command string.
    ShellInvocationRejected {
        /// The shell's logical name.
        shell: String,
    },
    /// An argument, program name, path, or environment entry contains a NUL.
    NulByteInInvocation,
    /// An environment variable name cannot be expressed in an environment block.
    ///
    /// `=` is the name/value separator on every supported platform, and an
    /// empty name has nothing to separate, so neither can survive being handed
    /// to a backend.
    UnrepresentableEnvironmentVariableName {
        /// The offending name.
        variable: String,
    },
    /// The profile requires an exact working directory and none was given.
    AmbientCwdRejected,
    /// The working directory is not an absolute path.
    NonAbsoluteCwd,
    /// Environment rules contradict each other for a variable.
    ContradictoryEnvironmentRules {
        /// The variable named by contradictory rules.
        variable: String,
    },
    /// A code-loading variable is admitted without an explicit acknowledgement.
    UnacknowledgedCodeLoadingVariable {
        /// The admitted variable.
        variable: String,
    },
    /// A hermetic profile admits a code-loading variable or ambient inheritance.
    HermeticProfileViolated,
    /// A capture budget observes nothing.
    ZeroCaptureBudget {
        /// The channel whose budget is zero.
        channel: BudgetChannel,
    },
    /// A capture budget exceeds the maximum bound.
    CaptureBudgetOverflow {
        /// The channel whose budget is too large.
        channel: BudgetChannel,
    },
    /// A budget retains more than it observes.
    InconsistentCaptureBudget {
        /// The channel whose budget is inconsistent.
        channel: BudgetChannel,
    },
    /// The profile requires a deadline and none was given.
    MissingDeadline,
    /// A deadline of zero can never be met.
    ZeroDeadline,
    /// The profile requires cancellation and the plan is not cancellable.
    MissingCancellationPolicy,
    /// The termination policy can never terminate anything.
    ImpossibleTerminationPolicy,
    /// The profile forbids a caller-streamed stdin channel.
    StreamedStdinRejected,
    /// A required subject identity is missing.
    MissingSubjectIdentity,
    /// A supplied opaque identity is blank, so it identifies nothing.
    BlankOpaqueIdentity {
        /// Which identity is blank.
        field: &'static str,
    },
    /// A supplied subject identity is stale or of unknown freshness.
    StaleSubjectIdentity {
        /// The reference that is not current.
        reference: String,
    },
    /// Authorization evidence was required and none was supplied.
    MissingAuthorizationEvidence,
    /// The authorization evidence uses a scheme version this build cannot read.
    UnsupportedAuthorizationScheme {
        /// The scheme version the evidence declared.
        declared: u32,
        /// The scheme version this build reads.
        supported: u32,
    },
    /// The supplied authorization evidence is not current.
    StaleAuthorizationEvidence,
    /// The supplied authorization does not establish authority to execute.
    InsufficientAuthorizationEvidence,
    /// The plan requires a platform the claim boundary does not cover.
    UnsupportedPlatformRequirement,
    /// A public projection would publish values the plan holds privately.
    PublicRetentionWouldExposePrivateValues,
}

/// Which output channel a budget rejection refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BudgetChannel {
    /// The child's standard output.
    Stdout,
    /// The child's standard error.
    Stderr,
}

impl fmt::Display for BudgetChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stdout => f.write_str("stdout"),
            Self::Stderr => f.write_str("stderr"),
        }
    }
}

impl fmt::Display for PlanRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion { declared, supported } => {
                write!(f, "plan declares schema v{declared}; this build supports v{supported}")
            }
            Self::MissingPlanIdentity => f.write_str("plan identity is blank"),
            Self::MissingOperationIdentity => f.write_str("operation identity is blank"),
            Self::EmptyExecutableIdentity => f.write_str("executable logical name is blank"),
            Self::UnresolvedExecutableIdentity => {
                f.write_str("executable was never resolved to an exact path")
            }
            Self::AmbientExecutableResolution => {
                f.write_str("executable would be resolved from the ambient environment at spawn")
            }
            Self::NonAbsoluteExecutablePath => f.write_str("resolved executable path is relative"),
            Self::ShellInvocationRejected { shell } => {
                write!(f, "shell invocation rejected: {shell} with an inline command flag")
            }
            Self::NulByteInInvocation => f.write_str("invocation contains a NUL byte"),
            Self::UnrepresentableEnvironmentVariableName { variable } => {
                write!(f, "environment variable name cannot be expressed: {variable:?}")
            }
            Self::AmbientCwdRejected => f.write_str("profile requires an exact working directory"),
            Self::NonAbsoluteCwd => f.write_str("working directory is not absolute"),
            Self::ContradictoryEnvironmentRules { variable } => {
                write!(f, "contradictory environment rules for {variable}")
            }
            Self::UnacknowledgedCodeLoadingVariable { variable } => {
                write!(f, "code-loading variable {variable} admitted without acknowledgement")
            }
            Self::HermeticProfileViolated => {
                f.write_str("hermetic profile admits ambient or code-loading input")
            }
            Self::ZeroCaptureBudget { channel } => {
                write!(f, "{channel} capture budget observes nothing")
            }
            Self::CaptureBudgetOverflow { channel } => {
                write!(f, "{channel} capture budget exceeds the maximum bound")
            }
            Self::InconsistentCaptureBudget { channel } => {
                write!(f, "{channel} retains more than it observes")
            }
            Self::MissingDeadline => f.write_str("profile requires a wall-clock deadline"),
            Self::ZeroDeadline => f.write_str("deadline of zero can never be met"),
            Self::MissingCancellationPolicy => f.write_str("profile requires a cancellable run"),
            Self::ImpossibleTerminationPolicy => {
                f.write_str("termination policy can never terminate the process tree")
            }
            Self::StreamedStdinRejected => {
                f.write_str("profile forbids a caller-streamed stdin channel")
            }
            Self::MissingSubjectIdentity => f.write_str("required subject identity is missing"),
            Self::BlankOpaqueIdentity { field } => {
                write!(f, "{field} is blank, so it identifies nothing")
            }
            Self::StaleSubjectIdentity { reference } => {
                write!(f, "subject reference {reference} is not current")
            }
            Self::MissingAuthorizationEvidence => {
                f.write_str("authorization evidence is missing or of unestablished freshness")
            }
            Self::UnsupportedAuthorizationScheme { declared, supported } => write!(
                f,
                "authorization evidence declares scheme v{declared}; this build reads v{supported}"
            ),
            Self::StaleAuthorizationEvidence => {
                f.write_str("authorization evidence is not current")
            }
            Self::InsufficientAuthorizationEvidence => {
                f.write_str("authorization evidence does not establish authority to execute")
            }
            Self::UnsupportedPlatformRequirement => {
                f.write_str("claim boundary does not cover the platform the profile requires")
            }
            Self::PublicRetentionWouldExposePrivateValues => {
                f.write_str("public projection would publish privately held values")
            }
        }
    }
}

impl std::error::Error for PlanRejection {}

/// A plan that passed validation.
///
/// The inner plan is private and there is no public constructor: the only way
/// to obtain one is [`ProcessPlan::validate`]. That is what makes validation
/// unbypassable through the production port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProcessPlan {
    plan: ProcessPlan,
    fingerprint: PlanFingerprint,
}

impl ValidatedProcessPlan {
    /// The validated plan.
    pub fn plan(&self) -> &ProcessPlan {
        &self.plan
    }

    /// The plan's public semantic fingerprint, computed once at validation.
    pub fn fingerprint(&self) -> PlanFingerprint {
        self.fingerprint
    }
}

impl ProcessPlan {
    /// Validate the plan, yielding the only startable form.
    pub fn validate(self) -> Result<ValidatedProcessPlan, PlanRejection> {
        validate_schema(&self)?;
        validate_identities(&self)?;
        validate_invocation(&self)?;
        validate_cwd(&self)?;
        validate_environment(&self)?;
        validate_budgets(&self)?;
        validate_lifecycle(&self)?;
        validate_subject(&self)?;
        validate_authorization(&self)?;
        validate_platform(&self)?;
        validate_retention(&self)?;
        let fingerprint = self.semantic_fingerprint();
        Ok(ValidatedProcessPlan { plan: self, fingerprint })
    }
}

fn validate_schema(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    if plan.schema_version() != PROCESS_DOMAIN_SCHEMA_VERSION {
        return Err(PlanRejection::UnsupportedSchemaVersion {
            declared: plan.schema_version().get(),
            supported: PROCESS_DOMAIN_SCHEMA_VERSION.get(),
        });
    }
    Ok(())
}

fn validate_identities(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    if plan.plan_id().is_blank() {
        return Err(PlanRejection::MissingPlanIdentity);
    }
    if plan.operation().is_blank() {
        return Err(PlanRejection::MissingOperationIdentity);
    }
    Ok(())
}

fn validate_invocation(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    let executable = plan.executable();
    let logical_name = executable.logical_name();
    if logical_name.trim().is_empty() {
        return Err(PlanRejection::EmptyExecutableIdentity);
    }
    if logical_name.contains('\0') || plan.argv().iter().any(|arg| arg.contains('\0')) {
        return Err(PlanRejection::NulByteInInvocation);
    }
    // Check the resolved path as well as the label. A plan that calls
    // `/bin/sh` "perl" is still handing a shell an inline command, and the
    // label is caller-supplied text that must not become authority.
    // A NUL in the resolved path is as unspawnable as one in the argv, and a
    // lossy view is sound for detecting it: NUL is valid UTF-8, so it survives
    // the conversion that mangles only invalid sequences.
    if let ExecutableResolution::Resolved { path, .. } = executable.resolution()
        && path.expose().to_string_lossy().contains('\0')
    {
        return Err(PlanRejection::NulByteInInvocation);
    }
    let resolved_file_name = match executable.resolution() {
        ExecutableResolution::Resolved { path, .. } => path
            .expose()
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        ExecutableResolution::Unresolved => String::new(),
    };
    for candidate in [logical_name, resolved_file_name.as_str()] {
        if is_shell_invocation(candidate, plan.argv()) {
            return Err(PlanRejection::ShellInvocationRejected { shell: candidate.to_string() });
        }
    }
    match executable.resolution() {
        ExecutableResolution::Unresolved => Err(PlanRejection::UnresolvedExecutableIdentity),
        ExecutableResolution::Resolved {
            provenance: ResolutionProvenance::AmbientLookup, ..
        } => Err(PlanRejection::AmbientExecutableResolution),
        ExecutableResolution::Resolved { path, .. } => {
            if path.is_absolute() {
                Ok(())
            } else {
                Err(PlanRejection::NonAbsoluteExecutablePath)
            }
        }
    }
}

/// The program name with any directory prefix and executable suffix removed.
fn program_base(name: &str) -> &str {
    let base = match name.rsplit(['/', '\\']).next() {
        Some(base) => base,
        None => name,
    };
    let stripped = EXECUTABLE_SUFFIXES.iter().find_map(|suffix| {
        let split = base.len().checked_sub(suffix.len())?;
        base.get(split..)
            .filter(|tail| tail.eq_ignore_ascii_case(suffix))
            .and_then(|_| base.get(..split))
    });
    match stripped {
        Some(base) => base,
        None => base,
    }
}

/// Whether a single argument hands a shell an inline command.
///
/// Three spellings, because a shell accepts all three:
///
/// - the exact flag (`-c`, `/C`, `-Command`);
/// - a same-token prefix form (`--command=...`);
/// - a **bundled short-option cluster** containing `c`. `bash -lc 'cmd'` and
///   `sh -ic 'cmd'` are ordinary idioms, and comparing the whole token against
///   `-c` misses every one of them.
fn is_inline_command_argument(arg: &str) -> bool {
    if INLINE_COMMAND_FLAGS.iter().any(|flag| flag.eq_ignore_ascii_case(arg)) {
        return true;
    }
    if INLINE_COMMAND_PREFIXES.iter().any(|prefix| {
        // `get` rather than a slice: a byte index that lands inside a
        // multi-byte character panics, and an argument is arbitrary text.
        arg.get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    }) {
        return true;
    }
    // A POSIX short-option cluster: one leading dash, then letters only. `-C`
    // is counted alongside `-c` because over-refusing a `noclobber` cluster
    // costs a caller one explicit plan, while under-refusing one hands a shell
    // a command string.
    match arg.strip_prefix('-') {
        Some(letters)
            if !letters.is_empty() && letters.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            letters.contains(['c', 'C'])
        }
        _ => false,
    }
}

/// Whether the invocation hands a shell an inline command string.
///
/// Only the argument positions where a shell is still parsing *its own*
/// options are examined. Option parsing ends at `--` or at the first operand,
/// so in `sh script.sh -c` the `-c` belongs to the script and the plan is
/// valid — scanning the whole argv would make ordinary shell tooling
/// unstartable.
///
/// The exception is a multi-call binary: `busybox sh -c 'cmd'` puts the applet
/// name in the first operand position, and stopping there would let every
/// BusyBox shell invocation through. When the operand that ends option parsing
/// is itself a shell, scanning continues past it; when it is any other applet
/// (`busybox ls -c`), the flags after it are that applet's business.
///
/// One inline form this cannot see is `sh -s`, which takes its commands from
/// standard input rather than argv. That is stdin's shape, not argv's, and it
/// is [`StdinPolicy`](super::StdinPolicy) that describes what a run feeds a
/// child.
fn is_shell_invocation(logical_name: &str, argv: &[String]) -> bool {
    let base = program_base(logical_name);
    if !SHELL_PROGRAMS.iter().any(|shell| shell.eq_ignore_ascii_case(base)) {
        return false;
    }
    let mut index = 0;
    // A multi-call binary names its applet in the first operand slot.
    if MULTI_CALL_PROGRAMS.iter().any(|name| name.eq_ignore_ascii_case(base))
        && let Some(first) = argv.first()
        && SHELL_PROGRAMS.iter().any(|shell| shell.eq_ignore_ascii_case(program_base(first)))
    {
        index = 1;
    }
    while let Some(arg) = argv.get(index) {
        // Tested before the operand rule below, because Windows shells spell
        // their flags `/C` — which is not dash-led, and would otherwise be
        // mistaken for the operand that ends option parsing.
        if is_inline_command_argument(arg) {
            return true;
        }
        if arg == "--" || !arg.starts_with('-') {
            // Options have ended; everything after belongs to the operand.
            return false;
        }
        // An option whose value is the next word consumes it. Without this the
        // value looks like the operand that ends option parsing, and every
        // flag after it — including `-c` — goes unexamined.
        index += if takes_a_separate_value(arg) { 2 } else { 1 };
    }
    false
}

/// Whether this option's value is the following argument.
///
/// Covers the exact spellings and short-option clusters ending in `o`, since
/// `bash -eo pipefail` puts the value after the cluster just as `-o` does.
fn takes_a_separate_value(arg: &str) -> bool {
    if OPTIONS_TAKING_A_SEPARATE_VALUE.iter().any(|option| option.eq_ignore_ascii_case(arg)) {
        return true;
    }
    match arg.strip_prefix(['-', '+']) {
        Some(letters)
            if !letters.is_empty() && letters.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            letters.ends_with('o')
        }
        _ => false,
    }
}

fn validate_cwd(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    match plan.cwd() {
        CwdPolicy::InheritAmbient => {
            if plan.profile().requires_exact_cwd() {
                Err(PlanRejection::AmbientCwdRejected)
            } else {
                Ok(())
            }
        }
        CwdPolicy::ExactDirectory(path) => {
            if path.expose().to_string_lossy().contains('\0') {
                Err(PlanRejection::NulByteInInvocation)
            } else if path.is_absolute() {
                Ok(())
            } else {
                Err(PlanRejection::NonAbsoluteCwd)
            }
        }
    }
}

fn validate_environment(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    let environment = plan.environment();
    if environment.projection_id().trim().is_empty() {
        return Err(PlanRejection::BlankOpaqueIdentity { field: "environment projection id" });
    }
    // A NUL anywhere in the environment makes every operating-system backend
    // refuse the spawn. Catching it here keeps the refusal in the validator,
    // where it carries a typed reason, rather than at the syscall.
    let names_with_nul = environment
        .allowed()
        .iter()
        .chain(environment.denied())
        .chain(environment.removed())
        .chain(environment.addition_names())
        .any(|name| name.as_str().contains('\0'));
    let values_with_nul = environment
        .addition_names()
        .filter_map(|name| environment.addition_value(name))
        .any(|value| value.expose().contains('\0'));
    if names_with_nul || values_with_nul {
        return Err(PlanRejection::NulByteInInvocation);
    }
    // `=` separates a name from its value in every platform's environment
    // block, so a name containing one cannot be expressed at all: the backend
    // would either refuse it or silently split it into a different variable.
    // An empty name is unrepresentable for the same reason.
    if let Some(name) = environment
        .allowed()
        .iter()
        .chain(environment.denied())
        .chain(environment.removed())
        .chain(environment.addition_names())
        .find(|name| name.as_str().is_empty() || name.as_str().contains('='))
    {
        return Err(PlanRejection::UnrepresentableEnvironmentVariableName {
            variable: name.to_string(),
        });
    }
    if let Some(variable) = environment.contradictions().first() {
        return Err(PlanRejection::ContradictoryEnvironmentRules {
            variable: variable.to_string(),
        });
    }
    let admitted_code_loading: Vec<EnvVarName> = environment.admitted_code_loading_variables();
    if plan.profile() == ExecutionProfile::HermeticProbe
        && (environment.inheritance() != AmbientInheritance::DenyAll
            || !admitted_code_loading.is_empty())
    {
        return Err(PlanRejection::HermeticProfileViolated);
    }
    if environment.code_loading() == CodeLoadingDisposition::Refused
        && let Some(variable) = admitted_code_loading.first()
    {
        return Err(PlanRejection::UnacknowledgedCodeLoadingVariable {
            variable: variable.to_string(),
        });
    }
    Ok(())
}

fn validate_budgets(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    for (channel, budget) in [
        (BudgetChannel::Stdout, plan.stdout_budget()),
        (BudgetChannel::Stderr, plan.stderr_budget()),
    ] {
        if budget.observe_limit_bytes == 0 {
            return Err(PlanRejection::ZeroCaptureBudget { channel });
        }
        if budget.observe_limit_bytes > MAX_CAPTURE_BUDGET_BYTES
            || budget.retain_limit_bytes > MAX_CAPTURE_BUDGET_BYTES
        {
            return Err(PlanRejection::CaptureBudgetOverflow { channel });
        }
        if budget.retain_limit_bytes > budget.observe_limit_bytes {
            return Err(PlanRejection::InconsistentCaptureBudget { channel });
        }
    }
    Ok(())
}

fn validate_lifecycle(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    match plan.deadline() {
        DeadlinePolicy::None if plan.profile().requires_deadline() => {
            return Err(PlanRejection::MissingDeadline);
        }
        DeadlinePolicy::Wall(duration) if duration.is_zero() => {
            return Err(PlanRejection::ZeroDeadline);
        }
        _ => {}
    }
    if plan.profile().requires_cancellation()
        && plan.cancellation() == CancellationPolicy::NotCancellable
    {
        return Err(PlanRejection::MissingCancellationPolicy);
    }
    if let TerminationPolicy::ProcessTree { graceful, then_forced } = plan.termination()
        && graceful.is_zero()
        && !then_forced
    {
        return Err(PlanRejection::ImpossibleTerminationPolicy);
    }
    if matches!(plan.stdin(), StdinPolicy::Streamed) && !plan.profile().permits_streamed_stdin() {
        return Err(PlanRejection::StreamedStdinRejected);
    }
    Ok(())
}

fn validate_subject(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    if plan.profile().requires_root_identity() && plan.subject().root.is_none() {
        return Err(PlanRejection::MissingSubjectIdentity);
    }
    for reference in plan.subject().references() {
        if reference.reference().trim().is_empty() {
            return Err(PlanRejection::BlankOpaqueIdentity { field: "subject reference" });
        }
        if reference.freshness() != EvidenceFreshness::Current {
            return Err(PlanRejection::StaleSubjectIdentity {
                reference: reference.reference().to_string(),
            });
        }
    }
    Ok(())
}

fn validate_authorization(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    let Some(authorization) = plan.authorization() else {
        return Err(PlanRejection::MissingAuthorizationEvidence);
    };
    // The reference is opaque, but the scheme it belongs to is not: evidence
    // written against a scheme this build cannot read may mean something else
    // entirely, so it is refused rather than trusted.
    if authorization.scheme_version() != SUPPORTED_AUTHORIZATION_SCHEME {
        return Err(PlanRejection::UnsupportedAuthorizationScheme {
            declared: authorization.scheme_version().get(),
            supported: SUPPORTED_AUTHORIZATION_SCHEME.get(),
        });
    }
    match authorization.freshness() {
        EvidenceFreshness::Current => {}
        EvidenceFreshness::Stale => return Err(PlanRejection::StaleAuthorizationEvidence),
        EvidenceFreshness::Unknown => return Err(PlanRejection::MissingAuthorizationEvidence),
    }
    if authorization.reference().trim().is_empty() {
        // Authority that names no decision cannot be verified by any backend.
        return Err(PlanRejection::InsufficientAuthorizationEvidence);
    }
    if authorization.strength() == AuthorizationStrength::NotProven {
        return Err(PlanRejection::InsufficientAuthorizationEvidence);
    }
    if plan.profile() == ExecutionProfile::HermeticProbe
        && authorization.strength() != AuthorizationStrength::HermeticNoAmbientInput
    {
        return Err(PlanRejection::InsufficientAuthorizationEvidence);
    }
    Ok(())
}

fn validate_platform(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    use super::identity::PlatformRequirement;
    let required_linux = plan.profile() == ExecutionProfile::LinuxOneShot;
    if required_linux && plan.claim_boundary().platform != PlatformRequirement::LinuxOnly {
        return Err(PlanRejection::UnsupportedPlatformRequirement);
    }
    Ok(())
}

/// Refuse a public projection that would republish the plan's own secrets.
///
/// This is deliberately the *only* thing checked here. A validator runs before
/// any child exists, so it cannot know what the child will write; it can only
/// see what the plan already holds. Treating a pass as proof that publishing
/// retained output is safe would read far more into this check than it does —
/// see [`PublicProjection::IncludeRetainedOutput`].
fn validate_retention(plan: &ProcessPlan) -> Result<(), PlanRejection> {
    if plan.retention().public_projection == PublicProjection::IncludeRetainedOutput
        && plan.carries_private_inputs()
    {
        return Err(PlanRejection::PublicRetentionWouldExposePrivateValues);
    }
    Ok(())
}
