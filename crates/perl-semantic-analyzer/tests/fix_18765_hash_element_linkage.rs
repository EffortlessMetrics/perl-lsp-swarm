#![deny(clippy::map_err_ignore)]
//! Regression pins for the #18765 lint false-positive family.
//!
//! Two families are pinned here against real Perl semantics:
//!
//! 1. **PL103 sigil bridge** (`$args{key}` ↔ `%args`): hash-element access
//!    must link to the hash declaration.  The bridge itself predates this
//!    issue; these rows keep it pinned so a future refactor cannot silently
//!    reintroduce 64 false Errors on a clean 8081-line module.
//! 2. **PL110 aggregate seeding**: bare `my @out;` / `my %h;` are
//!    defined-but-empty aggregates in Perl — reading or pushing onto them is
//!    NOT the runtime "Use of uninitialized value" warning a bare scalar
//!    raises, so the textbook accumulator pattern must not be flagged.
//!
//! Oracle, perl 5.38.2 (accumulator pattern compiles and runs clean):
//! ```text
//! $ perl -c -e 'use strict; use warnings;
//! sub render { my @out; push @out, "a"; push @out, "b"; return join(",", @out); }
//! print render();'
//! -e syntax OK
//! ```

use perl_semantic_analyzer::Parser;
use perl_semantic_analyzer::analysis::scope_analyzer::{IssueKind, ScopeAnalyzer, ScopeIssue};
use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_tdd_support::must;

fn scope_issues_strict(code: &str) -> Vec<ScopeIssue> {
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let pragma_map = PragmaTracker::build(&ast);
    let analyzer = ScopeAnalyzer::new();
    analyzer.analyze(&ast, code, &pragma_map)
}

fn undeclared_names(issues: &[ScopeIssue]) -> Vec<&str> {
    issues
        .iter()
        .filter(|i| i.kind == IssueKind::UndeclaredVariable)
        .map(|i| i.variable_name.as_str())
        .collect()
}

fn uninitialized_names(issues: &[ScopeIssue]) -> Vec<&str> {
    issues
        .iter()
        .filter(|i| i.kind == IssueKind::UninitializedVariable)
        .map(|i| i.variable_name.as_str())
        .collect()
}

// ---------------------------------------------------------------------------
// PL110 — bare aggregate declarations are defined-but-empty (#18765)
// ---------------------------------------------------------------------------

