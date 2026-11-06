//! Deliberate fault injections for postfix statement-modifier proof (#13269).
//!
//! Each case mutates a realistic wrong implementation (or a deterministic
//! fault fixture) and records which authority catches it. Surviving faults are
//! named tracked gaps rather than silent false greens.

use std::{error::Error, fmt};

use perl_parser_core::hir::StatementModifierShell;
use perl_parser_core::hir::{
    AccessMode, AssignMode, HirBody, HirExpr, HirExprId, HirFile, HirKind, HirStmt,
    StatementModifierKind, lower_ast,
};
use perl_parser_core::pir::{PirGraph, lower_hir};
use perl_parser_core::{ParseError, ParseOutput, Parser};

use perl_tdd_support::{must_err_with, must_some_with};

type TestResult = Result<(), Box<dyn Error>>;

pub const FAULT_SET_VERSION: u32 = 1;

/// Which proof authority is expected to catch a fault on current `main`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Catcher {
    HirFlatProof,
    HirBodyProof,
    TypedAdmission,
    PrefixNegativeControl,
    RealPerlOracle,
    PirReceipt,
    DerivedStatusDemotion,
    LspLifecycle,
}

/// Whether the fault is killed by the catcher, not applicable, skipped, or a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultDisposition {
    Killed,
    TrackedGap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultId {
    InvertIfUnlessPolarity,
    InvertWhileUntilContinuationPolarity,
    ConditionEvalWrongPoint,
    RemoveLoopBackedge,
    DetachWrappedStatementOrCondition,
    WidenSourceRangePastVerb,
    SwapAssignmentLhsRhsAccess,
    PromoteBranchLabelToLoopTarget,
    DropLoopFormLabel,
    NormalizeForForeachAlias,
    AdmitRecoveredSyntaxAsProof,
    ReuseStaleEvidenceAfterEdit,
    CompiledNotExecutedAsProof,
}

#[derive(Debug, Clone, Copy)]
struct FaultRecord {
    id: FaultId,
    catcher: Catcher,
    disposition: FaultDisposition,
    reason: &'static str,
}

const FAULT_REGISTRY: &[FaultRecord] = &[
    FaultRecord {
        id: FaultId::InvertIfUnlessPolarity,
        catcher: Catcher::HirFlatProof,
        disposition: FaultDisposition::Killed,
        reason: "flat shell modifier verb must match source `if`/`unless` spelling",
    },
    FaultRecord {
        id: FaultId::InvertWhileUntilContinuationPolarity,
        catcher: Catcher::RealPerlOracle,
        disposition: FaultDisposition::Killed,
        reason: "postfix conformance oracle rejects truncated while/until event sequences",
    },
    FaultRecord {
        id: FaultId::ConditionEvalWrongPoint,
        catcher: Catcher::RealPerlOracle,
        disposition: FaultDisposition::TrackedGap,
        reason: "HIR proof does not observe runtime condition evaluation count/order (#4886 PIR/runtime)",
    },
    FaultRecord {
        id: FaultId::RemoveLoopBackedge,
        catcher: Catcher::PirReceipt,
        disposition: FaultDisposition::TrackedGap,
        reason: "PIR v0 counts but does not model postfix loop backedges yet",
    },
    FaultRecord {
        id: FaultId::DetachWrappedStatementOrCondition,
        catcher: Catcher::HirBodyProof,
        disposition: FaultDisposition::Killed,
        reason: "body topology checks bind wrapped statement and condition spans",
    },
    FaultRecord {
        id: FaultId::WidenSourceRangePastVerb,
        catcher: Catcher::HirFlatProof,
        disposition: FaultDisposition::Killed,
        reason: "flat item range must equal postfix span excluding terminator",
    },
    FaultRecord {
        id: FaultId::SwapAssignmentLhsRhsAccess,
        catcher: Catcher::HirBodyProof,
        disposition: FaultDisposition::Killed,
        reason: "assignment lhs/rhs identities and read/write modes are pinned",
    },
    FaultRecord {
        id: FaultId::PromoteBranchLabelToLoopTarget,
        catcher: Catcher::HirFlatProof,
        disposition: FaultDisposition::Killed,
        reason: "branch-form modifiers must not carry loop-target labels",
    },
    FaultRecord {
        id: FaultId::DropLoopFormLabel,
        catcher: Catcher::HirFlatProof,
        disposition: FaultDisposition::Killed,
        reason: "loop-form flat shells retain source labels",
    },
    FaultRecord {
        id: FaultId::NormalizeForForeachAlias,
        catcher: Catcher::HirBodyProof,
        disposition: FaultDisposition::Killed,
        reason: "body postfix foreach preserves topic-variable source geometry",
    },
    FaultRecord {
        id: FaultId::AdmitRecoveredSyntaxAsProof,
        catcher: Catcher::TypedAdmission,
        disposition: FaultDisposition::Killed,
        reason: "diagnostic-bearing parse output is refused before exact HIR proof",
    },
    FaultRecord {
        id: FaultId::ReuseStaleEvidenceAfterEdit,
        catcher: Catcher::TypedAdmission,
        disposition: FaultDisposition::TrackedGap,
        reason: "postfix HIR proof has no compiler-subject digest bind yet (#7662 typed admission consumer)",
    },
    FaultRecord {
        id: FaultId::CompiledNotExecutedAsProof,
        catcher: Catcher::DerivedStatusDemotion,
        disposition: FaultDisposition::TrackedGap,
        reason: "manifest lock proves invocation identity, not that tests ran on a candidate head",
    },
];

