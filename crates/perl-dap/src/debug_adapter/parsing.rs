//! Debugger output parsing: normalize, infer types, parse stack frames, parse variables.

mod scope_variables;

#[cfg(test)]
use super::stack_frame_re;
use super::{
    CachedVariable, DebugAdapter, HashMap, PerlStackParser, PerlVariableRenderer, RenderedVariable,
    Source, StackFrame, Variable, VariableParser, VariableRenderer, ansi_escape_re,
    is_internal_frame_name_and_path, prompt_re,
};
use crate::parse_origin::{DebuggerOutputOrigin, OriginatedParseInput, ParseIdentity};
use crate::value::PerlValue;

impl DebugAdapter {
    /// Normalize debugger output lines for deterministic parsing by:
    /// - removing ANSI escape sequences
    /// - stripping all debugger prompt prefixes (e.g. `DB<1>`, `DB<2>`)
    ///
    /// A single output line from `perl -d` may contain multiple consecutive
    /// prompt tokens when the debugger processes several commands between stops
    /// (e.g. `  DB<1>   DB<2> main::(/path/file.pl:5):`). All prompts must
    /// be stripped so that the context pattern can match the tail of the line.
    pub(super) fn normalize_debugger_output_line(line: &str) -> String {
        let mut normalized = if let Some(re) = ansi_escape_re() {
            re.replace_all(line, "").into_owned()
        } else {
            line.to_string()
        };

        // Strip all occurrences of DB<N> prompt tokens from the line.
        while let Some(prompt_start) = normalized.find("DB<")
            && let Some(prompt_end) = normalized[prompt_start..].find('>')
        {
            let content_start = prompt_start + prompt_end + 1;
            normalized = normalized[content_start..].to_string();
        }

        normalized.trim().to_string()
    }

    /// Normalize one debugger output line for `x`-dump parsing.
    ///
    /// Like [`Self::normalize_debugger_output_line`] it strips ANSI escapes
    /// and `DB<N>` prompt tokens, but it preserves leading indentation: indent
    /// depth is the only signal separating a dump's top-level elements from
    /// the nested referent contents indented beneath them. Prompt tokens only
    /// ever prefix the top-level dump line, so a prompted line loses its
    /// remaining leading whitespace with them.
    pub(super) fn normalize_x_dump_line(line: &str) -> String {
        let mut normalized = if let Some(re) = ansi_escape_re() {
            re.replace_all(line, "").into_owned()
        } else {
            line.to_string()
        };

        let mut had_prompt = false;
        while let Some(prompt_start) = normalized.find("DB<")
            && let Some(prompt_end) = normalized[prompt_start..].find('>')
        {
            had_prompt = true;
            let content_start = prompt_start + prompt_end + 1;
            normalized = normalized[content_start..].to_string();
        }
        if had_prompt {
            normalized = normalized.trim_start().to_string();
        }

        normalized.trim_end().to_string()
    }

    /// Decompose one normalized `x`-dump line into `(indent, ordinal, payload)`.
    ///
    /// perl5db renders array/list elements as `<ordinal><two spaces><payload>`
    /// and nested hash entries as `<indent>'key' => payload`. Returns `None`
    /// for any other line shape so callers can reject non-dump output.
    fn x_dump_line_parts(text: &str) -> Option<(usize, Option<u64>, String)> {
        let indent = text.len() - text.trim_start_matches(' ').len();
        let rest = &text[indent..];

        if let Some(payload) = rest.strip_prefix('\'').and_then(|after_open| {
            let close = after_open.find('\'')?;
            after_open[close + 1..].strip_prefix(" => ")
        }) {
            return Some((indent, None, payload.to_string()));
        }

        let digits_end = rest.find(|character: char| !character.is_ascii_digit())?;
        if digits_end == 0 || !rest[digits_end..].starts_with("  ") {
            return None;
        }
        let ordinal = rest[..digits_end].parse::<u64>().ok()?;
        Some((indent, Some(ordinal), rest[digits_end + 2..].to_string()))
    }

    /// Reshape perl5db `x`-command dump lines into the single logical value
    /// text the evaluate response should present, with a coarse type name.
    ///
    /// perl5db's `x` renders its result as a numbered dump: one
    /// `<ordinal><two spaces><payload>` line per top-level element, with a
    /// nested referent's contents on indented follow-up lines. Presenting
    /// those lines verbatim made every scalar evaluate to an ordinal-prefixed
    /// string (`0  5` for `$a + $b`) and multi-element dumps collapse to their
    /// final dump line (`2  3` for `(1, 2, 3)`) (#17244).
    ///
    /// The reshape handles the unambiguous dump shape: a single element keeps
    /// its exact perl5db payload (`5`, `'hello'`, `undef`), and a flat
    /// multi-element dump joins as a parenthesized list (`(1, 2, 3)`).
    ///
    /// Framed input arrives through `normalize_debugger_output_line`, which
    /// trims indentation, so a dump containing nested referent contents is
    /// structurally indistinguishable from a flat dump of the same line
    /// sequence (`x [1, 2]` and `x ([1], [2])` frame identically up to their
    /// ambiguous tail). A multi-line string value is equally ambiguous:
    /// perl5db prints embedded newlines literally, so the payload's
    /// continuation lines arrive looking exactly like further dump elements
    /// (`0  'foo` / `1  bar'`). Rather than guess and fabricate a value, the
    /// reshape accepts only dumps whose every line is a strictly sequential
    /// top-level ordinal (`0, 1, 2, …`) at indent zero whose payload closes
    /// every quote it opens; any other shape declines, and the generic
    /// evaluate parse paths keep handling the output exactly as before
    /// (#5086 keeps address-preserving reference rendering a follow-up).
    pub(super) fn parse_evaluate_result_from_x_dump(lines: &[String]) -> Option<(String, String)> {
        let mut top_level: Vec<String> = Vec::new();
        for line in lines {
            let normalized = Self::normalize_x_dump_line(line);
            if normalized.trim().is_empty() {
                continue;
            }
            let (indent, ordinal, payload) = Self::x_dump_line_parts(&normalized)?;
            // A dump with nested (indented) referent contents or hash entries
            // is not unambiguously reshapable once trimmed; decline it whole.
            if indent > 0 || ordinal != Some(top_level.len() as u64) {
                return None;
            }
            // An odd number of quotes means the payload's string continues on
            // the next output line; the following lines would be continuations
            // masquerading as further elements.
            if payload.matches('\'').count() % 2 == 1 {
                return None;
            }
            top_level.push(payload);
        }

        match top_level.len() {
            0 => None,
            1 => {
                let payload = &top_level[0];
                Some((payload.clone(), Self::infer_debugger_value_type(payload)))
            }
            _ => Some((format!("({})", top_level.join(", ")), "array".to_string())),
        }
    }

