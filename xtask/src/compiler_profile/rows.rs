//! Profile rows and their closed disposition states.
//!
//! Rows are never omitted to express weakness: [`RowDisposition`] is an
//! exhaustive closed enum and every row carries its disposition plus its full
//! obligation payload. A row may declare several proof axes
//! ([`CompilerProfileRow::axis_specs`]); each axis demands its own evidence
//! and no axis ever satisfies another.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use super::CompilerProfileError;
use super::dimensions::ClaimFamily;
use super::dimensions::EvidenceClass;
use super::dimensions::ExecutionStage;
use super::dimensions::ProofAxis;
use super::dimensions::SemanticSupportLevel;
use super::dimensions::SourceTier;
use super::dimensions::SubjectSelector;
use super::dimensions::SupportClaim;
use super::dimensions::WorkPerformed;
use super::dimensions::WorkRequirement;
use super::dimensions::class_supports_family;
use super::dimensions::encode_set;
use super::fingerprint::CanonWriter;
use super::fingerprint::CanonicalEncode;
use super::identity::CompilerProfileRowId;
use super::requirements::AllowedLimitation;
use super::requirements::ClaimCeiling;
use super::requirements::CompletenessRequirement;
use super::requirements::InvalidationInput;
use super::requirements::LegacyExitRequirement;
use super::requirements::OwnerAndWakeEvent;

/// Typed activation condition for a conditional row.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum ConditionalActivation {
    /// Active while a workspace feature is enabled.
    WhenWorkspaceFeatureEnabled(
        /// Stable feature token.
        String,
    ),
    /// Active once an upstream release has been adopted.
    WhenUpstreamAdopted(
        /// Stable upstream reference token.
        String,
    ),
    /// Active only after an explicit reviewed activation decision.
    WhenExplicitlyActivated,
}

impl CanonicalEncode for ConditionalActivation {
    fn encode(&self, writer: &mut CanonWriter) {
        match self {
            Self::WhenWorkspaceFeatureEnabled(feature) => {
                writer.tag("cond_feature_enabled");
                writer.str_field("feat", feature);
            }
            Self::WhenUpstreamAdopted(release) => {
                writer.tag("cond_upstream_adopted");
                writer.str_field("rel", release);
            }
            Self::WhenExplicitlyActivated => writer.tag("cond_explicitly_activated"),
        }
    }
}

/// The closed set of row dispositions. Weakness is expressed inside this enum,
/// never by dropping rows.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub enum RowDisposition {
    /// Conjunctive obligation: the profile is valid only when this row holds.
    Required,
    /// Obligation under an explicit typed activation condition.
    Conditional(
        /// What activates the obligation.
        ConditionalActivation,
    ),
    /// Carried informationally; never conjunctive.
    Optional,
    /// Typed unsupported state with its reason.
    Unsupported {
        /// Why the subject is unsupported here.
        reason: String,
    },
    /// Typed not-applicable state with its reason.
    NotApplicable {
        /// Why the subject does not apply here.
        reason: String,
    },
}

impl RowDisposition {
    /// Whether this disposition demands evidence during validation.
    pub const fn requires_evidence(&self) -> bool {
        matches!(self, Self::Required)
    }

    /// Whether proof axes may be declared under this disposition.
    /// Unsupported and not-applicable states carry no obligations.
    pub const fn permits_axes(&self) -> bool {
        !matches!(self, Self::Unsupported { .. } | Self::NotApplicable { .. })
    }

    /// Validated constructors for reason-bearing closed states.
    pub fn unsupported(reason: &str) -> Result<Self, CompilerProfileError> {
        Self::non_empty_reason("unsupported", reason)?;
        Ok(Self::Unsupported { reason: reason.to_string() })
    }

    /// Validated constructor for the not-applicable state.
    pub fn not_applicable(reason: &str) -> Result<Self, CompilerProfileError> {
        Self::non_empty_reason("not-applicable", reason)?;
        Ok(Self::NotApplicable { reason: reason.to_string() })
    }

    fn non_empty_reason(kind: &str, reason: &str) -> Result<(), CompilerProfileError> {
        if reason.trim().is_empty() {
            return Err(CompilerProfileError::Structure {
                message: format!("{kind} disposition requires a non-empty reason"),
            });
        }
        Ok(())
    }
}

