//! Child-only isolation for Git commands that select an explicit repository.

use std::process::Command;

// Git's `rev-parse --local-env-vars` list. Keep global tool/user configuration
// and credentials intact; only repository selection and injected config belong
// to the calling hook rather than the explicitly selected child repository.
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
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

pub(crate) fn is_local_variable(name: &str) -> bool {
    LOCAL_ENV.contains(&name)
        || name.starts_with("GIT_CONFIG_KEY_")
        || name.starts_with("GIT_CONFIG_VALUE_")
}

/// Remove the caller's repository selectors from this child only. Apply any
/// deliberately derived linked-worktree overrides after calling this function.
pub fn isolate(command: &mut Command) -> &mut Command {
    for name in LOCAL_ENV {
        command.env_remove(name);
    }
    // COUNT removal disables injected entries; remove their payloads too.
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(is_local_variable) {
            command.env_remove(name);
        }
    }
    command
}

/// A Git subprocess with no inherited calling-repository context.
pub fn command() -> Command {
    let mut command = Command::new("git");
    isolate(&mut command);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    fn git(root: &Path, args: &[&str]) -> String {
        let mut child = if std::env::var_os("XTASK_GIT_ISOLATION_UNSAFE_CONTROL").is_some() {
            // The original constructor is exercised only in a disposable
            // re-entered child, never against the invoking checkout.
            Command::new("git")
        } else {
            command()
        };
        let output = child.arg("-C").arg(root).args(args).output().expect("spawn git");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("UTF-8 Git output").trim().to_string()
    }

    fn init(root: &Path) {
        fs::create_dir(root).expect("create fixture");
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Git isolation fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("value.txt"), "fixture").expect("write fixture");
        git(root, &["add", "value.txt"]);
        git(root, &["commit", "-qm", "fixture"]);
    }

    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).expect("read sentinel") {
                let path = entry.expect("sentinel entry").path();
                if path.is_dir() {
                    visit(root, &path, files);
                } else {
                    files.insert(
                        path.strip_prefix(root).expect("sentinel path").to_path_buf(),
                        fs::read(path).expect("read sentinel bytes"),
                    );
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(root, root, &mut files);
        files
    }

    #[test]
    fn explicit_repository_child() {
        let Some(root) = std::env::var_os("XTASK_GIT_ISOLATION_CHILD_ROOT") else { return };
        let root = Path::new(&root);
        git(root, &["config", "--local", "fixture.marker", "target"]);
        git(root, &["tag", "fixture-child"]);
        assert_eq!(git(root, &["config", "--get", "core.bare"]), "false");
    }

    #[test]
    fn hook_environment_cannot_mutate_sentinel() {
        for unsafe_control in [true, false] {
            let temp = tempfile::tempdir().expect("create disposable repositories");
            let target = temp.path().join("target");
            let sentinel = temp.path().join("sentinel");
            init(&target);
            init(&sentinel);
            let before = snapshot(&sentinel);
            let mut child = Command::new(std::env::current_exe().expect("test executable"));
            child
                .args([
                    "--exact",
                    "git_environment::tests::explicit_repository_child",
                    "--nocapture",
                ])
                .env("XTASK_GIT_ISOLATION_CHILD_ROOT", &target)
                .env("GIT_DIR", sentinel.join(".git"))
                .env("GIT_COMMON_DIR", sentinel.join(".git"))
                .env("GIT_WORK_TREE", &sentinel)
                .env("GIT_INDEX_FILE", sentinel.join(".git/index"))
                .env("GIT_OBJECT_DIRECTORY", sentinel.join(".git/objects"))
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", sentinel.join(".git/objects"))
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "core.bare")
                .env("GIT_CONFIG_VALUE_0", "true");
            if unsafe_control {
                child.env("XTASK_GIT_ISOLATION_UNSAFE_CONTROL", "1");
            } else {
                child.env_remove("XTASK_GIT_ISOLATION_UNSAFE_CONTROL");
            }
            let output = child.output().expect("re-enter isolated child");
            if unsafe_control {
                assert_ne!(
                    before,
                    snapshot(&sentinel),
                    "old constructor must reproduce corruption"
                );
                assert!(
                    !output.status.success(),
                    "old constructor must fail the explicit-root assertion"
                );
                eprintln!("RED: original Git constructor changed disposable sentinel");
                continue;
            }
            assert_eq!(before, snapshot(&sentinel), "calling repository changed");
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(git(&target, &["config", "--local", "fixture.marker"]), "target");
            assert_eq!(git(&target, &["tag", "--list", "fixture-child"]), "fixture-child");
            eprintln!("GREEN: isolated Git constructor preserved sentinel and selected target");
        }
    }
}
