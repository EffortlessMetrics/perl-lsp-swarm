//! Concept-level parser proofs for contextual statements and control flow (#6687).
//!
//! The tests deliberately stop at parser structure. CFG, control-transfer legality,
//! compile-time effects, and runtime exception behavior remain downstream concerns.

use perl_parser_core::error::RecoveryKind;
use perl_parser_core::{Node, NodeKind, ParseError, Parser};

const PHASES: [&str; 5] = ["BEGIN", "CHECK", "END", "INIT", "UNITCHECK"];

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    if parser.errors().is_empty() {
        Ok(ast)
    } else {
        Err(format!("expected a clean parse, got diagnostics: {:?}", parser.errors()))
    }
}

fn parse_with_inferred_semicolons(source: &str) -> Result<(Node, usize), String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed: {error:?}"))?;
    let inferred = parser
        .errors()
        .iter()
        .filter(|error| {
            matches!(error, ParseError::Recovered { kind: RecoveryKind::InferredSemicolon, .. })
        })
        .count();
    let unexpected = parser
        .errors()
        .iter()
        .filter(|error| {
            !matches!(error, ParseError::Recovered { kind: RecoveryKind::InferredSemicolon, .. })
        })
        .count();
    if unexpected == 0 {
        Ok((ast, inferred))
    } else {
        Err(format!(
            "expected only InferredSemicolon recovery, got diagnostics: {:?}",
            parser.errors()
        ))
    }
}

fn walk(node: &Node, visit: &mut impl FnMut(&Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn source_text(source: &str, node: &Node) -> Option<String> {
    source.get(node.location.start..node.location.end).map(str::to_owned)
}

fn exact_block_call(source: &str, block: &Node) -> Option<(String, String, String, Vec<String>)> {
    let NodeKind::Block { statements } = &block.kind else {
        return None;
    };
    let [statement] = statements.as_slice() else {
        return None;
    };
    let NodeKind::ExpressionStatement { expression } = &statement.kind else {
        return None;
    };
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return None;
    };

    Some((
        source_text(source, statement)?,
        source_text(source, expression)?,
        name.clone(),
        args.iter().map(|arg| source_text(source, arg)).collect::<Option<Vec<_>>>()?,
    ))
}

/// Observation record for one structured `try` statement.
///
/// Kept as a named struct because a 13-field tuple is above Rust's
/// `PartialEq`/`Debug` arity limit and breaks `assert_eq!`.
#[derive(Debug, PartialEq, Eq)]
struct TryStatementShape {
    full: Option<String>,
    body: Option<String>,
    body_is_block: bool,
    body_call: Option<(String, String, String, Vec<String>)>,
    catch_count: usize,
    catch_name: Option<String>,
    catch_name_span: Option<String>,
    catch_body: Option<String>,
    catch_is_block: bool,
    catch_call: Option<(String, String, String, Vec<String>)>,
    finally_body: Option<String>,
    finally_is_block: bool,
    finally_call: Option<(String, String, String, Vec<String>)>,
}

#[test]
fn try_call_and_try_statement_keep_exact_distinct_ownership() -> Result<(), String> {
    let source = concat!(
        "try(1);\n",
        "try { work(); } catch ($error) { recover(); } finally { cleanup(); }\n",
    );
    let ast = parse_clean(source)?;
    let mut calls = Vec::new();
    let mut statements = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::FunctionCall { name, args } if name == "try" => calls.push((
            source_text(source, node),
            args.iter().filter_map(|arg| source_text(source, arg)).collect::<Vec<_>>(),
            args.len() == 1 && matches!(&args[0].kind, NodeKind::Number { value } if value == "1"),
        )),
        NodeKind::Try { body, catch_blocks, finally_block } => {
            let catch = catch_blocks.first();
            let catch_name =
                catch.and_then(|(variable, _)| variable.as_ref()).map(|(name, _)| name.clone());
            let catch_name_span = catch
                .and_then(|(variable, _)| variable.as_ref())
                .and_then(|(_, location)| source.get(location.start..location.end))
                .map(str::to_owned);
            let catch_body = catch.and_then(|(_, block)| source_text(source, block));
            let catch_is_block =
                catch.is_some_and(|(_, block)| matches!(&block.kind, NodeKind::Block { .. }));
            let catch_call = catch.and_then(|(_, block)| exact_block_call(source, block));
            let finally_body =
                finally_block.as_deref().and_then(|block| source_text(source, block));
            let finally_is_block = finally_block
                .as_deref()
                .is_some_and(|block| matches!(&block.kind, NodeKind::Block { .. }));
            let finally_call =
                finally_block.as_deref().and_then(|block| exact_block_call(source, block));

            statements.push(TryStatementShape {
                full: source_text(source, node),
                body: source_text(source, body),
                body_is_block: matches!(&body.kind, NodeKind::Block { .. }),
                body_call: exact_block_call(source, body),
                catch_count: catch_blocks.len(),
                catch_name,
                catch_name_span,
                catch_body,
                catch_is_block,
                catch_call,
                finally_body,
                finally_is_block,
                finally_call,
            });
        }
        _ => {}
    });

    assert_eq!(
        calls,
        vec![(Some("try(1)".to_string()), vec!["1".to_string()], true)],
        "try(...) must remain one ordinary call with its own argument"
    );
    assert_eq!(
        statements,
        vec![TryStatementShape {
            full: Some(
                "try { work(); } catch ($error) { recover(); } finally { cleanup(); }".to_string(),
            ),
            body: Some("{ work(); }".to_string()),
            body_is_block: true,
            body_call: Some((
                "work()".to_string(),
                "work()".to_string(),
                "work".to_string(),
                vec![],
            )),
            catch_count: 1,
            catch_name: Some("$error".to_string()),
            catch_name_span: Some("$error".to_string()),
            catch_body: Some("{ recover(); }".to_string()),
            catch_is_block: true,
            catch_call: Some((
                "recover()".to_string(),
                "recover()".to_string(),
                "recover".to_string(),
                vec![],
            )),
            finally_body: Some("{ cleanup(); }".to_string()),
            finally_is_block: true,
            finally_call: Some((
                "cleanup()".to_string(),
                "cleanup()".to_string(),
                "cleanup".to_string(),
                vec![],
            )),
        }],
        "structured try must own one exact body, catch clause, and finally clause"
    );
    Ok(())
}

