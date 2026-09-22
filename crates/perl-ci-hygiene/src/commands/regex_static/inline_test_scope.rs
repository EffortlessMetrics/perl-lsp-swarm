//! Per-item inline-test classification for the regex-static ratchet.
//!
//! The old scanner truncated the whole file at the first `#[cfg(test)]` line, so an
//! early test-only item (a `use`, an enum, a `thread_local!`, …) hid all later
//! production code from the check. This tracker instead classifies each test-gated
//! item (or `mod`) individually: lines belonging to a test-only item are skipped,
//! and production scanning resumes as soon as the item ends.
//!
//! The flow mirrors [`LazyStaticScope`](super::lazy_scope::LazyStaticScope): callers
//! run detection over [`code_only`](super::lazy_scope::code_only)-sanitized text,
//! ask [`InlineTestScope::is_test_line`] *before* checking a line, then advance the
//! tracker with [`InlineTestScope::observe_line`] *after* (including on the
//! attribute line itself and on same-line items).
//!
//! State machine per file:
//!
//! * `pending`: a test attribute was seen and the item's first code line has not
//!   arrived yet. While pending, blank lines, further attribute lines, and
//!   comment-only / doc-comment lines (which `code_only` reduces to whitespace,
//!   so they read as blank) are skipped.
//! * attribute detection runs on the `code_only` text, `trim_start`ed:
//!   `#[cfg(test)]` marks a test-only item; `#[cfg(all(test, …))]` marks one only
//!   when the resolved item is a `mod` (mirroring `first_cfg_test_line_number`'s
//!   lookahead rule); `#![…]` inner attributes, `#[cfg_attr(…)]`,
//!   `#[cfg(not(test))]`, and `#[cfg(any(…))]` never match.
//! * same-line form (`#[cfg(test)] mod tests {`): non-attribute content following
//!   the attribute on the same line IS the item start. A bare `#[cfg(test)]` with
//!   nothing after it stays `pending`.
//! * item start: the first real code line while pending. Its net delimiter depth
//!   (via `delim_delta`) decides the span: depth-0 items (`use a::b;`,
//!   `struct X;`) open nothing, while braced items (`mod tests { … }`,
//!   `impl Drop for G { … }`, `thread_local! { … }`) span lines until their
//!   braces balance. Nested `#[cfg(test)]` lines inside an active item are simply
//!   skipped as part of it.
//! * EOF while pending or inside an item is fine: the scan just ends there.

use super::lazy_scope::delim_delta;

/// Which test gate opened a pending item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PendingAttr {
    /// A plain `#[cfg(test)]` attribute: the following item is test-only.
    Plain,
    /// A `#[cfg(all(test, …))]` attribute: test-only only when the resolved item
    /// is a `mod` declaration.
    AllTest,
}

/// Classify one leading `#…` attribute: plain test gate, all(test) gate, or other.
fn first_attr_kind(attr: &str) -> Option<PendingAttr> {
    if attr.starts_with("#![") {
        return None; // inner attribute: never a test gate by itself
    }
    if attr.starts_with("#[cfg(test)]") {
        return Some(PendingAttr::Plain);
    }
    if attr.starts_with("#[cfg(all(test") {
        return Some(PendingAttr::AllTest);
    }
    None
}

/// Split leading `#…` attributes off a `trim_start`ed code line, returning the
/// strongest test gate seen among them (plain wins over all(test)) and the
/// remainder after the last attribute (empty when the line holds nothing else).
fn split_leading_attributes(trimmed: &str) -> (Option<PendingAttr>, &str) {
    let mut rest = trimmed;
    let mut kind = None;
    loop {
        let current = rest.trim_start();
        if !current.starts_with('#') {
            return (kind, current);
        }
        match first_attr_kind(current) {
            Some(PendingAttr::Plain) => kind = Some(PendingAttr::Plain),
            // A plain gate seen earlier wins; otherwise record the all(test) gate.
            Some(PendingAttr::AllTest) => kind = kind.or(Some(PendingAttr::AllTest)),
            None => {}
        }
        match current.find(']') {
            Some(end) => rest = &current[end + 1..],
            None => return (kind, ""),
        }
    }
}

