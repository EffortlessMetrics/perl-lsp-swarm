//! Validate the active Editor Trust goal manifest.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};
use toml::Value;

const ACTIVE_GOAL_PATH: &str = ".perl-lsp/goals/active.toml";
const TOP_LEVEL_STRING_FIELDS: &[&str] = &["proposal", "plan", "previous_goal", "status_pointer"];
const TOP_LEVEL_DOCUMENT_STRING_FIELDS: &[&str] = &["proposal", "plan", "previous_goal"];
const TOP_LEVEL_ARRAY_FIELDS: &[&str] = &["specs", "adrs", "status_docs"];
const TOP_LEVEL_PLAN_DISCOVERY_FIELDS: &[&str] = &["proposal", "previous_goal", "status_pointer"];
const REQUIRED_TOP_LEVEL_FIELDS: &[&str] = &["id", "title", "status", "owner", "created"];
const REQUIRED_TOP_LEVEL_TEXT_FIELDS: &[&str] = &["objective"];
const OPTIONAL_TOP_LEVEL_TEXT_FIELDS: &[&str] = &["current_work_item", "next_action"];
const REQUIRED_TOP_LEVEL_TEXT_ARRAY_FIELDS: &[&str] = &["end_state", "claim_boundaries"];
const TRIMMED_TOP_LEVEL_FIELDS: &[&str] = &["title", "owner"];
const REQUIRED_WORK_ITEM_FIELDS: &[&str] =
    &["id", "status", "spec", "plan", "current_pointer", "claim_boundary"];
const TRIMMED_WORK_ITEM_TEXT_FIELDS: &[&str] =
    &["claim_boundary", "current_status", "trigger", "blocked_by"];
const ALLOWED_WORK_ITEM_STATUSES: &[&str] =
    &["active", "ready", "planned", "completed", "blocked", "deferred"];
const ACTIONABLE_WORK_ITEM_STATUSES: &[&str] = &["active", "ready"];
const CLOSED_WORK_ITEM_STATUSES: &[&str] = &["completed", "deferred"];
const REQUIRED_PROOF_COMMAND_PREFIX: &str = "rtk ";
const REQUIRED_PLAN_SECTION_HEADINGS: &[&str] =
    &["Claim boundary", "Non-goals", "Acceptance", "Proof commands", "Rollback"];
const SYMBOL_REFERENCE_MARKERS: &[&str] =
    &[".rs::", ".py::", ".ts::", ".tsx::", ".js::", ".md::", ".toml::"];

#[derive(Debug, Default)]
struct ValidationStats {
    work_items: usize,
    open_work_items: usize,
    actionable_work_items: usize,
    path_references: usize,
    proof_commands: usize,
    current_work_item: Option<String>,
    current_work_item_plan: Option<String>,
    current_work_item_pointer: Option<String>,
    current_work_item_status: Option<String>,
    current_work_item_claim_boundary: Option<String>,
    current_work_item_commands: Vec<String>,
}

#[derive(Debug)]
struct Reference {
    path: String,
    markdown_anchor: Option<String>,
    symbol: Option<String>,
}

#[derive(Debug, Default)]
struct WorkItemRefs {
    actionable_ids: BTreeSet<String>,
    active_ids: BTreeSet<String>,
    handoffs: BTreeMap<String, WorkItemHandoff>,
}

#[derive(Debug)]
struct WorkItemHandoff {
    plan: String,
    current_pointer: String,
    current_status: Option<String>,
    claim_boundary: String,
    commands: Vec<String>,
}

pub fn run() -> Result<()> {
    let root = project_root()?;
    let stats = validate(&root)?;
    println!(
        "active goal manifest check passed: {} work items ({} open, {} actionable, current: {}), {} path references, {} proof commands",
        stats.work_items,
        stats.open_work_items,
        stats.actionable_work_items,
        stats.current_work_item.as_deref().unwrap_or("none"),
        stats.path_references,
        stats.proof_commands
    );
    if let Some(plan) = &stats.current_work_item_plan {
        println!("current work item plan: {plan}");
    }
    if let Some(pointer) = &stats.current_work_item_pointer {
        println!("current work item pointer: {pointer}");
    }
    if let Some(status) = &stats.current_work_item_status {
        println!("current work item status: {status}");
    }
    if let Some(claim_boundary) = &stats.current_work_item_claim_boundary {
        println!("current work item claim boundary: {claim_boundary}");
    }
    if !stats.current_work_item_commands.is_empty() {
        println!("current work item proof commands:");
        for command in &stats.current_work_item_commands {
            println!("  - {command}");
        }
    }
    Ok(())
}

fn validate(root: &Path) -> Result<ValidationStats> {
    let mut stats = ValidationStats::default();
    let violations = collect_violations(root, &mut stats)?;
    if !violations.is_empty() {
        eprintln!("active goal manifest violations:");
        for violation in &violations {
            eprintln!("  - {violation}");
        }
        bail!("active goal manifest check failed with {} violation(s)", violations.len());
    }
    Ok(stats)
}

