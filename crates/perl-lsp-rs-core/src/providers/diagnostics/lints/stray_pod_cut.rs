//! Stray `=cut` diagnostic (PL412).
//!
//! A line-initial `=cut` with no preceding POD opener still starts a POD block
//! in perl: everything after it is documentation until the next `=cut` or EOF.
//! perl -c is silent. The lexer skips the block and never emits a `Pod` token,
//! so this lint reads the lexer's own amputation record
//! ([`perl_lexer::StrayCutOpen`]) instead of re-deriving the skip decision
//! from source bytes — a text scan cannot exclude `=cut` embedded in heredocs,
//! strings, or regexes without re-implementing the lexer's opinion. It must
//! run even when the amputation also produces blocking parse errors, because
//! the #5089 gate would otherwise hide the one finding that explains the
//! amputation.

use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_lexer::find_stray_cut_opens_lexed;

use super::super::internal_types::{Diagnostic, RelatedInformation};

/// Warn when a line-initial `=cut` opens POD because no opener preceded it.
///
/// `generation_stray_cuts` is the generation-owned record from
/// [`crate::providers::diagnostics::DocumentDiagnosticAnalysis::stray_cut_opens`]
/// when the caller holds one bound to this exact source; `None` lexes the
/// source directly. Either way the events are the lexer's own amputation
/// decision, so the two paths emit identical diagnostics — the cache only
/// moves the lexing cost from every diagnostics evaluation to once per
/// accepted generation (#19051 review).
pub fn check_stray_pod_cut(
    source: &str,
    generation_stray_cuts: Option<&[(usize, bool)]>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let direct: Vec<(usize, bool)>;
    let events: &[(usize, bool)] = match generation_stray_cuts {
        Some(cached) => cached,
        None => {
            direct = find_stray_cut_opens_lexed(source);
            &direct
        }
    };
    for (cut_offset, runs_to_eof) in events {
        emit_stray_cut(source, *cut_offset, *runs_to_eof, diagnostics);
    }
}

/// The whole `=cut` line, which is exactly the span the lexer dropped.
fn stray_cut_line_range(source: &str, cut_offset: usize) -> (usize, usize) {
    let start = source[..cut_offset.min(source.len())].rfind(['\n', '\r']).map_or(0, |idx| idx + 1);
    let end = source[start..].find(['\n', '\r']).map_or(source.len(), |idx| start + idx);
    (start, end)
}

fn emit_stray_cut(
    source: &str,
    cut_offset: usize,
    runs_to_eof: bool,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let range = stray_cut_line_range(source, cut_offset);
    let extent = if runs_to_eof {
        "perl treats the rest of the file as POD, so the trailing code is not compiled or indexed"
    } else {
        "perl treats the following lines as POD, up to the next =cut, so they are not compiled or \
         indexed"
    };
    diagnostics.push(Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::StrayPodCut.as_str().to_string()),
        message: format!("=cut without a preceding =pod; {extent}"),
        related_information: vec![RelatedInformation {
            location: range,
            message: "Remove the stray `=cut`, or open documentation with `=pod` / `=head1` \
                      before closing it."
                .to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        parse_error_fix_kind: None,
        suggestion: Some(
            "Delete the stray `=cut`, or wrap the documentation in `=pod` ... `=cut`.".to_string(),
        ),
    });
}

#[cfg(test)]
mod tests {
    use super::check_stray_pod_cut;
    use crate::providers::diagnostics::Diagnostic;

    fn findings(source: &str) -> Vec<(usize, usize, String)> {
        let mut diagnostics = Vec::new();
        check_stray_pod_cut(source, None, &mut diagnostics);
        diagnostics.into_iter().map(|d: Diagnostic| (d.range.0, d.range.1, d.message)).collect()
    }

    #[test]
    fn stray_cut_after_statement_end_warns_on_the_cut_line() {
        let source = "use strict;\nsub before { 1 };\n=cut\nsub after { 2 }\n";
        let found = findings(source);
        assert_eq!(found.len(), 1, "a lone =cut at statement start must warn: {found:?}");
        let (start, end, message) = &found[0];
        let line_start =
            perl_test_must::must_some_with(source.find("=cut"), "fixture must contain =cut");
        assert_eq!(*start, line_start, "the range must start at the =cut line start");
        assert_eq!(*end, line_start + 4, "the =cut line here is exactly the directive");
        assert!(message.contains("=cut"), "the message must mention =cut: {message}");
        assert!(
            message.to_ascii_lowercase().contains("pod"),
            "the message must explain the POD amputation: {message}"
        );
    }

