//! Git children select their repository from their arguments or working directory.

use std::process::Command;

// Git's `rev-parse --local-env-vars` inventory. In particular, `-C` does not
// override a hook's GIT_DIR, object/index paths or command-local configuration.
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
    // Older Git versions still report and interpret this internal selector.
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

pub(crate) fn command() -> Command {
    let mut command = Command::new("git");
    for key in LOCAL_ENV {
        command.env_remove(key);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{Result, ensure};
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};

    const PROBE_ROOT: &str = "PERL_CORE_HARNESS_GIT_ISOLATION_PROBE_ROOT";

    fn run(root: &Path, args: &[&str]) -> Result<String> {
        let output = command().arg("-C").arg(root).args(args).output()?;
        ensure!(
            output.status.success(),
            "Git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
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

    // Called only in a separate, exactly selected test process. Its cwd changes
    // cannot race the parent harness, and its poisoned selectors name disposable
    // repositories, never the checkout running this regression.
    fn probe(root: &Path) -> Result<()> {
        let caller_head = fs::read_to_string(root.join("caller-head"))?;
        let (_fixture, config) = crate::tests::current_authority_fixture()?;
        let fixture_root = config.repository_root.clone();
        let fixture_head = config.landed_sha.clone();
        crate::validate_current_authority(config)?;
        crate::validate_git_commit(&fixture_root, &fixture_head)?;
        ensure!(
            crate::validate_git_commit(&fixture_root, &caller_head).is_err(),
            "caller-only commit must not become a fixture commit"
        );
        crate::validate_git_ancestor(&fixture_root, &fixture_head, &fixture_head)?;
        ensure!(crate::validate_git_ancestor(&fixture_root, &caller_head, &fixture_head).is_err());
        ensure!(crate::git_blob_at(&fixture_root, &fixture_head, "fixture-base.txt")? == b"base\n");
        ensure!(crate::perl_tree_ref(&fixture_root) == fixture_head);
        ensure!(crate::perl_tree_ref(&fixture_root.join(".ci")) == "unknown");
        std::env::set_current_dir(&fixture_root)?;
        let observed_commit = crate::current_commit();
        // Let the fixture close on Windows too; only this child changed cwd.
        std::env::set_current_dir(root)?;
        ensure!(observed_commit == fixture_head);
        fs::write(root.join("probe-complete"), fixture_head)?;
        Ok(())
    }

    #[test]
    fn repository_local_environment_cannot_redirect_git() -> Result<()> {
        if let Some(root) = std::env::var_os(PROBE_ROOT) {
            return probe(Path::new(&root));
        }

        for mode in ["hook", "objects", "config", "super-prefix"] {
            let temp = tempfile::tempdir()?;
            let root = temp.path();
            let caller = root.join("caller");
            fs::create_dir(&caller)?;
            run(&caller, &["init", "--quiet"])?;
            run(&caller, &["config", "user.name", "Caller Guardian"])?;
            run(&caller, &["config", "user.email", "caller@example.invalid"])?;
            fs::write(caller.join("caller-only.txt"), b"caller-only object\n")?;
            run(&caller, &["add", "."])?;
            run(&caller, &["commit", "--quiet", "-m", "caller-only commit"])?;
            fs::write(root.join("caller-head"), run(&caller, &["rev-parse", "HEAD"])?)?;
            let before = snapshot(&caller)?;

            let git_dir = caller.join(".git");
            let mut child = Command::new(std::env::current_exe()?);
            child
                .args([
                    "--exact",
                    "git::tests::repository_local_environment_cannot_redirect_git",
                    "--nocapture",
                ])
                .env(PROBE_ROOT, root);
            // Do not let the *test driver* borrow selectors from a real hook.
            // Each poison mode below supplies only owned disposable paths.
            for key in LOCAL_ENV {
                child.env_remove(key);
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
                "super-prefix" => {
                    child.env("GIT_INTERNAL_SUPER_PREFIX", "foreign/");
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
            ensure!(
                snapshot(&caller)? == before,
                "{mode}: caller config, identity, refs, objects or index changed"
            );
            ensure!(
                output.status.success(),
                "{mode}: child failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            ensure!(root.join("probe-complete").is_file(), "{mode}: child probe did not execute");
            ensure!(run(&caller, &["config", "user.name"])? == "Caller Guardian");
            ensure!(run(&caller, &["config", "user.email"])? == "caller@example.invalid");
        }

        // Independent native inventory catches newly introduced Git-local
        // selectors instead of allowing the implementation's list to define its
        // own completeness. Configuration count removal makes KEY_n/VALUE_n inert.
        let output = command().args(["rev-parse", "--local-env-vars"]).output()?;
        ensure!(output.status.success());
        let native = String::from_utf8(output.stdout)?;
        ensure!(native.lines().any(|key| key == "GIT_DIR"));
        let command = command();
        let removals = command.get_envs().collect::<BTreeMap<_, _>>();
        // Keep the older-version contract covered even when the installed Git
        // no longer lists or interprets this key.
        ensure!(
            removals.get(OsString::from("GIT_INTERNAL_SUPER_PREFIX").as_os_str()) == Some(&None),
            "legacy Git-local super-prefix must be explicitly removed"
        );
        for key in native.lines() {
            ensure!(
                removals.get(OsString::from(key).as_os_str()) == Some(&None),
                "unisolated native Git-local variable {key}"
            );
        }
        Ok(())
    }
}
