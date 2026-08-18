use perl_ast::{AstInvariantOptions, AstInvariantReport, validate_ast};
use perl_parser::Parser;

fn assert_valid(source: &str, report: AstInvariantReport, path: &str) {
    assert!(
        report.is_valid(),
        "{path} returned a structurally invalid AST for {source:?}: {:#?}",
        report.findings
    );
    assert!(report.nodes_visited > 0, "{path} did not visit the root node");
}

#[test]
fn strict_parser_outputs_satisfy_the_shared_structural_oracle()
-> Result<(), Box<dyn std::error::Error>> {
    let cases = [
        "use strict; package Demo; sub answer { my ($x) = @_; return $x + 1; }",
        "my $café = 1; if ($café) { print qq/value=$café/; }",
        "my @values = (1, 2, 3); my $sum = $values[0] + $values[1];",
    ];

    for source in cases {
        let mut parser = Parser::new(source);
        let ast = parser.parse()?;
        assert_valid(
            source,
            validate_ast(source, &ast, AstInvariantOptions::default()),
            "strict parse",
        );
    }

    Ok(())
}

#[test]
fn recovered_parser_output_satisfies_the_same_structural_oracle() {
    let source = "my $x = ; print 1;";
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();

    assert!(!output.diagnostics.is_empty(), "fixture must exercise parser recovery");
    assert_valid(
        source,
        validate_ast(source, &output.ast, AstInvariantOptions::default()),
        "recovered parse",
    );
}

#[cfg(feature = "incremental")]
#[test]
fn incremental_edit_output_satisfies_the_same_structural_oracle()
-> Result<(), Box<dyn std::error::Error>> {
    use perl_parser::{Edit, IncrementalState, apply_edits};

    let source = "my $x = 1; print $x;";
    let start = source.find("= 1").ok_or("fixture lost its literal")? + 2;
    let edit = Edit {
        start_byte: start,
        old_end_byte: start + 1,
        new_end_byte: start + 1,
        new_text: "2".to_string(),
    };
    let mut state = IncrementalState::new(source.to_string());
    apply_edits(&mut state, &[edit])?;

    assert_eq!(state.source, "my $x = 2; print $x;");
    assert_valid(
        &state.source,
        validate_ast(
            &state.source,
            &state.snapshot().parse_output().ast,
            AstInvariantOptions::default(),
        ),
        "incremental edit",
    );

    Ok(())
}
