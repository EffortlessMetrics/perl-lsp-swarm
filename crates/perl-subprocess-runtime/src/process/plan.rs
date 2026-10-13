//! The versioned process plan and its policies.

use std::time::Duration;

use super::PROCESS_DOMAIN_SCHEMA_VERSION;
use super::encoding::{CanonicalEncoder, PlanFingerprint};
use super::environment::EnvironmentProjection;
use super::identity::{
    AuthorizationEvidence, CwdPolicy, ExecutableIdentity, ExecutionProfile, OperationId,
    OwnerDomain, PlanId, PlatformRequirement, PrivateBytes, SchemaVersion, SubjectIdentity,
    fingerprint_of_bytes,
};

/// Encode a duration without losing precision.
///
/// Seconds and nanoseconds are encoded separately. Truncating to milliseconds
/// would give two plans with different timing behavior the same identity,
/// which breaks the injectivity the canonical encoding is for.
fn encode_duration(encoder: &mut CanonicalEncoder, duration: Duration) {
    encoder.unsigned(duration.as_secs());
    encoder.unsigned(u64::from(duration.subsec_nanos()));
}

/// The largest capture budget a plan may declare (1 GiB).
///
/// Beyond this a "bounded" claim is not credible on a developer machine.
pub const MAX_CAPTURE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// How the child's stdin is supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinPolicy {
    /// stdin is closed immediately.
    Closed,
    /// A fixed, private byte payload is written and stdin is then closed.
    ///
    /// The payload's *digest* participates in the plan's canonical identity
    /// so that two plans feeding different input are distinguishable; the
    /// bytes themselves never do. See [`PrivateBytes`] for the privacy tier
    /// this implies and when to use a [`super::SecretValue`] instead.
    Bytes(PrivateBytes),
    /// The caller drives stdin over the run's lifetime.
    ///
    /// Only interactive profiles may use this.
    Streamed,
}

impl StdinPolicy {
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("stdin");
        match self {
            Self::Closed => {
                encoder.variant(0);
                encoder.absent();
            }
            Self::Bytes(bytes) => {
                encoder.variant(1);
                encoder.unsigned(bytes.len() as u64);
                encoder.nested_fingerprint(fingerprint_of_bytes(bytes.expose()));
            }
            Self::Streamed => {
                encoder.variant(2);
                encoder.absent();
            }
        }
    }
}

/// Which of a channel's two capture bounds a limit refers to.
///
/// [`CaptureBudget`] carries two independent numbers, and a limit event that
/// named only a byte count could not say which of them it had reached — the
/// two are equal under [`CaptureBudget::bounded`], so the count does not
/// distinguish them either. A consumer that cannot tell an observation bound
/// from a retention bound cannot tell "there may be more output" from "there
/// was more output and it was not kept".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CaptureBound {
    /// The supervisor stopped reading. Output beyond it was never seen.
    Observation,
    /// The supervisor stopped keeping what it read. It kept reading.
    Retention,
}

/// What happens when a stream reaches a capture bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputLimitAction {
    /// Keep the run alive, stop retaining, and record the truncation.
    TruncateAndContinue,
    /// Terminate the run and settle as an output-limit result.
    TerminateRun,
}

impl OutputLimitAction {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::TruncateAndContinue => 0,
            Self::TerminateRun => 1,
        }
    }
}

/// The bound on one output channel.
///
/// Observation and retention are separate numbers on purpose: a supervisor
/// may see far more bytes than a receipt is allowed to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureBudget {
    /// The most bytes the supervisor will read from the channel.
    pub observe_limit_bytes: u64,
    /// The most bytes the supervisor will retain for the result.
    pub retain_limit_bytes: u64,
    /// What to do once *either* bound is reached.
    ///
    /// One action for both bounds, applied to whichever is reached first. A
    /// caller setting [`OutputLimitAction::TerminateRun`] is saying "stop if
    /// this channel reaches a budget", and a budget that retains less than it
    /// observes is a budget the run can reach by retention alone.
    ///
    /// Documenting this as governing only the observation bound left the
    /// retention bound with no policy at all, while
    /// [`ProcessResult::new`](super::ProcessResult::new) accepted a
    /// retention-only truncation as evidence for an output-limit outcome — a
    /// result shape no stated policy could produce. Naming the bound in
    /// [`LimitEvidence`](super::LimitEvidence) and giving both bounds the same
    /// action closes that from both ends.
    ///
    /// The two actions differ in what they stop.
    /// [`OutputLimitAction::TruncateAndContinue`] at the retention bound stops
    /// retention and keeps reading, so the observed count stays truthful; at
    /// the observation bound it stops reading, and output past it was never
    /// seen by anyone.
    pub on_limit: OutputLimitAction,
}

