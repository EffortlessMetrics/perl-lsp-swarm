//! Remaining `expected_colon` pattern from CPAN `Test/Needs.pm` (#2721).
//!
//! Nine of the ten historical `expected_colon` files parse cleanly on current
//! main. The last first-error is a nested list-operator then-branch:
//! `_croak sprintf qq{"%s"}, $module`. `looks_like_bare_call` already admits
//! a builtin argument with parens (`croak ref($x)`) or a sigil
//! (`croak sprintf $fmt`), but not a quote-like first argument.

use crate::cpan_test_helpers::*;

use perl_parser_core::Parser;
use perl_parser_core::error::{ParseError, RecoveryKind, RecoverySite};

fn assert_clean_parse_without_blocking_diagnostics(source: &str) {
    assert_clean_parse(source);
    assert_no_blocking_diagnostics(source);
}

fn missing_semicolons(source: &str) -> usize {
    let mut parser = Parser::new(source);
    let _ = parser.parse();
    parser
        .errors()
        .iter()
        .filter(|error| {
            matches!(
                error,
                ParseError::Recovered {
                    site: RecoverySite::Statement,
                    kind: RecoveryKind::InferredSemicolon,
                    ..
                }
            )
        })
        .count()
}

#[test]
fn croak_sprintf_qq_in_ternary_then_branch() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? _croak sprintf qq{"%s"}, $module : 1;"#,
    );
}

#[test]
fn croak_sprintf_double_quoted_in_ternary_then_branch() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? _croak sprintf "%s", $module : 1;"#,
    );
}

#[test]
fn croak_sprintf_q_in_ternary_then_branch() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? _croak sprintf q{%s}, $module : 1;"#,
    );
}

#[test]
fn croak_join_comma_in_ternary_then_branch() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? _croak join ',', @names : 1;"#,
    );
}

#[test]
fn croak_sprintf_sigil_still_clean() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? _croak sprintf $module : 1;"#,
    );
}

#[test]
fn sprintf_qq_as_ternary_then_branch_itself() {
    assert_clean_parse_without_blocking_diagnostics(
        r#"my $x = $cond ? sprintf qq{"%s"}, $module : 1;"#,
    );
}

#[test]
fn adjacent_print_string_is_not_swallowed_as_nested_call() {
    // Pattern C must not treat `foo print "hi"` as `foo(print("hi"))`.
    let source = "foo\nprint \"hi\";\n";
    let ast = parse(source);
    let kinds = top_level_kinds(&ast);
    assert!(
        kinds.len() >= 2,
        "print-string after a bareword must remain a separate statement, got {kinds:?}\n{source}"
    );
}

#[test]
fn nullary_sub_newline_sprintf_string_is_not_swallowed_as_nested_call() {
    let source = "sub foo () { 1 }\nfoo\nsprintf \"%s\", 1;\n";
    assert_eq!(
        missing_semicolons(source),
        1,
        "nullary foo must not absorb a newline-separated sprintf call"
    );
}

#[test]
fn list_operator_sub_newline_sprintf_string_remains_one_call() {
    assert_clean_parse_without_blocking_diagnostics(
        "sub _croak { die @_ }\n_croak\nsprintf \"%s\", 1;\n",
    );
}

#[test]
fn test_needs_find_missing_colonful_match_then_croak_sprintf() {
    // Exact remaining first-error shape from Test/Needs.pm `_find_missing`.
    assert_clean_parse_without_blocking_diagnostics(
        r#"
sub _find_missing {
  my @bad = map {
    my ($module, $version) = @$_;
    $module eq 'perl' ? do {
      $version = _numify_version($version);
      "$]" < $version ? (sprintf "perl %s (have %.6f)", $version, $]) : ()
    }
    : $module =~ /^\d|[^\w:]|:::|[^:]:[^:]|^:|:$/
      ? _croak sprintf qq{"%s" does not look like a module name}, $module
    : _try_require($module) ? (
      defined $version && !_try_version($module, $version)
        ? "$module $version (have ".(defined $module->VERSION ? $module->VERSION : 'undef').')'
        : ()
    )
    : $version ? "$module $version"
    : $module;
  } _pairs(@_);
  @bad ? "Need " . join(', ', @bad) : undef;
}
"#,
    );
}
