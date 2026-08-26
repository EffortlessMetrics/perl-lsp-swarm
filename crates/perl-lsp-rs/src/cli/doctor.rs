//! Read-only first-run setup report.

#![expect(
    clippy::print_stderr,
    reason = "CLI tool module prints user-facing setup errors to stderr"
)]
#![expect(clippy::print_stdout, reason = "CLI tool module prints the doctor report to stdout")]

use crate::util::run_command_with_timeout;
use perl_lsp_rs_core::config::{
    Perl5LibPrecedence, PerlOracleEnv, RejectedIncludePath, WorkspaceConfig, load_project_config,
};
use perl_lsp_rs_core::external_tool_doctor::{
    critic_compatibility_entry, external_tool_doctor_entries, render_critic_compatibility_text,
    render_external_tool_doctor_text,
};
use perl_lsp_rs_core::external_tools::EXTERNAL_TOOL_REGISTRY;
use perl_lsp_rs_core::platform::{detect_perlbrew_perl, detect_plenv_perl, resolve_perl_path};
use perl_parser_core::path_security::{WorkspacePathError, validate_workspace_path};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const PERL5LIB_SOURCE: &str = "PERL5LIB";

/// Wall-clock timeout for external tool (`perltidy`/`perlcritic`) version probes.
const DOCTOR_TOOL_TIMEOUT_SECS: u64 = 5;

/// `perllsp --doctor --external-tools`: registry-driven, native-first
/// external-tooling report (#7212). Source-only projection of the canonical
/// registry (#7209): no probe, install, selection, or execution occurs, and
/// every verdict comes from the registry rows.
pub(super) fn run_doctor_external_tools(json: bool) -> i32 {
    let entries = external_tool_doctor_entries(EXTERNAL_TOOL_REGISTRY);
    if json {
        match serde_json::to_string_pretty(&entries) {
            Ok(json_str) => {
                println!("{json_str}");
                0
            }
            Err(err) => {
                eprintln!("Failed to serialize external-tool doctor report: {err}");
                1
            }
        }
    } else {
        print!("{}", render_external_tool_doctor_text(&entries));
        0
    }
}

/// `perllsp --doctor --critic-compatibility`: registry-driven Perl::Critic
/// configuration compatibility (#7212). Explains `.perlcriticrc` mapping
/// process-free; never offers a runtime engine switch.
pub(super) fn run_doctor_critic_compatibility(json: bool) -> i32 {
    let Some(entry) = critic_compatibility_entry(EXTERNAL_TOOL_REGISTRY) else {
        eprintln!("registry does not own a .perlcriticrc compatibility row");
        return 1;
    };
    if json {
        match serde_json::to_string_pretty(&entry) {
            Ok(json_str) => {
                println!("{json_str}");
                0
            }
            Err(err) => {
                eprintln!("Failed to serialize critic compatibility report: {err}");
                1
            }
        }
    } else {
        print!("{}", render_critic_compatibility_text(&entry));
        0
    }
}

/// `perllsp --doctor --dev-environment`: read-only development-environment
/// prerequisite report (#12595). Probes this machine for the four Windows
/// clone-and-make findings behind the #11869 audit: symlink privilege
/// (#12567), per-shell cargo/rustc identity versus the workspace toolchain
/// pin, bash flavor coverage for repository POSIX entrypoints, and Perl
/// identity divergence for DAP E2E / prove consumers. Typed statuses and
/// copyable fix lines follow the #7212 posture; the arm never auto-mutates
/// the machine beyond creating and removing one temporary symlink.
pub(super) fn run_doctor_dev_environment(json: bool) -> i32 {
    let report = build_dev_environment_report();
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(json_str) => {
                println!("{json_str}");
                0
            }
            Err(err) => {
                eprintln!("Failed to serialize dev-environment doctor report: {err}");
                1
            }
        }
    } else {
        print!("{}", render_dev_environment_report(&report));
        0
    }
}

pub(super) fn run_doctor(dir: &str, json: bool) -> i32 {
    match build_doctor_report_struct(dir) {
        Ok(report) => {
            if json {
                match serde_json::to_string_pretty(&report) {
                    Ok(json_str) => {
                        println!("{json_str}");
                        0
                    }
                    Err(err) => {
                        eprintln!("Failed to serialize doctor report: {err}");
                        1
                    }
                }
            } else {
                print!("{}", render_report(report));
                0
            }
        }
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

fn build_doctor_report_struct(dir: &str) -> Result<DoctorReport, String> {
    let workspace = workspace_dir(dir)?;
    let mut workspace_config = WorkspaceConfig::default();
    let config_report = load_workspace_config(&workspace, &mut workspace_config)?;
    let perl5lib_paths = std::env::var("PERL5LIB")
        .map(|value| WorkspaceConfig::parse_perl5lib(&value))
        .unwrap_or_default();
    let perl_report = probe_perl(&workspace_config, &workspace);
    let perltidy_report = probe_tool("perltidy", "--version");
    let perlcritic_report = probe_tool("perlcritic", "--version");
    let configured_paths = include_path_reports(
        &workspace,
        &workspace_config.include_paths,
        config_report.include_source,
    );
    let effective_paths = effective_root_reports(
        &workspace,
        &workspace_config,
        &perl5lib_paths,
        config_report.include_source,
    );
    let system_inc = system_inc_report(&workspace, &mut workspace_config);

    Ok(DoctorReport {
        workspace,
        config: config_report,
        perl: perl_report,
        perltidy: perltidy_report,
        perlcritic: perlcritic_report,
        perl5lib_paths,
        perl5lib_enabled: workspace_config.use_perl5lib,
        perl5lib_precedence: workspace_config.perl5lib_precedence.clone(),
        configured_paths,
        effective_paths,
        system_inc,
    })
}

#[derive(Serialize)]
struct DoctorReport {
    workspace: PathBuf,
    config: ProjectConfigReport,
    perl: PerlReport,
    perltidy: ToolReport,
    perlcritic: ToolReport,
    perl5lib_paths: Vec<String>,
    perl5lib_enabled: bool,
    perl5lib_precedence: Perl5LibPrecedence,
    configured_paths: Vec<PathReport>,
    effective_paths: Vec<PathReport>,
    system_inc: SystemIncReport,
}

#[derive(Clone, Serialize)]
struct ProjectConfigReport {
    status: ProjectConfigStatus,
    include_source: &'static str,
    /// Human-readable rendering of `.perl-lsp.toml` `include_paths` entries
    /// rejected during load (absolute, or escaping the workspace root). Empty
    /// when nothing was rejected. See
    /// `perl_lsp_rs_core::config::RejectedIncludePath::render`.
    rejected_include_paths: Vec<String>,
}

#[derive(Clone, Copy, Serialize)]
enum ProjectConfigStatus {
    Loaded,
    Missing,
}

#[derive(Serialize)]
struct PerlReport {
    binary: Option<PathBuf>,
    source: &'static str,
    version: Option<String>,
    error: Option<String>,
}

/// Report for an external tool (`perltidy`/`perlcritic`) that perl-lsp may
/// shell out to. Detection is read-only: the binary is located on `PATH` and
/// asked for its version. A missing binary is reported, not fatal — doctor
/// stays read-only and exits 0 as long as it can produce a report.
#[derive(Serialize)]
struct ToolReport {
    binary: Option<PathBuf>,
    source: &'static str,
    version: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct PathReport {
    raw: String,
    resolved: PathBuf,
    source: &'static str,
    status: &'static str,
}

struct EffectiveRootCandidate {
    raw: String,
    source: &'static str,
}

#[derive(Serialize)]
struct SystemIncReport {
    status: &'static str,
    paths: Vec<PathBuf>,
}

fn workspace_dir(dir: &str) -> Result<PathBuf, String> {
    let root = Path::new(dir);
    let metadata = match root.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("{dir}: directory not found"));
        }
        Err(error) => return Err(format!("{dir}: cannot access directory: {error}")),
    };

    if !metadata.is_dir() {
        return Err(format!("{dir}: not a directory"));
    }

    canonicalize_workspace_dir(dir, root)
}

fn canonicalize_workspace_dir(dir: &str, root: &Path) -> Result<PathBuf, String> {
    root.canonicalize().map_err(|error| format!("{dir}: cannot resolve directory: {error}"))
}

fn load_workspace_config(
    workspace: &Path,
    workspace_config: &mut WorkspaceConfig,
) -> Result<ProjectConfigReport, String> {
    match load_project_config(workspace) {
        Ok(Some(project_config)) => {
            let include_source = if project_config.perl.include_paths.is_empty() {
                "default includePaths"
            } else {
                ".perl-lsp.toml include_paths"
            };
            let rejected = project_config.apply_to_workspace_config(workspace_config, workspace);
            Ok(ProjectConfigReport {
                status: ProjectConfigStatus::Loaded,
                include_source,
                rejected_include_paths: rejected.iter().map(RejectedIncludePath::render).collect(),
            })
        }
        Ok(None) => Ok(ProjectConfigReport {
            status: ProjectConfigStatus::Missing,
            include_source: "default includePaths",
            rejected_include_paths: Vec::new(),
        }),
        Err(error) => Err(format!("{}: {error}", workspace.join(".perl-lsp.toml").display())),
    }
}

fn probe_perl(config: &WorkspaceConfig, workspace: &Path) -> PerlReport {
    probe_perl_with_resolver(config, workspace, resolve_perl_path_for_doctor)
}

fn probe_perl_with_resolver(
    config: &WorkspaceConfig,
    workspace: &Path,
    resolve_perl_path: impl FnOnce() -> anyhow::Result<(PathBuf, &'static str)>,
) -> PerlReport {
    let (binary, source) = match config.perl_path.as_deref().filter(|path| !path.trim().is_empty())
    {
        Some(path) => (PathBuf::from(path), "configured perl_path"),
        None => match resolve_perl_path() {
            Ok((path, source)) => (path, source),
            Err(error) => {
                return PerlReport {
                    binary: None,
                    source: "PATH",
                    version: None,
                    error: Some(format!("{error}")),
                };
            }
        },
    };

    let oracle = PerlOracleEnv::for_version_probe(binary.clone(), workspace.to_path_buf());
    let mut command = oracle.into_command();
    command.args(["-e", "print $^V"]);
    let timeout_secs = oracle.timeout.as_secs().max(1);
    match run_command_with_timeout(command, timeout_secs) {
        Ok(output) if output.status.success() => PerlReport {
            binary: Some(binary),
            source,
            version: Some(String::from_utf8_lossy(&output.stdout).trim().to_string()),
            error: None,
        },
        Ok(output) => PerlReport {
            binary: Some(binary),
            source,
            version: None,
            error: Some(version_probe_error(&output)),
        },
        Err(error) => {
            PerlReport { binary: Some(binary), source, version: None, error: Some(error) }
        }
    }
}

fn version_probe_error(output: &std::process::Output) -> String {
    version_probe_error_from_parts(&output.status.to_string(), &output.stderr)
}

/// Locate an external tool on `PATH`, using the hardened resolver on Windows
/// to avoid binary-planting via relative PATH entries or the CWD (#2764/#3028).
fn resolve_tool_on_path(name: &str) -> Option<PathBuf> {
    #[cfg(all(windows, not(target_arch = "wasm32")))]
    {
        perl_subprocess_runtime::resolve_program(name).ok().map(PathBuf::from)
    }
    #[cfg(all(not(windows), not(target_arch = "wasm32")))]
    {
        which::which(name).ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        None
    }
}

/// Locate `name` on `PATH` and ask it for its version via `version_arg`.
fn probe_tool(name: &'static str, version_arg: &str) -> ToolReport {
    probe_tool_with_resolver(name, version_arg, resolve_tool_on_path)
}

/// Dependency-injected core of [`probe_tool`]. `resolve` maps the tool name to
/// a discovered binary path (or `None` when absent), so tests can exercise the
/// missing-binary and version-probe-failure branches without a real install.
fn probe_tool_with_resolver(
    name: &'static str,
    version_arg: &str,
    resolve: impl FnOnce(&str) -> Option<PathBuf>,
) -> ToolReport {
    let binary = match resolve(name) {
        Some(path) => path,
        None => {
            return ToolReport {
                binary: None,
                source: "PATH",
                version: None,
                error: Some(format!("{name} not found on PATH")),
            };
        }
    };

    let mut command = Command::new(&binary);
    command.arg(version_arg);
    match run_command_with_timeout(command, DOCTOR_TOOL_TIMEOUT_SECS) {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from);
            ToolReport { binary: Some(binary), source: "PATH", version, error: None }
        }
        Ok(output) => ToolReport {
            binary: Some(binary),
            source: "PATH",
            version: None,
            error: Some(tool_version_probe_error(name, &output)),
        },
        Err(error) => {
            ToolReport { binary: Some(binary), source: "PATH", version: None, error: Some(error) }
        }
    }
}

fn tool_version_probe_error(name: &str, output: &std::process::Output) -> String {
    format!("{name} {}", version_probe_error(output))
}

fn version_probe_error_from_parts(status: &str, stderr: &[u8]) -> String {
    let mut error = format!("version probe exited with status {status}");
    let stderr = String::from_utf8_lossy(stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        error.push_str("; stderr: ");
        error.push_str(stderr);
    }
    error
}

fn resolve_perl_path_for_doctor() -> anyhow::Result<(PathBuf, &'static str)> {
    if let Some(path) = detect_perlbrew_perl() {
        return Ok((path, "perlbrew"));
    }
    if let Some(path) = detect_plenv_perl() {
        return Ok((path, "plenv"));
    }
    resolve_perl_path().map(|path| (path, "PATH"))
}

