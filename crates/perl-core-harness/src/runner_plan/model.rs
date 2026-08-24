//! Typed runner-selection and membership-parity receipts.
//!
//! Current declared plans use strict `perl_core_harness.runner_plan.v2`, where
//! every source row records an explicit [`DiscoveryFrame`] and the
//! normalization-schema identity that produced it. Merged #6772 established
//! deterministic declared plans, but its normalizer stripped one leading `../`
//! before deciding the logical root, so `t/`-relative and repository-root
//! spellings could collapse to one member. Frame identity closes that defect
//! and is load-bearing in plan validation and canonical digests.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const RUNNER_PLAN_SCHEMA_VERSION: &str = "perl_core_harness.runner_plan.v2";
/// Historical declared-plan schema. Readable as evidence only; it can never
/// satisfy current reconstruction, parity, authority, bundle, or publication claims.
#[allow(dead_code)]
pub const RUNNER_PLAN_V1_SCHEMA_VERSION: &str = "perl_core_harness.runner_plan.v1";
/// Identity of the lexical resolution law behind current source rows.
pub const SOURCE_NORMALIZATION_SCHEMA_VERSION: &str =
    "perl_core_harness.runner_source_normalization.v2";
pub const RUNNER_PARITY_SCHEMA_VERSION: &str = "perl_core_harness.runner_parity.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Test,
    Harness,
    DirectFallback,
}

impl RunnerKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "test" => Ok(Self::Test),
            "harness" => Ok(Self::Harness),
            "direct_fallback" | "direct-fallback" => Ok(Self::DirectFallback),
            other => Err(format!(
                "unsupported runner {other}; expected test, harness, or direct_fallback"
            )),
        }
    }

    pub fn entrypoint(self) -> &'static str {
        match self {
            Self::Test => "t/TEST",
            Self::Harness => "t/harness",
            Self::DirectFallback => "perl-core-harness direct fallback",
        }
    }
}

/// Explicit reference frame of one raw discovery spelling.
///
/// A raw member spelling is not a repository identity until this frame is
/// known: `lib/Foo/test.pl` relative to `t/` denotes `t/lib/Foo/test.pl`,
/// while `../lib/Foo/test.pl` relative to `t/` and `lib/Foo/test.pl`
/// relative to the repository root denote `lib/Foo/test.pl`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryFrame {
    /// Raw spelling is relative to the upstream logical `<repo>/t` directory.
    RunnerTDirectoryRelative,
    /// Raw spelling is relative to the logical repository root.
    RepositoryRootRelative,
    /// Raw spelling is already a repository-relative path and is independently admitted.
    CanonicalRepositoryPath,
}

impl DiscoveryFrame {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "runner_t_directory_relative" | "runner-t-directory-relative" => {
                Ok(Self::RunnerTDirectoryRelative)
            }
            "repository_root_relative" | "repository-root-relative" => {
                Ok(Self::RepositoryRootRelative)
            }
            "canonical_repository_path" | "canonical-repository-path" => {
                Ok(Self::CanonicalRepositoryPath)
            }
            other => Err(format!(
                "unsupported discovery frame {other}; expected runner-t-directory-relative, \
                 repository-root-relative, or canonical-repository-path"
            )),
        }
    }
}

