//! End-to-end proof that keyword completions split by syntactic role
//! (#14844). These tests go through `CompletionProvider::get_completions`,
//! not `is_in_expression_position`: that predicate has already looked
//! correct while user-visible offerings were wrong.

use super::*;
use perl_parser_core::Parser;
use perl_tdd_support::{must, must_some_with};

fn completions_at(source: &str) -> Vec<CompletionItem> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CompletionProvider::new(&ast);
    provider.get_completions(source, source.len())
}

fn keyword_labels(completions: &[CompletionItem]) -> Vec<&str> {
    completions
        .iter()
        .filter(|item| item.detail.as_deref() == Some("keyword"))
        .map(|item| item.label.as_ref())
        .collect()
}

fn has_keyword(completions: &[CompletionItem], label: &str) -> bool {
    completions.iter().any(|item| item.label == label && item.detail.as_deref() == Some("keyword"))
}

fn has_label(completions: &[CompletionItem], label: &str) -> bool {
    completions.iter().any(|item| item.label == label)
}

/// Keyword-detail rows that survive label-merge with builtins at empty prefix.
/// Characterized through `CompletionProvider::get_completions` (#14844). Drift
/// is the silent widen/narrow this oracle exists to catch.
const STATEMENT_KEYWORD_DETAIL_SURVIVORS: usize = 57;
const VALUE_KEYWORD_DETAIL_SURVIVORS: usize = 20;

/// `on_start => ` is a value position: anonymous `sub` is legal, `package` is not.
#[test]
fn fat_comma_value_offers_sub_not_package() {
    let source = "my %dispatch = (\n    on_start => ";
    let completions = completions_at(source);
    let labels = keyword_labels(&completions);
    assert!(
        has_keyword(&completions, "sub"),
        "fat-comma value must offer anonymous `sub`; keyword labels ({}) {labels:?}",
        labels.len()
    );
    assert!(
        !has_label(&completions, "package"),
        "fat-comma value must not offer statement-only `package`; keyword labels ({}) {labels:?}",
        labels.len()
    );
}

/// Comparison, match, and defined-or are value positions: expression-capable
/// keywords stay, statement-only ones do not.
///
/// `sub`/`do`/`my` are the keyword-path discriminators — they are not in the
/// builtin catalog, so a surviving `detail == "keyword"` item cannot come from
/// another source. `eval` overlaps a builtin and may lose keyword detail in
/// merge; it is still user-visible as a label.
#[test]
fn comparison_match_and_defined_or_offer_expression_ok_not_statement_only() {
    let positions = [
        ("comparison", "my $ok = $x == "),
        ("match", "my $ok = $x =~ "),
        ("defined-or", "my $val = $x // "),
    ];
    for (name, source) in positions {
        let completions = completions_at(source);
        let labels = keyword_labels(&completions);
        for expression_ok in ["sub", "do", "my"] {
            assert!(
                has_keyword(&completions, expression_ok),
                "{name} value must offer expression-capable `{expression_ok}` via the keyword path; keyword labels ({}) {labels:?}",
                labels.len()
            );
        }
        assert!(
            has_label(&completions, "eval"),
            "{name} value must still surface `eval` (keyword or builtin); keyword labels ({}) {labels:?}",
            labels.len()
        );
        assert!(
            !has_label(&completions, "package"),
            "{name} value must not offer statement-only `package`; keyword labels ({}) {labels:?}",
            labels.len()
        );
        assert!(
            !has_label(&completions, "if"),
            "{name} value must not offer statement-only `if`; keyword labels ({}) {labels:?}",
            labels.len()
        );
    }
}

/// A position the heuristic still treats as a statement must keep both sets,
/// so the change cannot pass by suppressing keywords everywhere.
#[test]
fn statement_position_offers_both_keyword_sets() {
    // Last non-whitespace is `}`, which is not an expression indicator.
    let source = "sub foo { 1 }\n\n";
    let completions = completions_at(source);
    let labels = keyword_labels(&completions);
    assert!(
        has_keyword(&completions, "sub"),
        "statement position must still offer `sub`; keyword labels ({}) {labels:?}",
        labels.len()
    );
    assert!(
        has_keyword(&completions, "package"),
        "statement position must still offer `package`; keyword labels ({}) {labels:?}",
        labels.len()
    );
    assert!(
        has_keyword(&completions, "if"),
        "statement position must still offer `if`; keyword labels ({}) {labels:?}",
        labels.len()
    );
}

