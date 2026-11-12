//! Typed formal-proof receipt model and fail-closed semantic validation.
//!
//! The generic receipt registry checks top-level membership. This module owns
//! the closed formal-method vocabulary and cross-field consistency laws.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const FORMAL_PROOF_CHECK: &str = "formal-proof";
pub const FORMAL_PROOF_SCHEMA_VERSION: &str = "formal_proof.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FormalProofReceipt {
    pub check: String,
    pub schema_version: String,
    pub claim: ClaimIdentity,
    pub source: SourceIdentity,
    pub closure: ClosureIdentity,
    pub toolchain: ToolchainIdentity,
    pub bounds: ProofBounds,
    pub obligations: Obligations,
    pub native: NativeEvidence,
    pub freshness: FreshnessEvidence,
    pub correspondence: CorrespondenceEvidence,
    pub artifacts: Vec<ArtifactIdentity>,
    pub disposition: ClaimDisposition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaimIdentity {
    pub id: String,
    pub generation: u32,
    pub owner_issue: u64,
    pub profile: String,
    pub method: FormalMethod,
    pub binding: ImplementationBinding,
    pub rollout: RolloutState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormalMethod {
    ModelChecked,
    BoundedProved,
    InterleavingChecked,
    DeductivelyProved,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImplementationBinding {
    ModelOnly,
    SubstrateOnly,
    LiveConsumerBound,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RolloutState {
    Planned,
    RunnerAvailable,
    RoutedAdvisory,
    SelectedIntegration,
    RequiredConsumption,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub commit: String,
    pub tree: String,
    pub state: SourceState,
    pub evaluated_content_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Clean,
    Dirty,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClosureIdentity {
    pub algorithm: String,
    pub digest: String,
    pub members: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolchainIdentity {
    pub provider: String,
    pub target: String,
    pub components: BTreeMap<String, ToolComponent>,
    pub features: Vec<String>,
    pub cfg: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolComponent {
    pub version: String,
    pub digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProofBounds {
    pub values: BTreeMap<String, Value>,
    pub assumptions: Vec<String>,
    pub fairness: Vec<String>,
    pub wall_seconds: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Obligations {
    pub positive: ExpectedSet,
    pub harness_controls: ExpectedSet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExpectedSet {
    pub expected: u32,
    pub observed: u32,
    pub satisfied: u32,
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeEvidence {
    pub result: NativeResult,
    pub reason: Option<NonPassReason>,
    pub process_settled: bool,
    pub properties: BTreeMap<String, PropertyOutcome>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeResult {
    Pass,
    Counterexample,
    NotProven,
    UnsupportedHost,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NonPassReason {
    Timeout,
    ResourceExhausted,
    Skipped,
    ZeroWork,
    MalformedOutput,
    UnrecognizedOutput,
    MissingControl,
    ToolError,
    ProcessUnsettled,
    UnsupportedConstruct,
    MissingArtifact,
    StaleInput,
    IncompleteCorrespondence,
    UnsupportedHost,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PropertyOutcome {
    Pass,
    Counterexample,
    NotProven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FreshnessEvidence {
    pub status: FreshnessStatus,
    pub evaluated_source_commit: String,
    pub evaluated_closure_digest: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessStatus {
    Current,
    Stale,
    Unknown,
    Retired,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CorrespondenceEvidence {
    pub status: CorrespondenceStatus,
    pub controls: ExpectedSet,
    pub artifact_ids: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CorrespondenceStatus {
    Established,
    Counterexample,
    NotProven,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIdentity {
    pub id: String,
    pub kind: ArtifactKind,
    pub digest: String,
    pub path: String,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Stdout,
    Stderr,
    StructuredReport,
    Counterexample,
    ProcessReceipt,
    Correspondence,
    Route,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ClaimDisposition {
    pub status: DispositionStatus,
    pub reason: Option<String>,
    pub superseded_by: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispositionStatus {
    Active,
    Retired,
    Superseded,
}

/// Validate one receipt without upgrading non-pass evidence into a pass.
pub fn validate_value(value: &Value) -> Vec<String> {
    let receipt = match serde_json::from_value::<FormalProofReceipt>(value.clone()) {
        Ok(receipt) => receipt,
        Err(error) => {
            return vec![format!("formal-proof receipt does not match the typed schema: {error}")];
        }
    };
    receipt.validation_errors()
}

impl FormalProofReceipt {
    fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        exact("check", &self.check, FORMAL_PROOF_CHECK, &mut errors);
        exact(
            "schema_version",
            &self.schema_version,
            FORMAL_PROOF_SCHEMA_VERSION,
            &mut errors,
        );
        nonempty("claim.id", &self.claim.id, &mut errors);
        if !valid_claim_id(&self.claim.id) {
            errors.push("claim.id must match FV-<UPPERCASE-TOKENS>-NNN".to_string());
        }
        if self.claim.generation == 0 || self.claim.owner_issue == 0 {
            errors.push("claim.generation and claim.owner_issue must be >= 1".to_string());
        }
        nonempty("claim.profile", &self.claim.profile, &mut errors);

        sha40("source.commit", &self.source.commit, &mut errors);
        sha40("source.tree", &self.source.tree, &mut errors);
        match self.source.state {
            SourceState::Dirty => match &self.source.evaluated_content_digest {
                Some(digest) => sha256("source.evaluated_content_digest", digest, &mut errors),
                None => errors.push(
                    "dirty source must bind source.evaluated_content_digest".to_string(),
                ),
            },
            SourceState::Clean | SourceState::Unknown => {
                if self.source.evaluated_content_digest.is_some() {
                    errors.push(
                        "only dirty source may carry evaluated_content_digest".to_string(),
                    );
                }
            }
        }

        nonempty("closure.algorithm", &self.closure.algorithm, &mut errors);
        sha256("closure.digest", &self.closure.digest, &mut errors);
        if self.closure.members.is_empty() {
            errors.push("closure.members must not be empty".to_string());
        }
        for (identity, digest) in &self.closure.members {
            nonempty("closure member identity", identity, &mut errors);
            sha256("closure member digest", digest, &mut errors);
        }

        nonempty("toolchain.provider", &self.toolchain.provider, &mut errors);
        nonempty("toolchain.target", &self.toolchain.target, &mut errors);
        if self.toolchain.components.is_empty() {
            errors.push("toolchain.components must not be empty".to_string());
        }
        for (name, component) in &self.toolchain.components {
            nonempty("toolchain component name", name, &mut errors);
            nonempty("toolchain component version", &component.version, &mut errors);
            sha256("toolchain component digest", &component.digest, &mut errors);
        }
        unique_nonempty("toolchain.features", &self.toolchain.features, &mut errors);
        unique_nonempty("toolchain.cfg", &self.toolchain.cfg, &mut errors);

        if self.bounds.values.is_empty() {
            errors.push("bounds.values must not be empty".to_string());
        }
        for (name, value) in &self.bounds.values {
            nonempty("bound name", name, &mut errors);
            if !(value.is_string() || value.is_number() || value.is_boolean()) {
                errors.push(format!("bound '{name}' must be a scalar JSON value"));
            }
        }
        unique_nonempty("bounds.assumptions", &self.bounds.assumptions, &mut errors);
        unique_nonempty("bounds.fairness", &self.bounds.fairness, &mut errors);
        if self.bounds.wall_seconds == 0 || self.bounds.output_bytes == 0 {
            errors.push("bounds wall_seconds and output_bytes must be >= 1".to_string());
        }

        validate_set("obligations.positive", &self.obligations.positive, true, &mut errors);
        validate_set(
            "obligations.harness_controls",
            &self.obligations.harness_controls,
            true,
            &mut errors,
        );
        validate_artifacts(&self.artifacts, &mut errors);
        self.validate_native(&mut errors);
        self.validate_freshness(&mut errors);
        self.validate_correspondence(&mut errors);
        self.validate_disposition(&mut errors);
        errors
    }

    fn validate_native(&self, errors: &mut Vec<String>) {
        match self.native.result {
            NativeResult::Pass => {
                if self.native.reason.is_some() {
                    errors.push("native pass must not carry a reason".to_string());
                }
                if !self.native.process_settled {
                    errors.push("native pass requires settled process custody".to_string());
                }
                if self.native.properties.is_empty()
                    || self
                        .native
                        .properties
                        .values()
                        .any(|outcome| *outcome != PropertyOutcome::Pass)
                {
                    errors.push(
                        "native pass requires a nonempty all-pass property denominator".to_string(),
                    );
                }
                complete("positive", &self.obligations.positive, errors);
                complete("harness controls", &self.obligations.harness_controls, errors);
                if !self.artifacts.iter().any(|artifact| artifact.required) {
                    errors.push("native pass requires a required raw artifact".to_string());
                }
            }
            NativeResult::Counterexample => {
                if self.native.reason.is_some() || !self.native.process_settled {
                    errors.push(
                        "counterexample requires no non-pass reason and settled process custody"
                            .to_string(),
                    );
                }
                if !self
                    .native
                    .properties
                    .values()
                    .any(|outcome| *outcome == PropertyOutcome::Counterexample)
                {
                    errors.push("counterexample requires a counterexample property".to_string());
                }
                if !self.artifacts.iter().any(|artifact| {
                    artifact.required && artifact.kind == ArtifactKind::Counterexample
                }) {
                    errors.push("counterexample requires a required trace artifact".to_string());
                }
            }
            NativeResult::NotProven => {
                if self.native.reason.is_none() {
                    errors.push("not_proven requires a typed reason".to_string());
                }
                if !self.native.process_settled
                    && self.native.reason != Some(NonPassReason::ProcessUnsettled)
                {
                    errors.push(
                        "unsettled process requires reason process_unsettled".to_string(),
                    );
                }
            }
            NativeResult::UnsupportedHost => {
                if self.native.reason != Some(NonPassReason::UnsupportedHost) {
                    errors.push(
                        "unsupported_host requires reason unsupported_host".to_string(),
                    );
                }
                if !self.native.properties.is_empty() {
                    errors.push(
                        "unsupported_host must not claim property observations".to_string(),
                    );
                }
            }
        }
    }

    fn validate_freshness(&self, errors: &mut Vec<String>) {
        sha40(
            "freshness.evaluated_source_commit",
            &self.freshness.evaluated_source_commit,
            errors,
        );
        sha256(
            "freshness.evaluated_closure_digest",
            &self.freshness.evaluated_closure_digest,
            errors,
        );
        match self.freshness.status {
            FreshnessStatus::Current => {
                if self.freshness.reason.is_some()
                    || self.freshness.evaluated_source_commit != self.source.commit
                    || self.freshness.evaluated_closure_digest != self.closure.digest
                {
                    errors.push(
                        "current freshness must have no reason and bind source plus closure"
                            .to_string(),
                    );
                }
            }
            FreshnessStatus::Stale | FreshnessStatus::Unknown => {
                required_reason("freshness.reason", &self.freshness.reason, errors);
            }
            FreshnessStatus::Retired => {
                required_reason("freshness.reason", &self.freshness.reason, errors);
                if self.disposition.status != DispositionStatus::Retired {
                    errors.push("retired freshness requires retired disposition".to_string());
                }
            }
            FreshnessStatus::Superseded => {
                required_reason("freshness.reason", &self.freshness.reason, errors);
                if self.disposition.status != DispositionStatus::Superseded {
                    errors.push(
                        "superseded freshness requires superseded disposition".to_string(),
                    );
                }
            }
        }
    }

    fn validate_correspondence(&self, errors: &mut Vec<String>) {
        if self.claim.binding == ImplementationBinding::ModelOnly {
            if self.correspondence.status != CorrespondenceStatus::NotApplicable
                || self.correspondence.controls.expected != 0
                || !self.correspondence.controls.ids.is_empty()
                || !self.correspondence.artifact_ids.is_empty()
                || self.correspondence.reason.is_some()
            {
                errors.push(
                    "model_only binding requires empty not_applicable correspondence".to_string(),
                );
            }
            return;
        }

        validate_set(
            "correspondence.controls",
            &self.correspondence.controls,
            true,
            errors,
        );
        unique_nonempty(
            "correspondence.artifact_ids",
            &self.correspondence.artifact_ids,
            errors,
        );
        match self.correspondence.status {
            CorrespondenceStatus::Established => {
                if self.correspondence.reason.is_some() {
                    errors.push("established correspondence must not carry a reason".to_string());
                }
                complete("correspondence controls", &self.correspondence.controls, errors);
                if self.correspondence.artifact_ids.is_empty() {
                    errors.push(
                        "established correspondence requires a bound artifact".to_string(),
                    );
                }
            }
            CorrespondenceStatus::Counterexample | CorrespondenceStatus::NotProven => {
                required_reason(
                    "correspondence.reason",
                    &self.correspondence.reason,
                    errors,
                );
            }
            CorrespondenceStatus::NotApplicable => errors.push(
                "non-model binding cannot use not_applicable correspondence".to_string(),
            ),
        }

        let ids = self
            .artifacts
            .iter()
            .map(|artifact| artifact.id.as_str())
            .collect::<BTreeSet<_>>();
        for artifact_id in &self.correspondence.artifact_ids {
            if !ids.contains(artifact_id.as_str()) {
                errors.push(format!(
                    "correspondence artifact '{artifact_id}' is not present in artifacts"
                ));
            }
        }
    }

    fn validate_disposition(&self, errors: &mut Vec<String>) {
        match self.disposition.status {
            DispositionStatus::Active => {
                if self.disposition.reason.is_some()
                    || self.disposition.superseded_by.is_some()
                    || matches!(
                        self.freshness.status,
                        FreshnessStatus::Retired | FreshnessStatus::Superseded
                    )
                {
                    errors.push(
                        "active disposition cannot be retired, superseded, or carry terminal metadata"
                            .to_string(),
                    );
                }
            }
            DispositionStatus::Retired => {
                required_reason("disposition.reason", &self.disposition.reason, errors);
                if self.disposition.superseded_by.is_some()
                    || self.freshness.status != FreshnessStatus::Retired
                    || self.claim.rollout != RolloutState::Retired
                {
                    errors.push(
                        "retired disposition requires retired freshness/rollout and no superseder"
                            .to_string(),
                    );
                }
            }
            DispositionStatus::Superseded => {
                required_reason("disposition.reason", &self.disposition.reason, errors);
                match &self.disposition.superseded_by {
                    Some(value) if valid_claim_id(value) => {}
                    _ => errors.push(
                        "superseded disposition requires a valid superseded_by claim".to_string(),
                    ),
                }
                if self.freshness.status != FreshnessStatus::Superseded {
                    errors.push(
                        "superseded disposition requires superseded freshness".to_string(),
                    );
                }
            }
        }
    }
}

fn validate_set(prefix: &str, set: &ExpectedSet, nonzero: bool, errors: &mut Vec<String>) {
    if nonzero && set.expected == 0 {
        errors.push(format!("{prefix}.expected must be >= 1"));
    }
    if set.observed > set.expected || set.satisfied > set.observed {
        errors.push(format!(
            "{prefix} counts must satisfy satisfied <= observed <= expected"
        ));
    }
    if u64::try_from(set.ids.len()).unwrap_or(u64::MAX) != u64::from(set.expected) {
        errors.push(format!("{prefix}.ids length must equal expected"));
    }
    unique_nonempty(&format!("{prefix}.ids"), &set.ids, errors);
}

fn complete(prefix: &str, set: &ExpectedSet, errors: &mut Vec<String>) {
    if set.observed != set.expected || set.satisfied != set.expected {
        errors.push(format!("{prefix} must be completely observed and satisfied"));
    }
}

fn validate_artifacts(artifacts: &[ArtifactIdentity], errors: &mut Vec<String>) {
    if artifacts.is_empty() {
        errors.push("artifacts must not be empty".to_string());
    }
    let mut ids = BTreeSet::new();
    for artifact in artifacts {
        nonempty("artifact.id", &artifact.id, errors);
        nonempty("artifact.path", &artifact.path, errors);
        sha256("artifact.digest", &artifact.digest, errors);
        if !ids.insert(artifact.id.as_str()) {
            errors.push(format!("duplicate artifact id '{}'", artifact.id));
        }
    }
}

fn unique_nonempty(prefix: &str, values: &[String], errors: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty(prefix, value, errors);
        if !seen.insert(value.as_str()) {
            errors.push(format!("duplicate value '{value}' in {prefix}"));
        }
    }
}

fn required_reason(field: &str, reason: &Option<String>, errors: &mut Vec<String>) {
    match reason {
        Some(value) => nonempty(field, value, errors),
        None => errors.push(format!("{field} must explain the non-current result")),
    }
}

fn exact(field: &str, value: &str, expected: &str, errors: &mut Vec<String>) {
    if value != expected {
        errors.push(format!("{field} must be '{expected}', got '{value}'"));
    }
}

fn nonempty(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{field} must not be empty"));
    }
}

fn sha40(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 40
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        errors.push(format!("{field} must be 40 lowercase hexadecimal characters"));
    }
}

fn sha256(field: &str, value: &str, errors: &mut Vec<String>) {
    let Some(hex) = value.strip_prefix("sha256:") else {
        errors.push(format!("{field} must use sha256:<64-lowercase-hex>"));
        return;
    };
    if hex.len() != 64
        || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        || hex.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        errors.push(format!("{field} must contain 64 lowercase hexadecimal characters"));
    }
}

fn valid_claim_id(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("FV-") else {
        return false;
    };
    let Some((prefix, suffix)) = rest.rsplit_once('-') else {
        return false;
    };
    suffix.len() == 3
        && suffix.bytes().all(|byte| byte.is_ascii_digit())
        && prefix.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const TREE: &str = "89abcdef0123456789abcdef0123456789abcdef";
    const DIGEST: &str =
        "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const ALT: &str =
        "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";

    fn valid_receipt() -> Value {
        json!({
            "check": "formal-proof",
            "schema_version": "formal_proof.v1",
            "claim": {
                "id": "FV-POSITION-LAWS-001",
                "generation": 1,
                "owner_issue": 19300,
                "profile": "smoke",
                "method": "bounded_proved",
                "binding": "live_consumer_bound",
                "rollout": "runner_available"
            },
            "source": {
                "commit": SHA,
                "tree": TREE,
                "state": "clean",
                "evaluated_content_digest": null
            },
            "closure": {
                "algorithm": "formal_closure.v1",
                "digest": DIGEST,
                "members": {
                    "crates/perl-line-index/src/lib.rs": ALT
                }
            },
            "toolchain": {
                "provider": "kani",
                "target": "x86_64-unknown-linux-gnu",
                "components": {
                    "kani": { "version": "0.68.0", "digest": ALT },
                    "cbmc": { "version": "6.8.0", "digest": DIGEST }
                },
                "features": [],
                "cfg": ["kani"]
            },
            "bounds": {
                "values": { "max_scalars": 4, "unwind": 8 },
                "assumptions": ["input is valid UTF-8"],
                "fairness": [],
                "wall_seconds": 120,
                "output_bytes": 1048576
            },
            "obligations": {
                "positive": {
                    "expected": 2,
                    "observed": 2,
                    "satisfied": 2,
                    "ids": ["roundtrip", "surrogate-boundary"]
                },
                "harness_controls": {
                    "expected": 1,
                    "observed": 1,
                    "satisfied": 1,
                    "ids": ["scalar-count-mutant"]
                }
            },
            "native": {
                "result": "pass",
                "reason": null,
                "process_settled": true,
                "properties": {
                    "roundtrip": "pass",
                    "surrogate-boundary": "pass"
                }
            },
            "freshness": {
                "status": "current",
                "evaluated_source_commit": SHA,
                "evaluated_closure_digest": DIGEST,
                "reason": null
            },
            "correspondence": {
                "status": "established",
                "controls": {
                    "expected": 1,
                    "observed": 1,
                    "satisfied": 1,
                    "ids": ["production-endpoint-shift"]
                },
                "artifact_ids": ["correspondence"],
                "reason": null
            },
            "artifacts": [
                {
                    "id": "native-report",
                    "kind": "structured_report",
                    "digest": DIGEST,
                    "path": "target/formal/native.json",
                    "required": true
                },
                {
                    "id": "correspondence",
                    "kind": "correspondence",
                    "digest": ALT,
                    "path": "target/formal/correspondence.json",
                    "required": true
                }
            ],
            "disposition": {
                "status": "active",
                "reason": null,
                "superseded_by": null
            }
        })
    }

    #[test]
    fn accepts_complete_current_pass() {
        let errors = validate_value(&valid_receipt());
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn typed_receipt_round_trips() {
        let receipt = serde_json::from_value::<FormalProofReceipt>(valid_receipt())
            .expect("valid receipt should parse");
        let encoded = serde_json::to_value(&receipt).expect("receipt should serialize");
        let decoded = serde_json::from_value::<FormalProofReceipt>(encoded)
            .expect("serialized receipt should parse");
        assert_eq!(decoded, receipt);
    }

    #[test]
    fn rejects_unknown_fields_and_zero_work_pass() {
        let mut value = valid_receipt();
        value["claim"]["invented"] = json!(true);
        let errors = validate_value(&value);
        assert!(errors.iter().any(|error| error.contains("unknown field")));

        let mut value = valid_receipt();
        value["obligations"]["positive"] = json!({
            "expected": 0,
            "observed": 0,
            "satisfied": 0,
            "ids": []
        });
        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("positive.expected must be >= 1"))
        );
    }

    #[test]
    fn accepts_honest_not_proven_and_stale_receipts() {
        let mut value = valid_receipt();
        value["native"] = json!({
            "result": "not_proven",
            "reason": "timeout",
            "process_settled": true,
            "properties": {}
        });
        value["obligations"]["positive"]["observed"] = json!(0);
        value["obligations"]["positive"]["satisfied"] = json!(0);
        value["obligations"]["harness_controls"]["observed"] = json!(0);
        value["obligations"]["harness_controls"]["satisfied"] = json!(0);
        value["freshness"]["status"] = json!("stale");
        value["freshness"]["reason"] = json!("production wrapper changed");
        value["freshness"]["evaluated_source_commit"] = json!(TREE);

        let errors = validate_value(&value);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn rejects_unsettled_pass_and_model_only_live_correspondence() {
        let mut value = valid_receipt();
        value["native"]["process_settled"] = json!(false);
        let errors = validate_value(&value);
        assert!(errors.iter().any(|error| error.contains("settled process")));

        let mut value = valid_receipt();
        value["claim"]["binding"] = json!("model_only");
        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("empty not_applicable"))
        );
    }

    #[test]
    fn counterexample_requires_bound_trace() {
        let mut value = valid_receipt();
        value["native"] = json!({
            "result": "counterexample",
            "reason": null,
            "process_settled": true,
            "properties": { "roundtrip": "counterexample" }
        });
        let errors = validate_value(&value);
        assert!(
            errors
                .iter()
                .any(|error| error.contains("trace artifact"))
        );
    }
}
