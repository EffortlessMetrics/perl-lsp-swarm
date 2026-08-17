//! Multi-line scope tracking for the regex-static ratchet.
//!
//! A `Regex::new(...)` / `RegexBuilder::new(...)` / `Regex::builder(...)` call is
//! allowed when it is reached from inside a lazily-evaluated static initializer
//! (`LazyLock`/`LazyCell`, or `OnceLock::get_or_init`),
//! because the regex is then compiled exactly once rather than per call.
//!
//! The existing hygiene ratchets are line-based scanners, so a `LazyLock::new(|| {
//! ... })` initializer that spans several lines needs a small stateful tracker —
//! this mirrors the `PrintAllowScope` precedent in `print_in_lib/allow_scopes.rs`,
//! but keyed on lazy-init openers and tracking `()`/`{}` delimiter depth (a
//! single-line `LazyLock::new(|| Regex::new(...))` uses parens, not braces).
//!
//! Delimiter counting and opener/constructor matching all run over [`code_only`] —
//! the line with string literals, char literals, and trailing `//` comments removed —
//! so brackets or the literal text `Regex::new(` appearing inside a string or comment
//! (very common in a regex-heavy codebase: `r"[{(]"`, help text, doc examples) never
//! corrupt the scope or produce a false violation.

/// Substrings that open a lazy / once-cell static initializer. A regex constructor
/// reached from inside one of these is compiled once and is therefore allowed.
const LAZY_INIT_TRIGGERS: [&str; 4] = [
    "LazyLock::new(", // std::sync::LazyLock (the only Lazy type after once_cell removal)
    "LazyCell::new(",
    ".get_or_init(", // OnceLock / OnceCell runtime-chosen pattern
    ".get_or_try_init(",
];

/// Returns `true` when the (already [`code_only`]-sanitized) line opens a lazy /
/// once-cell static initializer.
pub(super) fn line_opens_lazy_init(code: &str) -> bool {
    LAZY_INIT_TRIGGERS.iter().any(|trigger| code.contains(trigger))
}

/// Strip string literals, char literals, and a trailing `//` comment from a line,
/// leaving only code text. Delimiter counting and constructor matching run over the
/// result so literal/comment content can never affect detection.
///
/// Handles single-line `"..."` and raw (`r"..."`, `r#"..."#`) strings, char literals
/// (kept distinct from lifetimes like `'static`), and `//` comments. Multi-line
/// string literals are not tracked across lines — a rare case the baseline absorbs.
pub(super) fn code_only(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];

        // Trailing line comment `//` — the rest of the line is not code.
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        }

        // Raw string: r"..." or r#"..."# / r##"..."## …
        if c == 'r' {
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                // Consume the raw-string body up to the closing `"` + `hashes` `#`.
                i = skip_raw_string(&chars, j + 1, hashes);
                continue;
            }
        }

        // Normal string literal.
        if c == '"' {
            i = skip_normal_string(&chars, i + 1);
            continue;
        }

        // Char literal vs lifetime/label. A char literal is `'x'`, `'\n'`, `'\''`,
        // `'\u{7b}'`; a lifetime is `'static` (no closing quote soon after).
        if c == '\''
            && let Some(next) = skip_char_literal(&chars, i) {
                i = next; // literal dropped
                continue;
            }
            // Lifetime/label: keep the quote and continue (harmless — no delimiters).

        out.push(c);
        i += 1;
    }
    out
}

/// Given `start` pointing just past the opening `"` of a normal string, return the
/// index just past the closing `"` (or end of line if unterminated).
fn skip_normal_string(chars: &[char], start: usize) -> usize {
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2, // skip escape (and the escaped char)
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    i
}

/// Given `start` pointing just past the opening `"` of a raw string with `hashes`
/// hashes, return the index just past the closing `"###…` (or end of line).
fn skip_raw_string(chars: &[char], start: usize, hashes: usize) -> usize {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '"' {
            let closed = (1..=hashes).all(|k| chars.get(i + k) == Some(&'#'));
            if closed {
                return i + 1 + hashes;
            }
        }
        i += 1;
    }
    i
}

