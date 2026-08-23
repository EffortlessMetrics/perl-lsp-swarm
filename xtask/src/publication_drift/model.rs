use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(super) const SUPPORTED_SCHEMA_VERSION: u32 = 1;
pub(super) const SUPPORTED_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub(super) const EXPECTED_TRANSLATION: &str = "expected_publication_translation";
pub(super) const APPROVED_EXCLUSION: &str = "approved_lineage_exclusion";
pub(super) const RELEASE_METADATA: &str = "release_metadata_only";
pub(super) const PRODUCT_DRIFT: &str = "product_drift";
pub(super) const NOT_PROVEN_CLASS: &str = "unknown_or_not_proven";
pub(super) const REQUIRED_INVARIANTS: &[&str] = &[
    "targets_requested_are_built",
    "archive_members_match_consumers",
    "server_dap_pairing",
    "extension_claims_match_vsix",
    "public_install_docs_are_executable",
    "support_posture_matches_claims",
    "artifact_traceable_to_public_sha",
    "product_path_coverage_complete",
    "release_repo_unique_dispositions_complete",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubjectIdentity {
    pub(crate) repository: String,
    pub(crate) sha: String,
    pub(crate) tree_digest: String,
    pub(crate) version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestIdentity {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) swarm_sha: String,
    pub(crate) public_sha: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub(crate) schema_version: u32,
    pub(crate) swarm: SubjectIdentity,
    pub(crate) public: SubjectIdentity,
    pub(crate) manifest: Option<ManifestIdentity>,
    pub(crate) differences: Option<Vec<ObservedDifference>>,
    pub(crate) invariants: Option<Vec<ObservedInvariant>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedDifference {
    pub(crate) path: String,
    pub(crate) classification: String,
    pub(crate) behavior_changed: bool,
    pub(crate) manifest_rule: Option<String>,
    pub(crate) owner: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ObservedInvariant {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) owner: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PublicationManifest {
    pub(crate) schema_version: u32,
    pub(crate) swarm_repository: String,
    pub(crate) public_repository: String,
    pub(crate) swarm_sha: String,
    pub(crate) public_sha: String,
    pub(crate) swarm_tree_digest: String,
    pub(crate) public_tree_digest: String,
    pub(crate) version: String,
    pub(crate) rules: Vec<ManifestRule>,
    pub(crate) required_invariants: Vec<ManifestInvariant>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestRule {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) classification: String,
    pub(crate) owner: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestInvariant {
    pub(crate) id: String,
    pub(crate) owner: String,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedManifest {
    pub(crate) document: PublicationManifest,
    pub(crate) actual_sha256: String,
}

#[derive(Clone, Debug)]
pub(crate) enum AuthoritySource {
    Missing,
    Invalid { message: String, actual_sha256: Option<String> },
    Loaded(LoadedManifest),
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedAuthority {
    pub(crate) rules: BTreeMap<String, ManifestRule>,
    pub(crate) required_invariants: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Verdict {
    Clean,
    Drift,
    NotProven,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManifestVerificationStatus {
    Verified,
    Missing,
    Invalid,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ManifestVerification {
    pub(crate) status: ManifestVerificationStatus,
    pub(crate) actual_sha256: Option<String>,
    pub(crate) schema_version: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Receipt {
    pub(crate) schema_version: u32,
    pub(crate) comparison_version: Option<String>,
    pub(crate) swarm: SubjectIdentity,
    pub(crate) public: SubjectIdentity,
    pub(crate) manifest: Option<ManifestIdentity>,
    pub(crate) manifest_verification: ManifestVerification,
    pub(crate) differences: Vec<ClassifiedDifference>,
    pub(crate) invariants: Vec<ClassifiedInvariant>,
    pub(crate) authority_valid: bool,
    pub(crate) blockers: Vec<Blocker>,
    pub(crate) verdict: Verdict,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClassifiedDifference {
    pub(crate) path: String,
    pub(crate) declared_classification: String,
    pub(crate) effective_classification: String,
    pub(crate) behavior_changed: bool,
    pub(crate) manifest_rule: Option<String>,
    pub(crate) owner: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ClassifiedInvariant {
    pub(crate) id: String,
    pub(crate) status: String,
    pub(crate) owner: String,
    pub(crate) evidence: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct Blocker {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) owner: String,
}

#[derive(Debug, Default)]
pub(crate) struct ClassificationState {
    pub(crate) drift: bool,
    pub(crate) not_proven: bool,
    pub(crate) blockers: Vec<Blocker>,
}

impl ClassificationState {
    pub(crate) fn mark_drift(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.drift = true;
        self.push_blocker(code, message, owner);
    }

    pub(crate) fn mark_not_proven(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.not_proven = true;
        self.push_blocker(code, message, owner);
    }

    pub(crate) fn push_blocker(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
        owner: impl Into<String>,
    ) {
        self.blockers.push(Blocker {
            code: code.into(),
            message: message.into(),
            owner: owner.into(),
        });
    }
}
