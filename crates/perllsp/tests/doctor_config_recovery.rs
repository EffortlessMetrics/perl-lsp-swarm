//! Real CLI recovery from a project config discovered above the requested directory.
//!
//! Expected paths come from the fixture layout, independently of config discovery.
//! These tests cover stable layouts; they do not claim atomic config discovery when
//! another process replaces or removes a config while doctor is running.

#![deny(clippy::map_err_ignore)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;

type TestError = Box<dyn std::error::Error>;
type TestResult = Result<(), TestError>;

const INVALID_CONFIG: &str = "[perl\ninclude_paths = [\"lib\"]\n";
const VALID_CONFIG: &str = "[perl]\ninclude_paths = []\nuse_perl5lib = false\n";
const VALID_PERL: &str = "my $ok = 1;\nmy $value = 2;\nprint $ok + $value;\n";
const INVALID_PERL: &str = "my $ok = 1;\nmy $broken = ;\nprint $ok;\n";

struct Project {
    _temp: tempfile::TempDir,
    root: PathBuf,
    nested: PathBuf,
}

impl Project {
    fn new() -> Result<Self, TestError> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let nested = root.join("lib/deep");
        std::fs::create_dir_all(&nested)?;
        std::fs::write(root.join("app.pl"), VALID_PERL)?;
        Ok(Self { _temp: temp, root, nested })
    }

    fn config(&self) -> PathBuf {
        self.root.join(".perl-lsp.toml")
    }
}

fn run(cwd: &Path, args: &[&str]) -> Result<Output, TestError> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perllsp"));
    command.args(args).current_dir(cwd).stdin(Stdio::null());
    for key in [
        "RUST_LOG",
        "PERL_LSP_LOG",
        "PERL_LSP_LOG_FILE",
        "PERL_LSP_TIMING",
        "FORCE_COLOR",
        "CLICOLOR_FORCE",
    ] {
        command.env_remove(key);
    }
    Ok(command.output()?)
}

fn config_failure(output: Output, config: &Path, syntax: bool) -> TestResult {
    assert_eq!(output.status.code(), Some(1), "doctor must fail: {output:?}");
    assert!(output.stdout.is_empty(), "config failure leaked stdout: {output:?}");
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.starts_with(&format!("{}: ", config.display())),
        "doctor must name the selected fixture config {}: {stderr}",
        config.display()
    );
    if syntax {
        assert!(stderr.contains("syntax error"), "missing TOML cause: {stderr}");
        assert!(stderr.contains("line 1, column 6"), "missing TOML location: {stderr}");
        assert!(stderr.contains("[perl"), "missing invalid TOML source: {stderr}");
        assert!(stderr.contains('^'), "missing TOML location marker: {stderr}");
    } else {
        assert!(stderr.contains("regular file"), "missing directory recovery cause: {stderr}");
    }
    Ok(())
}

fn loaded_json(cwd: &Path, workspace: &Path) -> Result<Value, TestError> {
    let output = run(cwd, &["--doctor", ".", "--json"])?;
    assert_eq!(output.status.code(), Some(0), "corrected config must load: {output:?}");
    assert!(output.stderr.is_empty(), "successful JSON leaked stderr: {output:?}");
    // from_slice consumes one complete JSON value and rejects trailing report text.
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert!(report.is_object(), "doctor JSON must be a report: {report}");
    assert_eq!(report["workspace"], workspace.to_str().ok_or("non-UTF-8 workspace")?);
    assert_eq!(report["config"]["status"], "Loaded");
    assert_eq!(report["config"]["include_source"], "default includePaths");
    assert_eq!(report["config"]["rejected_include_paths"], serde_json::json!([]));
    Ok(report)
}

#[test]
fn doctor_root_failure_can_be_corrected_to_stable_success_json() -> TestResult {
    let project = Project::new()?;
    std::fs::write(project.config(), INVALID_CONFIG)?;
    config_failure(run(&project.root, &["--doctor"])?, &project.config(), true)?;
    config_failure(run(&project.root, &["--doctor", "--json"])?, &project.config(), true)?;

    std::fs::write(project.config(), VALID_CONFIG)?;
    let root_report = loaded_json(&project.root, &project.root)?;
    let nested_report = loaded_json(&project.nested, &project.nested)?;
    let repeated_report = loaded_json(&project.nested, &project.nested)?;
    // External-tool probes describe the host. Stability here belongs to the
    // existing workspace/config projection, not a new whole-report schema.
    assert_eq!(root_report["config"], nested_report["config"]);
    assert_eq!(nested_report["config"], repeated_report["config"]);
    assert_eq!(nested_report["workspace"], repeated_report["workspace"]);
    Ok(())
}