impl CanonicalEncode for RowDisposition {
    fn encode(&self, writer: &mut CanonWriter) {
        match self {
            Self::Required => writer.tag("disp_required"),
            Self::Conditional(activation) => {
                writer.tag("disp_conditional");
                activation.encode(writer);
            }
            Self::Optional => writer.tag("disp_optional"),
            Self::Unsupported { reason } => {
                writer.tag("disp_unsupported");
                writer.str_field("why", reason);
            }
            Self::NotApplicable { reason } => {
                writer.tag("disp_not_applicable");
                writer.str_field("why", reason);
            }
        }
    }
}

/// The evidence contract of one proof axis: which evidence classes may back
/// it, the provenance-tier floor, the observation-stage floor, and the work
/// requirement. Constructors reject classes that cannot support the axis
/// family, so cross-family acceptance sets are unrepresentable here.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct AxisProofSpec {
    /// Evidence classes that may satisfy this axis.
    pub acceptable_classes: BTreeSet<EvidenceClass>,
    /// Minimum accepted provenance tier.
    pub min_tier: SourceTier,
    /// Minimum observation stage.
    pub min_stage: ExecutionStage,
    /// Work that must have been performed.
    pub work: WorkRequirement,
}

/// Why one evidence record cannot back the axis it was offered to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AxisRejection {
    /// The evidence class is outside the axis accept list.
    ClassNotAccepted,
    /// The provenance tier sits below the axis floor.
    TierBelowFloor,
    /// The observation stage sits below the axis floor.
    StageBelowFloor,
    /// The performed work ran in the wrong context.
    WorkContextMismatch,
    /// The performed work misses the requirement.
    WorkBelowMinimum,
}

impl AxisProofSpec {
    /// Validated constructor bound to one exact axis.
    pub fn for_axis(
        axis: ProofAxis,
        acceptable_classes: BTreeSet<EvidenceClass>,
        min_tier: SourceTier,
        min_stage: ExecutionStage,
        work: WorkRequirement,
    ) -> Result<Self, CompilerProfileError> {
        if acceptable_classes.is_empty() {
            return Err(CompilerProfileError::Structure {
                message: "an axis proof spec must accept at least one evidence class".to_string(),
            });
        }
        for class in &acceptable_classes {
            if !class_supports_family(*class, axis.family) {
                return Err(CompilerProfileError::CrossSatisfaction {
                    row: String::new(),
                    detail: format!(
                        "evidence class {class:?} can never support the {:?} axis family",
                        axis.family
                    ),
                });
            }
        }
        Ok(Self { acceptable_classes, min_tier, min_stage, work })
    }

    /// Whether one observed record satisfies this spec's floors.
    pub(crate) fn accept_record(
        &self,
        class: EvidenceClass,
        tier: SourceTier,
        stage: ExecutionStage,
        performed: WorkPerformed,
    ) -> Result<(), AxisRejection> {
        if !self.acceptable_classes.contains(&class) {
            return Err(AxisRejection::ClassNotAccepted);
        }
        if tier < self.min_tier {
            return Err(AxisRejection::TierBelowFloor);
        }
        if !stage.at_least(self.min_stage) {
            return Err(AxisRejection::StageBelowFloor);
        }
        if performed.context != self.work.required_context {
            return Err(AxisRejection::WorkContextMismatch);
        }
        if performed.units < self.work.minimum_units {
            return Err(AxisRejection::WorkBelowMinimum);
        }
        Ok(())
    }
}

impl CanonicalEncode for AxisProofSpec {
    fn encode(&self, writer: &mut CanonWriter) {
        encode_set(&self.acceptable_classes, writer);
        self.min_tier.encode(writer);
        self.min_stage.encode(writer);
        self.work.encode(writer);
    }
}

/// One profile row: the unit of closure. Every semantic field participates in
/// the deterministic fingerprint; every obligation field is mandatory.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompilerProfileRow {
    /// Stable row identity owning the obligation.
    pub row_id: CompilerProfileRowId,
    /// Exact subject selector addressed by this row.
    pub subject: SubjectSelector,
    /// Human-readable proposition statement.
    pub statement: String,
    /// Closed disposition state of this row.
    pub disposition: RowDisposition,
    /// Observed/accepted/supported triple carried verbatim.
    pub support_claim: SupportClaim,
    /// Proof axes demanded by this row, each with its own evidence contract.
    pub axis_specs: BTreeMap<ProofAxis, AxisProofSpec>,
    /// Currentness/completeness rule for the subject coverage.
    pub completeness: CompletenessRequirement,
    /// Explicitly allowed limitations attached to this row.
    pub limitations: BTreeSet<AllowedLimitation>,
    /// Mandatory legacy-exit requirement (`NotApplicable` when none).
    pub legacy_exit: LegacyExitRequirement,
    /// Mandatory invalidation input declaration.
    pub invalidation_input: InvalidationInput,
    /// Mandatory claim ceiling.
    pub claim_ceiling: ClaimCeiling,
    /// Mandatory owner and wake event.
    pub owner: OwnerAndWakeEvent,
}

