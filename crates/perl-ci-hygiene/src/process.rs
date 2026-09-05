use color_eyre::eyre::{Context, Result};
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// PATH components a parent-process preflight can resolve coherently with a
/// child that runs under a selected `current_dir`.
///
/// Every wrapper in this module launches with `Command::current_dir(repo_root)`
/// while [`command_exists`] runs in the parent. A *relative* PATH component is
/// therefore resolved against two different directories: the parent's current
/// directory when probing, and `repo_root` when launching. An *empty* component
/// is the same defect in disguise — Unix `execvp` treats a zero-length entry as
/// the current directory, so it silently names `repo_root` at launch and the
/// parent's directory at probe time.
///
/// Only absolute components name the same directory from both processes, so
/// only absolute components are admitted. Dropping the rest also keeps a
/// repository-controlled file sitting in `repo_root` from being discovered — or
/// launched — as though it were an installed developer tool.
///
/// `Path::new("").is_absolute()` is false, so empty components are dropped by
/// the same rule that drops relative ones.
fn admissible_search_paths(path: Option<&OsStr>) -> Vec<PathBuf> {
    let Some(path) = path else {
        return Vec::new();
    };
    env::split_paths(path).filter(|dir| dir.is_absolute()).collect()
}

/// Bind the child's PATH to exactly the components [`command_exists`] searched,
/// so discovery and launch traverse one identical, cwd-independent list.
///
/// Without this the two disagree even when the probe is honest: with
/// `PATH=".:/usr/bin"` the probe admits `/usr/bin/tool` while a bare-name launch
/// under `current_dir(repo_root)` runs `repo_root/tool` instead.
///
/// When nothing is admissible the variable is *removed* rather than set to an
/// empty value, because an empty PATH means the current directory on Unix — the
/// `repo_root` candidate this policy refuses. Removing it is not by itself an
/// empty search list either: Unix `execvp` falls back to the platform's own
/// default directories when PATH is unset, so a bare name could still resolve
/// somewhere [`command_exists`] never looked. [`configure_child`] closes that
/// gap by refusing to launch a bare name at all when nothing is admissible.
fn apply_admissible_search_path(child: &mut Command, path: Option<&OsStr>) {
    match env::join_paths(admissible_search_paths(path)) {
        Ok(joined) if !joined.is_empty() => {
            child.env("PATH", joined);
        }
        // Either there is no admissible component, or one could not be
        // re-joined (a component containing the list separator). Fail closed.
        _ => {
            child.env_remove("PATH");
        }
    }
}

/// Whether `key` names the search-path variable on this platform.
///
/// Windows environment names are case-insensitive, so `Path` and `PATH` are the
/// same variable there. Unix names are exact.
fn is_search_path_key(key: &str) -> bool {
    #[cfg(windows)]
    {
        key.eq_ignore_ascii_case("PATH")
    }
    #[cfg(not(windows))]
    {
        key == "PATH"
    }
}

/// Shared child construction for every wrapper in this module.
///
/// The search-path policy governs exactly the launches whose resolution depends
/// on a search path, which is bare names only. An explicit command path is
/// resolved directly by the launch APIs, so its child keeps whatever `PATH` the
/// caller and environment gave it.
///
/// For a bare name the policy reads the *effective* search path — the value the
/// child would actually use, which is a caller-supplied `PATH` when one is given
/// and the inherited `PATH` otherwise. Evaluating only the inherited value would
/// reject a bare name that the caller's own absolute `PATH` resolves perfectly
/// well; evaluating only the caller's would let a relative component in an
/// explicitly configured `PATH` reach `repo_root`. One rule over the effective
/// value avoids both.
///
/// A bare name is refused when that effective path has no admissible component.
/// Removing PATH is not an empty search list — the platform substitutes its own
/// default directories — so launching anyway would run a tool that
/// [`command_exists`] reports absent, which is exactly the discovery/launch
/// disagreement this module exists to prevent.
fn configure_child(
    command: &str,
    repo_root: &Path,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<Command> {
    let mut child = Command::new(command);
    child.current_dir(repo_root).args(args);

    if !is_bare_name(command) {
        // An explicit path is resolved directly by the launch APIs, which never
        // consult a search path. There is no resolution here for the policy to
        // keep coherent, so the child's PATH is left exactly as the caller and
        // the environment set it — rewriting it would only strip entries the
        // command's own subprocesses were configured to use.
        for (key, value) in env_vars {
            child.env(key, value);
        }
        return Ok(child);
    }

    let inherited = env::var_os("PATH");
    // Later entries win, matching `Command::env`.
    let configured = env_vars
        .iter()
        .rev()
        .find(|(key, _)| is_search_path_key(key))
        .map(|(_, value)| OsStr::new(*value));
    let effective = configured.or(inherited.as_deref());

    if admissible_search_paths(effective).is_empty() {
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' is not available: the search path has no absolute \
             component, and a relative or empty component cannot be resolved to the \
             directory the child would run in"
        ));
    }

    for (key, value) in env_vars {
        // PATH is set from the effective value below so that one policy governs
        // it, whichever source supplied it.
        if !is_search_path_key(key) {
            child.env(key, value);
        }
    }
    apply_admissible_search_path(&mut child, effective);
    Ok(child)
}