impl CaptureBudget {
    /// A budget that observes and retains the same bound and truncates.
    pub fn bounded(limit_bytes: u64) -> Self {
        Self {
            observe_limit_bytes: limit_bytes,
            retain_limit_bytes: limit_bytes,
            on_limit: OutputLimitAction::TruncateAndContinue,
        }
    }

    /// A budget that observes a bound but retains nothing.
    pub fn observe_only(limit_bytes: u64) -> Self {
        Self {
            observe_limit_bytes: limit_bytes,
            retain_limit_bytes: 0,
            on_limit: OutputLimitAction::TruncateAndContinue,
        }
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder, label: &str) {
        encoder.section(label);
        encoder.unsigned(self.observe_limit_bytes);
        encoder.unsigned(self.retain_limit_bytes);
        encoder.variant(self.on_limit.discriminant());
    }
}

/// The wall-clock bound on a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeadlinePolicy {
    /// No deadline. Only profiles that do not require one may use this.
    None,
    /// A wall-clock deadline measured from the start attempt.
    Wall(Duration),
}

impl DeadlinePolicy {
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("deadline");
        match self {
            Self::None => {
                encoder.variant(0);
                encoder.absent();
            }
            Self::Wall(duration) => {
                encoder.variant(1);
                encode_duration(encoder, *duration);
            }
        }
    }
}

/// Whether and how a run can be cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationPolicy {
    /// The run cannot be cancelled once started.
    NotCancellable,
    /// The run can be cancelled, with a grace period before escalation.
    Cooperative {
        /// How long the child is given to exit after a cancellation request.
        grace: Duration,
    },
}

impl CancellationPolicy {
    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("cancellation");
        match self {
            Self::NotCancellable => {
                encoder.variant(0);
                encoder.absent();
            }
            Self::Cooperative { grace } => {
                encoder.variant(1);
                encode_duration(encoder, *grace);
            }
        }
    }
}

/// What the supervisor undertakes to terminate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminationPolicy {
    /// Only the direct child is signalled.
    ///
    /// Legitimate, but it is **not** process-tree cleanup: descendants may
    /// survive. A result produced under this policy says so.
    ImmediateChildOnly,
    /// The owned process group is terminated gracefully, then forcibly.
    ProcessTree {
        /// How long the group is given to exit after the graceful signal.
        graceful: Duration,
        /// Whether a forced kill follows the grace period.
        then_forced: bool,
    },
}

impl TerminationPolicy {
    /// Whether this policy can support a process-tree cleanup claim.
    pub fn claims_tree_cleanup(self) -> bool {
        matches!(self, Self::ProcessTree { .. })
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("termination");
        match self {
            Self::ImmediateChildOnly => {
                encoder.variant(0);
                encoder.absent();
                encoder.absent();
            }
            Self::ProcessTree { graceful, then_forced } => {
                encoder.variant(1);
                encode_duration(encoder, *graceful);
                encoder.flag(*then_forced);
            }
        }
    }
}

/// How much of a run's evidence may cross the public boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PublicProjection {
    /// Only redacted identities, counts, and dispositions.
    RedactedIdentitiesOnly,
    /// Retained output bytes may also be published.
    ///
    /// # What choosing this does and does not establish
    ///
    /// This is the owner's assertion about content the domain never sees. A
    /// plan is validated before anything runs, so nothing here can know what a
    /// child will write — output may carry a token it read from a file, an
    /// absolute path, or a message quoting its own environment.
    ///
    /// Validation therefore refuses only what it can actually see: publishing
    /// retained output while the *plan itself* holds private values, which
    /// would republish the caller's own secrets. Passing that check means the
    /// plan's inputs are clean, **not** that the output will be. Whoever
    /// publishes the projection owns reviewing or redacting what the child
    /// actually produced.
    IncludeRetainedOutput,
}

