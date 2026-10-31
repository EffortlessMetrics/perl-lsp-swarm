#[cfg(windows)]
use super::windows::{
    resolve_cmd_exe, resolve_windows_program, windows_has_expandable_percent_ref,
    windows_quote_for_cmd, windows_requires_cmd_shell,
};
use crate::SubprocessError;
use std::process::Command;

/// cmd.exe dynamic variables expanded on the `/C` command line even when absent
/// from the parent environment (command extensions enabled by default; we never
/// pass `/E:OFF`). See #17437.
#[cfg(any(windows, test))]
const CMD_DYNAMIC_VARIABLES: &[&str] = &[
    "CD",
    "DATE",
    "TIME",
    "RANDOM",
    "ERRORLEVEL",
    "CMDEXTVERSION",
    "CMDCMDLINE",
    "HIGHESTNUMANODENUMBER",
];

/// Whether cmd.exe would treat `name` as defined for `%NAME%` command-line expansion.
///
/// `env_lookup` mirrors `std::env::var_os(name).is_some()` at the production call
/// site; dynamic cmd.exe variables are accepted in addition (#17437).
#[cfg(any(windows, test))]
fn is_cmd_defined(name: &str, env_lookup: impl Fn(&str) -> bool) -> bool {
    if env_lookup(name) {
        return true;
    }
    CMD_DYNAMIC_VARIABLES.iter().any(|dynamic| dynamic.eq_ignore_ascii_case(name))
}