/// Returns `true` when the (already attribute-stripped, `trim_start`ed) item
/// start is a module declaration, mirroring `first_cfg_test_line_number`'s
/// `^\s*(?:pub\s+)?mod\s+` lookahead.
fn looks_like_mod_item(item: &str) -> bool {
    let rest = match item.strip_prefix("pub") {
        Some(after) if after.starts_with(char::is_whitespace) => after.trim_start(),
        _ => item,
    };
    match rest.strip_prefix("mod") {
        Some(after) => after.starts_with(char::is_whitespace),
        None => false,
    }
}

/// Tracks whether the current line belongs to a test-only item.
#[derive(Default)]
pub(super) struct InlineTestScope {
    /// Net open-delimiter depth since a multi-line test item began. `active`
    /// gates its meaning: when `active` is false, `depth` is always 0.
    depth: i32,
    active: bool,
    /// A test attribute was seen; the item's first code line has not arrived yet.
    pending: Option<PendingAttr>,
}

impl InlineTestScope {
    /// Whether the given [`code_only`](super::lazy_scope::code_only)-sanitized
    /// line belongs to test-only code and must be skipped by the scan.
    ///
    /// Checked *before* `observe_line`. A line that itself carries a test
    /// attribute is always a test line (the attribute gates its item); the
    /// `pending` / `active` arms cover the item-start and continuation lines,
    /// mirroring `LazyStaticScope::allows_current_line`'s opener arm for the
    /// single-line case.
    pub(super) fn is_test_line(&self, code: &str) -> bool {
        if self.active || self.pending.is_some() {
            return true;
        }
        split_leading_attributes(code.trim_start()).0.is_some()
    }

    /// Advance the tracker past the given
    /// [`code_only`](super::lazy_scope::code_only)-sanitized line.
    pub(super) fn observe_line(&mut self, code: &str) {
        if self.active {
            // Nested `#[cfg(test)]` lines inside an active test item need no
            // special handling: they are part of the item and depth accounting
            // continues over them.
            self.depth += delim_delta(code);
            if self.depth <= 0 {
                self.active = false;
                self.depth = 0;
            }
            return;
        }
        let (attr, rest) = split_leading_attributes(code.trim_start());
        // A fresh plain gate always wins; otherwise a new gate replaces the old
        // one, or the previous pending gate carries over attribute-free lines.
        let pending =
            if attr == Some(PendingAttr::Plain) || self.pending == Some(PendingAttr::Plain) {
                Some(PendingAttr::Plain)
            } else {
                attr.or(self.pending)
            };
        let Some(pending) = pending else {
            return; // production line with no test gate: nothing to track
        };
        if rest.is_empty() {
            // Bare attribute, blank line, further attribute line, or
            // comment-only line: keep awaiting the item's first code line.
            self.pending = Some(pending);
            return;
        }
        // First real code line while pending: this line starts the item.
        if pending == PendingAttr::Plain || looks_like_mod_item(rest) {
            self.pending = None;
            self.begin_item(code);
        } else {
            // `#[cfg(all(test, …))]` on a non-`mod` item (e.g. a lone `use` near
            // the top of a file): the item line itself was test-scoped, but
            // production scanning resumes immediately after it.
            self.pending = None;
        }
    }

    /// Enter a test item starting on the given sanitized line. Attribute text is
    /// delimiter-balanced, so measuring the whole line matches measuring just
    /// the item. A balanced (depth-0) item opens nothing; an unbalanced one
    /// spans lines until its delimiters balance.
    fn begin_item(&mut self, code: &str) {
        let delta = delim_delta(code);
        if delta > 0 {
            self.active = true;
            self.depth = delta;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lazy_scope::code_only;
    use super::*;

    /// Run lines through the before-check/after-observe split, returning the
    /// per-line `is_test_line` answers.
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
}
