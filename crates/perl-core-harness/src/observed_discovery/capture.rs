//! Exact supervised `t/TEST` observed-discovery capture route (#12283).
//!
//! This module runs one pinned target contract's discovery through the actual
//! upstream selector route and assembles a strict [`UpstreamDiscoveryReceiptV1`]
//! (#12281) from the captured process evidence. It performs no local member
//! discovery: the selector argv is derived from target/selector authority
//! (#6660/#6766), the process runs under the bounded capture supervisor, and
//! every row in the receipt is decoded from the retained raw stdout bytes.
//!
//! The historical profile-driven `discover()` command and the
//! `capture-discovery` raw envelope remain unchanged; production membership
//! cutover belongs to #12105.
//!
//! The pinned real-tree observation remains explicitly unproven until a real
//! prepared tree is captured: where exact upstream preparation is unavailable,
//! the process fixture stays hermetic and no receipt row is fabricated.

use crate::artifacts::{
    CaptureLimits, DiscoveryProcessOutcome, Options, parse_deadline_with_default,
    reject_output_aliases, reject_subject_destinations, run_bounded_command_with_limit,
    sanitize_perl_env, write_json,
};
use crate::build::{effective_selection, effective_selection_authority, find_target, sha256_bytes};
use crate::io::read_matrix;
use crate::model::{TargetAuthorityKind, TargetMatrixEntry, TargetSelector, UpstreamTargetMatrix};
use crate::observed_discovery::build::{build_observed_discovery_receipt, sha256_json};
use crate::observed_discovery::model::{
    DiscoverySubjectIdentity, MAX_RAW_STREAM_BYTES, ObservedDiscoveryInput, ProcessCompletion,
    RunnerArtifactIdentity, UpstreamDiscoveryReceiptV1,
};
use crate::runner_model::{DiscoveryFrame, RunnerKind};
use color_eyre::eyre::{Context, Result, bail};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default capture deadline for one observed discovery run.
pub const OBSERVE_DISCOVERY_DEFAULT_DEADLINE_SECONDS: u64 = 30 * 60;
/// Maximum capture deadline for one observed discovery run.
pub const OBSERVE_DISCOVERY_MAX_DEADLINE_SECONDS: u64 = 24 * 60 * 60;

static CAPTURE_NONCE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Exact configuration for one observed-discovery capture.
#[derive(Debug, Clone)]
pub struct ObserveDiscoveryConfig {
    /// Pinned target matrix (file or bundle directory) supplying target and
    /// selector authority.
    pub matrix: PathBuf,
    /// Exact target id from the matrix.
    pub target_id: String,
    /// Admitted upstream runner; only `test` is current.
    pub runner: RunnerKind,
    /// Prepared Perl tree containing the exact `t/TEST` artifact.
    pub perl_tree: PathBuf,
    /// Host Perl interpreter used to execute the runner artifact.
    pub host_perl: PathBuf,
    /// Measuring repository commit (lower-case hex, 40-64 characters).
    pub repository_commit: String,
    /// Resolved upstream Perl source reference.
    pub perl_ref: String,
    /// Caller-supplied prepared-tree identity reference (#12158 owns the
    /// composed subject; receipts retain it as an opaque reference).
    pub prepared_tree_identity: String,
    /// Caller-supplied host Perl identity reference.
    pub host_perl_identity: String,
    /// Receipt output path.
    pub output: PathBuf,
    /// Finite capture bounds (deadline, cancellation).
    pub limits: CaptureLimits,
}

/// The exact invocation identity bound before spawn.
///
/// Every field except the host paths is retained verbatim in the receipt; the
/// paths are presentation-only and never enter receipt identity.
#[derive(Debug, Clone)]
pub(crate) struct ObservationProcessPlan {
    /// Admitted upstream runner kind.
    pub(crate) runner: RunnerKind,
    /// Exact runner artifact identity measured from the prepared tree.
    pub(crate) runner_artifact: RunnerArtifactIdentity,
    /// Relative argv of the upstream script itself (`TEST --dumptests <selectors>`).
    pub(crate) argv: Vec<String>,
    /// Prepared-tree-relative working directory (`t`).
    pub(crate) working_directory: String,
    /// Behavior-bearing environment variables set for the process.
    pub(crate) environment: BTreeMap<String, String>,
    /// Absolute working directory (presentation only).
    pub(crate) t_dir: PathBuf,
}

