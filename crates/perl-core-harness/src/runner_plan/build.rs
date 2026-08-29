//! Deterministic runner-plan construction from one target contract and raw discovery.

use crate::model::{
    TargetAuthority, TargetAuthorityKind, TargetKind, TargetMatrixEntry, TargetScriptForm,
    UpstreamTargetMatrix,
};
use crate::normalize::{matches_any_selector, normalize_source_item, source_form_allowed};
use crate::runner_model::{
    DiscoveryFrame, InvocationCaptureStatus, RUNNER_PLAN_SCHEMA_VERSION, RunnerKind, RunnerPlan,
    RunnerScheduling, SOURCE_NORMALIZATION_SCHEMA_VERSION,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const INVOCATION_LIMITATION: &str = "per_file_upstream_scan_and_effective_invocation_not_captured";
const SCHEDULING_DECLARATION_LIMITATION: &str = "scheduling_inputs_are_declared_not_observed";
const DISCOVERY_DECLARATION_LIMITATION: &str =
    "raw_discovery_stream_is_declared_input_not_observed_runner_output";
const DIRECT_FALLBACK_LIMITATION: &str = "direct_fallback_missing_upstream_selection_context";
const ALTERNATE_RUNNER_LIMITATION: &str = "alternate_runner_requires_membership_parity_evidence";

#[cfg(test)]
pub(crate) fn build_runner_plan(
    matrix: &UpstreamTargetMatrix,
    target_id: &str,
    runner: RunnerKind,
    raw_discovery: &[u8],
    scheduling: RunnerScheduling,
) -> Result<RunnerPlan, String> {
    build_runner_plan_with_frame(
        matrix,
        target_id,
        runner,
        raw_discovery,
        DiscoveryFrame::CanonicalRepositoryPath,
        scheduling,
    )
}

pub(crate) fn build_runner_plan_with_frame(
    matrix: &UpstreamTargetMatrix,
    target_id: &str,
    runner: RunnerKind,
    raw_discovery: &[u8],
    discovery_frame: DiscoveryFrame,
    scheduling: RunnerScheduling,
) -> Result<RunnerPlan, String> {
    matrix.validate()?;
    let matrix_fingerprint = matrix.fingerprint()?;
    let entry = find_target(matrix, target_id)?;
    let (selectors, script_forms) = effective_selection(matrix, entry)?;
    if selectors.is_empty() || script_forms.is_empty() {
        return Err(format!("target {target_id} does not define a physical source population"));
    }
    let canonical_authority = effective_selection_authority(matrix, entry)?;
    let text = std::str::from_utf8(raw_discovery)
        .map_err(|error| format!("raw discovery is not UTF-8: {error}"))?;
    let mut source_items = Vec::new();
    let mut seen = BTreeSet::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let item = normalize_source_item(line, discovery_frame)?;
        if !source_form_allowed(item.source_form, &script_forms) {
            return Err(format!(
                "target {target_id} does not allow source form {:?} for {}",
                item.source_form, item.canonical_path
            ));
        }
        if !matches_any_selector(&item.canonical_path, &selectors) {
            return Err(format!(
                "discovered path {} is outside target {target_id}",
                item.canonical_path
            ));
        }
        if !seen.insert(item.canonical_path.clone()) {
            return Err(format!("raw discovery contains duplicate path {}", item.canonical_path));
        }
        source_items.push(item);
    }
    if source_items.is_empty() {
        return Err(format!("target {target_id} discovery contains no files"));
    }

    let normalized_order =
        source_items.iter().map(|item| item.canonical_path.clone()).collect::<Vec<_>>();
    let mut normalized_membership = normalized_order.clone();
    normalized_membership.sort();
    let target_contract_digest = sha256_json(&entry.contract)?;
    let raw_discovery_digest = sha256_bytes(raw_discovery);
    let mut limitations = vec![
        INVOCATION_LIMITATION.to_string(),
        SCHEDULING_DECLARATION_LIMITATION.to_string(),
        DISCOVERY_DECLARATION_LIMITATION.to_string(),
    ];
    if runner == RunnerKind::DirectFallback {
        limitations.push(DIRECT_FALLBACK_LIMITATION.to_string());
    } else if !runner_matches_authority(runner, canonical_authority.kind) {
        limitations.push(ALTERNATE_RUNNER_LIMITATION.to_string());
    }
    limitations.sort();

    let plan = RunnerPlan {
        schema_version: RUNNER_PLAN_SCHEMA_VERSION.to_string(),
        matrix_fingerprint,
        target_id: target_id.to_string(),
        target_contract_digest,
        runner,
        runner_entrypoint: runner.entrypoint().to_string(),
        canonical_selection_entrypoint: canonical_authority.entrypoint,
        raw_discovery_digest,
        normalization_schema: SOURCE_NORMALIZATION_SCHEMA_VERSION.to_string(),
        discovery_frame,
        source_items,
        normalized_order,
        normalized_membership,
        scheduling,
        invocation_capture: InvocationCaptureStatus::NotProven,
        limitations,
        claim_boundary: "normalized target membership derived from one caller-declared discovery stream and declared runner scheduling inputs only; which upstream runner produced the discovery bytes, observed scheduling state, per-file upstream _scan_test invocation, and compiler/runtime results are not proved".to_string(),
    };
    validate_runner_plan(&plan)?;
    Ok(plan)
}

