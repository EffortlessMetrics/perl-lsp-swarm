/// A print-lint opt-out attribute, once its whole span has been read.
pub(super) struct PrintAllowAttr {
    /// `true` for the inner form (`#![expect(…)]`), which applies to the file.
    pub(super) inner: bool,
}

/// What one line contributes to an attribute being read across lines.
struct LineScan {
    /// Net `[` minus `]`, counting only brackets outside strings and comments.
    depth_delta: isize,
    /// The line's code, with a trailing `//` comment and the contents of
    /// string literals removed.
    code: String,
    /// The line uses a lexical form this scanner does not model, so its
    /// bracket count means nothing.
    unsupported: bool,
}

/// Splits a line into code and not-code.
///
/// Reading an attribute by where its text ends is wrong twice over: Rust
/// allows a line comment after the closing bracket (`#[allow(…)] // why`,
/// which this repository writes), and a `reason = "…"` string can contain a
/// bracket. Both are settled by looking at brackets in code only.
///
/// A raw string (`r#"…"#`) is not modelled, and rather than reason about which
/// way a miscount would fall, the scan reports it and the attribute containing
/// it is refused outright. What an unmodelled form does to the bracket count is
/// not established either way -- it could as easily over-admit as under-admit --
/// so refusing is the only direction this code actually proves.
fn scan_line(line: &str) -> LineScan {
    let unsupported = starts_raw_string(line);
    let mut depth_delta = 0isize;
    let mut code = String::new();
    let mut chars = line.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '/' if chars.peek() == Some(&'/') => break,
            '[' => {
                depth_delta += 1;
                code.push(ch);
            }
            ']' => {
                depth_delta -= 1;
                code.push(ch);
            }
            _ => code.push(ch),
        }
    }

    LineScan { depth_delta, code, unsupported }
}

/// Whether the line opens a raw string (`r"…"`, `r#"…"#`, `r##"…"##`, …).
///
/// Rust puts no limit on the run of `#` between the `r` and the quote, so a
/// fixed one-hash check answered `false` for every deeper form and let it
/// through to a bracket scanner that cannot read it. This counts the whole run.
///
/// The `b` and `c` prefixes take the same raw variant (`br#"…"#`, `cr#"…"#`)
/// and are recognised for the same reason: admitting more forms only ever
/// refuses more attributes, which is the safe direction here.
///
/// The prefix has to begin a token. Without that check `stderr"` reads as a raw
/// string opener, which refused an ordinary wrapped attribute whose reason
/// clause happened to end in `r`.
///
/// The scan walks characters and carries the previous one rather than slicing
/// by byte offset, so there is no index into the middle of a multi-byte
/// character for a non-ASCII reason clause to land on.
fn starts_raw_string(line: &str) -> bool {
    let mut previous: Option<char> = None;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        let begins_token =
            previous.is_none_or(|prior| !prior.is_ascii_alphanumeric() && prior != '_');
        previous = Some(ch);
        if !begins_token {
            continue;
        }

        let mut lookahead = chars.clone();
        let mut marker = ch;
        if matches!(ch, 'b' | 'c') {
            let Some(next) = lookahead.next() else {
                continue;
            };
            marker = next;
        }
        if marker != 'r' {
            continue;
        }

        while lookahead.peek() == Some(&'#') {
            lookahead.next();
        }
        if lookahead.peek() == Some(&'"') {
            return true;
        }
    }

    false
}

/// An attribute currently being read, possibly across several lines.
struct OpenAttr {
    /// The inner form (`#![…]`), which applies to the whole file.
    inner: bool,
    /// Whether the attribute's own top-level path is `allow` or `expect`.
    admitted: bool,
    /// Whether a print lint has been named anywhere inside it.
    names_print_lint: bool,
    /// Unclosed `[` brackets.
    depth: isize,
}