#[test]
fn doctor_nested_text_failure_names_ancestor_config() -> TestResult {
    let project = Project::new()?;
    std::fs::write(project.config(), INVALID_CONFIG)?;
    config_failure(run(&project.nested, &["--doctor"])?, &project.config(), true)
}

#[test]
fn doctor_nested_json_failure_keeps_stderr_and_names_ancestor_config() -> TestResult {
    let project = Project::new()?;
    std::fs::write(project.config(), INVALID_CONFIG)?;
    config_failure(run(&project.nested, &["--doctor", "--json"])?, &project.config(), true)
}

#[test]
fn doctor_explicit_workspace_is_independent_of_process_cwd() -> TestResult {
    let project = Project::new()?;
    std::fs::write(project.config(), INVALID_CONFIG)?;
    let unrelated = tempfile::tempdir()?;
    // A malformed cwd-local config must not replace the explicitly requested
    // workspace's config in the diagnostic.
    std::fs::write(unrelated.path().join(".perl-lsp.toml"), INVALID_CONFIG)?;
    let nested = project.nested.to_str().ok_or("non-UTF-8 nested workspace")?;
    config_failure(run(unrelated.path(), &["--doctor", nested, "--json"])?, &project.config(), true)
}

#[test]
fn nearest_malformed_config_shadows_valid_parent_until_removed() -> TestResult {
    let project = Project::new()?;
    std::fs::write(project.config(), VALID_CONFIG)?;
    let shadow = project.root.join("lib/.perl-lsp.toml");
    std::fs::write(&shadow, INVALID_CONFIG)?;
    config_failure(run(&project.nested, &["--doctor", "--json"])?, &shadow, true)?;
    std::fs::remove_file(shadow)?;
    loaded_json(&project.nested, &project.nested)?;
    Ok(())
}

#[test]
fn malformed_ancestor_directory_is_named_and_can_be_replaced_with_config() -> TestResult {
    let project = Project::new()?;
    std::fs::create_dir(project.config())?;
    config_failure(run(&project.root, &["--doctor"])?, &project.config(), false)?;
    config_failure(run(&project.nested, &["--doctor", "--json"])?, &project.config(), false)?;
    std::fs::remove_dir(project.config())?;
    std::fs::write(project.config(), VALID_CONFIG)?;
    loaded_json(&project.nested, &project.nested)?;
    Ok(())
}

#[test]
fn ordinary_project_native_check_has_error_context_and_recovers_after_cwd_change() -> TestResult {
    let project = Project::new()?;
    let valid = run(&project.root, &["--check", "app.pl"])?;
    assert_eq!(valid.status.code(), Some(0), "valid native check failed: {valid:?}");
    assert!(valid.stderr.is_empty(), "valid native check leaked stderr: {valid:?}");
    assert!(String::from_utf8(valid.stdout)?.contains("app.pl: ok"));

    std::fs::write(project.root.join("app.pl"), INVALID_PERL)?;
    let invalid = run(&project.nested, &["--check", "../../app.pl"])?;
    assert_eq!(invalid.status.code(), Some(1), "invalid native check passed: {invalid:?}");
    assert!(invalid.stderr.is_empty(), "parse findings belong to stdout: {invalid:?}");
    let stdout = String::from_utf8(invalid.stdout)?;
    assert!(stdout.contains("../../app.pl: FAIL"), "missing failing path: {stdout}");
    assert!(stdout.contains("line 2, column"), "missing parse location: {stdout}");
    assert!(stdout.contains("2 | my $broken = ;"), "missing invalid source: {stdout}");
    assert!(stdout.contains('^'), "missing parse location marker: {stdout}");

    std::fs::write(project.root.join("app.pl"), VALID_PERL)?;
    let corrected = run(&project.nested, &["--check", "../../app.pl"])?;
    assert_eq!(corrected.status.code(), Some(0), "corrected native check failed: {corrected:?}");
    assert!(corrected.stderr.is_empty(), "corrected check leaked stderr: {corrected:?}");
    let stdout = String::from_utf8(corrected.stdout)?;
    assert!(stdout.contains("../../app.pl: ok"), "missing corrected path: {stdout}");
    assert!(!stdout.contains("FAIL"), "stale parse finding survived correction: {stdout}");
    Ok(())
}
