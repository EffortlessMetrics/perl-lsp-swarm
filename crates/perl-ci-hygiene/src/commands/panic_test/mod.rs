use color_eyre::eyre::{Result, eyre};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use perl_ci_hygiene::walk_rs_files;

use crate::{first_cfg_test_line_number, read_lines, walk_rust_source_files_for_ci_checks};

static PANIC_MACRO_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"panic!\s*[\(\{]"));
static COMMENT_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| Regex::new(r"^\s*//"));

/// Crates excluded from integration-test panic scanning — mirrors `CI_REPORT_CRATES_EXCLUDE`
/// in `main.rs` (legacy/test-support crates and self-check harnesses).
const CI_REPORT_CRATES_EXCLUDE: [&str; 5] = [
    "tree-sitter-perl-c",
    "perl-parser-pest",
    "perl-tdd-support",
    "perl-test-must",
    "perl-ci-hygiene",
];

fn regex_from_static(
    regex: &'static LazyLock<Result<Regex, regex::Error>>,
    label: &str,
) -> Result<&'static Regex> {
    regex.as_ref().map_err(|err| eyre!("{label} regex failed to compile: {err}"))
}

fn is_integration_test_file(path: &Path) -> bool {
    path.components().any(|component| component.as_os_str() == "tests")
}

fn is_complete_test_source_file(path: &Path) -> bool {
    is_integration_test_file(path)
        || path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
            ["_test.rs", "_tests.rs", "tests.rs"].iter().any(|suffix| name.ends_with(suffix))
        })
}

fn is_excluded_integration_test_path(path: &Path) -> bool {
    if path.components().any(|component| {
        let value = component.as_os_str();
        value == "benches" || value == "examples" || value == "bin"
    }) {
        return true;
    }

    path.components().any(|component| {
        CI_REPORT_CRATES_EXCLUDE
            .iter()
            .any(|item| component.as_os_str() == std::ffi::OsStr::new(item))
    })
}

fn walk_integration_test_files(repo_root: &Path) -> Vec<PathBuf> {
    walk_rs_files(&repo_root.join("crates"))
        .into_iter()
        .filter(|path| is_integration_test_file(path) && !is_excluded_integration_test_path(path))
        .collect()
}