/// Reads attributes a line at a time, joining the ones rustfmt has wrapped.
///
/// Three spellings defeated the substring matcher this replaces:
///
/// - `#[expect(…)]`, the form this repository actually writes and the
///   stricter one, since rustc warns when the lint it names never fires;
/// - an attribute rustfmt wrapped, because a `reason = "…"` clause made the
///   line long, so the opener and the lint name are on different lines;
/// - `#[allow(clippy::print_stdout)] // why`, where the closing bracket is
///   not the end of the line.
///
/// And one spelling it admitted that it should not have:
/// `#![cfg_attr(test, allow(clippy::print_stdout))]` names a lint setting
/// that applies under `cfg(test)` only. Admitting it exempted four
/// production files, `perl-lsp-rs-core/src/lib.rs` among them, in every
/// configuration. Only an unconditional `allow` or `expect` counts now:
/// admission reads the attribute's own top-level path rather than searching
/// its arguments.
#[derive(Default)]
pub(super) struct AttrJoiner {
    open: Option<OpenAttr>,
}

impl AttrJoiner {
    /// `true` while an attribute is still being read across lines.
    pub(super) fn in_attribute(&self) -> bool {
        self.open.is_some()
    }

    /// Feeds one line. Returns the attribute that ends on it, when that
    /// attribute is an unconditional opt-out naming a print lint.
    pub(super) fn feed(&mut self, line: &str) -> Option<PrintAllowAttr> {
        let scan = scan_line(line);
        let trimmed = scan.code.trim();

        let mut attr = match self.open.take() {
            Some(open) => open,
            None => {
                let inner = trimmed.starts_with("#![");
                if !inner && !trimmed.starts_with("#[") {
                    return None;
                }
                let path: String = trimmed
                    .trim_start_matches(['#', '!', '['])
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                    .collect();
                OpenAttr {
                    inner,
                    admitted: path == "allow" || path == "expect",
                    names_print_lint: false,
                    depth: 0,
                }
            }
        };

        attr.names_print_lint |= scan.code.contains("clippy::print_");
        // One unmodelled line spoils the whole attribute: the depth it
        // contributed is untrustworthy, so no part of this attribute can
        // grant an opt-out however the rest of it reads.
        attr.admitted &= !scan.unsupported;
        attr.depth += scan.depth_delta;

        if attr.depth <= 0 {
            return (attr.admitted && attr.names_print_lint)
                .then_some(PrintAllowAttr { inner: attr.inner });
        }

        self.open = Some(attr);
        None
    }
}

/// Returns `true` when a source file should be skipped wholesale by the print-macro check.
///
/// Files with an unconditional file-level `#![expect(clippy::print_stderr)]`
/// or `#![allow(clippy::print_stdout)]` attribute have been explicitly opted
/// out of the rule (e.g. `cli.rs` in the LSP binary crate). The attribute
/// must appear in the first 30 lines of the file (the module-doc / crate-doc
/// block). A `cfg_attr`-conditioned allowance is not an opt-out.
pub(super) fn file_has_print_allow(lines: &[String]) -> bool {
    let mut joiner = AttrJoiner::default();
    lines.iter().take(30).filter_map(|line| joiner.feed(line)).any(|attr| attr.inner)
}

pub(super) fn line_is_whole_line_comment(line: &str) -> bool {
    line.trim_start().starts_with("//")
}

#[derive(Default)]
pub(super) struct PrintAllowScope {
    pending_attr: bool,
    active_brace_depth: usize,
}

impl PrintAllowScope {
    pub(super) fn note_attribute(&mut self) {
        self.pending_attr = true;
    }

    pub(super) fn allows_current_line(&self) -> bool {
        self.pending_attr || self.active_brace_depth > 0
    }

    pub(super) fn observe_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() || line_is_whole_line_comment(line) || trimmed.starts_with("#[") {
            return;
        }

        if self.active_brace_depth > 0 {
            self.apply_brace_delta(line);
            return;
        }

        if self.pending_attr {
            let delta = brace_delta(line);
            if delta > 0 {
                self.pending_attr = false;
                self.active_brace_depth = delta as usize;
            } else if line.contains('{') || line.trim_end().ends_with(';') {
                // The attributed item ended on this line: either it opened and
                // closed its body here (`fn banner() { eprintln!("…"); }`, whose
                // net brace delta is zero), or it has no body at all (`use`, a
                // `const`). Either way the attribute is spent. Leaving it pending
                // exempted every later item until something else happened to
                // change the state.
                self.pending_attr = false;
            }
            // Otherwise the item's signature is still being read. A function whose
            // parameters or `where` clause wrap opens its brace several lines below
            // the attribute, and dropping the attribute on the first of those lines
            // lost the scope before the body it was written for ever started.
        }
    }

    fn apply_brace_delta(&mut self, line: &str) {
        let delta = brace_delta(line);
        if delta.is_negative() {
            self.active_brace_depth = self.active_brace_depth.saturating_sub(delta.unsigned_abs());
        } else {
            self.active_brace_depth = self.active_brace_depth.saturating_add(delta as usize);
        }
    }
}

