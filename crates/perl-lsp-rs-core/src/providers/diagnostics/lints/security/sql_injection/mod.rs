//! Conservative PL607 producer for dynamically composed SQL text at reviewed
//! DBI SQL-text sinks (#5035 / #16864).
//!
//! Honesty boundaries, each pinned by tests:
//! - Only reviewed DBI statement-taking sinks whose first argument is SQL
//!   text, and whose receiver carries same-file `DBI->connect(...)` evidence,
//!   warn. A method spelled the same on an unproven, shadowed, rebound, or
//!   later-connected receiver stays silent — a security warning never guesses
//!   DB-ness.
//! - Placeholders (`?`) with bind values are the negative control.
//! - A computed statement argument is a typed dynamic boundary and never warns.
//! - `execute` / `fetchrow_*` bind-or-fetch arguments are not SQL-text sinks.
//!
//! This is not a canonical call/value-fact engine, SQL parser, taint tracker,
//! currentness publisher, or autofix. Those remain separately owned.

mod classify;
mod receiver;

use perl_diagnostics::codes::{DiagnosticCode, DiagnosticSeverity};
use perl_parser_core::ast::{Node, NodeKind};

use crate::providers::diagnostics::internal_types::{Diagnostic, RelatedInformation};
use crate::providers::diagnostics::walker::walk_node;
use classify::classify_sql_text;
use receiver::{ReceiverAssignmentIndex, collect_receiver_assignments, scalar_variable_name};

/// DBI methods whose first argument is SQL command text (DBI 1.651).
///
/// `execute` / `fetchrow_*` / `fetchall_*` are absent: they consume bind
/// values or fetch from an already-prepared statement, not SQL text.
const DBI_SQL_TEXT_SINKS: &[&str] = &[
    "prepare",
    "prepare_cached",
    "do",
    "selectrow_array",
    "selectrow_arrayref",
    "selectrow_hashref",
    "selectall_arrayref",
    "selectall_hashref",
    "selectcol_arrayref",
];

fn is_dbi_sql_text_sink(method: &str) -> bool {
    DBI_SQL_TEXT_SINKS.contains(&method)
}

/// Detect SQL assembled from variables and passed to a reviewed DBI
/// statement-taking method.
pub(super) fn check_sql_injection(node: &Node, diagnostics: &mut Vec<Diagnostic>) {
    let mut index = ReceiverAssignmentIndex::new();
    collect_receiver_assignments(node, &mut index);
    let index = index.finish();
    walk_node(node, &mut |visited| {
        if let NodeKind::MethodCall { object, method, args } = &visited.kind {
            check_sql_injection_method_call(object, method, args, visited, &index, diagnostics);
        }
    });
}

fn check_sql_injection_method_call(
    object: &Node,
    method: &str,
    args: &[Node],
    node: &Node,
    index: &ReceiverAssignmentIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !is_dbi_sql_text_sink(method) {
        return;
    }

    // Receiver ambiguity boundary: only a receiver whose same-file pre-sink
    // assignments all come from `DBI->connect` is a proven DBI handle.
    let receiver_is_dbh = scalar_variable_name(object)
        .is_some_and(|name| index.is_proven_dbh(&name, node.location.start));
    if !receiver_is_dbh {
        return;
    }

    let Some(statement_arg) = sql_statement_argument(args) else {
        return;
    };

    if !classify_sql_text(statement_arg).is_admitted_composition() {
        return;
    }

    diagnostics.push(sql_composition_diagnostic(node, method));
}

fn sql_composition_diagnostic(node: &Node, method: &str) -> Diagnostic {
    let range = (node.location.start, node.location.end);
    Diagnostic {
        range,
        severity: DiagnosticSeverity::Warning,
        code: Some(DiagnosticCode::SecuritySqlInjection.as_str().to_string()),
        message: format!(
            "Interpolated SQL passed to ->{method}() is a SQL injection risk. Use placeholders (?) and bind values."
        ),
        related_information: vec![RelatedInformation {
            location: range,
            message: "Values interpolated or concatenated into the SQL text can change the statement's meaning when input is crafted. Placeholders with bind values keep the SQL text static.".to_string(),
        }],
        tags: Vec::new(),
        fixable: false,
        critic_observation: None,
        suggestion: Some(
            "Use placeholders: $dbh->prepare('... WHERE id = ?')->execute($user_id)".to_string(),
        ),
    }
}

/// The SQL statement argument of a DBI sink call.
///
/// Mirrors `check_two_arg_open`/`check_pipe_open`: the parser may represent
/// parenthesized call args as a flat `args` list or as a single wrapped
/// `ArrayLiteral`, so both shapes resolve to the effective argument list.
fn sql_statement_argument(args: &[Node]) -> Option<&Node> {
    match args {
        [Node { kind: NodeKind::ArrayLiteral { elements }, .. }] => elements.first(),
        [first, ..] => Some(first),
        [] => None,
    }
}

#[cfg(test)]
mod tests;