fn collect_violations(root: &Path, stats: &mut ValidationStats) -> Result<Vec<String>> {
    let manifest_text = read_text(root, ACTIVE_GOAL_PATH)?;
    let manifest: Value = toml::from_str(&manifest_text)
        .with_context(|| format!("failed to parse {ACTIVE_GOAL_PATH}"))?;
    let Some(table) = manifest.as_table() else {
        bail!("{ACTIVE_GOAL_PATH}: expected TOML table");
    };

    let mut violations = Vec::new();

    for field in REQUIRED_TOP_LEVEL_FIELDS {
        require_non_empty_string(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    for field in TRIMMED_TOP_LEVEL_FIELDS {
        validate_trimmed_string_field(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    validate_stable_id_field(ACTIVE_GOAL_PATH, table, "id", &mut violations);
    validate_created_date_format(table, &mut violations);
    for field in REQUIRED_TOP_LEVEL_TEXT_FIELDS {
        require_non_empty_string(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    for field in OPTIONAL_TOP_LEVEL_TEXT_FIELDS {
        validate_optional_text_field(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    for field in REQUIRED_TOP_LEVEL_TEXT_ARRAY_FIELDS {
        validate_text_array_field(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    if let Some(status) = string_field(table, "status")
        && status != "active"
    {
        violations.push(format!("{ACTIVE_GOAL_PATH}: status is {status:?}; expected \"active\""));
    }

    for field in TOP_LEVEL_STRING_FIELDS {
        validate_path_field(root, ACTIVE_GOAL_PATH, table, field, stats, &mut violations);
    }
    for field in TOP_LEVEL_DOCUMENT_STRING_FIELDS {
        validate_top_level_document_path_field(ACTIVE_GOAL_PATH, table, field, &mut violations);
    }
    for field in TOP_LEVEL_ARRAY_FIELDS {
        validate_path_array_field(root, ACTIVE_GOAL_PATH, table, field, stats, &mut violations);
    }
    let top_level_specs = collect_string_set(table, "specs");
    let top_level_adrs = collect_string_set(table, "adrs");
    let top_level_status_docs = collect_string_set(table, "status_docs");
    let top_level_end_state = collect_string_set(table, "end_state");
    let top_level_claim_boundaries = collect_string_set(table, "claim_boundaries");
    validate_status_pointer_membership(table, &top_level_status_docs, &mut violations);
    validate_top_level_contracts_in_plan(
        root,
        table,
        &top_level_specs,
        &top_level_adrs,
        &top_level_status_docs,
        &top_level_end_state,
        &top_level_claim_boundaries,
        &mut violations,
    );

    let active_plan = string_field(table, "plan");
    let work_items = validate_work_items(
        root,
        table,
        &top_level_specs,
        &top_level_status_docs,
        active_plan,
        stats,
        &mut violations,
    );
    validate_goal_status_has_open_work(table, stats, &mut violations);
    validate_goal_status_has_actionable_handoff(table, stats, &mut violations);
    validate_current_work_item(table, &work_items, stats, &mut violations);

    Ok(violations)
}

fn validate_work_items(
    root: &Path,
    table: &toml::Table,
    top_level_specs: &BTreeSet<String>,
    top_level_status_docs: &BTreeSet<String>,
    active_plan: Option<&str>,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) -> WorkItemRefs {
    let mut refs = WorkItemRefs::default();
    let Some(items) = table.get("work_item").and_then(Value::as_array) else {
        violations.push(format!("{ACTIVE_GOAL_PATH}: work_item must not be empty"));
        return refs;
    };
    if items.is_empty() {
        violations.push(format!("{ACTIVE_GOAL_PATH}: work_item must not be empty"));
        return refs;
    }

    let mut seen_ids = BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        let doc = format!("{ACTIVE_GOAL_PATH}: work_item[{index}]");
        let Some(item_table) = item.as_table() else {
            violations.push(format!("{doc} must be a TOML table"));
            continue;
        };
        stats.work_items += 1;

        for field in REQUIRED_WORK_ITEM_FIELDS {
            require_non_empty_string(&doc, item_table, field, violations);
        }
        for field in TRIMMED_WORK_ITEM_TEXT_FIELDS {
            validate_trimmed_string_field(&doc, item_table, field, violations);
        }
        record_work_item_status(item_table, stats);
        validate_work_item_status(&doc, item_table, violations);
        validate_work_item_status_context(&doc, item_table, violations);
        validate_work_item_spec_membership(&doc, item_table, top_level_specs, violations);
        validate_work_item_status_doc_pointer_membership(
            &doc,
            item_table,
            top_level_status_docs,
            violations,
        );
        validate_work_item_plan_membership(&doc, item_table, active_plan, violations);
        validate_work_item_plan_anchor(&doc, item_table, violations);
        validate_work_item_plan_section(root, &doc, item_table, violations);
        if let Some(id) = string_field(item_table, "id") {
            validate_stable_id_value(&doc, "id", id, violations);
            if !seen_ids.insert(id.to_string()) {
                violations.push(format!("{ACTIVE_GOAL_PATH}: duplicate work_item id {id:?}"));
            }
            if let Some(handoff) = collect_work_item_handoff(item_table) {
                refs.handoffs.insert(id.to_string(), handoff);
            }
            if string_field(item_table, "status")
                .is_some_and(|status| ACTIONABLE_WORK_ITEM_STATUSES.contains(&status))
            {
                refs.actionable_ids.insert(id.to_string());
            }
            if string_field(item_table, "status") == Some("active") {
                refs.active_ids.insert(id.to_string());
            }
        }
        validate_commands_field(&doc, item_table, stats, violations);

        for (field, value) in item_table {
            if is_work_item_path_field(field) {
                validate_path_value(root, &doc, field, value, stats, violations);
                validate_work_item_document_pointer_field(&doc, field, value, violations);
            }
        }
    }
    if refs.active_ids.len() > 1 {
        violations
            .push(format!("{ACTIVE_GOAL_PATH}: at most one work_item may have status \"active\""));
    }
    refs
}

fn validate_created_date_format(table: &toml::Table, violations: &mut Vec<String>) {
    let Some(created) = string_field(table, "created") else {
        return;
    };
    if !is_yyyy_mm_dd(created) {
        violations.push(format!("{ACTIVE_GOAL_PATH}: field created must use YYYY-MM-DD format"));
    }
}

fn is_yyyy_mm_dd(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let Some(year) = digits_to_u16(&bytes[..4]) else {
        return false;
    };
    let Some(month) = digits_to_u16(&bytes[5..7]) else {
        return false;
    };
    let Some(day) = digits_to_u16(&bytes[8..]) else {
        return false;
    };
    year > 0
        && month > 0
        && day > 0
        && month <= 12
        && day <= days_in_month(year, month).unwrap_or(0)
}

fn digits_to_u16(bytes: &[u8]) -> Option<u16> {
    let mut value = 0_u16;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.saturating_mul(10).saturating_add(u16::from(byte - b'0'));
    }
    Some(value)
}

fn days_in_month(year: u16, month: u16) -> Option<u16> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

fn is_leap_year(year: u16) -> bool {
    let year = u32::from(year);
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn validate_status_pointer_membership(
    table: &toml::Table,
    status_docs: &BTreeSet<String>,
    violations: &mut Vec<String>,
) {
    let Some(status_pointer) = string_field(table, "status_pointer") else {
        return;
    };
    let status_pointer_path = parse_reference(status_pointer.trim()).path;
    if !status_docs.contains(&status_pointer_path) {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: status_pointer path {status_pointer_path:?} is not listed in status_docs"
        ));
    }
}

fn validate_top_level_contracts_in_plan(
    root: &Path,
    table: &toml::Table,
    top_level_specs: &BTreeSet<String>,
    top_level_adrs: &BTreeSet<String>,
    top_level_status_docs: &BTreeSet<String>,
    top_level_end_state: &BTreeSet<String>,
    top_level_claim_boundaries: &BTreeSet<String>,
    violations: &mut Vec<String>,
) {
    let Some(plan) = string_field(table, "plan") else {
        return;
    };
    let reference = parse_reference(plan.trim());
    let rel_path = Path::new(&reference.path);
    if !is_repo_relative_reference(rel_path) {
        return;
    }
    let Ok(text) = fs::read_to_string(root.join(rel_path)) else {
        return;
    };

    if let Some(objective) = string_field(table, "objective") {
        let trimmed = objective.trim();
        if !contains_normalized_text(&text, trimmed) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level objective is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for field in TOP_LEVEL_PLAN_DISCOVERY_FIELDS {
        let Some(reference) = string_field(table, field) else {
            continue;
        };
        if !text.contains(reference) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level {field} reference {reference:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for spec in top_level_specs {
        if !text.contains(spec) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level spec {spec:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for adr in top_level_adrs {
        if !text.contains(adr) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level ADR {adr:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for status_doc in top_level_status_docs {
        if !text.contains(status_doc) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level status doc {status_doc:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for end_state in top_level_end_state {
        if !contains_normalized_text(&text, end_state) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level end_state {end_state:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
    for claim_boundary in top_level_claim_boundaries {
        if !contains_normalized_text(&text, claim_boundary) {
            violations.push(format!(
                "{ACTIVE_GOAL_PATH}: top-level claim boundary {claim_boundary:?} is not mentioned in top-level plan {plan:?}"
            ));
        }
    }
}

fn validate_stable_id_field(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    let Some(value) = string_field(table, field) else {
        return;
    };
    validate_stable_id_value(doc, field, value, violations);
}

fn validate_stable_id_value(doc: &str, field: &str, value: &str, violations: &mut Vec<String>) {
    if !is_stable_slug_id(value) {
        violations.push(format!(
            "{doc}: field {field} must be a stable id using lowercase ASCII letters, digits, and single hyphens: {value:?}"
        ));
    }
}

fn is_stable_slug_id(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }

    let mut previous_was_hyphen = false;
    for ch in chars {
        if ch == '-' {
            if previous_was_hyphen {
                return false;
            }
            previous_was_hyphen = true;
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            previous_was_hyphen = false;
        } else {
            return false;
        }
    }
    !previous_was_hyphen
}

fn validate_work_item_plan_anchor(doc: &str, table: &toml::Table, violations: &mut Vec<String>) {
    let (Some(id), Some(plan)) = (string_field(table, "id"), string_field(table, "plan")) else {
        return;
    };
    let Some((_, anchor)) = plan.split_once('#') else {
        violations.push(format!("{doc}: plan must include a markdown anchor for work item {id:?}"));
        return;
    };
    let expected = format!("work-item-{id}");
    if anchor != expected {
        violations.push(format!(
            "{doc}: plan anchor is {anchor:?}; expected {expected:?} to match work item id"
        ));
    }
}

fn validate_work_item_plan_membership(
    doc: &str,
    table: &toml::Table,
    active_plan: Option<&str>,
    violations: &mut Vec<String>,
) {
    let (Some(active_plan), Some(plan)) = (active_plan, string_field(table, "plan")) else {
        return;
    };
    let active_plan_path = parse_reference(active_plan.trim()).path;
    let work_item_plan_path = parse_reference(plan.trim()).path;
    if work_item_plan_path != active_plan_path {
        violations.push(format!(
            "{doc}: plan path {work_item_plan_path:?} must match top-level plan {active_plan_path:?}"
        ));
    }
}

fn validate_work_item_plan_section(
    root: &Path,
    doc: &str,
    table: &toml::Table,
    violations: &mut Vec<String>,
) {
    let Some(plan) = string_field(table, "plan") else {
        return;
    };
    let Some(section) = work_item_plan_section(root, plan.trim()) else {
        return;
    };

    validate_work_item_plan_headings(doc, plan, &section, violations);
    validate_work_item_plan_claim_boundary(doc, table, plan, &section, violations);
    validate_work_item_plan_current_pointer(doc, table, plan, &section, violations);
    validate_work_item_plan_current_status(doc, table, plan, &section, violations);
    validate_work_item_plan_routing_context(doc, table, plan, &section, violations);
    validate_work_item_plan_receipts(doc, table, plan, &section, violations);
    validate_work_item_plan_spec(doc, table, plan, &section, violations);
    validate_work_item_plan_status(doc, table, plan, &section, violations);
    validate_work_item_plan_commands(doc, table, plan, &section, violations);
}

fn work_item_plan_section(root: &Path, plan: &str) -> Option<String> {
    let reference = parse_reference(plan.trim());
    let anchor = reference.markdown_anchor.as_deref()?;
    let rel_path = Path::new(&reference.path);
    if !is_repo_relative_reference(rel_path) {
        return None;
    }
    let full_path = root.join(rel_path);
    let text = fs::read_to_string(&full_path).ok()?;
    markdown_section_by_anchor(&text, anchor).map(ToOwned::to_owned)
}

fn validate_work_item_plan_status(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(status) = string_field(table, "status") else {
        return;
    };
    let expected = format!("Status: {status}");
    if !section.lines().any(|line| line.trim() == expected) {
        violations.push(format!("{doc}: linked plan section {plan} must contain `{expected}`"));
    }
}

fn validate_work_item_plan_headings(
    doc: &str,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    for heading in REQUIRED_PLAN_SECTION_HEADINGS {
        if !section.lines().any(|line| line.trim() == *heading) {
            violations.push(format!("{doc}: linked plan section {plan} must contain `{heading}`"));
        }
    }
}

fn validate_work_item_plan_claim_boundary(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(claim_boundary) = string_field(table, "claim_boundary") else {
        return;
    };
    if !contains_normalized_text(section, claim_boundary) {
        violations.push(format!(
            "{doc}: linked plan section {plan} must include claim_boundary {claim_boundary:?}"
        ));
    }
}

fn validate_work_item_plan_current_pointer(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(current_pointer) = string_field(table, "current_pointer") else {
        return;
    };
    if !section.contains(current_pointer) {
        violations.push(format!(
            "{doc}: linked plan section {plan} must include current_pointer {current_pointer:?}"
        ));
    }
}

fn validate_work_item_plan_current_status(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(current_status) = string_field(table, "current_status") else {
        return;
    };
    if !contains_normalized_text(section, current_status) {
        violations.push(format!(
            "{doc}: linked plan section {plan} must include current_status {current_status:?}"
        ));
    }
}

fn validate_work_item_plan_routing_context(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    for field in ["trigger", "blocked_by"] {
        let Some(value) = string_field(table, field) else {
            continue;
        };
        let trimmed = value.trim();
        if !trimmed.is_empty() && !contains_normalized_text(section, trimmed) {
            violations.push(format!(
                "{doc}: linked plan section {plan} must include {field} {trimmed:?}"
            ));
        }
    }
}

fn validate_work_item_plan_receipts(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    for (field, value) in table {
        if !is_receipt_path_field(field) {
            continue;
        }
        let Some(receipt) = value.as_str() else {
            continue;
        };
        let trimmed = receipt.trim();
        if !trimmed.is_empty() && !section.contains(trimmed) {
            violations.push(format!(
                "{doc}: linked plan section {plan} must include {field} receipt {trimmed:?}"
            ));
        }
    }
}

fn validate_work_item_plan_spec(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(spec) = string_field(table, "spec") else {
        return;
    };
    if !section.contains(spec) {
        violations.push(format!("{doc}: linked plan section {plan} must mention spec {spec:?}"));
    }
}

fn validate_work_item_plan_commands(
    doc: &str,
    table: &toml::Table,
    plan: &str,
    section: &str,
    violations: &mut Vec<String>,
) {
    let Some(commands) = table.get("commands").and_then(Value::as_array) else {
        return;
    };
    for command in
        commands.iter().filter_map(Value::as_str).filter(|command| !command.trim().is_empty())
    {
        if !section.contains(command) {
            violations.push(format!(
                "{doc}: proof command is missing from linked plan section {plan}: {command}"
            ));
        }
    }
}

fn validate_goal_status_has_open_work(
    table: &toml::Table,
    stats: &ValidationStats,
    violations: &mut Vec<String>,
) {
    if string_field(table, "status") == Some("active") && stats.open_work_items == 0 {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: active goal must contain at least one non-completed work_item"
        ));
    }
}

fn validate_goal_status_has_actionable_handoff(
    table: &toml::Table,
    stats: &ValidationStats,
    violations: &mut Vec<String>,
) {
    if string_field(table, "status") == Some("active")
        && stats.open_work_items > 0
        && stats.actionable_work_items == 0
        && !has_non_empty_string_field(table, "next_action")
    {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: active goal with no actionable work_item must include next_action"
        ));
    }
}

fn validate_current_work_item(
    table: &toml::Table,
    work_items: &WorkItemRefs,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) {
    let current_work_item = string_field(table, "current_work_item");
    if string_field(table, "status") == Some("active")
        && stats.actionable_work_items > 0
        && current_work_item.is_none()
    {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: active goal with actionable work_item entries must include current_work_item"
        ));
        return;
    }

    let Some(current_work_item) = current_work_item else {
        return;
    };
    if current_work_item.trim().is_empty() {
        return;
    }
    if !is_stable_slug_id(current_work_item) {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: current_work_item must be a stable id using lowercase ASCII letters, digits, and single hyphens: {current_work_item:?}"
        ));
        return;
    }
    stats.current_work_item = Some(current_work_item.to_string());
    if !work_items.actionable_ids.contains(current_work_item) {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: current_work_item {current_work_item:?} must reference an active or ready work_item id"
        ));
        return;
    }
    if !work_items.active_ids.is_empty() && !work_items.active_ids.contains(current_work_item) {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: current_work_item {current_work_item:?} must reference the active work_item when any work_item has status \"active\""
        ));
        return;
    }
    if let Some(next_action) = string_field(table, "next_action")
        && !next_action.contains(current_work_item)
    {
        violations.push(format!(
            "{ACTIVE_GOAL_PATH}: next_action must mention current_work_item {current_work_item:?}"
        ));
    }
    if let Some(handoff) = work_items.handoffs.get(current_work_item) {
        stats.current_work_item_plan = Some(handoff.plan.clone());
        stats.current_work_item_pointer = Some(handoff.current_pointer.clone());
        stats.current_work_item_status.clone_from(&handoff.current_status);
        stats.current_work_item_claim_boundary = Some(handoff.claim_boundary.clone());
        stats.current_work_item_commands = handoff.commands.clone();
    }
}

fn collect_work_item_handoff(table: &toml::Table) -> Option<WorkItemHandoff> {
    Some(WorkItemHandoff {
        plan: string_field(table, "plan")?.to_string(),
        current_pointer: string_field(table, "current_pointer")?.to_string(),
        current_status: string_field(table, "current_status").map(ToOwned::to_owned),
        claim_boundary: string_field(table, "claim_boundary")?.to_string(),
        commands: collect_string_values(table, "commands"),
    })
}

fn collect_string_values(table: &toml::Table, field: &str) -> Vec<String> {
    table
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn record_work_item_status(table: &toml::Table, stats: &mut ValidationStats) {
    if let Some(status) = string_field(table, "status")
        && !CLOSED_WORK_ITEM_STATUSES.contains(&status)
    {
        stats.open_work_items += 1;
        if ACTIONABLE_WORK_ITEM_STATUSES.contains(&status) {
            stats.actionable_work_items += 1;
        }
    }
}

fn validate_work_item_status(doc: &str, table: &toml::Table, violations: &mut Vec<String>) {
    let Some(status) = string_field(table, "status") else {
        return;
    };
    if !ALLOWED_WORK_ITEM_STATUSES.contains(&status) {
        violations.push(format!("{doc}: status {status:?} is not an allowed work item status"));
    }
}

fn validate_work_item_status_context(doc: &str, table: &toml::Table, violations: &mut Vec<String>) {
    let Some(status) = string_field(table, "status") else {
        return;
    };
    match status {
        "active" | "ready" if !has_non_empty_string_field(table, "current_status") => {
            violations.push(format!("{doc}: actionable work item must include current_status"));
        }
        "planned"
            if !has_non_empty_string_field(table, "trigger")
                && !has_non_empty_string_field(table, "current_status") =>
        {
            violations
                .push(format!("{doc}: planned work item must include trigger or current_status"));
        }
        "blocked" if !has_non_empty_string_field(table, "blocked_by") => {
            violations.push(format!("{doc}: blocked work item must include blocked_by"));
        }
        "completed" if !has_receipt_path_field(table) => {
            violations
                .push(format!("{doc}: completed work item must include receipt or *_receipt"));
        }
        _ => {}
    }
}

fn validate_work_item_spec_membership(
    doc: &str,
    table: &toml::Table,
    top_level_specs: &BTreeSet<String>,
    violations: &mut Vec<String>,
) {
    let Some(spec) = string_field(table, "spec") else {
        return;
    };
    if !top_level_specs.contains(spec) {
        violations.push(format!("{doc}: spec {spec:?} is not listed in top-level specs"));
    }
}

fn validate_work_item_status_doc_pointer_membership(
    doc: &str,
    table: &toml::Table,
    top_level_status_docs: &BTreeSet<String>,
    violations: &mut Vec<String>,
) {
    let Some(pointer) = string_field(table, "current_pointer") else {
        return;
    };
    let pointer_path = parse_reference(pointer.trim()).path;
    if pointer_path.starts_with("docs/project/status/")
        && !top_level_status_docs.contains(&pointer_path)
    {
        violations.push(format!(
            "{doc}: status current_pointer {pointer_path:?} is not listed in top-level status_docs"
        ));
    }
}

fn validate_commands_field(
    doc: &str,
    table: &toml::Table,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) {
    let Some(values) = table.get("commands").and_then(Value::as_array) else {
        violations.push(format!("{doc}: field commands must be a non-empty string array"));
        return;
    };
    if values.is_empty() {
        violations.push(format!("{doc}: field commands must be a non-empty string array"));
        return;
    }

    let mut seen_commands = BTreeSet::new();
    for value in values {
        match value.as_str() {
            Some(command) if !command.trim().is_empty() => {
                stats.proof_commands += 1;
                let trimmed = command.trim();
                if command != trimmed {
                    violations.push(format!(
                        "{doc}: proof command must not include leading or trailing whitespace: {command:?}"
                    ));
                }
                if !seen_commands.insert(trimmed.to_string()) {
                    violations.push(format!("{doc}: duplicate proof command {trimmed:?}"));
                }
                if !command.starts_with(REQUIRED_PROOF_COMMAND_PREFIX) {
                    violations.push(format!(
                        "{doc}: proof command must start with {REQUIRED_PROOF_COMMAND_PREFIX:?}: {command}"
                    ));
                }
            }
            Some(_) => violations.push(format!("{doc}: field commands contains an empty command")),
            None => violations.push(format!("{doc}: field commands must contain only strings")),
        }
    }
}

fn is_work_item_path_field(field: &str) -> bool {
    matches!(field, "spec" | "plan" | "current_pointer") || is_receipt_path_field(field)
}

fn is_receipt_path_field(field: &str) -> bool {
    field == "receipt" || field.ends_with("_receipt")
}

fn has_receipt_path_field(table: &toml::Table) -> bool {
    table.iter().any(|(field, value)| {
        is_receipt_path_field(field) && value.as_str().is_some_and(|raw| !raw.trim().is_empty())
    })
}

fn validate_path_field(
    root: &Path,
    doc: &str,
    table: &toml::Table,
    field: &str,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) {
    let Some(value) = table.get(field) else {
        violations.push(format!("{doc}: field {field} must not be empty"));
        return;
    };
    validate_path_value(root, doc, field, value, stats, violations);
}

fn validate_path_array_field(
    root: &Path,
    doc: &str,
    table: &toml::Table,
    field: &str,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) {
    let Some(values) = table.get(field).and_then(Value::as_array) else {
        violations.push(format!("{doc}: field {field} must be a non-empty string array"));
        return;
    };
    if values.is_empty() {
        violations.push(format!("{doc}: field {field} must be a non-empty string array"));
        return;
    }
    let mut seen_values = BTreeSet::new();
    for value in values {
        if let Some(raw) = value.as_str()
            && !seen_values.insert(raw.to_string())
        {
            violations.push(format!("{doc}: field {field} contains duplicate path {raw:?}"));
        }
        if let Some(raw) = value.as_str() {
            validate_path_inventory_entry(doc, field, raw, violations);
        }
        validate_path_value(root, doc, field, value, stats, violations);
    }
}

fn validate_path_inventory_entry(doc: &str, field: &str, raw: &str, violations: &mut Vec<String>) {
    let reference = parse_reference(raw.trim());
    if reference.markdown_anchor.is_some() || reference.symbol.is_some() {
        violations.push(format!(
            "{doc}: field {field} is a document inventory and must not include anchors or symbols: {raw}"
        ));
    }
}

fn validate_top_level_document_path_field(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    let Some(raw) = string_field(table, field) else {
        return;
    };
    let reference = parse_reference(raw.trim());
    if reference.markdown_anchor.is_some() || reference.symbol.is_some() {
        violations.push(format!(
            "{doc}: field {field} is a document path and must not include anchors or symbols: {raw}"
        ));
    }
}

fn validate_work_item_document_pointer_field(
    doc: &str,
    field: &str,
    value: &Value,
    violations: &mut Vec<String>,
) {
    if is_receipt_path_field(field) {
        return;
    }
    let Some(raw) = value.as_str() else {
        return;
    };
    let reference = parse_reference(raw.trim());
    if field == "spec" && (reference.markdown_anchor.is_some() || reference.symbol.is_some()) {
        violations.push(format!(
            "{doc}: field spec is a document path and must not include anchors or symbols: {raw}"
        ));
        return;
    }
    if reference.symbol.is_some() {
        violations.push(format!(
            "{doc}: field {field} is a document pointer and must not include path::symbol references; use receipt fields for symbol anchors: {raw}"
        ));
    }
}

fn validate_text_array_field(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    let Some(values) = table.get(field).and_then(Value::as_array) else {
        violations.push(format!("{doc}: field {field} must be a non-empty string array"));
        return;
    };
    if values.is_empty() {
        violations.push(format!("{doc}: field {field} must be a non-empty string array"));
        return;
    }

    let mut seen_values = BTreeSet::new();
    for value in values {
        match value.as_str() {
            Some(text) if !text.trim().is_empty() => {
                let trimmed = text.trim();
                if text != trimmed {
                    violations.push(format!(
                        "{doc}: field {field} entry must not include leading or trailing whitespace: {text:?}"
                    ));
                }
                if !seen_values.insert(trimmed.to_string()) {
                    violations
                        .push(format!("{doc}: field {field} contains duplicate entry {trimmed:?}"));
                }
            }
            Some(_) => violations.push(format!("{doc}: field {field} contains an empty entry")),
            None => violations.push(format!("{doc}: field {field} must contain only strings")),
        }
    }
}

fn validate_optional_text_field(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    let Some(value) = table.get(field) else {
        return;
    };
    match value.as_str() {
        Some(text) if !text.trim().is_empty() => {}
        Some(_) => violations.push(format!("{doc}: field {field} must not be empty when present")),
        None => violations.push(format!("{doc}: field {field} must be a string when present")),
    }
}

fn validate_trimmed_string_field(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    let Some(value) = string_field(table, field) else {
        return;
    };
    let trimmed = value.trim();
    if !trimmed.is_empty() && value != trimmed {
        violations.push(format!(
            "{doc}: field {field} must not include leading or trailing whitespace: {value:?}"
        ));
    }
}

fn collect_string_set(table: &toml::Table, field: &str) -> BTreeSet<String> {
    table
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn validate_path_value(
    root: &Path,
    doc: &str,
    field: &str,
    value: &Value,
    stats: &mut ValidationStats,
    violations: &mut Vec<String>,
) {
    let Some(raw) = value.as_str() else {
        violations.push(format!("{doc}: field {field} must contain string path references"));
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        violations.push(format!("{doc}: field {field} contains an empty path reference"));
        return;
    }
    if raw != trimmed {
        violations.push(format!(
            "{doc}: field {field} path reference must not include leading or trailing whitespace: {raw:?}"
        ));
    }

    stats.path_references += 1;
    validate_reference(root, doc, field, trimmed, violations);
}

fn validate_reference(
    root: &Path,
    doc: &str,
    field: &str,
    raw: &str,
    violations: &mut Vec<String>,
) {
    let reference = parse_reference(raw);
    let rel_path = Path::new(&reference.path);
    if !is_repo_relative_reference(rel_path) {
        violations.push(format!("{doc}: field {field} must use repo-relative path: {raw}"));
        return;
    }

    let full_path = root.join(rel_path);
    if !full_path.exists() {
        violations.push(format!("{doc}: field {field} path does not exist: {}", reference.path));
        return;
    }

    if let Some(anchor) = &reference.markdown_anchor {
        validate_markdown_anchor(&full_path, doc, field, raw, anchor, violations);
    }
    if let Some(symbol) = &reference.symbol {
        validate_symbol_anchor(&full_path, doc, field, raw, symbol, violations);
    }
}

fn is_repo_relative_reference(path: &Path) -> bool {
    !path.is_absolute()
        && path.components().all(|component| {
            !matches!(component, Component::ParentDir | Component::Prefix(_) | Component::RootDir)
        })
}

fn parse_reference(raw: &str) -> Reference {
    let (without_anchor, markdown_anchor) = match raw.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor.to_string())),
        None => (raw, None),
    };
    let (path, symbol) = split_symbol_reference(without_anchor);
    Reference { path: path.to_string(), markdown_anchor, symbol: symbol.map(str::to_string) }
}

fn split_symbol_reference(raw: &str) -> (&str, Option<&str>) {
    for marker in SYMBOL_REFERENCE_MARKERS {
        if let Some(index) = raw.find(marker) {
            let extension = marker.trim_end_matches("::");
            let path_end = index + extension.len();
            let symbol_start = path_end + 2;
            return (&raw[..path_end], raw.get(symbol_start..));
        }
    }
    (raw, None)
}

fn validate_markdown_anchor(
    full_path: &Path,
    doc: &str,
    field: &str,
    raw: &str,
    anchor: &str,
    violations: &mut Vec<String>,
) {
    if anchor.trim().is_empty() {
        violations.push(format!("{doc}: field {field} has empty markdown anchor: {raw}"));
        return;
    }
    if full_path.extension().and_then(|extension| extension.to_str()) != Some("md") {
        return;
    }

    match fs::read_to_string(full_path) {
        Ok(text) => {
            let anchors = markdown_heading_anchors(&text);
            if !anchors.contains(anchor) {
                violations
                    .push(format!("{doc}: field {field} markdown anchor does not exist: {raw}"));
            }
        }
        Err(err) => violations.push(format!(
            "{doc}: field {field} failed to read markdown anchor target {}: {err}",
            full_path.display()
        )),
    }
}

fn validate_symbol_anchor(
    full_path: &Path,
    doc: &str,
    field: &str,
    raw: &str,
    symbol: &str,
    violations: &mut Vec<String>,
) {
    if symbol.trim().is_empty() {
        violations.push(format!("{doc}: field {field} has empty symbol anchor: {raw}"));
        return;
    }

    match fs::read_to_string(full_path) {
        Ok(text) => {
            let symbol_name = symbol.rsplit("::").next().unwrap_or(symbol);
            if !text.contains(symbol_name) {
                violations
                    .push(format!("{doc}: field {field} symbol anchor does not exist: {raw}"));
            }
        }
        Err(err) => violations.push(format!(
            "{doc}: field {field} failed to read symbol target {}: {err}",
            full_path.display()
        )),
    }
}

fn markdown_heading_anchors(text: &str) -> BTreeSet<String> {
    let mut anchors = BTreeSet::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            continue;
        };
        if !rest.starts_with('#') && !rest.starts_with(' ') {
            continue;
        }
        let heading = trimmed.trim_start_matches('#').trim();
        if heading.is_empty() {
            continue;
        }
        let slug = markdown_anchor_slug(heading);
        if !slug.is_empty() {
            anchors.insert(slug);
        }
    }
    anchors
}

fn markdown_section_by_anchor<'a>(text: &'a str, anchor: &str) -> Option<&'a str> {
    let mut target: Option<(usize, usize)> = None;
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let line_for_heading = line.trim_end_matches(['\r', '\n']);
        let Some((level, slug)) = markdown_heading_level_and_slug(line_for_heading) else {
            continue;
        };

        if let Some((target_level, section_start)) = target {
            if level <= target_level {
                return Some(&text[section_start..line_start]);
            }
        } else if slug == anchor {
            target = Some((level, offset));
        }
    }

    target.map(|(_, section_start)| &text[section_start..])
}

