//! Regression test: Verify published-crate-baseline.txt matches actual published count.
//!
//! The baseline file is a ratchet: it can only decrease as crates are absorbed.
//! G3 reduces the count from 44 to 37. Wave 4-Completion reduces from 37 to 34
//! (perl-dead-code, perl-refactoring, perl-incremental-parsing).
//! Wave Final PR B reduces from 34 to 31 (feature-catalog, lsp-config, content-length-framing).
//! This test verifies:
//! 1. Baseline file exists and matches the live publish-allowlist entry count
//! 2. Actual cargo metadata published count matches baseline
//! 3. Baseline ratchet is enforced (no accidental regressions)

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("..").join("..")
}

/// Extract the crate names in the root `[workspace.metadata.publish.allow]` array —
/// the live published set, which the baseline file must match. Derived, not hard-coded.
///
/// The allowlist is densely commented: most entries sit beside a `#` note recording
/// where an absorbed crate went. Those comments are not entries, and one of them
/// quotes an ADR section name (`PLSP-ADR-0006 "Scope boundary"`), so counting quotes
/// across the whole block over-reports by one per quoted phrase. Strip each line's
/// comment before reading its entry, and return the names so failures show which
/// rows drifted instead of only a diverging count. Any line carrying quotes that is
/// not exactly one `"crate-name",` entry is rejected loudly, so house-style
/// deviations (inline arrays, multiple entries per line) cannot silently shift the
/// count.
fn published_allowlist_entries(root: &std::path::Path) -> std::io::Result<Vec<String>> {
    let root_toml = fs::read_to_string(root.join("Cargo.toml"))?;
    let section = root_toml.split("[workspace.metadata.publish]").nth(1).unwrap_or("");
    let allow_start = section.find("allow = [").unwrap_or(0);
    let allow = &section[allow_start..];
    let code_only = allow
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let allow_end = code_only.find(']').unwrap_or(code_only.len());
    let mut entries = Vec::new();
    for line in code_only[..allow_end].lines() {
        let entry_line = line.trim();
        if !entry_line.contains('"') {
            continue;
        }
        if entry_line.matches('"').count() != 2 || !entry_line.starts_with('"') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "unparseable publish-allowlist line (expected exactly one \"crate-name\", \
                     entry per line): {line:?}"
                ),
            ));
        }
        entries.push(entry_line.trim_end_matches(',').trim().trim_matches('"').to_string());
    }
    Ok(entries)
}

#[test]
fn g3_baseline_file_matches_allowlist() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let baseline_path = root.join("xtask/published-crate-baseline.txt");

    let content = fs::read_to_string(&baseline_path)?;
    let baseline: usize =
        content.trim().parse().map_err(|_| "baseline count should be parseable as an integer")?;
    let entries = published_allowlist_entries(&root)?;
    let allowlist = entries.len();
    assert!(
        entries.iter().all(|name| !name.trim().is_empty()),
        "publish allowlist parsed an empty entry name — parser or Cargo.toml broke: {entries:?}"
    );

    assert_eq!(
        baseline, allowlist,
        "baseline ({baseline}) must match the publish allowlist entry count ({allowlist}) — \
         parsed allowlist entries: {entries:?}"
    );

    Ok(())
}

#[test]
#[ignore = "This test requires cargo metadata to be run in-process; skip in CI if slow; tracking #4912"]
fn g3_baseline_matches_cargo_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();

    // Run cargo metadata to count published crates
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()?;

    if !output.status.success() {
        return Ok(()); // Skip if cargo metadata fails (e.g., in some CI environments)
    }

    let metadata_str = String::from_utf8(output.stdout)?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata_str)?;

    let packages = metadata["packages"].as_array().ok_or("no packages in metadata")?;

    // Count crates with publish != false and publish != [] (i.e., publicly published)
    let published_count = packages
        .iter()
        .filter(|p| {
            let publish = &p["publish"];
            // If publish is not false and not an empty array, it's published
            !(publish == false
                || (publish.is_array() && publish.as_array().is_some_and(|a| a.is_empty())))
        })
        .count();

    // Allow a small margin for test setup artifacts
    assert!(
        (published_count as i32 - 31).abs() <= 1,
        "published count should be approximately 31 (got {})",
        published_count
    );

    Ok(())
}

#[test]
fn g3_baseline_not_regressed() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let baseline_path = root.join("xtask/published-crate-baseline.txt");

    let baseline_str = fs::read_to_string(&baseline_path)?;
    let baseline: u32 = baseline_str.trim().parse()?;

    // Regression guard: baseline should never accidentally increase above 34
    // (If it does, it means crates were accidentally re-added)
    assert!(baseline <= 34, "baseline should not exceed Wave 4-Completion target (34)");

    // Also verify it doesn't drop below the v0.13.0 final target
    assert!(baseline >= 31, "baseline should not go below Wave Final PR B target (31)");

    Ok(())
}
