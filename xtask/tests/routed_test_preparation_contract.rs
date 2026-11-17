//! Compilation staging cannot substitute for the routed runtime proof (#17459).
//!
//! #17151: the routed sweep must also *run* every selected test target. Cargo
//! exits after the first failing test executable unless `--no-fail-fast` is
//! set, so one red target in an alphabetically-earlier crate silently skipped
//! every later crate's whole `tests/` suite while the receipt still showed
//! only that first failure.
use serde_yaml_ng::Value;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::tempdir;
use xtask::cargo_target_census::test_target_reports;

fn root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("missing workspace root")?
        .to_path_buf())
}

/// The fixture's alphabetically-first integration test target, which fails.
const EARLY_TARGET: &str = "tests/aaa_early_failing.rs";
/// The fixture's alphabetically-last integration test target, which passes.
const LATER_TARGET: &str = "tests/zzz_later_passing.rs";

#[test]
fn preparation_preserves_runtime_command_and_budget() -> Result<(), Box<dyn std::error::Error>> {
    let policy: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root()?.join(".ci/gate-policy.yaml"))?)?;
    let gates = policy["gates"].as_sequence().ok_or("missing gates")?;
    let find =
        |name| gates.iter().find(|gate| gate["name"].as_str() == Some(name)).ok_or("missing gate");
    let runtime = find("unit_routed_full")?;
    let build = find("unit_routed_full_build")?;
    let command = runtime["command"].as_str().ok_or("missing runtime command")?;
    assert_eq!(
        command,
        // #17151: `--no-fail-fast` is required, not decorative. Without it
        // cargo exits at the first failing test executable and the routed
        // sweep silently skips every alphabetically-later crate's `tests/`
        // suite, so the gate can report a failure while most of its selected
        // targets never ran. The compile-only sibling carries the same flag so
        // the two commands stay in lockstep (the `--no-run` suffix is the only
        // difference between them).
        "cargo build -p perllsp --locked && env PERL_LSP_BIN=\"$PWD/target/debug/perllsp\" cargo test --locked --tests {package_args} --no-fail-fast"
    );
    assert_eq!(
        build["command"].as_str().and_then(|value| value.strip_suffix(" --no-run")),
        Some(command)
    );
    assert!(!command.contains("--no-run"));
    // The abort-on-first-failure override must survive in both the runtime
    // and the compile-only command; the runtime one is the coverage
    // guarantee, the compile-only one keeps the two rows identical apart from
    // `--no-run`.
    assert!(command.contains("--no-fail-fast"));
    assert!(build["command"].as_str().unwrap_or_default().contains("--no-fail-fast"));
    for key in ["tier", "required", "quarantine", "planning", "timeout_seconds"] {
        assert_eq!(build[key], runtime[key], "compile and runtime must share {key}");
    }
    assert_eq!(runtime["timeout_seconds"].as_u64(), Some(1500));
    assert_eq!(runtime["retry_count"].as_u64(), Some(1));
    assert_eq!(build["retry_count"].as_u64(), Some(0));
    Ok(())
}

/// #17151: the routed sweep runs every selected test target, even when an
/// alphabetically-earlier target fails.
///
/// The control is a fixture workspace with two integration test targets: one
/// that fails and one that passes, named so the failing target sorts first.
/// The fixture is run with the flags the policy's own routed command carries,
/// so removing `--no-fail-fast` from `.ci/gate-policy.yaml` fails this control
/// instead of silently restoring the masking.
///
/// The control discriminates in both directions. With the policy's flags the
/// later target runs; without `--no-fail-fast` cargo aborts at the first
/// failing executable and the later target never runs. A fixture whose later
/// target ran either way would prove nothing.
#[test]
fn routed_sweep_runs_every_selected_target_after_an_early_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let policy: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root()?.join(".ci/gate-policy.yaml"))?)?;
    let gates = policy["gates"].as_sequence().ok_or("missing gates")?;
    let runtime = gates
        .iter()
        .find(|gate| gate["name"].as_str() == Some("unit_routed_full"))
        .ok_or("missing gate")?;
    let command = runtime["command"].as_str().ok_or("missing runtime command")?;
    let flags = routed_cargo_test_flags(command)?;
    assert!(
        flags.iter().any(|flag| flag == "--no-fail-fast"),
        "the routed command must carry --no-fail-fast; got {flags:?}"
    );

    let fixture = tempdir()?;
    write_sweep_fixture(fixture.path())?;

    // The policy's own flags: the later target must run after the early
    // failure.
    let with_flag = run_sweep_fixture(fixture.path(), &flags)?;
    assert!(
        !with_flag.status.success(),
        "the fixture's early target must fail, or the control proves nothing"
    );
    let executed = executed_targets(&with_flag.output);
    assert!(
        executed.iter().any(|target| target == EARLY_TARGET),
        "the failing target must have run: {executed:?}"
    );
    assert!(
        executed.iter().any(|target| target == LATER_TARGET),
        "the later target must run after the early failure; this is the coverage the abort \
         skipped: {executed:?}"
    );

    // Without the flag: cargo aborts at the first failing executable, so the
    // later target never runs. This is the defect the flag repairs.
    let without_flag = run_sweep_fixture(fixture.path(), &flags_without_fail_fast(&flags))?;
    assert!(
        !without_flag.status.success(),
        "the fixture's early target must fail without the flag too"
    );
    let executed = executed_targets(&without_flag.output);
    assert!(
        executed.iter().any(|target| target == EARLY_TARGET),
        "the failing target must have run: {executed:?}"
    );
    assert!(
        !executed.iter().any(|target| target == LATER_TARGET),
        "without --no-fail-fast cargo must abort before the later target; a fixture whose later \
         target ran anyway would not discriminate: {executed:?}"
    );
    Ok(())
}

