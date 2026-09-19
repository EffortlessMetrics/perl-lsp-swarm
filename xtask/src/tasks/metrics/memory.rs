//! Memory plateau receipt and summary helpers.

use chrono::Utc;
use color_eyre::eyre::{Context, Result, eyre};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Options for `cargo xtask metrics memory`.
pub struct MemoryMetricsConfig {
    pub scenario: String,
    pub workload_json: PathBuf,
    pub plateau_json: PathBuf,
    pub receipt: Option<PathBuf>,
    pub commit: Option<String>,
    pub event: String,
    pub markdown: bool,
}

#[derive(Debug, Deserialize)]
struct WorkloadPayload {
    n_files: u64,
    n_changes: u64,
    #[serde(default)]
    workspace_symbol: bool,
    #[serde(default)]
    delete_after_close: bool,
    #[serde(default)]
    settle_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct PlateauSummary {
    samples: u64,
    tail_growth_kb: i64,
    tail_growth_pct: f64,
    median_tail_slope_kb_per_file: f64,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct MemoryPlateauReceipt {
    check: &'static str,
    kind: &'static str,
    schema_version: u32,
    event: String,
    verdict: &'static str,
    scenario: String,
    files: u64,
    changes_per_file: u64,
    workspace_symbol: bool,
    delete_after_close: bool,
    settle_seconds: f64,
    tail_growth_kb: i64,
    tail_growth_pct: f64,
    median_tail_slope_kb_per_file: f64,
    samples: u64,
    passed: bool,
    commit: Option<String>,
    generated_at: String,
    metrics: serde_json::Value,
    artifacts: Vec<serde_json::Value>,
}

/// Run `cargo xtask metrics memory`.
pub fn run(config: MemoryMetricsConfig) -> Result<()> {
    let workload = read_json::<WorkloadPayload>(&config.workload_json)?;
    let plateau = read_json::<PlateauSummary>(&config.plateau_json)?;

    let receipt = MemoryPlateauReceipt {
        check: "memory-plateau",
        kind: "memory_plateau",
        schema_version: 1,
        event: config.event,
        verdict: if plateau.passed { "pass" } else { "fail" },
        scenario: config.scenario,
        files: workload.n_files,
        changes_per_file: workload.n_changes,
        workspace_symbol: workload.workspace_symbol,
        delete_after_close: workload.delete_after_close,
        settle_seconds: workload.settle_seconds,
        tail_growth_kb: plateau.tail_growth_kb,
        tail_growth_pct: plateau.tail_growth_pct,
        median_tail_slope_kb_per_file: plateau.median_tail_slope_kb_per_file,
        samples: plateau.samples,
        passed: plateau.passed,
        commit: config.commit,
        generated_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        metrics: json!({
            "tail_growth_kb": plateau.tail_growth_kb,
            "tail_growth_pct": plateau.tail_growth_pct,
            "median_tail_slope_kb_per_file": plateau.median_tail_slope_kb_per_file,
            "samples": plateau.samples,
        }),
        artifacts: vec![
            json!({
                "kind": "workload_json",
                "path": config.workload_json,
            }),
            json!({
                "kind": "plateau_summary",
                "path": config.plateau_json,
            }),
        ],
    };

    if let Some(path) = &config.receipt {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .wrap_err_with(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, serde_json::to_string_pretty(&receipt)? + "\n")
            .wrap_err_with(|| format!("failed to write {}", path.display()))?;
    }

    if config.markdown {
        print_markdown(&receipt);
    } else {
        println!("{}", serde_json::to_string_pretty(&receipt)?);
    }

    Ok(())
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let text =
        fs::read_to_string(path).wrap_err_with(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).wrap_err_with(|| format!("invalid JSON in {}", path.display()))
}

fn print_markdown(receipt: &MemoryPlateauReceipt) {
    println!("## Memory plateau receipt");
    println!();
    println!(
        "| Scenario | Files | Changes/file | Tail growth KB | Median tail slope KB/file | Result |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | --- |");
    println!(
        "| {} | {} | {} | {} | {:.3} | {} |",
        receipt.scenario,
        receipt.files,
        receipt.changes_per_file,
        receipt.tail_growth_kb,
        receipt.median_tail_slope_kb_per_file,
        if receipt.passed { "passed" } else { "failed" }
    );
}

pub fn infer_scenario(workload_json: &Path) -> Result<String> {
    let name = workload_json
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| eyre!("cannot infer scenario from {}", workload_json.display()))?;

    match name {
        "nightly-doc-churn" | "doc_churn_500_delete" => Ok("lsp_doc_churn_delete".to_string()),
        "nightly-workspace-symbol" | "workspace_symbol_300_delete" => {
            Ok("lsp_workspace_symbol_churn_delete".to_string())
        }
        "pr-smoke-doc-churn" => Ok("lsp_doc_churn_delete_smoke".to_string()),
        other => Ok(other.replace('-', "_")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `memory_plateau` receipt must emit `schema_version` as a JSON number
    /// (`1`) to match the metrics family (`parser_accuracy.rs:1271`,
    /// `release_health.rs:667`, `lsp_stats.rs`, `ratchet.rs`). Stringified
    /// versions like `"1"` break string-equality consumers and break the
    /// peer-alignment test in `quality_ci_wiring_policy.rs`. See #15358.
    #[test]
    fn memory_plateau_receipt_emits_numeric_schema_version_one() -> Result<()> {
        let temp = tempfile::tempdir().wrap_err_with(|| {
            format!("failed to create tempdir at {}", std::env::temp_dir().display())
        })?;
        let workload = temp.path().join("nightly-doc-churn.json");
        let plateau = temp.path().join("plateau.json");
        let receipt = temp.path().join("memory.receipt.json");

        fs::write(
            &workload,
            r#"{"n_files":500,"n_changes":10,"workspace_symbol":false,"delete_after_close":false,"settle_seconds":2.5}"#,
        )
        .wrap_err_with(|| format!("failed to write {}", workload.display()))?;
        fs::write(
            &plateau,
            r#"{"samples":12,"tail_growth_kb":152,"tail_growth_pct":0.012,"median_tail_slope_kb_per_file":0.69,"passed":true}"#,
        )
        .wrap_err_with(|| format!("failed to write {}", plateau.display()))?;

        let config = MemoryMetricsConfig {
            scenario: "lsp_doc_churn_delete".to_string(),
            workload_json: workload.clone(),
            plateau_json: plateau.clone(),
            receipt: Some(receipt.clone()),
            commit: Some("abc123".to_string()),
            event: "local".to_string(),
            markdown: false,
        };
        run(config)?;

        let raw = fs::read_to_string(&receipt)
            .wrap_err_with(|| format!("failed to read {}", receipt.display()))?;
        let value: serde_json::Value = serde_json::from_str(&raw)
            .wrap_err_with(|| format!("invalid JSON in {}", receipt.display()))?;

        assert_eq!(
            value["schema_version"],
            serde_json::Value::from(1u32),
            "memory_plateau receipt must emit numeric schema_version == 1 to match metrics family; got {}",
            value["schema_version"]
        );
        assert!(
            value["schema_version"].is_number(),
            "memory_plateau receipt schema_version must be a JSON number, not a string"
        );
        assert_eq!(value["kind"], serde_json::Value::from("memory_plateau"));
        Ok(())
    }
}
