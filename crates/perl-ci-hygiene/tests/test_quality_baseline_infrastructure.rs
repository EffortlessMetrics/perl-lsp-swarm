//! Infrastructure tests for issue #3021 test-code quality baselines.
//!
//! Verifies baseline files, dev-dependency hygiene, and narrow panic burn-down
//! targets before broader mechanical cleanup lands.

use std::fs;
use std::path::{Path, PathBuf};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(".."))
}

fn read_usize_baseline(path: &Path) -> TestResult<usize> {
    let raw = fs::read_to_string(path)?;
    raw.trim().parse::<usize>().map_err(|err| format!("invalid baseline {:?}: {err}", path).into())
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> TestResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn cargo_toml_has_dev_dep(crate_name: &str, dep: &str) -> TestResult<bool> {
    let manifest = workspace_root().join("crates").join(crate_name).join("Cargo.toml");
    let content = fs::read_to_string(manifest)?;
    Ok(content.contains("[dev-dependencies]") && content.contains(dep))
}

fn rust_files_under(path: &Path) -> TestResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rust_files(path, &mut files)?;
    Ok(files)
}

fn all_files_have_allow_panic(paths: &[PathBuf]) -> TestResult<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    for path in paths {
        let content = fs::read_to_string(path)?;
        if !content.contains("#![allow(clippy::panic)]")
            && !content.contains("#[allow(clippy::panic)]")
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn line_has_match_arm_panic(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.contains("=>") && trimmed.contains("panic!")
}

fn match_arm_panic_hits(paths: &[&Path]) -> TestResult<Vec<String>> {
    let mut hits = Vec::new();
    for path in paths {
        let content = fs::read_to_string(path)?;
        for (idx, line) in content.lines().enumerate() {
            if line_has_match_arm_panic(line) {
                hits.push(format!("{}:{}: {}", path.display(), idx + 1, line.trim()));
            }
        }
    }
    Ok(hits)
}

/// Test 1: panic baseline file exists and matches the enforced checker output.
#[test]
fn test_panic_test_baseline_file_exists_and_contains_count() -> TestResult {
    use std::process::Command;

    let root = workspace_root();
    let baseline_path = root.join("ci").join("panic_test_baseline.txt");
    assert!(baseline_path.is_file(), "missing {:?}", baseline_path);

    let baseline = read_usize_baseline(&baseline_path)?;
    let bin = std::env::var_os("CARGO_BIN_EXE_perl-ci-hygiene")
        .ok_or("CARGO_BIN_EXE_perl-ci-hygiene was not set by cargo")?;
    let output = Command::new(bin).arg("check-panic-test").current_dir(&root).output()?;
    assert!(
        output.status.success(),
        "check-panic-test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported = stdout
        .lines()
        .find_map(|line| line.strip_prefix("test panic!: "))
        .and_then(|line| line.split_whitespace().next())
        .ok_or("missing panic count from check-panic-test output")?
        .parse::<usize>()?;
    assert_eq!(
        baseline, reported,
        "ci/panic_test_baseline.txt ({baseline}) must match check-panic-test ({reported})"
    );
    Ok(())
}

/// Test 2: unlinked TODO baseline for test code is zero.
#[test]
fn test_todo_test_baseline_file_exists_and_contains_zero() -> TestResult {
    let baseline_path = workspace_root().join("ci").join("todo_test_baseline.txt");
    assert!(baseline_path.is_file(), "missing {:?}", baseline_path);
    assert_eq!(read_usize_baseline(&baseline_path)?, 0);
    Ok(())
}

/// Test 3: perl-dead-code declares perl-tdd-support for test helpers.
#[test]
fn test_perl_dead_code_has_tdd_support_dev_dependency() -> TestResult {
    assert!(cargo_toml_has_dev_dep("perl-dead-code", "perl-tdd-support")?);
    Ok(())
}

/// Test 4: perl-dead-code integration tests opt out of workspace panic deny.
#[test]
fn test_perl_dead_code_tests_have_allow_clippy_panic() -> TestResult {
    let tests_dir = workspace_root().join("crates").join("perl-dead-code").join("tests");
    let files = rust_files_under(&tests_dir)?;
    assert!(
        all_files_have_allow_panic(&files)?,
        "every file in {:?} needs #![allow(clippy::panic)]",
        tests_dir
    );
    Ok(())
}

/// Test 5: perl-kwalitee (successor to absorbed perl-lsp-feature-policy) has perl-tdd-support.
#[test]
fn test_perl_kwalitee_has_tdd_support_dev_dependency() -> TestResult {
    assert!(cargo_toml_has_dev_dep("perl-kwalitee", "perl-tdd-support")?);
    Ok(())
}

/// Test 6: perl-kwalitee test module opts out of workspace panic deny.
#[test]
fn test_perl_kwalitee_tests_have_allow_clippy_panic() -> TestResult {
    let lib_rs = workspace_root().join("crates").join("perl-kwalitee").join("src").join("lib.rs");
    let content = fs::read_to_string(lib_rs)?;
    assert!(
        content.contains("#[cfg(test)]")
            && (content.contains("#![allow(clippy::panic)]")
                || content.contains("#[allow(clippy::panic)]")),
        "perl-kwalitee src/lib.rs test module needs allow(clippy::panic)"
    );
    Ok(())
}

/// Test 7: narrow perl-parser-core burn-down target is clean (coderef_invocation_tests.rs only).
#[test]
fn test_perl_parser_core_no_panic_in_match_arm_catches() -> TestResult {
    let target = workspace_root()
        .join("crates")
        .join("perl-parser-core")
        .join("src")
        .join("engine")
        .join("parser")
        .join("coderef_invocation_tests.rs");
    let hits = match_arm_panic_hits(&[target.as_path()])?;
    assert!(hits.is_empty(), "match-arm panic! remains:\n{}", hits.join("\n"));
    Ok(())
}

/// Test 8: narrow perl-dap burn-down target is clean (dap_adapter_tests.rs only).
#[test]
fn test_perl_dap_no_panic_in_match_arm_catches() -> TestResult {
    let target =
        workspace_root().join("crates").join("perl-dap").join("tests").join("dap_adapter_tests.rs");
    let hits = match_arm_panic_hits(&[target.as_path()])?;
    assert!(hits.is_empty(), "match-arm panic! remains:\n{}", hits.join("\n"));
    Ok(())
}

/// Test 9: perl-lexer has no match-arm panic catches (spec drift: was perl-builtins).
#[test]
fn test_perl_lexer_no_panic_in_match_arm_catches() -> TestResult {
    let tests_dir = workspace_root().join("crates").join("perl-lexer").join("tests");
    let files = rust_files_under(&tests_dir)?;
    let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let hits = match_arm_panic_hits(refs.as_slice())?;
    assert!(hits.is_empty(), "match-arm panic! found:\n{}", hits.join("\n"));
    Ok(())
}

/// Test 10: production baselines remain at their established zero-tolerance budgets.
#[test]
fn test_production_baselines_unchanged() -> TestResult {
    let root = workspace_root();
    assert_eq!(read_usize_baseline(&root.join("ci").join("panic_prod_baseline.txt"))?, 0);
    assert_eq!(read_usize_baseline(&root.join("ci").join("unwrap_prod_baseline.txt"))?, 1);
    Ok(())
}

/// Test 11: tree-sitter-perl-rs has no match-arm panic catches.
#[test]
fn test_tree_sitter_perl_rs_no_panic_in_match_arm_catches() -> TestResult {
    let tests_dir = workspace_root().join("crates").join("tree-sitter-perl-rs").join("tests");
    let files = rust_files_under(&tests_dir)?;
    let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let hits = match_arm_panic_hits(refs.as_slice())?;
    assert!(hits.is_empty(), "match-arm panic! found:\n{}", hits.join("\n"));
    Ok(())
}
