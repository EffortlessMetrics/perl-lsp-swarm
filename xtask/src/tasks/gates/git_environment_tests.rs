use super::*;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(root: &Path, args: &[&str]) -> String {
    let output = crate::git_environment::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("spawn fixture Git");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("UTF-8 Git output").trim().to_string()
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("sentinel directory") {
            let path = entry.expect("sentinel entry").path();
            if path.is_dir() {
                visit(root, &path, result);
            } else {
                result.insert(
                    path.strip_prefix(root).expect("sentinel path").to_path_buf(),
                    fs::read(path).expect("sentinel bytes"),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn selected_test_gate_child() {
    let Some(root) = std::env::var_os("XTASK_GIT_GATE_CHILD_ROOT") else { return };
    let root = Path::new(&root);
    let test_command = if cfg!(windows) {
        "cargo test"
    } else {
        "env PATH=\"$XTASK_GIT_GATE_TOOL_PATH\" cargo test"
    };
    let other_command = if cfg!(windows) {
        "cargo metadata"
    } else {
        "env PATH=\"$XTASK_GIT_GATE_TOOL_PATH\" cargo metadata"
    };
    let test =
        run_shell_command_with_timeout_in(test_command, &root.join("test.log"), 10, Some(root))
            .expect("test gate launcher");
    assert_eq!(test.exit_code, 0, "{}", test.stdout);
    assert!(test.stdout.lines().any(|line| line.trim() == "GIT_DIR="), "{}", test.stdout);
    assert!(test.stdout.contains("CARGO_BUILD_JOBS=2"), "{}", test.stdout);
    let other =
        run_shell_command_with_timeout_in(other_command, &root.join("other.log"), 10, Some(root))
            .expect("non-test gate launcher");
    assert_eq!(other.exit_code, 0);
    let inherited = std::env::var("GIT_DIR").expect("hostile selector only in child");
    assert!(other.stdout.contains(&format!("GIT_DIR={inherited}")), "{}", other.stdout);
    let rejected = run_shell_command_with_timeout_in(
        "env GIT_DIR=/foreign cargo test",
        &root.join("rejected.log"),
        10,
        Some(root),
    )
    .expect_err("command-local selector must be refused before spawn");
    assert!(!rejected.child_started);
}

#[test]
fn cargo_test_launcher_isolates_fixture_git_without_changing_other_gates() {
    let temp = tempfile::tempdir().expect("disposable test roots");
    let target = temp.path().join("target");
    let sentinel = temp.path().join("sentinel");
    let tools = temp.path().join("tools");
    for root in [&target, &sentinel] {
        fs::create_dir(root).expect("fixture directory");
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Gate fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        git(root, &["config", "commit.gpgsign", "false"]);
        fs::write(root.join("value.txt"), "fixture").expect("fixture data");
        git(root, &["add", "value.txt"]);
        git(root, &["commit", "-qm", "fixture"]);
    }
    fs::create_dir(&tools).expect("fake Cargo directory");
    #[cfg(windows)]
    fs::write(tools.join("cargo.cmd"),
        "@echo off\r\nif \"%1\"==\"test\" git config --local fixture.marker target\r\nif errorlevel 1 exit /b 1\r\necho GIT_DIR=%GIT_DIR%\r\necho CARGO_BUILD_JOBS=%CARGO_BUILD_JOBS%\r\n").expect("fake Cargo");
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool = tools.join("cargo");
        fs::write(&tool, "#!/bin/sh\nif [ \"$1\" = test ]; then git config --local fixture.marker target || exit 1; fi\nprintf 'GIT_DIR=%s\\nCARGO_BUILD_JOBS=%s\\n' \"${GIT_DIR-}\" \"${CARGO_BUILD_JOBS-}\"\n").expect("fake Cargo");
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))
            .expect("fake Cargo permissions");
    }
    let before = snapshot(&sentinel);
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").expect("PATH")));
    let output = Command::new(std::env::current_exe().expect("owning test executable"))
        .args([
            "--exact",
            "tasks::gates::git_environment_tests::selected_test_gate_child",
            "--nocapture",
        ])
        .env("XTASK_GIT_GATE_CHILD_ROOT", &target)
        .env("PATH", std::env::join_paths(&paths).expect("child-only PATH"))
        .env("XTASK_GIT_GATE_TOOL_PATH", std::env::join_paths(&paths).expect("child tool PATH"))
        .env("GIT_DIR", sentinel.join(".git"))
        .env("GIT_COMMON_DIR", sentinel.join(".git"))
        .env("GIT_WORK_TREE", &sentinel)
        .env("GIT_INDEX_FILE", sentinel.join(".git/index"))
        .env("GIT_OBJECT_DIRECTORY", sentinel.join(".git/objects"))
        .env("CARGO_BUILD_JOBS", "2")
        .output()
        .expect("re-enter gate launcher child");
    assert_eq!(before, snapshot(&sentinel), "gate changed unrelated repository");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(git(&target, &["config", "--local", "fixture.marker"]), "target");
}
