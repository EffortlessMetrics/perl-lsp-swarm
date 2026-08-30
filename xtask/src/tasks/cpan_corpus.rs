//! CPAN corpus acquisition, sweep, and ratchet management
//!
//! Provides sub-commands for:
//! - `fetch-list` — query MetaCPAN for top 1000 distributions by reverse dependency count
//! - `install` — install distributions into a local lib directory via `cpanm`
//! - `sweep` — run parser corpus sweep against installed CPAN modules
//! - `ratchet` — auto-append newly-clean modules to the CPAN manifest

use color_eyre::eyre::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::parser_corpus_sweep;

/// Default path for the pinned distribution list
const DIST_LIST_PATH: &str = ".ci/cpan-top-1000-distributions.txt";
/// Default path for the CPAN known-clean manifest
const CPAN_MANIFEST_PATH: &str = ".ci/cpan-corpus-manifest.txt";
/// Default path for the full CPAN corpus baseline report
const CPAN_BASELINE_PATH: &str = ".ci/cpan-corpus-baseline.json";
/// Default install target directory (relative to workspace root)
const CPAN_INSTALL_DIR: &str = "target/cpan-corpus";
/// Temp report path used by ratchet (relative to workspace root)
const CPAN_RATCHET_REPORT_PATH: &str = "target/cpan-corpus-ratchet-report.json";
/// cpanm cache directory preserved across install resets
const CPANM_CACHE_DIR: &str = ".cpanm";
/// Standalone cpanm bootstrap URL
const CPANM_STANDALONE_URL: &str = "https://cpanmin.us";
/// MetaCPAN API endpoint for distribution search (sorted by river.immediate)
const METACPAN_API: &str = "https://fastapi.metacpan.org/v1/distribution/_search";
/// Hard timeout for a batch cpanm invocation. Batch installs fall back to
/// per-distribution retries when one distribution wedges inside configure/build.
const CPANM_BATCH_TIMEOUT: Duration = Duration::from_mins(5);
/// Number of distributions installed per cpanm batch invocation.
const CPANM_BATCH_SIZE: usize = 25;
/// Hard timeout for a single-distribution retry. Native-heavy distributions
/// such as `PDL` legitimately need more wall-clock time than a whole batch
/// should get before we split it apart.
const CPANM_SINGLE_DIST_TIMEOUT: Duration = Duration::from_mins(5);

/// Wall-clock budget guard for one install pass (#12823). Scheduled runners
/// in this repository have been observed being preempted externally at
/// ~24m29s of job wall-clock; an unlimited cold install therefore never
/// reaches a save gate that requires one complete pass. A budgeted pass ends
/// on its own terms below that envelope and lets checkpoint persistence bank
/// whatever consistent state cpanm has already installed.
#[derive(Debug, Clone, Copy)]
struct InstallBudget {
    deadline: Option<Instant>,
}

impl InstallBudget {
    fn from_option(budget: Option<Duration>) -> Self {
        Self { deadline: budget.map(|budget| Instant::now() + budget) }
    }

    /// True once no further batch or individual retry may start.
    fn expired(&self) -> bool {
        self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// Timeout for one process invocation: never above the fixed per-phase
    /// ceiling, and never beyond the remaining overall budget. With no budget
    /// configured this is exactly the historical fixed ceiling.
    fn phase_timeout(&self, ceiling: Duration) -> Duration {
        match self.deadline {
            Some(deadline) => deadline.saturating_duration_since(Instant::now()).min(ceiling),
            None => ceiling,
        }
    }
}

/// Machine-readable completion marker consumed by the CI warm lane
/// (`corpus-warm-full`) to decide whether the corpus ratchet leg may run.
fn complete_marker(complete: bool) -> String {
    format!("CPAN_CORPUS_INSTALL_COMPLETE={complete}")
}

/// Whether this pass reached every distribution and therefore may report
/// `CPAN_CORPUS_INSTALL_COMPLETE=true`. Conservative on purpose: any explicit
/// budget stop OR a deadline found expired at plan end forces "incomplete",
/// because partial state banked under the canonical key would silently narrow
/// the corpus the ratchet enforces against (#12823).
fn plan_complete(budget_stopped: bool, budget: &InstallBudget) -> bool {
    !budget_stopped && !budget.expired()
}

/// Return the workspace root path, anchored at compile time to the xtask
/// crate's manifest directory. This makes every relative CPAN corpus path
/// resolve deterministically against the workspace root regardless of the
/// current working directory `cargo xtask` was invoked from.
///
/// Using `env!("CARGO_MANIFEST_DIR")` (the xtask crate dir) with `.parent()`
/// is robust because:
/// - it is baked into the xtask binary at build time, so no runtime shell-out,
/// - it does not depend on `CARGO_TARGET_DIR` or `std::env::current_dir()`,
/// - it always points at the workspace that built this xtask binary.
pub(crate) fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().map(PathBuf::from).unwrap_or(manifest_dir)
}

/// Join a path segment onto the workspace root so callers get an absolute
/// path even if the current working directory is not the workspace root.
fn workspace_path(rel: &str) -> PathBuf {
    workspace_root().join(rel)
}

