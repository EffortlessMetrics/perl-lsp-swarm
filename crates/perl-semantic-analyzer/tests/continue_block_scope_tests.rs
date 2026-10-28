#![deny(clippy::map_err_ignore)]
//! Continue blocks must agree across scope diagnostics, symbols, and hover.

use perl_semantic_analyzer::pragma_tracker::PragmaTracker;
use perl_semantic_analyzer::scope_analyzer::{IssueKind, ScopeAnalyzer};
use perl_semantic_analyzer::semantic::SemanticAnalyzer;
use perl_semantic_analyzer::symbol::SymbolKind;
use perl_semantic_analyzer::{Parser, SourceLocation};
use perl_tdd_support::{must, must_some_with};

#[test]
fn continue_declarations_and_uses_reach_symbols_and_hover() {
    for keyword in ["while", "until"] {
        let source =
            format!("use strict; {keyword} (0) {{ }} continue {{ my $step = 1; print $step; }}");
        let ast = must(Parser::new(&source).parse());
        let issues = ScopeAnalyzer::new().analyze(&ast, &source, &PragmaTracker::build(&ast));
        assert!(issues.is_empty(), "{keyword}: {issues:?}");
        let semantic = SemanticAnalyzer::analyze_with_source(&ast, &source);
        let table = semantic.symbol_table();
        let positions: Vec<_> = source.match_indices("$step").map(|(offset, _)| offset).collect();
        assert_eq!(positions.len(), 2);
        let start = positions[1];
        let location = SourceLocation { start, end: start + "$step".len() };
        let symbols = table.find_symbol("step", table.scope_at_offset(start), SymbolKind::scalar());
        assert_eq!(symbols.len(), 1, "{keyword}: continue declaration extracted");
        assert_eq!(symbols[0].location.start, positions[0]);
        let references = must_some_with(table.references.get("step"), "continue use extracted");
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].location, location);
        assert_eq!(references[0].scope_id, symbols[0].scope_id);
        assert_eq!(
            must_some_with(semantic.hover_at(location), "continue use has hover").signature,
            "my $step"
        );
        assert!(semantic.semantic_tokens().iter().any(|token| token.location == location));
    }
}

#[test]
fn continue_scope_keeps_nested_shadowing_and_outer_binding_separate() {
    let source = "use strict; use feature 'state'; state $step = 10; while (0) { } \
                  continue { my $step = 1; { print $step; } } print $step;";
    let ast = must(Parser::new(source).parse());
    let issues = ScopeAnalyzer::new().analyze(&ast, source, &PragmaTracker::build(&ast));
    assert!(!issues.iter().any(|issue| issue.kind == IssueKind::UndeclaredVariable));
    let semantic = SemanticAnalyzer::analyze_with_source(&ast, source);
    let table = semantic.symbol_table();
    let positions: Vec<_> = source.match_indices("$step").map(|(offset, _)| offset).collect();
    assert_eq!(positions.len(), 4);
    for (index, &start) in positions.iter().enumerate().skip(2) {
        let symbols = table.find_symbol("step", table.scope_at_offset(start), SymbolKind::scalar());
        let binding = must_some_with(symbols.first(), "use resolves to an existing binding");
        assert_eq!(binding.location.start, positions[if index == 2 { 1 } else { 0 }]);
        let hover = must_some_with(
            semantic.hover_at(SourceLocation { start, end: start + "$step".len() }),
            "resolved use has hover",
        );
        assert_eq!(hover.signature, if index == 2 { "my $step" } else { "state $step" });
    }
}

#[test]
fn body_and_continue_locals_do_not_escape_their_blocks() {
    let source = "use strict; while (0) { my $body_only = 2; print $body_only; } \
                  continue { print $body_only; my $step_only = 3; print $step_only; } \
                  print $step_only;";
    let ast = must(Parser::new(source).parse());
    let issues = ScopeAnalyzer::new().analyze(&ast, source, &PragmaTracker::build(&ast));
    let semantic = SemanticAnalyzer::analyze_with_source(&ast, source);
    for name in ["body_only", "step_only"] {
        let token = format!("${name}");
        let positions: Vec<_> = source.match_indices(&token).map(|(offset, _)| offset).collect();
        assert_eq!(positions.len(), 3);
        assert!(issues.iter().any(|issue| {
            issue.kind == IssueKind::UndeclaredVariable && issue.range.0 == positions[2]
        }));
        let table = semantic.symbol_table();
        let visible =
            table.find_symbol(name, table.scope_at_offset(positions[1]), SymbolKind::scalar());
        assert_eq!(visible.len(), 1, "{name}: declaration extracted in owning block");
        assert_eq!(visible[0].location.start, positions[0]);
        if name == "step_only" {
            assert!(
                semantic
                    .hover_at(SourceLocation {
                        start: positions[1],
                        end: positions[1] + token.len()
                    })
                    .is_some(),
                "continue-local use has hover"
            );
        }
        assert!(
            table
                .find_symbol(name, table.scope_at_offset(positions[2]), SymbolKind::scalar())
                .is_empty()
        );
        assert!(
            semantic
                .hover_at(SourceLocation { start: positions[2], end: positions[2] + token.len() })
                .is_none(),
            "{name}: local does not escape its block"
        );
    }
}

#[test]
fn existing_block_uses_before_declaration_keep_the_outer_binding() {
    let source = "use feature 'state'; state $value = 10; \
                  { print $value; my $value = $value; print $value; }";
    let ast = must(Parser::new(source).parse());
    let semantic = SemanticAnalyzer::analyze_with_source(&ast, source);
    let positions: Vec<_> = source.match_indices("$value").map(|(offset, _)| offset).collect();
    assert_eq!(positions.len(), 5);
    for index in [1, 3] {
        let start = positions[index];
        let hover = must_some_with(
            semantic.hover_at(SourceLocation { start, end: start + "$value".len() }),
            "pre-declaration and initializer uses retain outer hover",
        );
        assert_eq!(hover.signature, "state $value");
    }
}