/// Build the exact selector argv for one target through its selection
/// authority.
///
/// The argv keeps the target's selector spelling verbatim (directory roots,
/// exact files, glob patterns) plus the contract's runner switches; it never
/// expands the target into explicit `.t` members, so the upstream runner
/// itself performs the selection.
pub(crate) fn selector_arguments(
    matrix: &UpstreamTargetMatrix,
    entry: &TargetMatrixEntry,
) -> Result<Vec<String>, String> {
    let authority = effective_selection_authority(matrix, entry)?;
    if authority.kind != TargetAuthorityKind::Test {
        return Err(format!(
            "target {} selects through {:?} authority {}; the exact t/TEST observation route \
             requires test selection authority",
            entry.contract.target_id, authority.kind, authority.entrypoint
        ));
    }
    let (selectors, _script_forms) = effective_selection(matrix, entry)?;
    let mut arguments = Vec::new();
    for switch in &entry.contract.runner_switches {
        arguments.push(switch.clone());
    }
    for selector in &selectors {
        match selector {
            TargetSelector::RecursiveRoot { path } | TargetSelector::ExactFile { path } => {
                arguments.push(path.clone());
            }
            TargetSelector::NonRecursiveGlob { pattern } => arguments.push(pattern.clone()),
            TargetSelector::ExternalGlob { pattern } => {
                return Err(format!(
                    "external glob selector {pattern} is outside the exact t/TEST observation \
                     route vocabulary"
                ));
            }
            TargetSelector::ManifestPopulation { .. } => {
                return Err(format!(
                    "manifest population selectors on target {} are outside the exact t/TEST \
                     observation route vocabulary",
                    entry.contract.target_id
                ));
            }
        }
    }
    Ok(arguments)
}

/// Bind the exact invocation subject before spawn.
///
/// The artifact digest is measured from the exact `t/TEST` bytes in the
/// prepared tree, the argv comes from target authority alone, and the frame is
/// fixed by the runner route: upstream `t/TEST --dumptests` rewrites every
/// selected row to a repository-root-relative path (`sub dump_tests` at the
/// pinned ref rewrites `../x` to `x` and prefixes bare rows with `t/`), so the
/// honest frame is [`DiscoveryFrame::CanonicalRepositoryPath`].
/// [`DiscoveryFrame::RunnerTDirectoryRelative`] stays reserved for runners
/// that actually emit `t/`-relative rows.
///
/// The behavior-bearing environment is the capture baseline (`LC_ALL=C`)
/// overlaid with the target contract's declared environment, so an
/// environment-bearing target (for example `optional_bigmem`'s
/// `PERL_TEST_MEMORY`) executes and records exactly its declared process
/// contract; ambient caller variables beyond the sanitized set stay outside
/// receipt identity by the #12281 caller-supplied-reference limitation.
pub(crate) fn bind_process_plan(
    matrix: &UpstreamTargetMatrix,
    entry: &TargetMatrixEntry,
    runner: RunnerKind,
    perl_tree: &Path,
) -> Result<ObservationProcessPlan> {
    let selectors =
        selector_arguments(matrix, entry).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let t_dir = perl_tree.join("t");
    let script_path = t_dir.join("TEST");
    if !script_path.is_file() {
        bail!(
            "prepared Perl tree is missing the exact t/TEST runner artifact: {}",
            script_path.display()
        );
    }
    let artifact_bytes = fs::read(&script_path)
        .with_context(|| format!("reading runner artifact {}", script_path.display()))?;
    let mut argv = vec!["TEST".to_string(), "--dumptests".to_string()];
    argv.extend(selectors);
    let mut environment = BTreeMap::from([("LC_ALL".to_string(), "C".to_string())]);
    for (key, value) in &entry.contract.environment {
        environment.insert(key.clone(), value.clone());
    }
    Ok(ObservationProcessPlan {
        runner,
        runner_artifact: RunnerArtifactIdentity {
            canonical_path: "t/TEST".to_string(),
            content_sha256: sha256_bytes(&artifact_bytes),
        },
        argv,
        working_directory: "t".to_string(),
        environment,
        t_dir,
    })
}

