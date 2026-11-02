//! SQL::Abstract method completion contracts for #2979 (Phase 1: methods + guards).

use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_parser::Parser;

fn completions_at_end(source: &str) -> Vec<CompletionItem> {
    let ast = Parser::new(source).parse_with_recovery().ast;
    CompletionProvider::new_with_index_and_source(&ast, source, None)
        .get_completions(source, source.len())
}

fn labels(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.to_string()).collect()
}

fn has_label(labels: &[String], expected: &str) -> bool {
    labels.iter().any(|label| label == expected)
}

const SQL_ABSTRACT_METHODS: &[&str] =
    &["select", "insert", "update", "delete", "where", "generate", "values", "order_by"];

#[test]
fn completes_sql_abstract_methods_for_sql_variable_with_new_assignment() {
    let source = "use SQL::Abstract;\nmy $sql = SQL::Abstract->new;\n$sql->";
    let item_labels = labels(&completions_at_end(source));
    for method in SQL_ABSTRACT_METHODS {
        assert!(
            has_label(&item_labels, method),
            "expected SQL::Abstract method {method} after constructor assignment, got {item_labels:?}"
        );
    }
}

#[test]
fn completes_sql_abstract_methods_for_sqla_and_sql_abs_variables() {
    for (source, var) in [
        ("use SQL::Abstract;\nmy $sqla = SQL::Abstract->new;\n$sqla->", "$sqla"),
        ("use SQL::Abstract;\nmy $sql_abs = SQL::Abstract->new;\n$sql_abs->", "$sql_abs"),
    ] {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            has_label(&item_labels, "select"),
            "expected select completion for {var}, got {item_labels:?}"
        );
    }
}

#[test]
fn completes_sql_abstract_methods_for_named_variables_without_constructor() {
    for source in [
        "use SQL::Abstract;\nmy $sqla;\n$sqla->",
        "use SQL::Abstract;\nmy $sql_abs;\n$sql_abs->",
        "use SQL::Abstract;\nmy $sql;\n$sql->",
    ] {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            has_label(&item_labels, "insert"),
            "expected insert completion without explicit new in {source:?}, got {item_labels:?}"
        );
    }
}

#[test]
fn guard_pattern_prevents_false_positives_without_use_sql_abstract() {
    let source = "my $sql = SQL::Abstract->new;\n$sql->";
    let item_labels = labels(&completions_at_end(source));
    for method in ["select", "insert", "delete"] {
        assert!(
            !has_label(&item_labels, method),
            "SQL::Abstract method {method} must not appear without use SQL::Abstract"
        );
    }
}

#[test]
fn completes_sql_abstract_static_methods_after_import() {
    let source = "use SQL::Abstract;\nSQL::Abstract->sel";
    let item_labels = labels(&completions_at_end(source));
    assert!(has_label(&item_labels, "select"));
}

#[test]
fn completes_sql_abstract_methods_with_subclass_import() {
    let source = "use SQL::Abstract::Pg;\nmy $sql = SQL::Abstract->new;\n$sql->sel";
    let item_labels = labels(&completions_at_end(source));
    assert!(has_label(&item_labels, "select"));
}

#[test]
fn dollar_s_variable_should_not_trigger_sql_abstract_completion() {
    let source = "use SQL::Abstract;\nmy $s = SQL::Abstract->new;\n$s->";
    let item_labels = labels(&completions_at_end(source));
    for method in SQL_ABSTRACT_METHODS {
        assert!(
            !has_label(&item_labels, method),
            "$s must not trigger SQL::Abstract completion for {method}"
        );
    }
}

#[test]
fn completes_sql_abstract_static_new_after_import() {
    let source = "use SQL::Abstract;\nSQL::Abstract->n";
    let item_labels = labels(&completions_at_end(source));
    assert!(has_label(&item_labels, "new"));
}

#[test]
fn sql_abstract_name_inference_stays_bounded_after_reassignment() {
    let source = "use SQL::Abstract;\nuse HTTP::Tiny;\nmy $sql = SQL::Abstract->new;\n$sql = HTTP::Tiny->new;\n$sql->";
    let item_labels = labels(&completions_at_end(source));
    assert!(
        !has_label(&item_labels, "select"),
        "reassigned $sql must not keep SQL::Abstract completions"
    );
    assert!(has_label(&item_labels, "get") || has_label(&item_labels, "post"));
}

#[test]
fn sql_abstract_completion_items_have_documentation() -> Result<(), Box<dyn std::error::Error>> {
    let source = "use SQL::Abstract;\nmy $sql = SQL::Abstract->new;\n$sql->sel";
    let items = completions_at_end(source);
    let select = items
        .iter()
        .find(|item| item.label.as_ref() == "select")
        .ok_or("missing select completion item")?;
    let doc =
        select.documentation.as_deref().ok_or("select completion should carry documentation")?;
    assert!(
        doc.contains("SELECT") || doc.contains("select"),
        "select documentation should describe SELECT, got {doc:?}"
    );
    Ok(())
}
