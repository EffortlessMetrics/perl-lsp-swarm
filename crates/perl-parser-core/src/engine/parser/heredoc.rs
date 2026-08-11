/// Advance byte offset to just after the next line break (handles \n and \r\n)
fn after_line_break(src: &[u8], mut off: usize) -> usize {
    // Skip to newline if in middle of line
    while off < src.len() && src[off] != b'\n' && src[off] != b'\r' {
        off += 1;
    }
    if off < src.len() {
        if src[off] == b'\r' {
            off += 1;
            if off < src.len() && src[off] == b'\n' {
                off += 1;
            }
        } else if src[off] == b'\n' {
            off += 1;
        }
    }
    off
}

/// Parse heredoc delimiter from a string like "<<EOF", "<<'EOF'", "<<~EOF", "<<`EOF`"
fn parse_heredoc_delimiter(s: &str) -> (String, bool, bool, bool) {
    let mut chars = s.chars();

    // Skip <<
    chars.next();
    chars.next();

    // Check for indented heredoc
    let indented = if chars.as_str().starts_with('~') {
        chars.next();
        true
    } else {
        false
    };

    let rest = chars.as_str().trim();

    // Check for empty label (<<; or <<\n)
    if rest.is_empty() || rest.starts_with(';') {
        return (String::new(), true, indented, false);
    }

    // Check quoting to determine interpolation and command execution.
    // `<<\EOF` is Perl's backslash-quoted heredoc form: the terminator is
    // `EOF`, and the body is not interpolated. The lexer accepts this spelling
    // and includes the leading backslash in token text, so normalize it here
    // before the AST node and collector are populated.
    let (delimiter, interpolated, command) =
        if let Some(label) = rest.strip_prefix('\\') {
            (label.to_string(), false, false)
        } else if rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2 {
            // Double-quoted: interpolated body, but the delimiter label itself is
            // literal (Perl 5.38 keeps escape sequences like \n as two bytes).
            (rest[1..rest.len() - 1].to_string(), true, false)
        } else if rest.starts_with('\'') && rest.ends_with('\'') && rest.len() >= 2 {
            // Single-quoted: not interpolated, no unescape
            (rest[1..rest.len() - 1].to_string(), false, false)
        } else if rest.starts_with('`') && rest.ends_with('`') && rest.len() >= 2 {
            // Backtick: interpolated command body; delimiter label stays literal.
            (rest[1..rest.len() - 1].to_string(), true, true)
        } else {
            // Bare word: interpolated, no unescape (except maybe explicit escapes?)
            // Bare identifiers don't usually have escapes, but can have weird chars?
            // "EOF" -> EOF.
            (rest.to_string(), true, false)
        };

    (delimiter, interpolated, indented, command)
}

/// Map heredoc delimiter text to collector QuoteKind
fn map_heredoc_quote_kind(text: &str, _interpolated: bool) -> heredoc_collector::QuoteKind {
    // Skip << and optional ~
    let rest = text.trim_start_matches('<').trim_start_matches('~').trim();

    if rest.starts_with('\\') || rest.starts_with('\'') && rest.ends_with('\'') {
        heredoc_collector::QuoteKind::Single
    } else if rest.starts_with('"') && rest.ends_with('"') {
        heredoc_collector::QuoteKind::Double
    } else if rest.starts_with('`') && rest.ends_with('`') {
        heredoc_collector::QuoteKind::Backtick
    } else {
        // Bare word (unquoted)
        heredoc_collector::QuoteKind::Unquoted
    }
}

const MAX_HEREDOC_DEPTH: usize = 100;
const HEREDOC_TIMEOUT_MS: u64 = 5000;