fn brace_delta(line: &str) -> isize {
    let opens = line.chars().filter(|ch| *ch == '{').count() as isize;
    let closes = line.chars().filter(|ch| *ch == '}').count() as isize;
    opens - closes
}

#[cfg(test)]
mod tests {
    use color_eyre::eyre::{Result, ensure};

    use super::*;

    // ── allows_current_line discriminators ───────────────────────────────────

    #[test]
    fn allows_current_line_pending_attr_only_is_true() {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        // pending_attr=true, active_brace_depth=0 → true
        assert!(scope.allows_current_line());
    }

    #[test]
    fn allows_current_line_active_brace_depth_only_is_true() {
        let mut scope = PrintAllowScope::default();
        // Simulate entering a brace scope: note_attribute then observe a
        // line that opens a brace.  After observe_line, pending_attr is
        // consumed and active_brace_depth becomes 1.
        scope.note_attribute();
        scope.observe_line("pub fn foo() {");
        // pending_attr=false, active_brace_depth=1 → allows_current_line=true
        assert!(!scope.pending_attr, "pending_attr should have been consumed");
        assert_eq!(scope.active_brace_depth, 1, "brace should be open");
        assert!(scope.allows_current_line(), "active_brace_depth > 0 → true");
    }

    #[test]
    fn allows_current_line_neither_flag_is_false() {
        let scope = PrintAllowScope::default();
        // pending_attr=false, active_brace_depth=0 → false
        assert!(!scope.allows_current_line());
    }

    // ── observe_line: active_brace_depth branch ──────────────────────────────

    #[test]
    fn observe_line_while_in_brace_scope_applies_delta() {
        let mut scope = PrintAllowScope::default();
        // Enter brace scope via pending_attr path.
        scope.note_attribute();
        scope.observe_line("fn foo() {"); // depth becomes 1
        assert_eq!(scope.active_brace_depth, 1);
        // Observe a line that adds one more open brace.
        scope.observe_line("if true {"); // depth becomes 2
        assert_eq!(scope.active_brace_depth, 2);
        // Observe a closing brace — depth decrements.
        scope.observe_line("}"); // depth becomes 1
        assert_eq!(scope.active_brace_depth, 1);
    }

    #[test]
    fn observe_line_pending_attr_consumed_and_depth_set() {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        assert!(scope.pending_attr);
        scope.observe_line("fn foo() {");
        // pending_attr cleared, active_brace_depth opened
        assert!(!scope.pending_attr);
        assert_eq!(scope.active_brace_depth, 1);
    }

    #[test]
    fn observe_line_skips_empty_comment_and_attr_lines() {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        // These lines must not consume pending_attr
        scope.observe_line("   "); // whitespace-only
        scope.observe_line("// comment"); // whole-line comment
        scope.observe_line("#[inline]"); // attribute line
        assert!(scope.pending_attr, "pending_attr should survive non-consuming lines");
    }

    // ── apply_brace_delta negative delta ─────────────────────────────────────

    #[test]
    fn apply_brace_delta_decrements_depth_on_close() {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        scope.observe_line("fn foo() {"); // pending_attr consumed; depth = 1
        assert_eq!(scope.active_brace_depth, 1);
        scope.observe_line("}"); // apply_brace_delta: depth → 0
        assert_eq!(scope.active_brace_depth, 0);
    }

    #[test]
    fn apply_brace_delta_saturates_at_zero_on_excess_closes() {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        // Open two braces in a single line.
        scope.observe_line("fn foo() { if true {"); // depth = 2
        assert_eq!(scope.active_brace_depth, 2);
        // Close three braces — more than the depth; should saturate at 0.
        scope.observe_line("} } }"); // depth → 0, not underflow
        assert_eq!(scope.active_brace_depth, 0);
    }

    // ── brace_delta edge cases ────────────────────────────────────────────────

    #[test]
    fn brace_delta_balanced_line_returns_zero() {
        assert_eq!(brace_delta("let x = {1};"), 0);
    }