/// Execute one bound plan under the bounded capture supervisor and return the
/// typed terminal observation plus both raw stream envelopes.
pub(crate) fn execute_plan(
    plan: &ObservationProcessPlan,
    host_perl: &Path,
    limits: &CaptureLimits,
) -> (ProcessCompletion, Vec<u8>, bool, Vec<u8>, bool) {
    let mut command = Command::new(host_perl);
    command.current_dir(&plan.t_dir);
    command.args(&plan.argv);
    for (key, value) in &plan.environment {
        command.env(key, value);
    }
    sanitize_perl_env(&mut command);
    let (outcome, stdout, stderr) =
        run_bounded_command_with_limit(command, limits, MAX_RAW_STREAM_BYTES);
    let stdout_bytes = stdout.retained_bytes().unwrap_or_default();
    let stderr_bytes = stderr.retained_bytes().unwrap_or_default();
    let stdout_truncated = stdout.was_truncated();
    let stderr_truncated = stderr.was_truncated();
    let instrument_failed =
        stdout.capture_failure().is_some() || stderr.capture_failure().is_some();
    let completion = if instrument_failed {
        ProcessCompletion::InstrumentFailed
    } else {
        completion_from_outcome(&outcome)
    };
    (completion, stdout_bytes, stdout_truncated, stderr_bytes, stderr_truncated)
}

/// Project the supervisor's terminal taxonomy onto the receipt vocabulary
/// without collapsing distinct terminations.
fn completion_from_outcome(outcome: &DiscoveryProcessOutcome) -> ProcessCompletion {
    match outcome {
        DiscoveryProcessOutcome::Exited { code } => ProcessCompletion::ExitStatus { code: *code },
        DiscoveryProcessOutcome::Signaled { signal, .. } => u32::try_from(*signal)
            .map_or(ProcessCompletion::Unknown, |signal| ProcessCompletion::Signalled { signal }),
        DiscoveryProcessOutcome::TerminatedWithoutIdentity { .. } => ProcessCompletion::Unknown,
        DiscoveryProcessOutcome::TimedOut { deadline_ms, .. } => {
            ProcessCompletion::TimedOut { deadline_millis: *deadline_ms }
        }
        DiscoveryProcessOutcome::Cancelled { .. } => ProcessCompletion::Cancelled,
        DiscoveryProcessOutcome::SpawnFailed { .. }
        | DiscoveryProcessOutcome::WaitFailed { .. }
        | DiscoveryProcessOutcome::CaptureSetupFailed { .. } => ProcessCompletion::InstrumentFailed,
    }
}

/// Resolve the host interpreter against the caller's directory before the
/// child changes its working directory.
///
/// A relative interpreter path containing a separator would otherwise be
/// resolved relative to the runner's `t` directory by the spawned process. A
/// bare name stays bare so `PATH` lookup keeps working.
pub(crate) fn resolve_host_interpreter(host_perl: &Path) -> Result<PathBuf> {
    if host_perl.is_absolute() || host_perl.components().count() == 1 {
        return Ok(host_perl.to_path_buf());
    }
    let current = std::env::current_dir()
        .context("reading the current directory to resolve the relative host Perl path")?;
    Ok(current.join(host_perl))
}

/// Reject a receipt destination that would overwrite the pinned target
/// matrix itself: the matrix file, or any member of a bundle directory.
///
/// The matrix is the authority the receipt is validated against, so
/// overwriting it after capture would destroy the input while the command
/// still reports success.
fn reject_matrix_output_alias(matrix_path: &Path, output: &Path) -> Result<()> {
    let matrix_canonical = fs::canonicalize(matrix_path)
        .with_context(|| format!("canonicalizing target matrix {}", matrix_path.display()))?;
    let output_resolved = crate::artifacts::resolve_destination(output)?;
    if output_resolved == matrix_canonical || output_resolved.starts_with(&matrix_canonical) {
        bail!(
            "receipt output {} would overwrite the pinned target matrix {}",
            output.display(),
            matrix_path.display()
        );
    }
    Ok(())
}

/// Mint one capture identity unique to this observation.
///
/// The nonce combines wall-clock time, the observing process id, and an
/// in-process counter so two CLI processes minting in the same millisecond
/// still cannot share a capture identity.
fn mint_process_nonce() -> Result<String> {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("reading the system clock for the capture identity")?;
    let counter = CAPTURE_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(format!("observe-discovery-{}-{}-{counter}", since_epoch.as_millis(), std::process::id()))
}

