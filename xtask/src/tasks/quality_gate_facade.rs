//! Stable quality-gate facade.
//!
//! Candidate evaluation is intentionally clock-free. The underlying proof
//! engine still understands the legacy lifecycle fields, but this facade feeds
//! it a structurally equivalent policy whose valid `review_after` and `expires`
//! values cannot cross during evaluation, then restores the committed values in
//! the emitted receipt. Elapsed-time classification belongs to
//! `cargo xtask policy cadence`; it must not mutate an unchanged PR verdict.

#[path = "quality_gate.rs"]
mod implementation;

pub use implementation::{QualityGateArgs, QualityGateMode};

use chrono::NaiveDate;
use color_eyre::eyre::{Context, Result, bail};
use serde_json::Value as JsonValue;
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use toml::{Table as TomlTable, Value as TomlValue};

const LIFECYCLE_SENTINEL: &str = "9999-12-31";

#[derive(Clone, Debug, PartialEq, Eq)]
struct LifecycleDates {
    id: String,
    review_after: String,
    expires: String,
}

#[derive(Debug)]
struct NormalizedPolicy {
    text: String,
    lifecycle: Vec<LifecycleDates>,
    validation_error: Option<String>,
    synthetic_metadata_failure_only: bool,
}

#[derive(Debug)]
struct TempWorkspace {
    root: TempDir,
}

impl TempWorkspace {
    fn new() -> Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("perl-lsp-quality-gate-")
            .tempdir()
            .context("creating exclusive quality-gate workspace")?;
        Ok(Self { root })
    }

    fn policy(&self) -> PathBuf {
        self.root.path().join("quality-gate-exceptions.toml")
    }

    fn receipt(&self) -> PathBuf {
        self.root.path().join("quality-gate.json")
    }

    fn summary(&self) -> PathBuf {
        self.root.path().join("quality-gate.md")
    }
}

