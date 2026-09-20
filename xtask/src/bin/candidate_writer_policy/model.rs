use std::collections::BTreeSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FindingKind {
    CandidateDefinedWriter,
    LocalReusableWriter,
    MutableReusableWriter,
    UntrustedReusableWriter,
    UnprovenTokenAuthority,
    SelfModifyingWriter,
    CandidateCodeExecutionWriter,
}

impl FindingKind {
    const fn code(self) -> &'static str {
        match self {
            Self::CandidateDefinedWriter => "candidate_defined_writer",
            Self::LocalReusableWriter => "local_reusable_writer",
            Self::MutableReusableWriter => "mutable_reusable_writer",
            Self::UntrustedReusableWriter => "untrusted_reusable_writer",
            Self::UnprovenTokenAuthority => "unproven_token_authority",
            Self::SelfModifyingWriter => "self_modifying_writer",
            Self::CandidateCodeExecutionWriter => "candidate_code_execution_writer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Finding {
    pub(crate) workflow: String,
    pub(crate) job: String,
    pub(crate) kind: FindingKind,
    pub(crate) detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: job `{}` [{}] {}",
            self.workflow,
            self.job,
            self.kind.code(),
            self.detail
        )
    }
}

/// One exact reusable workflow approved by a protected control-plane decision.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TrustedWriter {
    pub(crate) repository: String,
    pub(crate) workflow_path: String,
    pub(crate) commit_sha: String,
}

impl TrustedWriter {
    pub(crate) fn new(
        repository: impl Into<String>,
        workflow_path: impl Into<String>,
        commit_sha: impl Into<String>,
    ) -> Self {
        Self {
            repository: repository.into(),
            workflow_path: workflow_path.into(),
            commit_sha: commit_sha.into(),
        }
    }
}

/// Reviewed trusted-writer authority consumed by the scanner.
#[derive(Debug, Clone)]
pub(crate) struct TrustedWriterPolicy {
    pub(crate) policy_identity: String,
    writers: BTreeSet<TrustedWriter>,
}

impl TrustedWriterPolicy {
    pub(crate) fn empty() -> Self {
        Self {
            policy_identity: "candidate-writer.trusted-writers.v1:none".into(),
            writers: BTreeSet::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_writers(
        policy_identity: impl Into<String>,
        writers: impl IntoIterator<Item = TrustedWriter>,
    ) -> Self {
        Self { policy_identity: policy_identity.into(), writers: writers.into_iter().collect() }
    }

    pub(crate) fn contains(&self, writer: &TrustedWriter) -> bool {
        self.writers.contains(writer)
    }
}

/// One candidate-defined writer path that predates this recurrence control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KnownIncident {
    pub(crate) workflow: &'static str,
    pub(crate) job: &'static str,
    pub(crate) kind: FindingKind,
    pub(crate) owning_issue: &'static str,
    pub(crate) note: &'static str,
}

impl KnownIncident {
    fn matches(&self, finding: &Finding) -> bool {
        self.workflow == finding.workflow && self.job == finding.job && self.kind == finding.kind
    }
}

/// Open candidate-writer incidents accepted while their retirement is pending.
///
/// This list may only shrink. `known_incidents_still_reproduce` fails when an
/// entry stops matching, so repairing a workflow forces its removal here rather
/// than leaving a stale exemption that would re-accept a future regression.
/// Adding an entry is not a repair and needs the owning issue's disposition.
pub(crate) const KNOWN_INCIDENTS: &[KnownIncident] = &[KnownIncident {
    workflow: "tokmd.yml",
    job: "comment",
    kind: FindingKind::CandidateDefinedWriter,
    owning_issue: "#7670",
    note: "candidate-controlled job holding `pull-requests: write`; retiring it needs the \
           protected publisher from #7656/#7664, so it is recorded rather than repaired here",
}];

/// Split a scan into paths this control rejects and recorded open incidents.
///
/// `stale` names incidents that no longer reproduce and must leave the list.
pub(crate) struct IncidentPartition<'a> {
    pub(crate) new_findings: Vec<&'a Finding>,
    pub(crate) stale: Vec<&'static KnownIncident>,
}

pub(crate) fn partition_incidents(findings: &[Finding]) -> IncidentPartition<'_> {
    let new_findings = findings
        .iter()
        .filter(|finding| !KNOWN_INCIDENTS.iter().any(|incident| incident.matches(finding)))
        .collect();
    let stale = KNOWN_INCIDENTS
        .iter()
        .filter(|incident| !findings.iter().any(|finding| incident.matches(finding)))
        .collect();
    IncidentPartition { new_findings, stale }
}