pub(crate) fn validate_runner_plan(plan: &RunnerPlan) -> Result<(), String> {
    if plan.schema_version != RUNNER_PLAN_SCHEMA_VERSION {
        return Err(format!("unsupported runner plan schema {}", plan.schema_version));
    }
    validate_sha256(&plan.matrix_fingerprint, "matrix fingerprint")?;
    validate_sha256(&plan.target_contract_digest, "target contract digest")?;
    validate_sha256(&plan.raw_discovery_digest, "raw discovery digest")?;
    if plan.normalization_schema != SOURCE_NORMALIZATION_SCHEMA_VERSION {
        return Err(format!(
            "unsupported source normalization schema {}",
            plan.normalization_schema
        ));
    }
    validate_stable_id(&plan.target_id, "target ID")?;
    if plan.runner_entrypoint != plan.runner.entrypoint() {
        return Err(format!(
            "runner plan entrypoint {} disagrees with {:?}",
            plan.runner_entrypoint, plan.runner
        ));
    }
    if plan.canonical_selection_entrypoint.trim().is_empty()
        || plan.claim_boundary.trim().is_empty()
    {
        return Err("runner plan contains incomplete identity or claim boundary".to_string());
    }
    if plan.source_items.is_empty() {
        return Err("runner plan contains no source items".to_string());
    }
    if plan.scheduling.jobs == Some(0) {
        return Err("runner plan jobs must be positive when present".to_string());
    }
    for (key, value) in &plan.scheduling.properties {
        if key.trim().is_empty() || value.trim().is_empty() {
            return Err(
                "runner plan scheduling properties require nonempty keys and values".to_string()
            );
        }
    }

    let mut seen = BTreeSet::new();
    for item in &plan.source_items {
        let normalized = normalize_source_item(&item.raw_path, item.discovery_frame)?;
        if normalized != *item {
            return Err(format!(
                "runner source item {} disagrees with normalized raw path",
                item.canonical_path
            ));
        }
        if !seen.insert(item.canonical_path.clone()) {
            return Err(format!(
                "runner plan contains duplicate source item {}",
                item.canonical_path
            ));
        }
    }
    let expected_order =
        plan.source_items.iter().map(|item| item.canonical_path.clone()).collect::<Vec<_>>();
    if expected_order != plan.normalized_order {
        return Err("runner plan normalized order disagrees with source items".to_string());
    }
    let mut expected_membership = expected_order.clone();
    expected_membership.sort();
    if expected_membership != plan.normalized_membership {
        return Err(
            "runner plan normalized membership is not sorted unique order projection".to_string()
        );
    }
    if plan.normalized_membership.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(
            "runner plan normalized membership must be strictly sorted and unique".to_string()
        );
    }
    if plan.limitations.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("runner plan limitations must be strictly sorted and unique".to_string());
    }
    if !has_limitation(&plan.limitations, INVOCATION_LIMITATION) {
        return Err("runner plan must retain its per-file invocation limitation".to_string());
    }
    if !has_limitation(&plan.limitations, SCHEDULING_DECLARATION_LIMITATION) {
        return Err("runner plan must classify scheduling inputs as declared rather than observed"
            .to_string());
    }
    if !has_limitation(&plan.limitations, DISCOVERY_DECLARATION_LIMITATION) {
        return Err(
            "runner plan must classify its raw discovery stream as a declared input rather than \
             observed runner output"
                .to_string(),
        );
    }
    match plan.runner {
        RunnerKind::DirectFallback => {
            if !has_limitation(&plan.limitations, DIRECT_FALLBACK_LIMITATION) {
                return Err(
                    "direct fallback plan is missing its upstream-context limitation".to_string()
                );
            }
        }
        RunnerKind::Test | RunnerKind::Harness => {
            if has_limitation(&plan.limitations, DIRECT_FALLBACK_LIMITATION) {
                return Err(
                    "upstream runner plan cannot retain a direct-fallback limitation".to_string()
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_runner_plan_against(
    matrix: &UpstreamTargetMatrix,
    raw_discovery: &[u8],
    plan: &RunnerPlan,
) -> Result<(), String> {
    validate_runner_plan(plan)?;
    let rebuilt = build_runner_plan_with_frame(
        matrix,
        &plan.target_id,
        plan.runner,
        raw_discovery,
        plan.discovery_frame,
        plan.scheduling.clone(),
    )?;
    if rebuilt != *plan {
        return Err(
            "runner plan does not match the supplied matrix, target contract, raw discovery, and declared scheduling inputs"
                .to_string(),
        );
    }
    Ok(())
}

pub(crate) fn runner_plan_digest(plan: &RunnerPlan) -> Result<String, String> {
    validate_runner_plan(plan)?;
    sha256_json(plan)
}

pub(crate) fn find_target<'a>(
    matrix: &'a UpstreamTargetMatrix,
    target_id: &str,
) -> Result<&'a TargetMatrixEntry, String> {
    matrix
        .targets
        .iter()
        .find(|entry| entry.contract.target_id == target_id)
        .ok_or_else(|| format!("target matrix has no target {target_id}"))
}

pub(crate) fn effective_selection(
    matrix: &UpstreamTargetMatrix,
    entry: &TargetMatrixEntry,
) -> Result<(Vec<crate::model::TargetSelector>, Vec<TargetScriptForm>), String> {
    match entry.contract.target_kind {
        TargetKind::PhysicalSeries | TargetKind::SelectorVariant => {
            Ok((entry.contract.selectors.clone(), entry.contract.script_forms.clone()))
        }
        TargetKind::EnvironmentVariant => {
            let base =
                entry.contract.variant_of.as_deref().ok_or_else(|| {
                    format!("target {} has no base target", entry.contract.target_id)
                })?;
            let base_entry = find_target(matrix, base)?;
            let (selectors, base_forms) = effective_selection(matrix, base_entry)?;
            let forms = if entry.contract.script_forms.is_empty() {
                base_forms
            } else {
                entry.contract.script_forms.clone()
            };
            Ok((selectors, forms))
        }
        TargetKind::PreparationOnly
        | TargetKind::GeneratedComposite
        | TargetKind::InstrumentationOnly => {
            Err(format!("target {} is not a physical runner population", entry.contract.target_id))
        }
    }
}

pub(crate) fn effective_selection_authority(
    matrix: &UpstreamTargetMatrix,
    entry: &TargetMatrixEntry,
) -> Result<TargetAuthority, String> {
    if let Some(authority) = &entry.contract.selection_authority {
        return Ok(authority.clone());
    }
    let base =
        entry.contract.variant_of.as_deref().ok_or_else(|| {
            format!("target {} has no selection authority", entry.contract.target_id)
        })?;
    effective_selection_authority(matrix, find_target(matrix, base)?)
}

fn runner_matches_authority(runner: RunnerKind, authority: TargetAuthorityKind) -> bool {
    matches!(
        (runner, authority),
        (RunnerKind::Test, TargetAuthorityKind::Test)
            | (RunnerKind::Harness, TargetAuthorityKind::Harness)
            | (RunnerKind::DirectFallback, TargetAuthorityKind::Explicit)
    )
}

fn has_limitation(limitations: &[String], expected: &str) -> bool {
    limitations.iter().any(|value| value == expected)
}

fn validate_stable_id(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        Err(format!("{label} must match [a-z0-9_]+: {value}"))
    } else {
        Ok(())
    }
}

fn sha256_json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("serializing runner authority: {error}"))?;
    Ok(sha256_bytes(&bytes))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