impl PublicProjection {
    pub(crate) fn discriminant(self) -> u16 {
        match self {
            Self::RedactedIdentitiesOnly => 0,
            Self::IncludeRetainedOutput => 1,
        }
    }
}

/// What a run keeps and what it may publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// Whether retained stdout bytes are kept in the result.
    pub retain_stdout: bool,
    /// Whether retained stderr bytes are kept in the result.
    pub retain_stderr: bool,
    /// What the public projection of the result may contain.
    pub public_projection: PublicProjection,
}

impl RetentionPolicy {
    /// Keep both channels but publish only redacted identities.
    pub fn private() -> Self {
        Self {
            retain_stdout: true,
            retain_stderr: true,
            public_projection: PublicProjection::RedactedIdentitiesOnly,
        }
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("retention");
        encoder.flag(self.retain_stdout);
        encoder.flag(self.retain_stderr);
        encoder.variant(self.public_projection.discriminant());
    }
}

/// What a plan's execution does *not* establish.
///
/// Recorded on the plan so that a result can carry the same non-claims
/// without a consumer having to remember them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimBoundary {
    /// The platform the plan requires.
    pub platform: PlatformRequirement,
}

impl ClaimBoundary {
    /// A Linux-only claim boundary.
    pub fn linux_only() -> Self {
        Self { platform: PlatformRequirement::LinuxOnly }
    }

    /// A platform-neutral claim boundary.
    pub fn any_platform() -> Self {
        Self { platform: PlatformRequirement::AnyPlatform }
    }

    pub(crate) fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("claim_boundary");
        encoder.variant(self.platform.discriminant());
    }
}

/// A declarative, versioned description of one process execution.
///
/// A plan is inert. It becomes startable only by passing
/// [`ProcessPlan::validate`], which is the sole constructor of
/// [`super::ValidatedProcessPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessPlan {
    schema_version: SchemaVersion,
    plan_id: PlanId,
    operation: OperationId,
    owner: OwnerDomain,
    profile: ExecutionProfile,
    executable: ExecutableIdentity,
    argv: Vec<String>,
    cwd: CwdPolicy,
    environment: EnvironmentProjection,
    stdin: StdinPolicy,
    stdout_budget: CaptureBudget,
    stderr_budget: CaptureBudget,
    deadline: DeadlinePolicy,
    cancellation: CancellationPolicy,
    termination: TerminationPolicy,
    retention: RetentionPolicy,
    subject: SubjectIdentity,
    authorization: Option<AuthorizationEvidence>,
    claim_boundary: ClaimBoundary,
}

impl ProcessPlan {
    /// Start building a plan.
    pub fn builder(
        plan_id: PlanId,
        operation: OperationId,
        owner: OwnerDomain,
        profile: ExecutionProfile,
        executable: ExecutableIdentity,
        environment: EnvironmentProjection,
    ) -> ProcessPlanBuilder {
        ProcessPlanBuilder {
            plan: Self {
                schema_version: PROCESS_DOMAIN_SCHEMA_VERSION,
                plan_id,
                operation,
                owner,
                profile,
                executable,
                argv: Vec::new(),
                cwd: CwdPolicy::InheritAmbient,
                environment,
                stdin: StdinPolicy::Closed,
                stdout_budget: CaptureBudget::bounded(1024 * 1024),
                stderr_budget: CaptureBudget::bounded(1024 * 1024),
                deadline: DeadlinePolicy::None,
                cancellation: CancellationPolicy::NotCancellable,
                termination: TerminationPolicy::ImmediateChildOnly,
                retention: RetentionPolicy::private(),
                subject: SubjectIdentity::default(),
                authorization: None,
                claim_boundary: ClaimBoundary::any_platform(),
            },
        }
    }

    /// The domain schema version this plan was built against.
    pub fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// The plan's correlation identity.
    pub fn plan_id(&self) -> &PlanId {
        &self.plan_id
    }

    /// The operation the plan serves.
    pub fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// The domain that owns the operation's meaning.
    pub fn owner(&self) -> OwnerDomain {
        self.owner
    }

    /// The execution shape the plan requires.
    pub fn profile(&self) -> ExecutionProfile {
        self.profile
    }

    /// The program to execute.
    pub fn executable(&self) -> &ExecutableIdentity {
        &self.executable
    }

