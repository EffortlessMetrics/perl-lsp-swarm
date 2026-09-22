//! Per-item inline-test classification for the regex-static ratchet.
//!
//! The old scanner truncated the whole file at the first `#[cfg(test)]` line, so an
//! early test-only item (a `use`, an enum, a `thread_local!`, …) hid all later
//! production code from the check. This tracker instead classifies each test-gated
//! item (or `mod`) individually: lines belonging to a test-only item are skipped,
//! and production scanning resumes as soon as the item ends.
//!
//! The flow mirrors [`LazyStaticScope`](super::lazy_scope::LazyStaticScope): callers
//! run detection over [`LineSanitizer`](super::lazy_scope::LineSanitizer)-sanitized
//! text (one sanitizer per file, shared with the lazy scope), ask
//! [`InlineTestScope::is_test_line`] *before* checking a line, then advance the
//! tracker with [`InlineTestScope::observe_line`] *after* (including on the
//! attribute line itself and on same-line items).
//!
//! State machine per file:
//!
//! * `gate_pending`: a test attribute was seen and the item's first code line has
//!   not arrived yet. While pending, blank lines, further attribute lines, block
//!   comments (which the sanitizer reduces to whitespace, so they read as blank),
//!   and multi-line attribute continuations are skipped.
//! * `open_no_scope`: the item's first code line was consumed but no `{`/`[`
//!   stands open yet — a bare signature (`fn helper()`), a `mod tests` whose brace
//!   sits on the next line, a `where` clause, or an attribute fragment (`Debug,`).
//!   Further code lines keep waiting until a scope opens or the item terminates.
//! * `active`/`depth`: the item opened a `{`/`[` scope; lines are test-only until
//!   the matching `}`/`]` balance restores depth to zero. `;` never terminates an
//!   active item.
//! * a gate is `#[cfg(test)]`, or `#[cfg(all(test` with the next character `,`,
//!   `)`, or whitespace — so `#[cfg(all(test, unix))]` gates while
//!   `#[cfg(all(testing))]` does not. `#![…]` inner attributes, `#[cfg_attr(…)]`,
//!   `#[cfg(not(test))]`, and `#[cfg(any(…))]` never match.
//! * same-line form (`#[cfg(test)] mod tests {`): non-attribute content following
//!   the attribute on the same line IS the item start (via
//!   `split_leading_attributes`). A bare `#[cfg(test)]` with nothing after it
//!   stays `gate_pending`.
//! * item start resolution: trimmed-empty, pure-attribute (`#[`-leading), and
//!   `]`-closer-only (`)]`) lines are trivia and keep waiting; a line with net
//!   item depth > 0 opens the `active` span; a depth-0 line with `;` terminates
//!   (braceless item); a brace-bearing line with depth <= 0 terminates (balanced
//!   inline item such as `fn f() {}`); anything else (bare signatures, `where`
//!   clauses, attribute fragments) keeps waiting and moves `gate_pending` to
//!   `open_no_scope`.
//! * item depth counts `{`/`}` and `[`/`]` only — parens never open or close an
//!   item scope, so multi-line signatures and `where` clauses cannot end it, and
//!   `test_cases![ … ]` macro bodies are covered. (The shared `delim_delta` keeps
//!   its paren counting for `LazyStaticScope`, whose lazy-init closers end with
//!   `});`.)
//! * `plain #[cfg(test)]` and `#[cfg(all(test, …))]` gate identically: every
//!   `cfg(all(test, …))` item is absent from production builds (the predicate
//!   requires `test = true`), so the whole item is skipped to its end, while
//!   balanced single-line items (a gated `use`) still resume immediately.
//! * EOF while pending, awaiting, or inside an item is fine: the scan just ends.
//!
//! Known line-based limitations (inherent to the ratchet, absorbed by the
//! baseline): a production suffix on the SAME line as a balanced test item (or on
//! its closing line, which is skipped wholesale) escapes scanning — unreachable in
//! practice because CI enforces rustfmt (one statement per line), and matching the
//! documented same-line-opener limitation of `LazyStaticScope`. Const-generic
//! braces inside signatures (`Foo<{N}>`) are likewise counted as scope
//! delimiters. Multi-line normal `"…"` strings are not tracked across lines (only
//! raw strings and block comments carry state); a `}` inside one could corrupt
//! depth, and the baseline absorbs the residue.

