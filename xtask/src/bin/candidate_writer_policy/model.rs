use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FindingKind {
    CandidateDefinedWriter,
    LocalReusableWriter,
    MutableReusableWriter,
    SelfModifyingWriter,
}

impl FindingKind {
    const fn code(self) -> &'static str {
        match self {
            Self::CandidateDefinedWriter => "candidate_defined_writer",
            Self::LocalReusableWriter => "local_reusable_writer",
            Self::MutableReusableWriter => "mutable_reusable_writer",
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
