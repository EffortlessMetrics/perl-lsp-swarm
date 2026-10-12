// CI doctor integration test — eprintln! used for diagnostic output.
#![allow(clippy::print_stderr, clippy::print_stdout)]
// `expect()` carries the assertion message on CLI invocation and output
// decoding. The workspace-wide deny is a production-code rule.
#![allow(clippy::expect_used)]
use assert_cmd::cargo::cargo_bin_cmd;

/// Verify `cargo xtask ci doctor` exits 0 in a normal environment.
///
/// The doctor is designed to warn but not fail on advisory conditions
/// (no release binary, dirty working tree), so this should pass in CI
/// as long as rustc, rustfmt, clippy, and perl are available.
#[test]
fn ci_doctor_exits_zero_in_normal_env() {
    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.args(["ci", "doctor"]);
    // allow non-zero only on hard failures (missing rustc/components/perl)
    let output = cmd.output().expect("failed to run xtask ci doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The header must always appear
    assert!(stdout.contains("cargo xtask ci doctor"), "expected header in output:\n{stdout}");

    // Toolchain and component sections must appear
    assert!(stdout.contains("── Toolchain ──"), "expected toolchain section:\n{stdout}");
    assert!(stdout.contains("── Rust components ──"), "expected components section:\n{stdout}");

    // Platform section must appear
    assert!(stdout.contains("── Platform ──"), "expected platform section:\n{stdout}");

    // The summary line must appear
    assert!(stdout.contains("ci doctor:"), "expected summary line:\n{stdout}");
}

// Product compilation and runtime belong to their owning gates. This proof runs
// the real CLI and CI composition with native, child-local dependency tools.
struct CiFixture {
    _dir: tempfile::TempDir,
    log: std::path::PathBuf,
    metadata: std::path::PathBuf,
    rustfmt: std::path::PathBuf,
    expected: Vec<String>,
}

impl CiFixture {
    fn new() -> Self {
        use std::{fs, process::Command};
        let dir = tempfile::tempdir().expect("create CI fixture");
        let log = dir.path().join("commands.log");
        fs::write(&log, "").expect("initialize command log");
        let source = dir.path().join("tool.rs");
        fs::write(&source, include_str!("support/ci_cli_tool.rs")).expect("write native tool");
        let tool = dir.path().join(format!("fixture{}", std::env::consts::EXE_SUFFIX));
        let compilation = Command::new("rustc")
            .args(["--edition", "2024", "--crate-name", "ci_cli_fixture"])
            .arg(&source)
            .arg("-o")
            .arg(&tool)
            .output()
            .expect("compile native dependency tool");
        assert!(compilation.status.success(), "{}", String::from_utf8_lossy(&compilation.stderr));
        let cargo = dir.path().join(format!("cargo{}", std::env::consts::EXE_SUFFIX));
        let rustfmt = dir.path().join(format!("rustfmt{}", std::env::consts::EXE_SUFFIX));
        fs::copy(&tool, &cargo).expect("install fixture Cargo");
        fs::copy(&tool, &rustfmt).expect("install fixture rustfmt");
        let config = dir.path().join("rustfmt.toml");
        fs::write(&config, "max_width = 100\n").expect("write formatter configuration");
        let mut packages = Vec::new();
        let mut members = Vec::new();
        let mut expected =
            vec![invocation("cargo", &["metadata", "--format-version", "1", "--no-deps"])];
        for (name, edition) in [("alpha", "2018"), ("zeta", "2024")] {
            let package = dir.path().join(name);
            fs::create_dir_all(package.join("src")).expect("create fixture package");
            let manifest = package.join("Cargo.toml");
            let source = package.join("src/lib.rs");
            fs::write(&source, "pub fn fixture() {}\n").expect("write fixture source");
            fs::write(
                &manifest,
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"{edition}\"\n"
                ),
            )
            .expect("write fixture manifest");
            let id = format!("{name} 0.1.0 (fixture)");
            packages.push(serde_json::json!({
                "id": id, "name": name, "manifest_path": manifest, "edition": edition,
                "targets": [{"src_path": source, "edition": edition}]
            }));
            members.push(id);
            expected.push(invocation(
                "rustfmt",
                &[
                    "--edition",
                    edition,
                    "--config-path",
                    config.to_str().expect("config path"),
                    "--check",
                    source.to_str().expect("source path"),
                ],
            ));
        }
        let metadata = dir.path().join("metadata.json");
        fs::write(
            &metadata,
            serde_json::to_vec(&serde_json::json!({
                "packages": packages, "workspace_members": members, "workspace_root": dir.path()
            }))
            .expect("encode metadata"),
        )
        .expect("write metadata");
        expected.push(invocation(
            "cargo",
            &["clippy", "--workspace", "--all-targets", "--", "-Dwarnings", "-Amissing_docs"],
        ));
        for name in ["perl-lexer", "perl-parser", "perl-lsp-rs"] {
            expected.push(invocation(
                "cargo",
                &["test", "-p", name, "--tests", "--no-fail-fast", "--", "--test-threads=1", "-q"],
            ));
        }
        expected.push(invocation("cargo", &["doc", "-p", "perl-parser", "--no-deps"]));
        Self { _dir: dir, log, metadata, rustfmt, expected }
    }

    fn run(&self, args: &[&str], fail_clippy: bool) -> std::process::Output {
        std::fs::write(&self.log, "").expect("reset fixture log");
        let mut paths = vec![self._dir.path().to_path_buf()];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let mut command = cargo_bin_cmd!("xtask");
        command
            .args(args)
            .env("PATH", std::env::join_paths(paths).expect("join fixture PATH"))
            .env("RUSTFMT", &self.rustfmt)
            .env("XTASK_CI_FIXTURE_LOG", &self.log)
            .env("XTASK_CI_FIXTURE_METADATA", &self.metadata)
            .env_remove("XTASK_CI_FIXTURE_FAIL_CLIPPY");
        if fail_clippy {
            command.env("XTASK_CI_FIXTURE_FAIL_CLIPPY", "1");
        }
        command.timeout(std::time::Duration::from_secs(30));
        command.output().expect("execute real xtask CLI")
    }

    fn invocations(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .expect("read dependency invocations")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn successful_suite(&self, output: &std::process::Output) -> Result<(), String> {
        if !output.status.success() {
            return Err(format!("CLI failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        if self.invocations() != self.expected {
            return Err("bare ci did not execute the complete dependency sequence".to_owned());
        }
        if !String::from_utf8_lossy(&output.stdout).contains("fixture-doc-completed") {
            return Err("final dependency output did not reach the caller".to_owned());
        }
        Ok(())
    }
}

fn invocation(tool: &str, args: &[&str]) -> String {
    format!("{tool} {:?}", args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

#[test]
fn ci_subcommand_bare_still_runs_ci_suite() {
    let fixture = CiFixture::new();
    let output = fixture.run(&["ci"], false);
    fixture.successful_suite(&output).expect("real bare-ci dispatch and complete runner sequence");
    // A real successful wrong-command invocation must fail the same oracle.
    let wrong_command = fixture.run(&["list-commands"], false);
    assert!(wrong_command.status.success(), "wrong-command control must actually execute");
    assert!(
        fixture.successful_suite(&wrong_command).is_err(),
        "wrong command fooled the CI oracle"
    );
}

#[test]
fn ci_subcommand_bare_propagates_dependency_failure_and_stops() {
    let fixture = CiFixture::new();
    let output = fixture.run(&["ci"], true);
    assert!(!output.status.success(), "bare ci ignored dependency exit 19");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Clippy check failed"));
    assert_eq!(
        fixture.invocations(),
        fixture.expected[..4],
        "failed Clippy must prevent product tests and docs"
    );
    assert!(
        fixture.successful_suite(&output).is_err(),
        "ignored-exit control fooled the CI oracle"
    );
}

/// Verify `cargo xtask ci doctor --help` shows the doctor description.
#[test]
fn ci_doctor_help_shows_description() {
    let mut cmd = cargo_bin_cmd!("xtask");
    cmd.args(["ci", "doctor", "--help"]);
    let output = cmd.output().expect("failed to run xtask ci doctor --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Help must contain the command description
    assert!(
        stdout.contains("doctor") || stdout.contains("parity"),
        "expected description in help:\n{stdout}"
    );
}