/// Configuration for cpan-corpus sub-commands
#[derive(Debug, Clone)]
pub struct CpanCorpusConfig {
    /// Path to the distribution list file
    pub dist_list: PathBuf,
    /// Path to the CPAN corpus manifest
    pub manifest: PathBuf,
    /// Local install directory for CPAN modules
    pub install_dir: PathBuf,
    /// Number of distributions to fetch (default 1000)
    pub top_n: usize,
    /// Verbose output
    pub verbose: bool,
    /// Force a full wipe of the install directory before installing.
    /// When false (default) and the install directory already contains
    /// `lib/perl5`, the reset is skipped and cpanm runs in incremental
    /// mode — only modules that are missing or out-of-date get installed.
    /// This turns re-runs into a cheap cache hit instead of a full rebuild.
    pub force_reset: bool,
    /// Optional wall-clock budget for the whole install pass. When set, the
    /// batch plan stops cleanly once the budget is exhausted (before starting
    /// further batches or individual retries), emits the machine-readable
    /// `CPAN_CORPUS_INSTALL_COMPLETE=false` marker, and exits successfully so
    /// already-installed state stays consistent for checkpoint persistence.
    /// When absent, install behaves exactly like the historical unlimited pass.
    pub time_budget: Option<Duration>,
}

impl Default for CpanCorpusConfig {
    fn default() -> Self {
        // Anchor default paths at the workspace root rather than the current
        // working directory, so `cargo xtask cpan-corpus ...` always looks
        // for the corpus in the same place `actions/cache` restored it to
        // (see issue #3189).
        Self {
            dist_list: workspace_path(DIST_LIST_PATH),
            manifest: workspace_path(CPAN_MANIFEST_PATH),
            install_dir: workspace_path(CPAN_INSTALL_DIR),
            top_n: 1000,
            verbose: false,
            force_reset: false,
            time_budget: None,
        }
    }
}

// --------------------------------------------------------------------------
// MetaCPAN response types
// --------------------------------------------------------------------------

#[derive(Deserialize)]
struct MetaCpanResponse {
    hits: MetaCpanHits,
}

#[derive(Deserialize)]
struct MetaCpanHits {
    hits: Vec<MetaCpanHit>,
}

#[derive(Deserialize)]
struct MetaCpanHit {
    _source: MetaCpanRelease,
}

#[derive(Deserialize)]
struct MetaCpanRelease {
    name: String,
}

// --------------------------------------------------------------------------
// fetch-list
// --------------------------------------------------------------------------

/// Query MetaCPAN for the top N distributions by reverse dependency count
/// and write them to the distribution list file.
pub fn fetch_list(config: &CpanCorpusConfig) -> Result<()> {
    println!("Fetching top {} distributions from MetaCPAN...", config.top_n);

    let query_body = serde_json::json!({
        "size": config.top_n,
        "query": { "match_all": {} },
        "sort": [{ "river.immediate": { "order": "desc" } }],
        "_source": ["name"]
    });

    let output = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            METACPAN_API,
            "-H",
            "Content-Type: application/json",
            "-d",
            &query_body.to_string(),
        ])
        .output()
        .context("Failed to run curl — is curl installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(color_eyre::eyre::eyre!("curl failed: {}", stderr));
    }

    let response: MetaCpanResponse =
        serde_json::from_slice(&output.stdout).context("Failed to parse MetaCPAN response")?;

    let distributions: Vec<&str> =
        response.hits.hits.iter().map(|h| h._source.name.as_str()).collect();

    println!("Got {} distributions from MetaCPAN", distributions.len());

    // Write to file
    if let Some(parent) = config.dist_list.parent() {
        fs::create_dir_all(parent).context("Failed to create directory for distribution list")?;
    }

    let mut file = fs::File::create(&config.dist_list)
        .with_context(|| format!("Failed to create {}", config.dist_list.display()))?;

    writeln!(file, "# CPAN top {} distributions by reverse dependency count", config.top_n)?;
    writeln!(file, "# Auto-generated by: cargo xtask cpan-corpus fetch-list")?;
    writeln!(file, "# Source: {METACPAN_API}")?;
    writeln!(file, "# Date: {}", chrono::Utc::now().to_rfc3339())?;
    writeln!(file, "#")?;

    for dist in &distributions {
        writeln!(file, "{dist}")?;
    }

    println!("Wrote {} distributions to {}", distributions.len(), config.dist_list.display());
    Ok(())
}

// --------------------------------------------------------------------------
// install
// --------------------------------------------------------------------------

