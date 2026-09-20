//! Exact operation subject identity for reachability operations.

use super::{
    ReachabilityContractError, ReachabilityOperationId, ReachabilityOperationKind,
    ReachabilityProfileId, ReachabilityStageId,
};
use serde::{Deserialize, Serialize};

/// Kind of identity carried by a [`ReachabilitySubjectIdentity`].
///
/// The closed kind set makes classification total: every identity slot is
/// one of these kinds, and no stage can replace an upstream subject with a
/// URI, path, request ID, display name, timestamp, thread ID, or local
/// generation counter because no such kind exists.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ReachabilitySubjectIdentityKind {
    /// Accepted workspace snapshot identity and generation.
    WorkspaceSnapshot,
    /// Project identity within the accepted workspace.
    Project,
    /// Workspace root identity and generation.
    Root,
    /// Configuration profile identity and generation.
    ConfigurationProfile,
    /// Environment identity and generation.
    Environment,
    /// Source document instance and generation scope.
    SourceDocumentInstance,
    /// Selected liveness fact-family/support profile.
    FactFamilySupport,
    /// Semantic outcome schema and claim ceiling.
    SemanticOutcomeSchema,
    /// Work-budget profile identity.
    WorkBudgetProfile,
    /// External request/connection/route identity where retained.
    ExternalControl,
    /// Instrument, tool, or schema identity.
    Instrument,
    /// Output identity appended by one completed stage.
    StageOutput,
}

/// One typed, opaque identity slot inside an operation subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ReachabilitySubjectIdentity {
    kind: ReachabilitySubjectIdentityKind,
    value: String,
    generation: Option<String>,
}

impl<'de> Deserialize<'de> for ReachabilitySubjectIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            kind: ReachabilitySubjectIdentityKind,
            value: String,
            generation: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        ReachabilitySubjectIdentity::new(raw.kind, raw.value, raw.generation)
            .map_err(serde::de::Error::custom)
    }
}

impl ReachabilitySubjectIdentity {
    /// Construct a typed identity with an optional non-empty generation.
    ///
    /// # Errors
    ///
    /// Returns [`ReachabilityContractError::EmptyIdentity`] when `value` is
    /// empty or `generation` is [`Some`] but empty.
    pub fn new(
        kind: ReachabilitySubjectIdentityKind,
        value: impl Into<String>,
        generation: Option<String>,
    ) -> Result<Self, ReachabilityContractError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        if let Some(generation) = generation.as_ref()
            && generation.is_empty()
        {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self { kind, value, generation })
    }

    /// The closed identity kind.
    #[must_use]
    pub const fn kind(&self) -> ReachabilitySubjectIdentityKind {
        self.kind
    }

    /// The opaque identity value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// The accepted generation bound to this identity, when one exists.
    #[must_use]
    pub fn generation(&self) -> Option<&str> {
        self.generation.as_deref()
    }
}

/// One stage's exact output identity appended to the operation subject.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReachabilityStageOutput {
    stage: ReachabilityStageId,
    output: ReachabilitySubjectIdentity,
}

impl ReachabilityStageOutput {
    /// Construct a stage-output append.
    #[must_use]
    pub const fn new(stage: ReachabilityStageId, output: ReachabilitySubjectIdentity) -> Self {
        Self { stage, output }
    }

    /// The stage that produced the output.
    #[must_use]
    pub const fn stage(&self) -> &ReachabilityStageId {
        &self.stage
    }

    /// The exact output identity.
    #[must_use]
    pub const fn output(&self) -> &ReachabilitySubjectIdentity {
        &self.output
    }
}

/// The exact, append-only subject of one reachability operation.
///
/// An operation may begin before every downstream identity exists. Each
/// stage appends its exact output identity through
/// [`append_stage_output`](Self::append_stage_output); no stage may replace
/// the upstream subject — the type exposes no method that can overwrite an
/// existing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReachabilityOperationSubject {
    operation_id: ReachabilityOperationId,
    kind: ReachabilityOperationKind,
    identities: Vec<ReachabilitySubjectIdentity>,
    budget_profile_id: ReachabilityProfileId,
    stage_outputs: Vec<ReachabilityStageOutput>,
}

