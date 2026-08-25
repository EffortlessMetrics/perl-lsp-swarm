//! Row-carried requirement records: completeness, limitations, legacy exit,
//! claim ceilings, invalidation inputs, ownership, evidence, and provenance.
//!
//! Every field here is mandatory on a [`super::rows::CompilerProfileRow`]:
//! constructors take them positionally so absence is unrepresentable, and
//! validators re-check their content.

use std::collections::BTreeSet;

use super::CompilerProfileError;
use super::dimensions::EvidenceClass;
use super::dimensions::ExecutionStage;
use super::dimensions::ProofAxis;
use super::dimensions::SourceTier;
use super::dimensions::WorkPerformed;
use super::dimensions::encode_set;
use super::fingerprint::CanonWriter;
use super::fingerprint::CanonicalEncode;
use super::identity::CompilerProfileRowId;

/// How current evidence must be at claim time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum CurrentnessRule {
    /// Evidence must be fresh when the profile is validated.
    FreshAtValidationTime,
    /// Evidence must stay fresh inside a declared validity window.
    FreshWithinDeclaredWindow,
    /// Evidence is pinned to an exact content digest.
    PinnedToContentDigest,
}

impl CanonicalEncode for CurrentnessRule {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::FreshAtValidationTime => "cur_fresh_at_validation_time",
            Self::FreshWithinDeclaredWindow => "cur_fresh_within_declared_window",
            Self::PinnedToContentDigest => "cur_pinned_to_content_digest",
        };
        writer.tag(tag);
    }
}

/// How completely a row's subject selector must be covered.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum CompletenessRule {
    /// Every subject matching the selector is covered.
    ExhaustiveAcrossSubjectSelector,
    /// A representative sample is covered.
    RepresentativeSample,
    /// A single named point is covered.
    SinglePointCheck,
}

impl CanonicalEncode for CompletenessRule {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::ExhaustiveAcrossSubjectSelector => "compl_exhaustive",
            Self::RepresentativeSample => "compl_representative_sample",
            Self::SinglePointCheck => "compl_single_point_check",
        };
        writer.tag(tag);
    }
}

/// The paired currentness/completeness rule every row carries.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompletenessRequirement {
    /// Currentness half of the rule.
    pub currentness: CurrentnessRule,
    /// Completeness half of the rule.
    pub completeness: CompletenessRule,
}

impl CanonicalEncode for CompletenessRequirement {
    fn encode(&self, writer: &mut CanonWriter) {
        self.currentness.encode(writer);
        self.completeness.encode(writer);
    }
}

/// One explicitly allowed limitation attached to a row or profile.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct AllowedLimitation {
    /// Stable limitation token.
    pub limitation_id: String,
    /// What exactly is limited.
    pub description: String,
}

impl AllowedLimitation {
    /// Validated constructor; ids are stable tokens and descriptions non-empty.
    pub fn new(limitation_id: &str, description: &str) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(limitation_id) {
            return Err(CompilerProfileError::Structure {
                message: format!("limitation id {limitation_id:?} must match [a-z0-9._-]"),
            });
        }
        if description.trim().is_empty() {
            return Err(CompilerProfileError::Structure {
                message: "limitation description must not be empty".to_string(),
            });
        }
        Ok(Self { limitation_id: limitation_id.to_string(), description: description.to_string() })
    }
}

impl CanonicalEncode for AllowedLimitation {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("lim_id", &self.limitation_id);
        writer.str_field("lim_desc", &self.description);
    }
}

/// Independent dimension of leaving a legacy path behind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum LegacyExitDimension {
    /// The replacement path must be current when exit is claimed.
    ReplacementCurrentness,
    /// The old path must be demonstrably absent after exit.
    OldPathAbsence,
    /// Recurrence of the old behavior must be guarded against.
    RecurrenceGuard,
}

impl CanonicalEncode for LegacyExitDimension {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::ReplacementCurrentness => "exit_replacement_currentness",
            Self::OldPathAbsence => "exit_old_path_absence",
            Self::RecurrenceGuard => "exit_recurrence_guard",
        };
        writer.tag(tag);
    }
}

/// The mandatory legacy-exit requirement of a row. `NotApplicable` is an
/// explicit typed state, never an omitted field.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum LegacyExitRequirement {
    /// No legacy-exit obligation applies to this row.
    NotApplicable,
    /// The listed dimensions must be proven before exit is claimed.
    Required(BTreeSet<LegacyExitDimension>),
}

impl LegacyExitRequirement {
    /// Explicit no-legacy obligation.
    pub const fn none() -> Self {
        Self::NotApplicable
    }

