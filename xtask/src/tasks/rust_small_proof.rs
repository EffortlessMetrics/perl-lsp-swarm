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
//! - Typed step receipts and exact-subject artifact binding are the remaining
//!   #8407 acceptance, delivered here: every run emits one versioned
//!   [`RustSmallProofReceipt`] binding the candidate SHA, toolchain, and
//!   scorecard profile/feature identity to every selected step's typed
//!   outcome. #8408 is the route-parity consumer that normalizes what each
//!   route may run around this command; adopting the receipt as route
//!   evidence in the workflow yml is that issue's claim, not this one's.

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
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

/// Banner names for the three non-`CARGO_STEPS` lane steps. Named constants so
/// the receipt's step vocabulary and the runtime banners cannot drift apart.
const CENSUS_STEP: &str = "references scorecard census";
const REPLAY_STEP: &str = "references scorecard replay";
const DIFF_HYGIENE_STEP: &str = "diff hygiene";

const DIFF_HYGIENE_ARGS: &[&str] = &["diff", "--check"];

/// Default receipt destination, matching the `target/receipts/` convention
/// every other xtask receipt uses (`xtask/CLAUDE.md`).
pub const DEFAULT_RECEIPT_PATH: &str = "target/receipts/rust-small-proof.json";

/// Receipt schema identity. Bump the suffix when the shape changes so an old
/// consumer refuses a receipt it cannot read instead of silently misreading it.
pub const RECEIPT_SCHEMA_VERSION: &str = "rust_small_proof.v1";

/// Typed per-step outcome. `NotRun` is a first-class state: when an early step
/// fails the lane stops, and the receipt must say the remaining steps were
/// *not run* rather than omitting them — an omitted step and a skipped step
/// have to stay distinguishable (#8407: a missing required step cannot yield
/// success).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    Ok,
    /// Nonzero exit: the product or its tests failed.
    ProductFailure,
    /// Terminated without an exit code (signal/cancellation).
    NotCompleted,
    /// The instrument itself could not run (spawn error, unreadable output).
    InstrumentFailure,
    /// A preceding step failed closed before this one was reached.
    NotRun,
}

/// Terminal classification for the whole lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofResult {
    Success,
    ProductFailure,
    NotCompleted,
    InstrumentFailure,
}

/// One selected step, recorded with the exact argv that ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepRecord {
    pub name: String,
    /// Full argv including the program, e.g. `["cargo", "fetch", "--locked"]`.
    pub argv: Vec<String>,
    pub outcome: StepOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Exact subject the proof ran against. Every field is derived from something
/// the run actually observed (`git rev-parse`, `rustc -V`, `cargo -V`) or from
/// the pinned argv itself — never from a duplicated literal that could drift
/// away from what the steps really used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofSubject {
    pub git_sha: String,
    pub rustc_version: String,
    pub cargo_version: String,
    pub scorecard_profile: String,
    pub scorecard_features: String,
    pub locked: bool,
}

/// One versioned receipt binding candidate/toolchain/profile identity to every
/// selected step (#8407 acceptance).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RustSmallProofReceipt {
    pub schema_version: String,
    pub subject: ProofSubject,
    pub steps: Vec<StepRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scorecard_census: Option<usize>,
    pub result: ProofResult,
}

/// The full ordered step vocabulary: the six pinned cargo steps, then census,
/// replay, and diff hygiene. Built from the same constants the runtime uses, so
/// a step added to `CARGO_STEPS` without a receipt entry is impossible.
fn expected_steps() -> Vec<(String, Vec<String>)> {
    let mut steps: Vec<(String, Vec<String>)> =
        CARGO_STEPS.iter().map(|(name, args)| ((*name).to_string(), argv("cargo", args))).collect();
    steps.push((CENSUS_STEP.to_string(), argv("cargo", SCORECARD_CENSUS_ARGS)));
    steps.push((REPLAY_STEP.to_string(), argv("cargo", SCORECARD_RUN_ARGS)));
    steps.push((DIFF_HYGIENE_STEP.to_string(), argv("git", DIFF_HYGIENE_ARGS)));
    steps
}

fn argv(program: &str, args: &[&str]) -> Vec<String> {
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(program.to_string());
    all.extend(args.iter().map(|arg| (*arg).to_string()));
    all
}

/// Read the value following `flag` in a pinned argv slice, so the receipt's
/// profile/feature identity is derived from the argv that actually ran.
fn flag_value(args: &[&str], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| *arg == flag)
        .and_then(|at| args.get(at + 1))
        .map(|v| (*v).to_string())
}