fn markdown_heading_level_and_slug(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if level == 0 {
        return None;
    }
    let heading = trimmed.trim_start_matches('#').trim();
    if heading.is_empty() {
        return None;
    }
    let slug = markdown_anchor_slug(heading);
    if slug.is_empty() {
        return None;
    }
    Some((level, slug))
}

fn markdown_anchor_slug(heading: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in heading.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_dash = false;
        } else if (ch.is_ascii_whitespace() || ch == '-') && !last_was_dash && !slug.is_empty() {
            slug.push('-');
            last_was_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn contains_normalized_text(haystack: &str, needle: &str) -> bool {
    let normalized_needle = normalize_ascii_whitespace(needle);
    !normalized_needle.is_empty()
        && normalize_ascii_whitespace(haystack).contains(&normalized_needle)
}

fn normalize_ascii_whitespace(text: &str) -> String {
    text.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn require_non_empty_string(
    doc: &str,
    table: &toml::Table,
    field: &str,
    violations: &mut Vec<String>,
) {
    match string_field(table, field) {
        Some(value) if !value.trim().is_empty() => {}
        Some(_) => violations.push(format!("{doc}: field {field} must not be empty")),
        None => violations.push(format!("{doc}: field {field} must be a string")),
    }
}

fn string_field<'a>(table: &'a toml::Table, field: &str) -> Option<&'a str> {
    table.get(field).and_then(Value::as_str)
}

