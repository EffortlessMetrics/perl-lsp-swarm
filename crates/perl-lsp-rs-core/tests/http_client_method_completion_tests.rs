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

    let put_labels =
        labels(&completions_at_end("use LWP::UserAgent;\nmy $ua = LWP::UserAgent->new;\n$ua->put"));
    assert!(has_label(&put_labels, "put"));
    let delete_labels = labels(&completions_at_end(
        "use LWP::UserAgent;\nmy $ua = LWP::UserAgent->new;\n$ua->delete",
    ));
    assert!(has_label(&delete_labels, "delete"));
}

#[test]
fn constructor_inference_is_import_receiver_and_assignment_bounded() {
    let mut sources: Vec<String> = [
        "my $http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n$http = Other::Client->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http_client = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy ($http, $other) = (HTTP::Tiny->new, 1);\n$http->po",
        "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new()->get($url);\n$http->po",
        "use HTTP::Tiny;\nmy $http = Other::Client->new;\nsub reset { $http = HTTP::Tiny->new; }\n$http->po",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    for operator in [
        ".=", "x=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", "&=", "|=", "^=", "&&=",
        "||=", "//=",
    ] {
        sources.push(format!(
            "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n$http {operator} 1;\n$http->po"
        ));
    }

    for source in sources {
        let item_labels = labels(&completions_at_end(&source));
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
        "use HTTP::Tiny;\nmy $pattern = qr{$http = HTTP::Tiny->new()};\n$http->po",
        "use HTTP::Tiny;\nmy $text = <<'END';\n$http = HTTP::Tiny->new;\nEND\n$http->po",
        "use HTTP::Tiny;\n=pod\n$http = HTTP::Tiny->new;\n=cut\n$http->po",
        "use HTTP::Tiny;\n=encoding utf8\n$http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\n=head5 Deep\n$http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\n=head6 Deeper\n$http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\n=pod\n=end comment\n$http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http;\n__DATA__\n$http = HTTP::Tiny->new;\n$http->po",
        "use HTTP::Tiny;\nmy $http;\n__END__\n$http = HTTP::Tiny->new;\n$http->po",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "post"),
            "non-code constructor text should stay quiet in {source:?}"
        );
    }
}

#[test]
fn begin_end_region_without_cut_stays_pod() {
    let begin_source = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n=begin comment\ndocumentation\n=end comment\n\n$http->po";
    let begin_labels = labels(&completions_at_end(begin_source));
    assert!(
        !has_label(&begin_labels, "post"),
        "a closed =begin/=end region must not resume code without =cut: {begin_labels:?}"
    );
}

#[test]
fn for_paragraph_blank_line_stays_pod_until_cut() {
    let source = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n=for comment\ntext\n\n$http->po";
    let item_labels = labels(&completions_at_end(source));
    assert!(
        !has_label(&item_labels, "post"),
        "a =for paragraph's blank line must not resume code without =cut: {item_labels:?}"
    );

    let cut_source =
        "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n=for comment\ntext\n\n=cut\n$http->po";
    let cut_labels = labels(&completions_at_end(cut_source));
    assert!(
        has_label(&cut_labels, "post"),
        "a real =cut must resume code after a =for paragraph: {cut_labels:?}"
    );
}

#[test]
fn runtime_import_forms_stay_position_bounded_and_decoy_immune() {
    let sources = [
        // Runtime-only loading (`require` + `->import`) is not `use` evidence:
        // the method-completion path never consulted the runtime import
        // authority, so this stays quiet exactly as before the textual scan
        // (parser-backed flow is owned by #13244).
        "require HTTP::Tiny;\nHTTP::Tiny->import(qw());\nmy $http = HTTP::Tiny->new;\n$http->po",
        "require LWP::UserAgent;\nLWP::UserAgent->import(qw());\nmy $ua = LWP::UserAgent->new;\n$ua->re",
        // Near-miss module names must not arm the HTTP catalogs.
        "use HTTP::Tinyish;\nmy $http = HTTP::Tiny->new;\n$http->po",
        "use LWP::UserAgent::Mock;\nmy $ua = LWP::UserAgent->new;\n$ua->re",
        // A `use` statement after the completion position is not import
        // evidence: the scan is position-bounded.
        "my $http = HTTP::Tiny->new;\n$http->po\nuse HTTP::Tiny;",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        let forbidden = if source.contains("LWP::UserAgent") { "request" } else { "post" };
        assert!(
            !has_label(&item_labels, forbidden),
            "runtime/decoy/unplaced import evidence must not arm the catalog in {source:?}"
        );
    }
}

