//! Workspace-bound launch execution admission (#14601).
//!
//! Source confinement from [`LaunchAuthority`] does not authorize which
//! interpreter runs or which code-loading environment variables reach the
//! debuggee. This module resolves one admitted interpreter identity and one
//! filtered launch environment for both the syntax-check and `perl -d` spawns.

use crate::platform::{self, PerlInterpreterResult};
use crate::security::launch_authority::{LaunchAuthorityMode, LaunchExecutionStartup, TrustedRoot};
use perl_lsp_rs_core::config::{PerlOracleEnv, WorkspaceConfig};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static PERL_NAME_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^perl(\d+(\.\d+)*)?$"));

/// Startup-resolved execution policy for workspace-bound sessions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchExecutionPolicy {
    /// Canonical interpreter paths the operator admitted at startup.
    pub admitted_interpreter_paths: Vec<PathBuf>,
    /// Whether launch.json may supply code-loading environment variables.
    pub allow_launch_code_loading_env: bool,
}

/// One admitted interpreter plus launch environment for every spawn site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedLaunchExecution {
    /// Canonical interpreter path used for every spawn in this launch.
    pub perl_binary: PathBuf,
    /// Launch.json `env` entries that may reach the child (code-loading keys
    /// removed unless explicitly admitted at startup).
    pub env_overrides: HashMap<String, String>,
    /// Whether code-loading launch env keys were admitted for this launch.
    pub allow_launch_code_loading_env: bool,
}

impl AdmittedLaunchExecution {
    /// Build the shared [`PerlOracleEnv`] for syntax check, capability probe, and
    /// `perl -d`. Ambient code-loading variables stay denied (#8688); only
    /// filtered launch overrides reach the child.
    pub fn build_spawn_oracle(&self, cwd: PathBuf) -> PerlOracleEnv {
        let mut oracle = PerlOracleEnv::for_version_probe(self.perl_binary.clone(), cwd);
        oracle
            .extra_env
            .extend(self.env_overrides.iter().map(|(key, value)| (key.clone(), value.clone())));
        oracle
    }
}

impl LaunchExecutionPolicy {
    /// Resolve execution policy from operator-owned startup inputs.
    pub fn resolve(
        startup: &LaunchExecutionStartup,
        _trusted_roots: &[TrustedRoot],
    ) -> Result<Self, LaunchExecutionError> {
        let mut paths = startup.admitted_interpreters.clone();
        if let Some(profile) =
            perl_lsp_rs_core::config::PerlToolchainProfile::resolve(&WorkspaceConfig::default())
        {
            paths.push(profile.into_perl_binary());
        }
        if paths.is_empty() {
            return Ok(Self {
                admitted_interpreter_paths: Vec::new(),
                allow_launch_code_loading_env: startup.allow_launch_code_loading_env,
            });
        }

        let mut canonical: Vec<PathBuf> = Vec::new();
        for raw in paths {
            let resolved = canonicalize_existing_interpreter(&raw).ok_or_else(|| {
                LaunchExecutionError::AdmittedInterpreterNotFound { path: raw.clone() }
            })?;
            if !is_valid_perl_basename(&resolved) {
                return Err(LaunchExecutionError::AdmittedInterpreterInvalid { path: raw });
            }
            if interpreter_lies_under_trusted_roots(&resolved, _trusted_roots) {
                return Err(LaunchExecutionError::AdmittedInterpreterUnderTrustedRoot {
                    path: raw.clone(),
                });
            }
            if !canonical.iter().any(|existing| existing == &resolved) {
                canonical.push(resolved);
            }
        }
        canonical.sort();

        Ok(Self {
            admitted_interpreter_paths: canonical,
            allow_launch_code_loading_env: startup.allow_launch_code_loading_env,
        })
    }

