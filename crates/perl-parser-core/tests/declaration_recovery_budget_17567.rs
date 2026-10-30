//! #17567: #16769's Error shell is deferred recovery work (#7074), while
//! its retained source item and diagnostic still obey core charging (#8786).
use perl_parser_core::{
    NodeKind, ParseBudget, ParseCoreDimension, ParseOutput, ParseStopCause, Parser,
    ParserConfigIdentity,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const INVALID: &str = "my (base, $kept); 1;";
const VALID: &str = "my (undef, $kept); 1;";
// Two source slots, declaration, number, expression statement, program.
// The invalid slot's synthetic Error wrapper is not admitted core work.
const CORE_NODES: usize = 6;

fn parse(source: &str, node_limit: usize, diagnostic_limit: usize) -> ParseOutput {
    let mut budget = ParseBudget::unlimited();
    budget.max_nodes_constructed = node_limit;
    budget.max_errors = diagnostic_limit;
    let config = ParserConfigIdentity::production_default().with_budget(budget);
    Parser::with_production_config(source, config).parse_with_recovery()
}

fn assert_recovered_shape(output: &ParseOutput) -> TestResult {
    let NodeKind::Program { statements } = &output.ast.kind else {
        return Err("expected program".into());
    };
    assert_eq!(statements.len(), 2, "recovery must preserve the following statement");
    let NodeKind::VariableListDeclaration { declarator, variables, .. } = &statements[0].kind
    else {
        return Err("expected declaration".into());
    };
    assert_eq!(declarator, "my");
    assert_eq!(variables.len(), 2);
    let recovered = &variables[0];
    let NodeKind::Error { message, partial: Some(partial), .. } = &recovered.kind else {
        return Err("invalid slot must retain its source item inside an Error shell".into());
    };
    assert_eq!(message, "Can't declare constant item in \"my\"");
    assert!(matches!(&partial.kind, NodeKind::Identifier { name } if name == "base"));
    assert_eq!((recovered.location.start, recovered.location.end), (4, 8));
    assert_eq!(partial.location, recovered.location);
    assert!(
        matches!(&variables[1].kind, NodeKind::Variable { sigil, name } if sigil == "$" && name == "kept")
    );
    assert_eq!(&INVALID[variables[1].location.start..variables[1].location.end], "$kept");
    let NodeKind::ExpressionStatement { expression } = &statements[1].kind else {
        return Err("expected retained expression statement".into());
    };
    assert!(matches!(&expression.kind, NodeKind::Number { value } if value == "1"));
    assert_eq!(&INVALID[expression.location.start..expression.location.end], "1");
    Ok(())
}

#[test]
fn declaration_recovery_shell_is_exempt_but_source_nodes_remain_charged() -> TestResult {
    for limit in [usize::MAX, CORE_NODES, CORE_NODES + 1] {
        let recovered = parse(INVALID, limit, usize::MAX);
        assert_eq!(recovered.stop_cause(), None);
        assert_eq!(recovered.budget_usage.nodes_constructed, CORE_NODES);
        assert_eq!(recovered.budget_usage.errors_emitted, 1);
        assert_eq!(recovered.diagnostics.len(), 1);
        assert!(recovered.diagnostics[0].blocks_clean_parse());
        assert_eq!(recovered.diagnostics[0].location(), Some(4));
        assert_recovered_shape(&recovered)?;

        let valid = parse(VALID, limit, usize::MAX);
        assert_eq!(valid.stop_cause(), None);
        assert_eq!(valid.budget_usage.nodes_constructed, CORE_NODES);
        assert!(valid.diagnostics.is_empty());
    }
    // Even a recovered parse must stop before the next admitted core node.
    for source in [INVALID, VALID] {
        let limited = parse(source, CORE_NODES - 1, usize::MAX);
        assert_eq!(limited.budget_usage.nodes_constructed, CORE_NODES - 1);
        assert_eq!(
            limited.stop_cause(),
            Some(ParseStopCause::CoreBudgetExhausted {
                dimension: ParseCoreDimension::NodesConstructed,
                limit: CORE_NODES - 1,
                usage: CORE_NODES - 1,
            })
        );
    }
    Ok(())
}

#[test]
fn declaration_recovery_diagnostic_uses_the_retention_budget() -> TestResult {
    let unlimited = parse(INVALID, CORE_NODES, usize::MAX);
    for limit in [0, 1] {
        let output = parse(INVALID, CORE_NODES, limit);
        assert_eq!(output.stop_cause(), None);
        assert_eq!(output.budget_usage.nodes_constructed, CORE_NODES);
        assert_eq!(output.budget_usage.errors_emitted, limit);
        assert_eq!(output.diagnostics.len(), limit);
        assert_eq!(output.ast.to_sexp(), unlimited.ast.to_sexp());
        assert_recovered_shape(&output)?;
    }
    Ok(())
}