/// Fail closed on any receipt that does not prove what it claims.
///
/// `expected_subject` is the subject the *consumer* requires (the current
/// checkout, for `--verify-receipt`). Passing `None` checks shape and internal
/// consistency only.
pub fn verify_receipt(
    receipt: &RustSmallProofReceipt,
    expected_subject: Option<&ProofSubject>,
) -> Result<()> {
    if receipt.schema_version != RECEIPT_SCHEMA_VERSION {
        bail!(
            "receipt schema '{}' is not '{RECEIPT_SCHEMA_VERSION}': malformed or stale receipt, \
             refusing to read it as Rust Small proof",
            receipt.schema_version
        );
    }

    let expected = expected_steps();
    if receipt.steps.len() != expected.len() {
        bail!(
            "receipt records {} steps but the lane defines {}: an omitted or extra step cannot \
             yield a valid proof",
            receipt.steps.len(),
            expected.len()
        );
    }
    for (recorded, (name, argv)) in receipt.steps.iter().zip(expected.iter()) {
        if &recorded.name != name {
            bail!(
                "receipt step '{}' is not the expected '{name}' at that position: step set or \
                 order drifted from the canonical lane",
                recorded.name
            );
        }
        if &recorded.argv != argv {
            bail!(
                "receipt step '{name}' argv {:?} does not match the pinned lane argv {argv:?}: \
                 the receipt does not describe the canonical proof",
                recorded.argv
            );
        }
    }

    // Unreached steps must form a suffix: the lane stops at its first failure,
    // so an `ok` step after a `not_run` one describes a run that cannot have
    // happened.
    if let Some(first_not_run) =
        receipt.steps.iter().position(|step| step.outcome == StepOutcome::NotRun)
        && let Some(later) = receipt
            .steps
            .iter()
            .skip(first_not_run)
            .find(|step| step.outcome != StepOutcome::NotRun)
    {
        bail!(
            "receipt records step '{}' as {:?} after an earlier not_run step: unreached steps \
             must form a suffix",
            later.name,
            later.outcome
        );
    }

    // Outcome/exit-code coherence. A `product_failure` may legitimately carry
    // exit code 0: the zero-census failure is exactly the product failure a
    // process exit code cannot express, and the receipt records it truthfully.
    for step in &receipt.steps {
        match step.outcome {
            StepOutcome::NotRun | StepOutcome::NotCompleted | StepOutcome::InstrumentFailure => {
                if step.exit_code.is_some() {
                    bail!(
                        "receipt step '{}' recorded {:?} with exit code {:?}: that outcome has no \
                         exit code",
                        step.name,
                        step.outcome,
                        step.exit_code
                    );
                }
            }
            StepOutcome::Ok => {
                if let Some(code) = step.exit_code
                    && code != 0
                {
                    bail!("receipt step '{}' recorded ok with nonzero exit code {code}", step.name);
                }
            }
            StepOutcome::ProductFailure => {}
        }
    }

    // The terminal result must reconcile with the first step that did not pass.
    let first_failure = receipt.steps.iter().find(|step| step.outcome != StepOutcome::Ok);
    match receipt.result {
        ProofResult::Success => {
            if let Some(step) = first_failure {
                bail!(
                    "receipt claims success while step '{}' recorded {:?}: a swallowed failure \
                     cannot be reported as green",
                    step.name,
                    step.outcome
                );
            }
            match receipt.scorecard_census {
                Some(0) | None => bail!(
                    "receipt claims success without a nonzero references scorecard census: \
                     refusing to accept an empty gate as proof"
                ),
                Some(_) => {}
            }
        }
        result => {
            let Some(step) = first_failure else {
                bail!(
                    "receipt claims {result:?} while every step recorded ok: a forged failure \
                     result cannot describe this lane"
                );
            };
            let implied = match step.outcome {
                StepOutcome::ProductFailure => ProofResult::ProductFailure,
                StepOutcome::NotCompleted => ProofResult::NotCompleted,
                StepOutcome::InstrumentFailure => ProofResult::InstrumentFailure,
                StepOutcome::NotRun => bail!(
                    "receipt claims {result:?} but its first non-ok step '{}' is not_run: a lane \
                     cannot stop before something failed",
                    step.name
                ),
                StepOutcome::Ok => bail!(
                    "receipt step '{}' classified as the first failure while recording ok",
                    step.name
                ),
            };
            if implied != result {
                bail!(
                    "receipt claims {result:?} but its first failing step '{}' recorded {:?}, \
                     which implies {implied:?}",
                    step.name,
                    step.outcome
                );
            }
            // A census the lane never reached cannot have produced a count.
            if let Some(census_step) = receipt.steps.get(CARGO_STEPS.len())
                && census_step.outcome == StepOutcome::NotRun
                && receipt.scorecard_census.is_some()
            {
                bail!(
                    "receipt reports a scorecard census of {:?} while the census step was never \
                     run",
                    receipt.scorecard_census
                );
            }
        }
    }

    if let Some(expected_subject) = expected_subject
        && &receipt.subject != expected_subject
    {
        bail!(
            "receipt subject does not match the verifying checkout.\n  receipt:  {:?}\n  \
             expected: {expected_subject:?}",
            receipt.subject
        );
    }

    Ok(())
}

