//! Lean failure-inventory and semantic-boundary identity checks for transitions.
//!
//! Kept separate from structural count reconciliation so ripr probes stay on the
//! new inventory seams rather than reformatted aggregate arithmetic.

use perl_core_harness_types::{ObservedSemanticBoundary, RunFailure, RunFileResult, RunnerStatus};
use std::collections::BTreeSet;

/// Reject Fail rows without a failure record, and empty failure buckets.
pub(super) fn validate_failure_inventory(
    failures: &[RunFailure],
    file_results: &[RunFileResult],
    side: &str,
) -> Result<(), String> {
    let failure_paths =
        failures.iter().map(|failure| failure.path.as_str()).collect::<BTreeSet<_>>();
    for failure in failures {
        if failure.bucket.trim().is_empty() {
            return Err(format!("{side} failure path {} has an empty bucket", failure.path));
        }
    }
    for result in file_results {
        if result.status == RunnerStatus::Fail && !failure_paths.contains(result.path.as_str()) {
            return Err(format!("{side} failing file {} has no failure record", result.path));
        }
    }
    Ok(())
}

/// Reject empty stable ids on semantic-boundary inventory entries.
pub(super) fn validate_semantic_boundary_identities(
    boundaries: &[ObservedSemanticBoundary],
    side: &str,
) -> Result<(), String> {
    for boundary in boundaries {
        if boundary.id.trim().is_empty() {
            return Err(format!(
                "{side} semantic boundary path {} has an empty stable id",
                boundary.path
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod ripr_inventory_observers {
    use super::*;
    use perl_core_harness_types::{
        ObservedSemanticBoundary, RunFailure, RunFileResult, RunnerStatus,
        SemanticBoundaryConfidence, SemanticBoundaryDisposition, SemanticBoundaryLockScope,
        SemanticBoundarySourceSpan,
    };

    #[test]
    fn inventory_validator_call_presence_observer() {
        let pass = [pass_result("base/0.t")];
        assert_eq!(validate_failure_inventory(&[], &pass, "current"), Ok(()));

        let fail = [fail_result("base/0.t")];
        let missing = validate_failure_inventory(&[], &fail, "current");
        assert_eq!(missing, Err("current failing file base/0.t has no failure record".into()));

        let empty_bucket = [sample_failure("base/0.t", "")];
        let bucket = validate_failure_inventory(&empty_bucket, &fail, "current");
        assert_eq!(bucket, Err("current failure path base/0.t has an empty bucket".into()));

        let ok_failure = [sample_failure("base/0.t", "parse_recovery")];
        assert_eq!(validate_failure_inventory(&ok_failure, &fail, "current"), Ok(()));

        assert_eq!(validate_semantic_boundary_identities(&[], "current"), Ok(()));
        let mut boundary = sample_boundary();
        assert_eq!(
            validate_semantic_boundary_identities(std::slice::from_ref(&boundary), "current"),
            Ok(())
        );
        boundary.id.clear();
        assert_eq!(
            validate_semantic_boundary_identities(&[boundary], "current"),
            Err("current semantic boundary path base/0.t has an empty stable id".into())
        );
    }

    fn pass_result(path: &str) -> RunFileResult {
        RunFileResult {
            path: path.into(),
            status: RunnerStatus::Pass,
            assertions_passed: 1,
            assertions_total: 1,
        }
    }

    fn fail_result(path: &str) -> RunFileResult {
        RunFileResult {
            path: path.into(),
            status: RunnerStatus::Fail,
            assertions_passed: 0,
            assertions_total: 1,
        }
    }

    fn sample_failure(path: &str, bucket: &str) -> RunFailure {
        RunFailure {
            path: path.into(),
            phase: "compile".into(),
            bucket: bucket.into(),
            first_diagnostic: "sample".into(),
            workstream: "parser".into(),
            lsp_impact: vec!["diagnostics".into()],
        }
    }

    fn sample_boundary() -> ObservedSemanticBoundary {
        ObservedSemanticBoundary {
            path: "base/0.t".into(),
            id: "boundary".into(),
            disposition: SemanticBoundaryDisposition::Unsupported,
            reason: "sample".into(),
            source_span: SemanticBoundarySourceSpan { start: 0, end: 1 },
            source_kind: "expression".into(),
            confidence: SemanticBoundaryConfidence::Unresolved,
            blocks_compilation: true,
            blocks_downstream_static_facts: true,
            lock_scope: SemanticBoundaryLockScope::None,
            owner_workstream: "parser".into(),
            supporting_test: "tests/sample.rs".into(),
        }
    }
}