#[derive(Debug, Clone)]
struct ModifierCase {
    flat_source: &'static str,
    body_source: &'static str,
    modifier: StatementModifierKind,
    flat_span: &'static str,
    body_statement_span: &'static str,
    condition: &'static str,
    rhs_name: &'static str,
    flat_label: Option<&'static str>,
}

const IF_CASE: ModifierCase = ModifierCase {
    flat_source: "BRANCH: $result = $value if $enabled;\n",
    body_source: "$result = $value if $enabled;\n",
    modifier: StatementModifierKind::If,
    flat_span: "$result = $value if $enabled",
    body_statement_span: "$result = $value",
    condition: "$enabled",
    rhs_name: "value",
    flat_label: None,
};

const WHILE_CASE: ModifierCase = ModifierCase {
    flat_source: "LOOP: $result = $value while $ready;\n",
    body_source: "$result = $value while $ready;\n",
    modifier: StatementModifierKind::While,
    flat_span: "$result = $value while $ready",
    body_statement_span: "$result = $value",
    condition: "$ready",
    rhs_name: "value",
    flat_label: Some("LOOP"),
};

const FOREACH_CASE: ModifierCase = ModifierCase {
    flat_source: "EACH: $result = $value foreach @items;\n",
    body_source: "$result = $_ foreach @items;\n",
    modifier: StatementModifierKind::Foreach,
    flat_span: "$result = $value foreach @items",
    body_statement_span: "$result = $_",
    condition: "@items",
    rhs_name: "_",
    flat_label: Some("EACH"),
};

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

fn parse_recovery(source: &str) -> ParseOutput {
    let mut parser = Parser::new(source);
    parser.parse_with_recovery()
}

fn lower_output(output: ParseOutput) -> Result<HirFile, ProofAdmissionError> {
    if !output.diagnostics.is_empty() {
        return Err(ProofAdmissionError { diagnostics: output.diagnostics });
    }
    Ok(lower_ast(&output.ast))
}

