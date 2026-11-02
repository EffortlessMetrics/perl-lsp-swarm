//! Remaining `expected_colon` pattern from CPAN `Test/Needs.pm` (#2721).
//!
//! Nine of the ten historical `expected_colon` files parse cleanly on current
//! main. The last first-error is a nested list-operator then-branch:
//! `_croak sprintf qq{"%s"}, $module`. `looks_like_bare_call` already admits
//! a builtin argument with parens (`croak ref($x)`) or a sigil
//! (`croak sprintf $fmt`), but not a quote-like first argument.

mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn croak_sprintf_qq_in_ternary_then_branch() {
    assert_clean_parse(r#"my $x = $cond ? _croak sprintf qq{"%s"}, $module : 1;"#);
}

#[test]
fn croak_sprintf_double_quoted_in_ternary_then_branch() {
    assert_clean_parse(r#"my $x = $cond ? _croak sprintf "%s", $module : 1;"#);
}

#[test]
fn croak_sprintf_q_in_ternary_then_branch() {
    assert_clean_parse(r#"my $x = $cond ? _croak sprintf q{%s}, $module : 1;"#);
}

#[test]
fn croak_join_comma_in_ternary_then_branch() {
    assert_clean_parse(r#"my $x = $cond ? _croak join ',', @names : 1;"#);
}

#[test]
fn croak_sprintf_sigil_still_clean() {
    assert_clean_parse(r#"my $x = $cond ? _croak sprintf $module : 1;"#);
}

#[test]
fn sprintf_qq_as_ternary_then_branch_itself() {
    assert_clean_parse(r#"my $x = $cond ? sprintf qq{"%s"}, $module : 1;"#);
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
fn test_needs_find_missing_colonful_match_then_croak_sprintf() {
    // Exact remaining first-error shape from Test/Needs.pm `_find_missing`.
    assert_clean_parse(
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