/// Read the distribution list and install each via cpanm into a local lib.
pub fn install(config: &CpanCorpusConfig) -> Result<()> {
    let mut distributions = read_dist_list(&config.dist_list)?;
    if distributions.is_empty() {
        println!(
            "Distribution list is empty: {}. Fetching top {} distributions first...",
            config.dist_list.display(),
            config.top_n
        );
        fetch_list(config)?;
        distributions = read_dist_list(&config.dist_list)?;
    }
    if distributions.is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "Distribution list is still empty after fetch: {}",
            config.dist_list.display(),
        ));
    }

    println!(
        "Installing {} distributions into {}",
        distributions.len(),
        config.install_dir.display()
    );

    let preserved_dist_list =
        config.dist_list.strip_prefix(&config.install_dir).ok().map(|_| config.dist_list.as_path());

    // Incremental cache: skip the wipe if the install directory is already
    // populated and the caller did not ask for a forced reset.  cpanm itself
    // is idempotent (`--local-lib` + `--notest` skips already-installed
    // modules), so keeping `lib/perl5` between runs turns a full rebuild
    // into a cheap delta install.
    let already_populated = is_install_populated(&config.install_dir);
    let should_reset = config.force_reset || !already_populated;

    let cpanm = if Command::new("cpanm").arg("--version").output().is_ok() {
        let launcher = CpanmLauncher::System;
        if should_reset {
            reset_install_dir(&config.install_dir, Some(&launcher), preserved_dist_list)?;
        } else {
            println!(
                "Existing install detected at {} — running incremental update (pass --reset for a full rebuild)",
                config.install_dir.display(),
            );
            fs::create_dir_all(&config.install_dir)
                .context("Failed to create install directory")?;
        }
        launcher
    } else if should_reset {
        reset_install_dir(&config.install_dir, None, preserved_dist_list)?;
        resolve_cpanm_launcher(config)?
    } else {
        println!(
            "Existing install detected at {} — running incremental update (pass --reset for a full rebuild)",
            config.install_dir.display(),
        );
        fs::create_dir_all(&config.install_dir).context("Failed to create install directory")?;
        resolve_cpanm_launcher(config)?
    };

    let local_lib =
        config.install_dir.canonicalize().unwrap_or_else(|_| config.install_dir.clone());
    let normalized_distributions: Vec<String> =
        distributions.iter().map(|dist| normalize_distribution_for_cpanm(dist)).collect();
    let install_items = normalized_distributions.len();
    let cpanm_home = cpanm_home_path(&local_lib);
    fs::create_dir_all(&cpanm_home).context("Failed to create cpanm cache directory")?;

    // Install in batches to avoid overly long command lines
    let batch_size = CPANM_BATCH_SIZE;
    let mut installed = 0usize;
    let mut failed = 0usize;
    let budget = InstallBudget::from_option(config.time_budget);
    let mut budget_stopped = false;

    for (batch_idx, chunk) in normalized_distributions.chunks(batch_size).enumerate() {
        let batch_num = batch_idx + 1;
        let total_batches = distributions.len().div_ceil(batch_size);

        if budget.expired() {
            println!(
                "Install budget exhausted before batch {batch_num}/{total_batches}; \
                 stopping with already-installed state kept intact"
            );
            budget_stopped = true;
            break;
        }
        println!("Batch {batch_num}/{total_batches}: installing {} distributions...", chunk.len());

        let mut cmd = cpanm.command();
        cmd.env("PERL_CPANM_HOME", &cpanm_home);
        cmd.env("PERL_MM_USE_DEFAULT", "1");
        cmd.env("NONINTERACTIVE_TESTING", "1");
        // Some CPAN distributions still try to prompt during configure/build.
        // Detach stdin so batch installs stay noninteractive and fail fast
        // instead of hanging on an inherited TTY.
        cmd.stdin(Stdio::null());
        cmd.arg("--notest");
        cmd.arg("--local-lib");
        cmd.arg(local_lib.display().to_string());
        cmd.arg("--quiet");

        for dist in chunk {
            cmd.arg(dist);
        }

        let output = run_command_with_timeout(cmd, budget.phase_timeout(CPANM_BATCH_TIMEOUT))?;
        let stderr = String::from_utf8_lossy(&output.output.stderr);
        if output.timed_out {
            if budget.expired() {
                println!(
                    "Batch {batch_num} reached the remaining install budget; \
                     stopping with already-installed state kept intact"
                );
                budget_stopped = true;
                break;
            }
            println!(
                "cpanm batch {batch_num} timed out after {}s; retrying distributions individually",
                CPANM_BATCH_TIMEOUT.as_secs()
            );
            let (batch_installed, batch_failed) = install_distributions_individually(
                &cpanm,
                &cpanm_home,
                &local_lib,
                chunk,
                config,
                &budget,
            )?;
            installed += batch_installed;
            failed += batch_failed;
            continue;
        }

        if !output.output.status.success() && config.verbose {
            eprintln!("cpanm batch {batch_num} warnings:\n{stderr}");
        }

        let failed_in_batch = count_cpanm_failures(&stderr);
        failed += failed_in_batch;
        installed += chunk.len().saturating_sub(failed_in_batch);
    }

    if failed > 0 {
        println!("\nInstall complete: {installed} installed, {failed} install failures");
    } else {
        println!("\nInstall complete: {installed} installed, {install_items} requested");
    }
    // Conservative truth guard (review finding on the retry path): if the
    // budget expired anywhere in this pass — including inside the final
    // batch's individual retries, where no loop guard runs again — the plan
    // may not have reached every distribution. Only a pass that provably
    // stopped on its own terms may claim completion; expiry forces the
    // incomplete marker so partial state can never be banked under the
    // canonical key (#12823 CRW-004/008).
    let complete = plan_complete(budget_stopped, &budget);
    if !complete {
        println!(
            "\nInstall stopped inside its time budget before finishing the distribution list; \
             remaining distributions stay queued for the next incremental pass"
        );
    }

    // Machine-readable completion status for the CI warm lane.
    println!("{}", complete_marker(complete));

    // Write inventory of installed .pm files
    let lib_perl5 = local_lib.join("lib/perl5");
    if lib_perl5.exists() {
        let pm_count = walkdir::WalkDir::new(&lib_perl5)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "pm"))
            .count();
        println!("Found {pm_count} .pm files in {}", lib_perl5.display());
    } else {
        println!("Warning: {} not found after install", lib_perl5.display());
    }

    Ok(())
}

fn normalize_distribution_for_cpanm(distribution: &str) -> String {
    if distribution == "libwww-perl" {
        return "LWP".to_string();
    }

    if distribution.contains('-') {
        distribution.replace('-', "::")
    } else {
        distribution.to_string()
    }
}

fn count_cpanm_failures(stderr: &str) -> usize {
    stderr
        .lines()
        .filter(|line| {
            line.contains("Couldn't find module or a distribution")
                || line.contains("Failed to fetch distribution")
                || line.contains("No such file or directory")
        })
        .count()
}

#[derive(Debug)]
struct TimedCommandOutput {
    output: Output,
    timed_out: bool,
}

fn run_command_with_timeout(mut cmd: Command, timeout: Duration) -> Result<TimedCommandOutput> {
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().context("Failed to run cpanm")?;
    let stdout_reader = child.stdout.take().map(spawn_output_reader);
    let stderr_reader = child.stderr.take().map(spawn_output_reader);

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait().context("Failed to poll cpanm process")? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                timed_out = true;
                let _ = child.kill();
                break child.wait().context("Failed to wait for timed-out cpanm process")?;
            }
            None => thread::sleep(Duration::from_millis(200)),
        }
    };

    let stdout = join_output_reader(stdout_reader);
    let stderr = join_output_reader(stderr_reader);

    Ok(TimedCommandOutput { output: Output { status, stdout, stderr }, timed_out })
}

