//! Signature parameters in in-body completion (#1641).

use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_parser::Parser;
use perl_test_must::must_some_with;

fn completions_after(source: &str, needle: &str) -> Vec<CompletionItem> {
    let trigger = must_some_with(source.find(needle), "fixture needle must exist") + needle.len();
    let ast = Parser::new(source).parse_with_recovery().ast;
    CompletionProvider::new_with_index_and_source(&ast, source, None)
        .get_completions(source, trigger)
}

fn variable_labels(items: &[CompletionItem]) -> Vec<&str> {
    items
        .iter()
        .filter(|item| item.label.starts_with(['$', '@', '%']))
        .map(|item| item.label.as_ref())
        .collect()
}

fn detail_for_label(items: &[CompletionItem], label: &str) -> Option<String> {
    items
        .iter()
        .find(|item| item.label.as_ref() == label)
        .and_then(|item| item.detail.as_ref().map(|detail| detail.to_string()))
}

#[test]
fn greet_scalar_signature_parameters_offer_signature_parameter_detail() {
    let source = concat!(
        "use feature 'signatures';\n",
        "no warnings 'experimental::signatures';\n",
        "sub greet ($name, $greeting = \"Hello\") {\n",
        "    $\n",
        "}\n"
    );

    let completions = completions_after(source, "    $");
    let labels = variable_labels(&completions);
    assert!(labels.contains(&"$name"), "expected $name in scalar completion inside greet; got {labels:?}");
    assert!(
        labels.contains(&"$greeting"),
        "expected $greeting in scalar completion inside greet; got {labels:?}"
    );
    assert_eq!(detail_for_label(&completions, "$name").as_deref(), Some("Signature Parameter"));
    assert_eq!(detail_for_label(&completions, "$greeting").as_deref(), Some("Signature Parameter"));
}

#[test]
fn greet_array_sigil_does_not_offer_scalar_signature_parameters() {
    let source = concat!(
        "use feature 'signatures';\n",
        "no warnings 'experimental::signatures';\n",
        "sub greet ($name, $greeting = \"Hello\") {\n",
        "    @\n",
        "}\n"
    );

    let completions = completions_after(source, "    @");
    let labels = variable_labels(&completions);
    assert!(
        !labels.iter().any(|label| *label == "$name" || *label == "$greeting"),
        "scalar signature parameters must not appear after @ trigger; got {labels:?}"
    );
}

#[test]
fn process_scalar_sigil_offers_no_array_or_hash_signature_parameters() {
    let source = concat!(
        "use feature 'signatures';\n",
        "no warnings 'experimental::signatures';\n",
        "sub process (@items, %options) {\n",
        "    $\n",
        "}\n"
    );

    let completions = completions_after(source, "    $");
    let labels = variable_labels(&completions);
    assert!(
        !labels.iter().any(|label| *label == "@items" || *label == "%options"),
        "no array/hash signature params after $ trigger; got {labels:?}"
    );
}

#[test]
fn process_array_sigil_offers_items_with_signature_parameter_detail() {
    let source = concat!(
        "use feature 'signatures';\n",
        "no warnings 'experimental::signatures';\n",
        "sub process (@items, %options) {\n",
        "    @\n",
        "}\n"
    );

    let completions = completions_after(source, "    @");
    let labels = variable_labels(&completions);
    assert!(
        labels.contains(&"@items"),
        "expected @items after @ trigger; got {labels:?}"
    );
    assert_eq!(detail_for_label(&completions, "@items").as_deref(), Some("Signature Parameter"));
}

#[test]
fn process_hash_sigil_offers_options_with_signature_parameter_detail() {
    let source = concat!(
        "use feature 'signatures';\n",
        "no warnings 'experimental::signatures';\n",
        "sub process (@items, %options) {\n",
        "    %\n",
        "}\n"
    );

    let completions = completions_after(source, "    %");
    let labels = variable_labels(&completions);
    assert!(
        labels.contains(&"%options"),
        "expected %options after % trigger; got {labels:?}"
    );
    assert_eq!(detail_for_label(&completions, "%options").as_deref(), Some("Signature Parameter"));
}
