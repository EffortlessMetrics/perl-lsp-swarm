use crate::syntax::cursor::RegexCursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbeddedCodeKind {
    Immediate,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmbeddedCodeFinding {
    pub(crate) offset: usize,
    pub(crate) end: usize,
    pub(crate) kind: EmbeddedCodeKind,
}

pub(crate) fn find_code_executions(pattern: &str) -> Vec<EmbeddedCodeFinding> {
    let mut cursor = RegexCursor::new(pattern);
    let mut findings = Vec::new();

    while let Some(ch) = cursor.current() {
        if cursor.skip_quoted_literal()
            || cursor.skip_escape()
            || cursor.skip_char_class()
            || cursor.skip_comment()
        {
            continue;
        }
        if ch == b'(' && cursor.peek(1) == Some(b'?') {
            let kind = if cursor.peek(2) == Some(b'{') {
                Some(EmbeddedCodeKind::Immediate)
            } else if cursor.peek(2) == Some(b'?') && cursor.peek(3) == Some(b'{') {
                Some(EmbeddedCodeKind::Deferred)
            } else {
                None
            };
            if let Some(kind) = kind {
                let offset = cursor.position();
                let end = embedded_region_end(pattern.as_bytes(), offset, kind);
                findings.push(EmbeddedCodeFinding { offset, end, kind });
                cursor.advance_to(end);
                continue;
            }
        }
        cursor.bump();
    }

    findings
}

fn embedded_region_end(bytes: &[u8], start: usize, kind: EmbeddedCodeKind) -> usize {
    let prefix_len = match kind {
        EmbeddedCodeKind::Immediate => 3,
        EmbeddedCodeKind::Deferred => 4,
    };
    let mut i = start.saturating_add(prefix_len).min(bytes.len());
    let mut brace_depth = 1usize;
    let mut quote = None;
    let mut escaped = false;

    while i < bytes.len() {
        let ch = bytes[i];
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            i += 1;
            continue;
        }

        match ch {
            b'\'' | b'"' => quote = Some(ch),
            b'{' => brace_depth = brace_depth.saturating_add(1),
            b'}' => {
                brace_depth = brace_depth.saturating_sub(1);
                if brace_depth == 0 {
                    return i.saturating_add(2).min(bytes.len());
                }
            }
            _ => {}
        }
        i += 1;
    }

    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_immediate_and_deferred_openers_once() {
        let findings = find_code_executions(r#"(?{ my $s = '(?{'; }) (??{ later })"#);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].kind, EmbeddedCodeKind::Immediate);
        assert_eq!(findings[0].offset, 0);
        assert!(findings[0].end > findings[0].offset);
        assert_eq!(findings[1].kind, EmbeddedCodeKind::Deferred);
    }

    #[test]
    fn advances_over_balanced_body_with_nested_braces_and_quotes() {
        let pattern = r#"(?{ my $x = { nested => "}" }; })tail"#;
        let findings = find_code_executions(pattern);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            &pattern[findings[0].offset..findings[0].end],
            r#"(?{ my $x = { nested => "}" }; })"#
        );
        assert_eq!(&pattern[findings[0].end..], "tail");
    }

    #[test]
    fn ignores_lookalike_text_outside_code_constructs() {
        assert!(find_code_executions(r"(?:not code) (?x) {").is_empty());
        assert!(find_code_executions(r"\(?{escaped}").is_empty());
    }

    #[test]
    fn peek_boundaries_distinguish_immediate_deferred_and_non_code() {
        let immediate = find_code_executions("(?{x})");
        assert_eq!(immediate.len(), 1);
        assert_eq!(immediate[0].kind, EmbeddedCodeKind::Immediate);
        assert_eq!(immediate[0].offset, 0);
        assert_eq!(immediate[0].end, 6);

        let deferred = find_code_executions("(??{x})");
        assert_eq!(deferred.len(), 1);
        assert_eq!(deferred[0].kind, EmbeddedCodeKind::Deferred);
        assert_eq!(deferred[0].offset, 0);
        assert_eq!(deferred[0].end, 7);

        assert!(find_code_executions("(?=").is_empty());
        assert!(find_code_executions("(?").is_empty());
        assert!(find_code_executions("(").is_empty());
    }
}
