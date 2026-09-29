use super::{CompletionItem, CompletionItemKind, CompletionProvider};
use perl_parser_core::Parser;
use perl_test_must::{must, must_some};
use perl_workspace::workspace_index::WorkspaceIndex;
use std::sync::Arc;
use url::Url;

fn completions_at(source: &str, position: usize) -> Vec<CompletionItem> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    CompletionProvider::new(&ast).get_completions(source, position)
}

fn completions_with_workspace(
    source: &str,
    position: usize,
    package_source: &str,
) -> Result<Vec<CompletionItem>, Box<dyn std::error::Error>> {
    let index = Arc::new(WorkspaceIndex::new());
    // Canonical initial-name fixture seeding (#16449 burndown): this seeds
    // on-disk workspace state, so call index_initial_file directly.
    index.index_initial_file(Url::parse("file:///lib/Animal.pm")?, package_source.to_string())?;
    let mut parser = Parser::new(source);
    let ast = parser.parse()?;
    Ok(CompletionProvider::new_with_index(&ast, Some(index)).get_completions(source, position))
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_ref()).collect()
}

fn has_label(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|item| item.label == label)
}

fn has_module(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|item| item.label == label && item.kind == CompletionItemKind::Module)
}

#[test]
fn interpolation_slot_offers_visible_lexical_and_suppresses_workspace_package()
-> Result<(), Box<dyn std::error::Error>> {
    let source = r#"my $name = "hi"; my $text = "Hello $"#;
    let items =
        completions_with_workspace(source, source.len(), "package Animal;\nsub speak {}\n1;\n")?;

    assert!(has_label(&items, "$name"), "missing lexical, got {:?}", labels(&items));
    assert!(
        !has_module(&items, "Animal"),
        "workspace package leaked into interpolation, got {:?}",
        labels(&items)
    );
    assert!(
        !items.iter().any(|item| item.kind == CompletionItemKind::Module),
        "no module/package candidates in an exact interpolation slot, got {:?}",
        labels(&items)
    );
    Ok(())
}

#[test]
fn interpolation_filters_prefix_among_multiple_lexicals() {
    let source = r#"my $name = 1; my $namer = 2; my $other = 3; my $text = "Hello $na"#;
    let items = completions_at(source, source.len());
    assert!(has_label(&items, "$name"));
    assert!(has_label(&items, "$namer"));
    assert!(!has_label(&items, "$other"), "prefix filter leaked $other: {:?}", labels(&items));
}

#[test]
fn interpolation_prefers_inner_shadowed_binding() {
    let source = "my $name = 1;\n{\n    our $name = 2;\n    my $text = \"Hello $na\";\n}\n";
    let position = must_some(source.find("$na")) + 3;
    let items = completions_at(source, position);
    let name_items: Vec<_> = items.iter().filter(|item| item.label == "$name").collect();
    assert_eq!(name_items.len(), 1, "shadowing must emit one $name: {:?}", labels(&items));
    let item = must_some(name_items.into_iter().next());
    assert!(
        item.detail.as_deref().is_some_and(|detail| detail.contains("our")),
        "expected the inner our binding, got {:?}",
        item.detail
    );
}

#[test]
fn braced_interpolation_replaces_the_name_inside_braces() {
    let source = r#"my $name = "hi"; my $text = "Hello ${na"#;
    let items = completions_at(source, source.len());
    let item = must_some(items.iter().find(|item| item.label == "$name"));
    assert_eq!(item.insert_text.as_deref(), Some("name"));
    assert_eq!(item.filter_text.as_deref(), Some("name"));
    let dollar = must_some(source.rfind('$'));
    let name_start = dollar + 2; // `${`
    assert_eq!(item.text_edit_range, Some((name_start, source.len())));
}

#[test]
fn qq_paired_delimiter_admits_interpolation_slot() {
    let source = r#"my $name = "hi"; my $text = qq{Hello $na"#;
    let items = completions_at(source, source.len());
    assert!(has_label(&items, "$name"), "qq{} slot missing $name: {:?}", labels(&items));
}

#[test]
fn qq_non_paired_delimiter_admits_interpolation_slot() {
    let source = r#"my $name = "hi"; my $text = qq!Hello $na"#;
    let items = completions_at(source, source.len());
    assert!(has_label(&items, "$name"), "qq! slot missing $name: {:?}", labels(&items));
}

#[test]
fn interpolating_heredoc_admits_lexical() {
    let source = "my $name = \"hi\";\nmy $text = <<EOF;\nHello $na";
    let items = completions_at(source, source.len());
    assert!(
        has_label(&items, "$name"),
        "interpolating heredoc missing $name: {:?}",
        labels(&items)
    );
}

#[test]
fn literal_heredoc_is_quiet() {
    let source = "my $name = \"hi\";\nmy $text = <<'EOF';\nHello $na";
    let items = completions_at(source, source.len());
    assert!(!has_label(&items, "$name"), "literal heredoc leaked $name: {:?}", labels(&items));
}