pub fn run(args: QualityGateArgs) -> Result<()> {
    let raw_policy = match fs::read_to_string(&args.exception_policy) {
        Ok(raw) => raw,
        // The existing engine deliberately turns unreadable policy into a
        // receipt-producing fail-closed result. Preserve that contract instead
        // of letting the facade return before its diagnostics are written.
        Err(_) => return implementation::run(args),
    };
    let normalized = normalize_policy(&raw_policy)?;
    let temporary = TempWorkspace::new()?;
    let temporary_policy = temporary.policy();
    let temporary_receipt = temporary.receipt();
    let temporary_summary = temporary.summary();
    fs::write(&temporary_policy, &normalized.text).with_context(|| {
        format!("writing normalized quality exception policy {}", temporary_policy.display())
    })?;

    let engine_args = QualityGateArgs {
        mode: args.mode.clone(),
        exception_policy: temporary_policy.clone(),
        ripr_receipt: args.ripr_receipt.clone(),
        ripr_pr_receipt: args.ripr_pr_receipt.clone(),
        review_receipt: args.review_receipt.clone(),
        coverage_receipt: args.coverage_receipt.clone(),
        codecov: args.codecov.clone(),
        patch_coverage: args.patch_coverage,
        ripr_base: args.ripr_base.clone(),
        ripr_head: args.ripr_head.clone(),
        receipt: temporary_receipt.clone(),
        summary: temporary_summary.clone(),
        check: false,
    };

    let engine_result = implementation::run(engine_args);
    if !temporary_receipt.is_file() || !temporary_summary.is_file() {
        return engine_result;
    }

    let mut receipt: JsonValue = serde_json::from_str(
        &fs::read_to_string(&temporary_receipt)
            .with_context(|| format!("reading {}", temporary_receipt.display()))?,
    )
    .context("parsing normalized quality-gate receipt")?;

    let temporary_policy_display = display_path(&temporary_policy);
    let temporary_receipt_display = display_path(&temporary_receipt);
    let temporary_summary_display = display_path(&temporary_summary);
    let original_policy_display = display_path(&args.exception_policy);
    let original_receipt_display = display_path(&args.receipt);
    let original_summary_display = display_path(&args.summary);
    let replacements = [
        (temporary_policy_display.as_str(), original_policy_display.as_str()),
        (temporary_receipt_display.as_str(), original_receipt_display.as_str()),
        (temporary_summary_display.as_str(), original_summary_display.as_str()),
    ];

    replace_json_strings(&mut receipt, &replacements);
    restore_lifecycle_dates(&mut receipt, &normalized.lifecycle);
    restore_policy_validation_error(
        &mut receipt,
        normalized.validation_error.as_deref(),
        normalized.synthetic_metadata_failure_only,
    );
    let receipt_text = format!("{}\n", serde_json::to_string_pretty(&receipt)?);

    let mut summary = fs::read_to_string(&temporary_summary)
        .with_context(|| format!("reading {}", temporary_summary.display()))?;
    for (from, to) in replacements {
        summary = summary.replace(from, to);
    }
    summary.push_str(
        "\n## Policy Lifecycle\n\n- authority: `cargo xtask policy cadence`\n- candidate impact: `advisory_only`\n",
    );
    if let Some(error) = normalized.validation_error.as_deref() {
        summary.push_str(&format!("- validation error: `{error}`\n"));
    }

    if args.check {
        assert_current(&args.receipt, &receipt_text, "quality gate JSON receipt")?;
        assert_current(&args.summary, &summary, "quality gate Markdown summary")?;
    } else {
        write_text(&args.receipt, &receipt_text)?;
        write_text(&args.summary, &summary)?;
    }

    let failed = receipt.get("decision").and_then(JsonValue::as_str) == Some("fail");
    if failed {
        bail!(
            "quality gate failed; see receipt {} and summary {}",
            args.receipt.display(),
            args.summary.display()
        );
    }

    engine_result?;
    println!(
        "quality gate passed; receipt {} summary {}",
        args.receipt.display(),
        args.summary.display()
    );
    Ok(())
}

fn normalize_policy(raw: &str) -> Result<NormalizedPolicy> {
    let Ok(mut policy) = toml::from_str::<TomlValue>(raw) else {
        return Ok(NormalizedPolicy {
            text: raw.to_string(),
            lifecycle: Vec::new(),
            validation_error: None,
            synthetic_metadata_failure_only: false,
        });
    };
    let Some(table) = policy.as_table_mut() else {
        return Ok(NormalizedPolicy {
            text: raw.to_string(),
            lifecycle: Vec::new(),
            validation_error: None,
            synthetic_metadata_failure_only: false,
        });
    };

    let original_metadata_is_valid = policy_metadata_is_valid(table);
    let mut validation_error = None;
    let mut synthetic_metadata_failure_only = false;
    if let Some(due_review) = table.get("due_review") {
        let Some(due_review) = due_review.as_str() else {
            return Ok(NormalizedPolicy {
                text: raw.to_string(),
                lifecycle: Vec::new(),
                validation_error: None,
                synthetic_metadata_failure_only: false,
            });
        };
        if !matches!(due_review, "warn" | "fail") {
            validation_error = Some(format!(
                "quality exception due_review must be warn or fail, found {due_review}"
            ));
            synthetic_metadata_failure_only = original_metadata_is_valid;
            // Ask the existing engine to emit its normal fail-closed receipt and
            // summary rather than returning before artifact publication.
            table.insert("status".to_string(), TomlValue::String("invalid".to_string()));
        }
    }
    table.insert("due_review".to_string(), TomlValue::String("warn".to_string()));

    let mut lifecycle = Vec::new();
    if let Some(exceptions) = table.get_mut("exception").and_then(TomlValue::as_array_mut) {
        for exception in exceptions {
            let Some(exception) = exception.as_table_mut() else {
                continue;
            };
            // Mirror the engine's required-field/kind/date boundary before
            // normalizing. Rows the engine will reject must stay byte-semantic
            // inputs to its structural diagnostics and must not disturb the
            // duplicate-ID restoration queues for accepted rows.
            if !candidate_exception_is_structurally_valid(exception) {
                continue;
            }
            let id = exception.get("id").and_then(TomlValue::as_str).map(str::to_string);
            let review_after =
                exception.get("review_after").and_then(TomlValue::as_str).map(str::to_string);
            let expires = exception.get("expires").and_then(TomlValue::as_str).map(str::to_string);

            let (Some(id), Some(review_after), Some(expires)) = (id, review_after, expires) else {
                continue;
            };

            lifecycle.push(LifecycleDates { id, review_after, expires });
            exception.insert(
                "review_after".to_string(),
                TomlValue::String(LIFECYCLE_SENTINEL.to_string()),
            );
            exception
                .insert("expires".to_string(), TomlValue::String(LIFECYCLE_SENTINEL.to_string()));
        }
    }

    Ok(NormalizedPolicy {
        text: toml::to_string(&policy)
            .context("serializing clock-free quality exception policy")?,
        lifecycle,
        validation_error,
        synthetic_metadata_failure_only,
    })
}

