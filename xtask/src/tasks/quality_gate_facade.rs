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
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use toml::Value as TomlValue;

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
}

#[derive(Debug)]
struct TempWorkspace {
    root: PathBuf,
}

impl TempWorkspace {
    fn new() -> Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("resolving quality-gate temporary workspace nonce")?
            .as_nanos();
        let root = std::env::temp_dir()
            .join(format!("perl-lsp-quality-gate-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)
            .with_context(|| format!("creating quality-gate workspace {}", root.display()))?;
        Ok(Self { root })
    }

    fn policy(&self) -> PathBuf {
        self.root.join("quality-gate-exceptions.toml")
    }

    fn receipt(&self) -> PathBuf {
        self.root.join("quality-gate.json")
    }

    fn summary(&self) -> PathBuf {
        self.root.join("quality-gate.md")
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _cleanup_result = fs::remove_dir_all(&self.root);
    }
}

pub fn run(args: QualityGateArgs) -> Result<()> {
    let raw_policy = fs::read_to_string(&args.exception_policy).with_context(|| {
        format!("reading quality exception policy {}", args.exception_policy.display())
    })?;
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
    let receipt_text = format!("{}\n", serde_json::to_string_pretty(&receipt)?);

    let mut summary = fs::read_to_string(&temporary_summary)
        .with_context(|| format!("reading {}", temporary_summary.display()))?;
    for (from, to) in replacements {
        summary = summary.replace(from, to);
    }
    summary.push_str(
        "\n## Policy Lifecycle\n\n- authority: `cargo xtask policy cadence`\n- candidate impact: `advisory_only`\n",
    );

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
        return Ok(NormalizedPolicy { text: raw.to_string(), lifecycle: Vec::new() });
    };
    let Some(table) = policy.as_table_mut() else {
        return Ok(NormalizedPolicy { text: raw.to_string(), lifecycle: Vec::new() });
    };

    if let Some(due_review) = table.get("due_review") {
        let Some(due_review) = due_review.as_str() else {
            return Ok(NormalizedPolicy { text: raw.to_string(), lifecycle: Vec::new() });
        };
        if !matches!(due_review, "warn" | "fail") {
            bail!("quality exception due_review must be warn or fail, found {due_review}");
        }
    }
    table.insert("due_review".to_string(), TomlValue::String("warn".to_string()));

    let mut lifecycle = Vec::new();
    if let Some(exceptions) = table.get_mut("exception").and_then(TomlValue::as_array_mut) {
        for exception in exceptions {
            let Some(exception) = exception.as_table_mut() else {
                continue;
            };
            let id = exception.get("id").and_then(TomlValue::as_str).map(str::to_string);
            let review_after =
                exception.get("review_after").and_then(TomlValue::as_str).map(str::to_string);
            let expires = exception.get("expires").and_then(TomlValue::as_str).map(str::to_string);

            let (Some(id), Some(review_after), Some(expires)) = (id, review_after, expires) else {
                continue;
            };
            if parse_date(&review_after).is_none() || parse_date(&expires).is_none() {
                continue;
            }

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
    })
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

fn restore_lifecycle_dates(receipt: &mut JsonValue, lifecycle: &[LifecycleDates]) {
    let Some(exceptions) =
        receipt.get_mut("temporary_exceptions").and_then(JsonValue::as_object_mut)
    else {
        return;
    };
    exceptions.insert("due_review".to_string(), JsonValue::String("advisory".to_string()));
    exceptions
        .insert("lifecycle_authority".to_string(), JsonValue::String("policy_cadence".to_string()));

    let Some(active) = exceptions.get_mut("active").and_then(JsonValue::as_array_mut) else {
        return;
    };
    for entry in active {
        let Some(id) = entry.get("id").and_then(JsonValue::as_str) else {
            continue;
        };
        let Some(dates) = lifecycle.iter().find(|dates| dates.id == id) else {
            continue;
        };
        let Some(entry) = entry.as_object_mut() else {
            continue;
        };
        entry.insert("review_after".to_string(), JsonValue::String(dates.review_after.clone()));
        entry.insert("expires".to_string(), JsonValue::String(dates.expires.clone()));
    }
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
    fn receipt_restores_committed_dates_without_a_clock_input() {
        let mut receipt = json!({
            "temporary_exceptions": {
                "due_review": "warn",
                "active": [{
                    "id": "fixture",
                    "review_after": LIFECYCLE_SENTINEL,
                    "expires": LIFECYCLE_SENTINEL
                }]
            }
        });
        let lifecycle = vec![LifecycleDates {
            id: "fixture".to_string(),
            review_after: "2026-09-16".to_string(),
            expires: "2026-09-30".to_string(),
        }];

        restore_lifecycle_dates(&mut receipt, &lifecycle);

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
        assert_eq!(
            receipt
                .pointer("/temporary_exceptions/active/0/review_after")
                .and_then(JsonValue::as_str),
            Some("2026-09-16")
        );
        assert_eq!(
            receipt.pointer("/temporary_exceptions/active/0/expires").and_then(JsonValue::as_str),
            Some("2026-09-30")
        );
    }
}
