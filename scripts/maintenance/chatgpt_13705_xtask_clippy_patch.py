#!/usr/bin/env python3
"""Apply the bounded #13705 xtask production-bin Clippy repair."""

from pathlib import Path


def replace_once(text: str, old: str, new: str, *, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def write_changed(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")
    print(f"patched {path}")


planning_path = Path("xtask/src/tasks/gates/planning_types.rs")
planning = planning_path.read_text(encoding="utf-8")
planning = replace_once(
    planning,
    "    CompileRoutePlanInput, ExpansionStatus, GateSelectorInput, LifecycleDisposition,",
    "    ExpansionStatus, GateSelectorInput, LifecycleDisposition,",
    label="production CompileRoutePlanInput import",
)
planning = replace_once(
    planning,
    "    use xtask::ci_route_plan::{Applicability, CiRoutePlanV1, PlannedOutcome, RouteSubjectRef};",
    """    use xtask::ci_route_plan::{
        Applicability, CiRoutePlanV1, CompileRoutePlanInput, PlannedOutcome, RouteSubjectRef,
    };""",
    label="route-plan test import",
)
write_changed(planning_path, planning)


gates_path = Path("xtask/src/tasks/gates.rs")
gates = gates_path.read_text(encoding="utf-8")
gates_test_anchor = """    #[test]
    fn log_reaches_test_execution_scans_env_wrapped_commands() {"""
gates_test = """    #[test]
    fn log_reaches_test_execution_skips_invalid_utf8_before_later_marker()
    -> color_eyre::eyre::Result<()> {
        let tmp = tempdir()?;
        let log_path = tmp.path().join("invalid-utf8-before-marker.log");
        fs::write(&log_path, b"compiler output: \\xff\\nrunning 3 tests\\n")?;

        assert_eq!(log_reaches_test_execution("cargo test -p xtask", &log_path)?, Some(true));
        Ok(())
    }

""" + gates_test_anchor
gates = replace_once(
    gates,
    gates_test_anchor,
    gates_test,
    label="gate-log test insertion anchor",
)
write_changed(gates_path, gates)


ripr_path = Path("xtask/src/tasks/ripr_evidence.rs")
ripr = ripr_path.read_text(encoding="utf-8")
ripr = replace_once(
    ripr,
    """    let normalized = normalize_path_text(raw_path);
    let normalized = normalized.strip_prefix("//?/").unwrap_or(&normalized);
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);""",
    """    let normalized = normalize_path_text(raw_path);
    let normalized = normalized.strip_prefix("//?/").unwrap_or(normalized.as_str());
    let normalized = normalized.strip_prefix("./").unwrap_or(normalized);""",
    label="surface path normalization chain",
)
ripr = replace_once(
    ripr,
    """fn normalize_repo_relative_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    normalized.strip_prefix("./").unwrap_or(&normalized).to_string()
}""",
    """fn normalize_repo_relative_path(path: &str) -> String {
    let normalized = normalize_path_text(path);
    normalized.strip_prefix("./").unwrap_or(normalized.as_str()).to_string()
}""",
    label="repo-relative path normalization",
)
ripr = replace_once(
    ripr,
    """            ".." => {
                if components.pop().is_none() {
                    return None;
                }
            }""",
    """            ".." => {
                components.pop()?;
            }""",
    label="lexical root escape propagation",
)

context_anchor = """#[cfg(test)]
fn pr_evidence_packet("""
context_definition = """#[derive(Clone, Copy)]
struct PrEvidencePacketContext<'a> {
    changed_file_count: usize,
    head_extents: Option<&'a HeadLineExtents>,
    attribution_scope: Option<&'a AttributionScope>,
    production_surface: Option<&'a ProductionSurface>,
}

#[cfg(test)]
fn pr_evidence_packet("""
ripr = replace_once(
    ripr,
    context_anchor,
    context_definition,
    label="packet context insertion anchor",
)

ripr = replace_once(
    ripr,
    """    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        changed_files.len(),
        None,
        None,
        None,
    )""",
    """    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        PrEvidencePacketContext {
            changed_file_count: changed_files.len(),
            head_extents: None,
            attribution_scope: None,
            production_surface: None,
        },
    )""",
    label="test packet caller",
)

ripr = replace_once(
    ripr,
    """    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        changed_files.len(),
        None,
        None,
        production_surface,
    )""",
    """    pr_evidence_packet_with_count(
        options,
        check_value,
        base_sha,
        head_sha,
        suppressions,
        PrEvidencePacketContext {
            changed_file_count: changed_files.len(),
            head_extents: None,
            attribution_scope: None,
            production_surface,
        },
    )""",
    label="surface packet caller",
)

ripr = replace_once(
    ripr,
    """    let packet = pr_evidence_packet_with_count(
        options,
        &check_value,
        &base_sha,
        &head_sha,
        &suppressions,
        changed_file_count,
        Some(&head_extents),
        attribution_scope.as_ref(),
        production_surface.as_ref(),
    );""",
    """    let packet = pr_evidence_packet_with_count(
        options,
        &check_value,
        &base_sha,
        &head_sha,
        &suppressions,
        PrEvidencePacketContext {
            changed_file_count,
            head_extents: Some(&head_extents),
            attribution_scope: attribution_scope.as_ref(),
            production_surface: production_surface.as_ref(),
        },
    );""",
    label="production packet caller",
)

ripr = replace_once(
    ripr,
    """fn pr_evidence_packet_with_count(
    options: &PrEvidenceOptions,
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    changed_file_count: usize,
    head_extents: Option<&HeadLineExtents>,
    attribution_scope: Option<&AttributionScope>,
    production_surface: Option<&ProductionSurface>,
) -> Value {
    let check_summary = check_value.get("summary").and_then(Value::as_object);""",
    """fn pr_evidence_packet_with_count(
    options: &PrEvidenceOptions,
    check_value: &Value,
    base_sha: &str,
    head_sha: &str,
    suppressions: &RiprSuppressionRules,
    context: PrEvidencePacketContext<'_>,
) -> Value {
    let PrEvidencePacketContext {
        changed_file_count,
        head_extents,
        attribution_scope,
        production_surface,
    } = context;
    let check_summary = check_value.get("summary").and_then(Value::as_object);""",
    label="packet helper signature",
)

if ripr.count("pr_evidence_packet_with_count(") != 4:
    raise SystemExit(
        "packet helper call inventory changed: expected definition plus three callers"
    )

write_changed(ripr_path, ripr)