impl<'a> Parser<'a> {
    /// Enqueue a heredoc declaration for later content collection
    fn push_heredoc_decl(
        &mut self,
        label: String,
        allow_indent: bool,
        quote: heredoc_collector::QuoteKind,
        decl_start: usize,
        decl_end: usize,
    ) {
        if self.pending_heredocs.len() >= MAX_HEREDOC_DEPTH {
            self.errors.push(ParseError::syntax(
                format!("Heredoc depth limit exceeded (max {})", MAX_HEREDOC_DEPTH),
                decl_start,
            ));
            return;
        }

        if self.pending_heredocs.is_empty() {
            self.heredoc_start_time = Some(Instant::now());
        }

        self.pending_heredocs.push_back(PendingHeredoc {
            label: Arc::from(label.as_str()),
            allow_indent,
            quote,
            decl_span: heredoc_collector::Span { start: decl_start, end: decl_end },
            body_start: after_line_break(self.src_bytes, decl_end),
        });
    }

    /// Drain heredocs added after `pending_start` and retain any parent statement's queue.
    ///
    /// A compound statement can declare a heredoc in its condition before recursively
    /// parsing its block. Statements in that block must attach their own heredocs as
    /// they complete, but cannot consume the parent's placeholder before its AST node
    /// exists. Keeping the prefix also prevents sequential block statements from
    /// accumulating against the global depth cap.
    #[allow(clippy::print_stderr, reason = "debug-only diagnostic — conditional on debug_assertions, cannot use #[expect]")]
    fn drain_pending_heredocs_from(&mut self, pending_start: usize, root: &mut Node) {
        if pending_start >= self.pending_heredocs.len() {
            return;
        }

        // Check for timeout
        if let Some(start) = self.heredoc_start_time {
            if start.elapsed().as_millis() > HEREDOC_TIMEOUT_MS as u128 {
                self.errors.push(ParseError::syntax(
                    format!("Heredoc parsing timed out (> {}ms)", HEREDOC_TIMEOUT_MS),
                    self.byte_cursor,
                ));
                // Clear pending to prevent further processing/hanging
                self.pending_heredocs.clear();
                self.heredoc_start_time = None;
                return;
            }
        }

        // Keep a copy of the suffix declarations so we can match outputs back to inputs.
        // The prefix belongs to an enclosing statement and must remain queued until that
        // statement has a complete AST node for attachment.
        let queued = self.pending_heredocs.split_off(pending_start);
        let pending: Vec<_> = queued.iter().cloned().collect();

        let out = collect_at_declaration_offsets(self.src_bytes, queued);

        // Zip 1:1 in order (collector preserves input order)
        for (decl, body) in pending.into_iter().zip(out.contents) {
            let mut attached = self.try_attach_heredoc_at_node(root, decl.decl_span, &body);
            if !attached {
                // Fallback: when statement recovery mutates span boundaries, the exact
                // decl-span match may miss the placeholder node. In that case, attach to
                // the next unresolved Heredoc node in source order.
                attached = self.try_attach_next_unresolved_heredoc(root, &body);
            }

            if !body.terminated {
                let label = if decl.label.is_empty() { "<empty>" } else { decl.label.as_ref() };
                self.errors.push(ParseError::SyntaxError {
                    message: format!("Unterminated heredoc: {}", label),
                    location: self.src_bytes.len(),
                });
            }

            // Defensive guardrail: warn if heredoc node wasn't found at expected span
            #[cfg(debug_assertions)]
            if !attached {
                eprintln!(
                    "[WARNING] drain_pending_heredocs: Failed to attach heredoc content at span {}..{} - no matching Heredoc node found in AST",
                    decl.decl_span.start, decl.decl_span.end
                );
            }
        }
        // A nested statement may attach a later body before its enclosing statement
        // attaches an earlier declaration. Never move the cursor backwards when the
        // parent queue is finally drained.
        self.byte_cursor = self.byte_cursor.max(out.next_offset);
        if self.pending_heredocs.is_empty() {
            self.heredoc_start_time = None;
        }
    }

