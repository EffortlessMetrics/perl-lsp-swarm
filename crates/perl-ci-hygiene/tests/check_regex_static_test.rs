#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new(label: &str) -> TestResult<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = env::temp_dir()
            .join(format!("perl-ci-hygiene-regex-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        fs::write(path.join("Cargo.toml"), "[workspace]\n")?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_crate_src(&self, crate_name: &str, file: &str, content: &str) -> TestResult<()> {
        let src = self.path.join("crates").join(crate_name).join("src");
        fs::create_dir_all(&src)?;
        fs::write(src.join(file), content)?;
        Ok(())
    }

    fn write_crate_test(&self, crate_name: &str, file: &str, content: &str) -> TestResult<()> {
        let tests = self.path.join("crates").join(crate_name).join("tests");
        fs::create_dir_all(&tests)?;
        fs::write(tests.join(file), content)?;
        Ok(())
    }

    fn write_baseline(&self, count: usize) -> TestResult<()> {
        let ci = self.path.join("ci");
        fs::create_dir_all(&ci)?;
        fs::write(ci.join("regex_static_baseline.txt"), format!("{count}\n"))?;
        Ok(())
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn perl_ci_hygiene_binary() -> TestResult<PathBuf> {
    env::var_os("CARGO_BIN_EXE_perl-ci-hygiene").map(PathBuf::from).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "CARGO_BIN_EXE_perl-ci-hygiene was not set by cargo",
        )
        .into()
    })
}

