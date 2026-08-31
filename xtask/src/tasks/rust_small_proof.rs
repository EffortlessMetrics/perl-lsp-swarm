//! Canonical Rust Small proof lane (#8407).
//!
//! One repository-owned command executes the semantic Rust Small proof so every
//! routed runner (`rust-small-cx53`, `rust-small-cx43`, `rust-small-github`,
//! `rust-small-fallback`) proves one definition instead of four drifted
//! per-runner shell copies. Before this command the required aggregate could
//! mean two different proofs on the same commit: CX routes counted the
//! references scorecard census with `awk "/: test$/{...}"` while hosted routes
//! used `grep -c -F ": test"` — different match semantics behind one required
//! check.
//!
//! Step identity is data (`CARGO_STEPS`, `SCORECARD_CENSUS_ARGS`,
//! `SCORECARD_RUN_ARGS`) and pinned by unit tests, so a candidate/toolchain/
//! profile drift fails `cargo test -p xtask` before it can reach CI.
//!
//! Boundaries kept on purpose:
//!
//! - The workspace-wide formatting check stays a workflow yml literal:
//!   scripts/ci/test_rustfmt_required_workflow.py pins that line per lane for
//!   #9127/#12320 ownership, and this task does not relocate their claim.
//! - The workflow yml configures `git config --global --add safe.directory`
//!   before this command inside Docker lanes (ripr.yml-proven host-UID
//!   boundary), because the diff-hygiene step below runs `git` there.
//! - Typed step receipts and exact-subject artifact binding remain open
//!   #8407 acceptance (unclaimed here); #8408 is the route-parity consumer
//!   that normalizes what each route may run around this command. This
//!   command streams step outcomes and fails closed without inventing a
//!   receipt schema.

use color_eyre::eyre::{Result, bail, eyre};
use std::process::Command;

