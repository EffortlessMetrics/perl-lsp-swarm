//! Edge cases for the canonical parser-backed trivia surface.

use perl_parser_core::Parser;
use perl_parser_core::trivia::{Trivia, TriviaLexer};
use perl_parser_core::trivia_parser::TriviaPreservingParser;
use perl_tdd_support::must_some;

#[test]
fn pod_without_cut_is_retained() {
    let source = r#"my $x = 1;

=head1 DESCRIPTION

This is documentation without a closing =cut
"#
    .to_string();

    let output = TriviaPreservingParser::new(source.clone()).parse();

    assert_eq!(output.source(), source);
    assert!(output.trivia.iter().any(|token| matches!(&token.trivia, Trivia::PodComment(_))));
}

#[test]
fn pod_at_start_of_file_is_retained() {
    let source = r#"=head1 NAME

MyModule - A test module

=cut

package MyModule;
"#
    .to_string();

    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| matches!(&token.trivia, Trivia::PodComment(_))));
}

#[test]
fn comment_without_newline_at_eof_is_retained() {
    let source = "my $x = 1; # comment without newline".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output
        .trivia
        .iter()
        .any(|token| matches!(&token.trivia, Trivia::LineComment(text) if text == "# comment without newline")));
}

#[test]
fn comment_after_division_uses_stateful_lexer_context() {
    let source = "my $x = 10 / 2; # keep".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(
        output
            .trivia
            .iter()
            .any(|token| matches!(&token.trivia, Trivia::LineComment(text) if text == "# keep"))
    );
}

#[test]
fn windows_line_endings_keep_comments_and_exact_source() {
    let source = "# Comment\r\nmy $x = 1;\r\n# Another\r\n".to_string();
    let output = TriviaPreservingParser::new(source.clone()).parse();
    let comment_count = output
        .trivia
        .iter()
        .filter(|token| matches!(&token.trivia, Trivia::LineComment(_)))
        .count();

    assert_eq!(output.source(), source);
    assert!(comment_count >= 2);
}

#[test]
fn unicode_in_comments_is_preserved() {
    let source = "# This comment has Unicode: 🦀 日本語\nmy $x = 1;".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| {
        matches!(&token.trivia, Trivia::LineComment(text) if text.contains('🦀') && text.contains('日'))
    }));
}

#[test]
fn unicode_before_comment_uses_character_columns() {
    let source = "my $é; # hi".to_string();
    let output = TriviaPreservingParser::new(source).parse();
    let comment = must_some(
        output
            .trivia
            .iter()
            .find(|token| matches!(&token.trivia, Trivia::LineComment(text) if text == "# hi")),
    );

    assert_eq!(comment.range.start.byte, 8);
    assert_eq!(comment.range.start.line, 1);
    assert_eq!(comment.range.start.column, 8);
}

#[test]
fn mixed_tabs_and_spaces_are_exact() {
    let source = " \t \t my $x = 1;".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(matches!(
        output.trivia.first().map(|token| &token.trivia),
        Some(Trivia::Whitespace(text)) if text == " \t \t "
    ));
}

#[test]
fn shebang_variations_are_comments_without_ast_fabrication() {
    for source in [
        "#!/usr/bin/perl\n",
        "#!/usr/bin/env perl\n",
        "#!/usr/local/bin/perl -w\n",
        "#! /usr/bin/perl\n",
    ] {
        let output = TriviaPreservingParser::new(source.to_string()).parse();

        assert!(output.trivia.iter().any(|token| {
            matches!(&token.trivia, Trivia::LineComment(text) if text.starts_with("#!"))
        }));
        assert!(matches!(&output.parse.ast.kind, perl_parser_core::NodeKind::Program { .. }));
    }
}

#[test]
fn multiple_empty_lines_remain_distinct_trivia() {
    let source = "my $x = 1;\n\n\n\nmy $y = 2;".to_string();
    let output = TriviaPreservingParser::new(source).parse();
    let newline_count =
        output.trivia.iter().filter(|token| matches!(&token.trivia, Trivia::Newline)).count();

    assert!(newline_count >= 4);
}