    /// The lexer's statement-start context is deliberately conservative
    /// (`;` / `{` / blank line — see `pod_after_statement_end`): a `=cut`
    /// directly after a bare closing brace stays code there, and the parser
    /// reports the assignment-shaped error instead. `perl -c` would treat
    /// that `=cut` as POD; aligning that context is lexer-oracle work, and
    /// the diagnostic must not claim an amputation the model did not perform.
    #[test]
    fn cut_after_bare_block_brace_is_silent() {
        let source = "sub before { 1 }\n=cut\nsub after { 2 }\n";
        assert!(
            findings(source).is_empty(),
            "without statement-end context the =cut stays code, so no amputation to report: {:?}",
            findings(source)
        );
    }

    #[test]
    fn runs_to_eof_and_bounded_variants_name_their_extent() {
        let to_eof = findings("use strict;\n=cut\nhidden code\n");
        assert_eq!(to_eof.len(), 1, "{to_eof:?}");
        assert!(
            to_eof[0].2.contains("rest of the file"),
            "the EOF variant must say the rest of the file became POD: {}",
            to_eof[0].2
        );

        let bounded = findings("use strict;\n=cut\nhidden\n=cut\nmy $x = 1;\n");
        assert_eq!(
            bounded.len(),
            1,
            "the opening stray =cut warns once; the closer does not: {bounded:?}"
        );
        assert!(
            bounded[0].2.contains("up to the next =cut"),
            "the bounded variant must name the extent: {}",
            bounded[0].2
        );
    }

    #[test]
    fn assignment_continuation_cut_is_silent() {
        assert!(
            findings("my $x = 1\n=cut\n").is_empty(),
            "a =cut that continues an expression is code, not POD"
        );
    }

    #[test]
    fn blank_line_preceded_cut_warns() {
        assert_eq!(findings("my $x = 1\n\n=cut\n").len(), 1);
    }

    #[test]
    fn pod_opened_block_is_silent() {
        assert!(
            findings("use strict;\n=pod\nDocs\n=cut\nsub after { 1 }\n").is_empty(),
            "a =cut closing a =pod block is not stray"
        );
        assert!(
            findings("package Foo;\n=head1 NAME\n\nFoo - a module\n\n=cut\nsub bar { 1 }\n")
                .is_empty(),
            "ordinary =head1 ... =cut documentation is not stray"
        );
    }

    #[test]
    fn cutlery_is_silent() {
        assert!(
            findings("use strict;\n=cutlery\nnot a cut directive\n").is_empty(),
            "=cutlery is a distinct POD command"
        );
    }

    #[test]
    fn cut_inside_multiline_string_is_silent() {
        let source = "my $s = \"first\n=cut\nlast\";\n";
        assert!(
            findings(source).is_empty(),
            "=cut inside a string must not warn: {:?}",
            findings(source)
        );
    }

    #[test]
    fn cut_inside_heredoc_body_is_silent() {
        let source = "my $text = <<'END';\n=cut\nEND\nprint $text;\n";
        assert!(
            findings(source).is_empty(),
            "=cut inside a heredoc body must not warn: {:?}",
            findings(source)
        );
    }

    #[test]
    fn cut_inside_multiline_regex_is_silent() {
        let source = "my $r = /\n=cut\n/;\nprint \"ok\";\n";
        assert!(
            findings(source).is_empty(),
            "=cut inside a regex literal must not warn: {:?}",
            findings(source)
        );
    }

    #[test]
    fn file_leading_cut_warns() {
        assert_eq!(findings("=cut\nsub hidden { 1 }\n").len(), 1);
    }

    /// A generation-owned cache must produce byte-identical findings to the
    /// direct lex path (#19051 review).
    #[test]
    fn cached_generation_events_match_direct_lexing() {
        use perl_lexer::find_stray_cut_opens_lexed;
        for source in [
            "use strict;\nsub before { 1 };\n=cut\nsub after { 2 }\n",
            "use strict;\n=cut\nhidden\n=cut\nmy $x = 1;\n",
            "my $text = <<'END';\n=cut\nEND\nprint $text;\n",
            "my $s = \"first\n=cut\nlast\";\n",
        ] {
            let mut direct = Vec::new();
            check_stray_pod_cut(source, None, &mut direct);
            let cached_slice = find_stray_cut_opens_lexed(source);
            let mut cached = Vec::new();
            check_stray_pod_cut(source, Some(&cached_slice), &mut cached);
            assert_eq!(
                direct.iter().map(|d| (d.range, &d.message)).collect::<Vec<_>>(),
                cached.iter().map(|d| (d.range, &d.message)).collect::<Vec<_>>(),
                "cached and direct paths must agree for {source:?}"
            );
        }
    }
}
