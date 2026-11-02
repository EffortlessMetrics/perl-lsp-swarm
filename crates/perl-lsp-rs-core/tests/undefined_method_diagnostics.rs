//! Integration tests for PL306 — undefined method calls on a known in-file class.
//!
//! Issue #18771: `$inv->find_skku(...)` (typo of `find_sku`) currently produces
//! an empty diagnostic set even though hover/definition already know the method
//! table. These tests pin the conservative in-file warning.

use std::sync::Arc;

use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticsProvider};
use perl_parser::Parser;
use perl_tdd_support::must_some;

fn diagnostics_for(source: &str) -> Vec<Diagnostic> {
    let output = Parser::new(source).parse_with_recovery();
    let ast = Arc::new(output.ast);
    DiagnosticsProvider::new().get_diagnostics(&ast, &output.diagnostics, source, None)
}

fn pl306(source: &str) -> Vec<Diagnostic> {
    diagnostics_for(source).into_iter().filter(|d| d.code.as_deref() == Some("PL306")).collect()
}

fn inventory_with_call(call: &str) -> String {
    format!(
        r#"package Inventory;
sub new {{ bless {{}}, shift }}
sub find_sku {{ my ($self, $sku) = @_; return $sku; }}
sub demo {{
    my $inv = Inventory->new;
    {call}
}}
"#
    )
}

#[test]
fn typo_method_on_constructor_receiver_emits_pl306() {
    let source = inventory_with_call("$inv->find_skku('GIZMO-B');");
    let diags = pl306(&source);
    assert_eq!(diags.len(), 1, "typo of a known method must emit PL306: {diags:?}");
    assert!(
        diags[0].message.contains("find_skku"),
        "PL306 must name the missing method: {}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("Inventory"),
        "PL306 must name the inferred class: {}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("runtime") || diags[0].message.contains("may be resolved"),
        "PL306 must acknowledge dynamic dispatch: {}",
        diags[0].message
    );
    let needle = "find_skku";
    let call_at = must_some(source.find(needle));
    let (start, end) = diags[0].range;
    assert!(
        start <= call_at && end >= call_at + needle.len(),
        "PL306 range {start}..{end} should cover `{needle}`"
    );
}

#[test]
fn known_method_on_constructor_receiver_is_silent() {
    let source = inventory_with_call("$inv->find_sku('GIZMO-B');");
    assert!(pl306(&source).is_empty(), "a method present in the class table must not warn");
}

#[test]
fn static_package_unknown_method_emits_pl306() {
    let source = r#"package Inventory;
sub new { bless {}, shift }
sub find_sku { }
Inventory->find_skku('x');
"#;
    let diags = pl306(source);
    assert_eq!(diags.len(), 1, "Class->unknown must emit PL306: {diags:?}");
}

#[test]
fn static_package_known_method_is_silent() {
    let source = r#"package Inventory;
sub find_sku { }
Inventory->find_sku('x');
"#;
    assert!(pl306(source).is_empty(), "Class->known must not warn");
}

#[test]
fn self_receiver_unknown_method_emits_pl306() {
    let source = r#"package Inventory;
sub find_sku { }
sub demo {
    my ($self) = @_;
    $self->find_skku();
}
"#;
    let diags = pl306(source);
    assert_eq!(diags.len(), 1, "$self->unknown in the defining package must emit PL306: {diags:?}");
}

#[test]
fn universal_methods_are_silent() {
    let source = r#"package Inventory;
sub new { bless {}, shift }
sub demo {
    my $inv = Inventory->new;
    $inv->can('find_sku');
    $inv->isa('Inventory');
    $inv->DOES('Inventory');
    $inv->VERSION();
}
"#;
    assert!(pl306(source).is_empty(), "UNIVERSAL methods must not warn: {:?}", pl306(source));
}

#[test]
fn autoload_suppresses_missing_method() {
    let source = r#"package Inventory;
sub AUTOLOAD { }
sub demo {
    my ($self) = @_;
    $self->find_skku();
}
"#;
    assert!(
        pl306(source).is_empty(),
        "AUTOLOAD means the call may resolve at runtime: {:?}",
        pl306(source)
    );
}

#[test]
fn unknown_receiver_is_silent() {
    let source = r#"package Inventory;
sub find_sku { }
sub demo {
    my $inv = shift;
    $inv->find_skku('x');
}
"#;
    assert!(pl306(source).is_empty(), "an untyped receiver must not warn: {:?}", pl306(source));
}