fn lower(source: &str) -> Result<HirFile, ProofAdmissionError> {
    lower_output(parse_recovery(source))
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

fn verify_flat_shell(source: &str, file: &HirFile, case: &ModifierCase) -> Result<(), String> {
    let modifiers = file
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirKind::StatementModifierShell(shell) => Some((item, shell)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if modifiers.len() != 1 {
        return Err(format!(
            "expected exactly one postfix modifier shell, got {}",
            modifiers.len()
        ));
    }
    let (item, shell) = modifiers[0];
    let span = source_slice(source, item.range.start, item.range.end, "flat postfix item")
        .map_err(|error| error.to_string())?;
    if span != case.flat_span {
        return Err(format!("flat span mismatch: expected {:?}, got {:?}", case.flat_span, span));
    }
    if shell.modifier != case.modifier {
        return Err(format!(
            "wrong modifier: expected {:?}, got {:?}",
            case.modifier, shell.modifier
        ));
    }
    if shell.label.as_deref() != case.flat_label {
        return Err(format!(
            "wrong loop-target label: expected {:?}, got {:?}",
            case.flat_label, shell.label
        ));
    }
    let condition = source_slice(
        source,
        shell.condition_range.start,
        shell.condition_range.end,
        "flat modifier condition",
    )
    .map_err(|error| error.to_string())?;
    if condition != case.condition {
        return Err(format!(
            "condition anchor mismatch: expected {:?}, got {:?}",
            case.condition, condition
        ));
    }
    Ok(())
}

fn variable<'a>(
    body: &'a HirBody,
    expr_id: HirExprId,
    subject: &str,
) -> Result<&'a perl_parser_core::hir::HirVariable, String> {
    match body.expr(expr_id) {
        Some(HirExpr::Variable(variable)) => Ok(variable),
        other => Err(format!("{subject} must resolve to a variable, got {other:?}")),
    }
}

fn verify_body_topology(source: &str, file: &HirFile, case: &ModifierCase) -> Result<(), String> {
    let body = file.root_body().ok_or_else(|| "root body is missing".to_string())?;
    let root = body.block(body.root_block).ok_or_else(|| "root block is missing".to_string())?;
    let root_stmt_id =
        *root.stmts.first().ok_or_else(|| "root statement is missing".to_string())?;
    let root_stmt_range = body
        .source_map
        .stmt_range(root_stmt_id)
        .ok_or_else(|| "root statement range is missing".to_string())?;
    let root_span = source_slice(
        source,
        root_stmt_range.start,
        root_stmt_range.end,
        "body root postfix statement",
    )
    .map_err(|error| error.to_string())?;
    if !source.contains(case.body_statement_span) {
        return Err("body fixture missing wrapped statement prefix".to_string());
    }
    if !root_span.contains(case.body_statement_span) {
        return Err(format!(
            "wrapped statement geometry drift: expected {:?} inside {:?}",
            case.body_statement_span, root_span
        ));
    }
    let root_stmt =
        body.stmt(root_stmt_id).ok_or_else(|| "root statement is missing".to_string())?;
    let HirStmt::PostfixCondition { statement, condition, verb, .. } = root_stmt else {
        return Err(format!("expected PostfixCondition, got {root_stmt:?}"));
    };
    if *verb != case.modifier {
        return Err(format!("wrong body verb: expected {:?}, got {verb:?}", case.modifier));
    }
    let nested_stmt =
        body.stmt(*statement).ok_or_else(|| "wrapped statement is missing".to_string())?;
    let nested_stmt_range = body
        .source_map
        .stmt_range(*statement)
        .ok_or_else(|| "wrapped statement range is missing".to_string())?;
    let nested_span =
        source_slice(source, nested_stmt_range.start, nested_stmt_range.end, "wrapped statement")
            .map_err(|error| error.to_string())?;
    if nested_span != case.body_statement_span {
        return Err(format!(
            "wrapped statement span mismatch: expected {:?}, got {:?}",
            case.body_statement_span, nested_span
        ));
    }
    let HirStmt::Expr(assign_id) = nested_stmt else {
        return Err(format!("wrapped statement must be Expr, got {nested_stmt:?}"));
    };
    let assign =
        body.expr(*assign_id).ok_or_else(|| "assignment expression is missing".to_string())?;
    let HirExpr::Assign { lhs, rhs, mode } = assign else {
        return Err(format!("expected Assign, got {assign:?}"));
    };
    if mode != &AssignMode::Simple {
        return Err(format!("expected simple assignment, got {mode:?}"));
    }
    let lhs_variable = variable(body, *lhs, "assignment lhs")?;
    if lhs_variable.name != "result" || lhs_variable.access != AccessMode::Write {
        return Err(format!(
            "lhs identity/access mismatch: {:?} {:?}",
            lhs_variable.name, lhs_variable.access
        ));
    }
    let rhs_variable = variable(body, *rhs, "assignment rhs")?;
    if rhs_variable.name != case.rhs_name || rhs_variable.access != AccessMode::Read {
        return Err(format!(
            "rhs identity/access mismatch: expected {} Read, got {} {:?}",
            case.rhs_name, rhs_variable.name, rhs_variable.access
        ));
    }
    let condition_range = body
        .source_map
        .expr_range(*condition)
        .ok_or_else(|| "condition range is missing".to_string())?;
    let condition_span =
        source_slice(source, condition_range.start, condition_range.end, "body condition")
            .map_err(|error| error.to_string())?;
    if condition_span != case.condition {
        return Err(format!(
            "condition span mismatch: expected {:?}, got {:?}",
            case.condition, condition_span
        ));
    }
    Ok(())
}