impl CompilerProfileRow {
    /// Validated constructor enforcing disposition/support consistency and
    /// making every obligation field explicit.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        row_id: CompilerProfileRowId,
        subject: SubjectSelector,
        statement: &str,
        disposition: RowDisposition,
        support_claim: SupportClaim,
        axis_specs: BTreeMap<ProofAxis, AxisProofSpec>,
        completeness: CompletenessRequirement,
        limitations: BTreeSet<AllowedLimitation>,
        legacy_exit: LegacyExitRequirement,
        invalidation_input: InvalidationInput,
        claim_ceiling: ClaimCeiling,
        owner: OwnerAndWakeEvent,
    ) -> Result<Self, CompilerProfileError> {
        let row = Self {
            row_id,
            subject,
            statement: statement.to_string(),
            disposition,
            support_claim,
            axis_specs,
            completeness,
            limitations,
            legacy_exit,
            invalidation_input,
            claim_ceiling,
            owner,
        };
        row.validate_row()?;
        Ok(row)
    }

    fn checked_claim(
        claim: &SupportClaim,
        row_id: &str,
    ) -> Result<SupportClaim, CompilerProfileError> {
        claim.check().map_err(|mut error| match error {
            CompilerProfileError::SupportOverstatement { ref mut detail, .. } => {
                CompilerProfileError::SupportOverstatement {
                    row: row_id.to_string(),
                    detail: detail.clone(),
                }
            }
            CompilerProfileError::DispositionConflict { ref mut detail, .. } => {
                CompilerProfileError::DispositionConflict {
                    row: row_id.to_string(),
                    detail: detail.clone(),
                }
            }
            other => other,
        })?;
        Ok(claim.clone())
    }

    /// Re-checks the row's own invariants after mutation; used by
    /// [`super::profile::CompilerProfileDefinition::validate`] so that
    /// post-construction edits cannot silently weaken a row.
    pub(crate) fn validate_row(&self) -> Result<(), CompilerProfileError> {
        let id = self.row_id.as_str();
        if self.statement.trim().is_empty() {
            return Err(CompilerProfileError::Structure {
                message: format!("row {id} statement must not be empty"),
            });
        }
        if !self.disposition.permits_axes() && !self.axis_specs.is_empty() {
            return Err(CompilerProfileError::DispositionConflict {
                row: id.to_string(),
                detail: "unsupported/not-applicable rows cannot declare proof axes".to_string(),
            });
        }
        if self.disposition.requires_evidence() && self.axis_specs.is_empty() {
            return Err(CompilerProfileError::MissingRequiredEvidence {
                row: id.to_string(),
                axis: "(no axes declared)".to_string(),
                detail: "required rows must declare at least one proof axis".to_string(),
            });
        }
        Self::checked_claim(&self.support_claim, id)?;
        if self.support_claim.semantic_support == SemanticSupportLevel::GeneralSemanticSupport
            && !self.axis_specs.keys().any(|axis| {
                matches!(
                    axis.family,
                    ClaimFamily::ProviderConsumption | ClaimFamily::EditAuthorization
                )
            })
        {
            return Err(CompilerProfileError::SupportOverstatement {
                row: id.to_string(),
                detail: "general semantic support requires at least one provider-consumption \
                         or edit-authorization axis"
                    .to_string(),
            });
        }
        Ok(())
    }
}

impl CanonicalEncode for CompilerProfileRow {
    fn encode(&self, writer: &mut CanonWriter) {
        self.row_id.encode(writer);
        self.subject.encode(writer);
        writer.str_field("stmt", &self.statement);
        self.disposition.encode(writer);
        self.support_claim.encode(writer);
        for (axis, spec) in &self.axis_specs {
            writer.tag("axis");
            axis.encode(writer);
            spec.encode(writer);
        }
        self.completeness.encode(writer);
        encode_set(&self.limitations, writer);
        self.legacy_exit.encode(writer);
        self.invalidation_input.encode(writer);
        self.claim_ceiling.encode(writer);
        self.owner.encode(writer);
    }
}
