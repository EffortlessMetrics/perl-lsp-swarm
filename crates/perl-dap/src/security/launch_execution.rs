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
            return Err(LaunchExecutionError::NoAdmittedInterpreter);
        }

        let mut canonical: Vec<PathBuf> = Vec::new();
        for raw in paths {
            let resolved = canonicalize_existing_interpreter(&raw).ok_or_else(|| {
                LaunchExecutionError::AdmittedInterpreterNotFound { path: raw.clone() }
            })?;
            if !is_valid_perl_basename(&resolved) {
                return Err(LaunchExecutionError::AdmittedInterpreterInvalid { path: raw });
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
        let launch_path = launch_args
            .get("env")
            .and_then(Value::as_object)
            .and_then(|env| env.get("PATH").or_else(|| env.get("path")))
            .and_then(|value| value.as_str());
        let resolved = resolve_interpreter_path(requested_interpreter, launch_path)
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
}

/// Passthrough admission for explicitly unbounded authority or legacy launches.
pub fn admit_launch_without_execution_policy(
    requested_interpreter: &str,
    launch_args: &Value,
    env_overrides: HashMap<String, String>,
) -> Result<AdmittedLaunchExecution, String> {
    let launch_path = launch_args
        .get("env")
        .and_then(Value::as_object)
        .and_then(|env| env.get("PATH").or_else(|| env.get("path")))
        .and_then(|value| value.as_str());
    let perl_binary = resolve_interpreter_path(requested_interpreter, launch_path)
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
    if allow_code_loading {
        return Ok(env_overrides);
    }
    let mut denied = Vec::new();
    let mut kept = HashMap::new();
    for (key, value) in env_overrides {
        if is_code_loading_env_key(&key) {
            denied.push(key);
        } else {
            kept.insert(key, value);
        }
    }
    if denied.is_empty() {
        return Ok(kept);
    }
    denied.sort();
    Err(format!(
        "Launch environment variable(s) {} are not admitted under workspace-bound launch \
         authority. Code-loading variables such as PERL5LIB and PERL5OPT require explicit \
         operator startup admission (--allow-launch-code-loading-env).",
        denied.join(", ")
    ))
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
        return resolve_perl_on_path_env(launch_path?);
    }
    let configured = if trimmed.eq_ignore_ascii_case("perl") { None } else { Some(trimmed) };
    match platform::find_perl_interpreter(configured) {
        PerlInterpreterResult::ConfiguredPath(path)
        | PerlInterpreterResult::FoundOnPath(path)
        | PerlInterpreterResult::FoundViaFallback { path, .. } => {
            canonicalize_existing_interpreter(&path)
        }
        PerlInterpreterResult::NotFound { .. } => None,
    }
}

fn resolve_perl_on_path_env(path_env: &str) -> Option<PathBuf> {
    let separator = if cfg!(windows) { ';' } else { ':' };
    for entry in path_env.split(separator).map(str::trim).filter(|entry| !entry.is_empty()) {
        let candidate = PathBuf::from(entry).join(if cfg!(windows) { "perl.exe" } else { "perl" });
        if let Some(canonical) = canonicalize_existing_interpreter(&candidate) {
            if is_valid_perl_basename(&canonical) {
                return Some(canonical);
            }
        }
    }
    None
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
    fn startup_policy_requires_an_admitted_interpreter_when_none_resolve() {
        let err = LaunchExecutionPolicy::resolve(&LaunchExecutionStartup::default(), &[]);
        match (
            err,
            perl_lsp_rs_core::config::PerlToolchainProfile::resolve(
                &perl_lsp_rs_core::config::WorkspaceConfig::default(),
            ),
        ) {
            (Err(LaunchExecutionError::NoAdmittedInterpreter), None) => {}
            (Ok(_), Some(_)) => {}
            (other, toolchain) => {
                panic!("unexpected resolve outcome: {other:?}, toolchain={toolchain:?}");
            }
        }
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
