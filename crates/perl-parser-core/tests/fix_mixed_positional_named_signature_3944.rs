//! Issue #3944: subroutine signatures that combine positional and named
//! parameters (Perl 5.44 / PPC0024) lacked a test fixture. Existing signature
//! coverage in `named_parameter_default_operators.rs` exercises the default
//! *operators* (`=`, `//=`, `||=`) and always pairs each named parameter with a
//! default. Two facets of the canonical mixed shape were untested:
//!
//!   1. two or more leading *positional* parameters followed by named ones, and
//!   2. a *bare* named parameter with no default (`:$debug`), which the parser
//!      records as `NamedParameter { default_operator: None, required: true }`.
//!
//! These tests pin the exact structure the parser produces for the issue's
//! canonical example so future changes to signature parsing (or downstream
//! semantic diagnostics for argument binding / ordering) have a baseline.

mod cpan_test_helpers;
use cpan_test_helpers::*;
use perl_parser_core::{Node, NodeKind};

/// A compact, order-preserving descriptor of every signature parameter in the
/// tree. Whole-sequence equality on the returned `Vec` is the strongest
/// discriminating oracle — it fails on a missing, reordered, or
/// misclassified parameter, not merely on a parse error.
fn signature_param_tags(source: &str) -> Vec<String> {
    fn var_name(variable: &Node) -> String {
        match &variable.kind {
            NodeKind::Variable { sigil, name } => format!("{sigil}{name}"),
            _ => "<non-variable>".to_string(),
        }
    }

    fn walk(node: &Node, out: &mut Vec<String>) {
        match &node.kind {
            NodeKind::MandatoryParameter { variable } => {
                out.push(format!("mandatory {}", var_name(variable)));
            }
            NodeKind::OptionalParameter { variable, .. } => {
                out.push(format!("optional {}", var_name(variable)));
            }
            NodeKind::SlurpyParameter { variable } => {
                out.push(format!("slurpy {}", var_name(variable)));
            }
            NodeKind::NamedParameter { variable, default_operator, required, .. } => {
                out.push(format!(
                    "named {} op={:?} required={}",
                    var_name(variable),
                    default_operator.as_deref(),
                    required
                ));
            }
            _ => {}
        }
        for child in node.children() {
            walk(child, out);
        }
    }

    let ast = parse(source);
    let mut out = Vec::new();
    walk(&ast, &mut out);
    out
}

#[test]
fn mixed_positional_and_named_signature_parses_with_expected_shape() {
    // The canonical PPC0024 example from issue #3944: two positional params,
    // one named param with an `=` default, and one bare named param.
    let src = r#"
use feature 'signatures';
sub process($x, $y, :$verbose = 0, :$debug) {
    print "$x $y\n";
}
"#;
    assert_clean_parse(src);
    assert_eq!(
        signature_param_tags(src),
        vec![
            "mandatory $x".to_string(),
            "mandatory $y".to_string(),
            "named $verbose op=Some(\"=\") required=false".to_string(),
            "named $debug op=None required=true".to_string(),
        ],
        "mixed signature: $x/$y positional, :$verbose has an `=` default (optional), \
         :$debug is a bare named parameter (no default, required)",
    );
}

#[test]
fn bare_named_parameter_without_default_is_required() {
    // Isolates facet (2): a named parameter with no default operator must be
    // recorded as required with no `default_operator`, distinct from the
    // defaulted named params covered elsewhere.
    let src = "sub f ($x, :$debug) { }";
    assert_clean_parse(src);
    assert_eq!(
        signature_param_tags(src),
        vec!["mandatory $x".to_string(), "named $debug op=None required=true".to_string(),],
        ":$debug with no default is a required NamedParameter with no default_operator",
    );
}