    #[test]
    fn brace_delta_open_only() {
        assert_eq!(brace_delta("fn foo() {"), 1);
    }

    #[test]
    fn brace_delta_close_only() {
        assert_eq!(brace_delta("}"), -1);
    }

    // ── file-level allow discriminators ──────────────────────────────────────

    #[test]
    fn file_has_print_allow_detects_inner_print_stderr() {
        let lines: Vec<String> = vec!["#![allow(clippy::print_stderr)]".to_owned()];
        assert!(file_has_print_allow(&lines));
    }

    #[test]
    fn file_has_print_allow_detects_inner_print_stdout() {
        let lines: Vec<String> = vec!["#![allow(clippy::print_stdout)]".to_owned()];
        assert!(file_has_print_allow(&lines));
    }

    #[test]
    fn file_has_print_allow_only_checks_first_30_lines() {
        let mut lines: Vec<String> = (0..30).map(|_| "// plain comment".to_owned()).collect();
        lines.push("#![allow(clippy::print_stderr)]".to_owned()); // line 31
        assert!(!file_has_print_allow(&lines));
    }

    /// Feeds each line and returns the attributes that completed, in order.
    fn joined(lines: &[&str]) -> Vec<bool> {
        let mut joiner = AttrJoiner::default();
        lines.iter().filter_map(|line| joiner.feed(line)).map(|attr| attr.inner).collect()
    }

    #[test]
    fn a_single_line_outer_allow_completes_as_outer() -> Result<()> {
        ensure!(
            joined(&["#[allow(clippy::print_stderr, clippy::print_stdout)]"]) == vec![false],
            "an outer allow naming print lints completes as outer"
        );
        Ok(())
    }

    #[test]
    fn a_single_line_inner_allow_completes_as_inner() -> Result<()> {
        ensure!(
            joined(&["#![allow(clippy::print_stderr)]"]) == vec![true],
            "an inner allow naming a print lint completes as inner"
        );
        Ok(())
    }