/// Run one exact observed discovery and assemble the strict #12281 receipt.
///
/// The receipt is returned for every constructible terminal state; only
/// `observed_complete` proves a complete observation. Construction failures
/// (for example an empty decoded stream, which the #12281 contract refuses
/// outright) surface as typed errors rather than fabricated receipts.
pub fn observe_discovery(config: &ObserveDiscoveryConfig) -> Result<UpstreamDiscoveryReceiptV1> {
    if config.runner != RunnerKind::Test {
        bail!(
            "runner {:?} is not an admitted observation route; the exact t/harness observation \
             lane is separate",
            config.runner
        );
    }
    validate_commit_shape(&config.repository_commit)?;
    let matrix = read_matrix(&config.matrix)?;
    let entry =
        find_target(&matrix, &config.target_id).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let perl_tree = fs::canonicalize(&config.perl_tree).with_context(|| {
        format!("canonicalizing prepared Perl tree {}", config.perl_tree.display())
    })?;
    if !perl_tree.is_dir() {
        bail!("prepared Perl tree is not a directory: {}", perl_tree.display());
    }
    let host_perl = resolve_host_interpreter(&config.host_perl)?;
    reject_matrix_output_alias(&config.matrix, &config.output)?;
    reject_output_aliases(
        &[perl_tree.join("t").join("TEST"), host_perl.clone()],
        std::slice::from_ref(&config.output),
    )?;
    reject_subject_destinations(&host_perl, &perl_tree, std::slice::from_ref(&config.output))?;

    let plan = bind_process_plan(&matrix, entry, config.runner, &perl_tree)?;
    let matrix_fingerprint =
        matrix.fingerprint().map_err(|error| color_eyre::eyre::eyre!(error))?;
    let target_contract_digest =
        sha256_json(&entry.contract).map_err(|error| color_eyre::eyre::eyre!(error))?;
    let subject = DiscoverySubjectIdentity {
        repository_commit: config.repository_commit.clone(),
        perl_ref: config.perl_ref.clone(),
        prepared_tree_identity: config.prepared_tree_identity.clone(),
        host_perl_identity: config.host_perl_identity.clone(),
        matrix_fingerprint,
        target_id: config.target_id.clone(),
        target_contract_digest,
        variant_target_id: None,
        instrumentation_id: None,
    };
    let process_nonce = mint_process_nonce()?;
    let (completion, stdout_bytes, stdout_truncated, stderr_bytes, stderr_truncated) =
        execute_plan(&plan, &host_perl, &config.limits);
    let input = ObservedDiscoveryInput {
        subject,
        runner: plan.runner,
        runner_artifact: plan.runner_artifact,
        argv: plan.argv,
        working_directory: plan.working_directory,
        environment: plan.environment,
        discovery_frame: DiscoveryFrame::CanonicalRepositoryPath,
        completion,
        process_nonce,
        stdout_bytes,
        stdout_truncated,
        stderr_bytes,
        stderr_truncated,
    };
    build_observed_discovery_receipt(&matrix, &input).map_err(|error| {
        color_eyre::eyre::eyre!(
            "observed discovery for target {} could not construct a strict receipt: {error}",
            config.target_id
        )
    })
}