fn capture_stdout(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| eyre!("subject capture instrument failure: {program} (spawn): {error}"))?;
    if !output.status.success() {
        bail!(
            "subject capture instrument failure: {program} {} exited {:?}",
            args.join(" "),
            output.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Capture the exact candidate/toolchain/profile subject *before* running the
/// proof, so an identity problem fails in seconds rather than after the full
/// lane has burned its runner minutes.
///
/// **This function is the trust root of the whole receipt claim.** A receipt
/// certifies "this subject *as observed here*", and nothing downstream can
/// re-derive that independently: the success-path self-check compares the
/// receipt against the very value this returned, and `--verify-receipt` calls
/// this same function, so producer and consumer would agree on an identical
/// wrong answer. If this ever reported something other than the checkout the
/// steps actually ran against — a `git rev-parse HEAD` racing a concurrent
/// checkout, or a refactor that read identity from a different source —
/// verification would still pass on a receipt that is wrong.
///
/// Closing that gap means recording raw command output and re-executing it at
/// verification time to byte-compare, which is a design change rather than a
/// tightening of this function. Until then, treat capture correctness as an
/// assumption of the claim, not something the receipt proves.
fn capture_subject() -> Result<ProofSubject> {
    let scorecard_profile = flag_value(SCORECARD_RUN_ARGS, "--profile").ok_or_else(|| {
        eyre!("scorecard argv lost its --profile pin; receipt cannot bind a profile")
    })?;
    let scorecard_features = flag_value(SCORECARD_RUN_ARGS, "--features").ok_or_else(|| {
        eyre!("scorecard argv lost its --features pin; receipt cannot bind features")
    })?;
    Ok(ProofSubject {
        git_sha: capture_stdout("git", &["rev-parse", "HEAD"])?,
        rustc_version: capture_stdout("rustc", &["--version"])?,
        cargo_version: capture_stdout("cargo", &["--version"])?,
        scorecard_profile,
        scorecard_features,
        locked: SCORECARD_RUN_ARGS.contains(&"--locked"),
    })
}

/// Remove a receipt left by an earlier run. A receipt that cannot be destroyed
/// fails the lane immediately: continuing would risk leaving a stale artifact
/// that outlives the run it claims to describe.
fn invalidate_prior_receipt(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(eyre!(
            "could not invalidate the prior receipt at {}: {error}; refusing to run with a stale \
             artifact in place",
            path.display()
        )),
    }
}

fn write_receipt(path: &Path, receipt: &RustSmallProofReceipt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .wrap_err_with(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(receipt)
        .wrap_err("serializing the Rust Small proof receipt")?;
    fs::write(path, json).wrap_err_with(|| format!("writing receipt {}", path.display()))
}

/// Read and validate a receipt against the current checkout. Runs no proof
/// steps: this is the consumer seam (#8408 route parity) for asking "does this
/// artifact actually certify this candidate?".
fn run_verify(path: &Path) -> Result<()> {
    let text =
        fs::read_to_string(path).wrap_err_with(|| format!("reading receipt {}", path.display()))?;
    let receipt: RustSmallProofReceipt = serde_json::from_str(&text).wrap_err_with(|| {
        format!("malformed receipt {}: not a valid receipt document", path.display())
    })?;
    verify_receipt(&receipt, Some(&capture_subject()?))?;
    println!(
        "[rust-small-proof] receipt {} verified: {} steps, census {:?}, result {:?}",
        path.display(),
        receipt.steps.len(),
        receipt.scorecard_census,
        receipt.result
    );
    Ok(())
}

pub fn run(receipt: Option<PathBuf>, verify: Option<PathBuf>) -> Result<()> {
    if let Some(path) = verify {
        return run_verify(&path);
    }
    let receipt_path = receipt.unwrap_or_else(|| PathBuf::from(DEFAULT_RECEIPT_PATH));
    run_proof(&receipt_path)
}

/// Accumulates step records so that a lane which fails closed still emits a
/// complete receipt describing what ran, what failed, and what never started.
struct Recorder {
    steps: Vec<StepRecord>,
}

impl Recorder {
    fn new() -> Self {
        Self { steps: Vec::new() }
    }

    fn record(
        &mut self,
        name: &str,
        argv: Vec<String>,
        outcome: StepOutcome,
        exit_code: Option<i32>,
    ) {
        self.steps.push(StepRecord { name: name.to_string(), argv, outcome, exit_code });
    }

    /// Mark every step the lane never reached, preserving the full vocabulary.
    fn fill_not_run(&mut self) {
        let expected = expected_steps();
        for (name, argv) in expected.into_iter().skip(self.steps.len()) {
            self.steps.push(StepRecord {
                name,
                argv,
                outcome: StepOutcome::NotRun,
                exit_code: None,
            });
        }
    }
}

fn run_proof(receipt_path: &Path) -> Result<()> {
    // Destroy any prior receipt before the first fallible step. `target/` is
    // reused across runs, so a receipt left by an earlier *successful* run of
    // the same candidate and toolchain would still verify — and every failure
    // below either returns before writing (subject capture) or tolerates a
    // write failure (`fail_closed`). Without this, a failed rerun could leave
    // a verifiable green artifact describing a run that did not happen.
    invalidate_prior_receipt(receipt_path)?;

    // Bind the subject next: a candidate/toolchain identity problem should
    // fail in seconds, not after the lane has burned its full runtime.
    let subject = capture_subject()?;

    let total = CARGO_STEPS.len() + 3;
    let mut done = 0usize;
    let mut recorder = Recorder::new();
    let mut census_count: Option<usize> = None;

    for (name, args) in CARGO_STEPS {
        let execution = execute_step("cargo", args, []);
        recorder.record(name, argv("cargo", args), execution.outcome(), execution.exit_code());
        if let Some(error) = execution.as_error(name) {
            return fail_closed(
                receipt_path,
                &subject,
                recorder,
                census_count,
                execution.result(),
                error,
            );
        }
        done += 1;
    }

    let census_argv = argv("cargo", SCORECARD_CENSUS_ARGS);
    println!("[rust-small-proof] $ cargo {}", SCORECARD_CENSUS_ARGS.join(" "));
    let census_output = match Command::new("cargo").args(SCORECARD_CENSUS_ARGS).output() {
        Ok(output) => output,
        Err(error) => {
            recorder.record(CENSUS_STEP, census_argv, StepOutcome::InstrumentFailure, None);
            return fail_closed(
                receipt_path,
                &subject,
                recorder,
                census_count,
                ProofResult::InstrumentFailure,
                eyre!(
                    "[{done}/{total}] references scorecard census instrument failure \
                     (spawn): {error}"
                ),
            );
        }
    };
    if !census_output.status.success() {
        // Nonzero census exit is a product/test failure (the scorecard target
        // failed to build or run), kept distinct from the spawn-instrument
        // failure above (#8407 acceptance: product vs instrument failure).
        recorder.record(
            CENSUS_STEP,
            census_argv,
            StepOutcome::ProductFailure,
            census_output.status.code(),
        );
        return fail_closed(
            receipt_path,
            &subject,
            recorder,
            census_count,
            ProofResult::ProductFailure,
            eyre!(
                "[{done}/{total}] references scorecard census product/test failure \
                 (exit {:?}): {}",
                census_output.status.code(),
                bounded_stderr(&census_output.stderr)
            ),
        );
    }
    let stdout = String::from_utf8_lossy(&census_output.stdout);
    let census = count_libtest_listed_tests(&stdout);
    println!("[{done}/{total}] references scorecard census: {census} tests");
    if census == 0 {
        // An empty listing exits zero, so this is the one product failure the
        // exit code cannot express. Record the census we actually observed so
        // the receipt shows *why* the lane refused to go green.
        census_count = Some(0);
        recorder.record(
            CENSUS_STEP,
            census_argv,
            StepOutcome::ProductFailure,
            census_output.status.code(),
        );
        return fail_closed(
            receipt_path,
            &subject,
            recorder,
            census_count,
            ProofResult::ProductFailure,
            eyre!(
                "[{done}/{total}] references scorecard census is zero; the required \
                 proof vanished from the listing — refusing to pass an empty gate"
            ),
        );
    }
    census_count = Some(census);
    recorder.record(CENSUS_STEP, census_argv, StepOutcome::Ok, census_output.status.code());
    done += 1;

    let replay = execute_step("cargo", SCORECARD_RUN_ARGS, SCORECARD_REPLAY_ENV);
    recorder.record(
        REPLAY_STEP,
        argv("cargo", SCORECARD_RUN_ARGS),
        replay.outcome(),
        replay.exit_code(),
    );
    if let Some(error) = replay.as_error(REPLAY_STEP) {
        return fail_closed(receipt_path, &subject, recorder, census_count, replay.result(), error);
    }
    done += 1;

    // Diff hygiene copied from the hosted contract so every route proves the
    // same source-cleanliness boundary (#8407 proof floor). Docker lanes pass
    // `git config --global --add safe.directory /workspace` first for the
    // host-checkout UID boundary.
    let diff_argv = argv("git", DIFF_HYGIENE_ARGS);
    let status = Command::new("git").args(DIFF_HYGIENE_ARGS).status();
    match status {
        Ok(status) if status.success() => {
            recorder.record(DIFF_HYGIENE_STEP, diff_argv, StepOutcome::Ok, status.code());
        }
        Ok(status) => {
            let outcome = match status.code() {
                Some(_) => StepOutcome::ProductFailure,
                None => StepOutcome::NotCompleted,
            };
            let result = match status.code() {
                Some(_) => ProofResult::ProductFailure,
                None => ProofResult::NotCompleted,
            };
            recorder.record(DIFF_HYGIENE_STEP, diff_argv, outcome, status.code());
            return fail_closed(
                receipt_path,
                &subject,
                recorder,
                census_count,
                result,
                eyre!(
                    "[{done}/{total}] git diff --check found whitespace/conflict-marker \
                     drift ({status}); fix the tree before claiming green"
                ),
            );
        }
        Err(error) => {
            recorder.record(DIFF_HYGIENE_STEP, diff_argv, StepOutcome::InstrumentFailure, None);
            return fail_closed(
                receipt_path,
                &subject,
                recorder,
                census_count,
                ProofResult::InstrumentFailure,
                eyre!("[{done}/{total}] git diff --check instrument failure (spawn): {error}"),
            );
        }
    }

    let receipt = RustSmallProofReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        subject,
        steps: recorder.steps,
        scorecard_census: census_count,
        result: ProofResult::Success,
    };
    // Self-check before claiming green: a success receipt that does not
    // describe the full canonical lane is not proof, and an unbound proof is
    // exactly what #8407 exists to prevent — so both fail the command.
    verify_receipt(&receipt, None)?;
    write_receipt(receipt_path, &receipt)?;

    println!("[rust-small-proof] all {total} steps ok");
    println!("[rust-small-proof] receipt written to {}", receipt_path.display());

    Ok(())
}

/// Emit the complete receipt for a failed lane, then propagate the original
/// failure. A receipt-write problem here is reported but never allowed to mask
/// the real proof failure it is describing.
fn fail_closed(
    receipt_path: &Path,
    subject: &ProofSubject,
    mut recorder: Recorder,
    census: Option<usize>,
    result: ProofResult,
    error: color_eyre::Report,
) -> Result<()> {
    recorder.fill_not_run();
    let receipt = RustSmallProofReceipt {
        schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
        subject: subject.clone(),
        steps: recorder.steps,
        scorecard_census: census,
        result,
    };
    if let Err(write_error) = write_receipt(receipt_path, &receipt) {
        eprintln!("[rust-small-proof] receipt emission also failed: {write_error:#}");
    }
    Err(error)
}

/// The classified result of running one lane step, kept as data so it can be
/// both recorded in the receipt and turned into the lane's failure.
enum StepExecution {
    Ok(Option<i32>),
    ProductFailure(i32),
    NotCompleted,
    InstrumentFailure(String),
}

impl StepExecution {
    fn outcome(&self) -> StepOutcome {
        match self {
            Self::Ok(_) => StepOutcome::Ok,
            Self::ProductFailure(_) => StepOutcome::ProductFailure,
            Self::NotCompleted => StepOutcome::NotCompleted,
            Self::InstrumentFailure(_) => StepOutcome::InstrumentFailure,
        }
    }

    fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Ok(code) => *code,
            Self::ProductFailure(code) => Some(*code),
            Self::NotCompleted | Self::InstrumentFailure(_) => None,
        }
    }

    /// Terminal lane classification for this step's failure. Only meaningful
    /// when the step did not succeed.
    fn result(&self) -> ProofResult {
        match self {
            Self::Ok(_) => ProofResult::Success,
            Self::ProductFailure(_) => ProofResult::ProductFailure,
            Self::NotCompleted => ProofResult::NotCompleted,
            Self::InstrumentFailure(_) => ProofResult::InstrumentFailure,
        }
    }

    fn as_error(&self, name: &str) -> Option<color_eyre::Report> {
        match self {
            Self::Ok(_) => None,
            Self::ProductFailure(code) => {
                Some(eyre!("step '{name}' failed: product/test failure (exit code {code})"))
            }
            Self::NotCompleted => {
                Some(eyre!("step '{name}' not completed: terminated without an exit code"))
            }
            Self::InstrumentFailure(error) => {
                Some(eyre!("step '{name}' failed: instrument spawn error: {error}"))
            }
        }
    }
}