fn policy_metadata_is_valid(table: &TomlTable) -> bool {
    table
        .get("owner")
        .and_then(TomlValue::as_str)
        .is_some_and(|value| !value.trim().is_empty())
        && table.get("status").and_then(TomlValue::as_str) == Some("active")
        && table
            .get("updated")
            .and_then(TomlValue::as_str)
            .is_some_and(|value| !value.trim().is_empty())
}

fn candidate_exception_is_structurally_valid(exception: &TomlTable) -> bool {
    let required = [
        "id",
        "kind",
        "scope",
        "owner",
        "reason",
        "final_target",
        "evidence",
        "removal_criteria",
        "created",
        "review_after",
        "expires",
    ];
    let required_fields_are_present = required.iter().all(|field| {
        exception
            .get(*field)
            .and_then(TomlValue::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    });
    let lifecycle_dates_are_valid = ["created", "review_after", "expires"].iter().all(|field| {
        exception.get(*field).and_then(TomlValue::as_str).and_then(parse_date).is_some()
    });

    required_fields_are_present
        && lifecycle_dates_are_valid
        && exception.get("kind").and_then(TomlValue::as_str) == Some("temporary_burndown")
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

fn restore_lifecycle_dates(receipt: &mut JsonValue, lifecycle: &[LifecycleDates]) {
    if let Some(exceptions) =
        receipt.get_mut("temporary_exceptions").and_then(JsonValue::as_object_mut)
    {
        exceptions.insert("due_review".to_string(), JsonValue::String("advisory".to_string()));
        exceptions.insert(
            "lifecycle_authority".to_string(),
            JsonValue::String("policy_cadence".to_string()),
        );
        if let Some(active) = exceptions.get_mut("active").and_then(JsonValue::as_array_mut) {
            restore_lifecycle_entries(active, lifecycle);
        }
    }

    let Some(actions) = receipt.get_mut("next_actions").and_then(JsonValue::as_array_mut) else {
        return;
    };
    for action in actions {
        let is_final_blocker = action.get("kind").and_then(JsonValue::as_str)
            == Some("quality_exception_active_final_blocker");
        if !is_final_blocker {
            continue;
        }
        if let Some(active) = action.get_mut("active").and_then(JsonValue::as_array_mut) {
            restore_lifecycle_entries(active, lifecycle);
        }
    }
}

fn restore_lifecycle_entries(active: &mut [JsonValue], lifecycle: &[LifecycleDates]) {
    let mut dates_by_id: BTreeMap<&str, VecDeque<&LifecycleDates>> = BTreeMap::new();
    for dates in lifecycle {
        dates_by_id.entry(&dates.id).or_default().push_back(dates);
    }

    for entry in active {
        let Some(id) = entry.get("id").and_then(JsonValue::as_str) else {
            continue;
        };
        let Some(dates) = dates_by_id.get_mut(id).and_then(VecDeque::pop_front) else {
            continue;
        };
        let Some(entry) = entry.as_object_mut() else {
            continue;
        };
        entry.insert("review_after".to_string(), JsonValue::String(dates.review_after.clone()));
        entry.insert("expires".to_string(), JsonValue::String(dates.expires.clone()));
    }
}

fn restore_policy_validation_error(
    receipt: &mut JsonValue,
    error: Option<&str>,
    synthetic_metadata_failure_only: bool,
) {
    let Some(error) = error else {
        return;
    };

    if let Some(exceptions) =
        receipt.get_mut("temporary_exceptions").and_then(JsonValue::as_object_mut)
    {
        exceptions.insert("validation_error".to_string(), JsonValue::String(error.to_string()));
    }

    let Some(actions) = receipt.get_mut("next_actions").and_then(JsonValue::as_array_mut) else {
        return;
    };
    let Some(action_index) = actions.iter().position(|action| {
        action.get("kind").and_then(JsonValue::as_str)
            == Some("quality_exception_policy_not_current")
            && action.get("reason").and_then(JsonValue::as_str) == Some("invalid_metadata")
    }) else {
        return;
    };

    let mut due_review_action = actions[action_index].clone();
    if synthetic_metadata_failure_only {
        actions.remove(action_index);
    }
    let Some(action) = due_review_action.as_object_mut() else {
        return;
    };
    action.insert("reason".to_string(), JsonValue::String("invalid_due_review".to_string()));
    action.insert("repair".to_string(), JsonValue::String(error.to_string()));
    actions.push(due_review_action);
}

fn replace_json_strings(value: &mut JsonValue, replacements: &[(&str, &str)]) {
    match value {
        JsonValue::String(text) => {
            for &(from, to) in replacements {
                *text = text.replace(from, to);
            }
        }
        JsonValue::Array(items) => {
            for item in items {
                replace_json_strings(item, replacements);
            }
        }
        JsonValue::Object(fields) => {
            for value in fields.values_mut() {
                replace_json_strings(value, replacements);
            }
        }
        _ => {}
    }
}

fn assert_current(path: &Path, expected: &str, label: &str) -> Result<()> {
    let existing = match fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            bail!("{label} is missing: {}", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("reading {label} {}", path.display()));
        }
    };
    if normalize_output(&existing) != normalize_output(expected) {
        bail!("{label} is stale: {}", path.display());
    }
    Ok(())
}

fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

fn normalize_output(value: &str) -> String {
    value.trim().replace("\r\n", "\n")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(review_after: &str, expires: &str) -> String {
        format!(
            r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "test"
status = "active"
updated = "2026-09-10"
due_review = "fail"

[requirements]
required_active = ["fixture"]

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
reason = "fixture"
final_target = "fixture"
evidence = "fixture"
removal_criteria = "fixture"
created = "2026-01-01"
review_after = "{review_after}"
expires = "{expires}"
"#
        )
    }

    #[test]
    fn temp_workspaces_are_exclusive() -> Result<()> {
        let first = TempWorkspace::new()?;
        let second = TempWorkspace::new()?;
        assert_ne!(first.root.path(), second.root.path());
        Ok(())
    }

    #[test]
    fn valid_lifecycle_dates_are_removed_from_candidate_evaluation() -> Result<()> {
        let normalized = normalize_policy(&policy("2000-01-01", "2000-01-02"))?;
        let value: TomlValue = toml::from_str(&normalized.text)?;
        assert_eq!(value.get("due_review").and_then(TomlValue::as_str), Some("warn"));
        let exception = value
            .get("exception")
            .and_then(TomlValue::as_array)
            .and_then(|entries| entries.first())
            .and_then(TomlValue::as_table)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing normalized exception"))?;
        assert_eq!(
            exception.get("review_after").and_then(TomlValue::as_str),
            Some(LIFECYCLE_SENTINEL)
        );
        assert_eq!(exception.get("expires").and_then(TomlValue::as_str), Some(LIFECYCLE_SENTINEL));
        assert_eq!(
            normalized.lifecycle,
            vec![LifecycleDates {
                id: "fixture".to_string(),
                review_after: "2000-01-01".to_string(),
                expires: "2000-01-02".to_string(),
            }]
        );
        assert!(normalized.validation_error.is_none());
        assert!(!normalized.synthetic_metadata_failure_only);
        Ok(())
    }

    #[test]
    fn malformed_lifecycle_date_remains_visible_to_structural_validation() -> Result<()> {
        let normalized = normalize_policy(&policy("2000-01-01", "not-a-date"))?;
        let value: TomlValue = toml::from_str(&normalized.text)?;
        let exception = value
            .get("exception")
            .and_then(TomlValue::as_array)
            .and_then(|entries| entries.first())
            .and_then(TomlValue::as_table)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing normalized exception"))?;
        assert_eq!(exception.get("expires").and_then(TomlValue::as_str), Some("not-a-date"));
        assert!(normalized.lifecycle.is_empty());
        Ok(())
    }

    #[test]
    fn unsupported_due_review_is_routed_through_fail_closed_artifacts() -> Result<()> {
        let source = policy("2099-01-01", "2099-12-31")
            .replace("due_review = \"fail\"", "due_review = \"error\"");
        let normalized = normalize_policy(&source)?;
        let value: TomlValue = toml::from_str(&normalized.text)?;

        assert_eq!(value.get("status").and_then(TomlValue::as_str), Some("invalid"));
        assert_eq!(value.get("due_review").and_then(TomlValue::as_str), Some("warn"));
        assert_eq!(
            normalized.validation_error.as_deref(),
            Some("quality exception due_review must be warn or fail, found error")
        );
        assert!(normalized.synthetic_metadata_failure_only);
        Ok(())
    }

    #[test]
    fn malformed_created_row_does_not_shift_duplicate_id_restoration() -> Result<()> {
        let source = r#"schema_version = 1
policy = "quality-gate-exceptions"
owner = "test"
status = "active"
updated = "2026-09-10"
due_review = "fail"

[requirements]
required_active = ["fixture"]

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
reason = "rejected"
final_target = "fixture"
evidence = "fixture"
removal_criteria = "fixture"
created = "not-a-date"
review_after = "2026-09-16"
expires = "2026-09-30"

[[exception]]
id = "fixture"
kind = "temporary_burndown"
scope = "project_coverage"
owner = "proof"
reason = "accepted"
final_target = "fixture"
evidence = "fixture"
removal_criteria = "fixture"
created = "2026-01-01"
review_after = "2026-10-16"
expires = "2026-10-30"
"#;
        let normalized = normalize_policy(source)?;

        assert_eq!(
            normalized.lifecycle,
            vec![LifecycleDates {
                id: "fixture".to_string(),
                review_after: "2026-10-16".to_string(),
                expires: "2026-10-30".to_string(),
            }]
        );
        Ok(())
    }

    #[test]
    fn receipt_restores_committed_dates_in_all_embedded_exception_arrays() {
        let mut receipt = json!({
            "temporary_exceptions": {
                "due_review": "warn",
                "active": [
                    {
                        "id": "fixture",
                        "review_after": LIFECYCLE_SENTINEL,
                        "expires": LIFECYCLE_SENTINEL
                    },
                    {
                        "id": "fixture",
                        "review_after": LIFECYCLE_SENTINEL,
                        "expires": LIFECYCLE_SENTINEL
                    }
                ]
            },
            "next_actions": [{
                "kind": "quality_exception_active_final_blocker",
                "active": [
                    {
                        "id": "fixture",
                        "review_after": LIFECYCLE_SENTINEL,
                        "expires": LIFECYCLE_SENTINEL
                    },
                    {
                        "id": "fixture",
                        "review_after": LIFECYCLE_SENTINEL,
                        "expires": LIFECYCLE_SENTINEL
                    }
                ]
            }]
        });
        let lifecycle = vec![
            LifecycleDates {
                id: "fixture".to_string(),
                review_after: "2026-09-16".to_string(),
                expires: "2026-09-30".to_string(),
            },
            LifecycleDates {
                id: "fixture".to_string(),
                review_after: "2026-10-16".to_string(),
                expires: "2026-10-30".to_string(),
            },
        ];

        restore_lifecycle_dates(&mut receipt, &lifecycle);

        for prefix in [
            "/temporary_exceptions/active",
            "/next_actions/0/active",
        ] {
            assert_eq!(
                receipt
                    .pointer(&format!("{prefix}/0/review_after"))
                    .and_then(JsonValue::as_str),
                Some("2026-09-16")
            );
            assert_eq!(
                receipt
                    .pointer(&format!("{prefix}/0/expires"))
                    .and_then(JsonValue::as_str),
                Some("2026-09-30")
            );
            assert_eq!(
                receipt
                    .pointer(&format!("{prefix}/1/review_after"))
                    .and_then(JsonValue::as_str),
                Some("2026-10-16")
            );
            assert_eq!(
                receipt
                    .pointer(&format!("{prefix}/1/expires"))
                    .and_then(JsonValue::as_str),
                Some("2026-10-30")
            );
        }
        assert_eq!(
            receipt.pointer("/temporary_exceptions/due_review").and_then(JsonValue::as_str),
            Some("advisory")
        );
        assert_eq!(
            receipt
                .pointer("/temporary_exceptions/lifecycle_authority")
                .and_then(JsonValue::as_str),
            Some("policy_cadence")
        );
    }

    #[test]
    fn combined_header_and_due_review_errors_remain_distinct() {
        let mut receipt = json!({
            "temporary_exceptions": {},
            "next_actions": [
                {
                    "kind": "quality_exception_policy_not_current",
                    "reason": "invalid_header",
                    "repair": "repair header",
                    "verify": "verify header",
                    "receipt": "receipt header"
                },
                {
                    "kind": "quality_exception_policy_not_current",
                    "reason": "invalid_metadata",
                    "repair": "repair metadata",
                    "verify": "verify metadata",
                    "receipt": "receipt metadata"
                }
            ]
        });

        restore_policy_validation_error(
            &mut receipt,
            Some("quality exception due_review must be warn or fail, found error"),
            false,
        );

        let reasons = receipt
            .get("next_actions")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(|action| action.get("reason").and_then(JsonValue::as_str))
            .collect::<Vec<_>>();
        assert_eq!(
            reasons,
            vec!["invalid_header", "invalid_metadata", "invalid_due_review"]
        );
    }

    #[test]
    fn synthetic_metadata_failure_is_replaced_by_due_review_diagnosis() {
        let mut receipt = json!({
            "temporary_exceptions": {},
            "next_actions": [{
                "kind": "quality_exception_policy_not_current",
                "reason": "invalid_metadata",
                "repair": "repair metadata",
                "verify": "verify metadata",
                "receipt": "receipt metadata"
            }]
        });

        restore_policy_validation_error(
            &mut receipt,
            Some("quality exception due_review must be warn or fail, found error"),
            true,
        );

        let reasons = receipt
            .get("next_actions")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .filter_map(|action| action.get("reason").and_then(JsonValue::as_str))
            .collect::<Vec<_>>();
        assert_eq!(reasons, vec!["invalid_due_review"]);
    }
}