/// `my @out; … push @out, …` — the textbook accumulator must not fire PL110.
#[test]
fn accumulator_array_without_initializer_is_not_uninitialized() {
    let code = r#"
use strict;
use warnings;

sub render {
    my @out;
    foreach my $item (@{$_[0]}) {
        push @out, $item->{name};
    }
    return join(",", @out);
}
"#;
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        !uninit.iter().any(|n| n.contains("out")),
        "bare `my @out;` is defined-but-empty in Perl; accumulator must not fire \
         UninitializedVariable, got: {:?} (all issues: {:?})",
        uninit,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// The Inventory.pm fixture shape from the issue: `my @hits; … push @hits`.
#[test]
fn accumulator_hits_array_issue_fixture_is_not_uninitialized() {
    let code = r#"
use strict;
use warnings;

sub find_low_stock {
    my ($self) = @_;
    my @hits;
    foreach my $item (@{$self->{items}}) {
        push @hits, $item if $item->{qty} < 5;
    }
    return @hits;
}
"#;
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        uninit.is_empty(),
        "accumulator `my @hits; push @hits` must produce no UninitializedVariable, got: \
         {:?} (all issues: {:?})",
        uninit,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// `my %h; $h{a} = 1;` — the element store bridges to `%h`, which is now
/// seeded as initialized; both the store and the `\%h` return stay clean.
#[test]
fn bare_hash_element_store_and_ref_return_are_not_uninitialized() {
    let code = r#"
use strict;
use warnings;

sub build_index {
    my %h;
    $h{a} = 1;
    return \%h;
}
"#;
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        !uninit.iter().any(|n| n.contains('h')),
        "bare `my %h;` is defined-but-empty in Perl; element store and ref return must \
         not fire UninitializedVariable, got: {:?} (all issues: {:?})",
        uninit,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// List-form aggregate declarations (`my (@out, %seen);`) are seeded like
/// the single-variable form; a bare scalar in the same list still fires.
///
/// Oracle, perl 5.38.2: `perl -we 'my (@a, %h, $s); push @a, 1; $h{x}=1;
/// print scalar(@a), scalar(%h);'` is silent; adding `print $s;` warns
/// `Use of uninitialized value $s in print`.
#[test]
fn list_declared_aggregates_are_not_uninitialized_but_scalars_are() {
    let code = "use strict;\nsub f { my (@out, %seen, $s); push @out, 1; $seen{x} = 1; \
                print scalar(@out), scalar(%seen); print $s; }\n";
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        !uninit.iter().any(|n| n.contains("out") || n.contains("seen")),
        "list-declared aggregates must not fire UninitializedVariable, got: {:?}",
        uninit
    );
    assert!(
        uninit.iter().any(|n| n.trim_start_matches('$') == "s"),
        "list-declared bare scalar read must keep firing, got: {:?} (all issues: {:?})",
        uninit,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// Explicit-empty control stays clean (pre-existing behavior).
#[test]
fn array_with_explicit_empty_initializer_is_not_uninitialized() {
    let code = "use strict;\nsub f { my @out = (); push @out, 1; return @out; }\n";
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        !uninit.iter().any(|n| n.contains("out")),
        "`my @out = ();` was already clean and must stay clean, got: {:?}",
        uninit
    );
}

/// Scalar control: a bare `my $x;` read is a REAL Perl runtime warning, so
/// PL110 must keep firing on it — this row is the guard against
/// over-suppression by the aggregate seeding.
///
/// Oracle, perl 5.38.2: `perl -we 'my $s; print $s;'` →
/// `Use of uninitialized value $s in print`.
#[test]
fn scalar_without_initializer_still_reports_uninitialized() {
    let code = "use strict;\nsub f { my $s; print $s; }\n";
    let issues = scope_issues_strict(code);
    let uninit = uninitialized_names(&issues);
    assert!(
        uninit.iter().any(|n| n.contains('s')),
        "bare `my $s;` read is a real Perl uninitialized-value warning and must keep \
         firing, got: {:?} (all issues: {:?})",
        uninit,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// PL103 — hash-element access links to the %hash declaration (pin the bridge)
// ---------------------------------------------------------------------------

/// The issue's dominant shape: `my ($class, %args) = @_;` + `$args{key}`.
#[test]
fn hash_element_access_on_list_assign_destructure_is_declared() {
    let code = r#"
use strict;
use warnings;

sub new {
    my ($class, %args) = @_;
    return bless { name => $args{name}, qty => $args{qty} }, $class;
}
"#;
    let issues = scope_issues_strict(code);
    let undeclared = undeclared_names(&issues);
    assert!(
        undeclared.is_empty(),
        "`my ($class, %args) = @_;` + `$args{{key}}` must not fire UndeclaredVariable \
         (sigil bridge), got: {:?} (all issues: {:?})",
        undeclared,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// Quoted keys, variable keys, nested elements, element stores,
/// `exists`/`delete`, `keys %args`, and hash slices all bridge to `%args`.
#[test]
fn hash_element_shapes_all_bridge_to_declared_hash() {
    let code = r#"
use strict;
use warnings;

sub handle {
    my (%args) = @_;
    my $k = 'a';
    $args{'quoted'} = 1;
    $args{$k} = 2;
    $args{nested}{inner} = 3;
    return unless exists $args{flag};
    delete $args{tmp};
    my @ks = keys %args;
    my @slice = @args{qw(a b)};
    return join(",", @ks, @slice, values %args);
}
"#;
    let issues = scope_issues_strict(code);
    let undeclared = undeclared_names(&issues);
    assert!(
        !undeclared.iter().any(|n| n.contains("args")),
        "all hash-element shapes must bridge to the declared %args, got: {:?} (all \
         issues: {:?})",
        undeclared,
        issues.iter().map(|i| (&i.kind, &i.variable_name)).collect::<Vec<_>>()
    );
}

/// File-scope `my %args` + string interpolation shape.
#[test]
fn interpolated_hash_element_on_file_scope_hash_is_declared() {
    let code = "use strict;\nmy %args = (who => 'world');\nmy $msg = \"hello $args{who}\";\n";
    let issues = scope_issues_strict(code);
    let undeclared = undeclared_names(&issues);
    assert!(
        !undeclared.iter().any(|n| n.contains("args")),
        "interpolated `$args{{who}}` on a file-scope %args must not fire \
         UndeclaredVariable, got: {:?}",
        undeclared
    );
}

/// Signature-based shape: `sub new ($class, %args)`.
#[test]
fn signature_hash_parameter_element_access_is_declared() {
    let code = r#"
use strict;
use warnings;
use feature 'signatures';
no warnings 'experimental::signatures';

sub new ($class, %args) {
    return bless { name => $args{name} }, $class;
}
"#;
    let issues = scope_issues_strict(code);
    let undeclared = undeclared_names(&issues);
    assert!(
        !undeclared.iter().any(|n| n.contains("args")),
        "signature `%args` + `$args{{name}}` must not fire UndeclaredVariable, got: {:?}",
        undeclared
    );
}

/// Genuine-undeclared control: without any declaration, `$missing{key}`
/// must still fire — the bridge must not blanket-silence hash access.
#[test]
fn genuinely_undeclared_hash_access_still_reports() {
    let code = "use strict;\nsub f { return $missing{key}; }\n";
    let issues = scope_issues_strict(code);
    let undeclared = undeclared_names(&issues);
    assert!(
        !undeclared.is_empty(),
        "hash access on a never-declared name is a real strict error and must fire \
         UndeclaredVariable, got no issues"
    );
}
