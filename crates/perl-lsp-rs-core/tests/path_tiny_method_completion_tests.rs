//! Path::Tiny method-completion contracts for #13192.

use perl_lsp_rs_core::providers::completion::{CompletionItem, CompletionProvider};
use perl_parser::Parser;

fn completions_at_end(source: &str) -> Vec<CompletionItem> {
    let ast = Parser::new(source).parse_with_recovery().ast;
    CompletionProvider::new_with_index_and_source(&ast, source, None)
        .get_completions(source, source.len())
}

fn completions_at_marker(source: &str) -> Vec<CompletionItem> {
    const MARKER: &str = "<|>";
    assert!(source.contains(MARKER), "completion marker should be present");
    let position = source.find(MARKER).unwrap_or_default();
    let source = source.replacen(MARKER, "", 1);
    let ast = Parser::new(&source).parse_with_recovery().ast;
    CompletionProvider::new_with_index_and_source(&ast, &source, None)
        .get_completions(&source, position)
}

fn labels(items: &[CompletionItem]) -> Vec<String> {
    items.iter().map(|item| item.label.to_string()).collect()
}

fn has_label(labels: &[String], expected: &str) -> bool {
    labels.iter().any(|label| label == expected)
}

#[test]
fn default_path_factory_assignment_enables_instance_catalog() {
    let source = "use Path::Tiny;\nmy $file = path(\"notes.txt\");\n$file->sl";
    let items = completions_at_end(source);
    let item_labels = labels(&items);

    assert!(has_label(&item_labels, "slurp"));
    assert!(has_label(&item_labels, "slurp_raw"));
    assert!(has_label(&item_labels, "slurp_utf8"));
    assert!(
        !has_label(&item_labels, "child"),
        "typed API methods should respect the method prefix"
    );

    let slurp = items.iter().find(|item| item.label.as_ref() == "slurp");
    assert!(slurp.is_some(), "slurp completion should be present");
    if let Some(slurp) = slurp {
        assert_eq!(
            slurp.text_edit_range,
            Some((source.len() - "sl".len(), source.len())),
            "completion should replace only the partial method token"
        );
        assert_eq!(slurp.insert_text.as_deref(), Some("slurp()"));
    }
}

#[test]
fn bare_path_factory_call_enables_instance_catalog() {
    let source = "use Path::Tiny;\nmy $file = path \"notes.txt\";\n$file->ch";
    let item_labels = labels(&completions_at_end(source));

    assert!(has_label(&item_labels, "child"));
    assert!(has_label(&item_labels, "children"));
    assert!(has_label(&item_labels, "chmod"));
    assert!(!has_label(&item_labels, "slurp"));
}

#[test]
fn path_factory_respects_explicit_empty_and_versioned_imports() {
    let explicit = labels(&completions_at_end(
        "use Path::Tiny qw(path);\nmy $file = path(\"notes.txt\");\n$file->sl",
    ));
    assert!(has_label(&explicit, "slurp"));

    let versioned = labels(&completions_at_end(
        "use Path::Tiny 0.150;\nmy $file = path(\"notes.txt\");\n$file->sl",
    ));
    assert!(has_label(&versioned, "slurp"));

    for source in [
        "use Path::Tiny qw();\nmy $file = path(\"notes.txt\");\n$file->sl",
        "use Path::Tiny ();\nmy $file = path(\"notes.txt\");\n$file->sl",
        "use Path::Tiny qw(cwd);\nmy $file = path(\"notes.txt\");\n$file->sl",
    ] {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "slurp"),
            "non-imported path factory should stay quiet in {source:?}"
        );
    }

    let class_api = labels(&completions_at_end(
        "use Path::Tiny ();\nmy $file = Path::Tiny->new(\"notes.txt\");\n$file->sl",
    ));
    assert!(
        has_label(&class_api, "slurp"),
        "empty symbol imports still load the Path::Tiny class API"
    );
}