/// Run one observed discovery, write the receipt, and validate the written
/// evidence by reconstruction before reporting the terminal disposition.
///
/// Every non-complete state is a typed failure exit, never a clean pass: the
/// receipt is retained on disk first so the evidence survives the nonzero
/// exit.
pub fn observe_discovery_command(config: &ObserveDiscoveryConfig) -> Result<()> {
    let receipt = observe_discovery(config)?;
    write_json(&config.output, &receipt)?;
    let matrix = read_matrix(&config.matrix)?;
    let written_bytes = fs::read(&config.output)
        .with_context(|| format!("reading back receipt {}", config.output.display()))?;
    let written: UpstreamDiscoveryReceiptV1 = serde_json::from_slice(&written_bytes)
        .with_context(|| format!("decoding written receipt {}", config.output.display()))?;
    crate::observed_discovery::build::check_observed_discovery_against(&matrix, &written).map_err(
        |error| {
            color_eyre::eyre::eyre!(
                "written receipt {} does not reconstruct against the pinned matrix: {error}",
                config.output.display()
            )
        },
    )?;
    let work = &receipt.payload.work;
    tracing::info!(
        target = %config.target_id,
        state = ?receipt.payload.state,
        selector_processes = 1u64,
        raw_stdout_bytes = work.raw_stdout_bytes,
        raw_stderr_bytes = work.raw_stderr_bytes,
        decoded_rows = work.decoded_rows,
        accepted_rows = work.accepted_rows,
        duplicate_rows = work.duplicate_rows,
        conflicting_rows = work.conflicting_rows,
        out_of_target_rows = work.out_of_target_rows,
        unsupported_source_form_rows = work.unsupported_source_form_rows,
        malformed_rows = work.malformed_rows,
        local_membership_files_enumerated = 0u64,
        local_membership_bytes_scanned = 0u64,
        direct_probe_rows_consumed = work.direct_probe_rows_consumed,
        "observed discovery capture"
    );
    if !receipt.payload.state.is_complete() {
        bail!(
            "observed discovery state is {:?}, not observed_complete; the typed receipt is \
             retained at {}",
            receipt.payload.state,
            config.output.display()
        );
    }
    Ok(())
}

/// Parse `perl-core-harness-artifacts observe-discovery` options.
pub(crate) fn observe_discovery_from_options(mut options: Options) -> Result<()> {
    let config = ObserveDiscoveryConfig {
        matrix: PathBuf::from(options.required("--matrix")?),
        target_id: options.required("--target")?,
        runner: parse_runner(&options.required("--runner")?)?,
        perl_tree: PathBuf::from(options.required("--perl-tree")?),
        host_perl: PathBuf::from(options.required("--host-perl")?),
        repository_commit: options.required("--commit")?,
        perl_ref: options.required("--perl-ref")?,
        prepared_tree_identity: options.required("--prepared-tree-identity")?,
        host_perl_identity: options.required("--host-perl-identity")?,
        output: PathBuf::from(options.required("--output")?),
        limits: CaptureLimits {
            deadline: parse_deadline_with_default(
                options.optional("--deadline-seconds")?.as_deref(),
                OBSERVE_DISCOVERY_DEFAULT_DEADLINE_SECONDS,
                OBSERVE_DISCOVERY_MAX_DEADLINE_SECONDS,
            )?,
            cancel_file: options.optional("--cancel-file")?.map(PathBuf::from),
        },
    };
    options.finish()?;
    observe_discovery_command(&config)
}

fn parse_runner(value: &str) -> Result<RunnerKind> {
    match RunnerKind::parse(value) {
        Ok(RunnerKind::Test) => Ok(RunnerKind::Test),
        Ok(other) => bail!(
            "runner {other:?} is not an admitted observation route; only --runner test is current"
        ),
        Err(error) => bail!("{error}"),
    }
}

