//! Merge-topology regression for #17995 (#13362 × #17053).
//!
//! The issue originally claimed `xtask/src/public_release_claims.rs` auto-merges
//! silently and can yield duplicate `pub const SCHEMA_VERSION` (E0428). Issue
//! comment 6214179893 corrected that model: merging the two PR heads is an
//! add/add conflict on this file (and the schema JSON), not a clean merge.
//!
//! These tests pin the corrected topology at the pinned census heads so a worker
//! cannot reintroduce the "silent per-file auto-merge" story. They do **not**
//! adjudicate which open PR should own `SCHEMA_VERSION`; that remains a product
//! decision between #13362 and #17053 while both stay open.

use color_eyre::eyre::{Context, Result, ensure, eyre};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// PR #13362 head at 2026-10-16 census (`feat/11548-inventory-derivative`).
const PR_13362_HEAD: &str = "5af03f99e701aaf8b9d630c960d80b65fd37dde5";
/// PR #17053 head at 2026-10-16 census (`ux/issue-8087`).
const PR_17053_HEAD: &str = "922b3e58d60fe82fab1d58a4c80a782aacea7e3b";

const TARGET_RS: &str = "xtask/src/public_release_claims.rs";
const TARGET_SCHEMA: &str = "schemas/public_release_claims.v2.schema.json";
const SCHEMA_VERSION_MARKER: &str = "pub const SCHEMA_VERSION";

fn project_root() -> Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| eyre!("xtask must live below the repository root"))
}

fn git(root: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))
}

