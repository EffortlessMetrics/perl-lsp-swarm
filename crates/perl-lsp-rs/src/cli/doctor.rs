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
    }
}