fn has_non_empty_string_field(table: &toml::Table, field: &str) -> bool {
    string_field(table, field).is_some_and(|value| !value.trim().is_empty())
}

fn read_text(root: &Path, rel: &str) -> Result<String> {
    let path = root.join(rel);
    fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    type TestResult<T = ()> = Result<T>;

    fn fixture() -> TestResult<TempDir> {
        let dir = tempfile::tempdir()?;
        write_file(dir.path(), "docs/proposals/proposal.md", "# Proposal\n")?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nLinked proposal: [Proposal](../../docs/proposals/proposal.md)\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal objective:\n\nMake the goal actionable from repo artifacts.\n\nGoal end state:\n\n- Agents can choose the next proof slice from the manifest.\n\nLane claim boundaries:\n\n- Do not promote provider behavior from manifest validation.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nCurrent implementation status\n\nReady when this fixture is selected.\n\nSupporting receipts\n\n`crates/example/src/lib.rs::receipt_symbol`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_file(dir.path(), ".perl-lsp/goals/archive/old.toml", "status = \"completed\"\n")?;
        write_file(dir.path(), "docs/project/status/status.md", "# Status\n")?;
        write_file(dir.path(), "docs/project/status/support.md", "# Support\n")?;
        write_file(dir.path(), "docs/specs/spec.md", "# Spec\n")?;
        write_file(dir.path(), "docs/specs/other.md", "# Other\n")?;
        write_file(dir.path(), "docs/adr/adr.md", "# ADR\n")?;
        write_file(dir.path(), "crates/example/src/lib.rs", "fn receipt_symbol() {}\n")?;
        Ok(dir)
    }

    fn write_manifest(root: &Path, body: &str) -> TestResult {
        write_file(root, ACTIVE_GOAL_PATH, body)
    }

    fn write_file(root: &Path, rel: &str, body: &str) -> TestResult {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, body)?;
        Ok(())
    }

    fn write_plan_with_status(root: &Path, status: &str) -> TestResult {
        write_file(
            root,
            "plans/goal/implementation-plan.md",
            &format!(
                "# Plan\n\nLinked proposal: [Proposal](../../docs/proposals/proposal.md)\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal objective:\n\nMake the goal actionable from repo artifacts.\n\nGoal end state:\n\n- Agents can choose the next proof slice from the manifest.\n\nLane claim boundaries:\n\n- Do not promote provider behavior from manifest validation.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: {status}\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nCurrent implementation status\n\nReady when this fixture is selected.\n\nSupporting receipts\n\n`crates/example/src/lib.rs::receipt_symbol`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n"
            ),
        )
    }

    fn valid_manifest() -> &'static str {
        r##"
