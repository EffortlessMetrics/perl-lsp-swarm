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
        Self {
            policy_identity: policy_identity.into(),
            writers: writers.into_iter().collect(),
        }
    }

    pub(crate) fn contains(&self, writer: &TrustedWriter) -> bool {
        self.writers.contains(writer)
    }
}