impl<'de> Deserialize<'de> for ReachabilityOperationSubject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            operation_id: ReachabilityOperationId,
            kind: ReachabilityOperationKind,
            identities: Vec<ReachabilitySubjectIdentity>,
            budget_profile_id: ReachabilityProfileId,
            stage_outputs: Vec<ReachabilityStageOutput>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let mut subject = ReachabilityOperationSubject::new(
            raw.operation_id,
            raw.kind,
            raw.identities,
            raw.budget_profile_id,
        )
        .map_err(serde::de::Error::custom)?;
        for output in raw.stage_outputs {
            subject.append_stage_output(output.stage().clone(), output.output().clone());
        }
        Ok(subject)
    }
}

impl ReachabilityOperationSubject {
    /// Construct an operation subject from validated identities.
    ///
    /// Identities are canonicalized into sorted, deduplicated order so the
    /// subject serializes deterministically.
    ///
    /// # Errors
    ///
    /// Returns a contract error when the identity list is empty (an
    /// operation must at least name its accepted authority) or contains a
    /// duplicate kind with conflicting values.
    pub fn new(
        operation_id: ReachabilityOperationId,
        kind: ReachabilityOperationKind,
        identities: Vec<ReachabilitySubjectIdentity>,
        budget_profile_id: ReachabilityProfileId,
    ) -> Result<Self, ReachabilityContractError> {
        if identities.is_empty() {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        let mut identities = identities;
        identities.sort();
        identities.dedup();
        // After dedup, any two adjacent identities of one kind differ in
        // value or generation: reject them so authority is never ambiguous
        // (two snapshots of one workspace with different generations are a
        // conflict, not a coincidence to order by).
        if identities.windows(2).any(|pair| pair[0].kind() == pair[1].kind()) {
            return Err(ReachabilityContractError::EmptyIdentity);
        }
        Ok(Self { operation_id, kind, identities, budget_profile_id, stage_outputs: Vec::new() })
    }

    /// The opaque operation identifier.
    #[must_use]
    pub fn operation_id(&self) -> &ReachabilityOperationId {
        &self.operation_id
    }

    /// The closed operation kind.
    #[must_use]
    pub const fn kind(&self) -> ReachabilityOperationKind {
        self.kind
    }

    /// The canonical, read-only identity list.
    #[must_use]
    pub fn identities(&self) -> &[ReachabilitySubjectIdentity] {
        &self.identities
    }

    /// The work-budget profile this operation runs under.
    #[must_use]
    pub fn budget_profile_id(&self) -> &ReachabilityProfileId {
        &self.budget_profile_id
    }

    /// The append-only stage-output list.
    #[must_use]
    pub fn stage_outputs(&self) -> &[ReachabilityStageOutput] {
        &self.stage_outputs
    }

    /// Append one stage's exact output identity. Appends never overwrite.
    pub fn append_stage_output(
        &mut self,
        stage: ReachabilityStageId,
        output: ReachabilitySubjectIdentity,
    ) {
        self.stage_outputs.push(ReachabilityStageOutput::new(stage, output));
    }

    /// The accepted authority identity of one kind, when retained.
    #[must_use]
    pub fn identity(
        &self,
        kind: ReachabilitySubjectIdentityKind,
    ) -> Option<&ReachabilitySubjectIdentity> {
        self.identities.iter().find(|identity| identity.kind() == kind)
    }

    /// Whether the observed accepted authority still matches this subject's
    /// retained authority of `kind`.
    ///
    /// A missing retained identity or a mismatching observation yields
    /// `false`: supersession checks fail closed.
    #[must_use]
    pub fn authority_matches(
        &self,
        kind: ReachabilitySubjectIdentityKind,
        observed: Option<&ReachabilitySubjectIdentity>,
    ) -> bool {
        match (self.identity(kind), observed) {
            (Some(expected), Some(observed)) => expected == observed,
            (Some(_), None) | (None, Some(_)) | (None, None) => false,
        }
    }
}