#[test]
fn escaped_sigil_is_quiet() {
    let source = r#"my $name = "hi"; my $text = "Hello \$na"#;
    let items = completions_at(source, source.len());
    assert!(!has_label(&items, "$name"), "escaped sigil leaked $name: {:?}", labels(&items));
}

#[test]
fn doubled_backslash_still_interpolates() {
    let source = r#"my $name = "hi"; my $text = "Hello \\$na"#;
    let items = completions_at(source, source.len());
    assert!(has_label(&items, "$name"), "\\\\$name should interpolate, got {:?}", labels(&items));
}

#[test]
fn single_quote_and_q_are_quiet() {
    let single = r#"my $name = "hi"; my $text = 'Hello $na"#;
    assert!(
        !has_label(&completions_at(single, single.len()), "$name"),
        "single-quoted slot leaked $name"
    );

    let q = r#"my $name = "hi"; my $text = q($na"#;
    assert!(!has_label(&completions_at(q, q.len()), "$name"), "q() slot leaked $name");
}

#[test]
fn interpolating_array_slot_offers_array_binding() {
    let source = r#"my @names = qw(a b); my $text = "Hello @na"#;
    let items = completions_at(source, source.len());
    assert!(has_label(&items, "@names"), "interpolating @names missing: {:?}", labels(&items));
}

#[test]
fn braced_array_interpolation_replaces_the_name_inside_braces() {
    let source = r#"my @names = qw(a); my $text = "Hello @{na"#;
    let items = completions_at(source, source.len());
    let item = must_some(items.iter().find(|item| item.label == "@names"));
    assert_eq!(item.insert_text.as_deref(), Some("names"));
    assert_eq!(item.filter_text.as_deref(), Some("names"));
}

#[test]
fn interpolating_string_does_not_admit_hash_sigil() {
    let source = r#"my %hash = (a => 1); my $text = "value %ha"#;
    let items = completions_at(source, source.len());
    assert!(
        !has_label(&items, "%hash"),
        "bare %hash is literal in interpolating strings: {:?}",
        labels(&items)
    );
}

#[test]
fn interpolating_heredoc_does_not_admit_hash_sigil() {
    let source = "my %hash = (a => 1);\nmy $text = <<EOF;\nvalue %ha";
    let items = completions_at(source, source.len());
    assert!(
        !has_label(&items, "%hash"),
        "bare %hash is literal in interpolating heredocs: {:?}",
        labels(&items)
    );
}

#[test]
fn braced_interpolation_admits_identifier_special_variables() {
    let source = r#"my $text = "${_"#;
    let items = completions_at(source, source.len());
    let item = must_some(items.iter().find(|item| item.label == "$_"));
    assert_eq!(item.insert_text.as_deref(), Some("_"));
    assert_eq!(item.filter_text.as_deref(), Some("_"));
}

#[test]
fn comment_and_pod_are_quiet() {
    let comment = "my $name = 1; # $na";
    assert!(!has_label(&completions_at(comment, comment.len()), "$name"));

    let pod = "=pod\n$name\n=cut\nmy $name = 1;\n";
    let pos = must_some(pod.find("$name"));
    assert!(!has_label(&completions_at(pod, pos + 3), "$name"));
}

#[test]
fn second_identical_fragment_uses_exact_source_slot() {
    let source = "my $name = 1;\nmy $a = \"Hello $na\";\nmy $b = \"Hello $na";
    let items = completions_at(source, source.len());
    let item = must_some(items.iter().find(|item| item.label == "$name"));
    let first_slot = must_some(source.find("$na"));
    let second_slot = must_some(source.rfind("$na"));
    assert_ne!(first_slot, second_slot);
    assert_eq!(item.text_edit_range, Some((second_slot, source.len())));
}

#[test]
fn unicode_and_crlf_geometry_keep_the_slot() {
    let unicode = "my $name = 1;\nmy $text = \"Hello 😀 $na";
    assert!(has_label(&completions_at(unicode, unicode.len()), "$name"));

    let crlf = "my $name = 1;\r\nmy $text = \"Hello $na";
    assert!(has_label(&completions_at(crlf, crlf.len()), "$name"));
}

#[test]
fn unclosed_interpolating_string_still_admits_the_slot() {
    let source = "my $name = 1;\nmy $text = \"Hello $na";
    assert!(has_label(&completions_at(source, source.len()), "$name"));
}

#[test]
fn code_state_sigil_completion_is_retained() {
    let source = "my $name = 1;\n$na";
    assert!(
        has_label(&completions_at(source, source.len()), "$name"),
        "ordinary code-state sigil completion must stay owned by the existing path"
    );

    let hash = "my %hash = (a => 1);\n%ha";
    assert!(
        has_label(&completions_at(hash, hash.len()), "%hash"),
        "ordinary code-state hash completion must stay owned by the existing path"
    );
}