fn include_path_reports(
    workspace: &Path,
    include_paths: &[String],
    source: &'static str,
) -> Vec<PathReport> {
    include_paths.iter().map(|path| path_report(workspace, path, source)).collect()
}

fn effective_root_reports(
    workspace: &Path,
    config: &WorkspaceConfig,
    perl5lib_paths: &[String],
    include_source: &'static str,
) -> Vec<PathReport> {
    let candidates = effective_root_candidates(config, perl5lib_paths, include_source);
    config
        .effective_include_paths(perl5lib_paths)
        .iter()
        .map(|path| {
            let (raw, source) = effective_root_details(path, &candidates);
            path_report_with_raw(workspace, path, raw, source)
        })
        .collect()
}

fn effective_root_candidates(
    config: &WorkspaceConfig,
    perl5lib_paths: &[String],
    include_source: &'static str,
) -> Vec<EffectiveRootCandidate> {
    let include_paths = config
        .include_paths
        .iter()
        .map(|path| EffectiveRootCandidate { raw: path.clone(), source: include_source });
    let perl5lib_paths = perl5lib_paths
        .iter()
        .map(|path| EffectiveRootCandidate { raw: path.clone(), source: PERL5LIB_SOURCE });

    if !config.use_perl5lib {
        return include_paths.collect();
    }

    match config.perl5lib_precedence {
        Perl5LibPrecedence::Prepend => perl5lib_paths.chain(include_paths).collect(),
        Perl5LibPrecedence::Append => include_paths.chain(perl5lib_paths).collect(),
    }
}

fn effective_root_details<'a>(
    path: &'a str,
    candidates: &'a [EffectiveRootCandidate],
) -> (&'a str, &'static str) {
    let normalized = normalized_path_text(path);
    for candidate in candidates {
        if normalized_path_text(&candidate.raw) == normalized {
            return (candidate.raw.as_str(), candidate.source);
        }
    }
    (path, "effective include path")
}

fn normalized_path_text(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = Path::new(trimmed).components().fold(PathBuf::new(), |mut acc, component| {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::Prefix(_)
            | std::path::Component::ParentDir
            | std::path::Component::Normal(_) => acc.push(component.as_os_str()),
        }
        acc
    });

    if normalized.as_os_str().is_empty() {
        ".".to_string()
    } else {
        normalized.to_string_lossy().into_owned()
    }
}

fn path_report(workspace: &Path, raw: &str, source: &'static str) -> PathReport {
    path_report_with_raw(workspace, raw, raw, source)
}

fn path_report_with_raw(
    workspace: &Path,
    resolved_path: &str,
    raw: &str,
    source: &'static str,
) -> PathReport {
    let resolved = resolve_path(workspace, resolved_path, source);
    let status = path_status(workspace, resolved_path, &resolved, source);
    PathReport { raw: raw.to_string(), resolved, source, status }
}

fn resolve_path(workspace: &Path, raw: &str, source: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else if is_perl5lib_source(source) {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    } else {
        workspace.join(path)
    }
}

fn is_perl5lib_source(source: &str) -> bool {
    source == "PERL5LIB"
}

fn path_status(workspace: &Path, raw: &str, path: &Path, source: &str) -> &'static str {
    let raw_path = Path::new(raw);
    if !is_perl5lib_source(source)
        && !raw_path.is_absolute()
        && let Err(error) = validate_workspace_path(raw_path, workspace)
    {
        return match error {
            WorkspacePathError::InvalidPathCharacters => "unsafe: invalid path",
            WorkspacePathError::PathOutsideWorkspace(_)
            | WorkspacePathError::PathTraversalAttempt(_)
            | WorkspacePathError::SymlinkOutsideWorkspace(_) => "unsafe: outside workspace",
        };
    }

    if path.is_dir() {
        "exists"
    } else if path.exists() {
        "not a directory"
    } else {
        "missing"
    }
}

fn system_inc_report(workspace: &Path, config: &mut WorkspaceConfig) -> SystemIncReport {
    if !config.use_system_inc {
        return SystemIncReport { status: "disabled", paths: Vec::new() };
    }

    let probe_cwd = PerlOracleEnv::for_startup_inc_probe(config)
        .map(|oracle| oracle.cwd)
        .unwrap_or_else(|| workspace.to_path_buf());
    let paths = config.get_system_inc().to_vec();
    let status = if paths.is_empty() { "enabled, no paths returned" } else { "enabled" };
    let resolved = paths
        .into_iter()
        .map(|path| if path.is_absolute() { path } else { probe_cwd.join(path) })
        .collect();
    SystemIncReport { status, paths: resolved }
}

// ── Development-environment prerequisites arm (#12595) ─────────────────────
//
// Privacy posture follows #7212: typed statuses from a closed set, resolved
// tool identities only, copyable environment-scoped fix lines, probe details
// that carry OS error codes rather than paths, and no auto-mutation.

/// Schema tag for the versioned dev-environment JSON report.
const DEV_ENVIRONMENT_SCHEMA: &str = "perllsp-doctor-dev-environment-v1";

/// Workspace MSRV label ([workspace.package] rust-version). Cross-checked
/// against rust-toolchain.toml and the crate manifest by the pin test.
const WORKSPACE_RUST_VERSION_LABEL: &str = "1.95";
const WORKSPACE_RUST_VERSION_MAJOR: u64 = 1;
const WORKSPACE_RUST_VERSION_MINOR: u64 = 95;

/// Channel pinned by rust-toolchain.toml. Rustup shims honor it; distro
/// shims (apt cargo) ignore it, which is exactly the edition-2024 failure
/// this arm exists to surface (#12595).
const TOOLCHAIN_CHANNEL_LABEL: &str = "1.95.0";

/// Prerequisite found and healthy for its role.
const STATUS_PRESENT: &str = "present";
/// Required prerequisite absent or unreachable.
const STATUS_MISSING: &str = "missing";
/// Probe does not apply on this host or flavor.
const STATUS_NOT_APPLICABLE: &str = "not_applicable";
/// Reachable but below the workspace MSRV.
const STATUS_STALE: &str = "stale";
/// Reachable but a distro/shim cargo that ignores rust-toolchain.toml.
const STATUS_NON_RUSTUP: &str = "non_rustup";
/// More than one distinct identity discoverable; consumers may pick either.
const STATUS_DIVERGENT: &str = "divergent";
/// The probe itself could not determine an honest verdict.
const STATUS_PROBE_ERROR: &str = "probe_error";

/// The closed set of dev-environment statuses, asserted by the schema tests;
/// JSON consumers may match on these codes exhaustively.
#[cfg(test)]
fn dev_env_status_codes() -> [&'static str; 7] {
    [
        STATUS_PRESENT,
        STATUS_MISSING,
        STATUS_NOT_APPLICABLE,
        STATUS_STALE,
        STATUS_NON_RUSTUP,
        STATUS_DIVERGENT,
        STATUS_PROBE_ERROR,
    ]
}

const PROVENANCE_RUSTUP_SHIM: &str = "rustup_shim";
const PROVENANCE_NON_RUSTUP: &str = "non_rustup";
const PROVENANCE_UNKNOWN: &str = "unknown";

const IDENTITY_MSYS_CYGWIN: &str = "msys_cygwin";
const IDENTITY_STRAWBERRY: &str = "strawberry";
const IDENTITY_SYSTEM_UNIX: &str = "system_unix";
const IDENTITY_UNKNOWN: &str = "unknown";

const FLAVOR_NATIVE_SHELL: &str = "native_shell";
const FLAVOR_GIT_BASH: &str = "git_bash";
const FLAVOR_WSL: &str = "wsl";

const HOST_PLATFORM_WINDOWS: &str = "windows";
const HOST_PLATFORM_UNIX: &str = "unix";

/// Wall-clock timeout for one dev-environment process probe.
const DEV_ENV_PROBE_TIMEOUT_SECS: u64 = 10;
/// WSL cold starts can take far longer than an ordinary spawn.
const DEV_ENV_WSL_TIMEOUT_SECS: u64 = 30;

/// Probe detail text is diagnosis, not logging; keep snippets bounded.
const DETAIL_MAX_CHARS: usize = 240;

/// os error 1314 (`ERROR_PRIVILEGE_NOT_HELD`): creating symlinks requires
/// Developer Mode or an elevated token (#12567).
#[cfg(windows)]
const WINDOWS_ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

/// Prescribed enablement step for the symlink privilege finding (#12595).
const FIX_SYMLINK_PRIVILEGE: &str =
    "Settings > Privacy & security > For developers > Developer Mode: On, or run from an elevated shell";

/// Copyable rustup bootstrap; guidance only — doctor never executes it.
const RUSTUP_INSTALL_ONE_LINER: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh";

const FIX_BASH_INSTALL_GIT_WINDOWS: &str =
    "fix: install Git for Windows so a POSIX bash is available: winget install --id Git.Git -e";

const FIX_PERL_IDENTITY_DIVERGENCE: &str = "fix: reorder PATH so your intended perl resolves first ('where perl' lists resolution order on Windows, 'which -a perl' elsewhere), or pin [perl] perl_path in .perl-lsp.toml";
const FIX_PERL_MISSING_WINDOWS: &str =
    "fix: install a native perl (for example Strawberry Perl) ahead of MSYS entries on PATH";
const FIX_PERL_MISSING_UNIX: &str =
    "fix: install a system perl via the distribution package manager";

/// Marker walked up from the working directory to locate the checkout.
const REPO_ENTRYPOINT_MARKER: &str = ".github/run_all_tests.sh";
/// Repository files whose execution assumes a POSIX bash (#12595).
const REPO_BASH_ENTRYPOINTS: [&str; 3] =
    [".github/run_all_tests.sh", "scripts", "scripts/cargo-safe"];
/// Documented-prerequisite line demanded by #12595.
const BASH_PREREQUISITE_LINE: &str = "Repository conformance entrypoints (.github/run_all_tests.sh, scripts/*.sh, scripts/cargo-safe) assume a POSIX bash; Git Bash ships with Git for Windows.";

const MAX_REPO_ROOT_WALK_DEPTH: usize = 12;

/// One parsed `major.minor.patch` toolchain version triple.
type VersionTriple = (u64, u64, u64);

/// Stdout/stderr bytes from a version probe, already run under its timeout.
type ProbeOutput = Result<std::process::Output, String>;

#[derive(Serialize)]
struct DevEnvironmentReport {
    schema: &'static str,
    host_platform: &'static str,
    workspace_rust_version: &'static str,
    toolchain_channel_pin: &'static str,
    symlink_privilege: SymlinkPrivilegeReport,
    cargo_toolchains: Vec<CargoToolchainReport>,
    bash_flavors: Vec<BashFlavorReport>,
    repo_entrypoints: RepoEntrypointsReport,
    documented_prerequisite: &'static str,
    perl_identity: PerlIdentityReport,
}

#[derive(Serialize)]
struct SymlinkPrivilegeReport {
    status: &'static str,
    detail: String,
    fix: Option<String>,
}

#[derive(Serialize)]
struct CargoToolchainReport {
    flavor: &'static str,
    status: &'static str,
    path: Option<String>,
    version: Option<String>,
    provenance: &'static str,
    meets_workspace_pin: Option<bool>,
    honors_toolchain_file: Option<bool>,
    error: Option<String>,
    fix: Option<String>,
}

#[derive(Serialize)]
struct BashFlavorReport {
    flavor: &'static str,
    status: &'static str,
    bash_path: Option<String>,
    runs_repo_entrypoints: Option<bool>,
    note: String,
    fix: Option<String>,
}

#[derive(Serialize)]
struct RepoEntrypointsReport {
    marker: &'static str,
    located: bool,
    complete: Option<bool>,
    note: String,
}

#[derive(Serialize)]
struct PerlIdentityReport {
    status: &'static str,
    path: Option<String>,
    version: Option<String>,
    identity: &'static str,
    other_identities: Vec<&'static str>,
    error: Option<String>,
    fix: Option<String>,
}

fn build_dev_environment_report() -> DevEnvironmentReport {
    build_dev_environment_report_in(&std::env::temp_dir())
}

/// Dependency seam for tests: `temp_base` receives the temporary symlink.
fn build_dev_environment_report_in(temp_base: &Path) -> DevEnvironmentReport {
    let windows_host = cfg!(windows);
    DevEnvironmentReport {
        schema: DEV_ENVIRONMENT_SCHEMA,
        host_platform: if windows_host { HOST_PLATFORM_WINDOWS } else { HOST_PLATFORM_UNIX },
        workspace_rust_version: WORKSPACE_RUST_VERSION_LABEL,
        toolchain_channel_pin: TOOLCHAIN_CHANNEL_LABEL,
        symlink_privilege: probe_symlink_privilege_in(temp_base),
        cargo_toolchains: build_cargo_toolchain_reports(windows_host),
        bash_flavors: build_bash_flavor_reports(),
        repo_entrypoints: build_repo_entrypoints_report(),
        documented_prerequisite: BASH_PREREQUISITE_LINE,
        perl_identity: probe_perl_identity(windows_host),
    }
}

fn build_cargo_toolchain_reports(windows_host: bool) -> Vec<CargoToolchainReport> {
    let mut reports = vec![probe_native_cargo()];
    if windows_host {
        reports.push(probe_git_bash_cargo());
        reports.push(probe_wsl_cargo());
    } else {
        // The flavor table keeps its shape on every host so JSON consumers
        // can rely on one row per flavor; Git Bash and WSL simply do not
        // apply outside Windows.
        reports.push(not_applicable_cargo_report(FLAVOR_GIT_BASH));
        reports.push(not_applicable_cargo_report(FLAVOR_WSL));
    }
    reports
}

