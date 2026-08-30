//! Exact HIR proof for Perl postfix statement modifiers.
//!
//! This suite exercises the `control.postfix_modifier` concept at both HIR
//! representations. It deliberately stops before PIR: postfix branch/loop edge
//! semantics remain spec-gated and are not implied by these assertions. The
//! `$_` checks prove only source-level topic-variable HIR reachability; they do
//! not claim implicit binding, iteration, or execution semantics.

use std::{error::Error, fmt};

use perl_parser_core::hir::{
    AccessMode, AssignMode, HIR_BODY_MODEL_VERSION, HirBody, HirExpr, HirExprId, HirFile, HirKind,
    HirStmt, RecoveryConfidence, Sigil, StatementModifierKind, VariableKind, lower_ast,
};
use perl_parser_core::{ParseError, ParseOutput, Parser};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug)]
struct ProofAdmissionError {
    diagnostics: Vec<ParseError>,
}

impl fmt::Display for ProofAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "proof input contains parser diagnostics: {:?}", self.diagnostics)
    }
}

impl Error for ProofAdmissionError {}

#[derive(Debug)]
struct ModifierCase {
    flat_source: &'static str,
    body_source: &'static str,
    modifier: StatementModifierKind,
    flat_span: &'static str,
    body_postfix_span: &'static str,
    body_statement_span: &'static str,
    condition: &'static str,
    condition_name: &'static str,
    condition_sigil: Sigil,
    rhs_name: &'static str,
    flat_label: Option<&'static str>,
}

const CASES: &[ModifierCase] = &[
    ModifierCase {
        flat_source: "BRANCH: $result = $value if $enabled;\n",
        body_source: "$result = $value if $enabled;\n",
        modifier: StatementModifierKind::If,
        flat_span: "$result = $value if $enabled",
        body_postfix_span: "$result = $value if $enabled",
        body_statement_span: "$result = $value",
        condition: "$enabled",
        condition_name: "enabled",
        condition_sigil: Sigil::Scalar,
        rhs_name: "value",
        flat_label: None,
    },
    ModifierCase {
        flat_source: "GUARD: $result = $value unless $disabled;\n",
        body_source: "$result = $value unless $disabled;\n",
        modifier: StatementModifierKind::Unless,
        flat_span: "$result = $value unless $disabled",
        body_postfix_span: "$result = $value unless $disabled",
        body_statement_span: "$result = $value",
        condition: "$disabled",
        condition_name: "disabled",
        condition_sigil: Sigil::Scalar,
        rhs_name: "value",
        flat_label: None,
    },
    ModifierCase {
        flat_source: "LOOP: $result = $value while $ready;\n",
        body_source: "$result = $value while $ready;\n",
        modifier: StatementModifierKind::While,
        flat_span: "$result = $value while $ready",
        body_postfix_span: "$result = $value while $ready",
        body_statement_span: "$result = $value",
        condition: "$ready",
        condition_name: "ready",
        condition_sigil: Sigil::Scalar,
        rhs_name: "value",
        flat_label: Some("LOOP"),
    },
    ModifierCase {
        flat_source: "UNTIL: $result = $value until $done;\n",
        body_source: "$result = $value until $done;\n",
        modifier: StatementModifierKind::Until,
        flat_span: "$result = $value until $done",
        body_postfix_span: "$result = $value until $done",
        body_statement_span: "$result = $value",
        condition: "$done",
        condition_name: "done",
        condition_sigil: Sigil::Scalar,
        rhs_name: "value",
        flat_label: Some("UNTIL"),
    },
    ModifierCase {
        flat_source: "EACH: $result = $value for @items;\n",
        body_source: "$result = $_ for @items;\n",
        modifier: StatementModifierKind::Foreach,
        flat_span: "$result = $value for @items",
        body_postfix_span: "$result = $_ for @items",
        body_statement_span: "$result = $_",
        condition: "@items",
        condition_name: "items",
        condition_sigil: Sigil::Array,
        rhs_name: "_",
        flat_label: Some("EACH"),
    },
    ModifierCase {
        flat_source: "EVERY: $result = $value foreach @items;\n",
        body_source: "$result = $_ foreach @items;\n",
        modifier: StatementModifierKind::Foreach,
        flat_span: "$result = $value foreach @items",
        body_postfix_span: "$result = $_ foreach @items",
        body_statement_span: "$result = $_",
        condition: "@items",
        condition_name: "items",
        condition_sigil: Sigil::Array,
        rhs_name: "_",
        flat_label: Some("EVERY"),
    },
];