fn spawn_output_reader<R>(mut reader: R) -> thread::JoinHandle<Vec<u8>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Err(err) = reader.read_to_end(&mut buffer) {
            eprintln!(
                "Warning: failed to read cpanm process output: {err} (captured {} bytes)",
                buffer.len()
            );
        }
        buffer
    })
}

fn join_output_reader(handle: Option<thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle.and_then(|join| join.join().ok()).unwrap_or_default()
}

fn install_distributions_individually(
    cpanm: &CpanmLauncher,
    cpanm_home: &Path,
    local_lib: &Path,
    chunk: &[String],
    config: &CpanCorpusConfig,
    budget: &InstallBudget,
) -> Result<(usize, usize)> {
    let mut installed = 0usize;
    let mut failed = 0usize;

    for dist in chunk {
        if budget.expired() {
            println!(
                "Install budget exhausted inside individual retries; {} distribution(s) in \
                 this batch stay queued for the next incremental pass",
                chunk.len().saturating_sub(installed).saturating_sub(failed),
            );
            break;
        }

        let mut cmd = cpanm.command();
        cmd.env("PERL_CPANM_HOME", cpanm_home);
        cmd.env("PERL_MM_USE_DEFAULT", "1");
        cmd.env("NONINTERACTIVE_TESTING", "1");
        cmd.stdin(Stdio::null());
        cmd.arg("--notest");
        cmd.arg("--local-lib");
        cmd.arg(local_lib.display().to_string());
        cmd.arg("--quiet");
        cmd.arg(dist);

        let output =
            run_command_with_timeout(cmd, budget.phase_timeout(CPANM_SINGLE_DIST_TIMEOUT))?;
        let stderr = String::from_utf8_lossy(&output.output.stderr);
        if output.timed_out {
            failed += 1;
            println!("  timed out after {}s: {}", CPANM_SINGLE_DIST_TIMEOUT.as_secs(), dist);
            continue;
        }

        let failed_for_dist = if output.output.status.success() {
            0
        } else {
            let explicit_failures = count_cpanm_failures(&stderr);
            explicit_failures.max(1)
        };

        if failed_for_dist > 0 {
            failed += failed_for_dist;
            if config.verbose {
                eprintln!("cpanm retry warnings for {dist}:\n{stderr}");
            }
        } else {
            installed += 1;
        }
    }

    Ok((installed, failed))
}

#[derive(Debug, Clone)]
enum CpanmLauncher {
    System,
    Bootstrapped(PathBuf),
}

impl CpanmLauncher {
    fn command(&self) -> Command {
        match self {
            Self::System => Command::new("cpanm"),
            Self::Bootstrapped(path) => {
                let mut cmd = Command::new("perl");
                cmd.arg(path);
                cmd
            }
        }
    }
}

fn resolve_cpanm_launcher(config: &CpanCorpusConfig) -> Result<CpanmLauncher> {
    if Command::new("cpanm").arg("--version").output().is_ok() {
        return Ok(CpanmLauncher::System);
    }

    let script_path = bootstrap_cpanm_script(config)?;
    Ok(CpanmLauncher::Bootstrapped(script_path))
}

/// Return true if the install directory already contains a non-empty
/// `lib/perl5` tree, meaning cpanm has previously installed modules here
/// and another install call can run in incremental mode.
fn is_install_populated(install_dir: &Path) -> bool {
    let lib_perl5 = install_dir.join("lib").join("perl5");
    let Ok(mut entries) = fs::read_dir(&lib_perl5) else {
        return false;
    };
    entries.next().is_some()
}

fn bootstrap_cpanm_path(install_dir: &Path) -> PathBuf {
    install_dir.join("bin").join("cpanm")
}

fn cpanm_home_path(install_dir: &Path) -> PathBuf {
    install_dir.join(CPANM_CACHE_DIR)
}

fn reset_install_dir(
    install_dir: &Path,
    launcher: Option<&CpanmLauncher>,
    preserved_file: Option<&Path>,
) -> Result<()> {
    fs::create_dir_all(install_dir).context("Failed to create install directory")?;

    let preserved_cpanm = match launcher {
        Some(CpanmLauncher::Bootstrapped(path)) if path.exists() => {
            Some(fs::read(path).with_context(|| {
                format!("Failed to read bootstrapped cpanm: {}", path.display())
            })?)
        }
        _ => None,
    };

    for entry in fs::read_dir(install_dir)
        .with_context(|| format!("Failed to read install directory: {}", install_dir.display()))?
    {
        let entry = entry.context("Failed to read install directory entry")?;
        let path = entry.path();
        if path.file_name().is_some_and(|name| name == CPANM_CACHE_DIR) && path.is_dir() {
            continue;
        }
        if preserved_file
            .is_some_and(|preserved| preserved == path || preserved.strip_prefix(&path).is_ok())
        {
            continue;
        }

        if path.is_dir() {
            fs::remove_dir_all(&path)
                .with_context(|| format!("Failed to clear {}", path.display()))?;
        } else {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to clear {}", path.display()))?;
        }
    }

    if let Some(bytes) = preserved_cpanm {
        let cpanm_path = bootstrap_cpanm_path(install_dir);
        if let Some(parent) = cpanm_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to recreate {}", parent.display()))?;
        }
        fs::write(&cpanm_path, bytes)
            .with_context(|| format!("Failed to restore {}", cpanm_path.display()))?;
    }

    Ok(())
}

