//! Regression tests for issue #1518: arrow-deref hash keys must not trigger
//! "Bareword not allowed under 'use strict'" diagnostics.
//!
//! In Perl, `$ref->{key}` auto-quotes the bareword `key` — it is semantically
//! identical to `$ref->{'key'}`.  The semantic analyzer was incorrectly emitting
//! a sev-1 `UnquotedBareword` diagnostic for the key portion, treating it as a
//! strict violation.
//!
//! These tests guard two properties:
//!   1. **No false positive** — arrow-deref hash keys do NOT emit `UnquotedBareword`.
//!   2. **No over-suppression** — genuine strict bareword violations still fire.

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &pragma_map)
}

fn bareword_issues(issues: &[ScopeIssue]) -> Vec<&ScopeIssue> {
    issues.iter().filter(|i| matches!(i.kind, IssueKind::UnquotedBareword)).collect()
}

// ---------------------------------------------------------------------------
// Section A: False-positive guard (these must emit ZERO UnquotedBareword)
// ---------------------------------------------------------------------------

/// Basic `$self->{name}` — the canonical OO accessor pattern.
#[test]
fn test_arrow_deref_simple_key_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
package My::App;
use strict;
use warnings;
sub greeting {
    my $self = shift;
    return $self->{name};
}
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$self->{{name}} should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// `$ref->{key}` — plain reference, not $self.
#[test]
fn test_arrow_deref_ref_key_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
use strict;
use warnings;
my $ref = { key => 'value' };
my $v = $ref->{key};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$ref->{{key}} should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// Chained arrow-deref: `$a->{b}{c}`.
#[test]
fn test_arrow_deref_chained_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
use strict;
use warnings;
my $a = { b => { c => 42 } };
my $v = $a->{b}{c};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "chained $a->{{b}}{{c}} should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// Method-chained arrow-deref: `$obj->method()->{key}`.
#[test]
fn test_arrow_deref_after_method_call_no_bareword_diagnostic()
-> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
package My::App;
use strict;
use warnings;
sub run {
    my $self = shift;
    my $data = $self->get_data()->{result};
    return $data;
}
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$obj->method()->{{key}} should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// Multiple hash keys in the same expression scope.
#[test]
fn test_arrow_deref_multiple_keys_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>>
{
    let code = r#"
use strict;
use warnings;
my $cfg = { host => 'localhost', port => 8080 };
my $host = $cfg->{host};
my $port = $cfg->{port};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "multiple arrow-deref keys should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Section B: quoted / expression keys — unaffected (must also be clean)
// ---------------------------------------------------------------------------

/// `$ref->{'k'}` — string-quoted key should never be flagged.
#[test]
fn test_arrow_deref_quoted_key_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
use strict;
use warnings;
my $ref = { k => 1 };
my $v = $ref->{'k'};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$ref->{{'k'}} (quoted) should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// `$ref->{$var}` — variable key should never be flagged.
#[test]
fn test_arrow_deref_variable_key_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>>
{
    let code = r#"
use strict;
use warnings;
my $ref = { k => 1 };
my $key = 'k';
my $v = $ref->{$key};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$ref->{{$key}} (variable key) should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Section C: Genuine strict-bareword violations must STILL fire
// ---------------------------------------------------------------------------

/// `my $x = SomeBareword;` must still emit UnquotedBareword.
#[test]
fn test_standalone_bareword_still_flagged() -> Result<(), Box<dyn std::error::Error>> {
    let code = r#"
use strict;
use warnings;
my $x = SomeBareword;
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.iter().any(|i| i.variable_name == "SomeBareword"),
        "standalone bareword 'SomeBareword' must still emit UnquotedBareword diagnostic; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}

/// Direct hash subscript `$hash{key}` must remain clean (pre-existing behaviour).
#[test]
fn test_direct_hash_subscript_key_no_bareword_diagnostic() -> Result<(), Box<dyn std::error::Error>>
{
    let code = r#"
use strict;
use warnings;
my %hash = (key => 1);
my $v = $hash{key};
"#;
    let issues = scope_issues_strict(code);
    let bw = bareword_issues(&issues);
    assert!(
        bw.is_empty(),
        "$hash{{key}} (direct subscript) should not emit UnquotedBareword; got: {:?}",
        bw.iter().map(|i| &i.variable_name).collect::<Vec<_>>()
    );
    Ok(())
}