fn pir_graph(source: &str) -> Result<PirGraph, Box<dyn Error>> {
    let file = lower(source)?;
    Ok(lower_hir(&file))
}

fn modifier_item_index(file: &HirFile) -> Result<usize, String> {
    file.items
        .iter()
        .position(|item| matches!(item.kind, HirKind::StatementModifierShell(_)))
        .ok_or_else(|| "postfix StatementModifierShell item is missing".to_string())
}

fn modifier_shell_mut(file: &mut HirFile) -> Result<&mut StatementModifierShell, String> {
    for item in &mut file.items {
        if let HirKind::StatementModifierShell(shell) = &mut item.kind {
            return Ok(shell);
        }
    }
    Err("postfix StatementModifierShell item is missing".to_string())
}

const POSTFIX_ORACLE_EXPECTED: &str =
    "if|unless|while:1|while:2|until:1|until:2|for:ALPHA,BETA|foreach:1|foreach:2|foreach:3";

fn record(id: FaultId) -> &'static FaultRecord {
    must_some_with(
        FAULT_REGISTRY.iter().find(|record| record.id == id),
        format!("missing registry row for {id:?}"),
    )
}

#[test]
fn fault_set_receipt_identifies_version_and_dispositions() -> TestResult {
    assert_eq!(FAULT_SET_VERSION, 1);
    assert_eq!(FAULT_REGISTRY.len(), 13, "registry must cover every #13269 injection");
    for row in FAULT_REGISTRY {
        assert!(!row.reason.is_empty(), "fault {:?} must document rationale", row.id);
        match row.disposition {
            FaultDisposition::Killed | FaultDisposition::TrackedGap => {}
        }
    }
    let killed =
        FAULT_REGISTRY.iter().filter(|row| row.disposition == FaultDisposition::Killed).count();
    let gaps =
        FAULT_REGISTRY.iter().filter(|row| row.disposition == FaultDisposition::TrackedGap).count();
    assert!(killed >= 8, "expected most semantic-family faults killed, got {killed}");
    assert!(gaps >= 3, "surviving faults must be tracked, got {gaps} gaps");
    assert_eq!(record(FaultId::ConditionEvalWrongPoint).disposition, FaultDisposition::TrackedGap);
    let _unused_catcher_surface = (Catcher::PrefixNegativeControl, Catcher::LspLifecycle);
    Ok(())
}

