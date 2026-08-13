mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn readonly_list_declaration_accepts_per_variable_attributes() {
    let source = r#"
use Readonly;
Readonly my ($tagged_ro :shared, $plain_ro) => (1, 2);
"#;
    assert_clean_parse(source);
}

#[test]
fn const_fast_list_declaration_accepts_per_variable_attributes() {
    let source = r#"
use Const::Fast;
const my ($tagged :shared, $plain) => (1, 2);
"#;
    assert_clean_parse(source);
}

#[test]
fn readonly_list_declaration_preserves_attribute_node() {
    let source = "Readonly my ($tagged_ro :shared, $plain_ro) => (1, 2);\n";
    let ast = parse(source);
    let sexp = ast.to_sexp();
    assert!(
        sexp.contains("attributes shared"),
        "expected VariableWithAttributes for :shared, got: {sexp}"
    );
    assert!(
        !sexp.contains("ERROR"),
        "declaration-arg attribute list must parse cleanly, got: {sexp}"
    );
}