/// Whether `command` is a bare name that the launch APIs will PATH-search.
///
/// A name carrying a path separator is resolved directly by `execvp` and by
/// `std`'s Windows `resolve_exe`, so no PATH traversal happens and the probe has
/// nothing to answer for. Backslash is a separator on Windows only; it is an
/// ordinary filename byte on Unix.
fn is_bare_name(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }
    #[cfg(windows)]
    {
        !command.contains(['\\', '/'])
    }
    #[cfg(not(windows))]
    {
        !command.contains('/')
    }
}

pub(crate) fn command_with_output(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if status != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {stderr}"
        ));
    }
    Ok(stdout)
}

pub(crate) fn command_with_output_all(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let status = output.status.code().unwrap_or(1);
    if status != 0 {
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {combined}"
        ));
    }
    Ok(combined)
}

pub(crate) fn command_with_input_with_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
    stdin_payload: &str,
) -> Result<(i32, String)> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = child.spawn().wrap_err_with(|| format!("running {command}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| color_eyre::eyre::eyre!("failed to open stdin for command {command}"))?;
        stdin
            .write_all(stdin_payload.as_bytes())
            .wrap_err_with(|| format!("writing to stdin for {command}"))?;
    }
    let output = child.wait_with_output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.stderr.is_empty() {
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok((status, combined))
}

pub(crate) fn command_output_with_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<(i32, String)> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((status, stdout))
}

pub(crate) fn command_timed_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<(i32, Duration)> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::null()).stderr(Stdio::null());
    let start = Instant::now();
    let status = child.status().wrap_err_with(|| format!("running {command}"))?;
    let elapsed = start.elapsed();
    Ok((status.code().unwrap_or(1), elapsed))
}

