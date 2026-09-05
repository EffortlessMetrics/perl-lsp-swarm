//! Bare-name availability probing for external tools.
//!
//! A live consumer asks "is this tool available?" before advertising a
//! capability, choosing an implementation, or emitting a "tool not installed"
//! diagnostic.  That question is a *trust* decision, not a convenience: the
//! answer decides whether a workspace-supplied file can influence product
//! behavior.
//!
//! The admission policy here is the one [`crate::os_runtime`] already applies
//! when it resolves a program for launch — only **absolute** `PATH` components
//! are searched, and a candidate sitting in the current directory is excluded —
//! so an availability probe cannot certify a subject the resolver would refuse.
//!
//! # Why the current directory is not searchable
//!
//! `std::env::split_paths` yields an empty component for `PATH=""`, `":"`, or a
//! trailing separator, and an empty or relative component names *this process's*
//! current directory.  For a language server that directory is routinely the
//! opened workspace, i.e. content the user has not vouched for.  A probe that
//! joins a bare tool name onto it cannot distinguish an installed tool from a
//! planted file (#2764 / #3028).
//!
//! # Deliberate limitations
//!
//! - **Not a guarantee.** A candidate can be removed, replaced, or lose
//!   permission between the probe and the spawn.  The launch stays
//!   authoritative for that race.
//! - **Stricter than the non-Windows launch path.** `Command::new` on Unix uses
//!   `execvp`, which honors relative and empty `PATH` components.  A tool
//!   reachable *only* through such a component is reported absent here.  That
//!   is the fail-closed direction and the honest answer for a component this
//!   probe cannot resolve to the same directory the child would; callers
//!   degrade to their tool-unavailable branch.
//! - **Bare names only.** A path-bearing input is not a `PATH` lookup and is
//!   refused; an explicit configured path carries its own identity and trust
//!   policy at the call site.

#[cfg(not(windows))]
use std::ffi::OsStr;
#[cfg(not(windows))]
use std::path::Path;

/// Whether a bare command name is available under the admission policy
/// described in the [module documentation](self).
///
/// Reads the inherited `PATH` and the process current directory.  Returns
/// `false` for a path-bearing input, for an absent `PATH`, and whenever no
/// admissible candidate survives — refusing is safer than certifying a subject
/// the resolver would not launch.
pub fn command_exists(command: &str) -> bool {
    #[cfg(windows)]
    {
        // The Windows resolver already implements this exact policy, including
        // the executable-extension rules `CreateProcess` applies.  Reuse it
        // rather than growing a second candidate generator.
        crate::os_runtime::resolve_windows_program_pub(command).is_some()
    }
    #[cfg(not(windows))]
    {
        let Ok(cwd) = std::env::current_dir() else {
            // Without a known current directory the CWD-exclusion layer cannot
            // be applied, so no candidate can be admitted.
            return false;
        };
        command_exists_in(command, std::env::var_os("PATH").as_deref(), &cwd)
    }
}

/// Pure availability decision over explicit inputs.
///
/// Extracted so the policy is provable without mutating process-global state:
/// the live consumer crates deny `unsafe_code`, and `std::env::set_var` is
/// `unsafe` in the pinned toolchain, so an environment-reading probe cannot be
/// exercised from their tests at all.
///
/// Cross-platform so the invariant is observable on Linux CI runners; on
/// Windows the production route is [`command_exists`]'s resolver arm, which
/// applies the same two layers plus the `CreateProcess` extension rules.
#[cfg(not(windows))]
pub(crate) fn command_exists_in(command: &str, path: Option<&OsStr>, cwd: &Path) -> bool {
    if command.is_empty() {
        return false;
    }
    // A name carrying a separator is not PATH-searched; it is a caller-resolved
    // location with its own trust policy, so it fails closed here. `/` is the
    // only separator on the platforms this arm compiles for.
    if command.contains('/') {
        return false;
    }
    let Some(path) = path else {
        return false;
    };

    let candidates: Vec<String> = std::env::split_paths(path)
        .filter_map(|component| {
            // Layer 1 — component admission, decided on the component as
            // written. Only absolute components are searched.
            //
            // This is not redundant with the selector's absolute-only check.
            // The selector sees resolved candidates, and a relative component
            // resolves to a directory *under* the current directory, so the
            // resulting candidate is absolute and indistinguishable from an
            // installed tool: `PATH=tools` yields `<cwd>/tools/<command>`,
            // whose parent is not `cwd`, so CWD exclusion alone would admit it.
            if !component.is_absolute() {
                return None;
            }
            // Resolving against `cwd` rather than the ambient process directory
            // keeps this function a faithful model of the search: for an
            // absolute component the join is the identity, and the pure seam
            // stays exercisable with a caller-supplied working directory.
            Some(cwd.join(component).join(command))
        })
        .filter(|candidate| is_executable_file(candidate))
        .filter_map(|candidate| candidate.to_str().map(str::to_string))
        .collect();

    let candidate_refs: Vec<&str> = candidates.iter().map(String::as_str).collect();
    // Layer 2 — CWD exclusion, applied by the shared selector so this probe and
    // the launch resolver cannot drift apart.
    crate::os_runtime::select_path_candidate(&candidate_refs, cwd).is_some()
}

/// Whether `candidate` is a regular file the current user may execute.
///
/// Mirrors what a `PATH` search means: a readable non-executable file of the
/// right name is not a usable tool, and reporting it available would move the
/// failure from the probe to a spawn the caller cannot explain.
#[cfg(not(windows))]
fn is_executable_file(candidate: &Path) -> bool {
    if !candidate.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        candidate
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests;
