//! Multi-line scope tracking for the regex-static ratchet.
//!
//! A `Regex::new(...)` / `RegexBuilder::new(...)` / `Regex::builder(...)` call is
//! allowed when it is reached from inside a lazily-evaluated static initializer
//! (`LazyLock`, `LazyCell`, `once_cell::sync::Lazy`, or `OnceLock::get_or_init`),
//! because the regex is then compiled exactly once rather than per call.
//!
//! The existing hygiene ratchets are line-based scanners, so a `LazyLock::new(|| {
//! ... })` initializer that spans several lines needs a small stateful tracker —
//! this mirrors the `PrintAllowScope` precedent in `print_in_lib/allow_scopes.rs`,
//! but keyed on lazy-init openers and tracking `()`/`{}` delimiter depth (a
//! single-line `LazyLock::new(|| Regex::new(...))` uses parens, not braces).

/// Substrings that open a lazy / once-cell static initializer. A regex constructor
/// reached from inside one of these is compiled once and is therefore allowed.
const LAZY_INIT_TRIGGERS: [&str; 5] = [
    "LazyLock::new(",
    "LazyCell::new(",
    "Lazy::new(",    // once_cell::sync::Lazy
    ".get_or_init(", // OnceLock / OnceCell runtime-chosen pattern
    ".get_or_try_init(",
];

/// Returns `true` when the line opens a lazy / once-cell static initializer.
pub(super) fn line_opens_lazy_init(line: &str) -> bool {
    LAZY_INIT_TRIGGERS.iter().any(|trigger| line.contains(trigger))
}

/// Returns `true` for lines that are entirely a `//` comment (indentation aside).
pub(super) fn line_is_whole_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

/// Net `()`/`{}` delimiter delta for a line (opens minus closes).
fn delim_delta(line: &str) -> i32 {
    let mut delta = 0;
    for ch in line.chars() {
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
    /// Whether a regex constructor on `line` is inside a lazy-static initializer.
    ///
    /// Checked *before* `observe_line`, so a single-line
    /// `LazyLock::new(|| Regex::new(...))` (which opens and closes on the same line)
    /// is covered by the `line_opens_lazy_init` arm even though it never activates
    /// the multi-line scope.
    pub(super) fn allows_current_line(&self, line: &str) -> bool {
        self.active || line_opens_lazy_init(line)
    }

    /// Advance the tracker past `line`.
    pub(super) fn observe_line(&mut self, line: &str) {
        if self.active {
            self.depth += delim_delta(line);
            if self.depth <= 0 {
                self.active = false;
                self.depth = 0;
            }
            return;
        }

        // A lazy initializer that leaves a delimiter open (delta > 0) spans
        // multiple lines; enter the multi-line scope. A balanced opener (delta == 0)
        // is fully self-contained on this line and needs no further tracking.
        if line_opens_lazy_init(line) {
            let delta = delim_delta(line);
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
        assert!(line_opens_lazy_init("static R: LazyLock<Regex> = LazyLock::new(|| {"));
        assert!(line_opens_lazy_init("LazyCell::new(|| foo())"));
        assert!(line_opens_lazy_init("Lazy::new(|| bar())"));
        assert!(line_opens_lazy_init("CELL.get_or_init(|| baz())"));
        assert!(line_opens_lazy_init("CELL.get_or_try_init(|| baz())"));
        assert!(!line_opens_lazy_init("let re = Regex::new(pat)?;"));
    }

    #[test]
    fn delim_delta_counts_parens_and_braces() {
        assert_eq!(delim_delta("LazyLock::new(|| {"), 2);
        assert_eq!(delim_delta("});"), -2);
        assert_eq!(delim_delta("Regex::new(r\"x\").unwrap()"), 0);
        assert_eq!(delim_delta("no delimiters here"), 0);
    }

    #[test]
    fn single_line_lazy_is_allowed_but_does_not_activate() {
        let mut scope = LazyStaticScope::default();
        let line = "static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r\"x\").unwrap());";
        assert!(scope.allows_current_line(line), "single-line lazy regex is allowed");
        scope.observe_line(line);
        // Balanced line: the multi-line scope must NOT stay active afterwards.
        assert!(!scope.active, "balanced single-line initializer must not activate scope");
    }

    #[test]
    fn multi_line_lazy_allows_inner_regex() {
        let mut scope = LazyStaticScope::default();

        let opener = "static RE: LazyLock<Regex> = LazyLock::new(|| {";
        assert!(scope.allows_current_line(opener));
        scope.observe_line(opener);
        assert!(scope.active, "unbalanced opener activates the multi-line scope");

        let inner = "    Regex::new(r\"...\").unwrap()";
        assert!(scope.allows_current_line(inner), "regex inside the initializer is allowed");
        scope.observe_line(inner);

        let closer = "});";
        scope.observe_line(closer);
        assert!(!scope.active, "closing delimiters end the scope");
    }

    #[test]
    fn bare_regex_after_scope_closes_is_not_allowed() {
        let mut scope = LazyStaticScope::default();
        for line in
            ["static RE: LazyLock<Regex> = LazyLock::new(|| {", "    Regex::new(r\"a\")", "});"]
        {
            scope.observe_line(line);
        }
        let bare = "    let re = Regex::new(user_input)?;";
        assert!(!scope.allows_current_line(bare), "regex after the scope closed is a violation");
    }

    #[test]
    fn get_or_init_multi_line_allows_inner_regex() {
        let mut scope = LazyStaticScope::default();
        let opener = "    CELL.get_or_init(|| {";
        scope.observe_line(opener);
        assert!(scope.active);
        let inner = "        Regex::new(pattern).unwrap()";
        assert!(scope.allows_current_line(inner));
    }

    #[test]
    fn whole_line_comment_detection() {
        assert!(line_is_whole_line_comment("   // Regex::new(fake) in a comment"));
        assert!(!line_is_whole_line_comment("let re = Regex::new(x); // trailing"));
    }
}