fn git_ok(root: &Path, args: &[&str]) -> Result<Output> {
    let output = git(root, args)?;
    if output.status.success() {
        return Ok(output);
    }
    Err(eyre!(
        "git {} failed (status={:?}) stderr=`{}` stdout=`{}`",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

fn fetch_commit(root: &Path, sha: &str) -> Result<()> {
    if git_ok(root, &["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_ok() {
        return Ok(());
    }
    git_ok(root, &["fetch", "--quiet", "origin", sha])
        .with_context(|| format!("fetching {sha}"))?;
    Ok(())
}

fn origin_main_rev(root: &Path) -> Result<String> {
    for reference in ["origin/main", "main"] {
        if git_ok(root, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")]).is_ok() {
            return git_output(root, &["rev-parse", reference]);
        }
    }
    git_ok(root, &["fetch", "--quiet", "origin", "main"]).context("fetching origin/main")?;
    git_output(root, &["rev-parse", "origin/main"])
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = git_ok(root, args)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .context("git output must be UTF-8")
}

fn ensure_census_objects(root: &Path) -> Result<()> {
    fetch_commit(root, PR_13362_HEAD)?;
    fetch_commit(root, PR_17053_HEAD)?;
    let _ = origin_main_rev(root)?;
    Ok(())
}

fn merge_tree(root: &Path, base: &str, head: &str) -> Result<(i32, String)> {
    let output = git(root, &["merge-tree", "--write-tree", base, head])?;
    let code = output.status.code().unwrap_or(-1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok((code, combined))
}

fn grep_count(root: &Path, rev: &str, path: &str, pattern: &str) -> Result<usize> {
    let output = git(root, &["grep", "-c", pattern, rev, "--", path])?;
    if output.status.success() {
        let line = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let count = line.rsplit(':').next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
        return Ok(count);
    }
    if output.status.code() == Some(1) {
        return Ok(0);
    }
    Err(eyre!("git grep failed: stderr=`{}`", String::from_utf8_lossy(&output.stderr).trim()))
}

fn path_exists_at(root: &Path, rev: &str, path: &str) -> Result<bool> {
    Ok(git_ok(root, &["cat-file", "-e", &format!("{rev}:{path}")]).is_ok())
}

/// Parse `merge-tree` unmerged lines: `mode blob stage\tpath`.
fn unmerged_blob_stages(text: &str, path: &str) -> Result<(Option<String>, Option<String>)> {
    let mut stage2 = None;
    let mut stage3 = None;
    for line in text.lines() {
        if !line.contains(path) {
            continue;
        }
        let Some((meta, file)) = line.split_once('\t') else {
            continue;
        };
        if file.trim() != path {
            continue;
        }
        let parts: Vec<_> = meta.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let stage = parts[2];
        let blob = parts[1].to_string();
        match stage {
            "2" => stage2 = Some(blob),
            "3" => stage3 = Some(blob),
            _ => {}
        }
    }
    Ok((stage2, stage3))
}

fn blob_text(root: &Path, blob: &str) -> Result<String> {
    let output = git_ok(root, &["cat-file", "-p", blob])?;
    String::from_utf8(output.stdout).context("blob must be UTF-8")
}

#[test]
fn main_still_lacks_public_release_claims_module() -> Result<()> {
    let root = project_root()?;
    ensure_census_objects(&root)?;
    let main_rev = origin_main_rev(&root)?;
    ensure!(
        !path_exists_at(&root, &main_rev, TARGET_RS)?,
        "origin/main must not yet carry {TARGET_RS}; both PRs are still add/add creators"
    );
    ensure!(
        grep_count(&root, &main_rev, TARGET_RS, SCHEMA_VERSION_MARKER)? == 0,
        "origin/main must not define SCHEMA_VERSION in {TARGET_RS}"
    );
    Ok(())
}

#[test]
fn each_census_head_defines_schema_version_once() -> Result<()> {
    let root = project_root()?;
    ensure_census_objects(&root)?;
    for (label, sha) in [("13362", PR_13362_HEAD), ("17053", PR_17053_HEAD)] {
        ensure!(path_exists_at(&root, sha, TARGET_RS)?, "PR #{label} head must create {TARGET_RS}");
        ensure!(
            grep_count(&root, sha, TARGET_RS, SCHEMA_VERSION_MARKER)? == 1,
            "PR #{label} head must define SCHEMA_VERSION exactly once"
        );
    }
    Ok(())
}

#[test]
fn pairwise_merge_is_add_add_not_silent_auto_merge() -> Result<()> {
    let root = project_root()?;
    ensure_census_objects(&root)?;
    let (code, text) = merge_tree(&root, PR_13362_HEAD, PR_17053_HEAD)?;
    ensure!(code != 0, "merging the two PR heads must not succeed cleanly");
    ensure!(
        text.contains(&format!("CONFLICT (add/add): Merge conflict in {TARGET_RS}")),
        "expected add/add on {TARGET_RS}; merge-tree output:\n{text}"
    );
    ensure!(
        text.contains(&format!("CONFLICT (add/add): Merge conflict in {TARGET_SCHEMA}")),
        "expected add/add on {TARGET_SCHEMA}; merge-tree output:\n{text}"
    );
    Ok(())
}

#[test]
fn careless_add_add_resolution_duplicates_schema_version() -> Result<()> {
    let root = project_root()?;
    ensure_census_objects(&root)?;
    let (_, text) = merge_tree(&root, PR_13362_HEAD, PR_17053_HEAD)?;
    let (stage2, stage3) = unmerged_blob_stages(&text, TARGET_RS)?;
    let stage2 = stage2.ok_or_else(|| eyre!("missing stage-2 blob for {TARGET_RS}"))?;
    let stage3 = stage3.ok_or_else(|| eyre!("missing stage-3 blob for {TARGET_RS}"))?;
    let side_a = blob_text(&root, &stage2)?;
    let side_b = blob_text(&root, &stage3)?;
    let merged = format!("{side_a}\n{side_b}");
    let duplicates = merged.matches(SCHEMA_VERSION_MARKER).count();
    ensure!(
        duplicates >= 2,
        "keeping both add/add sides must duplicate SCHEMA_VERSION (got {duplicates})"
    );
    Ok(())
}

#[test]
fn main_plus_13362_does_not_conflict_on_public_release_claims_rs() -> Result<()> {
    let root = project_root()?;
    ensure_census_objects(&root)?;
    let main_rev = origin_main_rev(&root)?;
    let (_, text) = merge_tree(&root, &main_rev, PR_13362_HEAD)?;
    ensure!(
        !text.contains(&format!("CONFLICT (add/add): Merge conflict in {TARGET_RS}"))
            && !text.contains(&format!("CONFLICT (content): Merge conflict in {TARGET_RS}")),
        "main × #13362 must not surface a conflict on {TARGET_RS}; residual hazard is \
         sequential landing after the other PR, not silent per-file auto-merge here.\n{text}"
    );
    Ok(())
}
