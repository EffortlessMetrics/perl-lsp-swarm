//! Inline value variable scanning support.
//!
//! ## Limitation: regex-only scan (no AST cross-check)
//!
//! The inline-values handler scans source lines with a regex that matches
//! any sigil-prefixed identifier (`$scalar`, `@array`, `%hash`). It does **not**
//! cross-check against the AST, so variables that appear inside double-quoted
//! strings, comments, or POD blocks can produce inline-value entries the debug
//! client will try to resolve via DAP. The lookup may fail silently or return
//! stale values for variables not actually in scope at the breakpoint.
//!
//! To reduce false positives the handler skips Perl comments (`# ...`) and POD
//! blocks (`=pod` … `=cut`, `=begin` … `=end`, and single-paragraph `=for`),
//! but string-interpolated variables are still
//! matched because distinguishing interpolation from real code requires AST
//! context (see issue #4630, non-goal: AST-driven inline values).
//!
//! The feature is profile-gated: it ships only in `production` and `all`
//! profiles (see `flags.rs`), not in the default `ga_lock` profile.

use std::sync::LazyLock;

static INLINE_VALUE_REGEX: LazyLock<Result<regex::Regex, regex::Error>> =
    LazyLock::new(|| regex::Regex::new(r"([$@%])([a-zA-Z_][a-zA-Z0-9_]*)"));

/// Regex matching the start of a POD block (e.g. `=pod`, `=head1`, `=cut`).
static POD_DIRECTIVE_REGEX: LazyLock<Result<regex::Regex, regex::Error>> =
    LazyLock::new(|| regex::Regex::new(r"^=[a-zA-Z]"));

pub(super) fn inline_value_regex() -> Option<&'static regex::Regex> {
    INLINE_VALUE_REGEX.as_ref().ok()
}

/// Returns true if the line is a Perl comment (starts with `#` after optional
/// whitespace).
pub(super) fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('#')
}

/// Returns true if the line begins a POD directive (e.g. `=pod`, `=head1`,
/// `=cut`). POD blocks start with `=` at the beginning of a line followed by
/// an alphabetic character.
pub(super) fn is_pod_directive(line: &str) -> bool {
    POD_DIRECTIVE_REGEX.as_ref().map(|re| re.is_match(line)).unwrap_or(false)
}

fn is_for_directive(trimmed: &str) -> bool {
    trimmed == "=for" || trimmed.starts_with("=for ") || trimmed.starts_with("=for\t")
}

fn is_end_directive(trimmed: &str) -> bool {
    trimmed == "=end" || trimmed.starts_with("=end ") || trimmed.starts_with("=end\t")
}

/// Update open/closed POD-block state for a directive line.
///
/// `=cut` and `=end …` close a block; `=for …` is a single-paragraph directive
/// and does not leave the block open; all other POD directives open a block
/// until the next `=cut` or `=end`.
pub(super) fn update_pod_state(line: &str, in_pod: &mut bool) {
    if !is_pod_directive(line) {
        return;
    }

    let trimmed = line.trim_start();
    if trimmed.starts_with("=cut") {
        *in_pod = false;
    } else if is_end_directive(trimmed) {
        *in_pod = false;
    } else if is_for_directive(trimmed) {
        // Single-paragraph POD: skip the directive line only.
    } else {
        *in_pod = true;
    }
}