fn lower_output(output: ParseOutput) -> Result<HirFile, ProofAdmissionError> {
    // Recovered syntax is diagnostic-bearing input, not an admissible HIR
    // candidate. Return the typed diagnostics before calling `lower_ast`, so
    // partial postfix HIR cannot enter this proof path.
    if !output.diagnostics.is_empty() {
        return Err(ProofAdmissionError { diagnostics: output.diagnostics });
    }
    Ok(lower_ast(&output.ast))
}

fn lower(source: &str) -> Result<HirFile, ProofAdmissionError> {
    lower_output(parse_recovery(source))
}

fn parse_recovery(source: &str) -> ParseOutput {
    let mut parser = Parser::new(source);
    parser.parse_with_recovery()
}

fn source_slice<'a>(
    source: &'a str,
    start: usize,
    end: usize,
    subject: &str,
) -> Result<&'a str, Box<dyn Error>> {
    source.get(start..end).ok_or_else(|| {
        format!("{subject} range {start}..{end} is outside source length {}", source.len()).into()
    })
}

fn variable<'a>(
    body: &'a HirBody,
    expr_id: HirExprId,
    subject: &str,
) -> Result<&'a perl_parser_core::hir::HirVariable, Box<dyn Error>> {
    match body.expr(expr_id) {
        Some(HirExpr::Variable(variable)) => Ok(variable),
        other => Err(format!("{subject} must resolve to a variable, got {other:?}").into()),
    }
}

#[test]
fn postfix_modifiers_preserve_exact_flat_hir_condition_and_label() -> TestResult {
    for case in CASES {
        let file = lower(case.flat_source)?;
        let modifiers = file
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirKind::StatementModifierShell(shell) => Some((item, shell)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            modifiers.len(),
            1,
            "source must lower to exactly one postfix modifier shell: {:?}",
            case.flat_source
        );
        let (item, shell) = modifiers.first().copied().ok_or_else(|| {
            format!("postfix modifier shell is missing for {:?}", case.flat_source)
        })?;
        assert_eq!(
            source_slice(
                case.flat_source,
                item.range.start,
                item.range.end,
                "flat-HIR postfix item",
            )?,
            case.flat_span,
            "flat HIR item must span the complete postfix statement, not only its condition or label"
        );
        assert_eq!(shell.modifier, case.modifier, "wrong modifier for {:?}", case.flat_source);
        assert_eq!(
            shell.label.as_deref(),
            case.flat_label,
            "wrong loop-target label for {:?}",
            case.flat_source
        );
        assert_eq!(
            source_slice(
                case.flat_source,
                shell.condition_range.start,
                shell.condition_range.end,
                "flat-HIR modifier condition",
            )?,
            case.condition,
            "flat HIR must anchor the exact condition operand for {:?}",
            case.flat_source
        );
        assert_eq!(item.anchor.node_kind, "StatementModifier");
        assert_eq!(
            item.recovery_confidence,
            RecoveryConfidence::Parsed,
            "valid postfix modifier must retain parsed confidence for {:?}",
            case.flat_source
        );
        assert!(
            !file
                .items
                .iter()
                .any(|candidate| matches!(&candidate.kind, HirKind::DynamicBoundary(_))),
            "static postfix modifier must not mint a dynamic boundary for {:?}",
            case.flat_source
        );
    }

    Ok(())
}