#[test]
fn every_phase_keyword_label_remains_distinct_from_its_phase_block() -> Result<(), String> {
    let source = concat!(
        "BEGIN: phase_label();\n",
        "BEGIN { phase_work(); }\n",
        "CHECK: phase_label();\n",
        "CHECK { phase_work(); }\n",
        "END: phase_label();\n",
        "END { phase_work(); }\n",
        "INIT: phase_label();\n",
        "INIT { phase_work(); }\n",
        "UNITCHECK: phase_label();\n",
        "UNITCHECK { phase_work(); }\n",
    );
    let (ast, inferred) = parse_with_inferred_semicolons(source)?;
    assert_eq!(
        inferred,
        PHASES.len(),
        "current main recovers one InferredSemicolon per phase-keyword label statement"
    );
    let mut labels = Vec::new();
    let mut phase_blocks = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::LabeledStatement { label, statement } if PHASES.contains(&label.as_str()) => {
            labels.push((
                label.clone(),
                source_text(source, node).map(|text| text.trim_end_matches(';').to_owned()),
                source_text(source, statement),
                matches!(
                    &statement.kind,
                    NodeKind::ExpressionStatement { expression }
                        if matches!(
                            &expression.kind,
                            NodeKind::FunctionCall { name, args }
                                if name == "phase_label" && args.is_empty()
                        )
                ),
            ));
        }
        NodeKind::PhaseBlock { phase, phase_span, block } if PHASES.contains(&phase.as_str()) => {
            phase_blocks.push((
                phase.clone(),
                phase_span.and_then(|span| source.get(span.start..span.end)).map(str::to_owned),
                source_text(source, node),
                source_text(source, block),
                matches!(&block.kind, NodeKind::Block { .. }),
                exact_block_call(source, block),
            ));
        }
        _ => {}
    });
    labels.sort();
    phase_blocks.sort();

    let expected_labels = PHASES
        .iter()
        .map(|phase| {
            (
                (*phase).to_string(),
                Some(format!("{phase}: phase_label()")),
                Some("phase_label()".to_string()),
                true,
            )
        })
        .collect::<Vec<_>>();
    let expected_phase_blocks = PHASES
        .iter()
        .map(|phase| {
            (
                (*phase).to_string(),
                Some((*phase).to_string()),
                Some(format!("{phase} {{ phase_work(); }}")),
                Some("{ phase_work(); }".to_string()),
                true,
                Some((
                    "phase_work()".to_string(),
                    "phase_work()".to_string(),
                    "phase_work".to_string(),
                    vec![],
                )),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(labels, expected_labels);
    assert_eq!(phase_blocks, expected_phase_blocks);
    Ok(())
}

#[test]
fn eval_and_do_keep_exact_block_versus_expression_children() -> Result<(), String> {
    let source = concat!(
        "eval { work(); };\n",
        "eval \"$code\";\n",
        "do { work(); };\n",
        "do \"file.pl\";\n",
    );
    let ast = parse_clean(source)?;
    let mut eval_blocks = Vec::new();
    let mut eval_expressions = Vec::new();
    let mut do_blocks = Vec::new();
    let mut do_expressions = Vec::new();

    walk(&ast, &mut |node| match &node.kind {
        NodeKind::Eval { block } if matches!(&block.kind, NodeKind::Block { .. }) => {
            eval_blocks.push((
                source_text(source, node),
                source_text(source, block),
                matches!(&block.kind, NodeKind::Block { .. }),
                exact_block_call(source, block),
            ));
        }
        NodeKind::Eval { block } => {
            let string_payload = match &block.kind {
                NodeKind::String { value, interpolated } => Some((value.clone(), *interpolated)),
                _ => None,
            };
            eval_expressions.push((
                source_text(source, node),
                source_text(source, block),
                string_payload,
            ));
        }
        NodeKind::Do { block } if matches!(&block.kind, NodeKind::Block { .. }) => {
            do_blocks.push((
                source_text(source, node),
                source_text(source, block),
                matches!(&block.kind, NodeKind::Block { .. }),
                exact_block_call(source, block),
            ));
        }
        NodeKind::Do { block } => {
            let string_payload = match &block.kind {
                NodeKind::String { value, interpolated } => Some((value.clone(), *interpolated)),
                _ => None,
            };
            do_expressions.push((
                source_text(source, node),
                source_text(source, block),
                string_payload,
            ));
        }
        _ => {}
    });

    assert_eq!(
        eval_blocks,
        vec![(
            Some("eval { work(); }".to_string()),
            Some("{ work(); }".to_string()),
            true,
            Some(("work()".to_string(), "work()".to_string(), "work".to_string(), vec![])),
        )]
    );
    assert_eq!(
        eval_expressions,
        vec![(
            Some("eval \"$code\"".to_string()),
            Some("\"$code\"".to_string()),
            Some(("\"$code\"".to_string(), true)),
        )]
    );
    assert_eq!(
        do_blocks,
        vec![(
            Some("do { work(); }".to_string()),
            Some("{ work(); }".to_string()),
            true,
            Some(("work()".to_string(), "work()".to_string(), "work".to_string(), vec![])),
        )]
    );
    assert_eq!(
        do_expressions,
        vec![(
            Some("do \"file.pl\"".to_string()),
            Some("\"file.pl\"".to_string()),
            Some(("\"file.pl\"".to_string(), true)),
        )]
    );
    Ok(())
}