fn not_applicable_cargo_report(flavor: &'static str) -> CargoToolchainReport {
    CargoToolchainReport {
        flavor,
        status: STATUS_NOT_APPLICABLE,
        path: None,
        version: None,
        provenance: PROVENANCE_UNKNOWN,
        meets_workspace_pin: None,
        honors_toolchain_file: None,
        error: None,
        fix: None,
    }
}

fn build_bash_flavor_reports() -> Vec<BashFlavorReport> {
    let mut reports = vec![native_shell_bash_report()];
    if cfg!(windows) {
        reports.push(git_bash_flavor_report());
        reports.push(wsl_bash_flavor_report());
    } else {
        // Flavor-table parity with Windows hosts (see
        // `build_cargo_toolchain_reports`): Git Bash and WSL are not
        // applicable flavors on Unix.
        reports.push(not_applicable_bash_flavor_report(FLAVOR_GIT_BASH));
        reports.push(not_applicable_bash_flavor_report(FLAVOR_WSL));
    }
    reports
}

fn not_applicable_bash_flavor_report(flavor: &'static str) -> BashFlavorReport {
    BashFlavorReport {
        flavor,
        status: STATUS_NOT_APPLICABLE,
        bash_path: None,
        runs_repo_entrypoints: None,
        note: "not an applicable flavor on this host".to_string(),
        fix: None,
    }
}

fn build_repo_entrypoints_report() -> RepoEntrypointsReport {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match locate_repo_root(&cwd) {
        None => RepoEntrypointsReport {
            marker: REPO_ENTRYPOINT_MARKER,
            located: false,
            complete: None,
            note: format!(
                "no {} found walking up from the working directory",
                REPO_ENTRYPOINT_MARKER
            ),
        },
        Some(root) => RepoEntrypointsReport {
            marker: REPO_ENTRYPOINT_MARKER,
            located: true,
            complete: Some(
                REPO_BASH_ENTRYPOINTS.iter().all(|relative| root.join(relative).exists()),
            ),
            note: format!("repository root: {}", root.display()),
        },
    }
}

/// Walk up from `start` until the repository entrypoint marker appears, with
/// a bounded depth so doctor cannot wander through unbounded parents.
fn locate_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    for _ in 0..=MAX_REPO_ROOT_WALK_DEPTH {
        if current.join(REPO_ENTRYPOINT_MARKER).is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
    None
}

// ── Symlink privilege probe (#12567) ────────────────────────────────────────

fn probe_symlink_privilege_in(base_dir: &Path) -> SymlinkPrivilegeReport {
    #[cfg(windows)]
    {
        probe_symlink_privilege_windows(base_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = base_dir;
        SymlinkPrivilegeReport {
            status: STATUS_NOT_APPLICABLE,
            detail: "file-symlink creation is unrestricted on this platform".to_string(),
            fix: None,
        }
    }
}

#[cfg(windows)]
fn probe_symlink_privilege_windows(base_dir: &Path) -> SymlinkPrivilegeReport {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let stem = format!("perllsp-doctor-symlink-{}-{nonce}", std::process::id());
    let target = base_dir.join(format!("{stem}.txt"));
    let link = base_dir.join(format!("{stem}.ln"));
    run_file_symlink_probe(&target, &link)
}

/// Create `link` pointing at `target`, classify the privilege outcome, then
/// always remove both files again. Detail messages carry only the OS error
/// code, never the probe paths (#7212 privacy posture).
#[cfg(windows)]
fn run_file_symlink_probe(target: &Path, link: &Path) -> SymlinkPrivilegeReport {
    let outcome = std::fs::write(target, b"perllsp doctor symlink-privilege probe (#12595)")
        .and_then(|()| std::os::windows::fs::symlink_file(target, link));
    let report = match outcome {
        Ok(()) => SymlinkPrivilegeReport {
            status: STATUS_PRESENT,
            detail: "created and removed a temporary file symlink".to_string(),
            fix: None,
        },
        Err(error) if error.raw_os_error() == Some(WINDOWS_ERROR_PRIVILEGE_NOT_HELD) => {
            SymlinkPrivilegeReport {
                status: STATUS_MISSING,
                detail: format!(
                    "creating a file symlink failed with os error \
                     {WINDOWS_ERROR_PRIVILEGE_NOT_HELD} (required privilege is not held)"
                ),
                fix: Some(FIX_SYMLINK_PRIVILEGE.to_string()),
            }
        }
        Err(error) => {
            let code = error
                .raw_os_error()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            SymlinkPrivilegeReport {
                status: STATUS_PROBE_ERROR,
                detail: format!("symlink probe failed with os error {code}"),
                fix: None,
            }
        }
    };
    let _ = std::fs::remove_file(link);
    let _ = std::fs::remove_file(target);
    report
}

// ── Cargo per shell flavor ─────────────────────────────────────────────────

/// How a reachable cargo was installed, decided purely from its resolved
/// path: rustup shims live under `.cargo/bin`/`.rustup` and honor
/// rust-toolchain.toml; anything else (apt/distro cargo) ignores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CargoProvenance {
    RustupShim,
    NonRustup,
}

impl CargoProvenance {
    const fn code(self) -> &'static str {
        match self {
            Self::RustupShim => PROVENANCE_RUSTUP_SHIM,
            Self::NonRustup => PROVENANCE_NON_RUSTUP,
        }
    }
}

fn probe_native_cargo() -> CargoToolchainReport {
    let Some(binary) = resolve_tool_on_path("cargo") else {
        return unreachable_cargo_report(FLAVOR_NATIVE_SHELL, "cargo not found on PATH");
    };
    let mut command = Command::new(&binary);
    command.arg("--version");
    cargo_report_from_output(
        FLAVOR_NATIVE_SHELL,
        binary,
        run_command_with_timeout(command, DEV_ENV_PROBE_TIMEOUT_SECS),
    )
}

fn probe_git_bash_cargo() -> CargoToolchainReport {
    let Some(bash_exe) = resolve_tool_on_path("bash") else {
        return unreachable_cargo_report(
            FLAVOR_GIT_BASH,
            "no bash.exe on PATH (Git Bash not detected)",
        );
    };
    if classify_windows_bash_path(&bash_exe.to_string_lossy()) == WindowsBashKind::WslSystem32 {
        return unreachable_cargo_report(
            FLAVOR_GIT_BASH,
            "PATH bash.exe resolves to the WSL System32 shim, not Git Bash",
        );
    }
    let mut command = Command::new(&bash_exe);
    command.args(["-c", SHELL_CARGO_PROBE_SCRIPT]);
    shell_cargo_report_from_output(
        FLAVOR_GIT_BASH,
        run_command_with_timeout(command, DEV_ENV_PROBE_TIMEOUT_SECS),
    )
}

fn probe_wsl_cargo() -> CargoToolchainReport {
    let Some(wsl_exe) = resolve_tool_on_path("wsl") else {
        return unreachable_cargo_report(FLAVOR_WSL, "wsl.exe not found on PATH");
    };
    let mut command = Command::new(&wsl_exe);
    command.args(["bash", "-c", SHELL_CARGO_PROBE_SCRIPT]);
    shell_cargo_report_from_output(
        FLAVOR_WSL,
        run_command_with_timeout(command, DEV_ENV_WSL_TIMEOUT_SECS),
    )
}

const SHELL_CARGO_PROBE_SCRIPT: &str = "command -v cargo && cargo --version";

/// Parse the first `major.minor.patch` word of a `cargo --version` line such
/// as `cargo 1.95.0 (8f3d0b0ac 2026-01-30)`; distro builds append their own
/// suffixes after the patch number.
fn parse_cargo_version_line(_line: &str) -> Option<VersionTriple> {
    None // scaffolding placeholder (#12595 red-first); replaced by the probing implementation
}

/// Scaffolding placeholder (#12595 red-first): assumes every reachable cargo
/// is a healthy rustup shim until the probing implementation lands.
fn classify_cargo_provenance(_cargo_path: &str) -> CargoProvenance {
    CargoProvenance::RustupShim
}

/// Which provider owns a Windows `bash.exe` resolution: System32 hosts the
/// WSL shim; Git/MSYS/Cygwin provide native POSIX bashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsBashKind {
    PosixProvider,
    WslSystem32,
    OtherProvider,
}

/// Scaffolding placeholder (#12595 red-first): classifies every bash as an
/// unknown provider until the probing implementation lands.
fn classify_windows_bash_path(_path_text: &str) -> WindowsBashKind {
    WindowsBashKind::OtherProvider
}

/// Decode child-process output. `wsl.exe` emits UTF-16LE through pipes while
/// ordinary children stay UTF-8, so both shapes must land as readable text.
fn decode_shell_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_owned()
}

/// Extract the `v5.42.2`-style token from a `perl --version` banner.
fn extract_perl_version(_version_output: &str) -> Option<String> {
    None // scaffolding placeholder (#12595 red-first)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PerlIdentityKind {
    MsysCygwin,
    Strawberry,
    SystemUnix,
    Unknown,
}

impl PerlIdentityKind {
    const fn code(self) -> &'static str {
        match self {
            Self::MsysCygwin => IDENTITY_MSYS_CYGWIN,
            Self::Strawberry => IDENTITY_STRAWBERRY,
            Self::SystemUnix => IDENTITY_SYSTEM_UNIX,
            Self::Unknown => IDENTITY_UNKNOWN,
        }
    }
}

/// Scaffolding placeholder (#12595 red-first): classifies every perl as an
/// unknown identity until the probing implementation lands.
fn classify_perl_identity(_path_text: &str, _windows_host: bool) -> PerlIdentityKind {
    PerlIdentityKind::Unknown
}

/// Distinct additional named perl identities among `discovered`, excluding
/// the primary kind and unnamed resolutions.
fn other_named_identities(
    _primary: PerlIdentityKind,
    _discovered: &[PerlIdentityKind],
) -> Vec<&'static str> {
    Vec::new() // scaffolding placeholder (#12595 red-first)
}

/// Fixed, well-known default install locations probed for additional perl
/// identities (#12595: common locations only — never a filesystem scan).
fn common_perl_candidate_paths(windows_host: bool) -> Vec<PathBuf> {
    if windows_host {
        vec![
            PathBuf::from("C:\\Strawberry\\perl\\bin\\perl.exe"),
            PathBuf::from("C:\\msys64\\usr\\bin\\perl.exe"),
            PathBuf::from("C:\\cygwin64\\usr\\bin\\perl.exe"),
        ]
    } else {
        vec![PathBuf::from("/usr/bin/perl")]
    }
}

fn probe_perl_identity(windows_host: bool) -> PerlIdentityReport {
    let Some(binary) = resolve_tool_on_path("perl") else {
        return PerlIdentityReport {
            status: STATUS_MISSING,
            path: None,
            version: None,
            identity: IDENTITY_UNKNOWN,
            other_identities: Vec::new(),
            error: Some("perl not found on PATH".to_string()),
            fix: Some(if windows_host { FIX_PERL_MISSING_WINDOWS } else { FIX_PERL_MISSING_UNIX }.to_string()),
        };
    };

    let mut command = Command::new(&binary);
    command.arg("--version");
    let (version, error) = match run_command_with_timeout(command, DEV_ENV_PROBE_TIMEOUT_SECS) {
        Ok(output) if output.status.success() => {
            (extract_perl_version(&decode_shell_output(&output.stdout)), None)
        }
        Ok(output) => (
            None,
            Some(truncate_for_detail(decode_shell_output(&output.stderr).trim(), DETAIL_MAX_CHARS)),
        ),
        Err(spawn_error) => (None, Some(truncate_for_detail(&spawn_error, DETAIL_MAX_CHARS))),
    };

    let path_text = binary.display().to_string();
    let identity = classify_perl_identity(&path_text, windows_host);
    let discovered: Vec<PerlIdentityKind> = common_perl_candidate_paths(windows_host)
        .into_iter()
        .filter(|candidate| !same_install_location(candidate, &binary))
        .filter(|candidate| candidate.exists())
        .map(|candidate| classify_perl_identity(&candidate.display().to_string(), windows_host))
        .collect();
    let other_identities = other_named_identities(identity, &discovered);
    let status =
        if other_identities.is_empty() { STATUS_PRESENT } else { STATUS_DIVERGENT };

    PerlIdentityReport {
        status,
        path: Some(path_text),
        version,
        identity: identity.code(),
        other_identities,
        error,
        fix: (status == STATUS_DIVERGENT).then(|| FIX_PERL_IDENTITY_DIVERGENCE.to_string()),
    }
}

fn same_install_location(left: &Path, right: &Path) -> bool {
    left.display().to_string().to_lowercase() == right.display().to_string().to_lowercase()
}

/// Decide the typed verdict for a reachable cargo: non-rustup wins over
/// staleness because it explains why the pin is ignored; a stale rustup
/// toolchain gets the alignment fix. Returns the status plus whether the
/// version meets the workspace MSRV (`None` when unparsable).
fn reachable_cargo_status(
    provenance: Option<CargoProvenance>,
    version: Option<VersionTriple>,
) -> (&'static str, Option<bool>) {
    let below_pin = version.map(version_below_workspace_pin);
    match (provenance, below_pin) {
        (Some(CargoProvenance::NonRustup), _) => (STATUS_NON_RUSTUP, below_pin.map(|below| !below)),
        (_, Some(true)) => (STATUS_STALE, Some(false)),
        _ => (STATUS_PRESENT, below_pin.map(|below| !below)),
    }
}

/// True when `major.minor` sits below the workspace MSRV; patch level never
/// rescues an older minor nor sinks the pinned minor.
fn version_below_workspace_pin(version: VersionTriple) -> bool {
    (version.0, version.1) < (WORKSPACE_RUST_VERSION_MAJOR, WORKSPACE_RUST_VERSION_MINOR)
}