#[test]
fn postfix_modifiers_preserve_exact_body_hir_topology_and_sources() -> TestResult {
    for case in CASES {
        let file = lower(case.body_source)?;
        assert_eq!(
            file.body_model_version, HIR_BODY_MODEL_VERSION,
            "production body lowering must attach the current HIR body model for {:?}",
            case.body_source
        );
        let body = file
            .root_body()
            .ok_or_else(|| format!("root body is missing for {:?}", case.body_source))?;
        let root = body
            .block(body.root_block)
            .ok_or_else(|| format!("root block is missing for {:?}", case.body_source))?;
        assert_eq!(
            root.stmts.len(),
            1,
            "source must lower to one root statement: {:?}",
            case.body_source
        );

        let root_stmt_id = root
            .stmts
            .first()
            .copied()
            .ok_or_else(|| format!("root statement is missing for {:?}", case.body_source))?;
        let root_stmt_range = body
            .source_map
            .stmt_range(root_stmt_id)
            .ok_or_else(|| format!("root statement range is missing for {:?}", case.body_source))?;
        assert_eq!(
            source_slice(
                case.body_source,
                root_stmt_range.start,
                root_stmt_range.end,
                "body-HIR root postfix statement",
            )?,
            case.body_postfix_span,
            "body HIR root statement must span the complete postfix statement"
        );
        let root_stmt = body
            .stmt(root_stmt_id)
            .ok_or_else(|| format!("root statement is missing for {:?}", case.body_source))?;
        // Body HIR deliberately has no label slot on `PostfixCondition`. The
        // flat `StatementModifierShell` owns loop-target label disposition;
        // this exhaustive shape is the body-model contract.
        let HirStmt::PostfixCondition { statement, condition, verb } = root_stmt else {
            return Err(format!(
                "expected body-HIR postfix condition for {:?}, got {root_stmt:?}",
                case.body_source
            )
            .into());
        };
        assert_eq!(*verb, case.modifier, "wrong body-HIR verb for {:?}", case.body_source);

        let nested_stmt = body
            .stmt(*statement)
            .ok_or_else(|| format!("wrapped statement is missing for {:?}", case.body_source))?;
        let nested_stmt_range = body.source_map.stmt_range(*statement).ok_or_else(|| {
            format!("wrapped statement range is missing for {:?}", case.body_source)
        })?;
        assert_eq!(
            source_slice(
                case.body_source,
                nested_stmt_range.start,
                nested_stmt_range.end,
                "body-HIR wrapped statement",
            )?,
            case.body_statement_span,
            "body HIR wrapped statement must remain linked to the source prefix"
        );
        let HirStmt::Expr(assign_id) = nested_stmt else {
            return Err(format!(
                "wrapped statement must remain an expression for {:?}, got {nested_stmt:?}",
                case.body_source
            )
            .into());
        };
        let assign = body
            .expr(*assign_id)
            .ok_or_else(|| format!("wrapped expression is missing for {:?}", case.body_source))?;
        let HirExpr::Assign { lhs, rhs, mode } = assign else {
            return Err(format!(
                "wrapped expression must remain an assignment for {:?}, got {assign:?}",
                case.body_source
            )
            .into());
        };
        assert_eq!(mode, &AssignMode::Simple);

        let lhs_variable = variable(body, *lhs, "assignment lhs")?;
        assert_eq!(lhs_variable.sigil, Sigil::Scalar);
        assert_eq!(lhs_variable.name, "result");
        assert_eq!(lhs_variable.kind, VariableKind::Package);
        assert_eq!(lhs_variable.access, AccessMode::Write);

        let rhs_variable = variable(body, *rhs, "assignment rhs")?;
        assert_eq!(rhs_variable.sigil, Sigil::Scalar);
        assert_eq!(rhs_variable.name, case.rhs_name);
        assert_eq!(rhs_variable.kind, VariableKind::Package);
        assert_eq!(rhs_variable.access, AccessMode::Read);

        if case.modifier == StatementModifierKind::Foreach {
            assert_eq!(
                source_slice(
                    case.body_source,
                    body.source_map
                        .expr_range(*rhs)
                        .ok_or_else(|| format!(
                            "topic-variable range is missing for {:?}",
                            case.body_source
                        ))?
                        .start,
                    body.source_map
                        .expr_range(*rhs)
                        .ok_or_else(|| format!(
                            "topic-variable range is missing for {:?}",
                            case.body_source
                        ))?
                        .end,
                    "postfix foreach topic variable",
                )?,
                "$_",
                "postfix for/foreach must preserve the source-level topic variable"
            );
        }

        let condition_variable = variable(body, *condition, "postfix condition")?;
        assert_eq!(
            condition_variable.sigil, case.condition_sigil,
            "wrong condition sigil for {:?}",
            case.body_source
        );
        assert_eq!(
            condition_variable.name, case.condition_name,
            "wrong condition variable for {:?}",
            case.body_source
        );
        assert_eq!(condition_variable.kind, VariableKind::Package);
        assert_eq!(condition_variable.access, AccessMode::Read);

        let assignment_range = body
            .source_map
            .expr_range(*assign_id)
            .ok_or_else(|| format!("assignment range is missing for {:?}", case.body_source))?;
        assert_eq!(
            source_slice(
                case.body_source,
                assignment_range.start,
                assignment_range.end,
                "body-HIR assignment",
            )?,
            case.body_statement_span,
            "wrapped statement geometry drifted for {:?}",
            case.body_source
        );

        let condition_range = body
            .source_map
            .expr_range(*condition)
            .ok_or_else(|| format!("condition range is missing for {:?}", case.body_source))?;
        assert_eq!(
            source_slice(
                case.body_source,
                condition_range.start,
                condition_range.end,
                "body-HIR condition",
            )?,
            case.condition,
            "body HIR must anchor the exact condition operand for {:?}",
            case.body_source
        );
    }

    Ok(())
}