// #7725 intake law, restated locally because this file is included verbatim
// by several crate roots (lib, runner-plan binary, integration proof); the
// canonical definition lives in the library root next to `validate_digest`.
fn is_lower_case_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
}

fn is_canonical_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(is_lower_case_hex_byte)
}

fn validate_sha256(value: &str, label: &str) -> Result<(), String> {
    if !is_canonical_sha256_hex(value) {
        Err(format!(
            "{label} must be a 64-character hexadecimal digest ([0-9a-f] lower-case): {value}"
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod digest_intake_case_tests {
    //! #7725: digests entering the runner-plan authority must keep exactly
    //! one canonical serialized spelling: lower-case hexadecimal.

    use super::validate_sha256;

    #[test]
    fn runner_plan_digests_accept_only_canonical_lower_case_hex() {
        assert!(validate_sha256(&"ab".repeat(32), "plan fingerprint").is_ok());
        assert!(validate_sha256(&"AB".repeat(32), "plan fingerprint").is_err());
        assert!(validate_sha256(&"aB".repeat(32), "plan fingerprint").is_err());
        assert!(validate_sha256(&"zz".repeat(32), "plan fingerprint").is_err());
        assert!(validate_sha256(&"ab".repeat(31), "plan fingerprint").is_err());
    }
}