    /// The structured arguments, excluding the program name.
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The working-directory policy.
    pub fn cwd(&self) -> &CwdPolicy {
        &self.cwd
    }

    /// The environment projection.
    pub fn environment(&self) -> &EnvironmentProjection {
        &self.environment
    }

    /// The stdin policy.
    pub fn stdin(&self) -> &StdinPolicy {
        &self.stdin
    }

    /// The stdout capture budget.
    pub fn stdout_budget(&self) -> CaptureBudget {
        self.stdout_budget
    }

    /// The stderr capture budget.
    pub fn stderr_budget(&self) -> CaptureBudget {
        self.stderr_budget
    }

    /// The deadline policy.
    pub fn deadline(&self) -> DeadlinePolicy {
        self.deadline
    }

    /// The cancellation policy.
    pub fn cancellation(&self) -> CancellationPolicy {
        self.cancellation
    }

    /// The termination policy.
    pub fn termination(&self) -> TerminationPolicy {
        self.termination
    }

    /// The retention policy.
    pub fn retention(&self) -> RetentionPolicy {
        self.retention
    }

    /// The subject the plan executes against.
    pub fn subject(&self) -> &SubjectIdentity {
        &self.subject
    }

    /// The authorization evidence, if any was supplied.
    pub fn authorization(&self) -> Option<&AuthorizationEvidence> {
        self.authorization.as_ref()
    }

    /// The plan's claim boundary.
    pub fn claim_boundary(&self) -> ClaimBoundary {
        self.claim_boundary
    }

    /// Whether the plan carries values that must not be published.
    pub fn carries_private_inputs(&self) -> bool {
        self.environment.carries_private_values()
            || matches!(&self.stdin, StdinPolicy::Bytes(bytes) if !bytes.is_empty())
    }

    /// The plan's canonical, secret-safe byte encoding.
    ///
    /// Deterministic under construction order: sets and maps are ordered, and
    /// every field is tagged and length-prefixed.
    ///
    /// # What is and is not bounded
    ///
    /// The *fingerprint* is bounded: 128 bits for any plan, however large.
    /// These *bytes* are not — they are linear in the plan's own size, since
    /// every identifier, argument, and variable name is written once. The
    /// encoding performs no amplification, so it cannot turn a small plan into
    /// a large buffer, but a caller that builds a plan with a megabyte of argv
    /// gets a megabyte of encoding.
    ///
    /// This domain deliberately caps neither. The limits that actually bite —
    /// `ARG_MAX`, the environment block size — are the platform's, they differ
    /// per target, and they are enforced at spawn by the backend that knows
    /// which platform it is on. Inventing a number here would be policy this
    /// domain does not own, and would reject plans a real system accepts.
    /// Whoever accepts *untrusted* plan input is the right place to bound the
    /// input, and that is not this type.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new();
        self.encode(&mut encoder);
        encoder.finish()
    }

    /// The plan's public semantic fingerprint.
    ///
    /// Fixed size for any plan: see [`Self::canonical_bytes`] for what that
    /// does and does not bound.
    ///
    /// # This is not an execution-input key
    ///
    /// Environment variable **values** are excluded from the encoding — not
    /// even as a nested fingerprint, because a fingerprint of a low-entropy
    /// secret is a guessable secret. Two plans that differ only in an
    /// addition's value therefore share this identity.
    ///
    /// So a consumer must not cache a result under it, and must not read a
    /// fingerprint match as "same inputs, reuse the outcome": doing either
    /// would serve one secret's result for a run configured with another. This
    /// answers "the same plan *shape*", and nothing more. A consumer that
    /// needs to key on values owns that keying itself, along with the handling
    /// the secrets in it require.
    ///
    /// [`StdinPolicy::Bytes`] is the contrasting case and shows the rule is
    /// about privacy tier rather than about omission: its content *is*
    /// fingerprinted, so two plans feeding different input stay distinguishable.
    pub fn semantic_fingerprint(&self) -> PlanFingerprint {
        let mut encoder = CanonicalEncoder::new();
        self.encode(&mut encoder);
        PlanFingerprint::new(encoder.finish_fingerprint())
    }

    fn encode(&self, encoder: &mut CanonicalEncoder) {
        encoder.section("process_plan");
        encoder.unsigned(u64::from(self.schema_version.get()));
        encoder.text(self.plan_id.as_str());
        encoder.text(self.operation.as_str());
        encoder.variant(self.owner.discriminant());
        encoder.variant(self.profile.discriminant());
        self.executable.encode(encoder);
        encoder.section("argv");
        encoder.unsigned(self.argv.len() as u64);
        for argument in &self.argv {
            encoder.text(argument);
        }
        self.cwd.encode(encoder);
        self.environment.encode(encoder);
        self.stdin.encode(encoder);
        self.stdout_budget.encode(encoder, "stdout_budget");
        self.stderr_budget.encode(encoder, "stderr_budget");
        self.deadline.encode(encoder);
        self.cancellation.encode(encoder);
        self.termination.encode(encoder);
        self.retention.encode(encoder);
        self.subject.encode(encoder);
        match &self.authorization {
            None => {
                encoder.absent();
            }
            Some(authorization) => authorization.encode(encoder),
        }
        self.claim_boundary.encode(encoder);
    }
}

