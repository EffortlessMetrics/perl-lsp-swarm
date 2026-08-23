//! End-to-end integration tests for workspace discovery.

use perl_workspace::discovery::{
    DiscoveryConfig, DiscoveryMethod, discover_perl_files, discover_perl_files_with_config,
    discover_perl_files_with_include_paths,
};
use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn create_file(root: &Path, relative: &str) -> TestResult {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, "# integration fixture\n")?;
    Ok(())
}

#[test]
fn discovers_fcgi_and_skips_perl_lsp_cache_by_default() -> TestResult {
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "app.fcgi")?;
    create_file(root, ".perl-lsp/cache/foo.pm")?;

    let result = discover_perl_files(root);

    assert!(contains_relative_file(&result.files, "app.fcgi"));
    assert!(!contains_relative_file(&result.files, ".perl-lsp/cache/foo.pm"));
    Ok(())
}

#[test]
fn configured_discovery_extensions_and_skipped_dirs_are_additive() -> TestResult {
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "custom/example.foo")?;
    create_file(root, "generated/cache.pm")?;

    let config = DiscoveryConfig::new(
        vec![".foo".to_string(), "FOO".to_string()],
        vec![" generated ".to_string()],
    );
    let result = discover_perl_files_with_config(root, &[] as &[&Path], &config);

    assert!(contains_relative_file(&result.files, "custom/example.foo"));
    assert!(!contains_relative_file(&result.files, "generated/cache.pm"));
    Ok(())
}

fn contains_relative_file(result_files: &[std::path::PathBuf], relative: &str) -> bool {
    let relative_path = Path::new(relative);
    result_files.iter().any(|path| path.ends_with(relative_path))
}

fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn run_git(root: &Path, args: &[&str]) -> TestResult {
    let status = Command::new("git").args(args).current_dir(root).status()?;
    if status.success() {
        return Ok(());
    }

    Err(format!("git command failed: git {}", args.join(" ")).into())
}

#[test]
fn discovers_files_via_walkdir_when_root_is_not_git_repo() -> TestResult {
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "lib/One.pm")?;
    create_file(root, "bin/run.pl")?;
    create_file(root, "notes.txt")?;

    let result = discover_perl_files(root);

    assert_eq!(result.method, DiscoveryMethod::Walk);
    assert_eq!(result.files.len(), 2);
    assert!(result.files.iter().any(|path| path.ends_with("lib/One.pm")));
    assert!(result.files.iter().any(|path| path.ends_with("bin/run.pl")));

    Ok(())
}

#[test]
fn discovers_files_via_git_and_honors_gitignore() -> TestResult {
    if !git_available() {
        return Ok(());
    }

    let tmp = TempDir::new()?;
    let root = tmp.path();

    run_git(root, &["init", "--quiet"])?;
    fs::write(root.join(".gitignore"), "target/\n")?;

    create_file(root, "lib/One.pm")?;
    create_file(root, "target/generated/Skipped.pm")?;

    let result = discover_perl_files(root);

    assert_eq!(result.method, DiscoveryMethod::Git);
    assert!(result.files.iter().any(|path| path.ends_with("lib/One.pm")));
    assert!(!result.files.iter().any(|path| path.to_string_lossy().contains("/target/")));

    Ok(())
}

#[test]
fn discovers_git_files_when_repo_root_path_contains_skipped_component_name() -> TestResult {
    if !git_available() {
        return Ok(());
    }

    let tmp = TempDir::new()?;
    let root = tmp.path().join("target").join("workspace");
    fs::create_dir_all(&root)?;

    run_git(&root, &["init", "--quiet"])?;
    create_file(&root, "lib/One.pm")?;

    let result = discover_perl_files(&root);

    assert_eq!(result.method, DiscoveryMethod::Git);
    assert_eq!(result.files.len(), 1);
    assert!(result.files.iter().any(|path| path.ends_with("lib/One.pm")));

    Ok(())
}