fn bootstrap_cpanm_script(config: &CpanCorpusConfig) -> Result<PathBuf> {
    let script_path = bootstrap_cpanm_path(&config.install_dir);
    if script_path.exists() {
        return Ok(script_path);
    }

    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent).context("Failed to create cpanm bootstrap directory")?;
    }

    println!("System cpanm not found; bootstrapping standalone cpanm to {}", script_path.display());

    let output = Command::new("curl")
        .args(["-fsSL", CPANM_STANDALONE_URL, "-o"])
        .arg(&script_path)
        .output()
        .context("Failed to run curl for standalone cpanm bootstrap")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(color_eyre::eyre::eyre!(
            "Failed to bootstrap standalone cpanm from {}: {}",
            CPANM_STANDALONE_URL,
            stderr,
        ));
    }

    Ok(script_path)
}

// --------------------------------------------------------------------------
// sweep
// --------------------------------------------------------------------------

/// Run the parser corpus sweep against the installed CPAN corpus.
pub fn sweep(config: &CpanCorpusConfig, output: Option<PathBuf>, enforce: bool) -> Result<()> {
    let lib_perl5 = config.install_dir.join("lib/perl5");
    if !lib_perl5.exists() {
        return Err(color_eyre::eyre::eyre!(
            "CPAN corpus not installed: {} not found. Run `cargo xtask cpan-corpus install` first.",
            lib_perl5.display(),
        ));
    }

    let base_roots = vec![lib_perl5.clone()];
    let corpus_roots = parser_corpus_sweep::resolve_corpus_roots(&base_roots);

    let baseline_path = if enforce {
        let bp = workspace_path(CPAN_BASELINE_PATH);
        if bp.exists() {
            Some(bp)
        } else {
            return Err(color_eyre::eyre::eyre!(
                "CPAN baseline missing: {}. Run `cargo xtask cpan-corpus sweep --output {}` or `just cpan-corpus-baseline-update` first.",
                bp.display(),
                CPAN_BASELINE_PATH,
            ));
        }
    } else {
        None
    };

    let sweep_config = parser_corpus_sweep::SweepConfig {
        corpus_profile: Some("cpan".to_string()),
        base_roots: base_roots.clone(),
        corpus_roots: corpus_roots.clone(),
        manifest_path: None,
        manifest_perl5lib: Vec::new(),
        output_path: output,
        baseline_path,
        enforce,
        verbose: config.verbose,
        receipt: true,
    };

    parser_corpus_sweep::run(sweep_config)?;

    if enforce {
        if !config.manifest.exists() {
            return Err(color_eyre::eyre::eyre!(
                "CPAN known-clean manifest missing: {}. Restore the tracked file or seed it with `just cpan-corpus-ratchet` after bootstrap.",
                config.manifest.display(),
            ));
        }

        let manifest_metadata = fs::metadata(&config.manifest)
            .with_context(|| format!("Failed to stat manifest: {}", config.manifest.display()))?;
        if manifest_metadata.len() == 0 {
            return Err(color_eyre::eyre::eyre!(
                "CPAN known-clean manifest is zero-length: {}. Restore the tracked placeholder or seed it with `just cpan-corpus-ratchet`.",
                config.manifest.display(),
            ));
        }

        let manifest_modules = parser_corpus_sweep::parse_manifest(&config.manifest)?;

        if manifest_modules.is_empty() {
            println!(
                "CPAN known-clean manifest is still in bootstrap state; skipping strict clean check ({})",
                config.manifest.display()
            );
        } else {
            println!(
                "\nChecking CPAN known-clean manifest ({} modules)...",
                manifest_modules.len()
            );
            let manifest_sweep = parser_corpus_sweep::SweepConfig {
                corpus_profile: Some("cpan-common".to_string()),
                base_roots,
                corpus_roots: corpus_roots.clone(),
                manifest_path: Some(config.manifest.clone()),
                manifest_perl5lib: corpus_roots,
                output_path: None,
                baseline_path: None,
                enforce: true,
                verbose: config.verbose,
                receipt: false,
            };
            parser_corpus_sweep::run(manifest_sweep)?;
        }
    }

    Ok(())
}

// --------------------------------------------------------------------------
// ratchet
// --------------------------------------------------------------------------