#[test]
fn prefix_control_flow_does_not_mint_postfix_modifier_proof() -> TestResult {
    for (source, subject, shell) in [
        ("if ($enabled) { $result = $value; }\n", "prefix if", "branch"),
        ("unless ($disabled) { $result = $value; }\n", "prefix unless", "branch"),
        ("while ($ready) { $result = $value; }\n", "prefix while", "loop"),
        ("until ($done) { $result = $value; }\n", "prefix until", "loop"),
        ("for (@items) { $result = $value; }\n", "prefix for", "loop"),
        ("foreach (@items) { $result = $value; }\n", "prefix foreach", "loop"),
    ] {
        let file = lower(source)?;
        let has_expected_shell = match shell {
            "branch" => file.items.iter().any(|item| matches!(&item.kind, HirKind::BranchShell(_))),
            "loop" => file.items.iter().any(|item| matches!(&item.kind, HirKind::LoopShell(_))),
            _ => false,
        };
        assert!(has_expected_shell, "{subject} must retain its prefix {shell} shell");
        assert!(
            !file.items.iter().any(|item| matches!(&item.kind, HirKind::StatementModifierShell(_))),
            "{subject} must not be counted as postfix-modifier proof"
        );
    }

    Ok(())
}

#[test]
fn malformed_and_chained_modifiers_are_rejected_before_hir_proof_admission() -> TestResult {
    for (source, subject) in [
        ("$result = $value if;\n", "missing modifier condition"),
        ("$result = $value if $enabled while $ready;\n", "chained statement modifiers"),
    ] {
        let output = parse_recovery(source);
        assert!(
            output.diagnostics.iter().any(|diagnostic| matches!(
                diagnostic,
                ParseError::UnexpectedToken { .. }
                    | ParseError::SyntaxError { .. }
                    | ParseError::Recovered { .. }
                    | ParseError::UnexpectedEof
            )),
            "{subject} must produce a typed syntax/recovery diagnostic: {source:?}; diagnostics: {:?}",
            output.diagnostics
        );
        // The error branch contains diagnostics but no HirFile, which is the
        // no-partial-HIR admission contract for recovered syntax.
        let rejection = match lower_output(output) {
            Ok(_) => return Err(format!("{subject} was admitted as HIR: {source:?}").into()),
            Err(error) => error,
        };
        assert!(
            !rejection.diagnostics.is_empty(),
            "{subject} rejection must retain typed parser diagnostics: {source:?}"
        );
    }

    Ok(())
}