/// One copyable fix line per failing shape (#7212: guidance only, never
/// auto-executed, scoped to the failing environment).
fn cargo_fix_line(flavor: &str, status: &str) -> Option<String> {
    match status {
        STATUS_NON_RUSTUP => Some(match flavor {
            FLAVOR_WSL => format!("fix: inside WSL run: {RUSTUP_INSTALL_ONE_LINER}"),
            _ => format!("fix: in this shell run: {RUSTUP_INSTALL_ONE_LINER}"),
        }),
        STATUS_STALE => Some(format!(
            "fix: align this shell's toolchain with rust-toolchain.toml \
             ({TOOLCHAIN_CHANNEL_LABEL}): rustup toolchain install {TOOLCHAIN_CHANNEL_LABEL}"
        )),
        STATUS_MISSING => Some(match flavor {
            FLAVOR_GIT_BASH => FIX_BASH_INSTALL_GIT_WINDOWS.to_string(),
            FLAVOR_WSL => format!(
                "fix: WSL answered but has no usable cargo; inside WSL run: \
                 {RUSTUP_INSTALL_ONE_LINER}"
            ),
            _ => format!("fix: install rustup (https://rustup.rs): {RUSTUP_INSTALL_ONE_LINER}"),
        }),
        _ => None,
    }
}

fn cargo_report_from_output(
    flavor: &'static str,
    binary: PathBuf,
    output: ProbeOutput,
) -> CargoToolchainReport {
    match output {
        Ok(process_output) if process_output.status.success() => {
            finish_reachable_cargo_report(flavor, Some(binary), &decode_shell_output(&process_output.stdout))
        }
        Ok(_) | Err(_) => failed_probe_cargo_report(flavor, output),
    }
}

/// Shell-mediated probe result (`command -v cargo && cargo --version`):
/// the first output line is the resolved cargo path, the rest is its banner.
fn shell_cargo_report_from_output(flavor: &'static str, output: ProbeOutput) -> CargoToolchainReport {
    match output {
        Ok(process_output) if process_output.status.success() => {
            let decoded = decode_shell_output(&process_output.stdout);
            let mut lines = decoded
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty());
            let cargo_path = lines.next().map(PathBuf::from);
            let version_text = lines.collect::<Vec<_>>().join(" ");
            finish_reachable_cargo_report(flavor, cargo_path, &version_text)
        }
        Ok(_) | Err(_) => failed_probe_cargo_report(flavor, output),
    }
}

fn failed_probe_cargo_report(flavor: &'static str, output: ProbeOutput) -> CargoToolchainReport {
    let detail = match output {
        Ok(process_output) => truncate_for_detail(
            &format!(
                "probe exited with status {}; stderr: {}",
                process_output.status,
                decode_shell_output(&process_output.stderr).trim()
            ),
            DETAIL_MAX_CHARS,
        ),
        Err(spawn_error) => truncate_for_detail(&spawn_error, DETAIL_MAX_CHARS),
    };
    unreachable_cargo_report(flavor, &detail)
}

fn finish_reachable_cargo_report(
    flavor: &'static str,
    binary: Option<PathBuf>,
    version_output: &str,
) -> CargoToolchainReport {
    let version_line = version_output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(String::from);
    let parsed_version = version_line.as_deref().and_then(parse_cargo_version_line);
    let provenance =
        binary.as_deref().and_then(Path::to_str).map(classify_cargo_provenance);
    let (status, meets_workspace_pin) = reachable_cargo_status(provenance, parsed_version);
    CargoToolchainReport {
        flavor,
        status,
        path: binary.as_ref().map(|path| path.display().to_string()),
        version: version_line,
        provenance: provenance.map_or(PROVENANCE_UNKNOWN, CargoProvenance::code),
        meets_workspace_pin,
        honors_toolchain_file: provenance
            .map(|provenance| provenance == CargoProvenance::RustupShim),
        error: None,
        fix: cargo_fix_line(flavor, status),
    }
}

fn unreachable_cargo_report(flavor: &'static str, detail: &str) -> CargoToolchainReport {
    CargoToolchainReport {
        flavor,
        status: STATUS_MISSING,
        path: None,
        version: None,
        provenance: PROVENANCE_UNKNOWN,
        meets_workspace_pin: None,
        honors_toolchain_file: None,
        error: Some(detail.to_string()),
        fix: cargo_fix_line(flavor, STATUS_MISSING),
    }
}

// ── Bash flavors and repository entrypoints ────────────────────────────────

fn native_shell_bash_report() -> BashFlavorReport {
    if cfg!(windows) {
        BashFlavorReport {
            flavor: FLAVOR_NATIVE_SHELL,
            status: STATUS_PRESENT,
            bash_path: None,
            runs_repo_entrypoints: Some(false),
            note: "PowerShell/cmd cannot execute .sh entrypoints; use Git Bash or WSL".to_string(),
            fix: None,
        }
    } else {
        BashFlavorReport {
            flavor: FLAVOR_NATIVE_SHELL,
            status: STATUS_PRESENT,
            bash_path: None,
            runs_repo_entrypoints: Some(true),
            note: "the POSIX login shell runs repository .sh entrypoints directly".to_string(),
            fix: None,
        }
    }
}

fn git_bash_flavor_report() -> BashFlavorReport {
    match resolve_tool_on_path("bash") {
        None => BashFlavorReport {
            flavor: FLAVOR_GIT_BASH,
            status: STATUS_MISSING,
            bash_path: None,
            runs_repo_entrypoints: None,
            note: "no bash.exe on PATH; repository .sh entrypoints cannot run natively"
                .to_string(),
            fix: Some(FIX_BASH_INSTALL_GIT_WINDOWS.to_string()),
        },
        Some(bash_exe)
            if classify_windows_bash_path(&bash_exe.to_string_lossy())
                == WindowsBashKind::WslSystem32 =>
        {
            BashFlavorReport {
                flavor: FLAVOR_GIT_BASH,
                status: STATUS_MISSING,
                bash_path: Some(bash_exe.display().to_string()),
                runs_repo_entrypoints: None,
                note: "PATH bash.exe is the WSL System32 shim, not Git Bash".to_string(),
                fix: Some(FIX_BASH_INSTALL_GIT_WINDOWS.to_string()),
            }
        }
        Some(bash_exe) => BashFlavorReport {
            flavor: FLAVOR_GIT_BASH,
            status: STATUS_PRESENT,
            bash_path: Some(bash_exe.display().to_string()),
            runs_repo_entrypoints: Some(true),
            note: "repository .sh entrypoints run under this native POSIX bash".to_string(),
            fix: None,
        },
    }
}

fn wsl_bash_flavor_report() -> BashFlavorReport {
    let Some(wsl_exe) = resolve_tool_on_path("wsl") else {
        return BashFlavorReport {
            flavor: FLAVOR_WSL,
            status: STATUS_MISSING,
            bash_path: None,
            runs_repo_entrypoints: None,
            note: "wsl.exe not found on PATH".to_string(),
            fix: None,
        };
    };
    let mut command = Command::new(&wsl_exe);
    command.arg("--status");
    match run_command_with_timeout(command, DEV_ENV_PROBE_TIMEOUT_SECS) {
        Ok(output) if output.status.success() => BashFlavorReport {
            flavor: FLAVOR_WSL,
            status: STATUS_PRESENT,
            bash_path: None,
            runs_repo_entrypoints: Some(true),
            note: "entrypoints would run against the WSL filesystem and toolchains; compare the wsl cargo flavor"
                .to_string(),
            fix: None,
        },
        Ok(output) => BashFlavorReport {
            flavor: FLAVOR_WSL,
            status: STATUS_MISSING,
            bash_path: None,
            runs_repo_entrypoints: None,
            note: truncate_for_detail(
                &format!("wsl.exe --status failed: {}", decode_shell_output(&output.stderr).trim()),
                DETAIL_MAX_CHARS,
            ),
            fix: None,
        },
        Err(timeout_error) => BashFlavorReport {
            flavor: FLAVOR_WSL,
            status: STATUS_MISSING,
            bash_path: None,
            runs_repo_entrypoints: None,
            note: truncate_for_detail(&timeout_error, DETAIL_MAX_CHARS),
            fix: None,
        },
    }
}

// ── Rendering ───────────────────────────────────────────────────────────────

fn render_dev_environment_report(report: &DevEnvironmentReport) -> String {
    let mut out = String::new();
    out.push_str("perl-lsp doctor - development environment\n");
    out.push_str("=========================================\n\n");
    out.push_str(&format!(
        "Pins: workspace rust-version {}, rust-toolchain.toml channel {}\n",
        report.workspace_rust_version, report.toolchain_channel_pin
    ));

    out.push_str(&format!("\nSymlink privilege: {}\n", report.symlink_privilege.status));
    out.push_str(&format!("  {}\n", report.symlink_privilege.detail));
    if let Some(fix) = &report.symlink_privilege.fix {
        out.push_str(&format!("  Fix: {fix}\n"));
    }

    out.push_str("\nCargo per shell flavor:\n");
    for cargo in &report.cargo_toolchains {
        out.push_str(&format!("  - {}: {}", cargo.flavor, cargo.status));
        if let Some(path) = &cargo.path {
            out.push_str(&format!(" | {path}"));
        }
        if let Some(version) = &cargo.version {
            out.push_str(&format!(" | {version}"));
        }
        out.push('\n');
        out.push_str(&format!("      provenance: {}\n", cargo.provenance));
        out.push_str(&format!(
            "      meets workspace pin ({}): {}\n",
            report.workspace_rust_version,
            render_optional_bool(cargo.meets_workspace_pin)
        ));
        if let Some(error) = &cargo.error {
            out.push_str(&format!("      error: {error}\n"));
        }
        if let Some(fix) = &cargo.fix {
            out.push_str(&format!("      Fix: {fix}\n"));
        }
    }

    out.push_str("\nBash flavors:\n");
    for flavor in &report.bash_flavors {
        out.push_str(&format!("  - {}: {}", flavor.flavor, flavor.status));
        if let Some(path) = &flavor.bash_path {
            out.push_str(&format!(" | {path}"));
        }
        if let Some(runs) = flavor.runs_repo_entrypoints {
            out.push_str(&format!(
                " | runs repo entrypoints: {}",
                render_optional_bool(Some(runs))
            ));
        }
        out.push('\n');
        out.push_str(&format!("      {}\n", flavor.note));
        if let Some(fix) = &flavor.fix {
            out.push_str(&format!("      Fix: {fix}\n"));
        }
    }

    out.push_str("\nRepository bash entrypoints:\n");
    out.push_str(&format!(
        "  located via {}: {}\n",
        report.repo_entrypoints.marker, report.repo_entrypoints.located
    ));
    if let Some(complete) = report.repo_entrypoints.complete {
        out.push_str(&format!("      all documented entrypoints present: {complete}\n"));
    }
    out.push_str(&format!("      {}\n", report.repo_entrypoints.note));
    out.push_str(&format!("\nPrerequisite: {}\n", report.documented_prerequisite));

    out.push_str("\nPerl identity:\n");
    match &report.perl_identity.path {
        Some(path) => {
            out.push_str("  resolved: ");
            out.push_str(path);
            if let Some(version) = &report.perl_identity.version {
                out.push_str(&format!(" ({version})"));
            }
            out.push_str(&format!(" [identity: {}]\n", report.perl_identity.identity));
        }
        None => out.push_str("  resolved: none\n"),
    }
    if !report.perl_identity.other_identities.is_empty() {
        out.push_str(&format!(
            "  WARNING: additional distinct identities also discoverable: {}; DAP E2E\n  and prove consumers may pick a different perl.\n",
            report.perl_identity.other_identities.join(", ")
        ));
    }
    if let Some(error) = &report.perl_identity.error {
        out.push_str(&format!("  error: {error}\n"));
    }
    if let Some(fix) = &report.perl_identity.fix {
        out.push_str(&format!("  Fix: {fix}\n"));
    }

    out.push_str("\nClaim boundary:\n");
    out.push_str(
        "  Read-only probes. Doctor creates and removes one temporary symlink, asks\n",
    );
    out.push_str(
        "  reachable shells for cargo/perl versions, and never installs, moves, or\n",
    );
    out.push_str("  configures anything.\n");
    out
}

fn render_optional_bool(value: Option<bool>) -> String {
    match value {
        Some(true) => "yes".to_string(),
        Some(false) => "no".to_string(),
        None => "unknown".to_string(),
    }
}

fn truncate_for_detail(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(max_chars).collect::<String>())
    }
}