    /// Admit the launch interpreter and environment under workspace-bound policy.
    pub fn admit_launch(
        &self,
        launch_args: &Value,
        requested_interpreter: &str,
        env_overrides: HashMap<String, String>,
        trusted_roots: &[TrustedRoot],
    ) -> Result<AdmittedLaunchExecution, String> {
        let launch_path = launch_env_path_from_args(launch_args);
        let resolved = resolve_interpreter_path(requested_interpreter, launch_path.as_deref())
            .ok_or_else(|| format_launch_interpreter_refusal(requested_interpreter))?;

        if interpreter_lies_under_trusted_roots(&resolved, trusted_roots) {
            return Err(
                "The selected Perl interpreter cannot live inside a trusted workspace root under \
                 workspace-bound launch authority. Point launch.json `perlPath` at a system or \
                 operator-admitted interpreter outside the repository, or reconfigure startup \
                 admission."
                    .to_string(),
            );
        }

        if !self.admitted_interpreter_paths.iter().any(|admitted| admitted == &resolved) {
            return Err(format!(
                "The selected Perl interpreter is not admitted by this adapter's startup \
                 configuration. Only operator-configured interpreters may run under \
                 workspace-bound launch authority. Update startup admission or set launch.json \
                 `perlPath` to an admitted interpreter."
            ));
        }

        let filtered_env =
            filter_launch_env_overrides(env_overrides, self.allow_launch_code_loading_env)?;

        Ok(AdmittedLaunchExecution {
            perl_binary: resolved,
            env_overrides: filtered_env,
            allow_launch_code_loading_env: self.allow_launch_code_loading_env,
        })
    }
}

/// Errors while resolving startup execution policy.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum LaunchExecutionError {
    #[error(
        "workspace-bound launch authority requires at least one admitted interpreter from startup \
         configuration or the toolchain profile"
    )]
    NoAdmittedInterpreter,
    #[error("admitted interpreter {path:?} does not exist or is not a regular file")]
    AdmittedInterpreterNotFound {
        /// Raw startup path.
        path: PathBuf,
    },
    #[error("admitted interpreter {path:?} is not a valid Perl executable name")]
    AdmittedInterpreterInvalid {
        /// Raw startup path.
        path: PathBuf,
    },
    #[error(
        "admitted interpreter {path:?} cannot live inside a trusted workspace root under \
         workspace-bound launch authority"
    )]
    AdmittedInterpreterUnderTrustedRoot {
        /// Raw startup path.
        path: PathBuf,
    },
}

/// Case-insensitive `PATH` from launch.json `env`, if present.
pub(crate) fn launch_env_path_from_args(args: &Value) -> Option<String> {
    args.get("env").and_then(Value::as_object).and_then(|env| {
        env.iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("PATH"))
            .and_then(|(_, value)| value.as_str())
            .map(str::to_string)
    })
}

/// Passthrough admission for explicitly unbounded authority or legacy launches.
pub fn admit_launch_without_execution_policy(
    requested_interpreter: &str,
    launch_args: &Value,
    env_overrides: HashMap<String, String>,
) -> Result<AdmittedLaunchExecution, String> {
    let launch_path = launch_env_path_from_args(launch_args);
    let perl_binary = resolve_interpreter_path(requested_interpreter, launch_path.as_deref())
        .ok_or_else(|| format_launch_interpreter_refusal(requested_interpreter))?;
    if !is_valid_perl_basename(&perl_binary) {
        return Err(format!(
            "Invalid Perl interpreter '{}'. Set launch.json `perlPath` to a Perl executable path \
             (for example, `perl` or `/usr/bin/perl`).",
            requested_interpreter
        ));
    }
    Ok(AdmittedLaunchExecution { perl_binary, env_overrides, allow_launch_code_loading_env: true })
}

/// Whether `mode` enforces workspace-bound execution admission.
pub fn mode_requires_execution_policy(mode: LaunchAuthorityMode) -> bool {
    mode == LaunchAuthorityMode::WorkspaceBound
}

fn format_launch_interpreter_refusal(requested: &str) -> String {
    format!(
        "Could not resolve the Perl interpreter '{requested}' for launch. Check launch.json \
         `perlPath` or install a Perl distribution on PATH."
    )
}