#[test]
fn constructor_inference_respects_lexical_shadowing_and_scope_exit() {
    let outer_shadowed = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n{\n    my $http = Other::Client->new;\n    $http->po\n}";
    let outer_shadowed_labels = labels(&completions_at_end(outer_shadowed));
    assert!(
        !has_label(&outer_shadowed_labels, "post"),
        "inner binding must not inherit the outer constructor"
    );

    let inner_does_not_leak = "use HTTP::Tiny;\n{\n    my $http = HTTP::Tiny->new;\n}\n$http->po";
    let inner_does_not_leak_labels = labels(&completions_at_end(inner_does_not_leak));
    assert!(
        !has_label(&inner_does_not_leak_labels, "post"),
        "a block-local constructor must not type an out-of-scope receiver"
    );
}

#[test]
fn undef_write_clears_constructor_evidence() {
    let cleared = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\nundef $http;\n$http->po";
    let cleared_labels = labels(&completions_at_end(cleared));
    assert!(
        !has_label(&cleared_labels, "post"),
        "`undef $http` must clear inferred constructor evidence: {cleared_labels:?}"
    );

    let paren_cleared = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\nundef($http);\n$http->po";
    let paren_cleared_labels = labels(&completions_at_end(paren_cleared));
    assert!(
        !has_label(&paren_cleared_labels, "post"),
        "`undef($http)` must also clear inferred constructor evidence: {paren_cleared_labels:?}"
    );

    let reassigned = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\nundef $http;\n$http = HTTP::Tiny->new;\n$http->po";
    let reassigned_labels = labels(&completions_at_end(reassigned));
    assert!(
        has_label(&reassigned_labels, "post"),
        "a constructor assignment after `undef` re-establishes the receiver type"
    );
}

#[test]
fn constructor_argument_semicolons_do_not_truncate_evidence() {
    let source = "use LWP::UserAgent;\nmy $ua = LWP::UserAgent->new(agent => 'foo;bar');\n$ua->re";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        has_label(&item_labels, "request"),
        "a quoted semicolon inside constructor arguments must not truncate the assignment: {item_labels:?}"
    );
    assert!(
        has_label(&item_labels, "requests_redirectable"),
        "constructor evidence must survive quoted semicolons: {item_labels:?}"
    );
}

#[test]
fn regex_literal_after_comment_is_still_detected() {
    let source = "use HTTP::Tiny;\nmy $http; # prior comment\nmy $pattern = qr{$http = HTTP::Tiny->new()};\n$http->po";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        !has_label(&item_labels, "post"),
        "constructor text inside a regex must stay quiet even after an earlier line comment: {item_labels:?}"
    );
}

#[test]
fn substitution_replacement_text_is_not_constructor_evidence() {
    let source = "use HTTP::Tiny;\nmy $http;\nmy $x = s;foo;$http = HTTP::Tiny->new;;\n$http->po";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        !has_label(&item_labels, "post"),
        "s/// replacement text must not become constructor assignment evidence: {item_labels:?}"
    );
}

#[test]
fn redeclared_our_bindings_share_constructor_evidence() {
    let source = "use HTTP::Tiny;\nour $http = HTTP::Tiny->new;\n{\n    our $http;\n    $http = Other::Client->new;\n}\n$http->po";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        !has_label(&item_labels, "post"),
        "a redeclared `our` binding aliases the same package variable, so the child write must replace stale constructor evidence: {item_labels:?}"
    );
}

#[test]
fn other_package_our_redeclaration_does_not_clear_shared_evidence() {
    let source = "use HTTP::Tiny;\nour $http = HTTP::Tiny->new;\n{\n    package Other;\n    our $http;\n    $http = Other::Client->new;\n}\n$http->po";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        has_label(&item_labels, "post"),
        "a different package's `our $http` is a distinct variable and must not clear the outer HTTP evidence: {item_labels:?}"
    );
}

#[test]
fn undef_named_method_or_sub_call_does_not_clear_evidence() {
    let method_call =
        "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n$cleaner->undef($http);\n$http->po";
    let method_labels = labels(&completions_at_end(method_call));
    assert!(
        has_label(&method_labels, "post"),
        "`$cleaner->undef($http)` does not change $http; evidence must survive: {method_labels:?}"
    );

    let sub_call = "use HTTP::Tiny;\nmy $http = HTTP::Tiny->new;\n&undef($http);\n$http->po";
    let sub_labels = labels(&completions_at_end(sub_call));
    assert!(
        has_label(&sub_labels, "post"),
        "`&undef($http)` is a subroutine call, not the builtin; evidence must survive: {sub_labels:?}"
    );
}