fn render_report(report: DoctorReport) -> String {
    let mut out = String::new();
    out.push_str("perl-lsp doctor\n");
    out.push_str("===============\n\n");
    out.push_str(&format!("Workspace: {}\n", report.workspace.display()));
    out.push_str(&format!("Project config: {}\n", render_project_config_status(&report.config)));
    if !report.config.rejected_include_paths.is_empty() {
        out.push_str("Rejected .perl-lsp.toml include_paths entries:\n");
        for rejected in &report.config.rejected_include_paths {
            out.push_str(&format!("  - {rejected}\n"));
        }
    }
    out.push_str(&format!("Perl: {}\n", render_perl_binary(&report.perl)));
    out.push_str(&format!("Perl version: {}\n", render_perl_version(&report.perl)));
    out.push_str(&format!("perltidy: {}\n", render_tool_report(&report.perltidy)));
    out.push_str(&format!("perlcritic: {}\n", render_tool_report(&report.perlcritic)));
    out.push_str(&format!(
        "PERL5LIB: {}\n",
        render_perl5lib_status(report.perl5lib_enabled, report.perl5lib_paths.len())
    ));
    out.push_str(&format!(
        "PERL5LIB precedence: {}\n\n",
        render_perl5lib_precedence(&report.perl5lib_precedence)
    ));

    out.push_str("Configured includePaths:\n");
    render_path_reports(&mut out, &report.configured_paths);
    out.push('\n');

    out.push_str("Effective @INC roots:\n");
    render_path_reports(&mut out, &report.effective_paths);
    out.push('\n');

    out.push_str(&format!("System @INC: {}\n", report.system_inc.status));
    if !report.system_inc.paths.is_empty() {
        for path in &report.system_inc.paths {
            out.push_str(&format!("  - {}\n", path.display()));
        }
    }
    out.push('\n');

    out.push_str("Module lookup example:\n");
    out.push_str(
        "  use Foo::Bar; searches Foo/Bar.pm under the effective roots above, in order.\n\n",
    );
    out.push_str("Next steps:\n");
    out.push_str("  - Add missing project module roots to .perl-lsp.toml [perl].include_paths.\n");
    out.push_str(
        "  - Set PERL5LIB or use_perl5lib intentionally; doctor reports whether it participates.\n",
    );
    out.push_str(
        "  - Fix roots marked unsafe; module resolution ignores relative roots that escape the workspace.\n",
    );
    if report.perltidy.binary.is_none() {
        out.push_str(
            "  - Install perltidy (cpanm Perl::Tidy) so external formatting via perltidy works; the native formatter does not require it.\n",
        );
    }
    if report.perlcritic.binary.is_none() {
        out.push_str(
            "  - Install perlcritic (cpanm Perl::Critic) so external critic-based diagnostics work; the native critic engine does not require it.\n",
        );
    }
    out.push_str(
        "  - Editor-only settings may still override this CLI report after initialization.\n\n",
    );
    out.push_str("Claim boundary:\n");
    out.push_str(
        "  Read-only CLI report. It does not start the LSP, mutate config, scan the workspace, or apply editor-specific settings.\n",
    );
    out
}

fn render_project_config_status(config: &ProjectConfigReport) -> &'static str {
    match config.status {
        ProjectConfigStatus::Loaded => "loaded .perl-lsp.toml",
        ProjectConfigStatus::Missing => "not found, using defaults",
    }
}

fn render_perl_binary(report: &PerlReport) -> String {
    match &report.binary {
        Some(path) => format!("{} ({})", path.display(), report.source),
        None => format!("not found ({})", report.source),
    }
}

fn render_perl_version(report: &PerlReport) -> String {
    if let Some(version) = report.version.as_deref().filter(|version| !version.is_empty()) {
        version.to_string()
    } else if let Some(error) = &report.error {
        format!("not available: {error}")
    } else {
        "not available".to_string()
    }
}

fn render_tool_report(report: &ToolReport) -> String {
    match &report.binary {
        Some(path) => {
            let version = report
                .version
                .as_deref()
                .filter(|version| !version.is_empty())
                .map(|version| version.to_string())
                .or_else(|| {
                    report.error.as_deref().map(|error| format!("version unavailable: {error}"))
                })
                .unwrap_or_else(|| "version unavailable".to_string());
            format!("{} ({}); {}", path.display(), report.source, version)
        }
        None => {
            let error = report
                .error
                .as_deref()
                .filter(|error| !error.is_empty())
                .unwrap_or("not found on PATH");
            format!("not found ({}); {error}", report.source)
        }
    }
}

fn render_perl5lib_status(enabled: bool, count: usize) -> String {
    if count == 0 {
        return "environment empty".to_string();
    }

    if enabled {
        format!("enabled, {count} environment path(s) participate")
    } else {
        format!("disabled by config, {count} environment path(s) ignored")
    }
}

fn render_perl5lib_precedence(precedence: &Perl5LibPrecedence) -> &'static str {
    match precedence {
        Perl5LibPrecedence::Prepend => "prepend",
        Perl5LibPrecedence::Append => "append",
    }
}

