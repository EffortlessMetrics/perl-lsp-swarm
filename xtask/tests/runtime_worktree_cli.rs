//! Explicit runtime-worktree selection through an immutable copied runner.

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn isolated(command: &mut Command) -> &mut Command {
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
}

fn git(root: &Path, args: &[&str]) -> Result<Output> {
    let mut command = xtask::git_environment::isolate(Command::new("git"));
    isolated(&mut command)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" })
        .args([
            "-c",
            "user.name=Runtime fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=",
            "-c",
            "init.templateDir=",
            "-c",
            "core.autocrlf=false",
        ])
        .current_dir(root)
        .args(args);
    let output = command.output().context("running fixture Git")?;
    if !output.status.success() {
        bail!("Git {args:?} in {}: {}", root.display(), String::from_utf8_lossy(&output.stderr));
    }
    Ok(output)
}

fn workspace(parent: &Path, name: &str) -> Result<PathBuf> {
    let root = parent.join(name);
    fs::create_dir_all(root.join("xtask/src"))?;
    fs::create_dir_all(root.join("crates/runtime-only/src"))?;
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[workspace]\nmembers=[\"xtask\",\"crates/runtime-only\"]\nresolver=\"3\"\n[workspace.package]\nrepository={:?}\n",
            env!("CARGO_PKG_REPOSITORY")
        ),
    )?;
    fs::write(
        root.join("xtask/Cargo.toml"),
        "[package]\nname=\"xtask\"\nversion=\"0.0.0\"\nedition=\"2024\"\n",
    )?;
    fs::write(root.join("xtask/src/main.rs"), "fn main() {}\n")?;
    fs::write(
        root.join("crates/runtime-only/Cargo.toml"),
        format!("[package]\nname={name:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n"),
    )?;
    fs::write(root.join("crates/runtime-only/src/lib.rs"), "")?;
    git(&root, &["init", "-q", "-b", "main", "--object-format=sha1"])?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-qm", "initial"])?;
    fs::write(root.join(format!("{name}-only.txt")), "runtime subject\n")?;
    git(&root, &["add", "."])?;
    git(&root, &["commit", "-qm", "runtime change"])?;
    Ok(root)
}

fn runner(binary: &Path, cwd: &Path, root: Option<&Path>, args: &[&str]) -> Result<Output> {
    let mut command = Command::new(binary);
    isolated(&mut command).current_dir(cwd);
    if let Some(root) = root {
        command.arg("--worktree-root").arg(root);
    }
    command.args(args).output().context("running immutable xtask")
}

