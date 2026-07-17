//! `check-doc-drift` — verify that active status/narrative docs agree with the
//! canonical workspace facts (workspace version + published-crate-allowlist
//! count) held in the root `Cargo.toml`.
//!
//! This is a regression guard for the recurring documentation-drift defect
//! tracked in issue #3023: after a release or a crate-surface change, the
//! human-owned narrative surfaces (`docs/project/status/index.md`,
//! `docs/project/status/release.md`, `docs/project/CURRENT_STATUS.md`) were
//! repeatedly left claiming a stale version or crate count while `Cargo.toml`
//! moved on.
//!
//! Design constraints:
//!
//! - **Canonical source of truth** is the root `Cargo.toml`: the workspace
//!   version (`[workspace.package] version`) and the length of the published
//!   allowlist (`[workspace.metadata.publish] allow`).
//! - **Only active surfaces are scanned.** Historical material — release-notes
//!   files, `RELEASE_HISTORY.md`, the dated per-release receipt sections inside
//!   `release.md`, changelog entries — is intentionally out of scope, so a
//!   line like `0.13.3 (31 crates)` in a past-release receipt is never flagged.
//!   Each claim is anchored to a labeled *current* line (e.g. `**Published
//!   crate surface**`), not to any bare version/count token.
//! - **A moved anchor is a hard failure**, not a silent pass: if an expected
//!   claim marker is present in the repo but its value can no longer be
//!   extracted, the check fails so the guard cannot be silently disabled by a
//!   doc refactor. A missing *file* is skipped (fork-friendly), mirroring
//!   `version_sync`.

use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

/// Semver fragment allowing an optional pre-release suffix (e.g. `0.17.0`,
/// `0.18.0-rc1`). Mirrors the fragment used by `version_sync`.
const VERSION_FRAGMENT: &str = r"\d+\.\d+\.\d+(?:-[A-Za-z0-9][A-Za-z0-9.\-]*)?";

/// Canonical facts read from the root `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanonicalFacts {
    /// `[workspace.package] version`.
    pub version: String,
    /// Number of entries in `[workspace.metadata.publish] allow`.
    pub published_crate_count: usize,
}

/// What a scanned claim asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimKind {
    /// The captured value must equal the canonical workspace version.
    Version,
    /// The captured value (a decimal integer) must equal the published-crate count.
    CrateCount,
}

/// One active-surface claim: a labeled line in a specific file whose value must
/// track a canonical fact.
struct ActiveClaim {
    /// Repo-relative path of the active surface.
    path: &'static str,
    /// Human-readable description of the claim (for the failure report).
    description: &'static str,
    kind: ClaimKind,
    /// Regex with exactly one capture group extracting the claimed value.
    pattern: &'static LazyLock<Regex>,
}

fn compile(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(re) => re,
        // The patterns are compile-time constants; a failure here is a bug.
        Err(err) => unreachable!("internal doc-drift regex must be valid: {err}"),
    }
}

// --- Claim patterns, anchored to labeled "current" lines only. ---

static CURRENT_STATUS_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(r"Workspace version line\*\*\s*\|\s*`v({VERSION_FRAGMENT})`"))
});
static CURRENT_STATUS_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"Published crate surface\*\*\s*\|\s*(\d+)\s+crates"));
static INDEX_VERSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(r"`v({VERSION_FRAGMENT})`\s+is the current workspace version"))
});
static INDEX_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"published crate surface is (\d+)\s+crates"));
static RELEASE_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"Workspace version line\*\*:\s*`v({VERSION_FRAGMENT})`")));
static RELEASE_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"Published crate surface\*\*:\s*(\d+)\s+crates"));

/// The active surfaces and the claims they must keep in sync with `Cargo.toml`.
const ACTIVE_CLAIMS: &[ActiveClaim] = &[
    ActiveClaim {
        path: "docs/project/CURRENT_STATUS.md",
        description: "CURRENT_STATUS.md workspace version line",
        kind: ClaimKind::Version,
        pattern: &CURRENT_STATUS_VERSION_RE,
    },
    ActiveClaim {
        path: "docs/project/CURRENT_STATUS.md",
        description: "CURRENT_STATUS.md published crate surface",
        kind: ClaimKind::CrateCount,
        pattern: &CURRENT_STATUS_COUNT_RE,
    },
    ActiveClaim {
        path: "docs/project/status/index.md",
        description: "status/index.md current workspace version",
        kind: ClaimKind::Version,
        pattern: &INDEX_VERSION_RE,
    },
    ActiveClaim {
        path: "docs/project/status/index.md",
        description: "status/index.md published crate surface",
        kind: ClaimKind::CrateCount,
        pattern: &INDEX_COUNT_RE,
    },
    ActiveClaim {
        path: "docs/project/status/release.md",
        description: "status/release.md workspace version line",
        kind: ClaimKind::Version,
        pattern: &RELEASE_VERSION_RE,
    },
    ActiveClaim {
        path: "docs/project/status/release.md",
        description: "status/release.md published crate surface",
        kind: ClaimKind::CrateCount,
        pattern: &RELEASE_COUNT_RE,
    },
];