#[test]
fn preparation_precedes_watchdog_and_has_separate_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow: Value =
        serde_yaml_ng::from_str(&fs::read_to_string(root()?.join(".github/workflows/ci.yml"))?)?;
    let required = &workflow["jobs"]["check-all-targets"];
    assert_eq!(required["name"].as_str(), Some("Compile All Targets (bit-rot guard)"));
    let required_steps = required["steps"].as_sequence().ok_or("missing required steps")?;
    let producer_step = required_steps
        .iter()
        .find(|step| {
            step["run"].as_str() == Some("bash scripts/ci/required-merge-surface.sh")
                && step["name"].as_str() == Some("Required merge surface")
        })
        .ok_or("missing shared required preparation contract producer")?;
    let route_command = fs::read_to_string(root()?.join("scripts/ci/required-merge-surface.sh"))?;
    assert!(
        route_command
            .contains("cargo test -p xtask --test routed_test_preparation_contract --locked --")
    );
    assert!(route_command.contains("3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;"));
    assert!(route_command.contains("preparation_preserves_runtime_command_and_budget"));
    assert!(
        route_command.contains("routed_sweep_runs_every_selected_target_after_an_early_failure")
    );
    assert!(route_command.contains("preparation_precedes_watchdog_and_has_separate_evidence"));
    assert_eq!(producer_step["continue-on-error"], Value::Null);
    // The revival sibling (pr-fast-unblocked, #19045) copies the preparation
    // and watchdog shape so the tier runs without the fatal disk floor.
    // Assert both jobs stay in lockstep; a one-sided edit must fail here,
    // not drift silently.
    for (job_name, receipt_prefix, missing_prefix) in [
        ("pr-smoke", "pr-fast-receipt-", "pr-fast-missing-receipt-"),
        ("pr-fast-unblocked", "pr-fast-unblocked-receipt-", "pr-fast-unblocked-missing-receipt-"),
    ] {
        let job = &workflow["jobs"][job_name];
        assert_eq!(job["timeout-minutes"].as_u64(), Some(75));
        let steps = job["steps"].as_sequence().ok_or("missing steps")?;
        let prepare = steps
            .iter()
            .position(|step| {
                step["name"].as_str() == Some("Prepare routed integration-test targets")
            })
            .ok_or("missing preparation")?;
        let execute = steps
            .iter()
            .position(|step| {
                step["name"].as_str() == Some("Run PR-fast via shared xtask gate runner")
            })
            .ok_or("missing runtime")?;
        assert!(prepare < execute);
        let build = steps[prepare]["run"].as_str().ok_or("missing preparation command")?;
        assert!(
            build.contains(
                "--gate unit_routed_full_build --subject target/receipts/ci-subject.json"
            )
        );
        assert!(build.contains("--receipt-path target/receipts/routed-test-build/receipt.json"));
        assert!(!build.contains("--receipt-path target/receipts/receipt.json"));
        assert!(!build.contains("|| true"));
        assert!(build.contains("preparation_status=$?"));
        assert!(build.contains("exit \"$preparation_status\""));
        assert!(
            build.contains("target/receipts/routed-test-build/logs/unit_routed_full_build.log")
        );
        assert_eq!(
            steps[prepare]["continue-on-error"].as_bool(),
            Some(true),
            "failed preparation must retain its receipt and allow all independent full-tier proofs"
        );
        assert_eq!(
            steps[prepare]["timeout-minutes"].as_u64(),
            Some(25),
            "preparation must be bounded inside the 75m job envelope"
        );
        assert!(build.contains("routed-test-build/logs/unit_routed_full_build.log"));
        let runtime = steps[execute]["run"].as_str().ok_or("missing runtime command")?;
        assert!(runtime.contains("timeout --signal=TERM --kill-after=60s 3600s"));
        assert!(
            runtime.contains(
                "gates --tier pr-fast --subject target/receipts/ci-subject.json --receipt"
            )
        );
        assert!(!runtime.contains("--gate unit_routed_full_build"));
        let upload = steps
            .iter()
            .find(|step| {
                step["with"]["name"].as_str().is_some_and(|name| name.starts_with(receipt_prefix))
            })
            .ok_or("missing receipt artifact")?;
        let upload_if = upload["if"].as_str().ok_or("missing receipt upload guard")?;
        assert!(
            upload_if.contains("hashFiles('target/receipts/receipt.json') != ''"),
            "tier-named receipt upload must require receipt.json (#17915)"
        );
        let missing = steps
            .iter()
            .find(|step| {
                step["with"]["name"].as_str().is_some_and(|name| name.starts_with(missing_prefix))
            })
            .ok_or("missing missing-receipt diagnostics artifact")?;
        let missing_if = missing["if"].as_str().ok_or("missing diagnostics upload guard")?;
        assert!(
            missing_if.contains("hashFiles('target/receipts/receipt.json') == ''"),
            "missing-receipt diagnostics must publish only without receipt.json"
        );
        let paths = upload["with"]["path"].as_str().ok_or("missing artifact paths")?;
        assert!(
            paths
                .lines()
                .any(|path| path.trim() == "target/receipts/routed-test-build/receipt.json")
        );
        assert!(paths.lines().any(|path| path.trim() == "target/receipts/logs/*.log"));
        assert!(
            paths.lines().any(|path| path.trim() == "target/receipts/routed-test-build/logs/*.log")
        );
    }
    Ok(())
}