    /// Attach collected heredoc content to its declaration node by matching declaration span
    /// Returns true if a matching Heredoc node was found and updated, false otherwise
    fn try_attach_heredoc_at_node(
        &self,
        root: &mut Node,
        decl_span: heredoc_collector::Span,
        body: &HeredocContent,
    ) -> bool {
        // Depth-first search for the Heredoc node with matching declaration span
        self.try_attach_at_node(root, decl_span, body)
    }

    /// Try to attach heredoc content at this node or its children
    #[allow(clippy::print_stderr, reason = "debug-only diagnostic — conditional on debug_assertions, cannot use #[expect]")]
    fn try_attach_at_node(
        &self,
        node: &mut Node,
        decl_span: heredoc_collector::Span,
        body: &HeredocContent,
    ) -> bool {
        // Check if this node's span matches the declaration span
        let node_matches =
            node.location.start == decl_span.start && node.location.end == decl_span.end;

        if node_matches {
            // Try to attach at this node
            if let NodeKind::Heredoc { content, body_span, .. } = &mut node.kind {
                // Reify the body bytes from src_bytes using the collector's segments
                let mut s = String::new();
                for (i, seg) in body.segments.iter().enumerate() {
                    if seg.end > seg.start {
                        let bytes = &self.src_bytes[seg.start..seg.end];
                        // Source is valid UTF-8 (enforced by lexer)
                        s.push_str(std::str::from_utf8(bytes).unwrap_or_default());
                    }
                    if i + 1 < body.segments.len() {
                        // Normalize line breaks for AST convenience
                        s.push('\n');
                    }
                }
                *content = s;

                // Store body span for breakpoint detection
                *body_span = if body.full_span.start < body.full_span.end {
                    Some(SourceLocation {
                        start: body.full_span.start,
                        end: body.full_span.end,
                    })
                } else {
                    None // Empty heredoc
                };

                return true;
            }
        }

        // Recursively search children (DFS) using for_each_child_mut
        let mut found = false;
        node.for_each_child_mut(|child| {
            if !found && self.try_attach_at_node(child, decl_span, body) {
                found = true;
            }
        });

        #[cfg(debug_assertions)]
        if !found && node_matches {
            eprintln!(
                "warn: no Heredoc node found for decl span {}..{} (matched span but not Heredoc kind)",
                decl_span.start, decl_span.end
            );
        }

        found
    }

    /// Fallback attachment strategy used when exact declaration-span matching fails.
    ///
    /// This can happen when error recovery shifts statement boundaries around the
    /// heredoc declaration. The collector output still preserves FIFO order, so
    /// attaching to the next unresolved Heredoc node keeps body/placeholder pairing
    /// stable while avoiding dropped heredoc bodies.
    fn try_attach_next_unresolved_heredoc(&self, root: &mut Node, body: &HeredocContent) -> bool {
        self.try_attach_at_next_unresolved_node(root, body)
    }

    fn try_attach_at_next_unresolved_node(&self, node: &mut Node, body: &HeredocContent) -> bool {
        if let NodeKind::Heredoc { content, body_span, .. } = &mut node.kind {
            let unresolved = content.is_empty() && body_span.is_none();
            if unresolved {
                let mut text = String::new();
                for (i, seg) in body.segments.iter().enumerate() {
                    if seg.end > seg.start {
                        let bytes = &self.src_bytes[seg.start..seg.end];
                        text.push_str(std::str::from_utf8(bytes).unwrap_or_default());
                    }
                    if i + 1 < body.segments.len() {
                        text.push('\n');
                    }
                }

                *content = text;
                *body_span = if body.full_span.start < body.full_span.end {
                    Some(SourceLocation {
                        start: body.full_span.start,
                        end: body.full_span.end,
                    })
                } else {
                    None
                };
                return true;
            }
        }

        let mut found = false;
        node.for_each_child_mut(|child| {
            if !found && self.try_attach_at_next_unresolved_node(child, body) {
                found = true;
            }
        });
        found
    }

}

#[cfg(test)]
mod heredoc_fuzz_tests {
    use super::{map_heredoc_quote_kind, parse_heredoc_delimiter};
    use crate::engine::parser::heredoc_collector::QuoteKind;
    use proptest::prelude::*;