    /// Infer a coarse DAP value type from literal-like debugger output.
    pub(super) fn infer_debugger_value_type(text: &str) -> String {
        if text == "undef" {
            "undef".to_string()
        } else if text.parse::<i64>().is_ok() {
            "integer".to_string()
        } else if text.parse::<f64>().is_ok() {
            "number".to_string()
        } else if text.starts_with('[') && text.ends_with(']') {
            "array".to_string()
        } else if text.starts_with('{') && text.ends_with('}') {
            "hash".to_string()
        } else {
            "string".to_string()
        }
    }

    /// Convert microcrate rendered variables into adapter-local protocol values.
    pub(super) fn rendered_to_variable(rendered: RenderedVariable) -> Variable {
        Variable {
            name: rendered.name,
            value: rendered.value,
            type_: rendered.type_name,
            variables_reference: Self::i64_to_i32_saturating(rendered.variables_reference),
            named_variables: rendered.named_variables.map(Self::i64_to_i32_saturating),
            indexed_variables: rendered.indexed_variables.map(Self::i64_to_i32_saturating),
            evaluate_name: rendered.evaluate_name,
        }
    }

    /// Determine if a variable name should appear in a given scope.
    pub(super) fn scope_allows_variable_name(scope_type: i32, name: &str) -> bool {
        match scope_type {
            // Locals
            1 => !name.contains("::"),
            // Package variables (qualified)
            2 => name.contains("::"),
            // Globals/specials
            3 => {
                matches!(name, "$_" | "@ARGV" | "%ENV" | "$!" | "$@" | "$/" | "$|" | "$0" | "$^W")
                    || name.starts_with("$^")
            }
            _ => true,
        }
    }

    /// Convert parsed stack frames from `perl-dap-stack` into local DAP response frames.
    pub(super) fn parse_stack_frames_from_text(
        input: OriginatedParseInput<'_>,
    ) -> (Vec<StackFrame>, HashMap<i32, Vec<String>>) {
        let mut parser = PerlStackParser::new();
        let mut arguments = HashMap::new();
        let frames = parser
            .parse_stack_trace_originated(input)
            .into_iter()
            .map(|frame| {
                let source = frame.source.unwrap_or_default();
                let path = source.path.unwrap_or_else(|| "<unknown>".to_string());
                let name = source.name.or_else(|| {
                    std::path::Path::new(&path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(ToString::to_string)
                });
                let id = Self::i64_to_i32_saturating(frame.id);
                if !frame.arguments.is_empty() {
                    arguments.insert(id, frame.arguments);
                }
                StackFrame {
                    id,
                    name: frame.name,
                    source: Source { name, path, source_reference: None },
                    line: Self::i64_to_i32_saturating(frame.line),
                    column: Self::i64_to_i32_saturating(frame.column),
                    end_line: frame.end_line.map(Self::i64_to_i32_saturating),
                    end_column: frame.end_column.map(Self::i64_to_i32_saturating),
                }
            })
            .collect();
        (frames, arguments)
    }

    /// Filter out internal debugger and historical shim frames from user-visible stack traces.
    pub(super) fn filter_user_visible_frames(frames: Vec<StackFrame>) -> Vec<StackFrame> {
        frames
            .into_iter()
            .filter(|frame| {
                !is_internal_frame_name_and_path(&frame.name, Some(frame.source.path.as_str()))
            })
            .collect()
    }

    /// Capture the suspension-position authority from a framed `T` parse.
    ///
    /// perl5db's `T` report opens with the debugger's own `DB::DB` frame when
    /// execution is suspended inside called code; that frame's position is
    /// the line the debuggee will execute next. Every user frame in the same
    /// report carries only its *caller's* position (`called from`), so
    /// without this authority the topmost user frame would report the call
    /// site instead of the suspension line (#17171). `None` when the report
    /// does not open with the debugger frame (e.g. a top-level stop).
    ///
    /// The capture is deliberately narrower than
    /// [`Self::filter_user_visible_frames`]'s internal predicate: it requires
    /// the leading frame to be named exactly `DB::DB` **and** its `called
    /// from` source to be non-debugger plumbing. A shim-internal leading
    /// frame (`Devel::TSPerlDAP::…`, or a `DB::*` frame inside `perl5db.pl`)
    /// carries shim positions, not the user's suspension line, and must
    /// never be projected onto a user frame (CodeRabbit follow-up on
    /// #17171).
    pub(super) fn suspension_position_from_internal_frames(
        frames: &[StackFrame],
    ) -> Option<StackFrame> {
        let first = frames.first()?;
        if first.name != "DB::DB" {
            return None;
        }
        let path = first.source.path.as_str();
        if path.contains("perl5db.pl") || path.contains("Devel::TSPerlDAP") {
            return None;
        }
        Some(first.clone())
    }

    /// Reattach the suspension position to the topmost user-visible frame.
    ///
    /// The T-derived frame keeps its identity, arguments, and caller chain;
    /// only its current source position is reconciled to the position the
    /// debugger's own frame reported for this suspension (#17171).
    pub(super) fn reconcile_top_frame_with_suspension_position(
        mut frames: Vec<StackFrame>,
        suspension: Option<&StackFrame>,
    ) -> Vec<StackFrame> {
        if let (Some(suspended), Some(top)) = (suspension, frames.first_mut()) {
            top.line = suspended.line;
            top.source = suspended.source.clone();
        }
        frames
    }

    /// Parse variables from debugger output lines using microcrate parser/renderer.
    ///
    /// Rows retain the typed value captured at parse time (see
    /// [`CachedVariable`]); a request's DAP `ValueFormat` is projected from
    /// those typed facts at response time (#9588).
    pub(super) fn parse_scope_variables_from_lines(
        lines: &[String],
        variables_ref: i32,
        start: usize,
        count: usize,
        origin: DebuggerOutputOrigin,
        identity: ParseIdentity,
    ) -> (Vec<CachedVariable>, HashMap<i32, Vec<CachedVariable>>) {
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};
        // Decode the scope kind from the variablesReference using the codec.
        // scope_variables::parse_assignments still expects i32 discriminant (1/2/3).
        // Invalid (non-Scope or None) refs return empty results — no crash.
        let scope_type = match VariableReference::decode(variables_ref) {
            Some(VariableReference::Scope { kind, .. }) => match kind {
                ScopeKind::Locals => 1_i32,
                ScopeKind::Package => 2_i32,
                ScopeKind::Globals => 3_i32,
                ScopeKind::Arguments => return (Vec::new(), HashMap::new()),
            },
            _ => return (Vec::new(), HashMap::new()),
        };
        let parsed = scope_variables::parse_assignments(lines, scope_type, origin, identity);
        let page = scope_variables::sort_and_paginate(parsed, start, count);

        let mut top_level = Vec::with_capacity(page.len());
        let mut child_cache = HashMap::new();
        for (idx, (name, value)) in page.into_iter().enumerate() {
            let child_ref = scope_variables::compute_child_reference(variables_ref, start, idx);
            let (top, cache_entry) = scope_variables::render_paged_variable(name, value, child_ref);
            top_level.push(top);
            if let Some((k, v)) = cache_entry {
                child_cache.insert(k, v);
            }
        }
        (top_level, child_cache)
    }