/// Net `{`/`}`/`[`/`]` delimiter delta for a (sanitized) line (opens minus
/// closes). Parens are deliberately ignored: they must never close an item
/// scope (multi-line signatures, `where` clauses), while brackets must open one
/// (`test_cases![ … ]` macro bodies).
fn item_delim_delta(code: &str) -> i32 {
    let mut delta = 0;
    for ch in code.chars() {
        match ch {
            '{' | '[' => delta += 1,
            '}' | ']' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Whether a `trim_start`ed attribute is a test gate: `#[cfg(test)]`, or
/// `#[cfg(all(test` with the next character `,`, `)`, or whitespace (so
/// `all(testing` / `all(test_style` never match).
fn is_test_gate_attr(attr: &str) -> bool {
    if attr.starts_with("#![") {
        return false; // inner attribute: never a test gate by itself
    }
    if attr.starts_with("#[cfg(test)]") {
        return true;
    }
    if let Some(rest) = attr.strip_prefix("#[cfg(all(test")
        && let Some(next) = rest.chars().next()
    {
        return next == ',' || next == ')' || next.is_whitespace();
    }
    false
}

/// Split leading `#…` attributes off a `trim_start`ed code line, returning
/// whether any test gate was seen and the remainder after the last attribute
/// (empty when the line holds nothing else, or when an attribute is
/// unresolvable because its `]` never arrives, e.g. `#[derive(`).
fn split_leading_attributes(trimmed: &str) -> (bool, &str) {
    let mut rest = trimmed;
    let mut gate = false;
    loop {
        let current = rest.trim_start();
        if !current.starts_with('#') {
            return (gate, current);
        }
        if is_test_gate_attr(current) {
            gate = true;
        }
        match current.find(']') {
            Some(end) => rest = &current[end + 1..],
            None => return (gate, ""),
        }
    }
}

/// Whether a (non-empty) attribute-stripped remainder is only the closers of a
/// multi-line attribute (`)]`), i.e. trivia that must not resolve the item.
fn is_closer_only(remainder: &str) -> bool {
    !remainder.is_empty()
        && remainder.chars().all(|c| c == ']' || c == ')' || c == ',' || c.is_whitespace())
}

/// Tracks whether the current line belongs to a test-only item.
#[derive(Default)]
pub(super) struct InlineTestScope {
    /// Net open brace/bracket depth since a multi-line test item began. `active`
    /// gates its meaning: when `active` is false, `depth` is always 0.
    depth: i32,
    active: bool,
    /// A test attribute was seen; the item's first code line has not arrived yet.
    gate_pending: bool,
    /// The item started but no `{`/`[` stands open yet (bare signature, `where`
    /// clause, attribute fragment, brace on a later line).
    open_no_scope: bool,
}

impl InlineTestScope {
    /// Whether the given (sanitized) line belongs to test-only code and must be
    /// skipped by the scan.
    ///
    /// Checked *before* `observe_line`. A line that itself carries a test
    /// attribute is always a test line (the attribute gates its item); the
    /// waiting / `active` arms cover the item-start and continuation lines,
    /// mirroring `LazyStaticScope::allows_current_line`'s opener arm for the
    /// single-line case.
    pub(super) fn is_test_line(&self, code: &str) -> bool {
        if self.active || self.gate_pending || self.open_no_scope {
            return true;
        }
        split_leading_attributes(code.trim_start()).0
    }

    /// Advance the tracker past the given (sanitized) line.
    pub(super) fn observe_line(&mut self, code: &str) {
        if self.active {
            // `;` is ignored while active: only the item delta moves depth.
            self.depth += item_delim_delta(code);
            if self.depth <= 0 {
                self.active = false;
                self.depth = 0;
            }
            return;
        }
        let (fresh_gate, rest) = split_leading_attributes(code.trim_start());
        if !fresh_gate && !self.gate_pending && !self.open_no_scope {
            return; // production line with no test gate: nothing to track
        }
        if rest.is_empty() || is_closer_only(rest) {
            // Bare attribute, blank line, comment-only line, further attribute
            // line, or multi-line attribute closer: keep awaiting the item.
            // A fresh gate (re)opens the wait; otherwise flags carry over.
            if fresh_gate {
                self.gate_pending = true;
                self.open_no_scope = false;
            }
            return;
        }
        // First real code line for the gated item (same-line remainder after a
        // gate, or a waiting line): resolve the item span.
        let delta = item_delim_delta(code);
        let has_brace = code.contains('{') || code.contains('}');
        let has_semi = code.contains(';');
        if delta > 0 {
            self.active = true;
            self.depth = delta;
            self.gate_pending = false;
            self.open_no_scope = false;
        } else if delta == 0 && has_semi {
            // Braceless item (`use a::b;`, `struct X;`): complete on this line.
            self.gate_pending = false;
            self.open_no_scope = false;
        } else if has_brace && delta <= 0 {
            // Balanced inline item (`fn f() {}`, `mod m {}`): complete.
            self.gate_pending = false;
            self.open_no_scope = false;
        } else {
            // Bare signature, `mod` name, `where` clause, or attribute fragment:
            // the item started but no scope stands open yet — keep waiting.
            self.gate_pending = false;
            self.open_no_scope = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lazy_scope::{LineSanitizer, code_only};
    use super::*;

    /// Run lines through the before-check/after-observe split, returning the
    /// per-line `is_test_line` answers (single-line sanitization).
    fn scan_lines(lines: &[&str]) -> Vec<bool> {
        let mut scope = InlineTestScope::default();
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let code = code_only(line);
            out.push(scope.is_test_line(&code));
            scope.observe_line(&code);
        }
        out
    }

    /// Run lines through a shared stateful sanitizer (multi-line lexical state),
    /// returning the per-line `is_test_line` answers.
    fn scan_sanitized(lines: &[&str]) -> Vec<bool> {
        let mut sanitizer = LineSanitizer::default();
        let mut scope = InlineTestScope::default();
        let mut out = Vec::with_capacity(lines.len());
        for line in lines {
            let code = sanitizer.sanitize(line);
            out.push(scope.is_test_line(&code));
            scope.observe_line(&code);
        }
        out
    }

    #[test]
    fn trailing_mod_tests_excluded_then_scan_resumes() {
        let verdicts = scan_lines(&[
            "pub fn safe() {}",
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    use regex::Regex;",
            "    #[test]",
            "    fn t() {",
            "        let _ = Regex::new(r\"x\").unwrap();",
            "    }",
            "}",
            "pub fn after() {}",
        ]);
        assert_eq!(
            verdicts,
            [
                false, false, // production before the gate
                true, true, true, true, true, true, true, true,  // gate + module body
                false, // production resumes once the module closes
            ]
        );
    }

    #[test]
    fn early_test_use_then_production_resume() {
        let verdicts = scan_lines(&[
            "use regex::Regex;",
            "#[cfg(test)]",
            "use std::cell::Cell;",
            "pub fn matches(pat: &str) -> bool {",
            "    Regex::new(pat).is_match(\"\")",
            "}",
        ]);
        assert_eq!(verdicts, [false, true, true, false, false, false]);
    }

    #[test]
    fn same_line_cfg_test_use_is_depth_zero_item() {
        let verdicts = scan_lines(&[
            "use regex::Regex;",
            "#[cfg(test)] use std::cell::Cell;",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [false, true, false]);
    }

    #[test]
    fn same_line_cfg_test_mod_opens_module() {
        let verdicts = scan_lines(&[
            "pub fn safe() {}",
            "#[cfg(test)] mod tests {",
            "    fn helper() {}",
            "}",
            "pub fn after() {}",
        ]);
        assert_eq!(verdicts, [false, true, true, true, false]);
    }

    #[test]
    fn interleaved_thread_local_and_impl_drop_items() {
        // The symbols.rs shape: test-only items interleaved with production code.
        let verdicts = scan_lines(&[
            "use regex::Regex;",
            "#[cfg(test)]",
            "use std::cell::Cell;",
            "pub fn first() {}",
            "#[cfg(test)]",
            "#[derive(Clone, Copy)]",
            "enum Fault {",
            "    Cancelled,",
            "}",
            "pub fn second() {}",
            "#[cfg(test)]",
            "thread_local! {",
            "    static FAULT: Cell<bool> = const { Cell::new(false) };",
            "}",
            "pub fn third() {}",
            "#[cfg(test)]",
            "#[must_use]",
            "struct Guard;",
            "#[cfg(test)]",
            "impl Drop for Guard {",
            "    fn drop(&mut self) {}",
            "}",
            "pub fn fourth() {}",
        ]);
        assert_eq!(
            verdicts,
            [
                false, // use regex::Regex;
                true, true,  // gate + test-only use
                false, // first
                true, true, true, true, true,  // gate + derive + enum body
                false, // second
                true, true, true, true,  // gate + thread_local! block
                false, // third
                true, true, true, // gate + must_use + struct (depth-0)
                true, true, true, true,  // gate + impl Drop block
                false, // fourth
            ]
        );
    }

    #[test]
    fn cfg_all_test_mod_excluded_but_use_not_excluding() {
        // `#[cfg(all(test, …))] mod …` is test-only …
        let mod_verdicts = scan_lines(&[
            "#[cfg(all(test, feature = \"x\"))]",
            "mod tests {",
            "    fn helper() {}",
            "}",
            "pub fn after() {}",
        ]);
        assert_eq!(mod_verdicts, [true, true, true, true, false]);

        // … while a lone `#[cfg(all(test, …))] use …` excludes only its own line.
        let use_verdicts = scan_lines(&[
            "#[cfg(all(test, not(target_arch = \"wasm32\")))] use crate::TestHelper;",
            "pub fn prod() {}",
        ]);
        assert_eq!(use_verdicts, [true, false]);
    }

    #[test]
    fn blank_and_comment_lines_between_gate_and_item_stay_pending() {
        let verdicts = scan_lines(&[
            "#[cfg(test)]",
            "",
            "// a comment explaining the test helper",
            "/// doc comment on the test item",
            "use std::cell::Cell;",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, false]);
    }

    #[test]
    fn non_test_attributes_never_match() {
        for line in [
            "#[cfg_attr(test, allow(dead_code))]",
            "#[cfg(not(test))]",
            "#[cfg(any(test, feature = \"x\"))]",
            "#![allow(dead_code)]",
            "#[derive(Debug)]",
        ] {
            let verdicts = scan_lines(&[line, "pub fn prod() {}"]);
            assert_eq!(verdicts, [false, false], "line must not gate: {line}");
        }
    }

    #[test]
    fn cfg_test_with_other_leading_attribute_still_gates() {
        let verdicts =
            scan_lines(&["#[allow(dead_code)] #[cfg(test)]", "struct Helper;", "pub fn prod() {}"]);
        assert_eq!(verdicts, [true, true, false]);
    }

    #[test]
    fn string_and_comment_content_does_not_corrupt_state() {
        // A production line merely mentioning the attribute inside a string or
        // comment must not open a pending gate …
        let verdicts = scan_lines(&[
            "pub fn docs() -> &'static str { \"example: #[cfg(test)]\" }",
            "// #[cfg(test)] in a comment",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [false, false, false]);

        // … and braces inside strings within a test item must not corrupt depth.
        let verdicts = scan_lines(&[
            "#[cfg(test)]",
            "mod tests {",
            "    fn t() { let s = \"unbalanced { brace\"; }",
            "}",
            "pub fn after() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, false]);
    }

    #[test]
    fn eof_while_pending_or_inside_item_just_ends() {
        let verdicts = scan_lines(&["pub fn prod() {}", "#[cfg(test)]"]);
        assert_eq!(verdicts, [false, true]);

        let verdicts = scan_lines(&["#[cfg(test)]", "mod tests {", "    fn t() {}"]);
        assert_eq!(verdicts, [true, true, true]);
    }

    // ── Round-2 regression unit tests (one per confirmed thread) ────────────

    #[test]
    fn cfg_all_testing_prefix_is_not_a_gate() {
        // T3/T5: `all(testing`, `all(test_style`, … must not gate.
        for line in [
            "#[cfg(all(testing))] mod optional {",
            "#[cfg(all(test_style))] mod optional {",
            "#[cfg(all(testing, unix))] mod optional {",
        ] {
            assert!(!is_test_gate_attr(line), "must not gate: {line}");
            let verdicts = scan_lines(&[line, "pub fn prod() {}"]);
            assert_eq!(verdicts, [false, false], "line must not gate: {line}");
        }
    }

    #[test]
    fn cfg_all_test_boundary_chars_gate() {
        // T3/T5: `,`, `)`, and whitespace after `all(test` are gates.
        for line in ["#[cfg(all(test, unix))]", "#[cfg(all(test))]", "#[cfg(all(test ))]"] {
            assert!(is_test_gate_attr(line), "must gate: {line}");
        }
        // No boundary character at all is not a gate.
        assert!(!is_test_gate_attr("#[cfg(all(test"));
    }

    #[test]
    fn brace_on_next_line_keeps_item_waiting() {
        // T1: `mod tests` with the brace on the next line stays test-only.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)]",
            "mod tests",
            "{",
            "    fn t() { let _ = 1; }",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, false]);
    }

    #[test]
    fn multiline_signature_with_where_clause_keeps_waiting() {
        // T1: parens never close the item; the `where` line and the brace wait.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)]",
            "fn helper<T>(",
            "    arg: T,",
            ") where T: Clone",
            "{",
            "    let _ = 1;",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, true, true, false]);
    }

    #[test]
    fn paren_heavy_signature_does_not_close_scope() {
        // T1/T8: once active, only braces/brackets move depth — `)` lines stay in.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)] fn helper() {",
            "    let x = foo(a, (b, c));",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, false]);
    }

    #[test]
    fn multiline_attribute_continuation_keeps_gate_open() {
        // T6: `Debug,` and `)]` must not resolve the item early.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)]",
            "#[derive(",
            "    Debug,",
            ")]",
            "fn helper() { let _ = 1; }",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, false]);
    }

    #[test]
    fn block_comment_between_gate_and_item_stays_pending() {
        // T2: a block comment after the gate is trivia, not the item.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)]",
            "/* test helper */",
            "fn helper() { let _ = 1; }",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, false]);
    }

    #[test]
    fn block_comment_depth_persists_across_lines() {
        // T7: a `}` inside a multi-line block comment must not close the module.
        let mut sanitizer = LineSanitizer::default();
        let codes: Vec<String> = ["mod tests {", "/*", "}", "*/", "fn t() { let _ = 1; }", "}"]
            .iter()
            .map(|line| sanitizer.sanitize(line))
            .collect();
        assert_eq!(codes[1], "");
        assert_eq!(codes[2], "");
        assert!(codes[4].contains("fn t()"));

        let verdicts = scan_sanitized(&[
            "#[cfg(test)] mod tests {",
            "/*",
            "}",
            "*/",
            "    fn t() { let _ = 1; }",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, true, false]);
    }

    #[test]
    fn raw_string_state_persists_across_lines() {
        // T7: a `}` inside a multi-line raw string must not close the module.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)] mod tests {",
            "    let s = r#\"",
            "}",
            "\"#;",
            "    fn t() { let _ = 1; }",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, true, true, true, false]);
    }

    #[test]
    fn bracket_macro_body_is_item_scope() {
        // T8: `test_cases![` opens the item scope until `];`.
        let verdicts = scan_sanitized(&[
            "#[cfg(test)] test_cases![",
            "    case_one,",
            "];",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, false]);
    }

    #[test]
    fn cfg_all_test_fn_body_skipped_to_end() {
        // T0: every `cfg(all(test, …))` item — not just `mod` — spans to its end.
        let verdicts = scan_sanitized(&[
            "#[cfg(all(test, unix))] fn helper() {",
            "    let _ = 1;",
            "}",
            "pub fn prod() {}",
        ]);
        assert_eq!(verdicts, [true, true, true, false]);
    }

    #[test]
    fn same_line_suffix_after_balanced_item_escapes() {
        // T4 (pinned limitation, NOT a bug): a production suffix sharing the
        // line with a balanced test item escapes — the whole line is test-only.
        // CI rustfmt (one statement per line) keeps this unreachable; the
        // baseline absorbs residuals.
        let verdicts = scan_lines(&["#[cfg(test)] struct Helper;", "pub fn prod() {}"]);
        assert_eq!(verdicts, [true, false]);
        // The suffix form below is skipped wholesale, so an embedded production
        // constructor would escape counting.
        let verdicts =
            scan_lines(&["#[cfg(test)] struct Helper; pub fn prod() {}", "pub fn next() {}"]);
        assert_eq!(verdicts, [true, false]);
    }
}