/// Builder for [`ProcessPlan`].
///
/// Defaults are the conservative ones: no arguments, ambient cwd, closed
/// stdin, bounded 1 MiB capture on each channel, no deadline, not
/// cancellable, immediate-child termination, and private retention. Anything
/// a profile requires beyond that must be set explicitly, and the validator
/// refuses the plan otherwise.
#[derive(Debug, Clone)]
pub struct ProcessPlanBuilder {
    plan: ProcessPlan,
}

impl ProcessPlanBuilder {
    /// Set the structured arguments.
    #[must_use]
    pub fn argv<I, S>(mut self, argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.plan.argv = argv.into_iter().map(Into::into).collect();
        self
    }

    /// Set the working-directory policy.
    #[must_use]
    pub fn cwd(mut self, cwd: CwdPolicy) -> Self {
        self.plan.cwd = cwd;
        self
    }

    /// Set the stdin policy.
    #[must_use]
    pub fn stdin(mut self, stdin: StdinPolicy) -> Self {
        self.plan.stdin = stdin;
        self
    }

    /// Set the stdout capture budget.
    #[must_use]
    pub fn stdout_budget(mut self, budget: CaptureBudget) -> Self {
        self.plan.stdout_budget = budget;
        self
    }

    /// Set the stderr capture budget.
    #[must_use]
    pub fn stderr_budget(mut self, budget: CaptureBudget) -> Self {
        self.plan.stderr_budget = budget;
        self
    }

    /// Set the deadline policy.
    #[must_use]
    pub fn deadline(mut self, deadline: DeadlinePolicy) -> Self {
        self.plan.deadline = deadline;
        self
    }

    /// Set the cancellation policy.
    #[must_use]
    pub fn cancellation(mut self, cancellation: CancellationPolicy) -> Self {
        self.plan.cancellation = cancellation;
        self
    }

    /// Set the termination policy.
    #[must_use]
    pub fn termination(mut self, termination: TerminationPolicy) -> Self {
        self.plan.termination = termination;
        self
    }

    /// Set the retention policy.
    #[must_use]
    pub fn retention(mut self, retention: RetentionPolicy) -> Self {
        self.plan.retention = retention;
        self
    }

    /// Set the subject identity.
    #[must_use]
    pub fn subject(mut self, subject: SubjectIdentity) -> Self {
        self.plan.subject = subject;
        self
    }

    /// Attach authorization evidence.
    #[must_use]
    pub fn authorization(mut self, authorization: AuthorizationEvidence) -> Self {
        self.plan.authorization = Some(authorization);
        self
    }

    /// Set the claim boundary.
    #[must_use]
    pub fn claim_boundary(mut self, claim_boundary: ClaimBoundary) -> Self {
        self.plan.claim_boundary = claim_boundary;
        self
    }

    /// Override the schema version.
    ///
    /// Exists so that a plan built against a future or retired schema can be
    /// constructed and then refused by the validator, rather than being
    /// impossible to express and therefore untestable.
    #[must_use]
    pub fn schema_version(mut self, schema_version: SchemaVersion) -> Self {
        self.plan.schema_version = schema_version;
        self
    }

    /// Finish the plan. The result is inert until validated.
    pub fn build(self) -> ProcessPlan {
        self.plan
    }
}