    /// Parse evaluate output from debugger lines into a DAP result payload.
    ///
    /// `allow_correlated_literal` may be true only when `lines` came from the
    /// begin/end markers for this exact evaluate request. The request frame is
    /// also what makes a matching assignment current rather than merely similar
    /// to an older debugger response.
    ///
    /// Returns the default (decimal) display string, the type name, and the
    /// typed value retained for response-time `ValueFormat` projection — the
    /// typed value is `None` for the correlated-literal branch, which has no
    /// typed authority and therefore never receives heuristic formatting
    /// (#9588).
    pub(super) fn parse_evaluate_result_from_lines(
        lines: &[String],
        expression: &str,
        allow_correlated_literal: bool,
        origin: DebuggerOutputOrigin,
        identity: ParseIdentity,
    ) -> Option<(String, String, Option<PerlValue>)> {
        if lines.is_empty() {
            return None;
        }

        let parser = VariableParser::new();
        let renderer = PerlVariableRenderer::new();

        for line in lines.iter().rev() {
            let normalized = Self::normalize_debugger_output_line(line);
            let text = normalized.trim();
            if text.is_empty() || prompt_re().is_some_and(|re| re.is_match(text)) {
                continue;
            }

            let input = OriginatedParseInput::new(origin, identity, text);
            if let Ok((name, value)) = parser.parse_assignment_originated(input) {
                let rendered = renderer.render(&name, &value);
                let type_name = rendered.type_name.unwrap_or_else(|| "string".to_string());
                if name == expression {
                    return Some((rendered.value, type_name, Some(value)));
                }
                continue;
            }

            if allow_correlated_literal {
                return Some((text.to_string(), Self::infer_debugger_value_type(text), None));
            }
        }

        None
    }

    /// Parse explicit debugger error lines from evaluate output.
    pub(super) fn parse_evaluate_error_from_lines(lines: &[String]) -> Option<String> {
        const ERROR_PREFIXES: &[&str] =
            &["Undefined", "Can't ", "syntax error", "Execution of ", "Use of uninitialized"];

        for line in lines.iter().rev() {
            let normalized = Self::normalize_debugger_output_line(line);
            let text = normalized.trim();
            if text.is_empty() || prompt_re().is_some_and(|re| re.is_match(text)) {
                continue;
            }

            if ERROR_PREFIXES.iter().any(|prefix| text.starts_with(prefix)) {
                return Some(format!("evaluate failed: {text}"));
            }
        }

        None
    }

    /// Refuse to derive an evaluate result from unframed debugger history.
    ///
    /// Recent output can contain an earlier response for the same expression,
    /// so even a matching assignment is not evidence for the current request.
    /// Only begin/end-framed output is accepted as an evaluate result.
    pub(super) fn parse_evaluate_result_from_output(
        &self,
        _expression: &str,
    ) -> Option<(String, String, Option<PerlValue>)> {
        None
    }

    /// Return an honest empty scope when debugger inspection is unavailable.
    ///
    /// The adapter cannot infer current package/global/lexical values merely
    /// from the requested scope kind. Returning representative `$VERSION` or
    /// `$_` values would make placeholders indistinguishable from observed
    /// debuggee state. Pagination arguments are intentionally ignored because
    /// an unavailable scope has no known page or total.
    pub(super) fn fallback_scope_variables(
        _variables_ref: i32,
        _start: usize,
        _count: usize,
    ) -> Vec<Variable> {
        Vec::new()
    }

    /// Parse stack trace output from Perl debugger "T" command
    ///
    /// AC8.2: Parse caller() + %DB::sub data from Perl debugger
    ///
    /// The Perl debugger "T" command outputs stack traces in formats like:
    /// ```text
    /// $ = main::compute_sum() called from file /app/main.pl line 15
    /// $ = main::process_data() called from file /app/main.pl line 10
    /// ```
    ///
    /// Or with frame numbers:
    /// ```text
    /// # 0 main::helper at /app/script.pl line 20
    /// # 1 Foo::bar called at /app/lib/Foo.pm line 15
    /// # 2 main::start at /app/script.pl line 5
    /// ```
    ///
    /// Returns a vector of StackFrame structs with accurate line numbers,
    /// source paths, and package-qualified function names.
    #[cfg(test)]
    pub(super) fn parse_stack_trace(output: &str) -> Vec<StackFrame> {
        let mut frames = Vec::new();
        let mut frame_id = 1;

        for line in output.lines() {
            // Try to match stack frame format
            if let Some(re) = stack_frame_re()
                && let Some(caps) = re.captures(line)
            {
                let func = caps.name("func").map(|m| m.as_str()).unwrap_or("main");
                let file = caps.name("file").map(|m| m.as_str()).unwrap_or("<unknown>");
                let line_num =
                    caps.name("line").and_then(|m| m.as_str().parse::<i32>().ok()).unwrap_or(1);

                // Extract file name from path for display
                let file_name = file.split(['/', '\\'].as_ref()).next_back().unwrap_or(file);

                frames.push(StackFrame {
                    id: frame_id,
                    name: func.to_string(),
                    source: Source {
                        name: Some(file_name.to_string()),
                        path: file.to_string(),
                        source_reference: None,
                    },
                    line: line_num,
                    column: 1, // Perl debugger doesn't provide column info by default
                    end_line: None,
                    end_column: None,
                });

                frame_id += 1;
            }
        }

        frames
    }
}

#[cfg(test)]
mod tests {
    use super::super::operation_broker::OperationBroker;
    use super::super::patterns::DEBUGGER_FRAME_POLL_MS;
    use super::super::*;
    use crate::parse_origin::{DebuggerOutputOrigin, OriginatedParseInput, ParseIdentity};
    use std::thread;
    use std::time::{Duration, Instant};