fn stdout(output: Output) -> Result<String> {
    if !output.status.success() {
        bail!("xtask failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    String::from_utf8(output.stdout).context("xtask stdout")
}

const CHANGE_SET: &[&str] =
    &["change-set", "--base", "HEAD^", "--head", "HEAD", "--format", "paths"];

#[test]
fn copied_runner_selects_root_and_child_cargo_from_different_directories() -> Result<()> {
    let temp = TempDir::new()?;
    let b = workspace(temp.path(), "runtime-b")?;
    let c = workspace(temp.path(), "runtime-c")?;
    let outside = temp.path().join("outside");
    let nested = b.join("nested");
    let linked = temp.path().join("linked worktree");
    git(
        &b,
        &["worktree", "add", "-q", "-b", "linked", linked.to_str().context("linked path UTF-8")?],
    )?;
    fs::write(linked.join("linked-only.txt"), "linked runtime subject\n")?;
    git(&linked, &["add", "."])?;
    git(&linked, &["commit", "-qm", "linked change"])?;
    fs::create_dir_all(&outside)?;
    fs::create_dir_all(&nested)?;
    let binary = temp.path().join(if cfg!(windows) { "copied-xtask.exe" } else { "copied-xtask" });
    fs::copy(env!("CARGO_BIN_EXE_xtask"), &binary)?;
    // A copy with identical bytes is the runner under test, not a rebuild in B.
    assert_eq!(fs::read(&binary)?, fs::read(env!("CARGO_BIN_EXE_xtask"))?);
    for cwd in [&b, &nested, &outside, &c] {
        assert_eq!(stdout(runner(&binary, cwd, Some(&b), CHANGE_SET)?)?, "runtime-b-only.txt\n");
        // This command actually launches cargo metadata; directory basename and
        // package name differ, making the child's selected workspace observable.
        assert_eq!(
            stdout(runner(
                &binary,
                cwd,
                Some(&b),
                &["resolve-package-name", "crates/runtime-only"]
            )?)?,
            "runtime-b\n"
        );
    }
    assert_eq!(
        stdout(runner(&binary, &outside, Some(&nested), CHANGE_SET)?)?,
        "runtime-b-only.txt\n"
    );
    assert_eq!(stdout(runner(&binary, &outside, Some(&c), CHANGE_SET)?)?, "runtime-c-only.txt\n");
    assert_eq!(stdout(runner(&binary, &outside, Some(&linked), CHANGE_SET)?)?, "linked-only.txt\n");
    // A relative selector is resolved against invocation cwd before entering B.
    assert_eq!(
        stdout(runner(&binary, temp.path(), Some(Path::new("runtime-b")), CHANGE_SET)?)?,
        "runtime-b-only.txt\n"
    );
    // Existing per-command fixture roots still override the helper's default.
    let mut args = CHANGE_SET.to_vec();
    args.extend(["--root", c.to_str().context("fixture path UTF-8")?]);
    assert_eq!(stdout(runner(&binary, &outside, Some(&b), &args)?)?, "runtime-c-only.txt\n");
    // Omission retains the compiled-root default even when invoked in B.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("source root")?;
    let source_head = String::from_utf8(git(source, &["rev-parse", "HEAD"])?.stdout)?;
    // B does not contain this source commit, so switching the omitted default
    // to invocation cwd fails instead of silently passing an empty self-diff.
    assert_eq!(
        stdout(runner(
            &binary,
            &b,
            None,
            &["change-set", "--base", source_head.trim(), "--head", "HEAD", "--format", "paths"]
        )?)?,
        ""
    );
    // A task with its own root helper must also write into B, without changing A.
    let report = "target/xtask/impacted-evidence/latest.json";
    let source_before = fs::read(source.join(report)).ok();
    stdout(runner(&binary, &outside, Some(&b), &["impacted-evidence"])?)?;
    assert!(b.join(report).is_file());
    assert!(b.join("target/xtask/impacted-evidence/latest.md").is_file());
    assert_eq!(fs::read(source.join(report)).ok(), source_before);
    // Default compatibility for the unchanged intentional library corpus anchor.
    assert_eq!(xtask::close_proof::corpus_root(), source.join(".ci/close-proof-contract"));
    // Library state and intentional source anchors are exercised after selection
    // in a fresh process, keeping cwd mutation out of parallel tests.
    let proof = isolated(&mut Command::new(std::env::current_exe()?))
        .env("XTASK_LIBRARY_ROOT_PROOF", &b)
        .args(["--exact", "library_selection_child", "--nocapture"])
        .output()?;
    assert!(stdout(proof)?.contains("selected library roots verified"));
    Ok(())
}

#[test]
// The parent requires this isolated child's explicit execution marker.
#[allow(clippy::print_stdout)]
fn library_selection_child() -> Result<()> {
    let Some(root) = std::env::var_os("XTASK_LIBRARY_ROOT_PROOF") else {
        return Ok(());
    };
    let root = PathBuf::from(root).canonicalize()?;
    xtask::utils::select_worktree_root(&root).map_err(|error| anyhow::anyhow!("{error:#}"))?;
    assert_eq!(xtask::utils::project_root().map_err(|error| anyhow::anyhow!("{error:#}"))?, root);
    assert_eq!(std::env::current_dir()?, root);
    assert!(
        xtask::install_surface_route_units::default_registry_path()
            .map_err(|error| anyhow::anyhow!("{error:#}"))?
            .starts_with(&root)
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().context("source root")?;
    assert_eq!(xtask::close_proof::corpus_root(), source.join(".ci/close-proof-contract"));
    println!("selected library roots verified");
    Ok(())
}

#[test]
fn invalid_explicit_targets_refuse_before_dispatch_without_fallback() -> Result<()> {
    let temp = TempDir::new()?;
    let valid = workspace(temp.path(), "runtime-valid")?;
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside)?;
    let unrelated = temp.path().join("unrelated");
    fs::create_dir_all(&unrelated)?;
    git(&unrelated, &["init", "-q", "-b", "main", "--object-format=sha1"])?;
    let wrong_identity = workspace(temp.path(), "wrong-identity")?;
    fs::write(
        wrong_identity.join("Cargo.toml"),
        "[workspace]\nmembers=[\"xtask\"]\n[workspace.package]\nrepository=\"https://example.invalid/unrelated\"\n",
    )?;
    let wrong_package = workspace(temp.path(), "wrong-package")?;
    fs::write(
        wrong_package.join("xtask/Cargo.toml"),
        "[package]\nname=\"unrelated\"\nversion=\"0.0.0\"\n",
    )?;
    for target in
        [outside.clone(), unrelated, wrong_identity, wrong_package, temp.path().join("missing")]
    {
        // list-commands always succeeds after dispatch, so failure specifically
        // witnesses root admission rather than a later task refusing its inputs.
        let result = runner(
            Path::new(env!("CARGO_BIN_EXE_xtask")),
            &valid,
            Some(&target),
            &["list-commands"],
        )?;
        assert!(!result.status.success(), "invalid target {} passed", target.display());
        assert!(result.stdout.is_empty(), "invalid target reached dispatch");
        assert!(String::from_utf8_lossy(&result.stderr).contains("--worktree-root"));
    }
    for selector in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        for value in [std::ffi::OsStr::new(""), valid.as_os_str()] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
            isolated(&mut command)
                .current_dir(&outside)
                .env(selector, value)
                .arg("--worktree-root")
                .arg(&valid)
                .arg("list-commands");
            let result = command.output()?;
            assert!(!result.status.success(), "ambient {selector} passed");
            assert!(String::from_utf8_lossy(&result.stderr).contains(selector));
            assert!(result.stdout.is_empty());
        }
    }
    Ok(())
}
