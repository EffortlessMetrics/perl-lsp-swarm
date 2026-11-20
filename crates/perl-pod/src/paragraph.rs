//! Byte-preserving physical line iteration over Perl source (#8444).

/// One physical source line with exact byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalLine<'a> {
    /// Byte offset where the line begins.
    pub start: usize,
    /// Byte offset after the line content (before the line terminator).
    pub content_end: usize,
    /// Byte offset after the line terminator (or `content_end` at EOF).
    pub line_end: usize,
    /// Line text without terminators.
    pub content: &'a str,
}

/// Iterate physical lines preserving LF, CRLF, and bare CR.
#[derive(Debug, Clone)]
pub struct PhysicalLines<'a> {
    source: &'a str,
    next_start: usize,
}

impl<'a> PhysicalLines<'a> {
    /// Create a line iterator over `source`.
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self { source, next_start: 0 }
    }
}

impl<'a> Iterator for PhysicalLines<'a> {
    type Item = PhysicalLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_start >= self.source.len() {
            return None;
        }
        let start = self.next_start;
        let rest = &self.source[start..];
        let mut content_end = start;
        let mut line_end = start;
        let mut found = false;
        for (idx, ch) in rest.char_indices() {
            if ch == '\n' {
                content_end = start + idx;
                line_end = start + idx + 1;
                found = true;
                break;
            }
            if ch == '\r' {
                content_end = start + idx;
                line_end = if rest.as_bytes().get(idx + 1) == Some(&b'\n') {
                    start + idx + 2
                } else {
                    start + idx + 1
                };
                found = true;
                break;
            }
        }
        if !found {
            content_end = self.source.len();
            line_end = self.source.len();
        }
        self.next_start = line_end;
        Some(PhysicalLine {
            start,
            content_end,
            line_end,
            content: &self.source[start..content_end],
        })
    }
}

/// Returns `true` when the physical line is blank (empty or whitespace only).
#[must_use]
pub fn is_blank_line(content: &str) -> bool {
    content.chars().all(char::is_whitespace)
}

/// Known POD command names recognized by the legacy extractor.
const KNOWN_COMMANDS: &[&str] = &[
    "pod", "cut", "head1", "head2", "head3", "head4", "head5", "head6", "over", "back", "item",
    "begin", "end", "for", "encoding",
];

/// Parsed column-zero POD command paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand {
    /// Command name without `=`.
    pub name: String,
    /// Full command paragraph range (line content only).
    pub range: crate::model::PodRange,
    /// Argument text range within the source.
    pub argument_range: crate::model::PodRange,
    /// Whether the command is in the known vocabulary.
    pub known: bool,
}

/// Parse a column-zero POD command paragraph from one physical line.
#[must_use]
pub fn parse_command_line(line: &PhysicalLine<'_>) -> Option<ParsedCommand> {
    let content = line.content;
    if !content.starts_with('=') {
        return None;
    }
    let rest = content.strip_prefix('=')?;
    let cmd_end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    if cmd_end == 0 {
        return None;
    }
    let cmd = rest.get(..cmd_end)?;
    if !cmd.chars().all(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let after = rest.get(cmd_end..)?;
    if !after.is_empty() && !after.chars().next().is_some_and(|c| c == ' ' || c == '\t') {
        return None;
    }
    let arg_start = line.start + 1 + cmd_end;
    let arg_trimmed = after.trim();
    let argument_range = if arg_trimmed.is_empty() {
        crate::model::PodRange { start: arg_start, end: arg_start }
    } else {
        let trim_leading = after.len() - after.trim_start().len();
        let start = arg_start + trim_leading;
        let end = start + arg_trimmed.len();
        crate::model::PodRange { start, end }
    };
    let known = KNOWN_COMMANDS.contains(&cmd);
    Some(ParsedCommand {
        name: cmd.to_string(),
        range: crate::model::PodRange { start: line.start, end: line.content_end },
        argument_range,
        known,
    })
}

/// Returns `true` when a physical line begins a verbatim paragraph.
#[must_use]
pub fn line_starts_verbatim(content: &str) -> bool {
    content.chars().next().is_some_and(|c| c == ' ' || c == '\t')
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_some_with;

    #[test]
    fn physical_lines_split_lf_crlf_and_cr() {
        let lf = "a\nb";
        let lines: Vec<_> = PhysicalLines::new(lf).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, "a");
        assert_eq!(lines[1].content, "b");

        let crlf = "a\r\nb";
        let lines: Vec<_> = PhysicalLines::new(crlf).collect();
        assert_eq!(lines[0].content, "a");
        assert_eq!(lines[1].content, "b");

        let cr = "a\rb";
        let lines: Vec<_> = PhysicalLines::new(cr).collect();
        assert_eq!(lines[0].content, "a");
        assert_eq!(lines[1].content, "b");
    }

    #[test]
    fn whitespace_prefix_predicate() {
        let after = " NAME";
        assert!(
            after.chars().next().is_some_and(char::is_whitespace),
            "expected leading whitespace"
        );
    }

    #[test]
    fn parse_command_line_matches_legacy_vocabulary() {
        let source = "=head1 NAME";
        let line = must_some_with(PhysicalLines::new(source).next(), "physical line");
        let cmd = must_some_with(parse_command_line(&line), "head1 command");
        assert_eq!(cmd.name, "head1");
        assert!(cmd.known);
        assert_eq!(&source[cmd.argument_range.start..cmd.argument_range.end], "NAME");
    }

    #[test]
    fn parse_command_line_rejects_prefix_lookalikes() {
        for text in ["=cutlery", "=headache", "=head1:"] {
            let line = must_some_with(PhysicalLines::new(text).next(), "physical line");
            if text == "=headache" || text == "=cutlery" {
                let cmd = must_some_with(parse_command_line(&line), "unknown command");
                assert!(!cmd.known);
            } else {
                assert!(parse_command_line(&line).is_none(), "{text}");
            }
        }
    }
}