fn walk_complete_test_source_files(repo_root: &Path) -> Vec<PathBuf> {
    walk_rs_files(&repo_root.join("crates"))
        .into_iter()
        .filter(|path| {
            is_complete_test_source_file(path) && !is_excluded_integration_test_path(path)
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
struct PanicSiteIdentity {
    path: String,
    enclosing_test_or_function: String,
    macro_family: &'static str,
    normalized_snippet: String,
    line: usize,
}

fn enclosing_test_or_function(lines: &[String], line_index: usize) -> String {
    lines[..=line_index]
        .iter()
        .rev()
        .find_map(|line| {
            let start = line.find("fn ")? + 3;
            let name = line[start..].split(['(', '<', '{', ' ']).next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn complete_panic_site_inventory(repo_root: &Path) -> Result<Vec<PanicSiteIdentity>> {
    let panic_re = regex_from_static(&PANIC_MACRO_RE, "panic macro")?;
    let comment_re = regex_from_static(&COMMENT_RE, "comment")?;
    let mut sites = Vec::new();

    for path in walk_complete_test_source_files(repo_root) {
        let lines = read_lines(&path)?;
        let relative =
            path.strip_prefix(repo_root).unwrap_or(&path).display().to_string().replace('\\', "/");
        for (index, line) in lines.iter().enumerate() {
            if comment_re.is_match(line) {
                continue;
            }
            for _ in panic_re.find_iter(line) {
                sites.push(PanicSiteIdentity {
                    path: relative.clone(),
                    enclosing_test_or_function: enclosing_test_or_function(&lines, index),
                    macro_family: "panic!",
                    normalized_snippet: line.split_whitespace().collect::<Vec<_>>().join(" "),
                    line: index + 1,
                });
            }
        }
    }
    sites.sort_by(|left, right| {
        (&left.path, left.line, &left.enclosing_test_or_function).cmp(&(
            &right.path,
            right.line,
            &right.enclosing_test_or_function,
        ))
    });
    Ok(sites)
}

/// Emit the complete test-source panic identity inventory without changing the
/// established count gate. This is the measurement surface for #2332 while
/// the accepted identity registry is being adjudicated.
pub(crate) fn write_inventory(repo_root: &Path) -> Result<i32> {
    let sites = complete_panic_site_inventory(repo_root)?;
    let json_sites = sites
        .iter()
        .map(|site| {
            serde_json::json!({
                "path": site.path,
                "enclosing_test_or_function": site.enclosing_test_or_function,
                "macro_family": site.macro_family,
                "normalized_snippet": site.normalized_snippet,
                "line": site.line,
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&json_sites)?);
    Ok(0)
}

fn count_panic_lines(
    lines: &[String],
    start_line: usize,
    panic_re: &Regex,
    comment_re: &Regex,
) -> usize {
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            let line_no = index + 1;
            line_no >= start_line && !comment_re.is_match(line)
        })
        .map(|(_, line)| panic_re.find_iter(line).count())
        .sum()
}

/// Count `panic!` macro uses in integration tests and `#[cfg(test)]` modules.
pub(crate) fn count_panic_in_test_code(repo_root: &Path) -> Result<usize> {
    let panic_re = regex_from_static(&PANIC_MACRO_RE, "panic macro")?;
    let comment_re = regex_from_static(&COMMENT_RE, "comment")?;
    let mut count = 0usize;

    for path in walk_integration_test_files(repo_root) {
        let lines = read_lines(&path)?;
        count += count_panic_lines(&lines, 1, panic_re, comment_re);
    }

    for path in walk_rust_source_files_for_ci_checks(repo_root)? {
        let lines = read_lines(&path)?;
        let inline_test_start = first_cfg_test_line_number(&path).unwrap_or(usize::MAX);
        if inline_test_start != usize::MAX {
            count += count_panic_lines(&lines, inline_test_start, panic_re, comment_re);
        }
    }

    Ok(count)
}

fn read_required_usize_baseline(path: &Path) -> Result<usize> {
    if !path.is_file() {
        return Err(eyre!("baseline file {:?} is missing; cannot enforce panic! budget", path));
    }
    let raw = std::fs::read_to_string(path).map_err(|err| eyre!("reading {:?}: {err}", path))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(eyre!("baseline file {:?} is empty; cannot enforce panic! budget", path));
    }
    trimmed.parse::<usize>().map_err(|err| eyre!("invalid baseline {:?}: {err}", path))
}

/// Enforce the test-code `panic!` budget recorded in `ci/panic_test_baseline.txt`.
pub(crate) fn check_panic_test(repo_root: &Path) -> Result<i32> {
    let current = count_panic_in_test_code(repo_root)?;
    let baseline_path = repo_root.join("ci/panic_test_baseline.txt");
    let baseline = read_required_usize_baseline(&baseline_path)?;

    println!("test panic!: {current} (baseline: {baseline})");
    if current > baseline {
        println!("FAIL: test panic! count ({current}) exceeds baseline ({baseline})");
        println!(
            "If you added panic! calls in test code, remove them or raise ci/panic_test_baseline.txt to the new count ({current})."
        );
        return Ok(1);
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tracked conversion debt: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/3021"
    )]
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempRepo {
        path: std::path::PathBuf,
    }

    impl TempRepo {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir()
                .join(format!("perl-ci-hygiene-panic-test-{label}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(path.join("ci"))?;
            fs::create_dir_all(path.join("crates/demo/tests"))?;
            fs::create_dir_all(path.join("crates/demo/src"))?;
            fs::write(path.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/demo\"]\n")?;
            fs::write(
                path.join("crates/demo/Cargo.toml"),
                "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )?;
            Ok(Self { path })
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn count_panic_in_test_code_counts_integration_tests() -> Result<()> {
        let repo = TempRepo::new("integration")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            r#"
#[test]
fn demo() {
    panic!("boom");
}
"#,
        )?;
        assert_eq!(count_panic_in_test_code(&repo.path)?, 1);
        Ok(())
    }

    #[test]
    fn complete_inventory_counts_test_suffixed_source_files() -> Result<()> {
        let repo = TempRepo::new("test-suffix")?;
        fs::write(
            repo.path.join("crates/demo/src/parser_tests.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        let inventory = complete_panic_site_inventory(&repo.path)?;
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].path, "crates/demo/src/parser_tests.rs");
        assert_eq!(inventory[0].enclosing_test_or_function, "demo");
        Ok(())
    }

    #[test]
    fn count_panic_in_test_code_ignores_production_code() -> Result<()> {
        let repo = TempRepo::new("production")?;
        fs::write(
            repo.path.join("crates/demo/src/lib.rs"),
            r#"
pub fn boom() {
    panic!("production");
}
"#,
        )?;
        assert_eq!(count_panic_in_test_code(&repo.path)?, 0);
        Ok(())
    }

    #[test]
    fn check_panic_test_fails_when_count_exceeds_baseline() -> Result<()> {
        let repo = TempRepo::new("baseline-fail")?;
        fs::write(repo.path.join("ci/panic_test_baseline.txt"), "0\n")?;
        fs::write(
            repo.path.join("crates/demo/tests/demo.rs"),
            "#[test]\nfn demo() { panic!(\"boom\"); }\n",
        )?;
        assert_eq!(check_panic_test(&repo.path)?, 1);
        Ok(())
    }

    #[test]
    fn check_panic_test_errors_when_baseline_missing() -> Result<()> {
        let repo = TempRepo::new("baseline-missing")?;
        let err = check_panic_test(&repo.path).unwrap_err();
        assert!(err.to_string().contains("missing"), "expected missing-baseline error, got: {err}");
        Ok(())
    }

    #[test]
    fn check_panic_test_errors_when_baseline_empty() -> Result<()> {
        let repo = TempRepo::new("baseline-empty")?;
        fs::write(repo.path.join("ci/panic_test_baseline.txt"), "  \n")?;
        let err = check_panic_test(&repo.path).unwrap_err();
        assert!(err.to_string().contains("empty"), "expected empty-baseline error, got: {err}");
        Ok(())
    }
}