/// If a char literal starts at `quote` (a `'`), return the index just past its
/// closing `'`. Returns `None` for a lifetime/label (e.g. `'static`).
fn skip_char_literal(chars: &[char], quote: usize) -> Option<usize> {
    let first = *chars.get(quote + 1)?;
    if first == '\\' {
        // Escaped char literal: '\n', '\'', '\u{7b}', … — scan to the closing quote.
        let mut i = quote + 2;
        while i < chars.len() {
            if chars[i] == '\'' {
                return Some(i + 1);
            }
            i += 1;
        }
        return None;
    }
    // Simple char literal: exactly one char then a closing quote.
    if chars.get(quote + 2) == Some(&'\'') {
        return Some(quote + 3);
    }
    // Otherwise a lifetime/label like 'static or 'a.
    None
}

/// Net `()`/`{}` delimiter delta for a (already [`code_only`]-sanitized) line
/// (opens minus closes).
fn delim_delta(code: &str) -> i32 {
    let mut delta = 0;
    for ch in code.chars() {
        match ch {
            '(' | '{' => delta += 1,
            ')' | '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Tracks whether the current line sits inside a multi-line lazy-static initializer.
#[derive(Default)]
pub(super) struct LazyStaticScope {
    /// Net open-delimiter depth since a multi-line initializer began. `active` gates
    /// its meaning: when `active` is false, `depth` is always 0.
    depth: i32,
    active: bool,
}

impl LazyStaticScope {
    /// Whether a regex constructor on the given [`code_only`]-sanitized line is
    /// inside a lazy-static initializer.
    ///
    /// Checked *before* `observe_line`, so a single-line
    /// `LazyLock::new(|| Regex::new(...))` (which opens and closes on the same line)
    /// is covered by the `line_opens_lazy_init` arm even though it never activates
    /// the multi-line scope.
    pub(super) fn allows_current_line(&self, code: &str) -> bool {
        self.active || line_opens_lazy_init(code)
    }

    /// Advance the tracker past the given [`code_only`]-sanitized line.
    pub(super) fn observe_line(&mut self, code: &str) {
        if self.active {
            self.depth += delim_delta(code);
            if self.depth <= 0 {
                self.active = false;
                self.depth = 0;
            }
            return;
        }

        // A lazy initializer that leaves a delimiter open (delta > 0) spans
        // multiple lines; enter the multi-line scope. A balanced opener (delta == 0)
        // is fully self-contained on this line and needs no further tracking.
        if line_opens_lazy_init(code) {
            let delta = delim_delta(code);
            if delta > 0 {
                self.active = true;
                self.depth = delta;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_trigger() {
        assert!(
            line_opens_lazy_init("static R: LazyLock<Regex> = LazyLock::new(|| {"),
            "LazyLock::new is a trigger"
        );
        assert!(line_opens_lazy_init("LazyCell::new(|| foo())"), "LazyCell::new is a trigger");
        assert!(line_opens_lazy_init("LazyLock::new(|| bar())"), "LazyLock::new is a trigger");
        assert!(line_opens_lazy_init("CELL.get_or_init(|| baz())"), "get_or_init is a trigger");
        assert!(
            line_opens_lazy_init("CELL.get_or_try_init(|| baz())"),
            "get_or_try_init is a trigger"
        );
        assert!(
            !line_opens_lazy_init("let re = Regex::new(pat)?;"),
            "a bare Regex::new is not a trigger"
        );
    }

    #[test]
    fn delim_delta_counts_parens_and_braces() {
        assert_eq!(delim_delta("LazyLock::new(|| {"), 2, "open paren and open brace each add one");
        assert_eq!(delim_delta("});"), -2, "close brace and close paren each subtract one");
        assert_eq!(delim_delta("Regex::new(r).unwrap()"), 0, "balanced parens net to zero");
        assert_eq!(delim_delta("no delimiters here"), 0, "no delimiters net to zero");
    }

    #[test]
    fn single_line_lazy_is_allowed_but_does_not_activate() {
        let mut scope = LazyStaticScope::default();
        let code = code_only(
            "static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"x\").unwrap());",
        );
        assert!(scope.allows_current_line(&code), "single-line lazy regex is allowed");
        scope.observe_line(&code);
        assert!(!scope.active, "balanced single-line initializer must not activate scope");
    }

    #[test]
    fn multi_line_lazy_allows_inner_regex() {
        let mut scope = LazyStaticScope::default();

        let opener = code_only("static RE: LazyLock<Regex> = LazyLock::new(|| {");
        assert!(scope.allows_current_line(&opener), "the opener line itself is allowed");
        scope.observe_line(&opener);
        assert!(scope.active, "unbalanced opener activates the multi-line scope");

        let inner = code_only("    Regex::new(r\"...\").unwrap()");
        assert!(scope.allows_current_line(&inner), "regex inside the initializer is allowed");
        scope.observe_line(&inner);

        let closer = code_only("});");
        scope.observe_line(&closer);
        assert!(!scope.active, "closing delimiters end the scope");
    }

    #[test]
    fn bare_regex_after_scope_closes_is_not_allowed() {
        let mut scope = LazyStaticScope::default();
        for line in
            ["static RE: LazyLock<Regex> = LazyLock::new(|| {", "    Regex::new(r\"a\")", "});"]
        {
            scope.observe_line(&code_only(line));
        }
        let bare = code_only("    let re = Regex::new(user_input)?;");
        assert!(!scope.allows_current_line(&bare), "regex after the scope closed is a violation");
    }

    #[test]
    fn get_or_init_multi_line_allows_inner_regex() {
        let mut scope = LazyStaticScope::default();
        scope.observe_line(&code_only("    CELL.get_or_init(|| {"));
        assert!(scope.active, "get_or_init opener activates the multi-line scope");
        let inner = code_only("        Regex::new(pattern).unwrap()");
        assert!(scope.allows_current_line(&inner), "regex inside get_or_init is allowed");
    }

    // ── code_only: string / char / comment stripping ────────────────────────

    #[test]
    fn code_only_strips_normal_string() {
        // The braces and the literal "Regex::new(" live inside a string → dropped.
        assert_eq!(code_only(r#"let s = "Regex::new({(";"#), "let s = ;");
    }

    #[test]
    fn code_only_strips_raw_string_with_hashes() {
        assert_eq!(code_only(r##"let p = r#"a"{("#;"##), "let p = ;");
    }

    #[test]
    fn code_only_strips_trailing_line_comment() {
        assert_eq!(code_only("let x = 1; // Regex::new(foo) {("), "let x = 1; ");
    }

    #[test]
    fn code_only_drops_char_literal_but_keeps_lifetime() {
        // Char literal '{' is dropped (no spurious open brace)…
        assert_eq!(delim_delta(&code_only("let c = '{';")), 0, "char literal brace is not counted");
        // …but a lifetime is left intact and contributes no delimiters.
        assert_eq!(delim_delta(&code_only("fn f<'a>(x: &'a str) {}")), 0, "lifetime keeps balance");
    }

    #[test]
    fn code_only_preserves_real_code_delimiters() {
        assert_eq!(delim_delta(&code_only("fn f() { g(); }")), 0);
        assert_eq!(delim_delta(&code_only("LazyLock::new(|| {")), 2);
    }

    #[test]
    fn scope_not_activated_by_regex_in_comment() {
        // Bug fix: a comment mentioning the opener must not activate the scope.
        let mut scope = LazyStaticScope::default();
        scope.observe_line(&code_only("/// Example: LazyLock::new(|| {"));
        assert!(!scope.active, "a comment opener must not activate the scope");
    }

    #[test]
    fn scope_not_leaked_by_unbalanced_brace_in_string() {
        // Bug fix: an unbalanced brace inside a string inside the closure must not
        // corrupt depth tracking.
        let mut scope = LazyStaticScope::default();
        scope.observe_line(&code_only("static RE: LazyLock<Regex> = LazyLock::new(|| {"));
        assert!(scope.active);
        scope.observe_line(&code_only(r#"    let doc = "extra brace {";"#));
        scope.observe_line(&code_only("    Regex::new(r\"x\").unwrap()"));
        scope.observe_line(&code_only("});"));
        assert!(!scope.active, "the scope must close exactly once despite the string brace");
    }

    #[test]
    fn whole_line_comment_yields_no_code() {
        assert_eq!(code_only("   // Regex::new(fake) in a comment"), "   ");
        assert!(
            !code_only("   // Regex::new(fake)").contains("Regex::new("),
            "a commented ctor must not survive sanitization"
        );
    }
}
