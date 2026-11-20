//! Atomic per-invocation receipt publication (#8191 / #8544).

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use color_eyre::eyre::{Context, Result, bail};

use super::model::SettledInvocationReceipt;

fn receipt_leaf_name(invocation_id: &str) -> Result<String> {
    let leaf = format!("{invocation_id}.json");
    let path = Path::new(&leaf);
    if leaf.is_empty() {
        bail!("invocation id must not be empty");
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            bail!("invocation id must not contain path traversal or separators");
        }
    }
    Ok(leaf)
}

/// Return the canonical receipt path for one invocation within a run tree.
pub fn invocation_receipt_path(
    run_tree: &Path,
    run_id: &str,
    invocation_id: &str,
) -> Result<PathBuf> {
    let leaf = receipt_leaf_name(invocation_id)?;
    Ok(run_tree.join("invocations").join(run_id).join(leaf))
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