/// Run a sweep and auto-append newly-clean modules to the manifest.
pub fn ratchet(config: &CpanCorpusConfig) -> Result<()> {
    let lib_perl5 = config.install_dir.join("lib/perl5");
    if !lib_perl5.exists() {
        return Err(color_eyre::eyre::eyre!(
            "CPAN corpus not installed: {} not found. Run `cargo xtask cpan-corpus install` first.",
            lib_perl5.display(),
        ));
    }

    // Run a verbose sweep to get per-file results
    let base_roots = vec![lib_perl5.clone()];
    let corpus_roots = parser_corpus_sweep::resolve_corpus_roots(&base_roots);

    // Write a temp report to capture results. Anchor at workspace root so
    // the target directory is the same one cargo uses for builds, even when
    // xtask is invoked from a subdirectory.
    let report_path = workspace_path(CPAN_RATCHET_REPORT_PATH);
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent).context("Failed to create report directory")?;
    }

    let sweep_config = parser_corpus_sweep::SweepConfig {
        corpus_profile: Some("cpan".to_string()),
        base_roots,
        corpus_roots,
        manifest_path: None,
        manifest_perl5lib: Vec::new(),
        output_path: Some(report_path.clone()),
        baseline_path: None,
        enforce: false,
        verbose: true,
        receipt: false,
    };

    parser_corpus_sweep::run(sweep_config)?;

    // Read the report to find clean files
    let report_json = fs::read_to_string(&report_path).context("Failed to read sweep report")?;
    let report: parser_corpus_sweep::SweepReport =
        serde_json::from_str(&report_json).context("Failed to parse sweep report")?;

    // Extract module names from clean file paths
    let clean_modules: BTreeSet<String> = report
        .file_results
        .iter()
        .filter(|r| r.status == "clean")
        .filter_map(|r| path_to_module_name(&r.path, &lib_perl5))
        .collect();

    if clean_modules.is_empty() {
        println!("No clean modules found to add to manifest.");
        return Ok(());
    }

    // Read existing manifest entries
    let existing: BTreeSet<String> = if config.manifest.exists() {
        parser_corpus_sweep::parse_manifest(&config.manifest)?.into_iter().collect()
    } else {
        BTreeSet::new()
    };

    let new_modules: BTreeSet<&String> = clean_modules.difference(&existing).collect();

    if new_modules.is_empty() {
        println!("All {} clean modules already in manifest.", clean_modules.len());
        return Ok(());
    }

    println!(
        "Adding {} new clean modules to manifest (total clean: {})",
        new_modules.len(),
        clean_modules.len()
    );

    // Append new modules to manifest
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config.manifest)
        .with_context(|| format!("Failed to open manifest: {}", config.manifest.display()))?;

    writeln!(file, "#")?;
    writeln!(file, "# Added by ratchet on {}", chrono::Utc::now().to_rfc3339())?;

    for module in &new_modules {
        writeln!(file, "{module}")?;
    }

    println!("Manifest updated: {}", config.manifest.display());

    // Clean up temp report
    let _ = fs::remove_file(&report_path);

    Ok(())
}

// --------------------------------------------------------------------------
// Helpers
// --------------------------------------------------------------------------

/// Read the distribution list, skipping comments and empty lines.
fn read_dist_list(path: &Path) -> Result<Vec<String>> {
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open distribution list: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let mut distributions = Vec::new();
    for line in reader.lines() {
        let line = line.context("Failed to read line")?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        distributions.push(trimmed.to_string());
    }
    Ok(distributions)
}

