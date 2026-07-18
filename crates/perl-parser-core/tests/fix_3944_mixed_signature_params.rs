//! #3944: mixed positional + named signature parameters (Perl 5.44 / PPC0024).
//!
//! `named_parameter_ast.rs` proves that named params surface and carry their
//! default; `named_parameters_coexist_with_positional_and_slurpy` proves the
//! *count* of named params in a mixed signature. Neither asserts that the
//! issue's exact shape — leading positionals followed by a *defaulted* and a
//! *required* named param, e.g. `sub f($x, $y, :$verbose = 0, :$debug)` —
//! parses cleanly and that the required/optional distinction is preserved in
//! that mixed context. This test closes that gap.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind};

/// Walk the tree collecting `(external_name, required, has_default)` for every
/// `NamedParameter`.
fn collect_named(node: &Node, out: &mut Vec<(String, bool, bool)>) {
    if let NodeKind::NamedParameter { external_name, required, default_value, .. } = &node.kind {
        out.push((external_name.clone(), *required, default_value.is_some()));
    }
    for child in node.children() {
        collect_named(child, out);
    }
}

#[test]
fn mixed_positional_and_named_signature_parses_cleanly() {
    // assert_clean_parse walks the AST for Error/Missing* nodes (shared helper).
    assert_clean_parse(
        r#"use feature 'signatures';
sub process($x, $y, :$verbose = 0, :$debug) {
    print "$x $y\n";
}"#,
    );
}

#[test]
fn mixed_signature_preserves_named_required_and_default_flags()
-> Result<(), Box<dyn std::error::Error>> {
    // `:$verbose = 0` is optional (has a default); `:$debug` is required (none).
    let ast = parse("sub process($x, $y, :$verbose = 0, :$debug) { }");
    let mut named = Vec::new();
    collect_named(&ast, &mut named);

    assert_eq!(named.len(), 2, "both named params must surface, got {named:?}");

    let verbose = named.iter().find(|n| n.0 == "verbose").ok_or("named param :$verbose")?;
    assert!(!verbose.1, ":$verbose has a default, so it is optional (required=false)");
    assert!(verbose.2, ":$verbose default value must be preserved");

    let debug = named.iter().find(|n| n.0 == "debug").ok_or("named param :$debug")?;
    assert!(debug.1, ":$debug has no default, so it is required (required=true)");
    assert!(!debug.2, ":$debug has no default value");
    Ok(())
}