    const FIXTURE: DebuggerOutputOrigin = DebuggerOutputOrigin::FixtureOrInstrumentInput;
    const FIXTURE_IDENTITY: ParseIdentity = ParseIdentity::new();

    #[test]
    pub(super) fn test_parse_scope_variables_from_best_effort_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec![
            "$foo = 42".to_string(),
            "@arr = (1, 2, 3)".to_string(),
            "%hash = {a => 1}".to_string(),
        ];
        let (vars, child_cache) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            0,
            20,
            DebuggerOutputOrigin::BestEffortDebuggeeOutput,
            ParseIdentity::new(),
        );
        let names: Vec<&str> = vars.iter().map(|v| v.row.name.as_str()).collect();
        if !names.contains(&"$foo")
            || !names.contains(&"@arr")
            || !names.contains(&"%hash")
            || child_cache.is_empty()
        {
            return Err("best-effort lines lost scalar or expandable variables".into());
        }
        Ok(())
    }

    #[test]
    pub(super) fn test_parse_scope_variables_are_sorted_for_stability()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec!["$zeta = 1".to_string(), "$alpha = 2".to_string(), "$mid = 3".to_string()];

        let (vars, _child_cache) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            0,
            20,
            FIXTURE,
            FIXTURE_IDENTITY,
        );
        let names = vars.iter().map(|v| v.row.name.as_str()).collect::<Vec<_>>();
        assert_eq!(names, vec!["$alpha", "$mid", "$zeta"]);
        Ok(())
    }

    #[test]
    pub(super) fn test_parse_scope_variables_child_refs_stable_across_pages()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec![
            "@alpha = (1, 2)".to_string(),
            "@beta = (3, 4)".to_string(),
            "@gamma = (5, 6)".to_string(),
        ];

        let (page_one, page_one_children) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            0,
            1,
            FIXTURE,
            FIXTURE_IDENTITY,
        );
        let (page_two, page_two_children) = DebugAdapter::parse_scope_variables_from_lines(
            &lines,
            11,
            1,
            1,
            FIXTURE,
            FIXTURE_IDENTITY,
        );

        let first_ref = page_one
            .first()
            .map(|variable| variable.row.variables_reference)
            .ok_or("expected first page variable")?;
        let second_ref = page_two
            .first()
            .map(|variable| variable.row.variables_reference)
            .ok_or("expected second page variable")?;

        assert_ne!(first_ref, second_ref, "paged variables must not reuse child references");
        assert!(page_one_children.contains_key(&first_ref));
        assert!(page_two_children.contains_key(&second_ref));
        Ok(())
    }

    #[test]
    pub(super) fn test_capture_framed_debugger_output_isolated_by_marker()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test("noise");
        adapter.push_recent_output_line_for_test(r#""DAP_BEGIN_100""#);
        adapter.push_recent_output_line_for_test("$a = 1");
        adapter.push_recent_output_line_for_test(r#""DAP_END_100""#);
        adapter.push_recent_output_line_for_test(r#""DAP_BEGIN_200""#);
        adapter.push_recent_output_line_for_test("$b = 2");
        adapter.push_recent_output_line_for_test(r#""DAP_END_200""#);

        let lines = adapter
            .capture_framed_debugger_output("DAP_BEGIN_200", "DAP_END_200", 200)
            .ok_or("expected framed output for marker 200")?;
        assert_eq!(lines, vec!["$b = 2".to_string()]);
        Ok(())
    }

    #[test]
    pub(super) fn test_capture_framed_debugger_output_handles_partial_marker_arrival()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        let recent_output = adapter.recent_output.clone();
        let producer = thread::spawn(move || {
            thread::sleep(Duration::from_millis(DEBUGGER_FRAME_POLL_MS * 2));
            let mut output = lock_or_recover(&recent_output, "test_partial_marker.recent_output");
            DebugAdapter::append_recent_output_line_locked(&mut output, r#""DAP_BEGIN_300""#);
            DebugAdapter::append_recent_output_line_locked(&mut output, "interleaved noise");
            DebugAdapter::append_recent_output_line_locked(&mut output, "$captured = 42");
            DebugAdapter::append_recent_output_line_locked(&mut output, r#""DAP_END_300""#);
        });

        let lines = adapter
            .capture_framed_debugger_output("DAP_BEGIN_300", "DAP_END_300", 500)
            .ok_or("expected framed output for delayed markers")?;
        producer.join().map_err(|_| "producer thread panicked")?;
        assert_eq!(lines, vec!["interleaved noise".to_string(), "$captured = 42".to_string()]);
        Ok(())
    }

    #[test]
    pub(super) fn test_capture_framed_debugger_output_timeout_without_end_marker() {
        let adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test(r#""DAP_BEGIN_500""#);
        adapter.push_recent_output_line_for_test("$value = 1");

        let start = Instant::now();
        let capture = adapter.capture_framed_debugger_output("DAP_BEGIN_500", "DAP_END_500", 1);
        assert!(capture.is_none(), "capture should timeout without end marker");
        assert!(start.elapsed() >= Duration::from_millis(DEBUGGER_QUERY_WAIT_MS));
    }

    #[test]
    pub(super) fn test_framed_capture_marker_scan_microbenchmark() {
        let mut lines = Vec::with_capacity(RECENT_OUTPUT_MAX_LINES);
        let mut raw_lines = Vec::with_capacity(RECENT_OUTPUT_MAX_LINES);
        for idx in 0..(RECENT_OUTPUT_MAX_LINES - 4) {
            let raw = format!("DB<1> noise line {idx}");
            lines.push(RecentOutputLine {
                id: idx as u64 + 1,
                normalized: DebugAdapter::normalize_debugger_output_line(&raw),
            });
            raw_lines.push(raw);
        }
        let begin_id = RECENT_OUTPUT_MAX_LINES as u64 - 3;
        lines.push(RecentOutputLine { id: begin_id, normalized: r#""DAP_BEGIN_900""#.to_string() });
        lines.push(RecentOutputLine { id: begin_id + 1, normalized: "$x = 1".to_string() });
        lines.push(RecentOutputLine { id: begin_id + 2, normalized: "$y = 2".to_string() });
        lines.push(RecentOutputLine {
            id: begin_id + 3,
            normalized: r#""DAP_END_900""#.to_string(),
        });

        raw_lines.extend([
            r#""DAP_BEGIN_900""#.to_string(),
            "$x = 1".to_string(),
            "$y = 2".to_string(),
            r#""DAP_END_900""#.to_string(),
        ]);
        let iterations = 300;
        let full_scan_start = Instant::now();
        for _ in 0..iterations {
            let normalized = raw_lines
                .iter()
                .map(|line| DebugAdapter::normalize_debugger_output_line(line))
                .collect::<Vec<_>>();
            let _ = normalized.iter().rposition(|line| line.contains("DAP_BEGIN_900")).and_then(
                |begin_idx| {
                    normalized[begin_idx + 1..]
                        .iter()
                        .position(|line| line.contains("DAP_END_900"))
                        .map(|end_rel| normalized[begin_idx + 1..begin_idx + 1 + end_rel].len())
                },
            );
        }
        let full_scan_elapsed = full_scan_start.elapsed();

        let incremental_start = Instant::now();
        for _ in 0..iterations {
            let mut saw_begin = false;
            for line in &lines {
                if !saw_begin {
                    if OperationBroker::line_contains_full_marker(&line.normalized, "DAP_BEGIN_900")
                    {
                        saw_begin = true;
                    }
                } else if OperationBroker::line_contains_full_marker(
                    &line.normalized,
                    "DAP_END_900",
                ) {
                    break;
                }
            }
        }
        let incremental_elapsed = incremental_start.elapsed();

        assert!(incremental_elapsed < full_scan_elapsed);
    }

    #[test]
    pub(super) fn test_stack_trace_returns_empty_without_live_session()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test("# 0 main::compute at /tmp/script.pl line 20");
        adapter.push_recent_output_line_for_test("# 1 Foo::process called at /tmp/Foo.pm line 15");

        let response = adapter.handle_request(1, "stackTrace", Some(json!({"threadId": 1})));
        match response {
            DapMessage::Response { success, body, .. } => {
                assert!(success);
                let body = body.ok_or("missing stackTrace body")?;
                let frames = body
                    .get("stackFrames")
                    .and_then(|v| v.as_array())
                    .ok_or("missing stackFrames")?;
                assert!(frames.is_empty());
            }
            _ => return Err("expected stackTrace response".into()),
        }
        Ok(())
    }

    #[test]
    pub(super) fn unframed_evaluate_does_not_reuse_matching_assignment() {
        let adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test("$result = 123");
        assert!(adapter.parse_evaluate_result_from_output("$result").is_none());
    }

    #[test]
    pub(super) fn unframed_evaluate_does_not_consume_unrelated_output() {
        let adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test("debuggee says hello");
        assert!(adapter.parse_evaluate_result_from_output("$result").is_none());
    }

    #[test]
    pub(super) fn framed_evaluate_accepts_correlated_literal_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec!["42".to_string()];
        let (value, ty, typed) = DebugAdapter::parse_evaluate_result_from_lines(
            &lines,
            "$result",
            true,
            FIXTURE,
            FIXTURE_IDENTITY,
        )
        .ok_or("framed literal should be accepted")?;
        assert_eq!(value, "42");
        assert_eq!(ty, "integer");
        // A correlated literal carries no typed authority: hex formatting must
        // never heuristically parse it (#9588).
        assert!(typed.is_none(), "literal branch must not retain typed authority");
        Ok(())
    }

    #[test]
    pub(super) fn framed_evaluate_requires_exact_assignment_name() {
        let lines = vec!["$result_extra = 123".to_string()];
        assert!(
            DebugAdapter::parse_evaluate_result_from_lines(
                &lines,
                "$result",
                false,
                FIXTURE,
                FIXTURE_IDENTITY
            )
            .is_none()
        );
    }

    /// perl5db's `x` renders a scalar as `0  <payload>`; the evaluate response
    /// must present the payload, not the dump ordinal prefix (#17244).
    #[test]
    pub(super) fn x_dump_scalar_strips_ordinal_prefix() {
        let lines = vec!["0  5".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&lines),
            Some(("5".to_string(), "integer".to_string()))
        );

        // Live framed shape: the debugger prompt rides the first dump line.
        let prompted = vec!["  DB<29> 0  1024".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&prompted),
            Some(("1024".to_string(), "integer".to_string()))
        );
    }

    /// A multi-element `x` dump must join into one faithful list rendering,
    /// not collapse to its final dump line (`2  3` for `(1,2,3)`) (#17244).
    #[test]
    pub(super) fn x_dump_list_joins_top_level_elements() {
        let lines = vec!["0  1".to_string(), "1  2".to_string(), "2  3".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&lines),
            Some(("(1, 2, 3)".to_string(), "array".to_string()))
        );

        let prompted = vec!["  DB<3> 0  1".to_string(), "1  2".to_string(), "2  3".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&prompted),
            Some(("(1, 2, 3)".to_string(), "array".to_string()))
        );
    }

    /// A dump carrying nested referent contents cannot be reshaped from
    /// framed input: `normalize_debugger_output_line` trims indentation, so
    /// `x [1, 2]` and `x ([1], [2])` frame identically up to their ambiguous
    /// tail. The reshape declines the whole dump and the generic parse paths
    /// keep the pre-#17244 behavior instead of fabricating a value (#17244).
    #[test]
    pub(super) fn x_dump_declines_nested_referent_contents() {
        // Framed shape: indentation already trimmed upstream.
        let arrayref =
            vec!["0  ARRAY(0x2035db81a48)".to_string(), "0  1".to_string(), "1  2".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&arrayref), None);

        // Nested hash entries ('key' => payload) are referent contents too.
        let hashref = vec!["0  HASH(0x1b7481614e8)".to_string(), "'a' => 1".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&hashref), None);

        // Raw shape with indent preserved: nested lines decline as well.
        let indented =
            vec!["0  ARRAY(0x557f8a9c)".to_string(), "   0  1".to_string(), "   1  2".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&indented), None);
    }

    /// perl5db prints embedded newlines literally, so a multi-line string
    /// value's continuation lines arrive looking exactly like further dump
    /// elements (`0  'foo` / `1  bar'`). An unterminated quote declines the
    /// reshape so the string is never fabricated into an array (#17244).
    #[test]
    pub(super) fn x_dump_declines_multiline_string_payloads() {
        // Live shape of `x $ml` for $ml = "foo\n1  bar" (perl5db 1.82).
        let multiline = vec!["0  'foo".to_string(), "1  bar'".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&multiline), None);

        // A multi-line element inside a list declines too.
        let list_with_multiline =
            vec!["0  'foo".to_string(), "1  bar'".to_string(), "2  5".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&list_with_multiline), None);
    }

    /// Quoted string payloads keep their exact perl5db rendering; `undef` and
    /// floats keep their coarse types after the prefix strip (#17244).
    #[test]
    pub(super) fn x_dump_payload_types_follow_stripped_text() {
        let quoted = vec!["0  'hello'".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&quoted),
            Some(("'hello'".to_string(), "string".to_string()))
        );

        let undef = vec!["0  undef".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&undef),
            Some(("undef".to_string(), "undef".to_string()))
        );

        let float = vec!["0  3.14".to_string()];
        assert_eq!(
            DebugAdapter::parse_evaluate_result_from_x_dump(&float),
            Some(("3.14".to_string(), "number".to_string()))
        );
    }

    /// Lines that are not an `x` dump must leave the reshape so the generic
    /// evaluate parse paths keep handling them unchanged (#17244).
    #[test]
    pub(super) fn x_dump_reshape_rejects_non_dump_lines() {
        // Bare literal (the pre-#17244 correlated-literal shape).
        let bare = vec!["42".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&bare), None);

        // perl5db renders empty structures without an ordinal.
        let empty = vec!["   empty array".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&empty), None);

        // Assignment-shaped read-back output belongs to the `p` path.
        let assignment = vec!["$x = 5".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&assignment), None);

        // A dump always opens at ordinal 0.
        let shifted = vec!["1  2".to_string(), "2  3".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&shifted), None);

        // A non-sequential top-level ordinal is not an `x` dump.
        let gap = vec!["0  1".to_string(), "5  2".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&gap), None);

        // Prompt-only or empty frames carry no result.
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&[]), None);
        let prompts = vec!["  DB<3>".to_string(), "".to_string()];
        assert_eq!(DebugAdapter::parse_evaluate_result_from_x_dump(&prompts), None);
    }

    /// `setVariable` / `setExpression` send `p {name} = {value}` then `p {name}` and read
    /// the framed output back. They must correlate against the variable they set: an empty
    /// subject matches no assignment, and the `continue` guarding the literal branch then
    /// discards the result entirely, failing both operations outright (#7275).
    #[test]
    pub(super) fn framed_read_back_accepts_assignment_for_the_named_subject()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec!["$x = 5".to_string()];

        let (value, _, typed) = DebugAdapter::parse_evaluate_result_from_lines(
            &lines,
            "$x",
            true,
            FIXTURE,
            FIXTURE_IDENTITY,
        )
        .ok_or("read-back for the named subject should be accepted")?;
        assert_eq!(value, "5");
        // The assignment branch retains typed authority for formatting (#9588).
        assert!(
            matches!(typed, Some(crate::value::PerlValue::Integer(5))),
            "assignment read-back must retain typed integer authority, got {typed:?}"
        );

        // The regression this guards: an empty subject yields nothing for the same output.
        assert!(
            DebugAdapter::parse_evaluate_result_from_lines(
                &lines,
                "",
                true,
                FIXTURE,
                FIXTURE_IDENTITY
            )
            .is_none(),
            "an empty subject must not be how set-variable read-back is correlated"
        );
        Ok(())
    }

    #[test]
    pub(super) fn framed_evaluate_rejects_expression_only_in_assignment_value() {
        let lines = vec![r#"$message = \"$result\""#.to_string()];
        assert!(
            DebugAdapter::parse_evaluate_result_from_lines(
                &lines,
                "$result",
                false,
                FIXTURE,
                FIXTURE_IDENTITY
            )
            .is_none()
        );
    }

    #[test]
    pub(super) fn unframed_evaluate_rejects_correlated_literal_without_request_frame() {
        let lines = vec!["42".to_string()];
        assert!(
            DebugAdapter::parse_evaluate_result_from_lines(
                &lines,
                "$result",
                false,
                FIXTURE,
                FIXTURE_IDENTITY
            )
            .is_none()
        );
    }

    #[test]
    pub(super) fn unavailable_scope_fallback_is_empty_for_every_scope_kind()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};

        for kind in
            [ScopeKind::Locals, ScopeKind::Package, ScopeKind::Globals, ScopeKind::Arguments]
        {
            let wire = VariableReference::Scope { frame_id: 1, kind }
                .encode()
                .ok_or("scope should encode")?;
            assert!(DebugAdapter::fallback_scope_variables(wire, 0, 10).is_empty());
        }
        Ok(())
    }

    /// Helper to create a test stack frame
    pub(super) fn make_test_frame(id: i32, name: &str, path: &str, line: i32) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: Source {
                name: Some(path.split('/').next_back().unwrap_or(path).to_string()),
                path: path.to_string(),
                source_reference: None,
            },
            line,
            column: 1,
            end_line: None,
            end_column: None,
        }
    }

    /// Test helper: Filter frames using the same logic as handle_stack_trace (AC8.2.1)
    pub(super) fn filter_internal_frames(frames: Vec<StackFrame>) -> Vec<StackFrame> {
        DebugAdapter::filter_user_visible_frames(frames)
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_removes_db_frames() {
        let frames = vec![
            make_test_frame(1, "main::hello", "/app/hello.pl", 10),
            make_test_frame(2, "DB::DB", "/usr/share/perl/5.34/perl5db.pl", 100),
            make_test_frame(3, "Foo::bar", "/app/lib/Foo.pm", 25),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "main::hello");
        assert_eq!(filtered[1].name, "Foo::bar");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_removes_shim_frames() {
        let frames = vec![
            make_test_frame(1, "Devel::TSPerlDAP::init", "/shim/TSPerlDAP.pm", 50),
            make_test_frame(2, "main::run", "/app/script.pl", 5),
            make_test_frame(3, "Devel::TSPerlDAP::handle_break", "/shim/TSPerlDAP.pm", 200),
            make_test_frame(4, "Utils::process", "/app/lib/Utils.pm", 42),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "main::run");
        assert_eq!(filtered[1].name, "Utils::process");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_removes_perl5db_source() {
        let frames = vec![
            make_test_frame(1, "main::start", "/app/main.pl", 1),
            make_test_frame(2, "some_internal", "/usr/lib/perl5/perl5db.pl", 999),
            make_test_frame(3, "App::process", "/app/lib/App.pm", 100),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "main::start");
        assert_eq!(filtered[1].name, "App::process");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_mixed_internal_frames() {
        let frames = vec![
            make_test_frame(1, "main::hello", "/app/hello.pl", 10),
            make_test_frame(2, "DB::sub", "/usr/share/perl/5.34/perl5db.pl", 2000),
            make_test_frame(3, "Foo::bar", "/app/lib/Foo.pm", 25),
            make_test_frame(4, "Devel::TSPerlDAP::step", "/shim/TSPerlDAP.pm", 150),
            make_test_frame(5, "DB::breakpoint", "/some/other/path.pm", 50),
            make_test_frame(6, "Baz::qux", "/app/lib/Baz.pm", 75),
            make_test_frame(7, "custom_handler", "/usr/lib/perl5/perl5db.pl", 1500),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].name, "main::hello");
        assert_eq!(filtered[1].name, "Foo::bar");
        assert_eq!(filtered[2].name, "Baz::qux");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_preserves_order() {
        let frames = vec![
            make_test_frame(1, "A::first", "/a.pm", 1),
            make_test_frame(2, "DB::internal", "/perl5db.pl", 100),
            make_test_frame(3, "B::second", "/b.pm", 2),
            make_test_frame(4, "Devel::TSPerlDAP::shim", "/shim.pm", 50),
            make_test_frame(5, "C::third", "/c.pm", 3),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].name, "A::first");
        assert_eq!(filtered[1].name, "B::second");
        assert_eq!(filtered[2].name, "C::third");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_all_internal() {
        let frames = vec![
            make_test_frame(1, "DB::main", "/perl5db.pl", 1),
            make_test_frame(2, "Devel::TSPerlDAP::init", "/shim.pm", 10),
            make_test_frame(3, "DB::sub", "/perl5db.pl", 50),
        ];
        let filtered = filter_internal_frames(frames);
        assert!(filtered.is_empty(), "Expected empty stack after filtering all internal frames");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_no_internal() {
        let frames = vec![
            make_test_frame(1, "main::start", "/app/main.pl", 1),
            make_test_frame(2, "Lib::helper", "/app/lib/Lib.pm", 50),
            make_test_frame(3, "Utils::format", "/app/lib/Utils.pm", 100),
        ];
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].name, "main::start");
        assert_eq!(filtered[1].name, "Lib::helper");
        assert_eq!(filtered[2].name, "Utils::format");
    }

    #[test]
    pub(super) fn test_stack_frame_filtering_empty_input() {
        let frames: Vec<StackFrame> = Vec::new();
        let filtered = filter_internal_frames(frames);
        assert!(filtered.is_empty());
    }

    #[test]
    pub(super) fn test_parse_stack_trace_simple_call_chain() {
        let output = r#"# 0 main::compute_sum at /app/script.pl line 20
# 1 Foo::process called at /app/lib/Foo.pm line 15
# 2 main::start at /app/script.pl line 5"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].id, 1);
        assert_eq!(frames[0].name, "main::compute_sum");
        assert_eq!(frames[0].source.path, "/app/script.pl");
        assert_eq!(frames[0].line, 20);
        assert_eq!(frames[0].source.name, Some("script.pl".to_string()));
        assert_eq!(frames[1].id, 2);
        assert_eq!(frames[1].name, "Foo::process");
        assert_eq!(frames[1].source.path, "/app/lib/Foo.pm");
        assert_eq!(frames[1].line, 15);
        assert_eq!(frames[2].id, 3);
        assert_eq!(frames[2].name, "main::start");
        assert_eq!(frames[2].source.path, "/app/script.pl");
        assert_eq!(frames[2].line, 5);
    }

    #[test]
    pub(super) fn test_parse_verbose_stack_frame_returns_argument_map() {
        let output = "$ = main::run($value, [1, 2], \"a,b\") called from file `script.pl' line 7";
        let input = OriginatedParseInput::new(FIXTURE, ParseIdentity::new(), output);
        let (frames, arguments) = DebugAdapter::parse_stack_frames_from_text(input);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, 1);
        assert_eq!(frames[0].name, "main::run");
        assert_eq!(
            arguments.get(&1),
            Some(&vec!["$value".to_string(), "[1, 2]".to_string(), "\"a,b\"".to_string()])
        );
    }

    /// Only a leading `DB::DB` frame pointing at non-debugger source carries
    /// the suspension position. Shim-internal leading frames
    /// (`Devel::TSPerlDAP::…`, or anything inside `perl5db.pl`) must not be
    /// projected onto the user frame (CodeRabbit follow-up on #17171).
    #[test]
    pub(super) fn test_suspension_position_requires_leading_db_db_frame() {
        fn frame(id: i32, name: &str, path: &str) -> StackFrame {
            StackFrame {
                id,
                name: name.to_string(),
                source: Source { name: None, path: path.to_string(), source_reference: None },
                line: 9,
                column: 1,
                end_line: None,
                end_column: None,
            }
        }
        let db_frame = [frame(1, "DB::DB", "F:/dbg/hello.pl")];
        assert!(
            DebugAdapter::suspension_position_from_internal_frames(&db_frame).is_some(),
            "the debugger's own frame over user code is the suspension authority"
        );
        let shim_frame = [frame(1, "Devel::TSPerlDAP::handle_break", "F:/dbg/hello.pl")];
        assert!(
            DebugAdapter::suspension_position_from_internal_frames(&shim_frame).is_none(),
            "a shim frame must not become the suspension position"
        );
        let perl5db_frame = [frame(1, "DB::DB", "/usr/lib/perl5/perl5db.pl")];
        assert!(
            DebugAdapter::suspension_position_from_internal_frames(&perl5db_frame).is_none(),
            "a DB::DB frame pointing inside perl5db.pl is shim plumbing, not user suspension"
        );
        let user_frame = [frame(1, "main::add", "F:/dbg/hello.pl")];
        assert!(
            DebugAdapter::suspension_position_from_internal_frames(&user_frame).is_none(),
            "a report without a leading debugger frame carries no suspension authority"
        );
    }

    #[test]
    pub(super) fn test_parse_stack_trace_multi_file_packages() {
        let output = r#"# 0 Utils::Helper::validate at /app/lib/Utils/Helper.pm line 42
# 1 Data::Processor::transform called at /app/lib/Data/Processor.pm line 120
# 2 Controller::API::handle_request at /app/controller/API.pm line 78
# 3 main::dispatch called at /app/app.pl line 10"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 4);
        assert_eq!(frames[0].name, "Utils::Helper::validate");
        assert_eq!(frames[1].name, "Data::Processor::transform");
        assert_eq!(frames[2].name, "Controller::API::handle_request");
        assert_eq!(frames[3].name, "main::dispatch");
        assert!(frames[0].source.path.contains("Utils/Helper.pm"));
        assert!(frames[1].source.path.contains("Data/Processor.pm"));
        assert!(frames[2].source.path.contains("controller/API.pm"));
        assert!(frames[3].source.path.contains("app.pl"));
    }

    #[test]
    pub(super) fn test_parse_stack_trace_recursive_calls() {
        let output = r#"# 0 main::factorial at /app/math.pl line 5
# 1 main::factorial called at /app/math.pl line 6
# 2 main::factorial called at /app/math.pl line 6
# 3 main::factorial called at /app/math.pl line 6
# 4 main::compute at /app/math.pl line 10"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0].name, "main::factorial");
        assert_eq!(frames[1].name, "main::factorial");
        assert_eq!(frames[2].name, "main::factorial");
        assert_eq!(frames[3].name, "main::factorial");
        assert_eq!(frames[4].name, "main::compute");
        assert_eq!(frames[0].id, 1);
        assert_eq!(frames[1].id, 2);
        assert_eq!(frames[2].id, 3);
        assert_eq!(frames[3].id, 4);
        assert_eq!(frames[4].id, 5);
    }

    #[test]
    pub(super) fn test_parse_stack_trace_anonymous_subs() {
        let output = r#"# 0 main::__ANON__ at /app/callback.pl line 15
# 1 Utils::map called at /app/lib/Utils.pm line 42
# 2 main::process_items at /app/callback.pl line 10"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].name, "main::__ANON__");
        assert_eq!(frames[1].name, "Utils::map");
        assert_eq!(frames[2].name, "main::process_items");
    }

    #[test]
    pub(super) fn test_parse_stack_trace_windows_paths() {
        let output = r#"# 0 main::test at C:\workspace\script.pl line 10
# 1 Foo::bar called at C:\workspace\lib\Foo.pm line 25"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].source.path, r"C:\workspace\script.pl");
        assert_eq!(frames[0].source.name, Some("script.pl".to_string()));
        assert_eq!(frames[1].source.path, r"C:\workspace\lib\Foo.pm");
        assert_eq!(frames[1].source.name, Some("Foo.pm".to_string()));
    }

    #[test]
    pub(super) fn test_parse_stack_trace_with_space_in_paths() {
        let output = r#"# 0 main::test at /tmp/My Project/script.pl line 10
# 1 Foo::bar called at C:\Work Files\lib\Foo.pm line 25"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].source.path, "/tmp/My Project/script.pl");
        assert_eq!(frames[0].source.name, Some("script.pl".to_string()));
        assert_eq!(frames[1].source.path, r"C:\Work Files\lib\Foo.pm");
        assert_eq!(frames[1].source.name, Some("Foo.pm".to_string()));
    }

    #[test]
    pub(super) fn test_parse_stack_trace_empty_output() {
        assert!(DebugAdapter::parse_stack_trace("").is_empty());
    }

    #[test]
    pub(super) fn test_parse_stack_trace_malformed_output() {
        assert!(DebugAdapter::parse_stack_trace("Random output\nDB<1>").is_empty());
    }

    #[test]
    pub(super) fn test_parse_and_filter_stack_trace() {
        let output = r#"# 0 main::user_func at /app/script.pl line 10
# 1 DB::DB called at /usr/share/perl/5.34/perl5db.pl line 100
# 2 Foo::process at /app/lib/Foo.pm line 25
# 3 Devel::TSPerlDAP::handle_break called at /shim/TSPerlDAP.pm line 50
# 4 main::start at /app/script.pl line 5"#;
        let frames = DebugAdapter::parse_stack_trace(output);
        assert_eq!(frames.len(), 5, "stack parsing must retain all frames before filtering");
        let filtered = filter_internal_frames(frames);
        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].name, "main::user_func");
        assert_eq!(filtered[1].name, "Foo::process");
        assert_eq!(filtered[2].name, "main::start");
    }

    #[test]
    pub(super) fn test_normalize_strips_single_db_prompt() {
        assert_eq!(
            DebugAdapter::normalize_debugger_output_line("DB<1> main::(/path/file.pl:5):"),
            "main::(/path/file.pl:5):"
        );
    }

    #[test]
    pub(super) fn test_normalize_strips_multiple_db_prompts() {
        assert_eq!(
            DebugAdapter::normalize_debugger_output_line(
                "  DB<1>   DB<2> main::(/path/file.pl:5):"
            ),
            "main::(/path/file.pl:5):"
        );
    }

    #[test]
    pub(super) fn test_normalize_strips_high_prompt_number() {
        assert_eq!(DebugAdapter::normalize_debugger_output_line("DB<100> $x = 42"), "$x = 42");
    }

    #[test]
    pub(super) fn test_normalize_no_prompt_passthrough() {
        assert_eq!(
            DebugAdapter::normalize_debugger_output_line("  main::(/path/file.pl:5):"),
            "main::(/path/file.pl:5):"
        );
    }

    #[test]
    pub(super) fn test_normalize_unclosed_prompt_passthrough() {
        assert_eq!(DebugAdapter::normalize_debugger_output_line("DB<incomplete"), "DB<incomplete");
    }

    #[test]
    pub(super) fn test_normalize_three_prompts_in_sequence() {
        assert_eq!(
            DebugAdapter::normalize_debugger_output_line("DB<1> DB<2> DB<3> my $x = 10;"),
            "my $x = 10;"
        );
    }

    #[test]
    fn test_stack_trace_does_not_use_snapshot_in_degraded_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.seed_session_for_test()?;

        let stale_frame =
            StackFrame::new(1, "old_func".to_string(), Source::new("/old/file.pl"), 5);
        adapter.inject_stack_frames_for_test(vec![stale_frame]);
        adapter.push_recent_output_line_for_test("main::(/test/file1.pl:4):");
        adapter.push_recent_output_line_for_test("main::(/test/file2.pl:5):");

        let response = adapter.handle_stack_trace(1, 1, Some(json!({"threadId": 1})));
        match response {
            DapMessage::Response { body: Some(body), .. } => {
                if let Ok(trace_response) =
                    serde_json::from_value::<crate::protocol::StackTraceResponseBody>(body)
                {
                    assert!(
                        trace_response.stack_frames.is_empty()
                            || trace_response.stack_frames.len() == 1
                    );
                }
            }
            _ => return Err("Expected response with body".into()),
        }
        Ok(())
    }

    #[test]
    pub(super) fn test_fallback_scope_variables_deep_frame_is_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::debug_adapter::var_ref::{ScopeKind, VariableReference};

        let scope_ref = VariableReference::Scope { frame_id: 10_000, kind: ScopeKind::Locals };
        let scope_wire = scope_ref.encode().ok_or("Scope{frame_id:10_000} should encode")?;
        assert_eq!(scope_wire, 100_001);
        assert!(DebugAdapter::fallback_scope_variables(scope_wire, 0, 10).is_empty());
        Ok(())
    }
}
