#![cfg(feature = "incremental")]
//! Public-contract regressions for exact whitespace incremental parsing.

use perl_parser::edit::Edit;
use perl_parser::incremental_v2::IncrementalParserV2;
use perl_parser::position::Position;
use perl_parser::{
    Parser,
    ast::{Node, NodeKind},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type NodeResult = Result<Node, Box<dyn std::error::Error>>;

fn edit(start: usize, old_end: usize, new_end: usize) -> Edit {
    Edit::new(
        start,
        old_end,
        new_end,
        Position::new(start, 0, start as u32),
        Position::new(old_end, 0, old_end as u32),
        Position::new(new_end, 0, new_end as u32),
    )
}

fn parse_fresh(source: &str) -> NodeResult {
    Ok(Parser::new(source).parse()?)
}

fn assert_not_zero_reparse_basic_path(parser: &IncrementalParserV2) {
    assert!(
        !parser.used_incremental_path()
            || parser.used_advanced_reuse()
            || parser.reparsed_nodes > 0,
        "token-body whitespace must not take the zero-reparse basic fast path"
    );
}

fn assert_leading_whitespace_reuse_matches_fresh(source: &str) -> TestResult {
    let shifted = format!(" {source}");
    let mut parser = IncrementalParserV2::new();
    parser.parse(source)?;
    parser.edit(edit(0, 0, 1));

    let incremental = parser.parse(&shifted)?;
    assert_eq!(incremental, parse_fresh(&shifted)?);
    assert!(parser.used_incremental_path());
    assert!(!parser.used_advanced_reuse());
    assert_eq!(parser.reparsed_nodes, 0);
    Ok(())
}

#[test]
fn adjacent_insertions_match_a_fresh_parse() -> TestResult {
    let source1 = "my $x = 1;";
    let source2 = "my $x   = 1;";
    let mut parser = IncrementalParserV2::new();
    parser.parse(source1)?;

    parser.edit(edit(6, 6, 7));
    parser.edit(edit(7, 7, 8));
    let incremental = parser.parse(source2)?;

    assert_eq!(incremental, parse_fresh(source2)?);
    assert!(parser.used_incremental_path());
    assert!(!parser.used_advanced_reuse());
    assert_eq!(parser.reparsed_nodes, 0);
    Ok(())
}

#[test]
fn utf8_before_the_edit_preserves_byte_geometry() -> TestResult {
    let source1 = "my $s = 'é';my $x = 1;";
    let insertion = source1.find("my $x").ok_or("expected second declaration")?;
    let source2 = "my $s = 'é'; my $x = 1;";
    let mut parser = IncrementalParserV2::new();
    parser.parse(source1)?;

    parser.edit(edit(insertion, insertion, insertion + 1));
    let incremental = parser.parse(source2)?;

    assert_eq!(incremental, parse_fresh(source2)?);
    assert!(parser.used_incremental_path());
    assert!(!parser.used_advanced_reuse());
    assert_eq!(parser.reparsed_nodes, 0);
    Ok(())
}

#[test]
fn crlf_insertion_matches_fresh_parse_geometry() -> TestResult {
    let source1 = "my $x = 1;\nmy $y = 2;";
    let newline = source1.find('\n').ok_or("expected newline")?;
    let source2 = "my $x = 1;\r\nmy $y = 2;";
    let mut parser = IncrementalParserV2::new();
    parser.parse(source1)?;

    parser.edit(edit(newline, newline, newline + 1));
    let incremental = parser.parse(source2)?;

    assert_eq!(incremental, parse_fresh(source2)?);
    assert!(parser.used_incremental_path());
    assert!(!parser.used_advanced_reuse());
    assert_eq!(parser.reparsed_nodes, 0);
    Ok(())
}

#[test]
fn declaration_payload_spans_match_a_fresh_parse() -> TestResult {
    assert_leading_whitespace_reuse_matches_fresh(
        "package Geometry::Pkg;\nsub work { 1; }\nBEGIN { 2; }\n",
    )
}

#[test]
fn heredoc_body_span_matches_a_fresh_parse() -> TestResult {
    assert_leading_whitespace_reuse_matches_fresh("my $text = <<'EOF';\nbody\nEOF\n")
}

#[test]
fn recovery_token_span_matches_a_fresh_parse() -> TestResult {
    assert_leading_whitespace_reuse_matches_fresh("my $x = ;")
}

#[test]
fn program_root_stays_anchored_with_leading_whitespace() -> TestResult {
    let source = "\n  my $x = 1;\n";
    let fresh = parse_fresh(source)?;
    let fresh_start = match &fresh.kind {
        NodeKind::Program { .. } => fresh.location.start,
        other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
    };
    assert_eq!(fresh_start, 0);

    let shifted = format!(" {source}");
    let shifted_fresh = parse_fresh(&shifted)?;
    let shifted_fresh_start = match &shifted_fresh.kind {
        NodeKind::Program { .. } => shifted_fresh.location.start,
        other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
    };

    let mut parser = IncrementalParserV2::new();
    parser.parse(source)?;
    parser.edit(edit(0, 0, 1));
    let incremental = parser.parse(&shifted)?;
    let incremental_start = match &incremental.kind {
        NodeKind::Program { .. } => incremental.location.start,
        other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
    };

    assert!(parser.used_incremental_path());
    assert_eq!(shifted_fresh_start, 0);
    assert_eq!(incremental_start, shifted_fresh_start);
    Ok(())
}

#[test]
fn token_body_whitespace_never_uses_the_basic_fast_path() -> TestResult {
    let cases = [
        ("my $s = \"a b\";", "my $s = \"a  b\";", "b\";"),
        ("my $x = 1; # a b\n", "my $x = 1; # a  b\n", "b\n"),
        ("print <<'EOF';\na b\nEOF\n", "print <<'EOF';\na  b\nEOF\n", "b\nEOF"),
    ];

    for (source1, source2, marker) in cases {
        let insertion = source1.find(marker).ok_or("expected token-body marker")?;
        let mut parser = IncrementalParserV2::new();
        parser.parse(source1)?;
        parser.edit(edit(insertion, insertion, insertion + 1));

        let incremental = parser.parse(source2)?;
        assert_eq!(incremental, parse_fresh(source2)?);
        assert_not_zero_reparse_basic_path(&parser);
    }
    Ok(())
}

#[test]
fn stale_temporal_order_falls_back_without_corrupting_the_tree() -> TestResult {
    let source1 = "my $x = 1;my $y = 2;";
    let source2 = " my $x = 1;my $y = 2; ";
    let mut parser = IncrementalParserV2::new();
    parser.parse(source1)?;

    // These edits were observed in temporal order: trailing insertion first,
    // then leading insertion. EditSet sorts by byte position, so the trailing
    // edit's progressive coordinate is stale after reordering and must not be
    // trusted by the exact whitespace path.
    parser.edit(edit(source1.len(), source1.len(), source1.len() + 1));
    parser.edit(edit(0, 0, 1));
    let incremental = parser.parse(source2)?;

    assert_eq!(incremental, parse_fresh(source2)?);
    assert_not_zero_reparse_basic_path(&parser);
    Ok(())
}

#[test]
fn mapped_statement_spans_are_safe_for_range_consumers() -> TestResult {
    let source1 = "my $x = 1;my $y = 2;";
    let boundary = source1.find("my $y").ok_or("expected second declaration")?;
    let source2 = "my $x = 1;\nmy $y = 2;";
    let mut parser = IncrementalParserV2::new();
    parser.parse(source1)?;
    parser.edit(edit(boundary, boundary, boundary + 1));

    let incremental = parser.parse(source2)?;
    assert_eq!(incremental, parse_fresh(source2)?);
    let statements = match &incremental.kind {
        NodeKind::Program { statements } => statements,
        other => return Err(format!("expected Program, got {}", other.kind_name()).into()),
    };
    let statement_text: Vec<&str> = statements
        .iter()
        .map(|statement| &source2[statement.location.start..statement.location.end])
        .collect();

    assert_eq!(statement_text, vec!["my $x = 1;", "my $y = 2;"]);
    Ok(())
}
