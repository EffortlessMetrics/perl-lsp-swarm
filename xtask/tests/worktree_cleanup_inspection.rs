use color_eyre::eyre::{Result, bail, eyre};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;
use walkdir::WalkDir;
use xtask::worktree_cleanup::{
    InspectOptions, WorktreeActionKind, WorktreeClassification, inspect_with_options,
    render_human,
};

#[derive(Debug)]
struct FixtureRepository {
    _temporary: TempDir,
    root: PathBuf,
    dirty_worktree: PathBuf,
    missing_worktree: PathBuf,
    clean_worktree: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
struct RepositorySnapshot {
    worktree_list: Vec<u8>,
    administrative_tree: BTreeMap<String, String>,
    refs: Vec<u8>,
    config: Vec<u8>,
    working_files: BTreeMap<String, String>,
}

impl FixtureRepository {
    fn create() -> Result<Self> {
        let temporary = TempDir::new()?;
        let root = temporary.path().join("repo");
        run_git(
            temporary.path(),
            &["init", "-b", "main", path_text(&root)?],
        )?;
        run_git(&root, &["config", "user.name", "EffortlessSteven"])?;
        run_git(
            &root,
            &["config", "user.email", "git@effortlesssteven.com"],
        )?;
        fs::write(root.join("seed.pl"), "use strict;\n")?;
        run_git(&root, &["add", "seed.pl"])?;
        run_git(&root, &["commit", "-m", "seed"])?;

        let managed_root = root.join(".claude").join("worktrees");
        fs::create_dir_all(&managed_root)?;
        let dirty_worktree = managed_root.join("dirty");
        let missing_worktree = managed_root.join("missing");
        let clean_worktree = managed_root.join("clean");

        add_worktree(&root, &dirty_worktree, "fixture/dirty")?;
        add_worktree(&root, &missing_worktree, "fixture/missing")?;
        add_worktree(&root, &clean_worktree, "fixture/clean")?;
        fs::write(dirty_worktree.join("untracked.txt"), "preserve me\n")?;
        fs::remove_dir_all(&missing_worktree)?;

        Ok(Self {
            _temporary: temporary,
            root,
            dirty_worktree,
            missing_worktree,
            clean_worktree,
        })
    }

    fn snapshot(&self) -> Result<RepositorySnapshot> {
        Ok(RepositorySnapshot {
            worktree_list: git_output(&self.root, &["worktree", "list", "--porcelain"])?.stdout,
            administrative_tree: snapshot_tree(&self.root.join(".git").join("worktrees"))?,
            refs: git_output(&self.root, &["show-ref"])?.stdout,
            config: git_output(&self.root, &["config", "--local", "--list"])?.stdout,
            working_files: snapshot_working_files(&[
                &self.root,
                &self.dirty_worktree,
                &self.clean_worktree,
            ])?,
        })
    }
}

#[test]
fn inspection_is_read_only_and_preserves_typed_uncertainty() -> Result<()> {
    let fixture = FixtureRepository::create()?;
    let before = fixture.snapshot()?;
    let options = offline_options();
    let first = inspect_with_options(&fixture.root, "2026-08-16T20:00:00Z", &options)?;
    let after = fixture.snapshot()?;
    assert_eq!(before, after);

    let dirty = entry_for(&first, &fixture.dirty_worktree)?;
    assert_eq!(dirty.classification, WorktreeClassification::Salvage);
    assert!(
        dirty
            .reason_tokens
            .iter()
            .any(|reason| reason == "untracked_work_present")
    );

    let missing = entry_for(&first, &fixture.missing_worktree)?;
    assert_eq!(missing.classification, WorktreeClassification::Review);
    assert!(missing.proposed_action.as_ref().is_some_and(|action| {
        action.kind == WorktreeActionKind::PruneAdministrativeRecord && !action.targetable
    }));
    assert!(!fixture.missing_worktree.exists());

    let clean = entry_for(&first, &fixture.clean_worktree)?;
    assert_eq!(clean.classification, WorktreeClassification::NotProven);
    assert!(
        clean
            .reason_tokens
            .iter()
            .any(|reason| reason == "open_pr_not_proven")
    );

    let second = inspect_with_options(&fixture.root, "2026-08-16T21:00:00Z", &options)?;
    assert_eq!(first.plan_digest, second.plan_digest);
    assert_ne!(first.observed_at, second.observed_at);

    let human = render_human(&first);
    let json = serde_json::to_string(&first)?;
    for entry in &first.entries {
        assert!(human.contains(&entry.entry_id));
        assert!(human.contains(entry.classification.as_str()));
        assert!(json.contains(&entry.entry_id));
        assert!(json.contains(entry.classification.as_str()));
    }
    Ok(())
}

fn offline_options() -> InspectOptions {
    InspectOptions {
        git_program: PathBuf::from("git"),
        gh_program: PathBuf::from("definitely-not-a-real-gh-binary-for-worktree-tests"),
    }
}

fn entry_for<'a>(
    plan: &'a xtask::worktree_cleanup::WorktreeCleanupPlan,
    path: &Path,
) -> Result<&'a xtask::worktree_cleanup::WorktreePlanEntry> {
    plan.entries
        .iter()
        .find(|entry| entry.path == path)
        .ok_or_else(|| eyre!("plan did not contain {}", path.display()))
}

fn add_worktree(root: &Path, path: &Path, branch: &str) -> Result<()> {
    run_git(
        root,
        &["worktree", "add", "-b", branch, path_text(path)?],
    )
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| eyre!("fixture path was not UTF-8: {}", path.display()))
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<()> {
    let output = git_output(cwd, args)?;
    if !output.status.success() {
        bail!(
            "git {} failed in {}: {}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn git_output(cwd: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(Into::into)
}

fn snapshot_tree(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut snapshot = BTreeMap::new();
    if !root.exists() {
        return Ok(snapshot);
    }
    for entry in WalkDir::new(root).sort_by_file_name() {
        let entry = entry?;
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| eyre!("snapshot path error: {error}"))?;
        let key = relative.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            snapshot.insert(format!("dir:{key}"), String::new());
        } else if entry.file_type().is_file() {
            snapshot.insert(format!("file:{key}"), digest_file(entry.path())?);
        } else if entry.file_type().is_symlink() {
            snapshot.insert(
                format!("symlink:{key}"),
                fs::read_link(entry.path())?.to_string_lossy().to_string(),
            );
        }
    }
    Ok(snapshot)
}

fn snapshot_working_files(roots: &[&Path]) -> Result<BTreeMap<String, String>> {
    let mut snapshot = BTreeMap::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root).sort_by_file_name() {
            let entry = entry?;
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| eyre!("working snapshot path error: {error}"))?;
            if relative
                .components()
                .next()
                .is_some_and(|component| component.as_os_str() == ".git")
            {
                continue;
            }
            if entry.file_type().is_file() {
                let key = format!(
                    "{}:{}",
                    root.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("root"),
                    relative.to_string_lossy().replace('\\', "/")
                );
                snapshot.insert(key, digest_file(entry.path())?);
            }
        }
    }
    Ok(snapshot)
}

fn digest_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path)?;
    let digest = Sha256::digest(bytes);
    Ok(hex(&digest))
}

fn hex(raw: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(raw.len() * 2);
    for byte in raw {
        output.push(char::from(HEX[usize::from(*byte >> 4)]));
        output.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    output
}