fn filter_launch_env_overrides(
    env_overrides: HashMap<String, String>,
    allow_code_loading: bool,
) -> Result<HashMap<String, String>, String> {
    let mut debugger_denied = Vec::new();
    let mut code_loading_denied = Vec::new();
    let mut kept = HashMap::new();
    for (key, value) in env_overrides {
        if is_debugger_startup_env_key(&key) {
            debugger_denied.push(key);
        } else if is_code_loading_env_key(&key) && !allow_code_loading {
            code_loading_denied.push(key);
        } else {
            kept.insert(key, value);
        }
    }
    if debugger_denied.is_empty() && code_loading_denied.is_empty() {
        return Ok(kept);
    }
    debugger_denied.sort();
    code_loading_denied.sort();
    if !debugger_denied.is_empty() {
        return Err(format!(
            "Launch environment variable(s) {} are not admitted under workspace-bound launch \
             authority. Debugger startup variables such as PERL5DB and PERLDB_OPTS cannot be \
             supplied through launch.json because `perl -d` evaluates them as code.",
            debugger_denied.join(", ")
        ));
    }
    Err(format!(
        "Launch environment variable(s) {} are not admitted under workspace-bound launch \
         authority. Code-loading variables such as PERL5LIB and PERL5OPT require explicit \
         operator startup admission (--allow-launch-code-loading-env).",
        code_loading_denied.join(", ")
    ))
}

fn is_debugger_startup_env_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("PERL5DB")
        || key.eq_ignore_ascii_case("PERLDB_OPTS")
        || key.eq_ignore_ascii_case("PERL5DB_THREADED")
}

fn is_code_loading_env_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("PERL5LIB")
        || key.eq_ignore_ascii_case("PERL5OPT")
        || key.eq_ignore_ascii_case("PERLLIB")
        || key.eq_ignore_ascii_case("PERL_LOCAL_LIB_ROOT")
        || key.eq_ignore_ascii_case("PERL_LOCAL_LIB_PREFIX")
}

fn interpreter_lies_under_trusted_roots(interpreter: &Path, roots: &[TrustedRoot]) -> bool {
    roots.iter().any(|root| interpreter.starts_with(root.canonical()))
}

fn resolve_interpreter_path(requested: &str, launch_path: Option<&str>) -> Option<PathBuf> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("perl") && launch_path.is_some() {
        return resolve_named_executable_on_path_env("perl", launch_path?);
    }
    if trimmed.eq_ignore_ascii_case("perl") {
        return match platform::find_perl_interpreter(None) {
            PerlInterpreterResult::ConfiguredPath(path)
            | PerlInterpreterResult::FoundOnPath(path)
            | PerlInterpreterResult::FoundViaFallback { path, .. } => {
                canonicalize_existing_interpreter(&path)
            }
            PerlInterpreterResult::NotFound { .. } => None,
        };
    }

    let candidate_path = PathBuf::from(trimmed);
    if candidate_path.is_file() {
        return canonicalize_existing_interpreter(&candidate_path);
    }
    if candidate_path.is_absolute()
        || trimmed.contains('/') 
        || trimmed.contains('\\')
    {
        return None;
    }

    if let Some(path_env) = launch_path {
        if let Some(found) = resolve_named_executable_on_path_env(trimmed, path_env) {
            return Some(found);
        }
    }
    if let Ok(path_env) = std::env::var("PATH") {
        if let Some(found) = resolve_named_executable_on_path_env(trimmed, &path_env) {
            return Some(found);
        }
    }
    None
}

fn resolve_named_executable_on_path_env(executable: &str, path_env: &str) -> Option<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let base = executable.strip_suffix(".exe").unwrap_or(executable);
    for entry in path_env.split(separator).map(str::trim).filter(|entry| !entry.is_empty()) {
        for name in candidate_executable_names(base) {
            let candidate = PathBuf::from(entry).join(name);
            if let Some(canonical) = canonicalize_existing_interpreter(&candidate) {
                if is_valid_perl_basename(&canonical) {
                    return Some(canonical);
                }
            }
        }
    }
    None
}

fn candidate_executable_names(base: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{base}.exe"), base.to_string()]
    } else {
        vec![base.to_string()]
    }
}

fn canonicalize_existing_interpreter(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    path.canonicalize().ok().filter(|canonical| canonical.is_file())
}

