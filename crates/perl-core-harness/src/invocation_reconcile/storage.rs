//! Atomic per-invocation receipt publication (#8191 / #8544).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use color_eyre::eyre::{Context, Result};

use super::model::SettledInvocationReceipt;

/// Return the canonical receipt path for one invocation within a run tree.
#[must_use]
pub fn invocation_receipt_path(run_tree: &Path, run_id: &str, invocation_id: &str) -> PathBuf {
    run_tree.join("invocations").join(run_id).join(format!("{invocation_id}.json"))
}

/// Write one complete settled receipt atomically; partial files are never published.
pub fn publish_settled_invocation_receipt(
    path: &Path,
    receipt: &SettledInvocationReceipt,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("creating invocation receipt directory {}", parent.display())
        })?;
    }
    let json =
        serde_json::to_string_pretty(receipt).context("serializing settled invocation receipt")?;
    let temp_path = path.with_extension("json.part");
    {
        let mut temp = fs::File::create(&temp_path)
            .with_context(|| format!("creating temporary receipt {}", temp_path.display()))?;
        temp.write_all(json.as_bytes())
            .with_context(|| format!("writing temporary receipt {}", temp_path.display()))?;
        temp.sync_all()
            .with_context(|| format!("syncing temporary receipt {}", temp_path.display()))?;
    }
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "publishing settled invocation receipt {} from {}",
            path.display(),
            temp_path.display()
        )
    })
}
