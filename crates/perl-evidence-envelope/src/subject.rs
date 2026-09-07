//! What (and which run) an evidence envelope is about.

use perl_source_identity::ProjectId;
use serde::{Deserialize, Serialize};

/// Which automation surface produced the run this envelope is about.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RunSource {
    /// A hosted CI workflow run (e.g. a GitHub Actions workflow run).
    Workflow,
    /// A run driven from a developer's local machine, outside any hosted
    /// workflow.
    Local,
}

impl std::fmt::Display for RunSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Workflow => "workflow",
            Self::Local => "local",
        })
    }
}

/// Identity of the specific run (workflow or local) that produced this
/// envelope's evidence.
///
/// `attempt` distinguishes re-runs of the same logical run (e.g. a GitHub
/// Actions re-run, or a local retry) that share `run_id` but represent
/// distinct executions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RunIdentity {
    /// Which kind of automation surface this run happened on.
    pub source: RunSource,
    /// A producer-defined identifier for the run (e.g. a workflow run ID or
    /// a local session UUID). Opaque to this crate.
    pub run_id: String,
    /// The attempt number within `run_id`, starting at a producer-defined
    /// baseline (typically `1`).
    pub attempt: u32,
}

impl RunIdentity {
    /// Construct a run identity from its source, run ID, and attempt number.
    #[must_use]
    pub fn new(source: RunSource, run_id: impl Into<String>, attempt: u32) -> Self {
        Self { source, run_id: run_id.into(), attempt }
    }
}

/// What this envelope's evidence is about: the owning project and the
/// specific commits/artifact/run in play.
///
/// `base_ref`/`head_ref`/`candidate_ref`/`artifact_ref` are plain,
/// producer-supplied opaque strings (e.g. git SHAs, tags, or artifact names)
/// rather than a new hashed identity type: they already have a stable,
/// authority-defined spelling upstream (git, a CI system, a package
/// registry), and re-hashing them here would create a second, redundant
/// spelling of the same reference rather than new identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EvidenceSubject {
    /// Durable identity of the project this evidence is about.
    pub project_id: ProjectId,
    /// The base commit/ref evidence is compared against, if applicable.
    pub base_ref: Option<String>,
    /// The head commit/ref evidence describes, if applicable.
    pub head_ref: Option<String>,
    /// The candidate branch/PR/commit reference, if evidence is about an
    /// in-flight candidate rather than a landed commit.
    pub candidate_ref: Option<String>,
    /// The specific artifact (binary, package, container digest) evidence
    /// describes, if applicable.
    pub artifact_ref: Option<String>,
    /// Identity of the run that produced this evidence.
    pub run: RunIdentity,
}

impl EvidenceSubject {
    /// Construct an evidence subject from its project, references, and run
    /// identity.
    #[must_use]
    pub fn new(
        project_id: ProjectId,
        base_ref: Option<String>,
        head_ref: Option<String>,
        candidate_ref: Option<String>,
        artifact_ref: Option<String>,
        run: RunIdentity,
    ) -> Self {
        Self { project_id, base_ref, head_ref, candidate_ref, artifact_ref, run }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn sample_subject() -> EvidenceSubject {
        EvidenceSubject::new(
            ProjectId::from_canonical_name("acme/widget"),
            Some("base-sha".to_string()),
            Some("head-sha".to_string()),
            Some("candidate-sha".to_string()),
            Some("artifact-name".to_string()),
            RunIdentity::new(RunSource::Workflow, "run-1", 1),
        )
    }

    #[test]
    fn run_source_serde_round_trip_every_variant() {
        for v in [RunSource::Workflow, RunSource::Local] {
            let json = serde_json::to_string(&v).expect("serialize");
            let back: RunSource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(v, back, "round-trip failed for {v}");
        }
    }

    #[test]
    fn run_source_rejects_unknown_variant_name() {
        assert!(serde_json::from_str::<RunSource>("\"made-up\"").is_err());
    }

    #[test]
    fn run_identity_distinguishes_attempt() {
        let a = RunIdentity::new(RunSource::Workflow, "run-1", 1);
        let b = RunIdentity::new(RunSource::Workflow, "run-1", 2);
        assert_ne!(a, b);
    }

    #[test]
    fn run_identity_distinguishes_source() {
        let a = RunIdentity::new(RunSource::Workflow, "run-1", 1);
        let b = RunIdentity::new(RunSource::Local, "run-1", 1);
        assert_ne!(a, b);
    }

    #[test]
    fn evidence_subject_serde_round_trip() {
        let original = sample_subject();
        let json = serde_json::to_string(&original).expect("serialize");
        let back: EvidenceSubject = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }

    #[test]
    fn evidence_subject_serde_round_trip_with_absent_refs() {
        let original = EvidenceSubject::new(
            ProjectId::from_canonical_name("acme/widget"),
            None,
            None,
            None,
            None,
            RunIdentity::new(RunSource::Local, "local-session-1", 1),
        );
        let json = serde_json::to_string(&original).expect("serialize");
        let back: EvidenceSubject = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }
}