#[test]
fn include_paths_allow_normally_skipped_local_lib_perl5() -> TestResult {
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "lib/My/App.pm")?;
    create_file(root, "local/lib/perl5/Remote/Module.pm")?;

    let include_paths = vec!["lib".to_string(), "local/lib/perl5".to_string()];
    let result = discover_perl_files_with_include_paths(root, &include_paths);

    assert!(contains_relative_file(&result.files, "lib/My/App.pm"));
    assert!(contains_relative_file(&result.files, "local/lib/perl5/Remote/Module.pm"));

    let lib_only_include_paths = vec!["lib".to_string()];
    let lib_only_result = discover_perl_files_with_include_paths(root, &lib_only_include_paths);

    assert!(contains_relative_file(&lib_only_result.files, "lib/My/App.pm"));
    assert!(!contains_relative_file(&lib_only_result.files, "local/lib/perl5/Remote/Module.pm"));

    Ok(())
}

/// Call-observation for the discovery git spawn: on a root outside any git
/// repository, `git ls-files` fails fast and discovery falls back to walking.
///
/// The spawned git gets an explicit null stdin (`Stdio::null()`): a git
/// inheriting an open, non-console stdin pipe blocks instead of exiting on
/// Windows, which stalled background workspace scans until unrelated client
/// input arrived. This test observes the completion contract - bounded time,
/// walk-fallback method, correct file set - for every step the spawn takes.
#[test]
fn git_spawn_completes_on_non_repo_root_without_caller_stdin() -> TestResult {
    if !git_available() {
        return Ok(());
    }
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "lib/One.pm")?;

    let started = std::time::Instant::now();
    let result = perl_workspace::discovery::discover_perl_files_with_config_and_cancel(
        root,
        &[] as &[std::path::PathBuf],
        &DiscoveryConfig::default(),
        || false,
    );
    let elapsed = started.elapsed();

    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "discovery must complete without waiting on caller input; took {elapsed:?}"
    );
    assert_eq!(result.method, DiscoveryMethod::Walk);
    assert!(!result.cancelled);
    assert!(contains_relative_file(&result.files, "lib/One.pm"));

    Ok(())
}

/// Exact boundary variant for the discovery cancellation checkpoints: a
/// cancellation observed after the git child was spawned kills the child and
/// returns the cancelled outcome with no files, rather than falling through
/// to the walk fallback or blocking on the child.
#[test]
fn git_discovery_cancelled_during_child_wait_returns_cancelled_result() -> TestResult {
    if !git_available() {
        return Ok(());
    }
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "lib/One.pm")?;

    // First check (pre-spawn) passes; the next checkpoint - the child wait
    // loop - cancels, so the child is killed and the outcome is Cancelled.
    let checkpoints = std::sync::atomic::AtomicUsize::new(0);
    let result = perl_workspace::discovery::discover_perl_files_with_config_and_cancel(
        root,
        &[] as &[std::path::PathBuf],
        &DiscoveryConfig::default(),
        || checkpoints.fetch_add(1, std::sync::atomic::Ordering::SeqCst) > 0,
    );

    assert!(result.cancelled, "wait-loop cancellation must surface");
    assert!(result.files.is_empty(), "cancelled discovery must return no files");

    Ok(())
}

/// Boundary variant for the pre-spawn checkpoint: an immediately cancelled
/// discovery never spawns the git child and still reports cancellation.
#[test]
fn git_discovery_cancelled_before_spawn_never_discovers() -> TestResult {
    if !git_available() {
        return Ok(());
    }
    let tmp = TempDir::new()?;
    let root = tmp.path();

    create_file(root, "lib/One.pm")?;

    let result = perl_workspace::discovery::discover_perl_files_with_config_and_cancel(
        root,
        &[] as &[std::path::PathBuf],
        &DiscoveryConfig::default(),
        || true,
    );

    assert!(result.cancelled);
    assert!(result.files.is_empty());

    Ok(())
}
