//! Git children operating on an explicitly selected repository.
//!
//! A hook's Git-local environment overrides both `-C` and `current_dir`.
//! Foreign-repository operations must use the selected repository's own refs,
//! index, objects and configuration. SSH selectors and global/system Git
//! configuration remain inherited; call-site network consent and watchdogs are
//! unchanged. Current-repository staged checks deliberately do not use this seam.

use std::process::Command;

// Git's `rev-parse --local-env-vars` inventory, also checked against the native
// executable below. Removing CONFIG_COUNT makes KEY_n/VALUE_n pairs inert.
const LOCAL_ENV: &[&str] = &[
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// Isolate a child that selects its own repositories from inherited Git context.
///
/// Apply immediately after constructing fixture Git commands, before deliberate
/// per-command overrides. Do not apply to current-repository staged operations.
/// SSH selectors and global/system Git configuration remain unchanged.
pub fn isolate(mut command: Command) -> Command {
    for name in LOCAL_ENV {
        command.env_remove(name);
    }
    command
}

/// Isolate a configured duct expression that selects a foreign repository.
///
/// Duct applies outer environment modifiers first. Wrap the configured
/// expression last; deliberate Git-local overrides must be inside this wrapper.
/// Current-repository staged operations deliberately retain their context.
pub fn isolate_duct(mut command: duct::Expression) -> duct::Expression {
    for name in LOCAL_ENV {
        command = command.env_remove(name);
    }
    command
}

pub(crate) fn command() -> Command {
    isolate(Command::new("git"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, ensure};
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};

    const PROBE_ROOT: &str = "XTASK_EXPLICIT_GIT_REPOSITORY_PROBE_ROOT";
    const TEST_NAME: &str =
        "git_environment::tests::explicit_repository_ignores_inherited_git_context";

    fn git(root: &Path, args: &[&str]) -> Result<String> {
        let output = command().current_dir(root).args(args).output()?;
        ensure!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn initialize(root: &Path, name: &str) -> Result<String> {
        fs::create_dir(root)?;
        git(root, &["init", "--quiet"])?;
        git(root, &["config", "user.name", name])?;
        git(root, &["config", "user.email", "fixture@example.invalid"])?;
        git(root, &["config", "commit.gpgsign", "false"])?;
        git(root, &["config", "core.hooksPath", ".git/fixture-hooks"])?;
        fs::write(root.join("value.txt"), format!("{name}\n"))?;
        git(root, &["add", "."])?;
        git(root, &["commit", "--quiet", "-m", name])?;
        git(root, &["rev-parse", "HEAD"])
    }

    fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
        let mut files = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    pending.push(path);
                } else {
                    files.insert(path.strip_prefix(root)?.to_path_buf(), fs::read(path)?);
                }
            }
        }
        Ok(files)
    }

    fn probe(root: &Path) -> Result<()> {
        let target = root.join("target");
        let head = initialize(&target, "Target Fixture")?;
        ensure!(git(&target, &["config", "user.name"])? == "Target Fixture");
        let caller_head = fs::read_to_string(root.join("caller-head"))?;
        ensure!(head != caller_head);
        let foreign_object =
            command().current_dir(&target).args(["cat-file", "-e", &caller_head]).output()?;
        ensure!(!foreign_object.status.success(), "borrowed the caller's object database");
        let receipt = xtask::git_ancestry::classify_ancestry(&target, "HEAD", "HEAD");
        ensure!(receipt.disposition == xtask::git_ancestry::AncestryDisposition::Ancestor);
        fs::write(root.join("probe-complete"), head)?;
        Ok(())
    }

    #[test]
    fn explicit_repository_ignores_inherited_git_context() -> Result<()> {
        if let Some(root) = std::env::var_os(PROBE_ROOT) {
            return probe(Path::new(&root));
        }
        for mode in ["hook", "objects", "config"] {
            let temp = tempfile::tempdir()?;
            let root = temp.path();
            let caller = root.join("caller");
            fs::write(root.join("caller-head"), initialize(&caller, "Caller Guardian")?)?;
            fs::write(caller.join("value.txt"), "staged\n")?;
            git(&caller, &["add", "."])?;
            fs::write(caller.join("value.txt"), "unstaged\n")?;
            fs::write(caller.join("untracked.txt"), "unique content\n")?;
            let before = snapshot(&caller)?;
            let git_dir = caller.join(".git");
            let mut child = Command::new(std::env::current_exe()?);
            child.args(["--exact", TEST_NAME, "--nocapture"]).env(PROBE_ROOT, root);
            for name in LOCAL_ENV {
                child.env_remove(name);
            }
            match mode {
                "hook" => {
                    child
                        .env("GIT_DIR", &git_dir)
                        .env("GIT_COMMON_DIR", &git_dir)
                        .env("GIT_WORK_TREE", &caller)
                        .env("GIT_INDEX_FILE", git_dir.join("index"))
                        .env("GIT_PREFIX", "foreign/");
                }
                "objects" => {
                    child
                        .env("GIT_OBJECT_DIRECTORY", git_dir.join("objects"))
                        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", git_dir.join("objects"));
                }
                _ => {
                    child
                        .env("GIT_CONFIG", git_dir.join("config"))
                        .env("GIT_CONFIG_PARAMETERS", "'user.name'='Poisoned Caller'")
                        .env("GIT_CONFIG_COUNT", "1")
                        .env("GIT_CONFIG_KEY_0", "user.email")
                        .env("GIT_CONFIG_VALUE_0", "poison@example.invalid");
                }
            }
            let output = child.output()?;
            ensure!(snapshot(&caller)? == before, "{mode}: calling repository changed");
            ensure!(
                output.status.success(),
                "{mode}: probe failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ensure!(root.join("probe-complete").is_file(), "{mode}: probe did not execute");
        }

        let output = command().args(["rev-parse", "--local-env-vars"]).output()?;
        ensure!(output.status.success());
        let native = String::from_utf8(output.stdout)?;
        ensure!(native.lines().any(|name| name == "GIT_DIR"));
        let cmd = command();
        let removals = cmd.get_envs().collect::<BTreeMap<_, _>>();
        for name in native.lines() {
            ensure!(
                removals.get(OsString::from(name).as_os_str()) == Some(&None),
                "unisolated native Git-local variable {name}"
            );
        }
        Ok(())
    }
}