    /// Require an explicit, non-empty set of exit dimensions.
    pub fn required(
        dimensions: BTreeSet<LegacyExitDimension>,
    ) -> Result<Self, CompilerProfileError> {
        if dimensions.is_empty() {
            return Err(CompilerProfileError::Structure {
                message: "legacy exit requirement with dimensions must list at least one \
                          dimension"
                    .to_string(),
            });
        }
        Ok(Self::Required(dimensions))
    }

    /// Whether any exit dimension is demanded.
    pub const fn demands_exit(&self) -> bool {
        matches!(self, Self::Required(_))
    }
}

impl CanonicalEncode for LegacyExitRequirement {
    fn encode(&self, writer: &mut CanonWriter) {
        match self {
            Self::NotApplicable => writer.tag("exit_not_applicable"),
            Self::Required(dimensions) => {
                writer.tag("exit_required");
                encode_set(dimensions, writer);
            }
        }
    }
}

/// The upper bound of what a satisfied row licenses downstream consumers to
/// say. This ceiling is descriptive data only: nothing in this module derives
/// support, release, or publication authority from a validated profile.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ClaimCeiling {
    /// Claims stay inside repository tooling and diagnostics.
    RepositoryInternalOnly,
    /// Claims may be stated in contributor-facing documentation.
    ContributorDocumentation,
    /// Claims may back documented product behavior.
    DocumentedProductBehavior,
    /// Claims may be quoted in public support statements.
    PublicSupportStatement,
}

impl CanonicalEncode for ClaimCeiling {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::RepositoryInternalOnly => "ceil_repository_internal_only",
            Self::ContributorDocumentation => "ceil_contributor_documentation",
            Self::DocumentedProductBehavior => "ceil_documented_product_behavior",
            Self::PublicSupportStatement => "ceil_public_support_statement",
        };
        writer.tag(tag);
    }
}

/// An input whose change invalidates previously established rows.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum InvalidationInput {
    /// Nothing invalidates this row besides revalidation itself.
    NoneDeclared,
    /// An upstream release invalidates the row.
    UpstreamRelease,
    /// Cutting a product release invalidates the row.
    ProductReleaseCut,
    /// Workspace configuration changes invalidate the row.
    WorkspaceConfigurationChange,
    /// Toolchain content changes invalidate the row.
    ToolchainChange,
    /// Only an explicit review request invalidates the row.
    ExplicitReviewRequest,
}

impl CanonicalEncode for InvalidationInput {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::NoneDeclared => "inval_none_declared",
            Self::UpstreamRelease => "inval_upstream_release",
            Self::ProductReleaseCut => "inval_product_release_cut",
            Self::WorkspaceConfigurationChange => "inval_workspace_configuration_change",
            Self::ToolchainChange => "inval_toolchain_change",
            Self::ExplicitReviewRequest => "inval_explicit_review_request",
        };
        writer.tag(tag);
    }
}

/// The event that wakes the owner to revalidate a row.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum WakeEvent {
    /// No wake event is scheduled; explicit review only.
    NoScheduledWake,
    /// The next product release wakes the owner.
    NextProductRelease,
    /// The next upstream release wakes the owner.
    NextUpstreamRelease,
    /// Closing a dependent issue wakes the owner.
    DependentIssueClosed,
    /// A scheduled review date wakes the owner.
    ScheduledReview,
}

impl CanonicalEncode for WakeEvent {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::NoScheduledWake => "wake_none_scheduled",
            Self::NextProductRelease => "wake_next_product_release",
            Self::NextUpstreamRelease => "wake_next_upstream_release",
            Self::DependentIssueClosed => "wake_dependent_issue_closed",
            Self::ScheduledReview => "wake_scheduled_review",
        };
        writer.tag(tag);
    }
}

/// Mandatory ownership record of a row: an owning issue plus a wake event
/// that triggers revalidation.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct OwnerAndWakeEvent {
    /// Owning issue reference (`#12186` style).
    pub owner_issue: String,
    /// Event that wakes the owner for revalidation.
    pub wake_event: WakeEvent,
}

impl OwnerAndWakeEvent {
    /// Validated constructor; the owner issue must be a `#<digits>` reference.
    pub fn new(owner_issue: &str, wake_event: WakeEvent) -> Result<Self, CompilerProfileError> {
        let valid_reference =
            matches!(owner_issue.strip_prefix('#'), Some(digits) if is_all_digits(digits));
        if !valid_reference {
            return Err(CompilerProfileError::Identity {
                message: format!("owner issue {owner_issue:?} must look like '#12186'"),
            });
        }
        Ok(Self { owner_issue: owner_issue.to_string(), wake_event })
    }
}

fn is_all_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

impl CanonicalEncode for OwnerAndWakeEvent {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("owner", &self.owner_issue);
        self.wake_event.encode(writer);
    }
}

