//! Stray `=cut` diagnostic (PL412).
//!
//! A line-initial `=cut` with no preceding POD opener still starts a POD block
//! in perl: everything after it is documentation until the next `=cut` or EOF.
//! perl -c is silent. The lexer skips POD and never emits a `Pod` token, so this
//! lint scans source bytes directly and must run even when the amputation also
//! produces blocking parse errors.

use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;

use super::super::internal_types::{Diagnostic, RelatedInformation};

/// Warn when a line-initial `=cut` opens POD because no opener preceded it.
pub fn check_stray_pod_cut(source: &str, diagnostics: &mut Vec<Diagnostic>) {
    let bytes = source.as_bytes();
    let mut in_pod = false;
    let mut last_significant: Option<u8> = None;
    let mut offset = 0;

    while offset < bytes.len() {
        let line_start = offset;
        let rest = &bytes[offset..];
        let rel_end = rest.iter().position(|&b| b == b'\n' || b == b'\r').unwrap_or(rest.len());
        let line = &rest[..rel_end];

        match classify_pod_line(line) {
            Some(PodLine::Cut) if in_pod => {
                in_pod = false;
                last_significant = None;
            }
            Some(kind) if !in_pod && at_directive_context(last_significant) => match kind {
                PodLine::Cut => {
                    emit_stray_cut(line_start, diagnostics);
                    in_pod = true;
                }
                PodLine::Opener => in_pod = true,
            },
            _ if !in_pod => {
                if line_is_blank(line) {
                    last_significant = None;
                } else if let Some(sig) = last_significant_on_line(line) {
                    last_significant = Some(sig);
                }
            }
            _ => {}
        }

        offset = line_start.saturating_add(rel_end);
        if offset >= bytes.len() {
            break;
        }
        if bytes[offset] == b'\r' {
            offset = offset.saturating_add(1);
            if offset < bytes.len() && bytes[offset] == b'\n' {
                offset = offset.saturating_add(1);
            }
        } else {
            offset = offset.saturating_add(1);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PodLine {
    Cut,
    Opener,
}

fn classify_pod_line(line: &[u8]) -> Option<PodLine> {
    let command = pod_command(line)?;
    if command == b"cut" { Some(PodLine::Cut) } else { Some(PodLine::Opener) }
}

/// POD command word after a column-0 `=`, if this line is a directive.
fn pod_command(line: &[u8]) -> Option<&[u8]> {
    if line.first().copied() != Some(b'=') {
        return None;
    }
    let rest = line.get(1..)?;
    let first = rest.first()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = 1;
    while end < rest.len() && (rest[end].is_ascii_alphanumeric() || rest[end] == b'_') {
        end += 1;
    }
    rest.get(..end)
}

fn at_directive_context(last_significant: Option<u8>) -> bool {
    match last_significant {
        None => true,
        Some(b';' | b'{' | b'}') => true,
        Some(_) => false,
    }
}

fn line_is_blank(line: &[u8]) -> bool {
    line.iter().all(|c| matches!(c, b' ' | b'\t' | 0x0c))
}

fn last_significant_on_line(line: &[u8]) -> Option<u8> {
    let mut end = line.len();
    for (i, &c) in line.iter().enumerate() {
        if c == b'#' && (i == 0 || line.get(i.saturating_sub(1)).copied() != Some(b'$')) {
            end = i;
            break;
        }
    }
    line.get(..end)?.iter().copied().rev().find(|c| !c.is_ascii_whitespace())
}

fn emit_stray_cut(start: usize, diagnostics: &mut Vec<Diagnostic>) {
    let end = start.saturating_add(4);
    diagnostics.push(Diagnostic {
        range: (start, end),
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::StrayPodCut.as_str().to_string()),
        message: "=cut without a preceding =pod; the parser treats the rest of the file as POD"
            .to_string(),
        related_information: vec![RelatedInformation {
            location: (start, end),
            message: "Remove the stray `=cut`, or open documentation with `=pod` / `=head1` \
                      before closing it."
                .to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Delete the stray `=cut`, or wrap the documentation in `=pod` ... `=cut`.".to_string(),
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::{PodLine, check_stray_pod_cut};
    use super::{at_directive_context, classify_pod_line, last_significant_on_line, pod_command};
    use crate::providers::diagnostics::Diagnostic;

    fn ranges(source: &str) -> Vec<(usize, usize)> {
        let mut diagnostics = Vec::new();
        check_stray_pod_cut(source, &mut diagnostics);
        diagnostics.into_iter().map(|d: Diagnostic| d.range).collect()
    }

    #[test]
    fn pod_command_is_word_bounded_and_column_zero() {
        assert_eq!(pod_command(b"=cut"), Some(&b"cut"[..]));
        assert_eq!(pod_command(b"=cut; still pod"), Some(&b"cut"[..]));
        assert_eq!(pod_command(b"=cutlery"), Some(&b"cutlery"[..]));
        assert_eq!(pod_command(b"=head1 NAME"), Some(&b"head1"[..]));
        assert_eq!(pod_command(b" =cut"), None);
        assert_eq!(pod_command(b"=1cut"), None);
    }

    #[test]
    fn classify_distinguishes_cut_from_openers() {
        assert_eq!(classify_pod_line(b"=cut"), Some(PodLine::Cut));
        assert_eq!(classify_pod_line(b"=pod"), Some(PodLine::Opener));
        assert_eq!(classify_pod_line(b"=cutlery"), Some(PodLine::Opener));
        assert_eq!(classify_pod_line(b"sub x { 1 }"), None);
    }

    #[test]
    fn statement_end_includes_brace_close() {
        assert!(at_directive_context(None));
        assert!(at_directive_context(Some(b';')));
        assert!(at_directive_context(Some(b'{')));
        assert!(at_directive_context(Some(b'}')));
        assert!(!at_directive_context(Some(b'1')));
    }

    #[test]
    fn last_significant_skips_line_comments() {
        assert_eq!(last_significant_on_line(b"sub before { 1 }"), Some(b'}'));
        assert_eq!(last_significant_on_line(b"use strict; # note"), Some(b';'));
        assert_eq!(last_significant_on_line(b"my $x = 1"), Some(b'1'));
        assert_eq!(last_significant_on_line(b"print $#_"), Some(b'_'));
        assert_eq!(last_significant_on_line(b"   "), None);
    }

    #[test]
    fn scanner_warns_after_sub_close_and_not_on_assignment() {
        assert_eq!(ranges("sub before { 1 }\n=cut\nsub after { 2 }\n").len(), 1);
        assert!(ranges("my $x = 1\n=cut\n").is_empty());
        assert_eq!(ranges("my $x = 1\n\n=cut\n").len(), 1);
        assert_eq!(ranges("use strict;\n=cut\nnot code\n=cut\nsub recovered { 1 }\n").len(), 1);
        assert!(ranges("use strict;\n=pod\nDocs\n=cut\nsub after { 1 }\n").is_empty());
    }
}
