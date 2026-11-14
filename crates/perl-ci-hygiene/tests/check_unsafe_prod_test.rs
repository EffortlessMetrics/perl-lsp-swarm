#![deny(clippy::map_err_ignore)]
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
            .join(format!("perl-ci-hygiene-unsafe-prod-{label}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        fs::write(path.join("Cargo.toml"), "[workspace]\nmembers = []\n")?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_baseline(&self, content: &str) -> TestResult<()> {
        let ci = self.path.join("ci");
        fs::create_dir_all(&ci)?;
        fs::write(ci.join("unsafe_prod_baseline.txt"), content)?;
        Ok(())
    }

    fn write_crate_src(&self, crate_name: &str, rel: &str, content: &str) -> TestResult<()> {
        let file = self.path.join("crates").join(crate_name).join(rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(file, content)?;
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

fn run_check_unsafe_prod(repo: &Path) -> TestResult<Output> {
    let bin = perl_ci_hygiene_binary()?;
    Ok(Command::new(bin).arg("check-unsafe-prod").current_dir(repo).output()?)
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(".."))
}

#[test]
fn workspace_unsafe_prod_baseline_matches_inventory() -> TestResult {
    let root = workspace_root();
    let out = run_check_unsafe_prod(&root)?;
    let stdout = stdout_of(&out);
    assert!(
        out.status.success(),
        "check-unsafe-prod failed:\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("match the expected site set"), "{stdout}");
    Ok(())
}

#[test]
fn rejects_bare_integer_baseline() -> TestResult {
    let repo = TempRepo::new("bare-int")?;
    repo.write_baseline("12\n")?;
    repo.write_crate_src("demo", "src/lib.rs", "pub fn ok() {}\n")?;
    let out = run_check_unsafe_prod(repo.path())?;
    let combined = format!("{}{}", stdout_of(&out), String::from_utf8_lossy(&out.stderr));
    assert!(!out.status.success());
    assert!(combined.contains("bare integer ratchet"), "{combined}");
    Ok(())
}

#[test]
fn detects_unexpected_site_not_in_baseline() -> TestResult {
    let repo = TempRepo::new("unexpected")?;
    repo.write_baseline(
        "# expected\n\
         crates/demo/src/lib.rs:2\n",
    )?;
    repo.write_crate_src(
        "demo",
        "src/lib.rs",
        "// SAFETY: test stub\n\
         unsafe { () }\n\
         // SAFETY: second site\n\
         unsafe { () }\n",
    )?;
    let out = run_check_unsafe_prod(repo.path())?;
    let stdout = stdout_of(&out);
    assert!(!out.status.success(), "{stdout}");
    assert!(stdout.contains("Unexpected sites"), "{stdout}");
    assert!(stdout.contains("crates/demo/src/lib.rs:4"), "{stdout}");
    Ok(())
}

#[test]
fn detects_missing_site_listed_in_baseline() -> TestResult {
    let repo = TempRepo::new("missing")?;
    repo.write_baseline(
        "# expected\n\
         crates/demo/src/lib.rs:2\n\
         crates/demo/src/removed.rs:1\n",
    )?;
    repo.write_crate_src(
        "demo",
        "src/lib.rs",
        "// SAFETY: only site\n\
         unsafe { () }\n",
    )?;
    let out = run_check_unsafe_prod(repo.path())?;
    let stdout = stdout_of(&out);
    assert!(!out.status.success(), "{stdout}");
    assert!(stdout.contains("Missing sites"), "{stdout}");
    assert!(stdout.contains("crates/demo/src/removed.rs:1"), "{stdout}");
    Ok(())
}

#[test]
fn site_swap_with_unchanged_count_fails() -> TestResult {
    let repo = TempRepo::new("swap")?;
    repo.write_baseline(
        "# two sites\n\
         crates/demo/src/a.rs:2\n\
         crates/demo/src/b.rs:2\n",
    )?;
    repo.write_crate_src(
        "demo",
        "src/a.rs",
        "// SAFETY: a\n\
         unsafe { () }\n",
    )?;
    repo.write_crate_src("demo", "src/b.rs", "pub fn fine() {}\n")?;
    repo.write_crate_src(
        "demo",
        "src/c.rs",
        "// SAFETY: moved site\n\
         unsafe { () }\n",
    )?;
    let out = run_check_unsafe_prod(repo.path())?;
    let stdout = stdout_of(&out);
    assert!(!out.status.success(), "{stdout}");
    assert!(stdout.contains("Unexpected sites"), "{stdout}");
    assert!(stdout.contains("crates/demo/src/c.rs:2"), "{stdout}");
    assert!(stdout.contains("Missing sites"), "{stdout}");
    assert!(stdout.contains("crates/demo/src/b.rs:2"), "{stdout}");
    Ok(())
}
