//! Canonical test-topology control plane for maintained compiler cohorts
//! (#12411, authority train #12125–#12129).
//!
//! This module owns the checked registration of compiler-profile proof
//! targets, fail-closed affected routing, and structured nonzero-work route
//! receipts. It is deliberately additive beside the landed compiler-profile
//! vocabulary:
//!
//! - `crate::compiler_profile_contract` keeps profile meaning and identity;
//! - `crate::compiler_profile_initial_rows` keeps the pinned profile rows;
//! - `crate::compiler_profile_observation` keeps the evidence envelope and
//!   adapter registry whose typed non-green states this module mirrors;
//! - `tasks::ci_route` keeps repository-wide CI proof-pack routing.
//!
//! No second evaluator, profile denominator, product behavior, workflow YAML,
//! or branch-protection surface is introduced here. Workflow or check colour is
//! never accepted as semantic profile evidence; only structured receipts
//! written by [`runner`] carry truth, and only per-target receipts can satisfy
//! per-target routes. One aggregate count cannot fill another target.
//!
//! Routing laws encoded here (issue #12411):
//!
//! - process exit zero, compilation, or a generated file is not nonzero-work
//!   proof: receipts require parsed executed work items above the row minimum;
//! - skipped, ignored, cfg-excluded, filtered-out-to-nothing, cancelled,
//!   timed-out, and instrument-failed runs stay non-green;
//! - advisory, scheduled, and manual receipts cannot satisfy a required row;
//! - dormant (`declared_pending`) rows are explicit and cannot go green; when
//!   affected selection hits one, routing fails loudly instead of skipping;
//! - unrelated changes emit an exact checked scoped no-op only through the
//!   canonical selector; they never force the full denominator;
//! - receipts bind the exact head SHA, so a stale receipt from another
//!   candidate never satisfies a route;
//! - the runner exposes no retry loop, and receipts record `retries = 0`
//!   (rerun-until-green laundering is structurally absent);
//! - new compiler-topology-named test targets discovered in the workspace but
//!   missing from the register fail the omitted-new-target guard.

pub mod model;
pub mod receipts;
pub mod route;
pub mod runner;

pub use model::{
    ExecutionKind, RECEIPT_SCHEMA_VERSION, REGISTER_SCHEMA_VERSION, RouteClass, TargetStatus,
    TopologyRegister, TopologyRow,
};
pub use receipts::{
    FanInEntry, FanInReport, FanInViolation, LibTestCounters, LibTestSummary, ReceiptVerdict,
    ScopeNamespace, ScopedNoopProof, TestTopologyReceipt, build_fan_in, canonical_fan_in_digest,
    evaluate_run, load_receipts, parse_libtest_summaries,
};
pub use route::{
    CONTROL_PLANE_PREFIXES, DiscoveredTestTarget, DiscoveryViolation, SelectionDecision,
    SelectionResult, check_discovery_membership, discover_workspace_test_targets, path_under_root,
    select_active_scope,
};
pub use runner::{receipt_path, run_row, run_selected_rows, write_receipt_atomic};