    fn heredoc_text_strategy() -> impl Strategy<Value = String> {
        prop::collection::vec(any::<char>(), 0..64).prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn fuzz_parse_heredoc_delimiter_never_panics(input in heredoc_text_strategy()) {
            let source = format!("<<{input}");
            let (delimiter, _interpolated, _indented, _command) = parse_heredoc_delimiter(&source);
            prop_assert!(delimiter.len() <= source.len());
        }

        #[test]
        fn fuzz_tilde_prefix_sets_indent_flag(payload in heredoc_text_strategy()) {
            let source = format!("<<~{payload}");
            let (_delimiter, _interpolated, indented, _command) = parse_heredoc_delimiter(&source);
            prop_assert!(indented);
        }

        #[test]
        fn fuzz_quote_kind_matches_outer_delimiters(label in "[^\n\r']{0,32}") {
            let single = format!("<<'{label}'");
            prop_assert!(matches!(map_heredoc_quote_kind(&single, false), QuoteKind::Single));

            let double = format!("<<\"{label}\"");
            prop_assert!(matches!(map_heredoc_quote_kind(&double, true), QuoteKind::Double));

            let backtick = format!("<<`{label}`");
            prop_assert!(matches!(map_heredoc_quote_kind(&backtick, true), QuoteKind::Backtick));
        }
    }
}

#[cfg(test)]
mod heredoc_branch_tests {
    use super::{after_line_break, map_heredoc_quote_kind, parse_heredoc_delimiter};
    use crate::engine::parser::heredoc_collector::QuoteKind;

    #[test]
    fn parses_empty_and_semicolon_terminated_labels() {
        assert_eq!(parse_heredoc_delimiter("<<"), (String::new(), true, false, false));
        assert_eq!(
            parse_heredoc_delimiter("<<;"),
            (String::new(), true, false, false)
        );
    }

    #[test]
    fn parses_indented_and_quoted_labels() {
        assert_eq!(
            parse_heredoc_delimiter("<<~EOF"),
            ("EOF".to_string(), true, true, false)
        );
        assert_eq!(
            parse_heredoc_delimiter("<<'EOF'"),
            ("EOF".to_string(), false, false, false)
        );
        // <<"E\nOF" keeps the backslash-n pair literal in the delimiter name;
        // quoting controls body interpolation, not label unescaping.
        assert_eq!(
            parse_heredoc_delimiter("<<\"E\\nOF\""),
            ("E\\nOF".to_string(), true, false, false)
        );
    }

    #[test]
    fn parses_literal_and_command_labels() {
        assert_eq!(
            parse_heredoc_delimiter(r"<<\EOF"),
            ("EOF".to_string(), false, false, false)
        );
        let command = format!("<<{}echo EOF{}", 96 as char, 96 as char);
        assert_eq!(
            parse_heredoc_delimiter(&command),
            ("echo EOF".to_string(), true, false, true)
        );
    }

    #[test]
    fn maps_quote_kinds_for_supported_delimiters() {
        assert!(matches!(
            map_heredoc_quote_kind(r"<<\EOF", false),
            QuoteKind::Single
        ));
        assert!(matches!(
            map_heredoc_quote_kind("<<'EOF'", false),
            QuoteKind::Single
        ));
        assert!(matches!(
            map_heredoc_quote_kind("<<\"EOF\"", true),
            QuoteKind::Double
        ));
        let command = format!("<<{}EOF{}", 96 as char, 96 as char);
        assert!(matches!(
            map_heredoc_quote_kind(&command, true),
            QuoteKind::Backtick
        ));
        assert!(matches!(
            map_heredoc_quote_kind("<<EOF", true),
            QuoteKind::Unquoted
        ));
    }