/// Convert a .pm file path to a Perl module name, relative to the lib directory.
///
/// e.g., `/path/to/lib/perl5/File/Find.pm` → `File::Find`
fn path_to_module_name(file_path: &str, lib_root: &Path) -> Option<String> {
    let path = Path::new(file_path);
    let relative = path.strip_prefix(lib_root).ok()?;

    // Skip architecture-specific subdirs (e.g., x86_64-linux-gnu-thread-multi/)
    let rel_str = relative.to_string_lossy();
    let module_path = if rel_str.contains("auto/") {
        // Skip XS auto directories
        return None;
    } else {
        relative
    };

    let stem = module_path.with_extension("");
    let module_name = stem.to_string_lossy().replace('/', "::");
    Some(module_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("perl-lsp-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn test_read_dist_list_parsing() -> Result<()> {
        let dir = unique_test_dir("cpan-dist-list");
        fs::create_dir_all(&dir)?;
        let path = dir.join("dists.txt");
        fs::write(&path, "# Header comment\n\nMoose\nDBI\n# Another comment\nTry-Tiny\n")?;
        let dists = read_dist_list(&path)?;
        assert_eq!(dists, vec!["Moose", "DBI", "Try-Tiny"]);
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn test_path_to_module_name() {
        let lib = PathBuf::from("/opt/cpan/lib/perl5");
        assert_eq!(
            path_to_module_name("/opt/cpan/lib/perl5/File/Find.pm", &lib),
            Some("File::Find".to_string()),
        );
        assert_eq!(
            path_to_module_name("/opt/cpan/lib/perl5/Moose.pm", &lib),
            Some("Moose".to_string()),
        );
        // auto/ directories should be skipped
        assert_eq!(path_to_module_name("/opt/cpan/lib/perl5/auto/DBI/DBI.so", &lib), None,);
    }

    #[test]
    fn test_path_to_module_name_outside_lib() {
        let lib = PathBuf::from("/opt/cpan/lib/perl5");
        assert_eq!(path_to_module_name("/some/other/path/Foo.pm", &lib), None,);
    }

    #[test]
    fn test_is_install_populated_detects_lib_perl5() -> Result<()> {
        let dir = unique_test_dir("cpan-populated");

        // Missing directory -> not populated
        assert!(!is_install_populated(&dir));

        // Empty install dir -> not populated
        fs::create_dir_all(&dir)?;
        assert!(!is_install_populated(&dir));

        // Empty lib/perl5 -> not populated (no entries)
        fs::create_dir_all(dir.join("lib").join("perl5"))?;
        assert!(!is_install_populated(&dir));

        // At least one file under lib/perl5 -> populated
        fs::write(dir.join("lib").join("perl5").join("Test.pm"), "1;\n")?;
        assert!(is_install_populated(&dir));

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn test_default_config() {
        let config = CpanCorpusConfig::default();
        // Default paths are anchored at the workspace root (see issue #3189)
        // so that `cargo xtask cpan-corpus ...` finds the corpus regardless
        // of the current working directory.
        let root = workspace_root();
        assert_eq!(config.dist_list, root.join(".ci/cpan-top-1000-distributions.txt"));
        assert_eq!(config.manifest, root.join(".ci/cpan-corpus-manifest.txt"));
        assert_eq!(config.install_dir, root.join("target/cpan-corpus"));
        assert_eq!(config.top_n, 1000);
    }

    #[test]
    fn test_workspace_root_points_at_workspace() {
        // Guardrail: `workspace_root()` must point at a directory that
        // contains the top-level workspace `Cargo.toml`. If xtask ever
        // moves out of `<workspace>/xtask/`, this assertion catches the
        // regression so the corpus ratchet path resolution does not drift.
        let root = workspace_root();
        assert!(
            root.join("Cargo.toml").exists(),
            "workspace_root() = {} must contain Cargo.toml",
            root.display()
        );
    }

    #[test]
    fn test_cpan_baseline_path_constant() {
        assert_eq!(CPAN_BASELINE_PATH, ".ci/cpan-corpus-baseline.json");
    }

    #[test]
    fn test_cpanm_batch_settings_constants() {
        // Regression guard: these values were chosen to give ~12s per-module
        // budget (300s / 25) while keeping batch-wedge retries manageable.
        // If you change them, update the comment in the install() function too.
        assert_eq!(CPANM_BATCH_TIMEOUT.as_secs(), 300);
        assert_eq!(CPANM_BATCH_SIZE, 25);
    }

    #[test]
    fn unbudgeted_install_matches_legacy_behavior() {
        // CRW-009: default config carries no deadline, and the phase timeout
        // with no budget is exactly the historical fixed ceiling.
        let config = CpanCorpusConfig::default();
        assert!(config.time_budget.is_none());

        let budget = InstallBudget::from_option(None);
        assert!(!budget.expired());
        assert_eq!(budget.phase_timeout(CPANM_BATCH_TIMEOUT), CPANM_BATCH_TIMEOUT);
        assert_eq!(budget.phase_timeout(CPANM_SINGLE_DIST_TIMEOUT), CPANM_SINGLE_DIST_TIMEOUT);
    }

    #[test]
    fn per_batch_timeout_caps_at_fixed_batch_ceiling_and_remaining_budget() {
        // CRW-008 part 1: a generous budget never extends the fixed ceiling;
        // a tight budget truncates the phase timeout to the remaining time.
        let long_deadline = Instant::now() + Duration::from_secs(600);
        let generous = InstallBudget { deadline: Some(long_deadline) };
        assert_eq!(generous.phase_timeout(CPANM_BATCH_TIMEOUT), CPANM_BATCH_TIMEOUT);

        let tight = InstallBudget { deadline: Some(Instant::now() + Duration::from_millis(250)) };
        let timeout = tight.phase_timeout(CPANM_BATCH_TIMEOUT);
        assert!(timeout <= Duration::from_millis(250), "budget must truncate the phase timeout");
    }

    #[test]
    fn expired_budget_attempts_no_further_batches_and_reports_incomplete() {
        // CRW-008 part 2: with an exhausted budget at most zero further
        // batches start (guard placed exactly where install() checks it), the
        // plan stops early, and the marker reports incomplete.
        let budget = InstallBudget::from_option(Some(Duration::from_millis(5)));
        thread::sleep(Duration::from_millis(40));

        let distributions: Vec<usize> = (0..50).collect();
        let mut attempted = 0usize;
        for _chunk in distributions.chunks(CPANM_BATCH_SIZE) {
            if budget.expired() {
                break;
            }
            attempted += 1;
        }
        assert_eq!(attempted, 0);
        assert_eq!(complete_marker(false), "CPAN_CORPUS_INSTALL_COMPLETE=false");
    }

    #[test]
    fn complete_marker_reflects_batch_plan_outcome() {
        // CRW-004: markers consumed by the workflow output capture.
        assert_eq!(complete_marker(true), "CPAN_CORPUS_INSTALL_COMPLETE=true");
    }

    #[test]
    fn budget_expiry_at_plan_end_forces_incomplete_marker() {
        // Regression pin for the review finding on the individual-retry path:
        // when the final batch's retries consume the remaining budget, no
        // loop guard runs again — so plan end itself must treat an expired
        // deadline as incomplete, or a partially-attempted corpus would be
        // banked under the canonical key and enforced by the ratchet.
        let expired = InstallBudget { deadline: Some(Instant::now() - Duration::from_secs(1)) };
        assert!(expired.expired());
        assert!(!plan_complete(false, &expired), "post-hoc expiry must force incomplete");
        assert!(!plan_complete(true, &expired), "explicit stop must force incomplete");
        assert_eq!(
            complete_marker(plan_complete(false, &expired)),
            "CPAN_CORPUS_INSTALL_COMPLETE=false"
        );

        let live = InstallBudget { deadline: Some(Instant::now() + Duration::from_secs(600)) };
        assert!(plan_complete(false, &live), "a finished pass inside its budget is complete");
    }

    #[test]
    fn unbudgeted_pass_completion_never_reads_a_deadline() {
        // CRW-009 seam: with no budget configured the deadline arm is inert —
        // legacy byte-for-byte behavior reports a clean pass as complete, and
        // only an explicit stop flag can make it incomplete.
        let none = InstallBudget::from_option(None);
        assert!(plan_complete(false, &none));
        assert!(!plan_complete(true, &none), "an explicit stop is incomplete regardless of budget");
    }

    #[test]
    fn test_cpanm_batch_size_chunking() {
        // Verify CPANM_BATCH_SIZE produces correct batch counts when used as
        // the chunk size — matches how install() uses it via .chunks(batch_size).
        // This test would fail if CPANM_BATCH_SIZE were accidentally set to 0
        // (panic) or to a value that doesn't divide the list as expected.
        let dists: Vec<String> = (0..60).map(|i| format!("Dist-{i}")).collect();
        let chunks: Vec<_> = dists.chunks(CPANM_BATCH_SIZE).collect();
        // 60 items / 25 per batch = 3 batches (25, 25, 10)
        assert_eq!(chunks.len(), 3, "expected 3 batches for 60 items at size {CPANM_BATCH_SIZE}");
        assert_eq!(chunks[0].len(), CPANM_BATCH_SIZE, "first batch should be full");
        assert_eq!(chunks[1].len(), CPANM_BATCH_SIZE, "second batch should be full");
        assert_eq!(chunks[2].len(), 10, "last batch should contain the remainder");

        // div_ceil matches install()'s total_batches formula
        assert_eq!(60usize.div_ceil(CPANM_BATCH_SIZE), 3);
    }

    #[test]
    fn test_bootstrap_cpanm_path() {
        let install_dir = PathBuf::from("target/cpan-corpus");
        assert_eq!(bootstrap_cpanm_path(&install_dir), install_dir.join("bin").join("cpanm"));
    }

    #[test]
    fn test_cpanm_home_path() {
        let install_dir = PathBuf::from("target/cpan-corpus");
        assert_eq!(cpanm_home_path(&install_dir), install_dir.join(".cpanm"));
    }

    #[test]
    fn test_normalize_distribution_for_cpanm() {
        assert_eq!(normalize_distribution_for_cpanm("Try-Tiny"), "Try::Tiny");
        assert_eq!(normalize_distribution_for_cpanm("ExtUtils-MakeMaker"), "ExtUtils::MakeMaker");
        assert_eq!(normalize_distribution_for_cpanm("namespace-autoclean"), "namespace::autoclean");
        assert_eq!(normalize_distribution_for_cpanm("libwww-perl"), "LWP");
        assert_eq!(normalize_distribution_for_cpanm("PathTools"), "PathTools");
    }

    #[test]
    fn test_count_cpanm_failures() {
        let stderr = "! Couldn't find module or a distribution Test-Simple\n\
! Failed to fetch distribution Foo\n\
Some other line";
        assert_eq!(count_cpanm_failures(stderr), 2);
    }

    #[test]
    fn test_run_command_with_timeout_captures_stderr() -> Result<()> {
        let mut cmd = Command::new("perl");
        cmd.args(["-e", "print STDERR qq(warn\\n);"]);

        let result = run_command_with_timeout(cmd, Duration::from_secs(2))?;
        assert!(!result.timed_out);
        assert!(result.output.status.success());
        assert_eq!(String::from_utf8_lossy(&result.output.stderr), "warn\n");
        Ok(())
    }

    #[test]
    fn test_run_command_with_timeout_kills_hung_process() -> Result<()> {
        let mut cmd = Command::new("perl");
        cmd.args(["-e", "$|=1; print STDERR qq(waiting\\n); sleep 5;"]);

        let started = Instant::now();
        let result = run_command_with_timeout(cmd, Duration::from_millis(200))?;
        assert!(result.timed_out);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timed command should be interrupted quickly"
        );
        assert!(String::from_utf8_lossy(&result.output.stderr).contains("waiting"));
        Ok(())
    }

    #[test]
    fn test_reset_install_dir_preserves_cache_and_bootstrapped_cpanm() -> Result<()> {
        let install_dir = unique_test_dir("cpan-reset-preserve");
        let cache_dir = install_dir.join(CPANM_CACHE_DIR);
        let cache_file = cache_dir.join("cache.txt");
        let lib_file = install_dir.join("lib/perl5/Test.pm");
        let man_file = install_dir.join("man/man3/Test.3pm");
        let cpanm_path = bootstrap_cpanm_path(&install_dir);

        fs::create_dir_all(cache_dir.clone())?;
        fs::create_dir_all(lib_file.parent().unwrap_or(&install_dir))?;
        fs::create_dir_all(man_file.parent().unwrap_or(&install_dir))?;
        fs::create_dir_all(cpanm_path.parent().unwrap_or(&install_dir))?;
        fs::write(&cache_file, "cache")?;
        fs::write(&lib_file, "module")?;
        fs::write(&man_file, "man")?;
        fs::write(&cpanm_path, "#!/usr/bin/env perl\n")?;

        reset_install_dir(
            &install_dir,
            Some(&CpanmLauncher::Bootstrapped(cpanm_path.clone())),
            None,
        )?;

        assert!(cache_file.exists());
        assert!(cpanm_path.exists());
        assert!(!install_dir.join("lib").exists());
        assert!(!install_dir.join("man").exists());

        fs::remove_dir_all(&install_dir)?;
        Ok(())
    }

    #[test]
    fn test_reset_install_dir_removes_stale_bin_for_system_cpanm() -> Result<()> {
        let install_dir = unique_test_dir("cpan-reset-system");
        let stale_bin = install_dir.join("bin/old-tool");
        let cache_dir = install_dir.join(CPANM_CACHE_DIR);

        fs::create_dir_all(stale_bin.parent().unwrap_or(&install_dir))?;
        fs::create_dir_all(&cache_dir)?;
        fs::write(&stale_bin, "old")?;

        reset_install_dir(&install_dir, Some(&CpanmLauncher::System), None)?;

        assert!(!install_dir.join("bin").exists());
        assert!(cache_dir.exists());

        fs::remove_dir_all(&install_dir)?;
        Ok(())
    }

    #[test]
    fn test_reset_install_dir_preserves_dist_list_under_install_dir() -> Result<()> {
        let install_dir = unique_test_dir("cpan-reset-dist-list");
        let dist_list = install_dir.join("lists/top-1000.txt");
        let stale_lib = install_dir.join("lib/perl5/Test.pm");

        fs::create_dir_all(dist_list.parent().unwrap_or(&install_dir))?;
        fs::create_dir_all(stale_lib.parent().unwrap_or(&install_dir))?;
        fs::write(&dist_list, "Test-Simple\n")?;
        fs::write(&stale_lib, "module")?;

        reset_install_dir(&install_dir, Some(&CpanmLauncher::System), Some(&dist_list))?;

        assert!(dist_list.exists());
        assert!(!install_dir.join("lib").exists());

        fs::remove_dir_all(&install_dir)?;
        Ok(())
    }
}