#[test]
fn path_api_requires_use_before_completion_position() {
    let factory_labels = labels(&completions_at_marker(
        "my $file = path(\"notes.txt\");\n$file->sl<|>\nuse Path::Tiny;",
    ));
    assert!(!has_label(&factory_labels, "slurp"));

    let static_labels = labels(&completions_at_marker("Path::Tiny->te<|>\nuse Path::Tiny;"));
    assert!(!has_label(&static_labels, "tempfile"));
    assert!(!has_label(&static_labels, "tempdir"));
}

#[test]
fn path_tiny_static_catalog_requires_import() {
    let imported = labels(&completions_at_end("use Path::Tiny;\nPath::Tiny->te"));
    assert!(has_label(&imported, "tempfile"));
    assert!(has_label(&imported, "tempdir"));

    let unimported = labels(&completions_at_end("Path::Tiny->te"));
    assert!(!has_label(&unimported, "tempfile"));
    assert!(!has_label(&unimported, "tempdir"));
}

#[test]
fn documented_class_factories_enable_instance_catalog() {
    let factories = [
        "Path::Tiny->new(\"notes.txt\")",
        "Path::Tiny->cwd",
        "Path::Tiny->rootdir",
        "Path::Tiny->tempfile",
        "Path::Tiny->tempdir",
    ];

    for factory in factories {
        let source = format!("use Path::Tiny;\nmy $path = {factory};\n$path->pa");
        let item_labels = labels(&completions_at_end(&source));
        assert!(
            has_label(&item_labels, "parent"),
            "documented class factory should identify Path::Tiny in {source:?}"
        );
        assert!(!has_label(&item_labels, "slurp"));
    }
}

#[test]
fn factory_inference_is_import_receiver_and_latest_assignment_bounded() {
    let sources = [
        "my $file = path(\"notes.txt\");\n$file->sl",
        "use Path::Tiny;\nmy $file = pathway(\"notes.txt\");\n$file->sl",
        "use Path::Tiny;\nmy $file = path(\"notes.txt\");\n$file = Other::Path->new;\n$file->sl",
        "use Path::Tiny;\nmy $filepath = path(\"notes.txt\");\n$file->sl",
        "my $file = Path::Tiny->new(\"notes.txt\");\n$file->sl",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "slurp"),
            "factory evidence should stay bounded in {source:?}"
        );
    }
}

#[test]
fn comment_between_module_and_semicolon_still_reads_default_import() {
    let source = "use Path::Tiny # load defaults\n;\nmy $file = path(\"notes.txt\");\n$file->sl";
    let item_labels = labels(&completions_at_end(source));

    assert!(
        has_label(&item_labels, "slurp"),
        "a trailing comment must not hide a default import: {source:?}"
    );
}

#[test]
fn trailing_dereference_yields_plain_string_and_does_not_arm_catalog() {
    let sources = [
        "use Path::Tiny;\nmy $file = path(\"notes.txt\")->stringify;\n$file->sl",
        "use Path::Tiny;\nmy $file = Path::Tiny->new(\"x\")->canonpath;\n$file->sl",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "slurp"),
            "a call chain continuing past the factory yields a plain string, not a \
             Path::Tiny object, so the instance catalog must stay quiet in {source:?}"
        );
    }
}

#[test]
fn textual_path_factory_mentions_outside_code_do_not_activate_catalog() {
    let sources = [
        "use Path::Tiny;\nmy $text = '$file = path(\"notes.txt\")';\n$file->sl",
        "use Path::Tiny;\n# $file = path(\"notes.txt\");\n$file->sl",
        "use Path::Tiny;\nmy $pattern = qr/$file = path(\"notes.txt\")/;\n$file->sl",
        "use Path::Tiny;\nmy $text = <<'END';\n$file = path(\"notes.txt\");\nEND\n$file->sl",
        "use Path::Tiny;\n=pod\n$file = path(\"notes.txt\");\n=cut\n$file->sl",
    ];

    for source in sources {
        let item_labels = labels(&completions_at_end(source));
        assert!(
            !has_label(&item_labels, "slurp"),
            "non-code factory text should stay quiet in {source:?}"
        );
    }
}