fn is_valid_perl_basename(path: &Path) -> bool {
    let trimmed = path.to_string_lossy();
    if trimmed.trim().is_empty() {
        return false;
    }
    let candidate = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(trimmed.as_ref())
        .to_ascii_lowercase();
    let candidate = candidate.strip_suffix(".exe").unwrap_or(&candidate);
    match &*PERL_NAME_RE {
        Ok(re) => re.is_match(candidate),
        Err(_) => candidate == "perl",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::launch_authority::{LaunchAuthorityStartup, LaunchExecutionStartup};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn startup_policy_allows_empty_interpreter_set_when_none_resolve() {
        let outcome = LaunchExecutionPolicy::resolve(&LaunchExecutionStartup::default(), &[]);
        match (
            outcome,
            perl_lsp_rs_core::config::PerlToolchainProfile::resolve(
                &perl_lsp_rs_core::config::WorkspaceConfig::default(),
            ),
        ) {
            (Ok(policy), None) => assert!(policy.admitted_interpreter_paths.is_empty()),
            (Ok(policy), Some(_)) => assert!(!policy.admitted_interpreter_paths.is_empty()),
            (Err(error), toolchain) => {
                panic!("unexpected resolve outcome: {error:?}, toolchain={toolchain:?}");
            }
        }
    }

    #[test]
    fn workspace_bound_refuses_debugger_startup_env_without_exception() {
        for key in ["PERL5DB", "PERLDB_OPTS", "PERL5DB_THREADED"] {
            let err = filter_launch_env_overrides(
                HashMap::from([(key.to_string(), "1".to_string())]),
                true,
            )
            .expect_err("debugger env must stay denied even when code-loading env is admitted");
            assert!(err.contains(key), "expected refusal to name {key}, got: {err}");
        }
    }

    #[test]
    fn launch_env_path_from_args_is_case_insensitive() {
        let args = serde_json::json!({ "env": { "Path": "/custom/bin" } });
        assert_eq!(
            launch_env_path_from_args(&args).as_deref(),
            Some("/custom/bin")
        );
    }

    #[test]
    #[cfg(unix)]
    fn bare_interpreter_name_resolves_via_path_env() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let bin = root.path().join("bin");
        fs::create_dir(&bin)?;
        let named = bin.join("perl5.34.0");
        fs::write(&named, b"#!/bin/sh\nexit 0\n")?;
        let mut perms = fs::metadata(&named)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&named, perms)?;

        let path_env = format!("{}:/usr/bin", bin.display());
        let resolved = resolve_interpreter_path("perl5.34.0", Some(&path_env))
            .expect("bare name must resolve on PATH");
        assert_eq!(resolved, named.canonicalize()?);
        Ok(())
    }

    #[test]
    fn workspace_bound_refuses_code_loading_env_without_startup_admission() {
        let policy = LaunchExecutionPolicy {
            admitted_interpreter_paths: vec![PathBuf::from("/usr/bin/perl")],
            allow_launch_code_loading_env: false,
        };
        let mut env = HashMap::new();
        env.insert("PERL5LIB".to_string(), "/outside/lib".to_string());
        env.insert("FOO".to_string(), "bar".to_string());
        let err =
            policy.admit_launch(&Value::Null, "/usr/bin/perl", env, &[]).expect_err("must refuse");
        assert!(err.contains("PERL5LIB"));
        let filtered = filter_launch_env_overrides(
            HashMap::from([("FOO".to_string(), "bar".to_string())]),
            false,
        )
        .expect("benign env");
        assert_eq!(filtered.get("FOO").map(String::as_str), Some("bar"));
    }

    #[test]
    #[cfg(unix)]
    fn unlisted_launch_interpreter_is_refused_under_policy() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let admitted = root.path().join("admitted-perl");
        fs::write(&admitted, b"#!/bin/sh\nexit 0\n")?;
        let mut perms = fs::metadata(&admitted)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&admitted, perms.clone())?;
        let outside = root.path().join("outside-perl");
        fs::write(&outside, b"#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&outside, perms)?;
        let policy = LaunchExecutionPolicy {
            admitted_interpreter_paths: vec![admitted.canonicalize()?],
            allow_launch_code_loading_env: false,
        };
        let err = policy
            .admit_launch(&Value::Null, &outside.to_string_lossy(), HashMap::new(), &[])
            .expect_err("outside interpreter");
        assert!(err.contains("not admitted"));
        Ok(())
    }

    #[test]
    fn launch_execution_startup_is_defaulted_in_authority_startup() {
        let startup = LaunchAuthorityStartup::default();
        assert!(!startup.execution.allow_launch_code_loading_env);
        assert!(startup.execution.admitted_interpreters.is_empty());
    }
}