id = "goal"
title = "Goal"
status = "active"
owner = "codex"
created = "2026-05-20"
proposal = "docs/proposals/proposal.md"
plan = "plans/goal/implementation-plan.md"
previous_goal = ".perl-lsp/goals/archive/old.toml"
status_pointer = "docs/project/status/status.md"
specs = ["docs/specs/spec.md"]
adrs = ["docs/adr/adr.md"]
status_docs = ["docs/project/status/status.md", "docs/project/status/support.md"]
objective = "Make the goal actionable from repo artifacts."
next_action = "Select work item demo before starting implementation."
end_state = ["Agents can choose the next proof slice from the manifest."]
claim_boundaries = ["Do not promote provider behavior from manifest validation."]

[[work_item]]
id = "demo"
status = "planned"
spec = "docs/specs/spec.md"
plan = "plans/goal/implementation-plan.md#work-item-demo"
current_pointer = "docs/project/status/status.md"
runtime_receipt = "crates/example/src/lib.rs::receipt_symbol"
claim_boundary = "No behavior change."
current_status = "Ready when this fixture is selected."
commands = ["rtk git diff --check"]
"##
    }

    #[test]
    fn accepts_manifest_with_existing_paths_and_anchors() -> TestResult {
        let dir = fixture()?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(violations.is_empty(), "valid manifest should pass: {violations:?}");
        assert_eq!(stats.work_items, 1);
        assert_eq!(stats.open_work_items, 1);
        assert_eq!(stats.actionable_work_items, 0);
        assert_eq!(stats.current_work_item.as_deref(), None);
        assert!(stats.path_references > 0);
        assert_eq!(stats.proof_commands, 1);
        Ok(())
    }

    #[test]
    fn counts_active_or_ready_work_items_as_actionable() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "ready")?;
        let manifest = valid_manifest()
            .replace("status = \"planned\"", "status = \"ready\"")
            .replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Select work item demo before starting implementation.\"\n",
            );
        write_manifest(dir.path(), &manifest)?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(violations.is_empty(), "ready manifest should pass: {violations:?}");
        assert_eq!(stats.work_items, 1);
        assert_eq!(stats.open_work_items, 1);
        assert_eq!(stats.actionable_work_items, 1);
        assert_eq!(stats.current_work_item.as_deref(), Some("demo"));
        assert_eq!(
            stats.current_work_item_plan.as_deref(),
            Some("plans/goal/implementation-plan.md#work-item-demo")
        );
        assert_eq!(
            stats.current_work_item_pointer.as_deref(),
            Some("docs/project/status/status.md")
        );
        assert_eq!(
            stats.current_work_item_status.as_deref(),
            Some("Ready when this fixture is selected.")
        );
        assert_eq!(stats.current_work_item_claim_boundary.as_deref(), Some("No behavior change."));
        assert_eq!(stats.current_work_item_commands, vec!["rtk git diff --check"]);
        Ok(())
    }

    #[test]
    fn accepts_single_active_work_item_as_current() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "active")?;
        let manifest = valid_manifest()
            .replace("status = \"planned\"", "status = \"active\"")
            .replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Continue the active demo work item.\"\n",
            );
        write_manifest(dir.path(), &manifest)?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(violations.is_empty(), "single active work item should pass: {violations:?}");
        assert_eq!(stats.work_items, 1);
        assert_eq!(stats.open_work_items, 1);
        assert_eq!(stats.actionable_work_items, 1);
        assert_eq!(stats.current_work_item.as_deref(), Some("demo"));
        Ok(())
    }

    #[test]
    fn rejects_non_slug_top_level_goal_id() -> TestResult {
        let dir = fixture()?;
        write_manifest(dir.path(), &valid_manifest().replace("id = \"goal\"", "id = \"Goal\""))?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field id must be a stable id")),
            "non-slug top-level goal id should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_non_active_top_level_goal_status() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"active\"", "status = \"completed\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("expected \"active\"")),
            "active manifest should reject non-active top-level status: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_top_level_title() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("title = \"Goal\"", "title = \" Goal \""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field title must not include")),
            "whitespace-padded top-level title should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_top_level_owner() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("owner = \"codex\"", "owner = \" codex \""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field owner must not include")),
            "whitespace-padded top-level owner should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_non_slug_work_item_id() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("id = \"demo\"", "id = \"demo item\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field id must be a stable id")),
            "non-slug work item id should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_id_with_consecutive_hyphens() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("id = \"demo\"", "id = \"demo--item\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field id must be a stable id")),
            "work item id with consecutive hyphens should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_non_slug_current_work_item() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "ready")?;
        let manifest = valid_manifest()
            .replace("status = \"planned\"", "status = \"ready\"")
            .replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo item\"\nnext_action = \"Select work item demo item before starting implementation.\"\n",
            );
        write_manifest(dir.path(), &manifest)?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("current_work_item must be a stable id")),
            "non-slug current_work_item should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_next_action_that_omits_current_work_item() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "ready")?;
        let manifest = valid_manifest()
            .replace("status = \"planned\"", "status = \"ready\"")
            .replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Continue the current ready slice.\"\n",
            );
        write_manifest(dir.path(), &manifest)?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("next_action must mention current_work_item")),
            "next_action should name current_work_item exactly: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_actionable_goal_without_current_work_item() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"planned\"", "status = \"ready\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.actionable_work_items, 1);
        assert!(
            violations.iter().any(|violation| violation.contains("current_work_item")),
            "actionable goal without current_work_item should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_current_work_item_that_is_not_actionable() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Select work item demo before starting implementation.\"\n",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.actionable_work_items, 0);
        assert!(
            violations.iter().any(|violation| violation.contains("current_work_item")),
            "current_work_item should reference an actionable id: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_actionable_work_item_without_current_status() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "ready")?;
        let manifest = valid_manifest()
            .replace("status = \"planned\"", "status = \"ready\"")
            .replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Select work item demo before starting implementation.\"\n",
            )
            .replace("current_status = \"Ready when this fixture is selected.\"\n", "");
        write_manifest(dir.path(), &manifest)?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation
                    .contains("actionable work item must include current_status")),
            "actionable work item should explain its current state: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_current_work_item_that_ignores_active_item() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\n## Work item: demo\n\n## Work item: other\n",
        )?;
        let second_item = r#"