fn render_path_reports(out: &mut String, reports: &[PathReport]) {
    if reports.is_empty() {
        out.push_str("  (none)\n");
        return;
    }

    for report in reports {
        out.push_str(&format!(
            "  - {} ({}, {}; raw: {})\n",
            report.resolved.display(),
            report.source,
            report.status,
            report.raw
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn normalized_path_text_collapses_current_dir() {
        assert_eq!(normalized_path_text("./lib/"), "lib");
        assert_eq!(normalized_path_text("."), ".");
    }

    #[test]
    fn normalized_path_text_empty_boundary() {
        assert_eq!(normalized_path_text(""), "");
        assert_eq!(normalized_path_text("   "), "");
    }

    #[test]
    fn path_report_marks_missing_existing_and_non_directory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let lib = temp.path().join("lib");
        let file = temp.path().join("not-lib");
        std::fs::create_dir_all(&lib)?;
        std::fs::write(&file, "not a directory")?;

        assert_eq!(path_report(temp.path(), "lib", "test").status, "exists");
        assert_eq!(path_report(temp.path(), "missing", "test").status, "missing");
        assert_eq!(path_report(temp.path(), "not-lib", "test").status, "not a directory");
        Ok(())
    }

    #[test]
    fn path_report_keeps_relative_perl5lib_roots_current_cwd_relative() -> TestResult {
        let workspace = tempfile::tempdir()?;
        let current_dir = std::env::current_dir()?;
        let raw = format!("perl-lsp-doctor-missing-perl5lib-{}", std::process::id());

        let report = path_report(workspace.path(), &raw, PERL5LIB_SOURCE);

        assert_eq!(report.raw, raw);
        assert_eq!(report.source, PERL5LIB_SOURCE);
        assert_eq!(report.resolved, current_dir.join(&report.raw));
        assert_eq!(report.status, "missing");
        Ok(())
    }

    #[test]
    fn resolve_path_boundary_discriminator_input_that_hits_the_boundary_source_equals_perl5lib_source()
    -> TestResult {
        let workspace = tempfile::tempdir()?;
        let current_dir = std::env::current_dir()?;
        let raw = "relative-perl5lib-root";

        assert_eq!(
            resolve_path(workspace.path(), raw, "PERL5LIB"),
            current_dir.join(raw),
            "input that hits the boundary: source == PERL5LIB_SOURCE"
        );
        Ok(())
    }

    #[test]
    fn is_perl5lib_source_boundary_discriminator_input_that_hits_the_boundary_source_equals_perl5lib_source()
     {
        assert!(
            is_perl5lib_source("PERL5LIB"),
            "input that hits the boundary: source == PERL5LIB_SOURCE"
        );
    }

    #[test]
    fn is_perl5lib_source_boundary_discriminator_source_not_equals_perl5lib_source() {
        assert!(
            !is_perl5lib_source("test"),
            "input that hits the boundary: source != PERL5LIB_SOURCE"
        );
    }

    #[test]
    fn path_status_boundary_discriminator_input_that_hits_the_boundary_source_equals_perl5lib_source()
    -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let shared = temp.path().join("shared").join("lib");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&shared)?;
        let raw = "../shared/lib";

        assert_eq!(
            path_status(&workspace, raw, &shared, "PERL5LIB"),
            "exists",
            "input that hits the boundary: source == PERL5LIB_SOURCE"
        );
        Ok(())
    }

    #[test]
    fn path_status_boundary_discriminator_input_that_hits_the_boundary_source_not_equals_perl5lib_source()
    -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let shared = temp.path().join("shared").join("lib");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&shared)?;
        let raw = "../shared/lib";
        let resolved = workspace.join(raw);

        assert_eq!(
            path_status(&workspace, raw, &resolved, "test"),
            "unsafe: outside workspace",
            "input that hits the boundary: source != PERL5LIB_SOURCE"
        );
        Ok(())
    }

    #[test]
    fn path_report_marks_workspace_escaping_relative_roots_unsafe() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let shared = temp.path().join("shared").join("lib");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&shared)?;

        let report = path_report(&workspace, "../shared/lib", "test");

        assert_eq!(report.status, "unsafe: outside workspace");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn path_report_marks_workspace_escaping_symlink_roots_unsafe() -> TestResult {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().join("workspace");
        let shared = temp.path().join("shared").join("lib");
        let link = workspace.join("linked-lib");
        std::fs::create_dir_all(&workspace)?;
        std::fs::create_dir_all(&shared)?;
        std::os::unix::fs::symlink(&shared, &link)?;

        let report = path_report(&workspace, "linked-lib", "test");

        assert_eq!(report.status, "unsafe: outside workspace");
        Ok(())
    }

    #[test]
    fn workspace_dir_reports_missing_directory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing-workspace");
        let missing_dir = missing.to_str().ok_or("non-UTF-8 temp path")?;

        let error = workspace_dir(missing_dir).err().ok_or("missing directory should fail")?;

        assert_eq!(error, format!("{missing_dir}: directory not found"));
        Ok(())
    }

    #[test]
    fn workspace_dir_reports_file_path() -> TestResult {
        let temp = tempfile::tempdir()?;
        let file = temp.path().join("workspace-file.pl");
        std::fs::write(&file, "use strict;\n")?;
        let file_path = file.to_str().ok_or("non-UTF-8 temp path")?;

        let error = workspace_dir(file_path).err().ok_or("file path should fail")?;

        assert_eq!(error, format!("{file_path}: not a directory"));
        Ok(())
    }

    #[test]
    fn workspace_dir_exact_error_variant() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing-workspace");
        let missing_dir = missing.to_str().ok_or("non-UTF-8 temp path")?;
        assert!(matches!(
            workspace_dir(missing_dir),
            Err(error) if error == format!("{missing_dir}: directory not found")
        ));

        let file = temp.path().join("workspace-file.pl");
        std::fs::write(&file, "use strict;\n")?;
        let file_path = file.to_str().ok_or("non-UTF-8 temp path")?;
        assert!(matches!(
            workspace_dir(file_path),
            Err(error) if error == format!("{file_path}: not a directory")
        ));

        let unresolvable = temp.path().join("missing-canonicalize");
        let unresolvable_dir = unresolvable.to_str().ok_or("non-UTF-8 temp path")?;
        assert!(matches!(
            canonicalize_workspace_dir(unresolvable_dir, &unresolvable),
            Err(error)
                if error.starts_with(&format!(
                    "{unresolvable_dir}: cannot resolve directory: "
                ))
        ));
        Ok(())
    }

    #[test]
    fn workspace_dir_boundary_discriminator_input_that_hits_the_boundary_error_kind_equals_std_io_error_kind_not_found()
    -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("definitely-missing-workspace");
        let missing_dir = missing.to_str().ok_or("non-UTF-8 temp path")?;

        let error = missing.metadata().err().ok_or("missing path should not stat")?;
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::NotFound,
            "input that hits the boundary: error.kind() == std::io::ErrorKind::NotFound"
        );
        let workspace_error =
            workspace_dir(missing_dir).err().ok_or("missing workspace should fail")?;

        assert_eq!(workspace_error, format!("{missing_dir}: directory not found"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn workspace_dir_permission_boundary_discriminator() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let locked_parent = temp.path().join("locked");
        std::fs::create_dir_all(&locked_parent)?;
        let child = locked_parent.join("child");
        let child_dir = child.to_str().ok_or("non-UTF-8 temp path")?;
        if current_uid_is_root() {
            assert_eq!(
                workspace_dir(child_dir).err().ok_or("root should see the missing child")?,
                format!("{child_dir}: directory not found")
            );
            return Ok(());
        }

        let original_permissions = std::fs::metadata(&locked_parent)?.permissions();
        let mut locked_permissions = original_permissions.clone();
        locked_permissions.set_mode(0o0);

        std::fs::set_permissions(&locked_parent, locked_permissions)?;
        let result = workspace_dir(child_dir);
        std::fs::set_permissions(&locked_parent, original_permissions)?;

        assert!(matches!(
            result,
            Err(error)
                if error.starts_with(&format!("{child_dir}: cannot access directory: "))
        ));
        Ok(())
    }

    #[cfg(unix)]
    fn current_uid_is_root() -> bool {
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|uid| uid_text_is_root(&uid))
    }

    #[cfg(unix)]
    fn uid_text_is_root(uid: &str) -> bool {
        uid.trim() == "0"
    }

    #[cfg(unix)]
    #[test]
    fn current_uid_is_root_boundary_discriminator_input_that_hits_the_boundary_uid_trim_equals_0() {
        let uid = "0\n";
        assert!(uid_text_is_root(uid), "input that hits the boundary: uid.trim() == \"0\"");
        assert!(!uid_text_is_root("1000\n"));
    }

    #[test]
    fn build_doctor_report_rejects_missing_workspace() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing-workspace");
        let missing_dir = missing.to_str().ok_or("non-UTF-8 temp path")?;

        let error = build_doctor_report_struct(missing_dir)
            .err()
            .ok_or("missing workspace should fail doctor")?;

        assert_eq!(error, format!("{missing_dir}: directory not found"));
        Ok(())
    }

    #[test]
    fn run_doctor_call_presence_observer() -> TestResult {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;

        assert_eq!(run_doctor(dir, false), 0);
        Ok(())
    }

    #[test]
    fn run_doctor_match_arm_discriminator() -> TestResult {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;
        assert_eq!(run_doctor(dir, false), 0);

        let missing = temp.path().join("missing-workspace");
        let missing_dir = missing.to_str().ok_or("non-UTF-8 temp path")?;
        assert_eq!(run_doctor(missing_dir, false), 1);
        Ok(())
    }

    #[test]
    fn effective_root_reports_label_perl5lib_and_configured_roots() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["lib".to_string()];
        config.use_perl5lib = true;

        let reports = effective_root_reports(
            temp.path(),
            &config,
            &["vendor/lib".to_string()],
            ".perl-lsp.toml include_paths",
        );

        let first = reports.first().ok_or("missing PERL5LIB report")?;
        assert_eq!(first.source, "PERL5LIB");
        assert_eq!(first.raw, "vendor/lib");
        assert!(first.resolved.ends_with("vendor/lib"));
        let second = reports.get(1).ok_or("missing configured include report")?;
        assert_eq!(second.source, ".perl-lsp.toml include_paths");
        assert_eq!(second.raw, "lib");
        assert!(second.resolved.ends_with("lib"));
        Ok(())
    }

    #[test]
    fn effective_root_reports_label_overlapping_append_root_as_configured() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["shared/lib".to_string()];
        config.use_perl5lib = true;
        config.perl5lib_precedence = Perl5LibPrecedence::Append;

        let reports = effective_root_reports(
            temp.path(),
            &config,
            &["shared/lib".to_string()],
            ".perl-lsp.toml include_paths",
        );

        assert_eq!(reports.len(), 1);
        let report = reports.first().ok_or("missing effective root report")?;
        assert_eq!(report.source, ".perl-lsp.toml include_paths");
        assert_eq!(report.raw, "shared/lib");
        assert!(report.resolved.ends_with("shared/lib"));
        Ok(())
    }

    #[test]
    fn effective_root_reports_label_overlapping_prepend_root_as_perl5lib() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["shared/lib".to_string()];
        config.use_perl5lib = true;
        config.perl5lib_precedence = Perl5LibPrecedence::Prepend;

        let reports = effective_root_reports(
            temp.path(),
            &config,
            &["shared/lib".to_string()],
            ".perl-lsp.toml include_paths",
        );

        assert_eq!(reports.len(), 1);
        let report = reports.first().ok_or("missing effective root report")?;
        assert_eq!(report.source, "PERL5LIB");
        assert_eq!(report.raw, "shared/lib");
        assert!(report.resolved.ends_with("shared/lib"));
        Ok(())
    }

    #[test]
    fn effective_root_reports_do_not_label_disabled_perl5lib_overlap_as_perl5lib() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["shared/lib".to_string()];
        config.use_perl5lib = false;

        let reports = effective_root_reports(
            temp.path(),
            &config,
            &["shared/lib".to_string()],
            ".perl-lsp.toml include_paths",
        );

        assert_eq!(reports.len(), 1);
        let report = reports.first().ok_or("missing effective root report")?;
        assert_eq!(report.source, ".perl-lsp.toml include_paths");
        assert_eq!(report.raw, "shared/lib");
        assert!(report.resolved.ends_with("shared/lib"));
        Ok(())
    }

    #[test]
    fn effective_root_reports_preserve_raw_contributor_for_normalized_roots() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut config = WorkspaceConfig::default();
        config.include_paths = vec!["./lib/".to_string()];
        config.use_perl5lib = true;
        config.perl5lib_precedence = Perl5LibPrecedence::Prepend;

        let reports = effective_root_reports(
            temp.path(),
            &config,
            &["./vendor/lib/".to_string()],
            ".perl-lsp.toml include_paths",
        );

        let env_report = reports.first().ok_or("missing PERL5LIB report")?;
        assert_eq!(env_report.source, "PERL5LIB");
        assert_eq!(env_report.raw, "./vendor/lib/");
        assert!(env_report.resolved.ends_with("vendor/lib"));

        let config_report = reports.get(1).ok_or("missing configured include report")?;
        assert_eq!(config_report.source, ".perl-lsp.toml include_paths");
        assert_eq!(config_report.raw, "./lib/");
        assert!(config_report.resolved.ends_with("lib"));
        Ok(())
    }

    #[test]
    fn effective_root_reports_use_winning_raw_value_for_overlapping_roots() -> TestResult {
        let temp = tempfile::tempdir()?;
        let mut append_config = WorkspaceConfig::default();
        append_config.include_paths = vec!["./shared/lib/".to_string()];
        append_config.use_perl5lib = true;
        append_config.perl5lib_precedence = Perl5LibPrecedence::Append;

        let append_reports = effective_root_reports(
            temp.path(),
            &append_config,
            &["shared/lib".to_string()],
            ".perl-lsp.toml include_paths",
        );
        let append_report = append_reports.first().ok_or("missing append root report")?;
        assert_eq!(append_report.source, ".perl-lsp.toml include_paths");
        assert_eq!(append_report.raw, "./shared/lib/");

        let mut prepend_config = WorkspaceConfig::default();
        prepend_config.include_paths = vec!["shared/lib".to_string()];
        prepend_config.use_perl5lib = true;
        prepend_config.perl5lib_precedence = Perl5LibPrecedence::Prepend;

        let prepend_reports = effective_root_reports(
            temp.path(),
            &prepend_config,
            &["./shared/lib/".to_string()],
            ".perl-lsp.toml include_paths",
        );
        let prepend_report = prepend_reports.first().ok_or("missing prepend root report")?;
        assert_eq!(prepend_report.source, "PERL5LIB");
        assert_eq!(prepend_report.raw, "./shared/lib/");
        Ok(())
    }

    #[test]
    fn effective_root_details_call_presence_observer() {
        let candidates = [EffectiveRootCandidate {
            raw: "./lib/".to_string(),
            source: ".perl-lsp.toml include_paths",
        }];

        let (raw, source) = effective_root_details("lib", &candidates);

        assert_eq!(raw, "./lib/");
        assert_eq!(source, ".perl-lsp.toml include_paths");
    }

    #[test]
    fn effective_root_details_normalized_path_text_boundary_discriminator() {
        let candidates =
            [EffectiveRootCandidate { raw: "./vendor/lib/".to_string(), source: "PERL5LIB" }];

        let (raw, source) = effective_root_details("vendor/lib", &candidates);

        assert_eq!(raw, "./vendor/lib/");
        assert_eq!(source, "PERL5LIB");
    }

    #[test]
    fn effective_root_details_boundary_discriminator_input_that_hits_the_boundary_normalized_path_text_candidate_raw_equals_normalized()
     {
        let candidates = [
            EffectiveRootCandidate { raw: "unrelated/lib".to_string(), source: "PERL5LIB" },
            EffectiveRootCandidate {
                raw: "./vendor/lib/".to_string(),
                source: ".perl-lsp.toml include_paths",
            },
        ];
        let candidate = &candidates[1];
        let normalized = normalized_path_text("vendor/lib");

        assert_eq!(
            normalized_path_text(&candidate.raw),
            normalized,
            "input that hits the boundary: normalized_path_text(&candidate.raw) == normalized"
        );
        let (raw, source) = effective_root_details("vendor/lib", &candidates);

        assert_eq!(raw, "./vendor/lib/");
        assert_eq!(source, ".perl-lsp.toml include_paths");
    }

    #[test]
    fn effective_root_details_falls_back_to_effective_path() {
        let candidates =
            [EffectiveRootCandidate { raw: "other/lib".to_string(), source: "PERL5LIB" }];

        let (raw, source) = effective_root_details("lib", &candidates);

        assert_eq!(raw, "lib");
        assert_eq!(source, "effective include path");
    }

    #[test]
    fn doctor_report_loads_project_config() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::create_dir_all(temp.path().join("custom/lib"))?;
        std::fs::write(
            temp.path().join(".perl-lsp.toml"),
            "[perl]\ninclude_paths = [\"custom/lib\"]\nuse_perl5lib = false\n",
        )?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;

        let report = render_report(build_doctor_report_struct(dir)?);

        assert!(report.contains("Project config: loaded .perl-lsp.toml"));
        assert!(report.contains("custom/lib"));
        assert!(report.contains("PERL5LIB precedence: prepend"));
        assert!(report.contains("Claim boundary:"));
        Ok(())
    }

    #[test]
    fn load_workspace_config_loaded_default_include_paths_boundary() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join(".perl-lsp.toml"), "[perl]\nuse_perl5lib = false\n")?;
        let mut workspace_config = WorkspaceConfig::default();

        let report = load_workspace_config(temp.path(), &mut workspace_config)?;

        assert!(matches!(report.status, ProjectConfigStatus::Loaded));
        assert_eq!(report.include_source, "default includePaths");
        Ok(())
    }

    #[test]
    fn load_workspace_config_surfaces_rejected_absolute_include_path() -> TestResult {
        let temp = tempfile::tempdir()?;
        // "/etc" is not absolute on Windows; there the entry would be rejected
        // as an unsafe relative path instead, so the test would pass for the
        // wrong reason. Mirrors the platform handling in
        // `apply_to_workspace_config_rejects_absolute_include_paths`.
        let absolute = if cfg!(windows) { r"C:\Windows" } else { "/etc" };
        std::fs::write(
            temp.path().join(".perl-lsp.toml"),
            format!("[perl]\ninclude_paths = [\"{}\", \"lib\"]\n", absolute.escape_default()),
        )?;
        let mut workspace_config = WorkspaceConfig::default();

        let report = load_workspace_config(temp.path(), &mut workspace_config)?;

        assert!(matches!(report.status, ProjectConfigStatus::Loaded));
        assert_eq!(workspace_config.include_paths, vec!["lib".to_string()]);
        assert_eq!(report.rejected_include_paths.len(), 1);
        assert!(report.rejected_include_paths[0].contains(absolute));
        Ok(())
    }

    #[test]
    fn doctor_report_prints_rejected_include_paths_section() -> TestResult {
        let temp = tempfile::tempdir()?;
        let absolute = if cfg!(windows) { r"C:\Windows" } else { "/etc" };
        std::fs::write(
            temp.path().join(".perl-lsp.toml"),
            format!("[perl]\ninclude_paths = [\"{}\"]\n", absolute.escape_default()),
        )?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;

        let rendered = render_report(build_doctor_report_struct(dir)?);

        assert!(rendered.contains("Rejected .perl-lsp.toml include_paths entries:"));
        assert!(rendered.contains(absolute));
        Ok(())
    }

    #[test]
    fn doctor_report_rejects_invalid_project_config() -> TestResult {
        let temp = tempfile::tempdir()?;
        std::fs::write(temp.path().join(".perl-lsp.toml"), "[perl\ninclude_paths = [\"lib\"]")?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;

        let error = build_doctor_report_struct(dir)
            .err()
            .ok_or("invalid project config should fail doctor")?;

        assert!(error.contains(".perl-lsp.toml"));
        assert!(error.contains("syntax error"));
        Ok(())
    }

    #[test]
    fn load_workspace_config_names_invalid_project_config_path() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config_path = temp.path().join(".perl-lsp.toml");
        std::fs::write(&config_path, "[perl\ninclude_paths = [\"lib\"]")?;
        let mut workspace_config = WorkspaceConfig::default();

        let error = load_workspace_config(temp.path(), &mut workspace_config)
            .err()
            .ok_or("invalid project config should fail")?;

        assert!(
            error.starts_with(&format!("{}: ", config_path.display())),
            "error should name the invalid config path: {error}"
        );
        assert!(error.contains("syntax error"));
        Ok(())
    }

    #[test]
    fn load_workspace_config_exact_error_variant() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config_path = temp.path().join(".perl-lsp.toml");
        std::fs::write(&config_path, "[perl\ninclude_paths = [\"lib\"]")?;
        let mut workspace_config = WorkspaceConfig::default();

        let result = load_workspace_config(temp.path(), &mut workspace_config);

        assert!(matches!(
            result,
            Err(error)
                if error.starts_with(&format!("{}: ", config_path.display()))
                    && error.contains("syntax error")
        ));
        Ok(())
    }

    #[test]
    fn render_perl5lib_status_reports_empty_and_disabled_boundaries() {
        assert_eq!(render_perl5lib_status(true, 0), "environment empty");
        assert_eq!(render_perl5lib_status(false, 0), "environment empty");
        assert_eq!(render_perl5lib_status(true, 2), "enabled, 2 environment path(s) participate");
        assert_eq!(
            render_perl5lib_status(false, 2),
            "disabled by config, 2 environment path(s) ignored"
        );
    }

    #[test]
    fn probe_perl_reports_configured_path_nonzero_probe() -> TestResult {
        let temp = tempfile::tempdir()?;
        let current_exe = std::env::current_exe()?;
        let mut config = WorkspaceConfig::default();
        config.perl_path = Some(current_exe.to_string_lossy().into_owned());

        let report = probe_perl(&config, temp.path());

        assert_eq!(report.source, "configured perl_path");
        assert_eq!(report.binary.as_deref(), Some(current_exe.as_path()));
        assert!(report.version.is_none());
        let error = report.error.ok_or("non-Perl executable should fail version probe")?;
        assert!(error.contains("version probe exited with status"));
        Ok(())
    }

    #[test]
    fn version_probe_error_preserves_stderr_guidance() {
        let error = version_probe_error_from_parts("exit status: 2", b"Can't locate App.pm\n");

        assert_eq!(
            error,
            "version probe exited with status exit status: 2; stderr: Can't locate App.pm"
        );
    }

    #[test]
    fn probe_perl_reports_path_resolution_error() -> TestResult {
        let temp = tempfile::tempdir()?;
        let config = WorkspaceConfig::default();

        let report = probe_perl_with_resolver(&config, temp.path(), || {
            Err(anyhow::anyhow!("perl binary not found on PATH"))
        });

        assert_eq!(report.source, "PATH");
        assert!(report.binary.is_none());
        assert!(report.version.is_none());
        let error = report.error.ok_or("missing Perl should report path resolution guidance")?;
        assert!(error.contains("perl binary not found on PATH"));
        Ok(())
    }

    #[test]
    fn probe_perl_reports_toolchain_resolver_source() -> TestResult {
        let temp = tempfile::tempdir()?;
        let current_exe = std::env::current_exe()?;
        let config = WorkspaceConfig::default();

        let report =
            probe_perl_with_resolver(&config, temp.path(), || Ok((current_exe.clone(), "plenv")));

        assert_eq!(report.source, "plenv");
        assert_eq!(report.binary.as_deref(), Some(current_exe.as_path()));
        assert!(report.version.is_none());
        let error = report.error.ok_or("non-Perl executable should fail version probe")?;
        assert!(error.contains("version probe exited with status"));
        Ok(())
    }

    #[test]
    fn probe_perl_reports_configured_path_spawn_error() -> TestResult {
        let temp = tempfile::tempdir()?;
        let missing = temp.path().join("missing-perl");
        let mut config = WorkspaceConfig::default();
        config.perl_path = Some(missing.to_string_lossy().into_owned());

        let report = probe_perl(&config, temp.path());

        assert_eq!(report.source, "configured perl_path");
        assert_eq!(report.binary.as_deref(), Some(missing.as_path()));
        assert!(report.version.is_none());
        let error = report.error.ok_or("missing configured Perl should fail to spawn")?;
        assert!(error.contains("command failed to start"));
        Ok(())
    }

    #[test]
    fn probe_tool_reports_missing_binary() -> TestResult {
        let report = probe_tool_with_resolver("perltidy", "--version", |_| None);

        assert_eq!(report.source, "PATH");
        assert!(report.binary.is_none());
        assert!(report.version.is_none());
        let error = report.error.ok_or("missing tool should report an error")?;
        assert!(error.contains("perltidy not found on PATH"));
        Ok(())
    }

    #[test]
    fn probe_tool_reports_version_probe_failure() -> TestResult {
        let current_exe = std::env::current_exe()?;
        let report = probe_tool_with_resolver("perltidy", "--version", |_| Some(current_exe));

        assert_eq!(report.source, "PATH");
        assert!(report.binary.is_some());
        assert!(report.version.is_none());
        let error = report.error.ok_or("non-perltidy binary should fail version probe")?;
        assert!(error.contains("perltidy version probe exited with status"));
        Ok(())
    }

    #[test]
    fn render_tool_report_found_with_version() {
        let report = ToolReport {
            binary: Some(PathBuf::from("/usr/bin/perltidy")),
            source: "PATH",
            version: Some("perltidy, v20240202".to_string()),
            error: None,
        };
        assert_eq!(render_tool_report(&report), "/usr/bin/perltidy (PATH); perltidy, v20240202");
    }

    #[test]
    fn render_tool_report_found_without_version() {
        let version_unavailable = ToolReport {
            binary: Some(PathBuf::from("/usr/bin/perlcritic")),
            source: "PATH",
            version: None,
            error: Some("perlcritic version probe exited with status 2".to_string()),
        };
        assert_eq!(
            render_tool_report(&version_unavailable),
            "/usr/bin/perlcritic (PATH); version unavailable: perlcritic version probe exited with status 2"
        );

        let no_error = ToolReport {
            binary: Some(PathBuf::from("/usr/bin/perlcritic")),
            source: "PATH",
            version: None,
            error: None,
        };
        assert_eq!(
            render_tool_report(&no_error),
            "/usr/bin/perlcritic (PATH); version unavailable"
        );
    }

    #[test]
    fn render_tool_report_missing() {
        let report = ToolReport {
            binary: None,
            source: "PATH",
            version: None,
            error: Some("perltidy not found on PATH".to_string()),
        };
        assert_eq!(render_tool_report(&report), "not found (PATH); perltidy not found on PATH");
    }

    #[test]
    fn doctor_report_includes_tool_detection_lines() -> TestResult {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().to_str().ok_or("non-UTF-8 temp path")?;

        let report = render_report(build_doctor_report_struct(dir)?);

        assert!(report.contains("perltidy:"));
        assert!(report.contains("perlcritic:"));
        Ok(())
    }

    #[test]
    fn doctor_report_next_steps_mention_install_hints_when_tools_missing() -> TestResult {
        // Force both tools to be reported as missing by injecting a resolver
        // that always returns None. Rebuild the report pieces the same way
        // `build_doctor_report_struct` does, but with the missing-tool reports, to
        // assert the Next steps hints render without depending on whether
        // perltidy/perlcritic are installed in the test environment.
        let temp = tempfile::tempdir()?;
        let workspace = workspace_dir(temp.path().to_str().ok_or("non-UTF-8 temp path")?)?;
        let perltidy = probe_tool_with_resolver("perltidy", "--version", |_| None);
        let perlcritic = probe_tool_with_resolver("perlcritic", "--version", |_| None);

        let rendered = render_report(DoctorReport {
            workspace,
            config: ProjectConfigReport {
                status: ProjectConfigStatus::Missing,
                include_source: "default includePaths",
                rejected_include_paths: Vec::new(),
            },
            perl: PerlReport {
                binary: None,
                source: "PATH",
                version: None,
                error: Some("perl binary not found on PATH".to_string()),
            },
            perltidy,
            perlcritic,
            perl5lib_paths: Vec::new(),
            perl5lib_enabled: false,
            perl5lib_precedence: Perl5LibPrecedence::Prepend,
            configured_paths: Vec::new(),
            effective_paths: Vec::new(),
            system_inc: SystemIncReport { status: "disabled", paths: Vec::new() },
        });

        assert!(rendered.contains("Install perltidy (cpanm Perl::Tidy)"));
        assert!(rendered.contains("Install perlcritic (cpanm Perl::Critic)"));
        Ok(())
    }

    #[test]
    fn system_inc_report_disabled_boundary() {
        let mut config = WorkspaceConfig::default();
        config.use_system_inc = false;

        let report = system_inc_report(Path::new("."), &mut config);

        assert_eq!(report.status, "disabled");
        assert!(report.paths.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn system_inc_report_enabled_resolves_probe_paths() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let fake_perl = temp.path().join("fake-perl");
        let absolute_inc = std::env::temp_dir().join("perl-lsp-doctor-system-inc-absolute");
        std::fs::write(
            &fake_perl,
            format!(
                "#!/bin/sh\nprintf '%s\\n' 'relative-system-inc'\nprintf '%s\\n' '{}'\n",
                absolute_inc.display()
            ),
        )?;
        let mut permissions = std::fs::metadata(&fake_perl)?.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_perl, permissions)?;
        let mut config = WorkspaceConfig::default();
        config.use_system_inc = true;
        config.perl_path = Some(fake_perl.to_string_lossy().into_owned());

        let report = system_inc_report(temp.path(), &mut config);
        let probe_cwd = std::env::current_dir()?;

        assert_eq!(report.status, "enabled");
        assert_eq!(report.paths, vec![probe_cwd.join("relative-system-inc"), absolute_inc]);
        Ok(())
    }

    #[test]
    fn render_path_reports_empty_boundary() {
        let mut out = String::new();

        render_path_reports(&mut out, &[]);

        assert_eq!(out, "  (none)\n");
    }

    #[test]
    fn render_status_binary_and_version_branch_discriminators() {
        let loaded = ProjectConfigReport {
            status: ProjectConfigStatus::Loaded,
            include_source: ".perl-lsp.toml include_paths",
            rejected_include_paths: Vec::new(),
        };
        let missing = ProjectConfigReport {
            status: ProjectConfigStatus::Missing,
            include_source: "default includePaths",
            rejected_include_paths: Vec::new(),
        };
        assert_eq!(render_project_config_status(&loaded), "loaded .perl-lsp.toml");
        assert_eq!(render_project_config_status(&missing), "not found, using defaults");

        let binary_report = PerlReport {
            binary: Some(PathBuf::from("/usr/bin/perl")),
            source: "PATH",
            version: Some("v5.40.0".to_string()),
            error: None,
        };
        assert_eq!(render_perl_binary(&binary_report), "/usr/bin/perl (PATH)");
        assert_eq!(render_perl_version(&binary_report), "v5.40.0");

        let error_report = PerlReport {
            binary: None,
            source: "PATH",
            version: None,
            error: Some("probe failed".to_string()),
        };
        assert_eq!(render_perl_binary(&error_report), "not found (PATH)");
        assert_eq!(render_perl_version(&error_report), "not available: probe failed");

        let empty_report =
            PerlReport { binary: None, source: "PATH", version: Some(String::new()), error: None };
        assert_eq!(render_perl_version(&empty_report), "not available");
    }

    #[test]
    fn run_doctor_external_tools_text_exit_zero() {
        assert_eq!(run_doctor_external_tools(false), 0);
    }

    #[test]
    fn run_doctor_external_tools_json_exit_zero() {
        assert_eq!(run_doctor_external_tools(true), 0);
    }

    #[test]
    fn run_doctor_critic_compatibility_exit_zero() {
        assert_eq!(run_doctor_critic_compatibility(false), 0);
        assert_eq!(run_doctor_critic_compatibility(true), 0);
    }

    #[test]
    fn run_cli_dispatches_doctor_external_tools() {
        let code = crate::run_cli(["perl-lsp", "--doctor", "--external-tools"]);
        assert_eq!(code, 0);
        let code = crate::run_cli(["perl-lsp", "--doctor", "--critic-compatibility"]);
        assert_eq!(code, 0);
    }

    #[test]
    fn run_cli_rejects_mode_flags_without_doctor() {
        assert_eq!(crate::run_cli(["perl-lsp", "--external-tools"]), 1);
        assert_eq!(crate::run_cli(["perl-lsp", "--critic-compatibility"]), 1);
        assert_eq!(crate::run_cli(["perl-lsp", "--dev-environment"]), 1);
    }

    // ── Development-environment arm (#12595) ────────────────────────────

    #[test]
    fn run_cli_dispatches_doctor_dev_environment() {
        assert_eq!(crate::run_cli(["perl-lsp", "--doctor", "--dev-environment"]), 0);
    }

    #[test]
    fn workspace_pins_match_toolchain_file_and_crate_manifest() -> TestResult {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let toolchain_path =
            Path::new(manifest_dir).join("..").join("..").join("rust-toolchain.toml");
        let toolchain = std::fs::read_to_string(&toolchain_path)
            .map_err(|error| format!("reading {}: {error}", toolchain_path.display()))?;
        let channel_line = toolchain
            .lines()
            .find(|line| line.trim_start().starts_with("channel"))
            .ok_or("rust-toolchain.toml should pin a channel")?;
        let channel = channel_line.split('"').nth(1).ok_or("quoted channel value")?;

        assert_eq!(channel, TOOLCHAIN_CHANNEL_LABEL, "rust-toolchain.toml drifted");
        assert_eq!(TOOLCHAIN_CHANNEL_LABEL, "1.95.0");
        let mut channel_parts = TOOLCHAIN_CHANNEL_LABEL.split('.');
        let channel_major = channel_parts.next().and_then(|part| part.parse::<u64>().ok());
        let channel_minor = channel_parts.next().and_then(|part| part.parse::<u64>().ok());
        assert_eq!(channel_major, Some(WORKSPACE_RUST_VERSION_MAJOR));
        assert_eq!(channel_minor, Some(WORKSPACE_RUST_VERSION_MINOR));
        assert_eq!(
            format!("{}.{}", WORKSPACE_RUST_VERSION_MAJOR, WORKSPACE_RUST_VERSION_MINOR),
            env!("CARGO_PKG_RUST_VERSION"),
            "workspace rust-version drifted; update WORKSPACE_RUST_VERSION_*"
        );
        Ok(())
    }

    #[test]
    fn parse_cargo_version_line_parses_rustup_and_distro_banners() {
        assert_eq!(
            parse_cargo_version_line("cargo 1.95.0 (8f3d0b0ac 2026-01-30)"),
            Some((1, 95, 0))
        );
        assert_eq!(parse_cargo_version_line("cargo 1.75.0 (2ca31a4c3 2023-12-26)"), Some((1, 75, 0)));
    }

    #[test]
    fn parse_cargo_version_line_rejects_non_version_words() {
        assert_eq!(parse_cargo_version_line(""), None);
        assert_eq!(parse_cargo_version_line("cargo docker-hash 2026-01-30"), None);
    }

    #[test]
    fn version_below_workspace_pin_boundary() {
        assert!(version_below_workspace_pin((1, 94, 9)));
        assert!(!version_below_workspace_pin((1, 95, 0)));
        assert!(!version_below_workspace_pin((1, 96, 0)));
        assert!(!version_below_workspace_pin((2, 0, 0)));
    }

    #[test]
    fn reachable_cargo_status_prefers_non_rustup_then_stale() {
        let (status, meets) =
            reachable_cargo_status(Some(CargoProvenance::NonRustup), Some((1, 75, 0)));
        assert_eq!(status, STATUS_NON_RUSTUP);
        assert_eq!(meets, Some(false));

        let (stale, stale_meets) =
            reachable_cargo_status(Some(CargoProvenance::RustupShim), Some((1, 90, 1)));
        assert_eq!(stale, STATUS_STALE);
        assert_eq!(stale_meets, Some(false));

        let (healthy, healthy_meets) =
            reachable_cargo_status(Some(CargoProvenance::RustupShim), Some((1, 95, 0)));
        assert_eq!(healthy, STATUS_PRESENT);
        assert_eq!(healthy_meets, Some(true));

        let (unparsable, unparsable_meets) =
            reachable_cargo_status(Some(CargoProvenance::RustupShim), None);
        assert_eq!(unparsable, STATUS_PRESENT);
        assert_eq!(unparsable_meets, None);
    }

    #[test]
    fn classify_cargo_provenance_separates_shims_from_distro_paths() {
        assert_eq!(
            classify_cargo_provenance(r"C:\Users\dev\.cargo\bin\cargo.exe"),
            CargoProvenance::RustupShim
        );
        assert_eq!(
            classify_cargo_provenance("/home/dev/.rustup/toolchains/1.95.0-x86_64/bin/cargo"),
            CargoProvenance::RustupShim
        );
        assert_eq!(
            classify_cargo_provenance("/usr/bin/cargo"),
            CargoProvenance::NonRustup,
            "apt/distro cargo must not pass as a rustup shim (#12595)"
        );
    }

    #[test]
    fn classify_windows_bash_path_separates_wsl_shim_from_git_bash() {
        assert_eq!(
            classify_windows_bash_path(r"C:\Windows\System32\bash.exe"),
            WindowsBashKind::WslSystem32
        );
        assert_eq!(
            classify_windows_bash_path(r"C:\Program Files\Git\bin\bash.exe"),
            WindowsBashKind::PosixProvider
        );
        assert_eq!(
            classify_windows_bash_path(r"C:\msys64\usr\bin\bash.exe"),
            WindowsBashKind::PosixProvider
        );
        assert_eq!(
            classify_windows_bash_path(r"D:\tools\busybox-bash.exe"),
            WindowsBashKind::OtherProvider
        );
    }

    #[test]
    fn decode_shell_output_decodes_utf16le_and_plain_utf8() {
        // wsl.exe emits its own diagnostics as UTF-16LE through pipes.
        let utf16_text = "Windows Subsystem for Linux";
        let mut utf16_bytes = Vec::new();
        for unit in utf16_text.encode_utf16() {
            utf16_bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode_shell_output(&utf16_bytes), utf16_text);

        assert_eq!(decode_shell_output(b"cargo 1.95.0\n"), "cargo 1.95.0\n");
        assert_eq!(decode_shell_output(&[0xEF, 0xBB, 0xBF, b'x']), "x", "BOM stripped");
    }

    #[test]
    fn extract_perl_version_finds_banner_token() {
        assert_eq!(
            extract_perl_version(
                "\nThis is perl 5, version 42, subversion 2 (v5.42.2) built for MSWin32-x64-multi-thread\n(with 2 registered patches)\n"
            ),
            Some("v5.42.2".to_string())
        );
        assert_eq!(extract_perl_version("no version banner"), None);
    }

    #[test]
    fn classify_perl_identity_from_synthetic_paths() {
        assert_eq!(
            classify_perl_identity(r"C:\msys64\usr\bin\perl.exe", true),
            PerlIdentityKind::MsysCygwin
        );
        assert_eq!(
            classify_perl_identity(r"C:\Strawberry\perl\bin\perl.exe", true),
            PerlIdentityKind::Strawberry
        );
        assert_eq!(
            classify_perl_identity("/usr/bin/perl", false),
            PerlIdentityKind::SystemUnix,
            "on Unix hosts /usr/bin/perl is the system perl"
        );
        assert_eq!(
            classify_perl_identity("/usr/bin/perl", true),
            PerlIdentityKind::MsysCygwin,
            "POSIX-rooted perl on a Windows host belongs to an MSYS/Cygwin environment"
        );
        assert_eq!(
            classify_perl_identity(r"C:\tools\somewhere\perl.exe", true),
            PerlIdentityKind::Unknown
        );
    }

    #[test]
    fn other_named_identities_flags_only_new_named_kinds() {
        let others = other_named_identities(
            PerlIdentityKind::MsysCygwin,
            &[
                PerlIdentityKind::MsysCygwin,
                PerlIdentityKind::Strawberry,
                PerlIdentityKind::Unknown,
            ],
        );
        assert_eq!(others, vec!["strawberry"]);

        assert!(other_named_identities(PerlIdentityKind::Strawberry, &[PerlIdentityKind::Strawberry])
            .is_empty());
        assert!(other_named_identities(
            PerlIdentityKind::Unknown,
            &[PerlIdentityKind::Unknown, PerlIdentityKind::Unknown]
        )
        .is_empty());
    }

    #[test]
    fn locate_repo_root_walks_up_and_fails_closed() -> TestResult {
        let temp = tempfile::tempdir()?;
        let nested = temp.path().join("a").join("b");
        std::fs::create_dir_all(&nested)?;
        std::fs::create_dir_all(temp.path().join(".github"))?;
        std::fs::write(temp.path().join(".github").join("run_all_tests.sh"), "#!/bin/sh\n")?;

        let root = locate_repo_root(&nested).ok_or("marker two levels up should be found")?;
        assert_eq!(root, temp.path());
        let marker_free = tempfile::tempdir()?;
        assert!(
            locate_repo_root(&marker_free.path().join("missing-root")).is_none(),
            "a checkout without the marker must fail closed"
        );
        Ok(())
    }

    #[test]
    fn common_perl_candidate_paths_probe_common_locations_only() {
        let windows_candidates = common_perl_candidate_paths(true);
        assert_eq!(windows_candidates.len(), 3);
        assert!(
            windows_candidates
                .iter()
                .any(|path| path.display().to_string().contains("Strawberry"))
        );
        assert_eq!(common_perl_candidate_paths(false).len(), 1);
    }

    #[test]
    fn cargo_fix_lines_cover_each_failure_shape() -> TestResult {
        let non_rustup = cargo_fix_line(FLAVOR_WSL, STATUS_NON_RUSTUP)
            .ok_or("non-rustup finding must carry a fix")?;
        assert!(non_rustup.contains("inside WSL"));
        assert!(non_rustup.contains("sh.rustup.rs"));

        let native_non_rustup = cargo_fix_line(FLAVOR_NATIVE_SHELL, STATUS_NON_RUSTUP)
            .ok_or("native non-rustup finding must carry a fix")?;
        assert!(native_non_rustup.contains("sh.rustup.rs"));

        let stale =
            cargo_fix_line(FLAVOR_GIT_BASH, STATUS_STALE).ok_or("stale finding must carry a fix")?;
        assert!(stale.contains("rustup toolchain install 1.95.0"));

        let missing = cargo_fix_line(FLAVOR_NATIVE_SHELL, STATUS_MISSING)
            .ok_or("unreachable flavor must carry a fix")?;
        assert!(missing.contains("install rustup"));

        assert!(cargo_fix_line(FLAVOR_NATIVE_SHELL, STATUS_PRESENT).is_none());
        Ok(())
    }

    #[test]
    fn dev_env_status_codes_are_the_closed_set() {
        let codes = dev_env_status_codes();
        assert_eq!(codes.len(), 7);
        for expected in [
            STATUS_PRESENT,
            STATUS_MISSING,
            STATUS_NOT_APPLICABLE,
            STATUS_STALE,
            STATUS_NON_RUSTUP,
            STATUS_DIVERGENT,
            STATUS_PROBE_ERROR,
        ] {
            assert!(codes.contains(&expected), "{expected} missing from closed set");
        }
    }

    fn synthetic_dev_environment_report() -> DevEnvironmentReport {
        DevEnvironmentReport {
            schema: DEV_ENVIRONMENT_SCHEMA,
            host_platform: HOST_PLATFORM_WINDOWS,
            workspace_rust_version: WORKSPACE_RUST_VERSION_LABEL,
            toolchain_channel_pin: TOOLCHAIN_CHANNEL_LABEL,
            symlink_privilege: SymlinkPrivilegeReport {
                status: STATUS_MISSING,
                detail: "creating a file symlink failed with os error 1314".to_string(),
                fix: Some(FIX_SYMLINK_PRIVILEGE.to_string()),
            },
            cargo_toolchains: vec![
                CargoToolchainReport {
                    flavor: FLAVOR_NATIVE_SHELL,
                    status: STATUS_PRESENT,
                    path: Some(r"C:\Users\dev\.cargo\bin\cargo.exe".to_string()),
                    version: Some("cargo 1.95.0 (8f3d0b0ac 2026-01-30)".to_string()),
                    provenance: PROVENANCE_RUSTUP_SHIM,
                    meets_workspace_pin: Some(true),
                    honors_toolchain_file: Some(true),
                    error: None,
                    fix: None,
                },
                CargoToolchainReport {
                    flavor: FLAVOR_WSL,
                    status: STATUS_NON_RUSTUP,
                    path: Some("/usr/bin/cargo".to_string()),
                    version: Some("cargo 1.75.0".to_string()),
                    provenance: PROVENANCE_NON_RUSTUP,
                    meets_workspace_pin: Some(false),
                    honors_toolchain_file: Some(false),
                    error: None,
                    fix: Some(format!("fix: inside WSL run: {RUSTUP_INSTALL_ONE_LINER}")),
                },
            ],
            bash_flavors: vec![BashFlavorReport {
                flavor: FLAVOR_GIT_BASH,
                status: STATUS_MISSING,
                bash_path: None,
                runs_repo_entrypoints: None,
                note: "no bash.exe on PATH".to_string(),
                fix: Some(FIX_BASH_INSTALL_GIT_WINDOWS.to_string()),
            }],
            repo_entrypoints: RepoEntrypointsReport {
                marker: REPO_ENTRYPOINT_MARKER,
                located: true,
                complete: Some(true),
                note: "repository root: <checkout>".to_string(),
            },
            documented_prerequisite: BASH_PREREQUISITE_LINE,
            perl_identity: PerlIdentityReport {
                status: STATUS_DIVERGENT,
                path: Some(r"C:\msys64\usr\bin\perl.exe".to_string()),
                version: Some("v5.42.2".to_string()),
                identity: IDENTITY_MSYS_CYGWIN,
                other_identities: vec![IDENTITY_STRAWBERRY],
                error: None,
                fix: Some(FIX_PERL_IDENTITY_DIVERGENCE.to_string()),
            },
        }
    }

    #[test]
    fn render_dev_environment_report_surfaces_findings_and_prescribed_fixes() {
        let rendered = render_dev_environment_report(&synthetic_dev_environment_report());

        assert!(rendered.contains("perl-lsp doctor - development environment"));
        assert!(rendered.contains("Symlink privilege: missing"));
        assert!(rendered.contains("Developer Mode"));
        assert!(rendered.contains("- wsl: non_rustup | /usr/bin/cargo | cargo 1.75.0"));
        assert!(rendered.contains("meets workspace pin (1.95): no"));
        assert!(rendered.contains("WARNING: additional distinct identities"));
        assert!(rendered.contains("strawberry"));
        assert!(rendered.contains(BASH_PREREQUISITE_LINE));
        assert!(rendered.contains("Claim boundary:"));
        assert!(rendered.matches("Fix:").count() >= 4);
    }

    #[test]
    fn dev_environment_json_schema_is_typed_and_closed_set() -> TestResult {
        use serde_json::Value;

        let temp = tempfile::tempdir()?;
        let report = build_dev_environment_report_in(temp.path());
        let json = serde_json::to_value(&report).map_err(|error| error.to_string())?;

        let schema = json.get("schema").and_then(Value::as_str).ok_or("schema field")?;
        assert_eq!(schema, DEV_ENVIRONMENT_SCHEMA);

        let allowed = dev_env_status_codes();

        let symlink_status = json
            .pointer("/symlink_privilege/status")
            .and_then(Value::as_str)
            .ok_or("symlink_privilege.status field")?;
        assert!(
            allowed.contains(&symlink_status),
            "unexpected symlink status {symlink_status}"
        );
        if cfg!(windows) {
            assert_ne!(
                symlink_status, STATUS_NOT_APPLICABLE,
                "a Windows host must actually probe symlink privilege"
            );
        } else {
            assert_eq!(symlink_status, STATUS_NOT_APPLICABLE);
        }

        let cargo_rows = json
            .get("cargo_toolchains")
            .and_then(Value::as_array)
            .ok_or("cargo_toolchains array")?;
        assert_eq!(cargo_rows.len(), 3, "one row per flavor on every host");
        for row in cargo_rows {
            let status = row.get("status").and_then(Value::as_str).ok_or("cargo row status")?;
            assert!(allowed.contains(&status), "unexpected cargo status {status}");
            let flavor = row.get("flavor").and_then(Value::as_str).ok_or("cargo row flavor")?;
            if !cfg!(windows) && flavor != FLAVOR_NATIVE_SHELL {
                let unix_status =
                    row.get("status").and_then(Value::as_str).ok_or("unix flavor status")?;
                assert_eq!(
                    unix_status, STATUS_NOT_APPLICABLE,
                    "git_bash/wsl are not applicable flavors on unix"
                );
            }
        }

        let bash_rows = json
            .get("bash_flavors")
            .and_then(Value::as_array)
            .ok_or("bash_flavors array")?;
        assert_eq!(bash_rows.len(), 3, "one row per flavor on every host");
        for row in bash_rows {
            let status = row.get("status").and_then(Value::as_str).ok_or("bash row status")?;
            assert!(allowed.contains(&status), "unexpected bash status {status}");
        }

        let perl_status = json
            .pointer("/perl_identity/status")
            .and_then(Value::as_str)
            .ok_or("perl_identity.status field")?;
        assert!(allowed.contains(&perl_status), "unexpected perl status {perl_status}");

        let prerequisite = json
            .get("documented_prerequisite")
            .and_then(Value::as_str)
            .ok_or("prerequisite line")?;
        assert_eq!(prerequisite, BASH_PREREQUISITE_LINE);
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn symlink_probe_reports_valid_status_and_always_cleans_up() -> TestResult {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("perllsp-doctor-probe-target.txt");
        let link = temp.path().join("perllsp-doctor-probe-target.ln");

        let report = run_file_symlink_probe(&target, &link);

        assert!(
            dev_env_status_codes().contains(&report.status),
            "unexpected probe status {}",
            report.status
        );
        assert_eq!(
            report.fix.as_deref() == Some(FIX_SYMLINK_PRIVILEGE),
            report.status == STATUS_MISSING,
            "only the privilege-not-held outcome prescribes the Developer Mode fix"
        );
        assert!(!link.exists(), "probe must remove the link");
        assert!(!target.exists(), "probe must remove the target file");
        Ok(())
    }
}
