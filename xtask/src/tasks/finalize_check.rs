use color_eyre::eyre::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

use crate::tasks::aggregate_receipts::{
    AggregatorReceipt, Classification, SCHEMA_VERSION, Verdict, evaluate_receipt,
};

#[derive(Debug, Clone)]
pub struct FinalizeCheckConfig {
    pub receipt: PathBuf,
    pub allow_noop: bool,
    pub fail_on_advisory: bool,
}

pub fn run(config: FinalizeCheckConfig) -> Result<()> {
    let body = fs::read_to_string(&config.receipt)
        .with_context(|| format!("failed to read receipt {}", config.receipt.display()))?;
    let receipt: AggregatorReceipt =
        serde_json::from_str(&body).context("failed to parse aggregator receipt JSON")?;
    if receipt.schema_version != SCHEMA_VERSION {
        bail!(
            "unsupported aggregator receipt schema_version: {} (expected {})",
            receipt.schema_version,
            SCHEMA_VERSION
        );
    }

    let (mut verdict, mut classification) =
        evaluate_receipt(&receipt.subreceipts, &receipt.missing_receipts, config.allow_noop);

    if verdict == Verdict::Pass && config.fail_on_advisory {
        let advisory_failed = receipt
            .subreceipts
            .iter()
            .any(|sub| !sub.required && matches!(sub.verdict, Verdict::Fail | Verdict::Warn));
        if advisory_failed {
            verdict = Verdict::Fail;
            classification = Classification::Unknown;
        }
    }

    println!(
        "finalized check={} verdict={:?} classification={:?}",
        receipt.check, verdict, classification
    );

    if verdict == Verdict::Fail {
        bail!("finalize-check failed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::aggregate_receipts::SCHEMA_VERSION as EXPECTED_SCHEMA_VERSION;
    use color_eyre::eyre::ContextCompat;
    use serde_json::json;
    use std::path::Path;
    use tempfile::tempdir;

    fn valid_receipt_json() -> serde_json::Value {
        json!({
            "check": "quality-gate",
            "schema_version": EXPECTED_SCHEMA_VERSION,
            "event": "pull_request",
            "verdict": "pass",
            "classification": "unknown",
            "subreceipts": [
                {
                    "name": "rust",
                    "required": true,
                    "verdict": "pass",
                    "classification": "unknown"
                }
            ],
            "missing_receipts": [],
            "repro": { "command": "cargo xtask aggregate-receipts" }
        })
    }

    fn write_receipt(dir: &Path, receipt: &serde_json::Value) -> Result<PathBuf> {
        let path = dir.join("aggregate.json");
        fs::write(&path, serde_json::to_string_pretty(receipt)?)
            .context("write aggregator receipt fixture")?;
        Ok(path)
    }

    fn finalize(path: PathBuf) -> Result<()> {
        run(FinalizeCheckConfig { receipt: path, allow_noop: true, fail_on_advisory: false })
    }

    #[test]
    fn receipt_with_current_schema_version_finalizes_pass() -> Result<()> {
        let dir = tempdir().context("create finalize receipt fixture dir")?;
        let path = write_receipt(dir.path(), &valid_receipt_json())?;

        finalize(path)?;

        Ok(())
    }

    #[test]
    fn receipt_with_future_schema_version_is_refused() -> Result<()> {
        let dir = tempdir().context("create finalize receipt fixture dir")?;
        let mut receipt = valid_receipt_json();
        receipt["schema_version"] = json!(EXPECTED_SCHEMA_VERSION + 1);
        let path = write_receipt(dir.path(), &receipt)?;

        let err =
            finalize(path).expect_err("future aggregator receipt schema_version must be refused");

        assert!(
            err.to_string().contains("unsupported aggregator receipt schema_version"),
            "unexpected refusal: {err}"
        );
        Ok(())
    }

    #[test]
    fn receipt_with_legacy_string_schema_version_is_refused() -> Result<()> {
        let dir = tempdir().context("create finalize receipt fixture dir")?;
        let mut receipt = valid_receipt_json();
        receipt["schema_version"] = json!(EXPECTED_SCHEMA_VERSION.to_string());
        let path = write_receipt(dir.path(), &receipt)?;

        let err = finalize(path)
            .expect_err("legacy string-typed aggregator receipt schema_version must be refused");

        assert!(
            err.to_string().contains("failed to parse aggregator receipt JSON"),
            "unexpected refusal: {err}"
        );
        Ok(())
    }

    #[test]
    fn receipt_with_missing_schema_version_is_refused() -> Result<()> {
        let dir = tempdir().context("create finalize receipt fixture dir")?;
        let mut receipt = valid_receipt_json();
        receipt
            .as_object_mut()
            .context("aggregator receipt fixture must be a JSON object")?
            .remove("schema_version");
        let path = write_receipt(dir.path(), &receipt)?;

        let err =
            finalize(path).expect_err("aggregator receipt without schema_version must be refused");

        assert!(
            err.to_string().contains("failed to parse aggregator receipt JSON"),
            "unexpected refusal: {err}"
        );
        Ok(())
    }
}
