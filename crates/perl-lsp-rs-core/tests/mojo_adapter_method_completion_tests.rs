//! Static method completion contracts for issue #2977.

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

#[test]
fn mojo_pg_static_catalog_requires_import() {
    let imported = labels(&completions_at_end("use Mojo::Pg;\nMojo::Pg->"));
    assert!(has_label(&imported, "new"), "Mojo::Pg should offer new: {imported:?}");
    for method in ["db", "from_string", "reset"] {
        assert!(
            !has_label(&imported, method),
            "instance method {method} must not be offered on Mojo::Pg->"
        );
    }

    let unimported = labels(&completions_at_end("Mojo::Pg->d"));
    assert!(
        !has_label(&unimported, "db"),
        "Mojo::Pg::db must be gated by an explicit import: {unimported:?}"
    );
}

#[test]
fn mojo_mysql_static_catalog_and_prefix_filter() {
    let imported = labels(&completions_at_end("use Mojo::mysql;\nMojo::mysql->"));
    for method in ["new", "strict_mode"] {
        assert!(has_label(&imported, method), "Mojo::mysql should offer {method}: {imported:?}");
    }
    for method in ["db", "from_string", "close_idle_connections"] {
        assert!(
            !has_label(&imported, method),
            "instance method {method} must not be offered on Mojo::mysql->"
        );
    }
    for method in ["isa", "can", "DOES", "VERSION"] {
        assert!(has_label(&imported, method), "generic method {method} should remain available");
    }

    let filtered = labels(&completions_at_end("use Mojo::mysql;\nMojo::mysql->st"));
    assert!(has_label(&filtered, "strict_mode"), "strict_mode should match st: {filtered:?}");
    assert!(!has_label(&filtered, "db"), "db should not match st: {filtered:?}");

    let unimported = labels(&completions_at_end("Mojo::mysql->st"));
    assert!(
        !has_label(&unimported, "strict_mode"),
        "Mojo::mysql::strict_mode must be gated by an explicit import: {unimported:?}"
    );

    let unknown = labels(&completions_at_end("Mojo::Unknown->st"));
    assert!(
        !has_label(&unknown, "strict_mode"),
        "unknown adapters must not inherit Mojo::mysql methods: {unknown:?}"
    );
}