[[work_item]]
id = "other"
status = "active"
spec = "docs/specs/spec.md"
plan = "plans/goal/implementation-plan.md#work-item-other"
current_pointer = "docs/project/status/status.md"
runtime_receipt = "crates/example/src/lib.rs::receipt_symbol"
claim_boundary = "No behavior change."
commands = ["rtk git diff --check"]
"#;
        let manifest =
            valid_manifest().replace("status = \"planned\"", "status = \"ready\"").replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Continue current work item.\"\n",
            );
        write_manifest(dir.path(), &format!("{manifest}{second_item}"))?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.actionable_work_items, 2);
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("must reference the active work_item")),
            "current_work_item should follow active work item when one exists: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_multiple_active_work_items() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\n## Work item: demo\n\n## Work item: other\n",
        )?;
        let second_item = r#"
[[work_item]]
id = "other"
status = "active"
spec = "docs/specs/spec.md"
plan = "plans/goal/implementation-plan.md#work-item-other"
current_pointer = "docs/project/status/status.md"
runtime_receipt = "crates/example/src/lib.rs::receipt_symbol"
claim_boundary = "No behavior change."
commands = ["rtk git diff --check"]
"#;
        let manifest =
            valid_manifest().replace("status = \"planned\"", "status = \"active\"").replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "current_work_item = \"demo\"\nnext_action = \"Continue current work item.\"\n",
            );
        write_manifest(dir.path(), &format!("{manifest}{second_item}"))?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.actionable_work_items, 2);
        assert!(
            violations.iter().any(|violation| violation.contains("at most one work_item")),
            "multiple active work items should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_active_goal_without_actionable_work_or_next_action() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "next_action = \"Select work item demo before starting implementation.\"\n",
                "",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.open_work_items, 1);
        assert_eq!(stats.actionable_work_items, 0);
        assert!(
            violations.iter().any(|violation| violation.contains("next_action")),
            "active goal with no actionable handoff should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_next_action_when_present() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "next_action = \"Select work item demo before starting implementation.\"",
                "next_action = \"\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field next_action")),
            "empty next_action should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_spec_path() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("docs/specs/spec.md", "docs/specs/missing.md"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("docs/specs/missing.md")),
            "missing spec path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_top_level_path_field() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("proposal = \"docs/proposals/proposal.md\"", "proposal = \"\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("empty path reference")),
            "empty top-level path field should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_top_level_path_array_entry() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "specs = [\"docs/specs/spec.md\"]",
                "specs = [\"docs/specs/spec.md\", \"\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("empty path reference")),
            "empty top-level path array entry should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_work_item_receipt_path() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "runtime_receipt = \"crates/example/src/lib.rs::receipt_symbol\"",
                "runtime_receipt = \"\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("empty path reference")),
            "empty work item receipt path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_markdown_anchor() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("#work-item-demo", "#missing-anchor"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("markdown anchor")),
            "missing markdown anchor should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_symbol_anchor() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("receipt_symbol", "missing_receipt_symbol"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("symbol anchor")),
            "missing symbol anchor should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_missing_objective() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("objective = \"Make the goal actionable from repo artifacts.\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field objective")),
            "missing objective should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_malformed_created_date() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("created = \"2026-05-20\"", "created = \"2026/05/20\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("YYYY-MM-DD")),
            "malformed created date should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_created_calendar_date() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("created = \"2026-05-20\"", "created = \"2026-02-29\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("YYYY-MM-DD")),
            "invalid calendar date should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn accepts_valid_leap_day_created_date() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("created = \"2026-05-20\"", "created = \"2024-02-29\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(violations.is_empty(), "valid leap day date should pass: {violations:?}");
        Ok(())
    }

    #[test]
    fn rejects_empty_end_state() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "end_state = [\"Agents can choose the next proof slice from the manifest.\"]",
                "end_state = []",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field end_state")),
            "empty end_state should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_claim_boundary_entry() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "claim_boundaries = [\"Do not promote provider behavior from manifest validation.\"]",
                "claim_boundaries = [\"\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field claim_boundaries")),
            "empty claim boundary entry should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_end_state_entry() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "end_state = [\"Agents can choose the next proof slice from the manifest.\"]",
                "end_state = [\"Agents can choose the next proof slice from the manifest.\", \"Agents can choose the next proof slice from the manifest.\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("duplicate entry")),
            "duplicate end_state entry should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_claim_boundary_entry() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "claim_boundaries = [\"Do not promote provider behavior from manifest validation.\"]",
                "claim_boundaries = [\" Do not promote provider behavior from manifest validation. \"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("leading or trailing whitespace")),
            "whitespace-padded claim boundary entry should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_without_proof_commands() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("commands = [\"rtk git diff --check\"]\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field commands")),
            "missing proof commands should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_status_mismatched_with_plan_section() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "ready")?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must contain `Status: planned`")),
            "mismatched plan status should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_claim_boundary_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "claim_boundary = \"No behavior change.\"",
                "claim_boundary = \"Different claim boundary.\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must include claim_boundary")),
            "work item claim boundary absent from linked plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_current_pointer_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_pointer = \"docs/project/status/status.md\"",
                "current_pointer = \"docs/project/status/support.md\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must include current_pointer")),
            "work item current pointer absent from linked plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_current_status_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_status = \"Ready when this fixture is selected.\"",
                "current_status = \"Different current status.\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must include current_status")),
            "work item current_status absent from linked plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn accepts_wrapped_work_item_current_status_in_plan_section() -> TestResult {
        let dir = fixture()?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?.replace(
            "Ready when this fixture is selected.",
            "Ready when this fixture\nis selected.",
        );
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.is_empty(),
            "wrapped work item current_status should be accepted: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_work_item_claim_boundary() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "claim_boundary = \"No behavior change.\"",
                "claim_boundary = \" No behavior change. \"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation
                .contains("field claim_boundary must not include leading or trailing whitespace")),
            "whitespace-padded work item claim_boundary should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_work_item_current_status() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_status = \"Ready when this fixture is selected.\"",
                "current_status = \" Ready when this fixture is selected. \"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation
                .contains("field current_status must not include leading or trailing whitespace")),
            "whitespace-padded work item current_status should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_trigger_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_status = \"Ready when this fixture is selected.\"\n",
                "current_status = \"Ready when this fixture is selected.\"\ntrigger = \"Run after upstream proof changes.\"\n",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must include trigger")),
            "work item trigger missing from plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_blocked_by_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "blocked")?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"planned\"", "status = \"blocked\"").replace(
                "current_status = \"Ready when this fixture is selected.\"\n",
                "blocked_by = \"Waiting for upstream proof.\"\n",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must include blocked_by")),
            "work item blocker missing from plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_work_item_trigger() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_status = \"Ready when this fixture is selected.\"\n",
                "current_status = \"Ready when this fixture is selected.\"\ntrigger = \" Run after upstream proof changes. \"\n",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation
                .contains("field trigger must not include leading or trailing whitespace")),
            "whitespace-padded work item trigger should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_work_item_blocked_by() -> TestResult {
        let dir = fixture()?;
        write_plan_with_status(dir.path(), "blocked")?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"planned\"", "status = \"blocked\"").replace(
                "current_status = \"Ready when this fixture is selected.\"\n",
                "blocked_by = \" Waiting for upstream proof. \"\n",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation
                .contains("field blocked_by must not include leading or trailing whitespace")),
            "whitespace-padded work item blocked_by should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_receipt_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?
            .replace("`crates/example/src/lib.rs::receipt_symbol`", "");
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("must include runtime_receipt receipt")),
            "work item receipt missing from plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_proof_command() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("commands = [\"rtk git diff --check\"]", "commands = [\"\"]"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("empty command")),
            "empty proof command should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_proof_command() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "commands = [\"rtk git diff --check\"]",
                "commands = [\"rtk git diff --check \"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation
                .contains("proof command must not include leading or trailing whitespace")),
            "whitespace-padded proof command should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_proof_command() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "commands = [\"rtk git diff --check\"]",
                "commands = [\"rtk git diff --check\", \"rtk git diff --check\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("duplicate proof command")),
            "duplicate proof command should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_unprefixed_proof_command() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "commands = [\"rtk git diff --check\"]",
                "commands = [\"git diff --check\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("proof command must start")),
            "unprefixed proof command should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_proof_command_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\n## Work item: demo\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation
                    .contains("proof command is missing from linked plan section")),
            "missing plan command mirror should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_proof_command_only_present_in_another_work_item_section() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\n## Work item: demo\n\n## Work item: other\n\n```bash\nrtk git diff --check\n```\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation
                    .contains("proof command is missing from linked plan section")),
            "commands mirrored in a different work item section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_top_level_spec_path() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "specs = [\"docs/specs/spec.md\"]",
                "specs = [\"docs/specs/spec.md\", \"docs/specs/spec.md\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("duplicate path")),
            "duplicate top-level spec path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_work_item_id() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\n## Work item: demo\n\n## Work item: other\n",
        )?;
        let second_item = r#"