/// Empty-prefix keyword-detail counts are locked so a silent widen back to
/// the full inventory (or a suppress-everywhere change) cannot hide behind
/// `sub`/`package` presence. Label-merge with builtins steals some
/// `detail == "keyword"` rows, so these counts are the surviving keyword-detail
/// set, not `keywords().len()`.
#[test]
fn keyword_counts_match_syntactic_role_lists() {
    let statement = completions_at("sub foo { 1 }\n\n");
    let value = completions_at("my %dispatch = (\n    on_start => ");
    let statement_keywords = keyword_labels(&statement);
    let value_keywords = keyword_labels(&value);
    let expression_ok = keywords::EXPRESSION_OK_KEYWORDS;
    let statement_only = keywords::STATEMENT_ONLY_KEYWORDS;

    assert!(
        has_keyword(&statement, "package") && has_keyword(&statement, "sub"),
        "statement position must keep both sets; keyword labels ({}) {statement_keywords:?}",
        statement_keywords.len()
    );
    assert!(
        has_keyword(&value, "sub") && !has_label(&value, "package"),
        "value position must keep expression_ok `sub` and drop `package`; keyword labels ({}) {value_keywords:?}",
        value_keywords.len()
    );

    for label in &value_keywords {
        assert!(
            expression_ok.binary_search(label).is_ok(),
            "value position widened to non-expression_ok `{label}`; keyword labels ({}) {value_keywords:?}",
            value_keywords.len()
        );
        assert!(
            statement_only.binary_search(label).is_err(),
            "value position offered statement_only `{label}` via the keyword path; keyword labels ({}) {value_keywords:?}",
            value_keywords.len()
        );
    }

    assert_eq!(
        statement_keywords.len(),
        STATEMENT_KEYWORD_DETAIL_SURVIVORS,
        "statement-position keyword-detail count drifted (label-merge survivors); got {} {statement_keywords:?}",
        statement_keywords.len()
    );
    assert_eq!(
        value_keywords.len(),
        VALUE_KEYWORD_DETAIL_SURVIVORS,
        "value-position keyword-detail count drifted; expected expression_ok survivors only, got {} {value_keywords:?}",
        value_keywords.len()
    );
    assert!(
        value_keywords.len() < statement_keywords.len(),
        "value keywords must be a proper subset of statement keywords so widening to all is visible"
    );

    let package = must_some_with(
        statement_only.iter().find(|kw| **kw == "package"),
        "package stays statement_only",
    );
    assert_eq!(*package, "package");
    assert!(expression_ok.contains(&"sub"), "anonymous `sub` stays expression_ok");
}

/// Prefix `s` in a value position must still reach the keyword path for `sub`
/// and must not revive `package` via the unfiltered inventory.
#[test]
fn fat_comma_prefix_s_offers_sub_not_package() {
    let source = "my %dispatch = (\n    on_start => s";
    let completions = completions_at(source);
    assert!(has_keyword(&completions, "sub"), "prefix `s` after fat comma must offer `sub`");
    assert!(
        !has_label(&completions, "package"),
        "prefix `s` after fat comma must not offer `package`"
    );
}

fn keyword_item<'a>(completions: &'a [CompletionItem], label: &str) -> &'a CompletionItem {
    must_some_with(
        completions
            .iter()
            .find(|item| item.label == label && item.detail.as_deref() == Some("keyword")),
        "keyword-detail item",
    )
}

/// Selecting `sub` after `=>` must insert an anonymous subroutine, not `sub NAME`.
#[test]
fn fat_comma_sub_inserts_anonymous_snippet() {
    let completions = completions_at("my %dispatch = (\n    on_start => ");
    assert_eq!(keyword_item(&completions, "sub").insert_text.as_deref(), Some("sub {\n    $0\n}"));
}

#[test]
fn statement_position_sub_inserts_named_snippet() {
    let completions = completions_at("sub foo { 1 }\n\n");
    assert_eq!(
        keyword_item(&completions, "sub").insert_text.as_deref(),
        Some("sub ${1:name} {\n    $0\n}")
    );
}

/// Flush-against-operator value positions must use `expression_ok`, including
/// a prefix that would match statement-only `package` if the full inventory leaked.
#[test]
fn flush_operators_offer_expression_ok_not_statement_only() {
    let empty_positions = [
        ("fat-comma", "my %d = (on_start =>"),
        ("comparison", "my $ok = $x =="),
        ("match", "my $ok = $x =~"),
        ("defined-or", "my $val = $x //"),
    ];
    for (name, source) in empty_positions {
        let completions = completions_at(source);
        assert!(has_keyword(&completions, "sub"), "{name} flush value must offer `sub`");
        assert!(!has_label(&completions, "package"), "{name} flush value must not offer `package`");
    }

    let package_prefix_positions = [
        ("fat-comma", "my %d = (on_start =>p"),
        ("comparison", "my $ok = $x ==p"),
        ("match", "my $ok = $x =~p"),
        ("defined-or", "my $val = $x //p"),
    ];
    for (name, source) in package_prefix_positions {
        let completions = completions_at(source);
        assert!(
            !has_label(&completions, "package"),
            "{name} flush prefix `p` must not offer `package`"
        );
    }
}