#[test]
fn dynamic_method_name_is_silent() {
    let source = r#"package Inventory;
sub find_sku { }
sub demo {
    my ($self, $name) = @_;
    $self->$name();
}
"#;
    assert!(pl306(source).is_empty(), "dynamic $obj->$name must not warn: {:?}", pl306(source));
}

#[test]
fn external_parent_suppresses_missing_method() {
    let source = r#"package Child;
use parent 'External::Parent';
sub demo {
    my ($self) = @_;
    $self->not_defined_here();
}
"#;
    assert!(
        pl306(source).is_empty(),
        "inheritance from an unseen parent is unknown, not missing: {:?}",
        pl306(source)
    );
}

#[test]
fn in_file_parent_method_is_silent() {
    let source = r#"package Parent;
sub inherited { }

package Child;
use parent 'Parent';
sub demo {
    my ($self) = @_;
    $self->inherited();
}
"#;
    assert!(
        pl306(source).is_empty(),
        "in-file inherited method must not warn: {:?}",
        pl306(source)
    );
}

#[test]
fn in_file_parent_unknown_method_emits_pl306() {
    let source = r#"package Parent;
sub inherited { }

package Child;
use parent 'Parent';
sub demo {
    my ($self) = @_;
    $self->missing_everywhere();
}
"#;
    let diags = pl306(source);
    assert_eq!(
        diags.len(),
        1,
        "a method absent from the in-file inheritance chain must emit PL306: {diags:?}"
    );
}

#[test]
fn moo_generated_accessor_is_silent() {
    let source = r#"package Item;
use Moo;
has 'sku' => (is => 'ro');
sub demo {
    my ($self) = @_;
    $self->sku();
}
"#;
    assert!(
        pl306(source).is_empty(),
        "Moo has-generated accessor must not warn: {:?}",
        pl306(source)
    );
}

#[test]
fn moo_unknown_method_emits_pl306() {
    let source = r#"package Item;
use Moo;
has 'sku' => (is => 'ro');
sub demo {
    my ($self) = @_;
    $self->skku();
}
"#;
    let diags = pl306(source);
    assert_eq!(
        diags.len(),
        1,
        "unknown method on a Moo class with no parents must warn: {diags:?}"
    );
}

#[test]
fn constructor_new_on_moo_class_is_silent() {
    let source = r#"package Item;
use Moo;
has 'sku' => (is => 'ro');
my $item = Item->new(sku => 'A');
"#;
    assert!(pl306(source).is_empty(), "Moo provides new: {:?}", pl306(source));
}

#[test]
fn warning_not_error() {
    let source = inventory_with_call("$inv->find_skku('x');");
    let diag = must_some(pl306(&source).into_iter().next());
    assert_eq!(
        diag.severity,
        perl_diagnostics::codes::DiagnosticSeverity::Warning,
        "issue #18771 requires a warning, not an error"
    );
}

#[test]
fn reassignment_clears_stale_constructor_type() {
    let source = r#"package Old;
sub new { bless {}, shift }
package Other;
sub new_from_cache { bless {}, shift }
sub send { }
package Demo;
sub demo {
    my $obj = Old->new;
    $obj = Other->new_from_cache;
    $obj->send;
}
"#;
    assert!(
        pl306(source).is_empty(),
        "reassignment must drop the old inferred class: {:?}",
        pl306(source)
    );
}

#[test]
fn inner_block_lexical_does_not_escape_scope() {
    let source = r#"package Outer;
sub new { bless {}, shift }
sub outer_only { }
package Inner;
sub new { bless {}, shift }
package Demo;
sub demo {
    my $obj = Outer->new;
    { my $obj = Inner->new; }
    $obj->outer_only;
}
"#;
    assert!(
        pl306(source).is_empty(),
        "inner block shadow must not leak inferred type: {:?}",
        pl306(source)
    );
}

#[test]
fn lexical_sub_is_not_a_package_method() {
    let source = r#"package P;
my sub foo { }
P->foo();
"#;
    assert_eq!(
        pl306(source).len(),
        1,
        "my sub must not suppress PL306 on Package->call: {:?}",
        pl306(source)
    );
}

#[test]
fn reopened_segment_accessor_is_silent() {
    let source = r#"package Item;
use Moo;
sub demo {
    my ($self) = @_;
    $self->sku;
}
package Item;
has 'sku' => (is => 'ro');
"#;
    assert!(
        pl306(source).is_empty(),
        "accessor declared in a later package segment must not warn: {:?}",
        pl306(source)
    );
}