[[work_item]]
id = "demo"
status = "planned"
spec = "docs/specs/spec.md"
plan = "plans/goal/implementation-plan.md#work-item-other"
current_pointer = "docs/project/status/status.md"
claim_boundary = "No behavior change."
commands = ["rtk git diff --check"]
"#;
        write_manifest(dir.path(), &format!("{}{}", valid_manifest(), second_item))?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("duplicate work_item id")),
            "duplicate work item id should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_absolute_path_reference() -> TestResult {
        let dir = fixture()?;
        let absolute_spec =
            dir.path().join("docs/specs/spec.md").display().to_string().replace('\\', "/");
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "specs = [\"docs/specs/spec.md\"]",
                &format!("specs = [\"{absolute_spec}\"]"),
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("repo-relative path")),
            "absolute path references should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_rooted_path_reference() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("specs = [\"docs/specs/spec.md\"]", "specs = [\"/outside/spec.md\"]"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("repo-relative path")),
            "rooted path references should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_parent_directory_path_reference() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("specs = [\"docs/specs/spec.md\"]", "specs = [\"../outside/spec.md\"]"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("repo-relative path")),
            "parent-directory path references should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_whitespace_padded_path_reference() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("specs = [\"docs/specs/spec.md\"]", "specs = [\" docs/specs/spec.md \"]"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("leading or trailing whitespace")),
            "whitespace-padded path references should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_anchor_in_top_level_path_inventory() -> TestResult {
        let dir = fixture()?;
        write_file(dir.path(), "docs/specs/spec.md", "# Spec\n\n## Details\n")?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "specs = [\"docs/specs/spec.md\"]",
                "specs = [\"docs/specs/spec.md#details\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("document inventory")),
            "top-level path inventories should reject anchors: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_symbol_in_top_level_path_inventory() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "status_docs = [\"docs/project/status/status.md\", \"docs/project/status/support.md\"]",
                "status_docs = [\"docs/project/status/status.md::Status\", \"docs/project/status/support.md\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("document inventory")),
            "top-level path inventories should reject symbols: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_spec_missing_from_top_level_specs() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("specs = [\"docs/specs/spec.md\"]", "specs = [\"docs/specs/other.md\"]"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level specs")),
            "work item spec absent from top-level specs should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_spec_missing_from_plan_section() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace(
                    "specs = [\"docs/specs/spec.md\"]",
                    "specs = [\"docs/specs/spec.md\", \"docs/specs/other.md\"]",
                )
                .replace("spec = \"docs/specs/spec.md\"", "spec = \"docs/specs/other.md\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must mention spec")),
            "work item spec absent from linked plan section should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_contract_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level ADR")),
            "top-level ADR missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_handoff_reference_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal objective:\n\nMake the goal actionable from repo artifacts.\n\nGoal end state:\n\n- Agents can choose the next proof slice from the manifest.\n\nLane claim boundaries:\n\n- Do not promote provider behavior from manifest validation.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level proposal reference")),
            "top-level proposal missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_objective_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nLinked proposal: [Proposal](../../docs/proposals/proposal.md)\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal end state:\n\n- Agents can choose the next proof slice from the manifest.\n\nLane claim boundaries:\n\n- Do not promote provider behavior from manifest validation.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level objective")),
            "top-level objective missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_claim_boundary_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nLinked proposal: [Proposal](../../docs/proposals/proposal.md)\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal objective:\n\nMake the goal actionable from repo artifacts.\n\nGoal end state:\n\n- Agents can choose the next proof slice from the manifest.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level claim boundary")),
            "top-level claim boundary missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_end_state_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nLinked proposal: [Proposal](../../docs/proposals/proposal.md)\nPrevious goal archive: [old](../../.perl-lsp/goals/archive/old.toml)\n\nGoal objective:\n\nMake the goal actionable from repo artifacts.\n\nLane claim boundaries:\n\n- Do not promote provider behavior from manifest validation.\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level end_state")),
            "top-level end_state missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_top_level_status_doc_missing_from_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level status doc")),
            "top-level status doc missing from linked plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_plan_section_without_rollback_heading() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/goal/implementation-plan.md",
            "# Plan\n\nCurrent merged contract anchors:\n\n- [Spec](../../docs/specs/spec.md)\n- [ADR](../../docs/adr/adr.md)\n\nStatus owners:\n\n- [status](../../docs/project/status/status.md)\n- [support](../../docs/project/status/support.md)\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n",
        )?;
        write_manifest(dir.path(), valid_manifest())?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must contain `Rollback`")),
            "work item plan section without rollback heading should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_status_current_pointer_missing_from_top_level_status_docs() -> TestResult {
        let dir = fixture()?;
        write_file(dir.path(), "docs/project/status/other.md", "# Other\n")?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_pointer = \"docs/project/status/status.md\"",
                "current_pointer = \"docs/project/status/other.md\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("top-level status_docs")),
            "status current pointer absent from top-level status docs should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_plan_outside_top_level_plan() -> TestResult {
        let dir = fixture()?;
        write_file(
            dir.path(),
            "plans/other/implementation-plan.md",
            "# Other Plan\n\n## Work item: demo\n\nStatus: planned\nLinked spec: [Spec](../../docs/specs/spec.md)\nCurrent pointer: `docs/project/status/status.md`\n\nClaim boundary\n\nNo behavior change.\n\nNon-goals\n\nNo behavior change.\n\nAcceptance\n\nThe manifest remains checkable.\n\nProof commands\n\n```bash\nrtk git diff --check\n```\n\nRollback\n\nRevert the fixture PR.\n",
        )?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "plan = \"plans/goal/implementation-plan.md#work-item-demo\"",
                "plan = \"plans/other/implementation-plan.md#work-item-demo\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("must match top-level plan")),
            "work item plan path outside top-level plan should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_status_pointer_missing_from_status_docs() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "status_docs = [\"docs/project/status/status.md\", \"docs/project/status/support.md\"]",
                "status_docs = [\"docs/project/status/support.md\"]",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("status_pointer")),
            "status_pointer should be listed in status_docs: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn accepts_anchored_status_pointer_when_path_is_listed_in_status_docs() -> TestResult {
        let dir = fixture()?;
        write_file(dir.path(), "docs/project/status/status.md", "# Status\n\n## Current\n")?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?.replace(
            "[status](../../docs/project/status/status.md)",
            "[status](../../docs/project/status/status.md#current)",
        );
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "status_pointer = \"docs/project/status/status.md\"",
                "status_pointer = \"docs/project/status/status.md#current\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(violations.is_empty(), "anchored status pointer should pass: {violations:?}");
        Ok(())
    }

    #[test]
    fn rejects_anchor_in_top_level_proposal_document_path() -> TestResult {
        let dir = fixture()?;
        write_file(dir.path(), "docs/proposals/proposal.md", "# Proposal\n\n## Details\n")?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?
            .replace("docs/proposals/proposal.md", "docs/proposals/proposal.md#details");
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "proposal = \"docs/proposals/proposal.md\"",
                "proposal = \"docs/proposals/proposal.md#details\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("field proposal is a document path")),
            "anchored top-level proposal path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_anchor_in_top_level_plan_document_path() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "plan = \"plans/goal/implementation-plan.md\"",
                "plan = \"plans/goal/implementation-plan.md#plan\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field plan is a document path")),
            "anchored top-level plan path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_symbol_in_top_level_previous_goal_document_path() -> TestResult {
        let dir = fixture()?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?.replace(
            ".perl-lsp/goals/archive/old.toml",
            ".perl-lsp/goals/archive/old.toml::status",
        );
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "previous_goal = \".perl-lsp/goals/archive/old.toml\"",
                "previous_goal = \".perl-lsp/goals/archive/old.toml::status\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("field previous_goal is a document path")),
            "symbol top-level previous_goal path should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_symbol_in_work_item_plan_document_pointer() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "plan = \"plans/goal/implementation-plan.md#work-item-demo\"",
                "plan = \"plans/goal/implementation-plan.md::Plan#work-item-demo\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("field plan is a document pointer")),
            "symbol work-item plan pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_anchor_in_work_item_spec_document_pointer() -> TestResult {
        let dir = fixture()?;
        write_file(dir.path(), "docs/specs/spec.md", "# Spec\n\n## Section\n")?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?
            .replace("docs/specs/spec.md", "docs/specs/spec.md#section");
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("spec = \"docs/specs/spec.md\"", "spec = \"docs/specs/spec.md#section\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field spec is a document path")),
            "anchored work-item spec pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_symbol_in_work_item_spec_document_pointer() -> TestResult {
        let dir = fixture()?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?
            .replace("docs/specs/spec.md", "docs/specs/spec.md::Spec");
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("spec = \"docs/specs/spec.md\"", "spec = \"docs/specs/spec.md::Spec\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field spec is a document path")),
            "symbol work-item spec pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_symbol_in_work_item_current_pointer() -> TestResult {
        let dir = fixture()?;
        let plan = read_text(dir.path(), "plans/goal/implementation-plan.md")?.replace(
            "Current pointer: `docs/project/status/status.md`",
            "Current pointer: `docs/project/status/status.md::Status`",
        );
        write_file(dir.path(), "plans/goal/implementation-plan.md", &plan)?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace(
                "current_pointer = \"docs/project/status/status.md\"",
                "current_pointer = \"docs/project/status/status.md::Status\"",
            ),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations
                .iter()
                .any(|violation| violation.contains("field current_pointer is a document pointer")),
            "symbol work-item current pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_unknown_work_item_status() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"planned\"", "status = \"maybe\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("allowed work item status")),
            "unknown work item status should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_planned_work_item_without_routing_context() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("current_status = \"Ready when this fixture is selected.\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("trigger or current_status")),
            "planned work item without routing context should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_blocked_work_item_without_blocker_context() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("status = \"planned\"", "status = \"blocked\"")
                .replace("current_status = \"Ready when this fixture is selected.\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("blocked_by")),
            "blocked work item without blocker context should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_completed_work_item_without_receipt() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("status = \"planned\"", "status = \"completed\"")
                .replace("runtime_receipt = \"crates/example/src/lib.rs::receipt_symbol\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("receipt or *_receipt")),
            "completed work item without receipt should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_active_goal_without_open_work_item() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("status = \"planned\"", "status = \"completed\""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert_eq!(stats.open_work_items, 0);
        assert!(
            violations.iter().any(|violation| violation.contains("active goal")),
            "active goal without open work should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_without_plan_pointer() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest()
                .replace("plan = \"plans/goal/implementation-plan.md#work-item-demo\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field plan")),
            "missing work item plan pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_work_item_without_current_pointer() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("current_pointer = \"docs/project/status/status.md\"\n", ""),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("field current_pointer")),
            "missing work item current pointer should be rejected: {violations:?}"
        );
        Ok(())
    }

    #[test]
    fn rejects_plan_anchor_that_does_not_match_work_item_id() -> TestResult {
        let dir = fixture()?;
        write_manifest(
            dir.path(),
            &valid_manifest().replace("#work-item-demo", "#work-item-other"),
        )?;
        let mut stats = ValidationStats::default();

        let violations = collect_violations(dir.path(), &mut stats)?;

        assert!(
            violations.iter().any(|violation| violation.contains("plan anchor")),
            "plan anchor mismatched with work item id should be rejected: {violations:?}"
        );
        Ok(())
    }
}