    #[test]
    fn advances_over_lf_crlf_and_end_of_input() {
        assert_eq!(after_line_break(b"abc\ndef", 0), 4);
        assert_eq!(after_line_break(b"abc\r\ndef", 0), 5);
        assert_eq!(after_line_break(b"abc\rdef", 0), 4);
        assert_eq!(after_line_break(b"abcdef", 0), 6);
        assert_eq!(after_line_break(b"abc\ndef", 99), 99);
    }

    // --- parse_heredoc_delimiter indented-combination branch coverage ---

    /// Indented heredoc (`<<~`) combined with every quoting style must set
    /// `indented = true` while preserving the correct interpolated / command
    /// semantics for each quote kind.  The non-indented variants are already
    /// covered by `parses_indented_and_quoted_labels` and
    /// `parses_literal_and_command_labels`; this test targets the previously
    /// missing `indented = true` × quote-kind cross product.
    #[test]
    fn parses_indented_combined_with_every_quote_style() {
        // <<~'EOF'  — indented + single-quoted → not interpolated, not command
        assert_eq!(
            parse_heredoc_delimiter("<<~'EOF'"),
            ("EOF".to_string(), false, true, false)
        );

        // <<~"EOF"  — indented + double-quoted → interpolated, not command
        assert_eq!(
            parse_heredoc_delimiter("<<~\"EOF\""),
            ("EOF".to_string(), true, true, false)
        );

        // <<~`cmd`  — indented + backtick → interpolated, command execution
        let indented_backtick = format!("<<~{}cmd{}", 96u8 as char, 96u8 as char);
        assert_eq!(
            parse_heredoc_delimiter(&indented_backtick),
            ("cmd".to_string(), true, true, true)
        );

        // <<~\EOF  — indented + backslash-quoted → not interpolated, not command
        assert_eq!(
            parse_heredoc_delimiter("<<~\\EOF"),
            ("EOF".to_string(), false, true, false)
        );
    }

    /// `<<~;` and `<<~` (empty after the tilde) must trigger the early-return
    /// path with `indented = true`.  The non-indented equivalents (`<<;`, `<<`)
    /// are already covered; this test proves the indented flag is preserved even
    /// on the early-exit path.
    #[test]
    fn parses_indented_empty_and_semicolon_labels() {
        // <<~   — nothing after the tilde
        assert_eq!(
            parse_heredoc_delimiter("<<~"),
            (String::new(), true, true, false)
        );
        // <<~;  — semicolon acts as the statement terminator, not part of the label
        assert_eq!(
            parse_heredoc_delimiter("<<~;"),
            (String::new(), true, true, false)
        );
    }

    // --- map_heredoc_quote_kind indented-prefix branch coverage ---

    /// `map_heredoc_quote_kind` strips both `<` characters and the optional `~`
    /// before inspecting the quote delimiter.  Callers can pass the raw token
    /// text with the `~` prefix; the function must resolve to the same
    /// QuoteKind as the non-indented equivalent.
    #[test]
    fn maps_quote_kinds_for_indented_delimiters() {
        // <<~'EOF' → Single (same as <<\EOF and <<'EOF')
        assert!(matches!(
            map_heredoc_quote_kind("<<~'EOF'", false),
            QuoteKind::Single
        ));
        // <<~"EOF" → Double
        assert!(matches!(
            map_heredoc_quote_kind("<<~\"EOF\"", true),
            QuoteKind::Double
        ));
        // <<~`EOF` → Backtick
        let indented_backtick = format!("<<~{}EOF{}", 96u8 as char, 96u8 as char);
        assert!(matches!(
            map_heredoc_quote_kind(&indented_backtick, true),
            QuoteKind::Backtick
        ));
        // <<~EOF   → Unquoted (bare word)
        assert!(matches!(
            map_heredoc_quote_kind("<<~EOF", true),
            QuoteKind::Unquoted
        ));
        // <<~\EOF  → Single (backslash-quoted is mapped to Single)
        assert!(matches!(
            map_heredoc_quote_kind("<<~\\EOF", false),
            QuoteKind::Single
        ));
    }
}