/// Resolve `(program, args)` into a spawnable [`Command`], failing closed on
/// Windows when the program cannot be resolved to a safe absolute path.
///
/// # Security: never fall back to the bare program name
///
/// On Windows, `Command::new(bare_name)` triggers CreateProcess's CWD-first
/// executable search, so a binary planted in the LSP workspace root would run in
/// preference to the real tool (binary-planting RCE — #2764).  The selector
/// `resolve_windows_program` already searches only absolute PATH directories and
/// excludes the CWD, returning `None` when nothing safe is found.  The hole this
/// closes is the *caller's* old `unwrap_or_else(|| program.to_string())`, which
/// restored the bare name on that `None` and re-armed the exact RCE the selector
/// had just prevented.  We now propagate an error instead — the tool is reported
/// missing rather than risking execution of a planted binary.  The same rule
/// applies to the `cmd.exe` used for `.bat`/`.cmd` wrappers (see
/// [`resolve_cmd_exe`]): it is resolved to an absolute path or refused.
///
/// # Windows `.bat`/`.cmd` wrappers: the `/C` payload must escape std's quoting
///
/// Batch wrappers are launched as `cmd.exe /D /V:OFF /S /C <payload>` where
/// `<payload>` is composed in *cmd.exe* quoting conventions
/// ([`windows_quote_for_cmd`]: per-token `"…"`, `""` doubling, `%` verbatim).
/// It is therefore appended with `CommandExt::raw_arg` — std's default MSVC
/// escaping would rewrite the payload's `"` into `\"`, which cmd.exe does not
/// understand, and after `/S` quote-stripping the program token became
/// `\"C:\…\tool.bat\"` → "is not recognized" (#17371).  The payload is wrapped
/// in one extra outer quote pair because `/S` mode strips the first character
/// (a `"`) and the last quote on the line, leaving exactly the per-token-quoted
/// command line cmd.exe must execute.  This is verified end-to-end by
/// `bat_wrapper_arguments_round_trip_through_real_cmd_spawn` in `crate::tests`.
///
/// `%` has no command-line escape in cmd.exe: a `%NAME%` span naming a defined
/// environment variable would be silently substituted before the child sees it,
/// so such arguments are refused (fail closed) rather than corrupted.
pub(crate) fn resolve_command_invocation(
    program: &str,
    args: &[&str],
) -> Result<Command, SubprocessError> {
    #[cfg(windows)]
    {
        let resolved_program = resolve_windows_program(program).ok_or_else(|| {
            SubprocessError::new(format!(
                "command not found in any absolute PATH directory \
                 (current directory excluded for security): {program}"
            ))
        })?;
        if windows_requires_cmd_shell(&resolved_program) {
            let cmd_exe = resolve_cmd_exe().ok_or_else(|| {
                SubprocessError::new(format!(
                    "cmd.exe not found via %SystemRoot%\\System32 or %ComSpec%; \
                     refusing to execute batch wrapper via a CWD-searchable bare \
                     name: {resolved_program}"
                ))
            })?;
            // cmd.exe expands a `%NAME%` span on the /C line when NAME is defined
            // in the environment or is one of cmd.exe's dynamic variables, and `%`
            // has no command-line escape. Pass such arguments through and the tool
            // silently receives the substituted text — fail closed with the
            // offending argument named instead (#17371, #17437).
            for arg in std::iter::once(resolved_program.as_str()).chain(args.iter().copied()) {
                if windows_has_expandable_percent_ref(arg, |name| {
                    is_cmd_defined(name, |n| std::env::var_os(n).is_some())
                }) {
                    return Err(SubprocessError::new(format!(
                        "argument contains a %...% reference that cmd.exe would substitute \
                         with an environment variable, and % has no command-line escape: \
                         {arg:?}"
                    )));
                }
            }
            let command_line = std::iter::once(resolved_program.as_str())
                .chain(args.iter().copied())
                .map(windows_quote_for_cmd)
                .collect::<Vec<_>>()
                .join(" ");
            // /D    - disable AutoRun registry commands.
            // /V:OFF  disable delayed expansion so that !VAR! patterns in
            //       arguments are not expanded even when the caller's
            //       environment has delayed expansion enabled.
            // /S    - strip the first character of the /C payload (a `"`) and
            //       the last quote on the line.  The payload therefore carries
            //       one extra outer quote pair around the per-token-quoted
            //       command line, and must be appended with `raw_arg` so the
            //       inner quotes reach cmd.exe unescaped (#17371).
            let mut cmd = Command::new(&cmd_exe);
            cmd.args(["/D", "/V:OFF", "/S", "/C"]);
            use std::os::windows::process::CommandExt as _;
            cmd.raw_arg(format!("\"{command_line}\""));
            // The absolute cmd.exe path — never the bare "cmd.exe", which would
            // itself be subject to the CWD-first CreateProcess search.
            return Ok(cmd);
        }
        let mut cmd = Command::new(&resolved_program);
        cmd.args(args);
        Ok(cmd)
    }
    #[cfg(not(windows))]
    {
        // Non-Windows: CreateProcess CWD-search semantics do not apply; the OS
        // resolves `program` via PATH (or as an explicit path) without a
        // CWD-first lookup, so pass through unchanged.
        let mut cmd = Command::new(program);
        cmd.args(args);
        Ok(cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::is_cmd_defined;

    #[test]
    fn is_cmd_defined_accepts_environment_via_lookup() {
        let env = ["MY_TOOL", "other"];
        let lookup = |name: &str| env.iter().any(|e| e.eq_ignore_ascii_case(name));
        assert!(is_cmd_defined("MY_TOOL", lookup));
        assert!(is_cmd_defined("my_tool", lookup));
        assert!(!is_cmd_defined("NOT_SET", lookup));
    }

    #[test]
    fn is_cmd_defined_accepts_cmd_dynamic_variables_without_env_entry() {
        let lookup = |_name: &str| false;
        for name in [
            "CD",
            "cd",
            "DATE",
            "TIME",
            "RANDOM",
            "ERRORLEVEL",
            "CMDEXTVERSION",
            "CMDCMDLINE",
            "HIGHESTNUMANODENUMBER",
        ] {
            assert!(
                is_cmd_defined(name, lookup),
                "{name} must be treated as cmd-defined even when absent from the environment"
            );
        }
        assert!(!is_cmd_defined("TEMP", lookup));
        assert!(!is_cmd_defined("f", lookup));
    }

    /// `%CD%` is expanded by cmd.exe even when `CD` is absent from the parent
    /// environment; the resolver must fail closed before spawn (#17437).
    #[cfg(windows)]
    #[test]
    fn resolve_fails_closed_on_dynamic_cd_percent_with_cd_unset() {
        use super::resolve_command_invocation;
        use crate::{OsSubprocessRuntime, SubprocessRuntime};

        let saved_cd = std::env::var_os("CD");
        std::env::remove_var("CD");

        let resolve_result = resolve_command_invocation(r"C:\tools\wrapper.bat", &["--label=%CD%"]);
        assert!(
            resolve_result.is_err(),
            "dynamic %CD% must fail closed at resolve; got: {resolve_result:?}"
        );
        let err = resolve_result.expect_err("must be Err");
        assert!(
            err.message.contains("--label=%CD%"),
            "error must name the offending argument; got: {}",
            err.message
        );

        let workspace =
            std::env::temp_dir().join(format!("cmd_dynamic_cd_guard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&workspace);
        std::fs::create_dir_all(&workspace).expect("create temp workspace");
        let probe = workspace.join("noop.bat");
        std::fs::write(&probe, "@echo off\r\necho ok\r\n").expect("write probe bat");

        let runtime = OsSubprocessRuntime::with_timeout(30);
        let spawn_result = runtime.run_command(
            probe.to_str().expect("temp path is UTF-8"),
            &["--label=%CD%"],
            None,
        );
        let _ = std::fs::remove_dir_all(&workspace);

        assert!(
            spawn_result.is_err(),
            "dynamic %CD% must fail closed at run_command; got: {spawn_result:?}"
        );
        let spawn_err = spawn_result.expect_err("must be Err");
        assert!(
            spawn_err.message.contains("--label=%CD%"),
            "spawn error must name the offending argument; got: {}",
            spawn_err.message
        );

        match saved_cd {
            Some(value) => std::env::set_var("CD", value),
            None => std::env::remove_var("CD"),
        }
    }
}