fn run_check_regex_static(repo: &Path) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    Ok(Command::new(bin).arg("check-regex-static").current_dir(repo).output()?)
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A bare `Regex::new(...)` in a function body exceeds a zero baseline and fails.
#[test]
fn detects_per_call_regex_new() -> TestResult {
    let repo = TempRepo::new("per-call")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
pub fn matches(pat: &str, hay: &str) -> bool {
    let re = Regex::new(pat).unwrap();
    re.is_match(hay)
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_ne!(
        out.status.code(),
        Some(0),
        "per-call Regex::new should fail against a zero baseline\nstdout: {}",
        stdout_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("FAIL"), "output should mention FAIL\nstdout: {stdout}");
    assert!(
        stdout.contains("lib.rs:4"),
        "output should point at the offending line\nstdout: {stdout}"
    );
    Ok(())
}

/// A single-line `LazyLock<Regex>` static passes — the pattern is compiled once.
#[test]
fn allows_single_line_lazylock_static() -> TestResult {
    let repo = TempRepo::new("single-line-lazy")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
use std::sync::LazyLock;
static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").unwrap());
pub fn word_count(s: &str) -> usize {
    WORD_RE.find_iter(s).count()
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "single-line LazyLock<Regex> should pass\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// A multi-line `LazyLock::new(|| { ... Regex::new(...) ... })` initializer passes.
#[test]
fn allows_multi_line_lazylock_static() -> TestResult {
    let repo = TempRepo::new("multi-line-lazy")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
use std::sync::LazyLock;
static COMPLEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    let pattern = r"\d{3}-\d{4}";
    Regex::new(pattern).expect("valid pattern")
});
pub fn is_phone(s: &str) -> bool {
    COMPLEX_RE.is_match(s)
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "multi-line LazyLock<Regex> should pass\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// An `OnceLock` populated via `get_or_init(|| Regex::new(...))` passes.
#[test]
fn allows_oncelock_get_or_init() -> TestResult {
    let repo = TempRepo::new("oncelock")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
use std::sync::OnceLock;
static CACHE: OnceLock<Regex> = OnceLock::new();
pub fn cached() -> &'static Regex {
    CACHE.get_or_init(|| {
        Regex::new(r"[a-z]+").unwrap()
    })
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "OnceLock get_or_init should pass\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// `RegexBuilder::new(...)` and `Regex::builder(...)` are detected too.
#[test]
fn detects_regex_builder_variants() -> TestResult {
    let repo = TempRepo::new("builder-variants")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::RegexBuilder;
pub fn ci(pat: &str) {
    let _ = RegexBuilder::new(pat).case_insensitive(true).build();
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_ne!(
        out.status.code(),
        Some(0),
        "per-call RegexBuilder::new should fail against a zero baseline\nstdout: {}",
        stdout_of(&out)
    );
    Ok(())
}

/// Regex constructors inside `#[cfg(test)]` modules are excluded from the scan.
#[test]
fn ignores_cfg_test_modules() -> TestResult {
    let repo = TempRepo::new("cfg-test")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
pub fn safe() {}

#[cfg(test)]
mod tests {
    use regex::Regex;
    #[test]
    fn t() {
        let _ = Regex::new(r"x").unwrap();
    }
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "Regex::new inside #[cfg(test)] must be excluded\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// Regex constructors in a `tests/` directory are excluded from the scan.
#[test]
fn ignores_tests_directory() -> TestResult {
    let repo = TempRepo::new("tests-dir")?;
    repo.write_baseline(0)?;
    // A clean lib so the crate exists.
    repo.write_crate_src("my-crate", "lib.rs", "pub fn safe() {}\n")?;
    repo.write_crate_test(
        "my-crate",
        "integration.rs",
        "fn helper() { let _ = regex::Regex::new(r\"x\").unwrap(); }\n",
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "Regex::new in tests/ must be excluded\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// A non-zero baseline absorbs the accepted runtime-input exceptions: a count
/// equal to the baseline passes.
#[test]
fn count_equal_to_baseline_passes() -> TestResult {
    let repo = TempRepo::new("equal-baseline")?;
    repo.write_baseline(1)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
pub fn runtime(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "count == baseline should pass\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// When the count drops below the baseline, the gate passes and nudges a ratchet-down.
#[test]
fn below_baseline_prints_note() -> TestResult {
    let repo = TempRepo::new("below-baseline")?;
    repo.write_baseline(5)?;
    repo.write_crate_src("my-crate", "lib.rs", "pub fn clean() {}\n")?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "below-baseline should pass\nstdout: {}",
        stdout_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("NOTE") && stdout.contains("ratchet down"),
        "below-baseline should nudge a ratchet-down\nstdout: {stdout}"
    );
    Ok(())
}

/// A commented-out regex constructor is not counted.
#[test]
fn ignores_commented_regex() -> TestResult {
    let repo = TempRepo::new("commented")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        "pub fn safe() {}\n// let _ = Regex::new(r\"x\"); // example in a comment\n",
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "commented-out Regex::new must not count\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// The literal text `Regex::new(` inside a string literal (help text, error
/// message, doc example) must NOT be counted as a violation.
#[test]
fn ignores_regex_text_in_string_literal() -> TestResult {
    let repo = TempRepo::new("string-literal")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
pub fn help_text() -> &'static str {
    "Example usage: Regex::new(pattern) compiles a pattern with braces {("
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_eq!(
        out.status.code(),
        Some(0),
        "Regex::new mentioned in a string literal must not count\nstdout: {}\nstderr: {}",
        stdout_of(&out),
        String::from_utf8_lossy(&out.stderr),
    );
    Ok(())
}

/// Two regex constructors on one line count as two, not one.
#[test]
fn counts_two_ctors_on_one_line() -> TestResult {
    let repo = TempRepo::new("two-on-one")?;
    // Baseline of 1 must still fail: the line holds two real per-call ctors.
    repo.write_baseline(1)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
pub fn two(p1: &str, p2: &str) -> (Regex, Regex) {
    (Regex::new(p1).unwrap(), Regex::new(p2).unwrap())
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_ne!(
        out.status.code(),
        Some(0),
        "two ctors on one line must count as two (exceeds baseline 1)\nstdout: {}",
        stdout_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(stdout.contains("count (2)"), "the reported count should be 2\nstdout: {stdout}");
    Ok(())
}

/// An unbalanced brace inside a string inside a lazy closure must not leak the
/// scope forward and mask a later, unrelated per-call regex.
#[test]
fn unbalanced_brace_in_string_does_not_mask_later_violation() -> TestResult {
    let repo = TempRepo::new("scope-leak")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
use std::sync::LazyLock;
static RE: LazyLock<Regex> = LazyLock::new(|| {
    let _doc = "closing syntax looks like this: });";
    Regex::new(r"\d+").unwrap()
});
pub fn bad(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    // The lazy static is fine, but `bad`'s per-call regex is a real violation and
    // must still be counted despite the brace-laden string in the closure above.
    assert_ne!(
        out.status.code(),
        Some(0),
        "a later per-call regex must not be masked by a string brace in a lazy closure\nstdout: {}",
        stdout_of(&out)
    );
    let stdout = stdout_of(&out);
    assert!(
        stdout.contains("count (1)"),
        "exactly the one real violation should be counted\nstdout: {stdout}"
    );
    Ok(())
}

/// A doc comment that merely mentions the lazy-init opener must not activate the
/// scope and thereby mask a later per-call regex.
#[test]
fn doc_comment_opener_does_not_leak_scope() -> TestResult {
    let repo = TempRepo::new("doc-comment-leak")?;
    repo.write_baseline(0)?;
    repo.write_crate_src(
        "my-crate",
        "lib.rs",
        r#"
use regex::Regex;
/// Example: static RE: LazyLock<Regex> = LazyLock::new(|| {
pub fn safe() {}
pub fn later_bad(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}
"#,
    )?;

    let out = run_check_regex_static(repo.path())?;
    assert_ne!(
        out.status.code(),
        Some(0),
        "a comment mentioning the opener must not mask a real later violation\nstdout: {}",
        stdout_of(&out)
    );
    Ok(())
}