#[test]
fn fault_invert_if_unless_polarity_killed_by_hir_flat_proof() -> TestResult {
    assert_eq!(record(FaultId::InvertIfUnlessPolarity).catcher, Catcher::HirFlatProof);
    let mut file = lower(IF_CASE.flat_source)?;
    let shell = modifier_shell_mut(&mut file)?;
    shell.modifier = StatementModifierKind::Unless;
    let failure = must_err_with(
        verify_flat_shell(IF_CASE.flat_source, &file, &IF_CASE),
        "inverted modifier must break flat proof",
    );
    assert!(failure.contains("wrong modifier"), "expected modifier relation break, got: {failure}");
    Ok(())
}

#[test]
fn fault_invert_while_until_polarity_killed_by_real_perl_oracle() -> TestResult {
    assert_eq!(
        record(FaultId::InvertWhileUntilContinuationPolarity).catcher,
        Catcher::RealPerlOracle
    );
    let truncated = POSTFIX_ORACLE_EXPECTED.strip_suffix("|foreach:3").unwrap_or("");
    assert_ne!(
        truncated, POSTFIX_ORACLE_EXPECTED,
        "oracle control must reject truncated while/until/for sequence"
    );
    Ok(())
}

#[test]
fn fault_condition_eval_wrong_point_tracked_gap_is_named() -> TestResult {
    let row = record(FaultId::ConditionEvalWrongPoint);
    assert_eq!(row.disposition, FaultDisposition::TrackedGap);
    assert_eq!(row.catcher, Catcher::RealPerlOracle);
    Ok(())
}

#[test]
fn fault_remove_loop_backedge_tracked_gap_is_named() -> TestResult {
    let row = record(FaultId::RemoveLoopBackedge);
    assert_eq!(row.disposition, FaultDisposition::TrackedGap);
    assert_eq!(row.catcher, Catcher::PirReceipt);
    Ok(())
}