/// Collaboration surfaces whose live state must never enter the evidence
/// model. Values of this type cannot become evidence: the evidence
/// constructor refuses them outright.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum CollaborationSurface {
    /// A GitHub issue's open/closed/label state.
    Issue,
    /// A pull request's reviewable state.
    PullRequest,
    /// A workflow run's status.
    WorkflowRun,
}

impl CanonicalEncode for CollaborationSurface {
    fn encode(&self, writer: &mut CanonWriter) {
        let tag = match self {
            Self::Issue => "surf_issue",
            Self::PullRequest => "surf_pull_request",
            Self::WorkflowRun => "surf_workflow_run",
        };
        writer.tag(tag);
    }
}

/// Typed provenance offered to the evidence constructor. Only
/// [`ExternalProvenance::ProfileDomainArtifacts`] is representable as
/// evidence; collaboration state is rejected input.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ExternalProvenance {
    /// Artifacts owned by the profile domain itself.
    ProfileDomainArtifacts {
        /// Reference to the artifact (path, corpus id, or similar token).
        reference: String,
    },
    /// Live issue/PR/workflow state — always rejected as evidence input.
    CollaborationSurfaceState {
        /// Which collaboration surface the state came from.
        surface: CollaborationSurface,
        /// Surface identifier.
        identifier: String,
    },
}

impl CanonicalEncode for ExternalProvenance {
    fn encode(&self, writer: &mut CanonWriter) {
        match self {
            Self::ProfileDomainArtifacts { reference } => {
                writer.tag("prov_profile_domain");
                writer.str_field("ref", reference);
            }
            Self::CollaborationSurfaceState { surface, identifier } => {
                writer.tag("prov_collaboration_state");
                surface.encode(writer);
                writer.str_field("ident", identifier);
            }
        }
    }
}

/// One observed evidence record binding an exact `(row, axis)` pair to a
/// concrete evidence class, tier, stage, work performed, and domain-owned
/// provenance.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct EvidenceRecord {
    /// Stable record identity.
    pub record_id: String,
    /// The exact row this evidence belongs to.
    pub row_id: CompilerProfileRowId,
    /// The exact axis this evidence backs; it never satisfies any other axis.
    pub axis: ProofAxis,
    /// The class of evidence offered.
    pub class: EvidenceClass,
    /// Provenance tier of the evidence source.
    pub tier: SourceTier,
    /// Stage at which the proposition was observed.
    pub stage_observed: ExecutionStage,
    /// Work actually performed to produce this record.
    pub work: WorkPerformed,
    /// Domain-owned provenance (collaboration state cannot appear here).
    pub provenance: ExternalProvenance,
}

impl EvidenceRecord {
    /// Constructor refusing collaboration-surface provenance. Issue, PR, and
    /// workflow state can be *offered* but never accepted into the model.
    #[allow(clippy::too_many_arguments)]
    pub fn finish(
        record_id: &str,
        row_id: CompilerProfileRowId,
        axis: ProofAxis,
        class: EvidenceClass,
        tier: SourceTier,
        stage_observed: ExecutionStage,
        work: WorkPerformed,
        provenance: ExternalProvenance,
    ) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(record_id) {
            return Err(CompilerProfileError::Structure {
                message: format!("evidence record id {record_id:?} must match [a-z0-9._-]"),
            });
        }
        if matches!(provenance, ExternalProvenance::CollaborationSurfaceState { .. }) {
            return Err(CompilerProfileError::RejectedProvenance {
                detail: "issue/PR/workflow state cannot enter the evidence model; only \
                         profile-domain artifacts are accepted"
                    .to_string(),
            });
        }
        Ok(Self {
            record_id: record_id.to_string(),
            row_id,
            axis,
            class,
            tier,
            stage_observed,
            work,
            provenance,
        })
    }

    /// Convenience constructor using profile-domain artifact provenance.
    #[allow(clippy::too_many_arguments)]
    pub fn from_domain_artifact(
        record_id: &str,
        row_id: CompilerProfileRowId,
        axis: ProofAxis,
        class: EvidenceClass,
        tier: SourceTier,
        stage_observed: ExecutionStage,
        work: WorkPerformed,
        reference: &str,
    ) -> Result<Self, CompilerProfileError> {
        Self::finish(
            record_id,
            row_id,
            axis,
            class,
            tier,
            stage_observed,
            work,
            ExternalProvenance::ProfileDomainArtifacts { reference: reference.to_string() },
        )
    }
}

impl CanonicalEncode for EvidenceRecord {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("evid", &self.record_id);
        self.row_id.encode(writer);
        self.axis.encode(writer);
        self.class.encode(writer);
        self.tier.encode(writer);
        self.stage_observed.encode(writer);
        self.work.encode(writer);
        self.provenance.encode(writer);
    }
}