/// The `cargo test` flags the policy's routed command carries.
///
/// The routed command is `cargo build … && env VAR=… cargo test <flags>`. The
/// package placeholders are dropped because the fixture is a single package;
/// every other flag the policy sets is passed through verbatim so the fixture
/// exercises the policy's own cargo invocation.
fn routed_cargo_test_flags(command: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let cargo_test = command.rsplit("&&").next().unwrap_or(command);
    let tokens: Vec<&str> = cargo_test.split_whitespace().collect();
    let test_index = tokens
        .iter()
        .position(|token| *token == "test")
        .ok_or("the routed command must invoke `cargo test`")?;
    let mut flags = Vec::new();
    let mut skip_next = false;
    for token in &tokens[test_index + 1..] {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *token == "{package_args}" {
            continue;
        }
        if *token == "-p" {
            skip_next = true;
            continue;
        }
        flags.push((*token).to_string());
    }
    Ok(flags)
}

/// The same flags with the abort-on-first-failure override removed.
fn flags_without_fail_fast(flags: &[String]) -> Vec<String> {
    flags.iter().filter(|flag| flag.as_str() != "--no-fail-fast").cloned().collect()
}

/// Write the fixture workspace: two integration test targets, the
/// alphabetically-first of which fails.
fn write_sweep_fixture(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join("tests"))?;
    fs::write(
        root.join("Cargo.toml"),
        "\
[package]
name = \"routed-sweep-fixture\"
version = \"0.0.0\"
edition = \"2021\"
publish = false

# An empty `[workspace]` table detaches the fixture from any parent workspace
# so cargo builds it as its own workspace root.
[workspace]
",
    )?;
    fs::write(
        root.join(EARLY_TARGET),
        "\
#[test]
fn early_target_fails() {
    assert_eq!(1, 2, \"deliberate early failure in the routed-sweep fixture\");
}
",
    )?;
    fs::write(
        root.join(LATER_TARGET),
        "\
#[test]
fn later_target_passes() {
    assert_eq!(2 + 2, 4);
}
",
    )?;
    Ok(())
}

/// The outcome of one fixture sweep.
struct SweepRun {
    status: std::process::ExitStatus,
    output: String,
}

/// Run the fixture with the given `cargo test` flags.
fn run_sweep_fixture(
    root: &Path,
    flags: &[String],
) -> Result<SweepRun, Box<dyn std::error::Error>> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsStr::new("cargo").to_os_string());
    // The fixture has no dependencies, so its lockfile is generated offline
    // and the `--locked` flag the policy carries stays meaningful.
    let lockfile = cargo_command(&cargo, root).args(["generate-lockfile", "--offline"]).output()?;
    if !lockfile.status.success() {
        return Err(format!(
            "fixture lockfile generation failed:\n{}",
            String::from_utf8_lossy(&lockfile.stderr)
        )
        .into());
    }
    let output = cargo_command(&cargo, root).arg("test").args(flags).arg("--offline").output()?;
    Ok(SweepRun {
        status: output.status,
        // Cargo reports the targets it ran on stderr and the test harness
        // results on stdout; the census needs both.
        output: format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    })
}

/// A cargo invocation against the fixture, isolated from the outer build.
fn cargo_command(cargo: &OsStr, root: &Path) -> Command {
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        // The fixture builds into its own target directory so it never
        // contends with the outer build's lock.
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env("CARGO_TERM_COLOR", "never")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTFLAGS");
    command
}

/// The test targets cargo reported as executed.
fn executed_targets(output: &str) -> Vec<String> {
    test_target_reports(output)
        .into_iter()
        .filter(|report| !report.built)
        .map(|report| report.target)
        .collect()
}