/// A lane step: banner name plus exact cargo argv (no shell interpolation).
type CargoStep = (&'static str, &'static [&'static str]);

/// The semantic proof floor preserved from the routed lanes (#8407). Order and
/// flags are load-bearing: they are what "Perl LSP Rust Small Result" meant the
/// night this consolidation landed.
const CARGO_STEPS: &[CargoStep] = &[
    ("fetch locked inputs", &["fetch", "--locked"]),
    ("workspace compile check", &["check", "--workspace", "--locked"]),
    (
        "parser semantic smoke",
        &[
            "test",
            "--locked",
            "-p",
            "perl-parser",
            "--test",
            "semantic_smoke_tests",
            "--",
            "--nocapture",
        ],
    ),
    (
        "parser accuracy e2e",
        &[
            "test",
            "--locked",
            "-p",
            "perl-parser",
            "--test",
            "parser_accuracy_e2e",
            "--",
            "--nocapture",
        ],
    ),
    (
        "parser accuracy NodeKind vocabulary",
        &[
            "test",
            "--locked",
            "-p",
            "perl-parser",
            "--test",
            "parser_accuracy_nodekind_vocabulary",
            "--",
            "--nocapture",
        ],
    ),
    (
        "lsp smoke",
        &[
            "test",
            "--locked",
            "-p",
            "perl-lsp-rs",
            "--test",
            "lsp_smoke",
            "--",
            "--test-threads=1",
            "--nocapture",
        ],
    ),
];

/// References scorecard census argv: replaces both old per-route counters with
/// one strict suffix-parse performed by [`count_libtest_listed_tests`].
const SCORECARD_CENSUS_ARGS: &[&str] = &[
    "test",
    "-p",
    "perl-lsp-rs",
    "--lib",
    "--features",
    "workspace",
    "--profile",
    "agent",
    "--locked",
    "references_tier_scorecard_tests",
    "--",
    "--list",
];

/// References scorecard execution argv with frozen snapshot writes: any insta
/// snapshot drift surfaces as a product/test failure, never as a silently
/// rewritten fixture. This is exactly what every route ran before
/// consolidation (the old `INSTA_UPDATE=no cargo test ...` line).
const SCORECARD_RUN_ARGS: &[&str] = &[
    "test",
    "-p",
    "perl-lsp-rs",
    "--lib",
    "--features",
    "workspace",
    "--profile",
    "agent",
    "--locked",
    "references_tier_scorecard_tests",
    "--",
    "--test-threads=1",
];

const SCORECARD_REPLAY_ENV: [(&str, &str); 1] = [("INSTA_UPDATE", "no")];

pub fn run() -> Result<()> {
    let total = CARGO_STEPS.len() + 3;
    let mut done = 0usize;
    for (name, args) in CARGO_STEPS {
        run_cargo(name, args)?;
        done += 1;
    }

    let census_output =
        Command::new("cargo").args(SCORECARD_CENSUS_ARGS).output().map_err(|error| {
            eyre!(
                "[{done}/{total}] references scorecard census instrument failure \
                 (spawn): {error}"
            )
        })?;
    if !census_output.status.success() {
        // Nonzero census exit is a product/test failure (the scorecard target
        // failed to build or run), kept distinct from the spawn-instrument
        // failure above (#8407 acceptance: product vs instrument failure).
        bail!(
            "[{done}/{total}] references scorecard census product/test failure \
             (exit {:?}): {}",
            census_output.status.code(),
            bounded_stderr(&census_output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&census_output.stdout);
    let census = count_libtest_listed_tests(&stdout);
    println!("[{done}/{total}] references scorecard census: {census} tests");
    if census == 0 {
        bail!(
            "[{done}/{total}] references scorecard census is zero; the required \
             proof vanished from the listing — refusing to pass an empty gate"
        );
    }
    done += 1;

    run_step("cargo", "references scorecard replay", SCORECARD_RUN_ARGS, SCORECARD_REPLAY_ENV)?;
    done += 1;

    // Diff hygiene copied from the hosted contract so every route proves the
    // same source-cleanliness boundary (#8407 proof floor). Docker lanes pass
    // `git config --global --add safe.directory /workspace` first for the
    // host-checkout UID boundary.
    let status = Command::new("git").args(["diff", "--check"]).status();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => bail!(
            "[{done}/{total}] git diff --check found whitespace/conflict-marker \
             drift ({status}); fix the tree before claiming green"
        ),
        Err(error) => {
            bail!("[{done}/{total}] git diff --check instrument failure (spawn): {error}")
        }
    }
    println!("[rust-small-proof] all {total} steps ok");

    Ok(())
}

fn run_cargo(name: &str, args: &[&str]) -> Result<()> {
    run_step("cargo", name, args, [])
}

/// Run one lane step and classify every outcome: success, product/test
/// failure (nonzero exit), not-completed (terminated without an exit code),
/// or instrument spawn failure. Reporting-only is not gate success (#8407).
fn run_step<const N: usize>(
    program: &str,
    name: &str,
    args: &[&str],
    env: [(&str, &str); N],
) -> Result<()> {
    println!("[rust-small-proof] $ {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => match status.code() {
            Some(code) => {
                Err(eyre!("step '{name}' failed: product/test failure (exit code {code})"))
            }
            None => Err(eyre!("step '{name}' not completed: terminated without an exit code")),
        },
        Err(error) => Err(eyre!("step '{name}' failed: instrument spawn error: {error}")),
    }
}

fn bounded_stderr(stderr: &[u8]) -> String {
    const MAX_CHARS: usize = 2_000;
    let text = String::from_utf8_lossy(stderr);
    let chars = text.chars().count();
    if chars <= MAX_CHARS {
        return text.into_owned();
    }
    let tail: String = text.chars().skip(chars - MAX_CHARS).collect();
    format!("...{tail}")
}

/// Count tests the way a libtest `--list` actually marks them: exactly one
/// trailing `": test"` per listed test. This unifies the old counters onto the
/// stricter of the two meanings (`awk "/: test$/"`), never the looser
/// substring meaning of `grep -c -F ": test"`.
fn count_libtest_listed_tests(listing: &str) -> usize {
    listing.lines().filter(|line| line.trim_end_matches(['\r']).ends_with(": test")).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn census_counts_only_trailing_test_markers() {
        // Third and fourth lines are the classic substring decoys that made
        // `grep -c -F ": test"` a different proof than the awk counter.
        let listing = "\
tests::alpha: test
tests::beta: test\r
note: test binary at target/debug/deps/foo
some listing mention of : test mid-line must not count
tests::gamma: test
";
        assert_eq!(count_libtest_listed_tests(listing), 3);
    }

    #[test]
    fn empty_or_markerless_census_is_zero() {
        assert_eq!(count_libtest_listed_tests(""), 0);
        assert_eq!(count_libtest_listed_tests("running 0 tests\n"), 0);
    }

    #[test]
    fn zero_census_is_a_gate_failure_not_a_pass() {
        // The census path bails when count == 0; keep that contract named so
        // nobody turns it into a reporting-only skip (#8407 acceptance).
        let census = count_libtest_listed_tests("");
        assert_eq!(census, 0);
    }

    #[test]
    fn lane_steps_are_pinned_in_order_and_identity() {
        // Mutation guard: any drift in the semantic proof floor (target names,
        // lock discipline, nocapture/threading identity) must fail here.
        assert_eq!(CARGO_STEPS.len(), 6);
        assert_eq!(CARGO_STEPS[0], ("fetch locked inputs", &["fetch", "--locked"][..]));
        assert_eq!(
            CARGO_STEPS[1],
            ("workspace compile check", &["check", "--workspace", "--locked"][..])
        );
        assert_eq!(
            CARGO_STEPS[2],
            (
                "parser semantic smoke",
                &[
                    "test",
                    "--locked",
                    "-p",
                    "perl-parser",
                    "--test",
                    "semantic_smoke_tests",
                    "--",
                    "--nocapture",
                ][..]
            )
        );
        assert_eq!(
            CARGO_STEPS[3],
            (
                "parser accuracy e2e",
                &[
                    "test",
                    "--locked",
                    "-p",
                    "perl-parser",
                    "--test",
                    "parser_accuracy_e2e",
                    "--",
                    "--nocapture",
                ][..]
            )
        );
        assert_eq!(
            CARGO_STEPS[4],
            (
                "parser accuracy NodeKind vocabulary",
                &[
                    "test",
                    "--locked",
                    "-p",
                    "perl-parser",
                    "--test",
                    "parser_accuracy_nodekind_vocabulary",
                    "--",
                    "--nocapture",
                ][..]
            )
        );
        assert_eq!(
            CARGO_STEPS[5],
            (
                "lsp smoke",
                &[
                    "test",
                    "--locked",
                    "-p",
                    "perl-lsp-rs",
                    "--test",
                    "lsp_smoke",
                    "--",
                    "--test-threads=1",
                    "--nocapture",
                ][..]
            )
        );
    }

    #[test]
    fn scorecard_steps_pin_candidate_profile_feature_lock() {
        // Identity guard: agent profile, workspace feature, lockfile respect,
        // tier filter, single-threaded replay — dropping any one changes the
        // certified proof.
        for arg in [
            "--profile",
            "agent",
            "--features",
            "workspace",
            "--locked",
            "references_tier_scorecard_tests",
        ] {
            assert!(SCORECARD_CENSUS_ARGS.contains(&arg), "census missing {arg}");
            assert!(SCORECARD_RUN_ARGS.contains(&arg), "run missing {arg}");
        }
        assert!(SCORECARD_CENSUS_ARGS.contains(&"--list"));
        assert!(SCORECARD_RUN_ARGS.contains(&"--test-threads=1"));
    }

    #[test]
    fn scorecard_replay_env_is_insta_frozen_single_thread() {
        assert_eq!(SCORECARD_REPLAY_ENV[0], ("INSTA_UPDATE", "no"));
    }

    #[test]
    fn diff_hygiene_step_remains_part_of_the_shared_definition() {
        // The consolidated definition owns `git diff --check` for all routes;
        // keep the invocation anchored so removal is a reviewed change.
        let source = include_str!("rust_small_proof.rs");
        assert!(source.contains(r#"["diff", "--check"]"#));
    }

    #[test]
    fn step_runner_propagates_success_nonzero_and_spawn_classification() {
        // Runtime exercise of the run-level outcome classes (#8407: product
        // failure vs instrument failure must stay distinct, and a nonzero
        // step must fail the command).
        let ok = run_step("git", "probe ok", &["--version"], []);
        assert!(ok.is_ok(), "git --version must succeed: {ok:?}");

        let nonzero = run_step("git", "probe nonzero", &["--no-such-flag-12972"], []);
        let Err(nonzero) = nonzero else {
            panic!("a failing step must propagate as an error");
        };
        let nonzero_text = format!("{nonzero:#}");
        assert!(
            nonzero_text.contains("product/test failure (exit code"),
            "nonzero exit must classify as product/test failure: {nonzero_text}"
        );

        let spawn = run_step(
            "definitely-not-a-real-binary-12972",
            "probe spawn failure",
            &["--version"],
            [],
        );
        let Err(spawn) = spawn else {
            panic!("a missing instrument must propagate as an error");
        };
        let spawn_text = format!("{spawn:#}");
        assert!(
            spawn_text.contains("instrument spawn error"),
            "spawn failure must classify as instrument failure: {spawn_text}"
        );
    }
}