/// Fail fast on a malformed repository commit before spending a supervised
/// run; the receipt constructor re-validates the same law afterwards.
fn validate_commit_shape(commit: &str) -> Result<()> {
    let lowercase_hex =
        commit.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if commit.len() < 40 || commit.len() > 64 || !lowercase_hex {
        bail!(
            "--commit must be a 40-64 character lower-case hexadecimal repository commit, \
             found {commit}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    //! Focused unit proof for the pure derivation seams of the capture route:
    //! selector-argv construction stays a function of target authority alone,
    //! and the supervisor-outcome projection keeps distinct terminations
    //! distinct. Process-level behavior is proven by the hermetic exact-process
    //! integration suite (`tests/observed_discovery_capture.rs`).

    use super::{
        DiscoveryProcessOutcome, ObserveDiscoveryConfig, bind_process_plan,
        completion_from_outcome, parse_runner, selector_arguments, validate_commit_shape,
    };
    use crate::artifacts::CaptureLimits;
    use crate::build::find_target;
    use crate::io::read_matrix;
    use crate::model::TargetAuthorityKind;
    use crate::observed_discovery::model::ProcessCompletion;
    use crate::runner_model::RunnerKind;
    use color_eyre::eyre::{Result, bail};
    use std::path::{Path, PathBuf};

    fn matrix() -> Result<crate::model::UpstreamTargetMatrix> {
        read_matrix(&repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"))
    }

    fn repo_file(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
    }

    fn entry<'m>(
        matrix: &'m crate::model::UpstreamTargetMatrix,
        target_id: &str,
    ) -> Result<&'m crate::model::TargetMatrixEntry> {
        find_target(matrix, target_id).map_err(|error| color_eyre::eyre::eyre!(error))
    }

    #[test]
    fn selector_argv_keeps_contract_spelling_without_expansion() -> Result<()> {
        let matrix = matrix()?;
        for (target_id, expected_argv) in [
            ("component_base", vec!["base".to_string()]),
            ("component_comp", vec!["comp".to_string()]),
            ("component_run", vec!["run".to_string()]),
        ] {
            let arguments = selector_arguments(&matrix, entry(&matrix, target_id)?)
                .map_err(|error| color_eyre::eyre::eyre!(error))?;
            assert_eq!(arguments, expected_argv, "{target_id} selector argv");
            // The argv is a pure function of target authority: it contains
            // selector spellings, never expanded `.t` member paths.
            assert!(
                arguments.iter().all(|argument| !argument.ends_with(".t")),
                "{target_id} selector argv must not expand into explicit members"
            );
        }
        Ok(())
    }

    #[test]
    fn non_test_authorities_and_foreign_selectors_refuse_the_route() -> Result<()> {
        let matrix = matrix()?;
        for target_id in [
            "component_op",
            "manifest_root_lib",
            "make_test_reonly",
            "prep_test",
            // Carries a manifest-population selector even though its authority
            // is t/TEST: `--core` populations stay outside this route.
            "selector_test_core",
        ] {
            let found = entry(&matrix, target_id)?;
            let Err(error) = selector_arguments(&matrix, found) else {
                bail!("{target_id} must refuse the exact t/TEST observation route");
            };
            assert!(
                error.contains("t/TEST observation route")
                    || error.contains("requires test selection authority")
                    || error.contains("physical runner population")
                    || error.contains("no selection authority"),
                "unexpected {target_id} refusal: {error}"
            );
        }
        // A composite/legacy target has no physical population at all.
        let Err(_) = selector_arguments(&matrix, entry(&matrix, "legacy_custom_full_test")?) else {
            bail!("generated composites must refuse the observation route");
        };
        Ok(())
    }

    #[test]
    fn process_plan_binds_artifact_bytes_argv_and_frame_before_spawn() -> Result<()> {
        let matrix = matrix()?;
        let temp = tempfile::tempdir()?;
        let tree = temp.path().join("prepared");
        let t_dir = tree.join("t");
        std::fs::create_dir_all(t_dir.join("base"))?;
        std::fs::write(t_dir.join("base").join("if.t"), "1;\n")?;
        std::fs::write(t_dir.join("TEST"), "#!./perl\n# hermetic upstream stand-in\n")?;

        let plan =
            bind_process_plan(&matrix, entry(&matrix, "component_base")?, RunnerKind::Test, &tree)?;
        assert_eq!(
            plan.argv,
            vec!["TEST".to_string(), "--dumptests".to_string(), "base".to_string()]
        );
        assert_eq!(plan.working_directory, "t");
        assert_eq!(plan.runner_artifact.canonical_path, "t/TEST");
        assert_eq!(
            plan.runner_artifact.content_sha256,
            crate::build::sha256_bytes(b"#!./perl\n# hermetic upstream stand-in\n")
        );
        assert_eq!(
            plan.environment,
            std::collections::BTreeMap::from([("LC_ALL".to_string(), "C".to_string())])
        );
        // A changed artifact changes the bound identity.
        std::fs::write(t_dir.join("TEST"), "#!./perl\n# drifted artifact\n")?;
        let drifted =
            bind_process_plan(&matrix, entry(&matrix, "component_base")?, RunnerKind::Test, &tree)?;
        assert_ne!(plan.runner_artifact.content_sha256, drifted.runner_artifact.content_sha256);

        // A missing artifact is a typed refusal, never a silent walk.
        std::fs::remove_file(t_dir.join("TEST"))?;
        let Err(error) =
            bind_process_plan(&matrix, entry(&matrix, "component_base")?, RunnerKind::Test, &tree)
        else {
            bail!("missing t/TEST must refuse plan binding");
        };
        assert!(
            error.to_string().contains("missing the exact t/TEST runner artifact"),
            "unexpected missing-artifact error: {error}"
        );
        Ok(())
    }

    /// Review repair (P2): the target contract's declared environment is part
    /// of the exact invocation subject, so an environment-bearing target binds
    /// its declared variables into the plan and the receipt.
    #[test]
    fn target_contract_environment_is_applied_and_recorded() -> Result<()> {
        let matrix = matrix()?;
        let temp = tempfile::tempdir()?;
        let tree = temp.path().join("prepared");
        let t_dir = tree.join("t");
        std::fs::create_dir_all(t_dir.join("base"))?;
        std::fs::write(t_dir.join("TEST"), "#!./perl\n")?;

        // A pinned test-authority target with a declared environment.
        let plan = bind_process_plan(
            &matrix,
            entry(&matrix, "optional_bigmem")?,
            RunnerKind::Test,
            &tree,
        )?;
        assert_eq!(
            plan.environment,
            std::collections::BTreeMap::from([
                ("LC_ALL".to_string(), "C".to_string()),
                ("PERL_TEST_MEMORY".to_string(), "enabled".to_string()),
            ]),
            "the contract environment must overlay the capture baseline"
        );
        assert_eq!(plan.argv.len(), 3, "optional_bigmem keeps its recursive-root selector");

        // A contract variable wins over a colliding baseline entry: the target
        // authority owns the behavior-bearing process contract.
        let mut colliding = entry(&matrix, "component_base")?.clone();
        colliding.contract.environment.insert("LC_ALL".to_string(), "en_US.UTF-8".to_string());
        let plan = bind_process_plan(&matrix, &colliding, RunnerKind::Test, &tree)?;
        assert_eq!(plan.environment.get("LC_ALL").map(String::as_str), Some("en_US.UTF-8"));
        Ok(())
    }

    /// Review repair (P2): a receipt destination may never overwrite the
    /// pinned matrix authority it will be validated against.
    #[test]
    fn receipt_output_cannot_alias_the_pinned_matrix() -> Result<()> {
        let matrix_dir = repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1");
        let member = matrix_dir.join("01-components-a.json");
        let Err(error) = super::reject_matrix_output_alias(&matrix_dir, &member) else {
            bail!("a bundle-member output must be rejected");
        };
        assert!(
            error.to_string().contains("would overwrite the pinned target matrix"),
            "unexpected matrix-alias error: {error}"
        );
        let single = repo_file(".ci/perl-core-harness/upstream-targets-blead-drift.v1.json");
        let Err(_) = super::reject_matrix_output_alias(&single, &single) else {
            bail!("a single-file matrix output must be rejected");
        };
        let elsewhere = std::env::temp_dir().join("plsw-12283-receipt-check.json");
        assert!(super::reject_matrix_output_alias(&matrix_dir, &elsewhere).is_ok());
        Ok(())
    }

    /// Review repair (P2): a relative path-bearing interpreter resolves
    /// against the caller's directory, not the runner's future cwd.
    #[test]
    fn relative_host_interpreters_resolve_against_the_caller_directory() -> Result<()> {
        let bare = super::resolve_host_interpreter(Path::new("perl"))?;
        assert_eq!(bare, Path::new("perl"), "bare names stay for PATH lookup");

        let absolute =
            PathBuf::from(if cfg!(windows) { "C:\\tools\\perl.exe" } else { "/usr/bin/perl" });
        assert_eq!(super::resolve_host_interpreter(&absolute)?, absolute);

        let relative = Path::new(if cfg!(windows) { "tools\\perl.exe" } else { "tools/perl" });
        let resolved = super::resolve_host_interpreter(relative)?;
        assert!(resolved.is_absolute(), "path-bearing relatives become caller-absolute");
        assert!(
            resolved.ends_with(relative),
            "resolution must preserve the interpreter spelling: {}",
            resolved.display()
        );
        Ok(())
    }

    /// Review repair (P2): capture identities stay unique across observing
    /// processes minting in the same millisecond.
    #[test]
    fn capture_nonces_carry_a_process_unique_component() -> Result<()> {
        let first = super::mint_process_nonce()?;
        let second = super::mint_process_nonce()?;
        assert_ne!(first, second);
        let tail = first
            .strip_prefix("observe-discovery-")
            .ok_or_else(|| color_eyre::eyre::eyre!("nonce lost its prefix: {first}"))?;
        let parts = tail.split('-').count();
        assert_eq!(parts, 3, "nonce combines time, process id, and counter: {first}");
        assert!(
            first.contains(&std::process::id().to_string()),
            "nonce must carry the observing process id: {first}"
        );
        Ok(())
    }

    #[test]
    fn supervisor_outcomes_project_onto_receipt_completions() -> Result<()> {
        let cases = [
            (
                DiscoveryProcessOutcome::Exited { code: 0 },
                ProcessCompletion::ExitStatus { code: 0 },
            ),
            (
                DiscoveryProcessOutcome::Exited { code: 7 },
                ProcessCompletion::ExitStatus { code: 7 },
            ),
            (
                DiscoveryProcessOutcome::Signaled {
                    signal: 9,
                    signal_name: "SIGKILL".to_string(),
                    core_dumped: false,
                },
                ProcessCompletion::Signalled { signal: 9 },
            ),
            (
                DiscoveryProcessOutcome::TimedOut {
                    deadline_ms: 1_000,
                    phase: crate::artifacts::CaptureDeadlinePhase::Process,
                },
                ProcessCompletion::TimedOut { deadline_millis: 1_000 },
            ),
            (
                DiscoveryProcessOutcome::Cancelled { source: "operator".to_string() },
                ProcessCompletion::Cancelled,
            ),
            (
                DiscoveryProcessOutcome::TerminatedWithoutIdentity {
                    platform: "windows".to_string(),
                },
                ProcessCompletion::Unknown,
            ),
            (
                DiscoveryProcessOutcome::SpawnFailed { error: "no such file".to_string() },
                ProcessCompletion::InstrumentFailed,
            ),
        ];
        for (outcome, expected) in cases {
            assert_eq!(completion_from_outcome(&outcome), expected, "{outcome:?}");
        }
        Ok(())
    }

    #[test]
    fn route_admission_and_commit_shape_fail_closed() -> Result<()> {
        assert!(parse_runner("test").is_ok());
        for refused in ["harness", "direct_fallback"] {
            let Err(error) = parse_runner(refused) else {
                bail!("runner {refused} must refuse the observation route");
            };
            assert!(
                error.to_string().contains("not an admitted observation route"),
                "unexpected runner refusal: {error}"
            );
        }
        // A runner outside the vocabulary fails at parsing, still closed.
        assert!(parse_runner("make").is_err());
        for bad_commit in ["", "zzz", &"a".repeat(39), &"a".repeat(65), &"G".repeat(40)] {
            assert!(
                validate_commit_shape(bad_commit).is_err(),
                "commit {bad_commit} must fail fast"
            );
        }
        assert!(validate_commit_shape(&"a".repeat(40)).is_ok());
        assert!(validate_commit_shape(&"0123456789abcdef".repeat(4)).is_ok());
        Ok(())
    }

    #[test]
    fn configuration_requires_the_exact_subject_references() -> Result<()> {
        let matrix = matrix()?;
        let authority =
            crate::build::effective_selection_authority(&matrix, entry(&matrix, "component_base")?)
                .map_err(|error| color_eyre::eyre::eyre!(error))?;
        assert_eq!(authority.kind, TargetAuthorityKind::Test);
        assert_eq!(authority.entrypoint, "t/TEST");

        // The config surface stays caller-supplied-reference shaped: identities
        // are opaque strings bound into the receipt, not paths.
        let config = ObserveDiscoveryConfig {
            matrix: repo_file(".ci/perl-core-harness/upstream-targets-5.42.2.v1"),
            target_id: "component_base".to_string(),
            runner: RunnerKind::Test,
            perl_tree: PathBuf::from("does-not-exist"),
            host_perl: PathBuf::from("perl"),
            repository_commit: "a".repeat(40),
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-generation-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            output: PathBuf::from("receipt.json"),
            limits: CaptureLimits {
                deadline: std::time::Duration::from_secs(30),
                cancel_file: None,
            },
        };
        let Err(error) = super::observe_discovery(&config) else {
            bail!("a missing prepared tree must refuse the observation");
        };
        assert!(
            error.to_string().contains("canonicalizing prepared Perl tree"),
            "unexpected missing-tree error: {error}"
        );
        Ok(())
    }
}
