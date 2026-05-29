//! Coverage for production-shaped Perl constructs carried by the C tree-sitter grammar.
//!
//! These tests intentionally exercise broad grammar areas instead of only the
//! high-level binding APIs: data sections, modern control flow, regex operators,
//! symbol-table constructs, Unicode identifiers, and editor matchup queries.

use std::error::Error;

use tree_sitter::{Node, Query, QueryCursor, StreamingIterator};
use tree_sitter_perl_c::{language, parse_perl_bytes, parse_perl_code};

fn assert_valid_tree(
    source_name: &str,
    source: &str,
    required_kinds: &[&str],
) -> Result<(), Box<dyn Error>> {
    let tree = parse_perl_code(source)?;
    let root = tree.root_node();

    assert_eq!(root.kind(), "source_file", "{source_name} should parse as a source_file");
    assert!(
        !root.has_error(),
        "{source_name} should parse without ERROR nodes: {}",
        root.to_sexp()
    );

    let sexp = root.to_sexp();
    for required_kind in required_kinds {
        assert!(
            sexp.contains(required_kind),
            "{source_name} parse tree should contain `{required_kind}`; tree was: {sexp}"
        );
    }

    Ok(())
}

fn first_descendant_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_descendant_kind(child, kind) {
            return Some(found);
        }
    }

    None
}

#[test]
fn corpus_constructs_cover_data_sections_and_eof_markers() -> Result<(), Box<dyn Error>> {
    let source = b"1 + 2;\n__DATA__\n$this = not `code`\n";

    let tree = parse_perl_bytes(source)?;
    let root = tree.root_node();

    assert_eq!(root.kind(), "source_file");
    assert!(!root.has_error(), "data-section input should parse cleanly: {}", root.to_sexp());

    let sexp = root.to_sexp();
    assert!(sexp.contains("eof_marker"), "expected __DATA__ marker in tree: {sexp}");
    assert!(sexp.contains("data_section"), "expected post-__DATA__ payload in tree: {sexp}");
    Ok(())
}

#[test]
fn corpus_constructs_cover_modern_try_catch_finally() -> Result<(), Box<dyn Error>> {
    let source = r#"try {
    die "boom";
} catch ($error) {
    warn $error;
} finally {
    say "done";
}
"#;

    assert_valid_tree(
        "try/catch/finally",
        source,
        &["try_statement", "catch", "finally", "scalar", "interpolated_string_literal"],
    )
}

#[test]
fn corpus_constructs_cover_regex_substitution_transliteration_and_named_captures()
-> Result<(), Box<dyn Error>> {
    let source = r#"my $text = "abc123";
$text =~ s/(\w+)(\d+)/$1 . ($2 + 1)/eg;
$text =~ tr/a-z/A-Z/;
$text =~ m{(?<word>\w+)\s+\g{word}};
"#;

    assert_valid_tree(
        "regex operators",
        source,
        &[
            "substitution_regexp",
            "replacement",
            "transliteration_expression",
            "match_regexp",
            "regexp_content",
        ],
    )
}

#[test]
fn corpus_constructs_cover_symbol_table_and_tie_forms() -> Result<(), Box<dyn Error>> {
    let source = r#"our $scalar = 1;
*alias = \$scalar;
local *STDOUT;
tie my %hash, 'Tie::StdHash';
"#;

    assert_valid_tree(
        "symbol table and tie forms",
        source,
        &[
            "glob",
            "refgen_expression",
            "localization_expression",
            "ambiguous_function_call_expression",
        ],
    )
}

#[test]
fn corpus_constructs_cover_unicode_identifiers_and_positions() -> Result<(), Box<dyn Error>> {
    let source = "use utf8;\nmy $café = \"☕\";\nsub привет { return $café; }\n";
    let tree = parse_perl_code(source)?;
    let root = tree.root_node();

    assert_eq!(root.kind(), "source_file");
    assert!(!root.has_error(), "unicode source should parse cleanly: {}", root.to_sexp());

    let subroutine = first_descendant_kind(root, "subroutine_declaration_statement")
        .ok_or("expected subroutine declaration in unicode fixture")?;
    assert_eq!(subroutine.start_position().row, 2);

    let sexp = root.to_sexp();
    assert!(sexp.contains("use_statement"), "expected utf8 pragma use statement: {sexp}");
    assert!(
        sexp.contains("subroutine_declaration_statement"),
        "expected unicode subroutine: {sexp}"
    );
    Ok(())
}

#[test]
fn corpus_constructs_cover_matchup_query_scopes() -> Result<(), Box<dyn Error>> {
    let source = r#"sub inspect {
    if ($_[0]) {
        while (my $line = <STDIN>) {
            last if $line =~ /done/;
        }
    } else {
        try {
            return 'fallback';
        } catch ($error) {
            return $error;
        } finally {
            return 'cleanup';
        }
    }
}
"#;

    let tree = parse_perl_code(source)?;
    let root = tree.root_node();
    assert!(!root.has_error(), "matchup fixture should parse cleanly: {}", root.to_sexp());

    let query =
        Query::new(&language(), include_str!("../../../tree-sitter-perl/queries/matchup.scm"))?;
    let mut cursor = QueryCursor::new();
    let mut capture_names = Vec::new();

    let mut captures = cursor.captures(&query, root, source.as_bytes());
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures.get(*capture_index).ok_or("missing matchup capture")?;
        let name = query
            .capture_names()
            .get(capture.index as usize)
            .ok_or("missing matchup capture name")?;
        capture_names.push((*name).to_string());
    }

    for expected in ["scope.fun", "open.fun", "scope.loop", "mid.loop.1", "scope.try", "mid.try.1"]
    {
        assert!(
            capture_names.iter().any(|name| name == expected),
            "expected matchup capture `{expected}` in {capture_names:?}"
        );
    }

    Ok(())
}