/// Read the canonical workspace version and published-crate count from the root
/// `Cargo.toml`.
pub(crate) fn read_canonical_facts(repo_root: &Path) -> Result<CanonicalFacts> {
    let path = repo_root.join("Cargo.toml");
    let raw = fs::read_to_string(&path).map_err(|e| eyre!("reading {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&raw).map_err(|e| eyre!("parsing {}: {e}", path.display()))?;

    let version = value
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| eyre!("Cargo.toml is missing [workspace.package] version"))?
        .to_string();

    let allow = value
        .get("workspace")
        .and_then(|w| w.get("metadata"))
        .and_then(|m| m.get("publish"))
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
        .ok_or_else(|| eyre!("Cargo.toml is missing [workspace.metadata.publish] allow array"))?;
    let published_crate_count = allow.len();

    Ok(CanonicalFacts { version, published_crate_count })
}

/// Outcome of evaluating a single claim against a file's contents.
enum ClaimOutcome {
    /// File not present — skipped (fork-friendly).
    FileMissing,
    /// Claim value matches canonical.
    Ok,
    /// Claim value found but does not match canonical (found, expected).
    Mismatch { found: String, expected: String },
    /// The anchor marker could not be located in an existing file.
    AnchorMissing,
}

/// Evaluate one claim against the on-disk file.
fn evaluate_claim(
    repo_root: &Path,
    facts: &CanonicalFacts,
    claim: &ActiveClaim,
) -> Result<ClaimOutcome> {
    let abs = repo_root.join(claim.path);
    if !abs.is_file() {
        return Ok(ClaimOutcome::FileMissing);
    }
    let raw = fs::read_to_string(&abs).map_err(|e| eyre!("reading {}: {e}", abs.display()))?;
    let Some(caps) = claim.pattern.captures(&raw) else {
        return Ok(ClaimOutcome::AnchorMissing);
    };
    let found = caps
        .get(1)
        .ok_or_else(|| eyre!("claim pattern for {} has no capture group", claim.description))?
        .as_str()
        .to_string();

    let expected = match claim.kind {
        ClaimKind::Version => facts.version.clone(),
        ClaimKind::CrateCount => facts.published_crate_count.to_string(),
    };

    if found == expected {
        Ok(ClaimOutcome::Ok)
    } else {
        Ok(ClaimOutcome::Mismatch { found, expected })
    }
}

/// Run the doc-drift check. Returns exit code `0` when every active claim
/// agrees with the canonical facts, `1` otherwise.
pub(crate) fn check_doc_drift(repo_root: &Path) -> Result<i32> {
    let facts = read_canonical_facts(repo_root)?;

    println!("Documentation drift check:");
    println!("  Canonical workspace version: {}", facts.version);
    println!("  Canonical published crates:  {}", facts.published_crate_count);

    let mut problems: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for claim in ACTIVE_CLAIMS {
        match evaluate_claim(repo_root, &facts, claim)? {
            ClaimOutcome::FileMissing => {
                println!("  [skip] {} — {} not present", claim.description, claim.path);
            }
            ClaimOutcome::Ok => {
                checked += 1;
            }
            ClaimOutcome::Mismatch { found, expected } => {
                problems.push(format!(
                    "  {}:{} — claims {:?}, canonical is {:?}",
                    claim.path, claim.description, found, expected
                ));
            }
            ClaimOutcome::AnchorMissing => {
                problems.push(format!(
                    "  {}:{} — expected claim marker not found; the check anchor moved. \
                     Update crates/perl-ci-hygiene/src/commands/doc_drift.rs",
                    claim.path, claim.description
                ));
            }
        }
    }

    if problems.is_empty() {
        println!("Documentation drift check: {checked} active claim(s) agree with Cargo.toml");
        return Ok(0);
    }

    eprintln!(
        "Documentation drift detected: {} active claim(s) out of sync with Cargo.toml",
        problems.len()
    );
    for problem in &problems {
        eprintln!("{problem}");
    }
    eprintln!(
        "Fix the active status/narrative docs to match the canonical workspace version and \
         published-crate count in Cargo.toml (see issue #3023)."
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn unique_temp_dir(label: &str) -> Result<PathBuf> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| eyre!("system clock before unix epoch: {e}"))?
            .as_nanos();
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "perl-ci-hygiene-doc-drift-{label}-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).map_err(|e| eyre!("creating {}: {e}", dir.display()))?;
        Ok(dir)
    }

    fn cargo_toml(version: &str, crates: &[&str]) -> String {
        let allow = crates.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
        format!(
            "[workspace.package]\nversion = \"{version}\"\n\n\
             [workspace.metadata.publish]\nallow = [{allow}]\n"
        )
    }

    /// A CURRENT_STATUS.md fixture whose active claims use `version`/`count`.
    fn current_status_doc(version: &str, count: usize) -> String {
        format!(
            "# perl-lsp Current Status\n\n\
             | Metric | Value | Source |\n| --- | --- | --- |\n\
             | **Workspace version line** | `v{version}` | [`Cargo.toml`](../../Cargo.toml) |\n\
             | **Published crate surface** | {count} crates | [`allow`](../../Cargo.toml) |\n"
        )
    }

    fn write_current_status(root: &Path, version: &str, count: usize) -> Result<()> {
        let dir = root.join("docs/project");
        fs::create_dir_all(&dir).map_err(|e| eyre!("mkdir: {e}"))?;
        fs::write(dir.join("CURRENT_STATUS.md"), current_status_doc(version, count))
            .map_err(|e| eyre!("write: {e}"))?;
        Ok(())
    }

    #[test]
    fn read_canonical_facts_reads_version_and_count() -> Result<()> {
        let root = unique_temp_dir("facts")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b", "c"]))
            .map_err(|e| eyre!("write: {e}"))?;
        let facts = read_canonical_facts(&root)?;
        assert_eq!(facts.version, "0.17.0");
        assert_eq!(facts.published_crate_count, 3);
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn passes_when_active_claims_match_canonical() -> Result<()> {
        let root = unique_temp_dir("match")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b"]))
            .map_err(|e| eyre!("write: {e}"))?;
        write_current_status(&root, "0.17.0", 2)?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 0, "matching active claims should pass");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn fails_on_stale_version_claim() -> Result<()> {
        let root = unique_temp_dir("stale-version")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b"]))
            .map_err(|e| eyre!("write: {e}"))?;
        // Doc still claims the previous release.
        write_current_status(&root, "0.16.0", 2)?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 1, "stale version claim must fail the check");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn fails_on_stale_crate_count_claim() -> Result<()> {
        let root = unique_temp_dir("stale-count")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b", "c"]))
            .map_err(|e| eyre!("write: {e}"))?;
        // Cargo.toml has 3 published crates; doc still claims 2.
        write_current_status(&root, "0.17.0", 2)?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 1, "stale crate-count claim must fail the check");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn ignores_historical_version_and_count_mentions() -> Result<()> {
        // A release.md-shaped fixture: the *current* claims are correct, but the
        // file also contains historical receipt lines with old versions and an
        // old "(31 crates)" mention. The check must pass — historical mentions
        // are not anchored claims.
        let root = unique_temp_dir("historical")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b"]))
            .map_err(|e| eyre!("write: {e}"))?;
        let dir = root.join("docs/project/status");
        fs::create_dir_all(&dir).map_err(|e| eyre!("mkdir: {e}"))?;
        let release = "\
# Release Readiness

## Current Release Call

**Workspace version line**: `v0.17.0` (Cargo.toml workspace.package.version)
**Published crate surface**: 2 crates

## Historical 0.13.1 Receipts

- crates.io publish run verified all 32 crates at `0.13.1`
- workspace version line is `v0.12.3`; older receipts mention 0.13.3 (31 crates)
";
        fs::write(dir.join("release.md"), release).map_err(|e| eyre!("write: {e}"))?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 0, "historical version/count mentions must not be flagged");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn fails_when_anchor_marker_is_removed() -> Result<()> {
        // File exists but the labeled claim line is gone → the guard must fail
        // rather than silently pass (a refactor must not disable the check).
        let root = unique_temp_dir("anchor-gone")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a", "b"]))
            .map_err(|e| eyre!("write: {e}"))?;
        let dir = root.join("docs/project");
        fs::create_dir_all(&dir).map_err(|e| eyre!("mkdir: {e}"))?;
        // No "Workspace version line" / "Published crate surface" markers.
        fs::write(dir.join("CURRENT_STATUS.md"), "# perl-lsp Current Status\n\nNo claims here.\n")
            .map_err(|e| eyre!("write: {e}"))?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 1, "a moved/removed anchor must fail the check");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }

    #[test]
    fn skips_missing_files() -> Result<()> {
        // Only Cargo.toml present; no docs at all. Fork-friendly: no claims to
        // check → pass.
        let root = unique_temp_dir("no-docs")?;
        fs::write(root.join("Cargo.toml"), cargo_toml("0.17.0", &["a"]))
            .map_err(|e| eyre!("write: {e}"))?;
        let code = check_doc_drift(&root)?;
        assert_eq!(code, 0, "missing active-surface files are skipped, not failed");
        fs::remove_dir_all(&root).ok();
        Ok(())
    }
}