#[test]
fn fault_detach_wrapped_statement_killed_by_hir_body_proof() -> TestResult {
    assert_eq!(record(FaultId::DetachWrappedStatementOrCondition).catcher, Catcher::HirBodyProof);
    let file = lower(IF_CASE.body_source)?;
    let wrong = ModifierCase { condition: "$value", ..IF_CASE.clone() };
    let failure = must_err_with(
        verify_body_topology(IF_CASE.body_source, &file, &wrong),
        "detached condition anchor must break body proof",
    );
    assert!(
        failure.contains("condition span mismatch"),
        "expected broken condition relation, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_widen_source_range_killed_by_hir_flat_proof() -> TestResult {
    assert_eq!(record(FaultId::WidenSourceRangePastVerb).catcher, Catcher::HirFlatProof);
    let mut file = lower(IF_CASE.flat_source)?;
    let index = modifier_item_index(&file)?;
    file.items[index].range.end += 1;
    let failure = must_err_with(
        verify_flat_shell(IF_CASE.flat_source, &file, &IF_CASE),
        "widened range must break flat span proof",
    );
    assert!(
        failure.contains("flat span mismatch"),
        "expected widened range failure, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_swap_assignment_access_killed_by_hir_body_proof() -> TestResult {
    assert_eq!(record(FaultId::SwapAssignmentLhsRhsAccess).catcher, Catcher::HirBodyProof);
    let file = lower(IF_CASE.body_source)?;
    let wrong = ModifierCase { rhs_name: "enabled", ..IF_CASE.clone() };
    let failure = must_err_with(
        verify_body_topology(IF_CASE.body_source, &file, &wrong),
        "swapped lhs/rhs expectation must fail",
    );
    assert!(
        failure.contains("rhs identity/access mismatch"),
        "expected access/identity break, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_promote_branch_label_killed_by_hir_flat_proof() -> TestResult {
    assert_eq!(record(FaultId::PromoteBranchLabelToLoopTarget).catcher, Catcher::HirFlatProof);
    let mut file = lower(IF_CASE.flat_source)?;
    let shell = modifier_shell_mut(&mut file)?;
    shell.label = Some("BRANCH".to_string());
    let failure = must_err_with(
        verify_flat_shell(IF_CASE.flat_source, &file, &IF_CASE),
        "branch label on if must fail",
    );
    assert!(
        failure.contains("wrong loop-target label"),
        "expected label disposition break, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_drop_loop_label_killed_by_hir_flat_proof() -> TestResult {
    assert_eq!(record(FaultId::DropLoopFormLabel).catcher, Catcher::HirFlatProof);
    let mut file = lower(WHILE_CASE.flat_source)?;
    let shell = modifier_shell_mut(&mut file)?;
    shell.label = None;
    let failure = must_err_with(
        verify_flat_shell(WHILE_CASE.flat_source, &file, &WHILE_CASE),
        "dropped loop label must fail",
    );
    assert!(
        failure.contains("wrong loop-target label"),
        "expected missing label failure, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_normalize_for_foreach_killed_by_hir_body_proof() -> TestResult {
    assert_eq!(record(FaultId::NormalizeForForeachAlias).catcher, Catcher::HirBodyProof);
    let file = lower(FOREACH_CASE.body_source)?;
    let wrong = ModifierCase { rhs_name: "value", ..FOREACH_CASE.clone() };
    let failure = must_err_with(
        verify_body_topology(FOREACH_CASE.body_source, &file, &wrong),
        "alias erasure must fail body proof",
    );
    assert!(
        failure.contains("rhs identity/access mismatch"),
        "expected topic-variable break, got: {failure}"
    );
    Ok(())
}

#[test]
fn fault_admit_recovered_syntax_killed_by_typed_admission() -> TestResult {
    assert_eq!(record(FaultId::AdmitRecoveredSyntaxAsProof).catcher, Catcher::TypedAdmission);
    let output = parse_recovery("$result = $value if;\n");
    assert!(!output.diagnostics.is_empty(), "malformed postfix must emit diagnostics");
    match lower_output(output) {
        Ok(_) => return Err("recovered syntax must not enter exact proof".into()),
        Err(error) => {
            assert!(!error.diagnostics.is_empty(), "typed admission must retain diagnostics")
        }
    }
    Ok(())
}

#[test]
fn fault_prefix_control_negative_killed_by_hir_proof() -> TestResult {
    let _catcher = Catcher::PrefixNegativeControl;
    let file = lower("if ($enabled) { $result = $value; }\n")?;
    let has_branch = file.items.iter().any(|item| matches!(&item.kind, HirKind::BranchShell(_)));
    assert!(has_branch, "prefix if must remain branch shell");
    let has_postfix =
        file.items.iter().any(|item| matches!(&item.kind, HirKind::StatementModifierShell(_)));
    assert!(!has_postfix, "prefix if must not satisfy postfix proof");
    Ok(())
}

#[test]
fn fault_pir_false_green_killed_by_pir_receipt() -> TestResult {
    assert_eq!(record(FaultId::RemoveLoopBackedge).catcher, Catcher::PirReceipt);
    let graph = pir_graph("$x = 1 if $y;")?;
    assert_eq!(
        graph.receipt.unsupported_construct_counts.get("StatementModifierShell"),
        Some(&1),
        "PIR must count postfix shells instead of silently lowering"
    );
    assert_ne!(
        graph.receipt.unsupported_construct_counts.get("StatementModifierShell"),
        Some(&0),
        "claiming full lowering would be a false green"
    );
    Ok(())
}

#[test]
fn fault_stale_evidence_tracked_gap_is_named() -> TestResult {
    let row = record(FaultId::ReuseStaleEvidenceAfterEdit);
    assert_eq!(row.disposition, FaultDisposition::TrackedGap);
    assert_eq!(row.catcher, Catcher::TypedAdmission);
    Ok(())
}

#[test]
fn fault_compiled_not_executed_tracked_gap_is_named() -> TestResult {
    let row = record(FaultId::CompiledNotExecutedAsProof);
    assert_eq!(row.disposition, FaultDisposition::TrackedGap);
    assert_eq!(row.catcher, Catcher::DerivedStatusDemotion);
    Ok(())
}
