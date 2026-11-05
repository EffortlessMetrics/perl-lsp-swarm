//! Parser-backed expression-eval regressions for #19087.

use super::{CriticCategory, CriticContext, CriticFinding, NativeCriticRegistry, StringEvalRule};
use crate::config::{EffectiveCriticState, EffectiveNativeCriticConfig};
use crate::providers::diagnostics::{Diagnostic, DiagnosticSeverity, security::check_security};
use crate::tooling::perl_critic::{
    CriticConfig, CriticFindingOrigin, NativeCriticProfile, NativeCriticRunCompleteness,
    NativeCriticService, NativeCriticSubject, RunGate, Severity, critic_source_identity_for_uri,
};
use perl_parser::Parser;
use perl_parser_core::{Node, NodeKind, SourceLocation};
use perl_tdd_support::{must, must_some};

// Expected spans are literal source expectations. The parser currently omits
// the outer closing parenthesis from grouped expression operands; these cases
// preserve that existing geometry rather than claiming to repair it here.
const EXPRESSIONS: &[(&str, &str, &str)] = &[
    ("eval $obj->get_code();", "MethodCall", "eval $obj->get_code()"),
    ("eval get_code();", "FunctionCall", "eval get_code()"),
    ("eval $codes[0];", "Binary", "eval $codes[0]"),
    ("eval $codes{choice};", "Binary", "eval $codes{choice}"),
    ("eval($flag ? $left : $right);", "Ternary", "eval($flag ? $left : $right"),
    ("eval do { '40 + 2' };", "Do", "eval do { '40 + 2' }"),
    ("eval(do { '40 + 2' });", "Do", "eval(do { '40 + 2' }"),
];

fn parse_clean(source: &str) -> Node {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    assert!(parser.errors().is_empty(), "{source:?}: {:?}", parser.errors());
    ast
}

fn first_eval_operand(node: &Node) -> Option<&Node> {
    match &node.kind {
        NodeKind::Eval { block } => Some(block),
        _ => node.children().into_iter().find_map(first_eval_operand),
    }
}

fn native_findings(source: &str, ast: &Node, config: &CriticConfig) -> Vec<CriticFinding> {
    NativeCriticRegistry::with_rules(vec![Box::new(StringEvalRule)])
        .check(&CriticContext::new(source, ast, config))
}

fn core_findings(ast: &Node) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    check_security(ast, &mut diagnostics);
    diagnostics.into_iter().filter(|row| row.code.as_deref() == Some("PL600")).collect()
}

#[test]
fn native_string_eval_reports_supported_expression_operands() {
    let mut observed = Vec::new();
    let mut expected = Vec::new();
    for &(source, kind, span) in EXPRESSIONS {
        let ast = parse_clean(source);
        assert_eq!(must_some(first_eval_operand(&ast)).kind.kind_name(), kind, "{source}");
        let findings = native_findings(source, &ast, &CriticConfig::default());
        for finding in &findings {
            assert_eq!(finding.rule_id, "native.security.string_eval");
            assert_eq!(finding.suppression_key, "native.security.string_eval");
            assert_eq!(finding.category, CriticCategory::Security);
            assert_eq!(finding.severity, Severity::Harsh);
            assert!(finding.fix.is_none(), "expression eval has no safe automatic replacement");
        }
        observed.push(
            findings
                .iter()
                .map(|row| source[row.range.start.byte..row.range.end.byte].to_string())
                .collect::<Vec<_>>(),
        );
        expected.push(vec![span.to_string()]);
    }
    assert_eq!(observed, expected, "every supported expression must produce one native finding");
}

#[test]
fn core_string_eval_reports_supported_expression_operands() {
    let mut observed = Vec::new();
    let mut expected = Vec::new();
    for &(source, kind, span) in EXPRESSIONS {
        let ast = parse_clean(source);
        assert_eq!(must_some(first_eval_operand(&ast)).kind.kind_name(), kind, "{source}");
        let findings = core_findings(&ast);
        for finding in &findings {
            assert_eq!(finding.severity, DiagnosticSeverity::Warning);
            assert!(!finding.fixable);
            assert!(finding.critic_observation.is_none(), "overlap admission is unchanged");
        }
        observed.push(
            findings
                .iter()
                .map(|row| source[row.range.0..row.range.1].to_string())
                .collect::<Vec<_>>(),
        );
        expected.push(vec![span.to_string()]);
    }
    assert_eq!(observed, expected, "every supported expression must produce one PL600 finding");
}

#[test]
fn string_eval_accepts_blocks_and_non_eval_syntax() {
    for source in [
        "eval { get_code(); };",
        "my $text = 'eval $obj->get_code()';",
        "$obj->eval($code);",
        "my %handlers = (eval => $code);",
        "&eval(get_code());",
        "&eval($code);",
        "&eval();",
    ] {
        let ast = parse_clean(source);
        assert!(native_findings(source, &ast, &CriticConfig::default()).is_empty(), "{source}");
        assert!(core_findings(&ast).is_empty(), "{source}");
    }
}

