use crate::syntax::cursor::RegexCursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmbeddedCodeKind {
    Immediate,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmbeddedCodeFinding {
    pub(crate) offset: usize,
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
            if cursor.peek(2) == Some(b'{') {
                findings.push(EmbeddedCodeFinding {
                    offset: cursor.position(),
                    kind: EmbeddedCodeKind::Immediate,
                });
            } else if cursor.peek(2) == Some(b'?') && cursor.peek(3) == Some(b'{') {
                findings.push(EmbeddedCodeFinding {
                    offset: cursor.position(),
                    kind: EmbeddedCodeKind::Deferred,
                });
            }
        }
        cursor.bump();
    }

    findings
}
