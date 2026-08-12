//! Debugger output parsing: normalize, infer types, parse stack frames, parse variables.

mod scope_variables;

#[cfg(test)]
use super::stack_frame_re;
use super::{
    DebugAdapter, HashMap, PerlStackParser, PerlVariableRenderer, RenderedVariable, Source,
    StackFrame, Variable, VariableParser, VariableRenderer, ansi_escape_re,
    is_internal_frame_name_and_path, prompt_re,
};

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
        output: &str,
    ) -> (Vec<StackFrame>, HashMap<i32, Vec<String>>) {
        let mut parser = PerlStackParser::new();
        let mut arguments = HashMap::new();
        let frames = parser
            .parse_stack_trace(output)
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

    /// Parse variables from debugger output lines using microcrate parser/renderer.
    pub(super) fn parse_scope_variables_from_lines(
        lines: &[String],
        variables_ref: i32,
        start: usize,
        count: usize,
    ) -> (Vec<Variable>, HashMap<i32, Vec<Variable>>) {
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
        let parsed = scope_variables::parse_assignments(lines, scope_type);
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

    /// Parse variables from recent debugger output using microcrate parser/renderer.
    pub(super) fn parse_scope_variables_from_output(
        &self,
        variables_ref: i32,
        start: usize,
        count: usize,
    ) -> (Vec<Variable>, HashMap<i32, Vec<Variable>>) {
        let lines = self.snapshot_recent_output_lines();
        Self::parse_scope_variables_from_lines(&lines, variables_ref, start, count)
    }

    /// Parse evaluate output from debugger lines into a DAP result payload.
    ///
    /// `allow_correlated_literal` may be true only when `lines` came from the
    /// begin/end markers for this exact evaluate request. The request frame is
    /// also what makes a matching assignment current rather than merely similar
    /// to an older debugger response.
    pub(super) fn parse_evaluate_result_from_lines(
        lines: &[String],
        expression: &str,
        allow_correlated_literal: bool,
    ) -> Option<(String, String)> {
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

            if let Ok((name, value)) = parser.parse_assignment(text) {
                let rendered = renderer.render(&name, &value);
                let type_name = rendered.type_name.unwrap_or_else(|| "string".to_string());
                if name == expression {
                    return Some((rendered.value, type_name));
                }
                continue;
            }

            if allow_correlated_literal {
                return Some((text.to_string(), Self::infer_debugger_value_type(text)));
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
    ) -> Option<(String, String)> {
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
    use super::super::*;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    pub(super) fn test_parse_scope_variables_from_recent_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let adapter = DebugAdapter::new();
        adapter.push_recent_output_line_for_test("$foo = 42");
        adapter.push_recent_output_line_for_test("@arr = (1, 2, 3)");
        adapter.push_recent_output_line_for_test("%hash = {a => 1}");

        let (vars, child_cache) = adapter.parse_scope_variables_from_output(11, 0, 20);
        let names: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"$foo"));
        assert!(names.contains(&"@arr"));
        assert!(names.contains(&"%hash"));
        assert!(!child_cache.is_empty(), "expected child cache entries for expandable values");
        Ok(())
    }

    #[test]
    pub(super) fn test_parse_scope_variables_are_sorted_for_stability()
    -> Result<(), Box<dyn std::error::Error>> {
        let lines = vec!["$zeta = 1".to_string(), "$alpha = 2".to_string(), "$mid = 3".to_string()];

        let (vars, _child_cache) =
            DebugAdapter::parse_scope_variables_from_lines(&lines, 11, 0, 20);
        let names = vars.iter().map(|v| v.name.as_str()).collect::<Vec<_>>();
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

        let (page_one, page_one_children) =
            DebugAdapter::parse_scope_variables_from_lines(&lines, 11, 0, 1);
        let (page_two, page_two_children) =
            DebugAdapter::parse_scope_variables_from_lines(&lines, 11, 1, 1);

        let first_ref = page_one
            .first()
            .map(|variable| variable.variables_reference)
            .ok_or("expected first page variable")?;
        let second_ref = page_two
            .first()
            .map(|variable| variable.variables_reference)
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
    pub(super) fn test_capture_framed_debugger_output_respects_cancellation() {
        let adapter = DebugAdapter::new();
        adapter.cancel_requested.store(true, Ordering::Release);

        let capture = adapter.capture_framed_debugger_output("DAP_BEGIN_400", "DAP_END_400", 200);
        assert!(capture.is_none(), "capture should stop when request is cancelled");
        assert!(!adapter.cancel_requested.load(Ordering::Acquire));
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
        for idx in 0..(RECENT_OUTPUT_MAX_LINES - 4) {
            let raw = format!("DB<1> noise line {idx}");
            lines.push(RecentOutputLine {
                id: idx as u64 + 1,
                normalized: DebugAdapter::normalize_debugger_output_line(&raw),
                raw,
            });
        }
        let begin_id = RECENT_OUTPUT_MAX_LINES as u64 - 3;
        lines.push(RecentOutputLine {
            id: begin_id,
            raw: r#""DAP_BEGIN_900""#.to_string(),
            normalized: r#""DAP_BEGIN_900""#.to_string(),
        });
        lines.push(RecentOutputLine {
            id: begin_id + 1,
            raw: "$x = 1".to_string(),
            normalized: "$x = 1".to_string(),
        });
        lines.push(RecentOutputLine {
            id: begin_id + 2,
            raw: "$y = 2".to_string(),
            normalized: "$y = 2".to_string(),
        });
        lines.push(RecentOutputLine {
            id: begin_id + 3,
            raw: r#""DAP_END_900""#.to_string(),
            normalized: r#""DAP_END_900""#.to_string(),
        });

        let iterations = 300;
        let full_scan_start = Instant::now();
        for _ in 0..iterations {
            let normalized = lines
                .iter()
                .map(|line| DebugAdapter::normalize_debugger_output_line(&line.raw))
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
                    if DebugAdapter::line_contains_full_marker(&line.normalized, "DAP_BEGIN_900") {
                        saw_begin = true;
                    }
                } else if DebugAdapter::line_contains_full_marker(&line.normalized, "DAP_END_900") {
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
        let (value, ty) = DebugAdapter::parse_evaluate_result_from_lines(&lines, "$result", true)
            .ok_or("framed literal should be accepted")?;
        assert_eq!(value, "42");
        assert_eq!(ty, "integer");
        Ok(())
    }

    #[test]
    pub(super) fn framed_evaluate_requires_exact_assignment_name() {
        let lines = vec!["$result_extra = 123".to_string()];
        assert!(DebugAdapter::parse_evaluate_result_from_lines(&lines, "$result", false).is_none());
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
        let (frames, arguments) = DebugAdapter::parse_stack_frames_from_text(output);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].id, 1);
        assert_eq!(frames[0].name, "main::run");
        assert_eq!(
            arguments.get(&1),
            Some(&vec!["$value".to_string(), "[1, 2]".to_string(), "\"a,b\"".to_string()])
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

        let response = adapter.handle_stack_trace(1, 1, None);
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