#[test]
fn string_eval_finds_the_inner_expression_once() {
    let source = "eval { eval $obj->get_code(); };";
    let ast = parse_clean(source);
    assert!(matches!(must_some(first_eval_operand(&ast)).kind, NodeKind::Block { .. }));
    let native = native_findings(source, &ast, &CriticConfig::default());
    let core = core_findings(&ast);
    assert_eq!((native.len(), core.len()), (1, 1));
    assert_eq!(
        &source[native[0].range.start.byte..native[0].range.end.byte],
        "eval $obj->get_code()"
    );
    assert_eq!(&source[core[0].range.0..core[0].range.1], "eval $obj->get_code()");
}

#[test]
fn string_eval_limits_recovery_exclusion_to_the_immediate_operand() {
    let mut observed = Vec::new();
    for (source, immediate_recovery) in [
        ("eval $obj->;", true),
        ("eval $code .;", false),
        ("my $unfinished = ;\neval $obj->get_code();", false),
    ] {
        let mut parser = Parser::new(source);
        let ast = must(parser.parse());
        assert!(!parser.errors().is_empty(), "fixture must exercise recovery: {source}");
        let operand = must_some(first_eval_operand(&ast));
        assert_eq!(operand.kind.is_recovery(), immediate_recovery, "{source}: {operand:?}");
        if source == "eval $code .;" {
            assert!(matches!(&operand.kind, NodeKind::Binary { op, .. } if op == "."));
            assert!(operand.children().iter().any(|child| child.kind.is_recovery()));
        }
        observed.push((
            native_findings(source, &ast, &CriticConfig::default()).len(),
            core_findings(&ast).len(),
        ));
    }
    assert_eq!(observed, [(0, 0), (1, 1), (1, 1)]);
}

#[test]
fn string_eval_function_call_fallback_preserves_argument_conventions() {
    // Ordinary eval parses as Eval. These compatibility nodes deliberately
    // exercise the retained FunctionCall fallback with parser-proven operands.
    let source = "eval $obj->get_code();";
    let ast = parse_clean(source);
    let method = must_some(first_eval_operand(&ast)).clone();
    let block_ast = parse_clean("eval { get_code(); };");
    let block = must_some(first_eval_operand(&block_ast)).clone();
    let location = SourceLocation { start: 0, end: source.len() - 1 };
    let wrapped = Node::new(NodeKind::ArrayLiteral { elements: vec![method.clone()] }, location);
    let recovery = Node::new(NodeKind::MissingExpression, SourceLocation { start: 5, end: 5 });
    let mut observed = Vec::new();
    for args in [vec![method], vec![wrapped], vec![block], vec![recovery], Vec::new()] {
        let call = Node::new(NodeKind::FunctionCall { name: "eval".to_string(), args }, location);
        observed.push((
            native_findings(source, &call, &CriticConfig::default()).len(),
            core_findings(&call).len(),
        ));
    }
    // Core already diagnoses a no-argument fallback; native requires an
    // operand. This change does not unify those existing conventions.
    assert_eq!(observed, [(1, 1), (1, 1), (0, 0), (0, 0), (0, 1)]);
}

#[test]
fn native_string_eval_expression_obeys_policy_and_native_suppression() {
    let source = "eval $obj->get_code();";
    let ast = parse_clean(source);
    let mut observed = Vec::new();
    for config in [
        CriticConfig::default(),
        CriticConfig {
            exclude: vec!["native.security.string_eval".to_string()],
            ..CriticConfig::default()
        },
        CriticConfig { severity: Severity::Stern as u8, ..CriticConfig::default() },
    ] {
        observed.push(native_findings(source, &ast, &config).len());
    }
    let suppressed =
        format!("## no critic native.security.string_eval -- reviewed dynamic dispatch\n{source}");
    let ast = parse_clean(&suppressed);
    observed.push(native_findings(&suppressed, &ast, &CriticConfig::default()).len());
    assert_eq!(observed, [1, 0, 0, 0], "the unsuppressed control must report first");
}

#[test]
fn recommended_service_normalizes_string_eval_without_core_observations() {
    let source = "use strict;\nuse warnings;\nmy $obj;\neval $obj->get_code();\n";
    let ast = parse_clean(source);
    let state = EffectiveCriticState::Native(EffectiveNativeCriticConfig {
        profile: NativeCriticProfile::Recommended,
        severity_threshold: 1,
        include: Vec::new(),
        exclude: Vec::new(),
        owning_root: None,
    });
    let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
        "file:///expression_eval.pl",
        critic_source_identity_for_uri("file:///expression_eval.pl", 1),
        &ast,
        source,
        state,
        Vec::new(),
        RunGate::open(),
        RunGate::open(),
    ));
    assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Complete);
    assert_eq!(run.work().observation_candidates_collected, 0);
    assert_eq!(run.work().unresolved_producer_identities, 0);
    assert_eq!(run.findings().len(), 1);
    let finding = &run.findings()[0];
    assert_eq!(finding.canonical_id(), Some("critic.security.string_eval"));
    assert_eq!(finding.public_code(), "native.security.string_eval");
    assert_eq!(finding.severity(), Severity::Harsh);
    assert_eq!(
        &source[finding.range().start.byte..finding.range().end.byte],
        "eval $obj->get_code()"
    );
    assert!(!finding.has_available_fix());
    assert_eq!(finding.contributors().len(), 1);
    assert_eq!(finding.contributors()[0].identity().origin(), CriticFindingOrigin::NativeCritic);
}
