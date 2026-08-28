//! Exact HIR proof for Perl postfix statement modifiers.
//!
//! This suite exercises the `control.postfix_modifier` concept at both HIR
//! representations. It deliberately stops before PIR: postfix branch/loop edge
//! semantics remain spec-gated and are not implied by these assertions.

use std::error::Error;

use perl_parser_core::Parser;
use perl_parser_core::hir::{
    AccessMode, AssignMode, HirBody, HirExpr, HirExprId, HirFile, HirKind, HirStmt, Sigil,
    StatementModifierKind, VariableKind, lower_ast,
};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug)]
struct ModifierCase {
    flat_source: &'static str,
    body_source: &'static str,
    modifier: StatementModifierKind,
    condition: &'static str,
    condition_name: &'static str,
    condition_sigil: Sigil,
    flat_label: Option<&'static str>,
}

const CASES: &[ModifierCase] = &[
    ModifierCase {
        flat_source: "BRANCH: $result = $value if $enabled;\n",
        body_source: "$result = $value if $enabled;\n",
        modifier: StatementModifierKind::If,
        condition: "$enabled",
        condition_name: "enabled",
        condition_sigil: Sigil::Scalar,
        flat_label: None,
    },
    ModifierCase {
        flat_source: "GUARD: $result = $value unless $disabled;\n",
        body_source: "$result = $value unless $disabled;\n",
        modifier: StatementModifierKind::Unless,
        condition: "$disabled",
        condition_name: "disabled",
        condition_sigil: Sigil::Scalar,
        flat_label: None,
    },
    ModifierCase {
        flat_source: "LOOP: $result = $value while $ready;\n",
        body_source: "$result = $value while $ready;\n",
        modifier: StatementModifierKind::While,
        condition: "$ready",
        condition_name: "ready",
        condition_sigil: Sigil::Scalar,
        flat_label: Some("LOOP"),
    },
    ModifierCase {
        flat_source: "UNTIL: $result = $value until $done;\n",
        body_source: "$result = $value until $done;\n",
        modifier: StatementModifierKind::Until,
        condition: "$done",
        condition_name: "done",
        condition_sigil: Sigil::Scalar,
        flat_label: Some("UNTIL"),
    },
    ModifierCase {
        flat_source: "EACH: $result = $value for @items;\n",
        body_source: "$result = $value for @items;\n",
        modifier: StatementModifierKind::Foreach,
        condition: "@items",
        condition_name: "items",
        condition_sigil: Sigil::Array,
        flat_label: Some("EACH"),
    },
    ModifierCase {
        flat_source: "EVERY: $result = $value foreach @items;\n",
        body_source: "$result = $value foreach @items;\n",
        modifier: StatementModifierKind::Foreach,
        condition: "@items",
        condition_name: "items",
        condition_sigil: Sigil::Array,
        flat_label: Some("EVERY"),
    },
];

fn lower(source: &str) -> HirFile {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    assert!(
        parser.errors().is_empty(),
        "fixture must parse without recovery: {source:?}: {:?}",
        parser.errors()
    );
    lower_ast(&output.ast)
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
        let file = lower(case.flat_source);
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
        let file = lower(case.body_source);
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
        let root_stmt = body
            .stmt(root_stmt_id)
            .ok_or_else(|| format!("root statement is missing for {:?}", case.body_source))?;
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
        assert_eq!(rhs_variable.name, "value");
        assert_eq!(rhs_variable.kind, VariableKind::Package);
        assert_eq!(rhs_variable.access, AccessMode::Read);

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
            "$result = $value",
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
fn prefix_control_flow_does_not_mint_postfix_modifier_proof() {
    let branch_file = lower("if ($enabled) { $result = $value; }\n");
    assert!(
        branch_file.items.iter().any(|item| matches!(&item.kind, HirKind::BranchShell(_))),
        "prefix if control must retain its branch shell"
    );
    assert!(
        !branch_file
            .items
            .iter()
            .any(|item| matches!(&item.kind, HirKind::StatementModifierShell(_))),
        "prefix if control must not be counted as postfix-modifier proof"
    );

    let loop_file = lower("while ($ready) { $result = $value; }\n");
    assert!(
        loop_file.items.iter().any(|item| matches!(&item.kind, HirKind::LoopShell(_))),
        "prefix loop control must retain its loop shell"
    );
    assert!(
        !loop_file
            .items
            .iter()
            .any(|item| matches!(&item.kind, HirKind::StatementModifierShell(_))),
        "prefix loop control must not be counted as postfix-modifier proof"
    );
}