pub(crate) fn command_with_output_allow_empty_match(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    let status = output.status.code().unwrap_or(1);
    if status != 0 && status != 1 {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(color_eyre::eyre::eyre!(
            "command '{command}' failed (exit {status}): {stderr}"
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn command_with_output_allow_failure(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<String> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    child.stdout(Stdio::piped()).stderr(Stdio::piped());
    let output = child.output().wrap_err_with(|| format!("running {command}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn command_status(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<i32> {
    let mut child = configure_child(command, repo_root, args, env_vars)?;
    let status = child.status().wrap_err_with(|| format!("running {command}"))?;
    Ok(status.code().unwrap_or(1))
}

pub(crate) fn command_status_strict(
    repo_root: &Path,
    command: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> Result<()> {
    let status = command_status(repo_root, command, args, env_vars)?;
    if status != 0 {
        return Err(color_eyre::eyre::eyre!("{command} failed with code {status}"));
    }
    Ok(())
}

/// Whether a bare command name resolves on the admissible search path.
///
/// The answer is coherent with the launches this module performs on the same
/// search path: discovery and every wrapper traverse the same absolute-only
/// component list (see [`admissible_search_paths`] and
/// [`apply_admissible_search_path`]), so the probe cannot certify one candidate
/// while the child runs another.
///
/// Advisory limitations that remain, by design:
///
/// - The probe is not a guarantee. A candidate can be removed, replaced, or
///   lose permission between discovery and spawn; the real launch stays
///   authoritative for that race and for lifecycle failure.
/// - A relative or empty PATH component is never admitted, so a tool reachable
///   *only* through one is reported absent. Callers degrade to their
///   tool-unavailable branch, which is the honest answer for a component this
///   process cannot resolve to the same directory the child would.
/// - Explicit configured paths are not bare names and are always rejected here;
///   they carry their own trust and identity policy at the call site.
/// - The probe reads the *inherited* PATH. [`configure_child`] resolves a bare
///   name against the *effective* search path, which is a caller-supplied
///   `PATH` in `env_vars` when one is given. A wrapper call that supplies its
///   own `PATH` therefore launches against a search path this probe never
///   examined, and a preflight says nothing about it. No caller in this crate
///   supplies one — every `env_vars` entry here is unrelated to PATH — so the
///   two agree in practice; where they would not, the launch is authoritative
///   and refuses on its own terms.
pub(crate) fn command_exists(command: &str) -> bool {
    let path = env::var_os("PATH");
    command_exists_in_path(command, path.as_deref())
}

fn command_exists_in_path(command: &str, path: Option<&OsStr>) -> bool {
    if !is_bare_name(command) {
        return false;
    }
    let Some(path) = path else {
        return false;
    };

    #[cfg(windows)]
    {
        // Mirror std's *selection* before judging launchability: std's
        // `program_exists` is `GetFileAttributesW`-based and admits
        // directories and broken links, so std stops at the first
        // attribute-resolving candidate and the real launch then fails if
        // that subject is not a file. Selecting the same first candidate via
        // `symlink_metadata` (which likewise does not follow links) keeps the
        // probe from reporting a later PATH entry the launch would never
        // reach; the probe is true only when the selected subject is a
        // regular file, so directory and broken-link selections fail closed.
        windows_command_candidates(command, path)
            .into_iter()
            .find(|candidate| std::fs::symlink_metadata(candidate).is_ok())
            .is_some_and(|candidate| candidate.is_file())
    }
    #[cfg(not(windows))]
    {
        // Only admissible (absolute) components are searched: a relative or
        // empty component would be resolved here against the *parent's*
        // current directory while the eventual launch resolves it against the
        // child's `current_dir`.
        admissible_search_paths(Some(path)).into_iter().any(|dir| dir.join(command).is_file())
    }
}

/// Pure candidate generator for the Windows probe, mirroring the executable
/// search that `std::process::Command` performs for a bare file name, in PATH
/// order.
///
/// Authority: the pinned toolchain (Rust 1.95.0, `rust-toolchain.toml`),
/// `library/std/src/sys/process/windows.rs` (`resolve_exe` / `search_paths`):
///
/// - A bare name containing `.` anywhere is searched verbatim — the launch API
///   appends nothing, so `tool.cmd` probes `tool.cmd` only, never
///   `tool.cmd.exe`. An explicit `.bat`/`.cmd` name is the *only* way
///   `Command::new` reaches a script (std then routes it through `cmd.exe`).
/// - A bare name without `.` is searched only as `<name>.exe`. `PATHEXT` is
///   never consulted by `Command::new`, so it is not authority here either.
/// - Only the admissible (absolute) PATH components are searched. std skips
///   empty entries itself; relative entries are dropped by
///   [`admissible_search_paths`] because they would name the parent's current
///   directory here and the child's `current_dir` at launch.
/// - A name carrying a path separator is not PATH-searched by the launch API
///   at all, so the probe fails closed (no candidates) for it.
///
/// Deliberate scope limits versus the full launch route: std additionally
/// searches the application, system, and Windows directories after the child
/// PATH; this probe covers the PATH leg only, because every caller probes for
/// tools expected on PATH. Selection mirrors std's `GetFileAttributesW`-based
/// `program_exists` (first attribute-resolving candidate wins, directories and
/// broken links included); the caller in `command_exists_in_path` then fails
/// closed unless that selected subject is a regular file, matching the
/// eventual launch outcome. The spawn result remains authoritative for races
/// and lifecycle failure.
///
/// Compiled on every platform so the candidate rules are unit-tested off
/// Windows; the production caller exists only under `cfg(windows)`.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_command_candidates(command: &str, path: &OsStr) -> Vec<PathBuf> {
    if command.is_empty() || command.contains(['\\', '/']) {
        return Vec::new();
    }
    let has_extension = command.contains('.');
    admissible_search_paths(Some(path))
        .into_iter()
        .map(|dir| {
            let candidate = dir.join(command);
            if has_extension { candidate } else { candidate.with_extension("exe") }
        })
        .collect()
}

pub(crate) fn command_output_lines(output: &str) -> Vec<String> {
    output.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests;