#[test]
fn pod_with_special_commands_is_retained() {
    let source = r#"=pod

=encoding utf8

=for html <div>content</div>

=begin text

Some text block

=end text

=cut

my $x = 1;
"#
    .to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| matches!(&token.trivia, Trivia::PodComment(_))));
}

#[test]
fn custom_pod_commands_follow_canonical_marker_rules() {
    let source = "=custom metadata\nbody\n=cut\nmy $x = 1;\n".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| {
        matches!(&token.trivia, Trivia::PodComment(text) if text.contains("=custom metadata"))
    }));
}

#[test]
fn cut_prefix_does_not_end_pod_without_a_command_boundary() {
    let source = "=pod\n=cutlery\nstill pod\n=cut\nmy $x = 1;\n".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| {
        matches!(&token.trivia, Trivia::PodComment(text) if text.contains("=cutlery\nstill pod") && text.ends_with("=cut\n"))
    }));
}

#[test]
fn hash_in_string_is_not_comment_trivia() {
    let source = "my $x = \"# not a comment\";".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().all(|token| !matches!(&token.trivia, Trivia::LineComment(_))));
}

#[test]
fn heredoc_hashes_do_not_crash_canonical_parse() {
    let source = r#"my $text = <<'END';
# This is not a comment
# It's part of the here-doc
END
my $x = 1;
"#
    .to_string();
    let output = TriviaPreservingParser::new(source.clone()).parse();

    assert_eq!(output.source(), source);
    assert!(matches!(&output.parse.ast.kind, perl_parser_core::NodeKind::Program { .. }));
    assert!(output.trivia.iter().all(|token| !matches!(&token.trivia, Trivia::LineComment(_))));
}

#[test]
fn equals_expression_is_not_pod() {
    let source = "my $x = 1;\nmy $result = $x == 42;\n".to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().all(|token| !matches!(&token.trivia, Trivia::PodComment(_))));
}

#[test]
fn inline_pod_is_retained_without_replacing_canonical_ast() {
    let source = r#"sub foo {
    my $x = shift;

=for comment
Hidden documentation
=cut

    return $x * 2;
}
"#
    .to_string();
    let output = TriviaPreservingParser::new(source.clone()).parse();
    let mut canonical = Parser::new(&source);
    let canonical_output = canonical.parse_with_recovery();

    assert!(output.trivia.iter().any(|token| matches!(&token.trivia, Trivia::PodComment(_))));
    assert_eq!(output.parse.ast.to_sexp(), canonical_output.ast.to_sexp());
    assert_eq!(output.parse.diagnostics, canonical_output.diagnostics);
}

#[test]
fn unicode_whitespace_does_not_replace_canonical_parser() {
    let source = "my\u{00A0}$x\u{2003}=\u{3000}1;".to_string();
    let output = TriviaPreservingParser::new(source.clone()).parse();

    assert_eq!(output.source(), source);
    assert!(matches!(&output.parse.ast.kind, perl_parser_core::NodeKind::Program { .. }));
}

#[test]
fn bare_carriage_returns_preserve_exact_source() {
    let source = "my $x = 1;\rmy $y = 2;\r".to_string();
    let output = TriviaPreservingParser::new(source.clone()).parse();

    assert_eq!(output.source(), source);
}

#[test]
fn nested_pod_is_retained_as_one_source_region_for_now() {
    let source = r#"=begin html

=begin nested
This shouldn't work but let's test it
=end nested

=end html

=cut

my $x = 1;
"#
    .to_string();
    let output = TriviaPreservingParser::new(source).parse();

    assert!(output.trivia.iter().any(|token| matches!(&token.trivia, Trivia::PodComment(_))));
}

#[test]
fn low_level_legacy_lexer_still_collects_trivia_during_migration() {
    let source = "# comment\nmy $x = 1;".to_string();
    let mut lexer = TriviaLexer::new(&source);
    let (_, trivia) = must_some(lexer.next_token_with_trivia());

    assert!(trivia.iter().any(|token| matches!(&token.trivia, Trivia::LineComment(_))));
}
