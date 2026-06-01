use color_eyre::eyre::{Result, bail};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

pub(super) fn git_changed_files(base: &str, head: &str) -> Result<Vec<String>> {
    let output =
        Command::new("git").args(["diff", "--name-only", &format!("{base}...{head}")]).output()?;
    if !output.status.success() {
        bail!(
            "git diff --name-only {base}...{head} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw = String::from_utf8(output.stdout)?;
    Ok(normalize_changed_files(raw.lines().map(ToString::to_string).collect()))
}

pub(super) fn normalize_changed_files(files: Vec<String>) -> Vec<String> {
    files
        .into_iter()
        .map(|file| file.replace('\\', "/"))
        .filter(|file| !file.trim().is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn write_receipt<T: Serialize>(path: &Path, receipt: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(receipt)?;
    write_text(path, &format!("{json}\n"))
}

pub(super) fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}