    #[test]
    fn an_expect_attribute_counts_the_same_as_an_allow() -> Result<()> {
        // `#[expect]` is what this repository actually writes, and it is the
        // stricter spelling: rustc warns when the lint never fires. A matcher
        // that knew only `#[allow(` reported every one of them as an offender.
        ensure!(
            joined(&[r#"#[expect(clippy::print_stdout, reason = "CLI report")]"#]) == vec![false],
            "`expect` opts out exactly as `allow` does"
        );
        Ok(())
    }

    #[test]
    fn an_attribute_wrapped_by_rustfmt_is_read_as_one() -> Result<()> {
        // rustfmt splits this as soon as the reason clause makes the line long,
        // so the opener and the lint name never share a line.
        ensure!(
            joined(&[
                "#[expect(",
                "    clippy::print_stderr,",
                r#"    reason = "batch CLI unit — diagnostics intentionally use stderr""#,
                ")]",
            ]) == vec![false],
            "a wrapped attribute is joined into one"
        );
        Ok(())
    }

    #[test]
    fn a_wrapped_inner_attribute_keeps_its_inner_form() -> Result<()> {
        ensure!(
            joined(&[
                "#![expect(",
                "    clippy::print_stdout,",
                r#"    reason = "CLI output module""#,
                ")]",
            ]) == vec![true],
            "joining a wrapped attribute must not lose its inner form"
        );
        Ok(())
    }

    #[test]
    fn a_trailing_comment_does_not_hold_the_attribute_open() -> Result<()> {
        // Rust allows a line comment after the closing bracket, and this
        // repository writes them. Deciding closure from the line's last two
        // characters left the attribute open, and everything after it was
        // skipped -- often to end of file.
        let mut joiner = AttrJoiner::default();
        let done = joiner.feed("#[allow(clippy::print_stdout)] // test-only diagnostics");
        ensure!(done.is_some(), "the attribute closes at its bracket, not at the line end");
        ensure!(!joiner.in_attribute(), "nothing stays open to swallow the following lines");
        Ok(())
    }

    #[test]
    fn a_bracket_inside_a_reason_string_does_not_hold_it_open() -> Result<()> {
        let mut joiner = AttrJoiner::default();
        let done = joiner.feed(r#"#[expect(clippy::print_stdout, reason = "renders [rows]")]"#);
        ensure!(done.is_some(), "a bracket inside a string is not a bracket");
        ensure!(!joiner.in_attribute(), "the attribute closed on its own line");
        Ok(())
    }

    #[test]
    fn an_attribute_written_with_a_raw_string_reason_is_refused() -> Result<()> {
        // The scanner does not model `r#"…"#`, so the bracket count on that
        // line means nothing. Refusing is the only direction provable here:
        // an unmodelled form could over-admit just as easily as under-admit,
        // and an over-admitting opt-out silences real prints.
        ensure!(
            joined(&[r##"#[expect(clippy::print_stdout, reason = r#"renders [rows]"#)]"##])
                .is_empty(),
            "an attribute carrying an unmodelled lexical form must not opt out"
        );
        Ok(())
    }

    #[test]
    fn a_reason_clause_ending_in_r_is_not_read_as_a_raw_string() -> Result<()> {
        // The paired half of the control above. `starts_raw_string` has to
        // require that the `r` begins a token: without that, the `r"` inside
        // `stderr"` reads as a raw-string opener and refuses an ordinary
        // attribute, which is the over-refusing direction of the same defect.
        ensure!(
            joined(&[r#"#[expect(clippy::print_stderr, reason = "writes to stderr")]"#])
                == vec![false],
            "a reason clause whose text ends in `r` still opts out"
        );
        Ok(())
    }

    #[test]
    fn a_raw_string_reason_is_refused_at_every_delimiter_depth() -> Result<()> {
        // Rust puts no limit on the run of `#` between the `r` and the quote.
        // A fixed one-hash check answered `false` for every deeper form, so
        // the lines the refusal above promises to catch reached the bracket
        // scanner anyway -- the promise held at depth 0 and 1 and nowhere else.
        for hashes in 0..4 {
            let fence = "#".repeat(hashes);
            let line = format!(
                r#"#[expect(clippy::print_stdout, reason = r{fence}"renders [rows]"{fence})]"#
            );
            ensure!(
                joined(&[line.as_str()]).is_empty(),
                "a raw string at delimiter depth {hashes} must not opt out"
            );
        }
        Ok(())
    }

    #[test]
    fn a_byte_or_c_string_raw_prefix_is_refused_at_depth_too() -> Result<()> {
        // `br#"…"#` and `cr#"…"#` are the same unmodelled form wearing a
        // prefix, and they carry the same arbitrary delimiter run.
        for prefix in ["br", "cr"] {
            for hashes in 0..3 {
                let fence = "#".repeat(hashes);
                let line = format!(
                    r#"#[expect(clippy::print_stdout, reason = {prefix}{fence}"renders [rows]"{fence})]"#
                );
                ensure!(
                    joined(&[line.as_str()]).is_empty(),
                    "a `{prefix}` raw string at depth {hashes} must not opt out"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_hash_run_that_no_quote_closes_is_not_a_raw_string() -> Result<()> {
        // The paired half of the depth controls, and the reason counting the
        // run is safe: the count still has to end at a quote. A scanner that
        // stopped at the run would read an issue reference in a reason clause
        // as an opener and refuse ordinary source -- the over-refusing
        // direction of the same defect.
        for hashes in 1..4 {
            let fence = "#".repeat(hashes);
            let line =
                format!(r#"#[expect(clippy::print_stderr, reason = "see r{fence}16232 [rows]")]"#);
            ensure!(
                joined(&[line.as_str()]) == vec![false],
                "a `#` run of {hashes} that no quote follows still opts out"
            );
        }
        Ok(())
    }

    #[test]
    fn a_raw_prefix_inside_a_word_is_not_an_opener_at_any_depth() -> Result<()> {
        // The token-boundary rule has to survive the deeper forms as well:
        // `substr##"` carries a real delimiter run and a real quote, and is
        // still an identifier followed by a string, not a raw string.
        for hashes in 0..4 {
            let fence = "#".repeat(hashes);
            for word in ["substr", "verb", "sync"] {
                let line = format!(r#"{word}{fence}"x""#);
                ensure!(
                    !starts_raw_string(&line),
                    "`{word}` with a run of {hashes} is an identifier, not a raw string"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn a_cfg_attr_conditioned_allowance_is_not_an_opt_out() -> Result<()> {
        // `#![cfg_attr(test, allow(clippy::print_stdout))]` names a lint
        // setting that applies under `cfg(test)` only. Admitting it on the
        // strength of the substring `allow(` exempted four production files,
        // `perl-lsp-rs-core/src/lib.rs` among them, in every configuration.
        ensure!(
            joined(&["#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]"])
                .is_empty(),
            "a conditional allowance completes as no opt-out at all"
        );
        let lines = vec!["#![cfg_attr(test, allow(clippy::print_stdout))]".to_owned()];
        ensure!(!file_has_print_allow(&lines), "a conditional allowance does not exempt the file");
        Ok(())
    }

    #[test]
    fn an_unconditional_allowance_beside_it_still_opts_out() -> Result<()> {
        // The paired half of the control above: tightening admission must not
        // stop recognising the spelling that does opt out.
        let lines = vec![
            "#![cfg_attr(test, allow(clippy::print_stdout))]".to_owned(),
            "#![allow(clippy::print_stdout)]".to_owned(),
        ];
        ensure!(file_has_print_allow(&lines), "the unconditional allowance still exempts the file");
        Ok(())
    }

    #[test]
    fn an_attribute_on_a_balanced_one_line_item_is_spent_there() -> Result<()> {
        // `#[expect(…)] fn banner() { eprintln!("…"); }` has a net brace delta
        // of zero and no trailing semicolon, so the attribute stayed pending
        // and exempted every later item in the file.
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        scope.observe_line("fn banner() { eprintln!(\"hi\"); }");
        ensure!(!scope.allows_current_line(), "the item ended, so the attribute is spent");
        Ok(())
    }

    #[test]
    fn a_later_item_after_a_balanced_one_line_item_is_not_exempt() -> Result<()> {
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        scope.observe_line("fn allowed() { eprintln!(\"ok\"); }");
        scope.observe_line("fn offender() {");
        ensure!(!scope.allows_current_line(), "the next function carries no attribute of its own");
        Ok(())
    }

    #[test]
    fn an_attribute_that_names_no_print_lint_completes_as_nothing() -> Result<()> {
        ensure!(
            joined(&["#[expect(clippy::too_many_lines)]"]).is_empty(),
            "an attribute naming another lint grants no print opt-out"
        );
        Ok(())
    }

    #[test]
    fn an_attribute_that_never_closes_opts_nothing_out() -> Result<()> {
        // The conservative direction for a gate: a print the scanner cannot
        // prove is intentional is one it still reports.
        let mut joiner = AttrJoiner::default();
        ensure!(joiner.feed("#[expect(").is_none(), "an unclosed attribute completes nothing");
        ensure!(
            joiner.feed("    clippy::print_stderr,").is_none(),
            "naming the lint does not close the attribute"
        );
        ensure!(joiner.in_attribute(), "the attribute is still being read");
        Ok(())
    }

    #[test]
    fn an_attribute_scope_survives_a_wrapped_signature() -> Result<()> {
        // `#[expect(…)] fn run_cli<I, S>(args: I) -> i32 where …` opens its brace
        // several lines below the attribute. Spending the attribute on the first
        // of those lines lost the scope before the body it was written for began.
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        for line in
            ["pub fn run_cli<I, S>(args: I) -> i32", "where", "    I: IntoIterator<Item = S>,"]
        {
            scope.observe_line(line);
            ensure!(scope.allows_current_line(), "scope lost at `{line}`");
        }
        scope.observe_line("    S: Into<String>, {");
        ensure!(scope.allows_current_line(), "the body the attribute was written for is exempt");
        Ok(())
    }

    #[test]
    fn an_attribute_on_a_statement_item_is_spent_at_its_semicolon() -> Result<()> {
        // Without this the pending attribute would leak down the rest of the file.
        let mut scope = PrintAllowScope::default();
        scope.note_attribute();
        scope.observe_line("use std::io::Write;");
        ensure!(!scope.allows_current_line(), "the statement ended, so the attribute is spent");
        Ok(())
    }

    #[test]
    fn line_is_whole_line_comment_true_for_double_slash() {
        assert!(line_is_whole_line_comment("  // this is a comment"));
    }

    #[test]
    fn line_is_whole_line_comment_false_for_code() {
        assert!(!line_is_whole_line_comment("let x = 1; // inline comment"));
    }
}