/// Run one lane step and classify every outcome: success, product/test
/// failure (nonzero exit), not-completed (terminated without an exit code),
/// or instrument spawn failure. Reporting-only is not gate success (#8407).
fn execute_step<const N: usize>(
    program: &str,
    args: &[&str],
    env: [(&str, &str); N],
) -> StepExecution {
    println!("[rust-small-proof] $ {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    match command.status() {
        Ok(status) if status.success() => StepExecution::Ok(status.code()),
        Ok(status) => match status.code() {
            Some(code) => StepExecution::ProductFailure(code),
            None => StepExecution::NotCompleted,
        },
        Err(error) => StepExecution::InstrumentFailure(error.to_string()),
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

    /// `Result`-shaped view of one step, matching how `run_proof` turns a
    /// classification into the lane's failure.
    fn run_step<const N: usize>(
        program: &str,
        name: &str,
        args: &[&str],
        env: [(&str, &str); N],
    ) -> Result<()> {
        match execute_step(program, args, env).as_error(name) {
            Some(error) => Err(error),
            None => Ok(()),
        }
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

    // ---------------------------------------------------------------------
    // Receipt contract (#8407: one versioned receipt binding candidate,
    // toolchain, and profile identity to every selected step).
    //
    // Each negative test below mutates exactly one property of an otherwise
    // valid receipt, so a green result here means the validator rejects that
    // specific lie — not that it rejects everything.
    // ---------------------------------------------------------------------

    fn sample_subject() -> ProofSubject {
        ProofSubject {
            git_sha: "c626bb1e5f0000000000000000000000000000ab".to_string(),
            rustc_version: "rustc 1.90.0 (deadbeef 2026-01-01)".to_string(),
            cargo_version: "cargo 1.90.0 (deadbeef 2026-01-01)".to_string(),
            scorecard_profile: "agent".to_string(),
            scorecard_features: "workspace".to_string(),
            locked: true,
        }
    }

    fn success_receipt() -> RustSmallProofReceipt {
        let steps = expected_steps()
            .into_iter()
            .map(|(name, argv)| StepRecord {
                name,
                argv,
                outcome: StepOutcome::Ok,
                exit_code: Some(0),
            })
            .collect();
        RustSmallProofReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
            subject: sample_subject(),
            steps,
            scorecard_census: Some(42),
            result: ProofResult::Success,
        }
    }

    fn rejection(receipt: &RustSmallProofReceipt, subject: Option<&ProofSubject>) -> String {
        match verify_receipt(receipt, subject) {
            Ok(()) => panic!("verification accepted a receipt it must reject"),
            Err(error) => format!("{error:#}"),
        }
    }

    #[test]
    fn expected_steps_cover_the_whole_lane_in_order() {
        let steps = expected_steps();
        assert_eq!(steps.len(), CARGO_STEPS.len() + 3, "receipt vocabulary must cover every step");
        let names: Vec<&str> = steps.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names[0], "fetch locked inputs");
        assert_eq!(names[CARGO_STEPS.len()], CENSUS_STEP);
        assert_eq!(names[CARGO_STEPS.len() + 1], REPLAY_STEP);
        assert_eq!(names[CARGO_STEPS.len() + 2], DIFF_HYGIENE_STEP);
        // argv is recorded with its program so a receipt is self-describing.
        assert_eq!(steps[0].1, vec!["cargo", "fetch", "--locked"]);
        assert_eq!(steps[CARGO_STEPS.len() + 2].1, vec!["git", "diff", "--check"]);
    }

    #[test]
    fn a_valid_success_receipt_verifies_against_its_own_subject() {
        let receipt = success_receipt();
        let subject = sample_subject();
        assert!(verify_receipt(&receipt, None).is_ok());
        assert!(verify_receipt(&receipt, Some(&subject)).is_ok());
    }

    #[test]
    fn an_honest_failure_receipt_still_verifies() {
        // Negative control for the validator itself: verification asserts that
        // a receipt tells the truth, not that the lane was green. A recorded
        // failure with its untouched remainder must be accepted, otherwise the
        // tests above would pass for the wrong reason.
        let mut receipt = success_receipt();
        receipt.result = ProofResult::ProductFailure;
        receipt.scorecard_census = None;
        receipt.steps[1].outcome = StepOutcome::ProductFailure;
        receipt.steps[1].exit_code = Some(101);
        for step in receipt.steps.iter_mut().skip(2) {
            step.outcome = StepOutcome::NotRun;
            step.exit_code = None;
        }
        assert!(
            verify_receipt(&receipt, Some(&sample_subject())).is_ok(),
            "an honest failure receipt must verify"
        );
    }

    #[test]
    fn an_omitted_step_cannot_yield_a_valid_receipt() {
        let mut receipt = success_receipt();
        receipt.steps.remove(3);
        let text = rejection(&receipt, None);
        assert!(text.contains("omitted or extra step"), "{text}");
    }

    #[test]
    fn an_extra_step_cannot_yield_a_valid_receipt() {
        let mut receipt = success_receipt();
        receipt.steps.push(StepRecord {
            name: "smuggled extra proof".to_string(),
            argv: vec!["cargo".to_string(), "test".to_string()],
            outcome: StepOutcome::Ok,
            exit_code: Some(0),
        });
        let text = rejection(&receipt, None);
        assert!(text.contains("omitted or extra step"), "{text}");
    }

    #[test]
    fn a_renamed_or_reordered_step_is_refused() {
        let mut receipt = success_receipt();
        receipt.steps.swap(0, 1);
        let text = rejection(&receipt, None);
        assert!(text.contains("is not the expected"), "{text}");
    }

    #[test]
    fn step_argv_drift_is_refused() {
        // Dropping `--locked` from a recorded step changes what was proven,
        // even though the step name is untouched.
        let mut receipt = success_receipt();
        receipt.steps[0].argv.retain(|arg| arg != "--locked");
        let text = rejection(&receipt, None);
        assert!(text.contains("does not match the pinned lane argv"), "{text}");
    }

    #[test]
    fn a_swallowed_nonzero_exit_cannot_be_reported_as_success() {
        let mut receipt = success_receipt();
        receipt.steps[2].outcome = StepOutcome::ProductFailure;
        receipt.steps[2].exit_code = Some(101);
        let text = rejection(&receipt, None);
        assert!(text.contains("swallowed failure"), "{text}");
    }

    #[test]
    fn a_not_run_tail_cannot_be_reported_as_success() {
        // A success receipt whose tail never ran: the `not_run` steps form a
        // valid suffix, so this reaches the success check rather than the
        // suffix check.
        let mut receipt = success_receipt();
        for step in receipt.steps.iter_mut().skip(5) {
            step.outcome = StepOutcome::NotRun;
            step.exit_code = None;
        }
        let text = rejection(&receipt, None);
        assert!(text.contains("swallowed failure"), "{text}");
    }

    #[test]
    fn success_without_a_nonzero_census_is_refused() {
        let mut zero = success_receipt();
        zero.scorecard_census = Some(0);
        assert!(rejection(&zero, None).contains("empty gate"));

        let mut missing = success_receipt();
        missing.scorecard_census = None;
        assert!(rejection(&missing, None).contains("empty gate"));
    }

    #[test]
    fn a_stale_or_malformed_schema_version_is_refused() {
        let mut receipt = success_receipt();
        receipt.schema_version = "rust_small_proof.v0".to_string();
        let text = rejection(&receipt, None);
        assert!(text.contains("malformed or stale receipt"), "{text}");
    }

    #[test]
    fn a_wrong_candidate_identity_is_refused() {
        let receipt = success_receipt();
        let mut other_candidate = sample_subject();
        other_candidate.git_sha = "0000000000000000000000000000000000000000".to_string();
        let text = rejection(&receipt, Some(&other_candidate));
        assert!(text.contains("does not match the verifying checkout"), "{text}");
    }

    #[test]
    fn a_wrong_profile_or_toolchain_identity_is_refused() {
        let receipt = success_receipt();

        let mut other_profile = sample_subject();
        other_profile.scorecard_profile = "release".to_string();
        assert!(rejection(&receipt, Some(&other_profile)).contains("does not match"));

        let mut other_features = sample_subject();
        other_features.scorecard_features = "default".to_string();
        assert!(rejection(&receipt, Some(&other_features)).contains("does not match"));

        let mut other_toolchain = sample_subject();
        other_toolchain.rustc_version = "rustc 1.70.0".to_string();
        assert!(rejection(&receipt, Some(&other_toolchain)).contains("does not match"));
    }

    #[test]
    fn subject_profile_and_features_are_derived_from_the_pinned_argv() {
        // Identity is read out of the argv that actually runs, so the receipt
        // cannot claim a profile the scorecard step did not use.
        assert_eq!(flag_value(SCORECARD_RUN_ARGS, "--profile").as_deref(), Some("agent"));
        assert_eq!(flag_value(SCORECARD_RUN_ARGS, "--features").as_deref(), Some("workspace"));
        assert_eq!(flag_value(SCORECARD_RUN_ARGS, "--no-such-flag"), None);
        assert!(SCORECARD_RUN_ARGS.contains(&"--locked"));
    }

    #[test]
    fn a_failed_lane_records_the_untouched_remainder_as_not_run() {
        // The receipt of a lane that stopped at step 2 must still carry all
        // nine steps: omission and non-execution have to stay distinguishable.
        let mut recorder = Recorder::new();
        recorder.record(
            "fetch locked inputs",
            argv("cargo", &["fetch", "--locked"]),
            StepOutcome::Ok,
            Some(0),
        );
        recorder.record(
            "workspace compile check",
            argv("cargo", &["check", "--workspace", "--locked"]),
            StepOutcome::ProductFailure,
            Some(101),
        );
        recorder.fill_not_run();

        assert_eq!(recorder.steps.len(), expected_steps().len());
        assert_eq!(recorder.steps[1].outcome, StepOutcome::ProductFailure);
        assert!(
            recorder.steps.iter().skip(2).all(|step| step.outcome == StepOutcome::NotRun),
            "every unreached step must be recorded as not_run"
        );

        let receipt = RustSmallProofReceipt {
            schema_version: RECEIPT_SCHEMA_VERSION.to_string(),
            subject: sample_subject(),
            steps: recorder.steps,
            scorecard_census: None,
            result: ProofResult::ProductFailure,
        };
        assert!(verify_receipt(&receipt, Some(&sample_subject())).is_ok());
    }

    /// A receipt for a lane that failed at `fail_at` with `outcome`.
    fn failure_receipt(
        fail_at: usize,
        outcome: StepOutcome,
        result: ProofResult,
    ) -> RustSmallProofReceipt {
        let mut receipt = success_receipt();
        receipt.result = result;
        receipt.scorecard_census = None;
        receipt.steps[fail_at].outcome = outcome;
        receipt.steps[fail_at].exit_code = match outcome {
            StepOutcome::ProductFailure => Some(101),
            _ => None,
        };
        for step in receipt.steps.iter_mut().skip(fail_at + 1) {
            step.outcome = StepOutcome::NotRun;
            step.exit_code = None;
        }
        receipt
    }

    #[test]
    fn a_forged_failure_result_over_all_ok_steps_is_refused() {
        // Review #13882: internal consistency was previously checked only for
        // success, so a failure result over nine ok steps verified.
        let mut receipt = success_receipt();
        receipt.result = ProofResult::ProductFailure;
        let text = rejection(&receipt, None);
        assert!(text.contains("forged failure result"), "{text}");
    }

    #[test]
    fn a_terminal_result_that_contradicts_its_failing_step_is_refused() {
        let mut receipt =
            failure_receipt(2, StepOutcome::ProductFailure, ProofResult::ProductFailure);
        receipt.result = ProofResult::InstrumentFailure;
        let text = rejection(&receipt, None);
        assert!(text.contains("implies ProductFailure"), "{text}");

        let mut swapped =
            failure_receipt(2, StepOutcome::InstrumentFailure, ProofResult::InstrumentFailure);
        swapped.result = ProofResult::ProductFailure;
        assert!(rejection(&swapped, None).contains("implies InstrumentFailure"));
    }

    #[test]
    fn a_misplaced_not_run_step_is_refused() {
        // `not_run` must be a suffix: an ok step after an unreached one
        // describes a lane that resumed after stopping.
        let mut receipt =
            failure_receipt(4, StepOutcome::ProductFailure, ProofResult::ProductFailure);
        receipt.steps[6].outcome = StepOutcome::Ok;
        receipt.steps[6].exit_code = Some(0);
        let text = rejection(&receipt, None);
        assert!(text.contains("must form a suffix"), "{text}");
    }

    #[test]
    fn a_failure_result_whose_first_non_ok_step_is_not_run_is_refused() {
        let mut receipt = success_receipt();
        receipt.result = ProofResult::ProductFailure;
        receipt.scorecard_census = None;
        for step in receipt.steps.iter_mut().skip(3) {
            step.outcome = StepOutcome::NotRun;
            step.exit_code = None;
        }
        let text = rejection(&receipt, None);
        assert!(text.contains("cannot stop before something failed"), "{text}");
    }

    #[test]
    fn incoherent_outcome_and_exit_code_combinations_are_refused() {
        let mut not_run_with_code =
            failure_receipt(1, StepOutcome::ProductFailure, ProofResult::ProductFailure);
        not_run_with_code.steps[5].exit_code = Some(0);
        let text = rejection(&not_run_with_code, None);
        assert!(text.contains("that outcome has no exit code"), "{text}");

        let mut instrument_with_code =
            failure_receipt(1, StepOutcome::InstrumentFailure, ProofResult::InstrumentFailure);
        instrument_with_code.steps[1].exit_code = Some(127);
        let text = rejection(&instrument_with_code, None);
        assert!(text.contains("that outcome has no exit code"), "{text}");

        let mut ok_with_nonzero = success_receipt();
        ok_with_nonzero.steps[0].exit_code = Some(101);
        assert!(rejection(&ok_with_nonzero, None).contains("ok with nonzero exit code 101"));
    }

    #[test]
    fn a_census_count_from_a_step_that_never_ran_is_refused() {
        let mut receipt =
            failure_receipt(1, StepOutcome::ProductFailure, ProofResult::ProductFailure);
        receipt.scorecard_census = Some(42);
        let text = rejection(&receipt, None);
        assert!(text.contains("census step was never run"), "{text}");
    }

    #[test]
    fn the_zero_census_product_failure_shape_this_lane_actually_emits_still_verifies() {
        // Negative control for the exit-code rule: the census exits 0 while
        // failing the gate, so `product_failure` with exit code 0 is the one
        // truthful combination the runtime produces. Rejecting it would make
        // the verifier refuse a receipt this command emits.
        let mut receipt = failure_receipt(
            CARGO_STEPS.len(),
            StepOutcome::ProductFailure,
            ProofResult::ProductFailure,
        );
        receipt.steps[CARGO_STEPS.len()].exit_code = Some(0);
        receipt.scorecard_census = Some(0);
        assert!(
            verify_receipt(&receipt, Some(&sample_subject())).is_ok(),
            "the emitted zero-census failure shape must verify"
        );
    }

    #[test]
    fn a_prior_receipt_is_destroyed_before_a_run_can_fail() {
        // Review #13882: subject capture can fail before anything is written,
        // and the failure path tolerates a write error, so a previously
        // written *green* receipt for the same subject would otherwise survive
        // and verify.
        let dir = std::env::temp_dir().join(format!("rsp-invalidate-{}", std::process::id()));
        let Ok(()) = fs::create_dir_all(&dir) else {
            panic!("temp dir must be creatable");
        };
        let path = dir.join("rust-small-proof.json");

        let Ok(json) = serde_json::to_string_pretty(&success_receipt()) else {
            panic!("receipt must serialize");
        };
        let Ok(()) = fs::write(&path, json) else { panic!("seed receipt must be writable") };
        assert!(path.exists(), "the stale receipt must exist before the run");

        let Ok(()) = invalidate_prior_receipt(&path) else {
            panic!("invalidation must succeed");
        };
        assert!(!path.exists(), "no prior receipt may survive into a run that can fail");

        // Idempotent: a first run with no prior receipt is not an error.
        assert!(invalidate_prior_receipt(&path).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn receipt_round_trips_and_uses_a_stable_wire_vocabulary() {
        let receipt = success_receipt();
        let Ok(json) = serde_json::to_string_pretty(&receipt) else {
            panic!("receipt must serialize");
        };
        assert!(json.contains(r#""schema_version": "rust_small_proof.v1""#), "{json}");
        assert!(json.contains(r#""result": "success""#), "{json}");
        assert!(json.contains(r#""outcome": "ok""#), "{json}");

        let Ok(parsed) = serde_json::from_str::<RustSmallProofReceipt>(&json) else {
            panic!("receipt must deserialize");
        };
        assert_eq!(parsed, receipt);
    }

    #[test]
    fn step_execution_classifies_outcome_result_and_exit_code_together() {
        // The receipt outcome and the terminal lane result are derived from
        // one classification, so they cannot disagree about what happened.
        let product = StepExecution::ProductFailure(101);
        assert_eq!(product.outcome(), StepOutcome::ProductFailure);
        assert_eq!(product.result(), ProofResult::ProductFailure);
        assert_eq!(product.exit_code(), Some(101));

        let not_completed = StepExecution::NotCompleted;
        assert_eq!(not_completed.outcome(), StepOutcome::NotCompleted);
        assert_eq!(not_completed.result(), ProofResult::NotCompleted);
        assert_eq!(not_completed.exit_code(), None);

        let instrument = StepExecution::InstrumentFailure("no such binary".to_string());
        assert_eq!(instrument.outcome(), StepOutcome::InstrumentFailure);
        assert_eq!(instrument.result(), ProofResult::InstrumentFailure);
        assert_eq!(instrument.exit_code(), None);
    }
}
