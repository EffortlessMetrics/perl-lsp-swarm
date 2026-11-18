//! The routed sweep must run every selected test target (#17151).
//!
//! `unit_routed_full` runs `cargo test --locked --tests {package_args}`. Cargo
//! exits after the first failing test executable unless `--no-fail-fast` is
//! set, and the routed package list comes from a `BTreeSet`, so it is
//! alphabetical. One red target in an alphabetically-earlier crate therefore
//! silently skipped every later crate's whole `tests/` suite while the receipt
//! still showed only that first failure.
//!
//! This suite carries the discriminating control for that repair. It is a
//! separate file from `routed_test_preparation_contract` because that suite's
//! command is an immutable required-merge-surface block pinned to a baseline
//! commit; a new contract appends after that immutable prefix instead of
//! editing it.
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
/// The command the required merge-surface producer runs for this suite.
const PRODUCER_COMMAND: &str =
    "cargo test -p xtask --test routed_test_sweep_control --locked -- --nocapture";

/// The routed sweep runs every selected test target, even when an
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

/// This control only proves something if a lane actually executes it.
///
/// The suite is reachable from the required merge-surface producer, and that
/// producer runs in the required `Compile All Targets (bit-rot guard)`
/// context. A suite that only compiles would leave the masking repair
/// unobserved (#14585 / #14178 failure mode).
#[test]
fn the_sweep_control_is_wired_into_the_required_merge_surface()
-> Result<(), Box<dyn std::error::Error>> {
    let root = root()?;
    let producer = fs::read_to_string(root.join("scripts/ci/required-merge-surface.sh"))?;
    assert!(
        producer.lines().any(|line| line.trim() == PRODUCER_COMMAND),
        "the required merge-surface producer must execute this suite; a suite that only compiles \
         proves nothing"
    );
    let workflow = fs::read_to_string(root.join(".github/workflows/ci.yml"))?;
    assert!(
        required_context_executes_producer(&workflow)?,
        "producer must belong to the required job"
    );
    let document: Value = serde_yaml_ng::from_str(&workflow)?;
    let mut moved = document.clone();
    moved["jobs"]["check-all-targets"]["name"] = Value::String("Advisory replacement".into());
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&moved)?)?);
    let mut disabled = document.clone();
    disabled["jobs"]["check-all-targets"]["if"] = Value::Bool(false);
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&disabled)?)?);
    let mut missing_need = document.clone();
    missing_need["jobs"]["check-all-targets"]["needs"] =
        Value::Sequence(vec![Value::String("draft-pr-check".into())]);
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&missing_need)?)?);
    for mask_job in [true, false] {
        let mut masked = document.clone();
        if mask_job {
            masked["jobs"]["check-all-targets"]["continue-on-error"] = Value::Bool(true);
        } else {
            for step in
                masked["jobs"]["check-all-targets"]["steps"].as_sequence_mut().ok_or("steps")?
            {
                if step["run"]
                    .as_str()
                    .is_some_and(|run| run.trim() == "bash scripts/ci/required-merge-surface.sh")
                {
                    step["continue-on-error"] = Value::Bool(true);
                }
            }
        }
        assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&masked)?)?);
    }
    let mut extra_need = document.clone();
    extra_need["jobs"]["check-all-targets"]["needs"]
        .as_sequence_mut()
        .ok_or("needs")?
        .push(Value::String("unrelated".into()));
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&extra_need)?)?);
    extra_need["jobs"]["check-all-targets"]["if"] = Value::Null;
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&extra_need)?)?);
    let mut wrapped = document.clone();
    let original = wrapped["jobs"]["check-all-targets"]["if"].as_str().ok_or("job condition")?;
    wrapped["jobs"]["check-all-targets"]["if"] =
        Value::String(format!("  ${{{{  {original}  }}}}  "));
    wrapped["jobs"]["check-all-targets"]["needs"].as_sequence_mut().ok_or("needs")?.reverse();
    assert!(required_context_executes_producer(&serde_yaml_ng::to_string(&wrapped)?)?);
    let mut disabled_step = document;
    let steps = disabled_step["jobs"]["check-all-targets"]["steps"]
        .as_sequence_mut()
        .ok_or("steps missing")?;
    for step in steps {
        if step["run"]
            .as_str()
            .is_some_and(|run| run.trim() == "bash scripts/ci/required-merge-surface.sh")
        {
            step["if"] = Value::String("${{ false }}".into());
        }
    }
    assert!(!required_context_executes_producer(&serde_yaml_ng::to_string(&disabled_step)?)?);
    Ok(())
}

fn expression_body(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix("${{").and_then(|body| body.strip_suffix("}}")).unwrap_or(text).trim()
}

fn required_context_executes_producer(workflow: &str) -> Result<bool, serde_yaml_ng::Error> {
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(workflow)?;
    let job = &document["jobs"]["check-all-targets"];
    let legitimate_condition = "needs.draft-pr-check.outputs.run_ci == 'true' && needs.preflight-latest-check.outputs.is_latest == 'true'";
    let direct_needs = job["needs"].as_sequence().is_some_and(|needs| {
        needs.len() == 2
            && ["draft-pr-check", "preflight-latest-check"]
                .iter()
                .all(|required| needs.iter().any(|need| need.as_str() == Some(required)))
    });
    let not_masked = |value: &Value| {
        value.is_null()
            || value.as_bool() == Some(false)
            || value.as_str().is_some_and(|text| expression_body(text) == "false")
    };
    let job_enabled = job["if"].is_null()
        || job["if"]
            .as_str()
            .is_some_and(|condition| expression_body(condition) == legitimate_condition);
    Ok(job_enabled
        && direct_needs
        && not_masked(&job["continue-on-error"])
        && job["name"].as_str() == Some("Compile All Targets (bit-rot guard)")
        && job["steps"].as_sequence().is_some_and(|steps| {
            steps.iter().any(|step| {
                not_masked(&step["continue-on-error"])
                    && (step["if"].is_null()
                        || step["if"].as_bool() == Some(true)
                        || step["if"].as_str().is_some_and(|condition| {
                            matches!(expression_body(condition), "true" | "success()")
                        }))
                    && step["run"].as_str().is_some_and(|run| {
                        run.trim() == "bash scripts/ci/required-merge-surface.sh"
                    })
            })
        }))
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
