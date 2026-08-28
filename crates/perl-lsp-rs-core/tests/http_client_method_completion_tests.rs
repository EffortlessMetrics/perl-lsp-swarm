//! HTTP client method-completion contracts for #13143.

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
fn http_tiny_static_catalog_requires_import() {
    let imported = labels(&completions_at_end("use HTTP::Tiny;\nHTTP::Tiny->can_"));
    assert!(
        has_label(&imported, "can_ssl"),
        "HTTP::Tiny should expose its documented class method after import"
    );

    let unimported = labels(&completions_at_end("HTTP::Tiny->can_"));
    assert!(
        !has_label(&unimported, "can_ssl"),
        "module-specific class methods must not leak without an import"
    );
}

#[test]
fn http_tiny_constructor_assignment_enables_instance_catalog() {
    let source = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\nmy $status = 200;\n$http->po";
    let items = completions_at_end(source);
    let item_labels = labels(&items);

    assert!(has_label(&item_labels, "post"));
    assert!(has_label(&item_labels, "post_form"));
    assert!(!has_label(&item_labels, "get"), "typed API methods should respect the method prefix");

    let post = items.iter().find(|item| item.label.as_ref() == "post");
    assert!(post.is_some(), "post completion should be present");
    if let Some(post) = post {
        assert_eq!(
            post.text_edit_range,
            Some((source.len() - "po".len(), source.len())),
            "completion should replace only the partial method token"
        );
        assert_eq!(post.insert_text.as_deref(), Some("post()"));
    }
}

#[test]
fn lwp_user_agent_constructor_assignment_enables_instance_catalog() {
    let source = "use LWP::UserAgent;\nmy $ua = LWP::UserAgent -> new(timeout => 10);\n$ua->re";
    let item_labels = labels(&completions_at_end(source));

    assert!(has_label(&item_labels, "request"));
    assert!(has_label(&item_labels, "requests_redirectable"));
    assert!(!has_label(&item_labels, "get"), "typed API methods should respect the method prefix");
}

#[test]
fn constructor_inference_is_import_receiver_and_assignment_bounded() {
    let sources = [
        "my $http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n$http = Other::Client->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http_client = HTTP::Tiny->new;\n$http->po",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "post"),
            "constructor evidence should stay bounded in {source:?}"
        );
    }
}

#[test]
fn textual_constructor_mentions_outside_code_do_not_activate_catalog() {
    let sources = [
        "use HTTP::Tiny;\nmy $text = '$http = HTTP::Tiny->new';\n$http->po",
        "use HTTP::Tiny;\n# $http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy $pattern = qr/$http = HTTP::Tiny->new/;\n$http->po",
        "use HTTP::Tiny;\nmy $text = <<'END';\n$http = HTTP::Tiny->new;\nEND\n$http->po",
        "use HTTP::Tiny;\n=pod\n$http = HTTP::Tiny->new;\n=cut\n$http->po",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "post"),
            "non-code constructor text should stay quiet in {source:?}"
        );
    }
}