/// Typed failure classes of the lexical discovery-frame resolution law.
///
/// Every variant fails closed: no filesystem access, checkout state, or
/// ambient path spelling is consulted to repair an ambiguous spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NormalizeError {
    Empty,
    ContainsNul,
    BackslashSeparator { raw: String },
    AbsolutePath { raw: String },
    InvalidComponent { raw: String, component: String },
    EscapeAboveRoot { raw: String },
    ResolvesToEmpty { raw: String },
    UnsupportedForm { canonical_repo_path: String },
    UnsupportedNamespace { canonical_repo_path: String },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceForm {
    DotT,
    TestPl,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePathClass {
    LocalT,
    RootLib,
    Dist,
    Ext,
    Cpan,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationContextClass {
    BaseCompRun,
    LocalTestInit,
    RootLibU1,
    DistributionU2T,
}

/// One current runner-plan source identity row.
///
/// `canonical_repo_path` is the row's target-membership identity: it is the
/// string contributed to `normalized_order` and `normalized_membership`.
/// `raw_path` retains the original caller spelling verbatim (after outer
/// whitespace trim only), independent of the canonical projection and of
/// canonical membership order.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSourceIdentityV2 {
    pub raw_path: String,
    pub discovery_frame: DiscoveryFrame,
    pub canonical_repo_path: String,
    pub source_form: SourceForm,
    pub path_class: SourcePathClass,
    pub invocation_context: InvocationContextClass,
    pub normalization_version: String,
}

// The historical v1 surface is a read-only evidence seam for merged #6772
// receipts; current construction never produces it, so non-test targets see no
// constructors. This mirrors the verbatim-shared-module allow used by the bins.
#[allow(dead_code)]
/// Historical v1 source row. Retained so merged #6772 receipts stay readable;
/// it carries no discovery frame and can never back a current claim.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerSourceItemV1 {
    pub raw_path: String,
    pub canonical_path: String,
    pub source_form: SourceForm,
    pub path_class: SourcePathClass,
    pub invocation_context: InvocationContextClass,
}

#[allow(dead_code)]
/// Historical v1 declared runner plan. Read-only evidence surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerPlanV1 {
    pub schema_version: String,
    pub matrix_fingerprint: String,
    pub target_id: String,
    pub target_contract_digest: String,
    pub runner: RunnerKind,
    pub runner_entrypoint: String,
    pub canonical_selection_entrypoint: String,
    pub raw_discovery_digest: String,
    pub source_items: Vec<RunnerSourceItemV1>,
    pub normalized_order: Vec<String>,
    pub normalized_membership: Vec<String>,
    pub scheduling: RunnerScheduling,
    pub invocation_capture: InvocationCaptureStatus,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}

impl RunnerPlanV1 {
    /// Decode historical v1 receipt bytes, refusing any other schema version.
    #[allow(dead_code)]
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let plan: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("decoding historical runner plan v1: {error}"))?;
        if plan.schema_version != RUNNER_PLAN_V1_SCHEMA_VERSION {
            return Err(format!(
                "historical runner plan decoder requires {}; found {}",
                RUNNER_PLAN_V1_SCHEMA_VERSION, plan.schema_version
            ));
        }
        Ok(plan)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerScheduling {
    pub jobs: Option<u32>,
    pub asap: bool,
    pub state_ordering: bool,
    pub properties: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationCaptureStatus {
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerPlan {
    pub schema_version: String,
    pub matrix_fingerprint: String,
    pub target_id: String,
    pub target_contract_digest: String,
    pub runner: RunnerKind,
    pub runner_entrypoint: String,
    pub canonical_selection_entrypoint: String,
    pub raw_discovery_digest: String,
    pub source_items: Vec<RunnerSourceIdentityV2>,
    pub normalized_order: Vec<String>,
    pub normalized_membership: Vec<String>,
    pub scheduling: RunnerScheduling,
    pub invocation_capture: InvocationCaptureStatus,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipParityStatus {
    Parity,
    Mismatch,
    NotProven,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerParityReport {
    pub schema_version: String,
    pub matrix_fingerprint: String,
    pub target_id: String,
    pub target_contract_digest: String,
    pub left_plan_digest: String,
    pub right_plan_digest: String,
    pub left_raw_discovery_digest: String,
    pub right_raw_discovery_digest: String,
    pub left_runner: RunnerKind,
    pub right_runner: RunnerKind,
    pub membership_status: MembershipParityStatus,
    pub missing_from_right: Vec<String>,
    pub extra_in_right: Vec<String>,
    pub order_equal: bool,
    pub scheduling_equal: bool,
    pub invocation_capture: InvocationCaptureStatus,
    pub limitations: Vec<String>,
    pub claim_boundary: String,
}
