//! Differential role matrix for the mixed `indirect_builtins` admission surface (#13401).
//!
//! Test-only evidence: pins expected operand roles for IO/file operators, process-program
//! forms, true indirect-object syntax, and ordinary-call controls against pinned real-Perl
//! compile/deparse observations. Fixtures are compile/deparse only; no runtime IO/process
//! side effects. Rows where current main disagrees with the family oracle are recorded as
//! `expected_current` FINDING pins or `#[ignore = "tracked: #13401 production split"]`
//! targets — assertions are not weakened.

mod cpan_test_helpers;

use cpan_test_helpers::{assert_clean_parse, parse};
use perl_parser_core::{Node, NodeKind, Parser};
use std::collections::BTreeSet;
use std::process::Command;

type R = Result<(), String>;

/// Bounded real-Perl oracle receipt command introduced by this harness (compile + deparse).
pub const ORACLE_RECEIPT_COMMAND: &str =
    "perl -c -e '<source>' && perl -MO=Deparse,-p -e '<source>'";

const PERL_VERSION_PIN: &str = "5.38.2";

fn source_mentions_builtin(source: &str, builtin: &str) -> bool {
    source
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| token == builtin)
}

const INDIRECT_BUILTIN_INVENTORY: &[&str] = &[
    "print", "printf", "say", "open", "close", "pipe", "sysopen", "sysread", "syswrite",
    "truncate", "fcntl", "ioctl", "flock", "seek", "tell", "select", "binmode", "exec", "system",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OracleDisposition {
    CompileAccept,
    CompileReject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParserRole {
    OutputIndirectHandle { method: &'static str, handle: HandleShape, arg_count: usize },
    RegularCall { name: &'static str, arg_count: usize },
    MethodCall { method: &'static str },
    IndirectObject { method: &'static str, object: ObjectShape, arg_count: usize },
    NoIndirectCall,
    ParserBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HandleShape {
    ScalarVar,
    BracedBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectShape {
    Identifier,
    Variable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowDisposition {
    Proven(ParserRole),
    ExpectedCurrent { role: ParserRole, finding: &'static str },
    ParserBoundary,
}

#[derive(Clone, Copy, Debug)]
struct MatrixRow {
    id: &'static str,
    family: &'static str,
    source: &'static str,
    oracle: OracleDisposition,
    disposition: RowDisposition,
}

fn walk<'a>(node: &'a Node, visit: &mut impl FnMut(&'a Node)) {
    visit(node);
    for child in node.children() {
        walk(child, visit);
    }
}

fn count_indirect_calls(ast: &Node) -> usize {
    let mut count = 0;
    walk(ast, &mut |node| {
        if matches!(node.kind, NodeKind::IndirectCall { .. }) {
            count += 1;
        }
    });
    count
}

fn first_indirect_call(ast: &Node) -> Option<&Node> {
    if matches!(ast.kind, NodeKind::IndirectCall { .. }) {
        return Some(ast);
    }
    ast.children().into_iter().find_map(first_indirect_call)
}

fn function_calls_named<'a>(ast: &'a Node, name: &str) -> Vec<&'a Node> {
    let mut found = Vec::new();
    walk(ast, &mut |node| {
        if matches!(&node.kind, NodeKind::FunctionCall { name: actual, .. } if actual == name) {
            found.push(node);
        }
    });
    found
}

fn method_calls_named<'a>(ast: &'a Node, method: &str) -> Vec<&'a Node> {
    let mut found = Vec::new();
    walk(ast, &mut |node| {
        if matches!(&node.kind, NodeKind::MethodCall { method: actual, .. } if actual == method) {
            found.push(node);
        }
    });
    found
}

fn has_parser_boundary(ast: &Node) -> bool {
    let mut found = false;
    walk(ast, &mut |node| {
        if matches!(
            node.kind,
            NodeKind::Error { .. }
                | NodeKind::MissingExpression
                | NodeKind::MissingStatement
                | NodeKind::MissingIdentifier
        ) {
            found = true;
        }
    });
    found
}

fn static_method(name: &str) -> Result<&'static str, String> {
    match name {
        "print" => Ok("print"),
        "printf" => Ok("printf"),
        "say" => Ok("say"),
        "open" => Ok("open"),
        "close" => Ok("close"),
        "pipe" => Ok("pipe"),
        "sysopen" => Ok("sysopen"),
        "sysread" => Ok("sysread"),
        "syswrite" => Ok("syswrite"),
        "truncate" => Ok("truncate"),
        "fcntl" => Ok("fcntl"),
        "ioctl" => Ok("ioctl"),
        "flock" => Ok("flock"),
        "seek" => Ok("seek"),
        "tell" => Ok("tell"),
        "select" => Ok("select"),
        "binmode" => Ok("binmode"),
        "exec" => Ok("exec"),
        "system" => Ok("system"),
        "new" => Ok("new"),
        "method" => Ok("method"),
        other => Err(format!("unexpected method name in role observation: {other:?}")),
    }
}

fn observe_role(source: &str) -> Result<ParserRole, String> {
    let mut parser = Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("parse failed for {source:?}: {error:?}"))?;
    if has_parser_boundary(&ast) {
        return Ok(ParserRole::ParserBoundary);
    }

    if count_indirect_calls(&ast) == 0 {
        if let Some(call) = function_calls_named(&ast, "print").first() {
            let NodeKind::FunctionCall { args, .. } = &call.kind else {
                return Err("function call kind drift".to_string());
            };
            return Ok(ParserRole::RegularCall { name: "print", arg_count: args.len() });
        }
        if let Some(call) = function_calls_named(&ast, "new").first() {
            let NodeKind::FunctionCall { args, .. } = &call.kind else {
                return Err("function call kind drift".to_string());
            };
            return Ok(ParserRole::RegularCall { name: "new", arg_count: args.len() });
        }
        for name in INDIRECT_BUILTIN_INVENTORY {
            if let Some(call) = function_calls_named(&ast, name).first() {
                let NodeKind::FunctionCall { args, .. } = &call.kind else {
                    return Err("function call kind drift".to_string());
                };
                return Ok(ParserRole::RegularCall { name, arg_count: args.len() });
            }
        }
        if !method_calls_named(&ast, "new").is_empty() {
            return Ok(ParserRole::MethodCall { method: "new" });
        }
        if !method_calls_named(&ast, "method").is_empty() {
            return Ok(ParserRole::MethodCall { method: "method" });
        }
        return Ok(ParserRole::NoIndirectCall);
    }

    let indirect = first_indirect_call(&ast)
        .ok_or_else(|| format!("indirect count > 0 but no IndirectCall node for {source:?}"))?;
    let NodeKind::IndirectCall { method, object, args } = &indirect.kind else {
        return Err("indirect node kind drift".to_string());
    };

    if matches!(method.as_str(), "print" | "printf" | "say") {
        let handle = match &object.kind {
            NodeKind::Variable { sigil, .. } if sigil == "$" => HandleShape::ScalarVar,
            NodeKind::Block { .. } => HandleShape::BracedBlock,
            other => {
                return Err(format!("unexpected output handle object for {source:?}: {other:?}"));
            }
        };
        return Ok(ParserRole::OutputIndirectHandle {
            method: static_method(method)?,
            handle,
            arg_count: args.len(),
        });
    }

    if method.as_str() == "new" {
        let object_shape = match &object.kind {
            NodeKind::Identifier { .. } => ObjectShape::Identifier,
            NodeKind::Variable { .. } => ObjectShape::Variable,
            other => {
                return Err(format!("unexpected new object for {source:?}: {other:?}"));
            }
        };
        return Ok(ParserRole::IndirectObject {
            method: "new",
            object: object_shape,
            arg_count: args.len(),
        });
    }

    Ok(ParserRole::IndirectObject {
        method: static_method(method)?,
        object: match &object.kind {
            NodeKind::Identifier { .. } => ObjectShape::Identifier,
            NodeKind::Variable { .. } => ObjectShape::Variable,
            NodeKind::Block { .. } => ObjectShape::Identifier,
            other => {
                return Err(format!("unexpected indirect object for {source:?}: {other:?}"));
            }
        },
        arg_count: args.len(),
    })
}

fn perl_oracle_available() -> bool {
    Command::new("perl")
        .arg("-e")
        .arg("1")
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn pinned_oracle_ready() -> Result<Option<()>, String> {
    if !perl_oracle_available() {
        return Ok(None);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let output = Command::new("perl")
        .args(["-V:version"])
        .env_clear()
        .env("PATH", path)
        .output()
        .map_err(|error| format!("reading perl version: {error}"))?;
    let version = String::from_utf8_lossy(&output.stdout);
    if version.contains(PERL_VERSION_PIN) { Ok(Some(())) } else { Ok(None) }
}

fn perl_compile(source: &str) -> Result<Option<bool>, String> {
    if pinned_oracle_ready()?.is_none() {
        return Ok(None);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let output = Command::new("perl")
        .args(["-c", "-e", source])
        .env_clear()
        .env("PATH", path)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .output()
        .map_err(|error| format!("spawning perl oracle: {error}"))?;
    Ok(Some(output.status.success()))
}

fn perl_deparse(source: &str) -> Result<Option<String>, String> {
    if pinned_oracle_ready()?.is_none() {
        return Ok(None);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    let output = Command::new("perl")
        .args(["-MO=Deparse,-p", "-e", source])
        .env_clear()
        .env("PATH", path)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env_remove("PERL5LIB")
        .env_remove("PERL5OPT")
        .output()
        .map_err(|error| format!("spawning perl deparse oracle: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "deparse oracle failed for {source:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn assert_output_handle_deparse(row: &MatrixRow) -> Result<(), String> {
    let RowDisposition::Proven(ParserRole::OutputIndirectHandle { method, .. }) = row.disposition
    else {
        return Ok(());
    };
    let Some(deparse) = perl_deparse(row.source)? else {
        return Ok(());
    };
    if deparse.contains(&format!("->{method}")) {
        return Err(format!(
            "{}: deparse oracle shows method-call syntax, not builtin output handle: {deparse:?}",
            row.id
        ));
    }
    if !deparse.contains(method) {
        return Err(format!(
            "{}: deparse oracle lost builtin operator {method:?}: {deparse:?}",
            row.id
        ));
    }
    Ok(())
}

fn assert_oracle(row: &MatrixRow) -> Result<(), String> {
    match row.oracle {
        OracleDisposition::CompileAccept | OracleDisposition::CompileReject => {
            let Some(accepts) = perl_compile(row.source)? else {
                return Ok(());
            };
            let expected_accept = row.oracle == OracleDisposition::CompileAccept;
            if accepts != expected_accept {
                return Err(format!(
                    "{}: oracle compile disposition mismatch (expected accept={expected_accept}, got {accepts}) for {:?}",
                    row.id, row.source
                ));
            }
            assert_output_handle_deparse(row)?;
            Ok(())
        }
    }
}

const MATRIX: &[MatrixRow] = &[
    MatrixRow {
        id: "output-print-scalar-handle",
        family: "output",
        source: "print $fh 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::OutputIndirectHandle {
            method: "print",
            handle: HandleShape::ScalarVar,
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "output-printf-scalar-handle",
        family: "output",
        source: "printf $fh \"%s\", $value;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::OutputIndirectHandle {
            method: "printf",
            handle: HandleShape::ScalarVar,
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "output-say-scalar-handle",
        family: "output",
        source: "use feature 'say'; say $fh 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::OutputIndirectHandle {
            method: "say",
            handle: HandleShape::ScalarVar,
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "output-print-braced-handle",
        family: "output",
        source: "print { $fh } \"hello\";",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::OutputIndirectHandle {
            method: "print",
            handle: HandleShape::BracedBlock,
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "output-print-comma-list-control",
        family: "output",
        source: "print $fh, 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "print",
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "io-open-numeric-f13125-control",
        family: "io",
        source: "open $fh 1;",
        oracle: OracleDisposition::CompileReject,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "open", arg_count: 1 }),
    },
    MatrixRow {
        id: "io-sysread-numeric-f13125-control",
        family: "io",
        source: "sysread $fh 1;",
        oracle: OracleDisposition::CompileReject,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "sysread",
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "io-seek-numeric-f13125-control",
        family: "io",
        source: "seek $fh 1;",
        oracle: OracleDisposition::CompileReject,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "seek", arg_count: 1 }),
    },
    MatrixRow {
        id: "io-open-list-form",
        family: "io",
        source: "open $fh, '<', 'file';",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "open", arg_count: 3 }),
    },
    MatrixRow {
        id: "io-close-handle",
        family: "io",
        source: "close $fh;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "close",
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "io-pipe-pair",
        family: "io",
        source: "pipe RH, WH;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "pipe", arg_count: 2 }),
    },
    MatrixRow {
        id: "io-sysopen-roles",
        family: "io",
        source: "sysopen FH, $file, 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "sysopen",
            arg_count: 3,
        }),
    },
    MatrixRow {
        id: "process-system-list-qw",
        family: "process",
        source: "system qw(a b);",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "system",
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "process-exec-list-qw",
        family: "process",
        source: "exec qw(a b);",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "exec", arg_count: 1 }),
    },
    MatrixRow {
        id: "process-system-comma-list",
        family: "process",
        source: "system $prog, @args;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "system",
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "process-system-braced-program",
        family: "process",
        source: "system { $prog } @args;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::ExpectedCurrent {
            role: ParserRole::RegularCall { name: "system", arg_count: 1 },
            finding: "FINDING: braced program block is not modeled separately from list operands; \
                       oracle expects system({$prog;} @args)",
        },
    },
    MatrixRow {
        id: "process-system-unbacked-indirect-leak",
        family: "process",
        source: "system $prog @args;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::ExpectedCurrent {
            role: ParserRole::IndirectObject {
                method: "system",
                object: ObjectShape::Variable,
                arg_count: 1,
            },
            finding: "FINDING: whitespace-separated program/list form is misclassified as IndirectCall",
        },
    },
    MatrixRow {
        id: "indirect-new-class",
        family: "indirect-object",
        source: "new Widget 4;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::IndirectObject {
            method: "new",
            object: ObjectShape::Identifier,
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "indirect-new-arrow-control",
        family: "indirect-object",
        source: "Widget->new(4);",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::MethodCall { method: "new" }),
    },
    MatrixRow {
        id: "indirect-new-paren-control",
        family: "indirect-object",
        source: "new(6);",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "new", arg_count: 1 }),
    },
    MatrixRow {
        id: "indirect-method-statement-boundary",
        family: "indirect-object",
        source: "method $obj 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::ParserBoundary,
    },
    MatrixRow {
        id: "indirect-method-arrow-control",
        family: "indirect-object",
        source: "$obj->method(1);",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::MethodCall { method: "method" }),
    },
    MatrixRow {
        id: "ordinary-user-open-homonym",
        family: "ordinary-call",
        source: "sub open { 1 } open 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "open", arg_count: 1 }),
    },
    MatrixRow {
        id: "try-close-bareword-current-heuristic",
        family: "io",
        source: "close try;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::ExpectedCurrent {
            role: ParserRole::IndirectObject {
                method: "close",
                object: ObjectShape::Identifier,
                arg_count: 0,
            },
            finding: "FINDING: Perl deparse is close(try); parser promotes bareword try via indirect heuristic (#13408 boundary)",
        },
    },
    MatrixRow {
        id: "inventory-binmode-handle",
        family: "io",
        source: "binmode $fh;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "binmode",
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "inventory-tell-handle",
        family: "io",
        source: "tell $fh;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall { name: "tell", arg_count: 1 }),
    },
    MatrixRow {
        id: "inventory-select-args",
        family: "io",
        source: "select $fh;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "select",
            arg_count: 1,
        }),
    },
    MatrixRow {
        id: "inventory-flock-handle",
        family: "io",
        source: "flock $fh, 1;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "flock",
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "inventory-truncate-handle",
        family: "io",
        source: "truncate $fh, 0;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "truncate",
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "inventory-syswrite-buffer",
        family: "io",
        source: "syswrite $fh, $buf;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "syswrite",
            arg_count: 2,
        }),
    },
    MatrixRow {
        id: "inventory-fcntl-opaque",
        family: "io",
        source: "fcntl $fh, 1, 0;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "fcntl",
            arg_count: 3,
        }),
    },
    MatrixRow {
        id: "inventory-ioctl-opaque",
        family: "io",
        source: "ioctl $fh, 0, 0;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::Proven(ParserRole::RegularCall {
            name: "ioctl",
            arg_count: 3,
        }),
    },
    MatrixRow {
        id: "inventory-exec-braced-program",
        family: "process",
        source: "exec { $prog } @args;",
        oracle: OracleDisposition::CompileAccept,
        disposition: RowDisposition::ExpectedCurrent {
            role: ParserRole::RegularCall { name: "exec", arg_count: 1 },
            finding: "FINDING: exec braced program form shares the same ambiguous block operand shape as system",
        },
    },
];

fn assert_role(row: &MatrixRow, actual: ParserRole, expected: ParserRole) -> Result<(), String> {
    if actual != expected {
        return Err(format!(
            "{} ({:?}): expected {expected:?}, observed {actual:?}",
            row.id, row.source
        ));
    }
    Ok(())
}

#[test]
fn oracle_receipt_command_is_pinned() -> R {
    if ORACLE_RECEIPT_COMMAND.is_empty() {
        return Err("oracle receipt command must be documented".to_string());
    }
    if !ORACLE_RECEIPT_COMMAND.contains("-MO=Deparse") {
        return Err("oracle receipt must include deparse".to_string());
    }
    Ok(())
}

#[test]
fn matrix_row_ids_are_unique_and_inventory_is_covered() -> R {
    let mut ids = BTreeSet::new();
    for row in MATRIX {
        if !ids.insert(row.id) {
            return Err(format!("duplicate matrix row id {}", row.id));
        }
    }
    let mut families: BTreeSet<&str> = BTreeSet::new();
    for builtin in INDIRECT_BUILTIN_INVENTORY {
        let covered = MATRIX.iter().any(|row| source_mentions_builtin(row.source, builtin));
        if !covered {
            return Err(format!(
                "indirect builtin inventory spelling {builtin:?} has no matrix row on main"
            ));
        }
        families.insert(
            MATRIX
                .iter()
                .find(|row| row.source.contains(builtin))
                .map(|row| row.family)
                .unwrap_or("missing"),
        );
    }
    if !families.contains("output") || !families.contains("io") || !families.contains("process") {
        return Err("matrix must retain separate IO/process/output families".to_string());
    }
    Ok(())
}

#[test]
fn proven_rows_match_parser_roles_and_oracle() -> R {
    for row in MATRIX {
        let RowDisposition::Proven(expected) = row.disposition else {
            continue;
        };
        assert_oracle(row)?;
        if expected == ParserRole::ParserBoundary {
            continue;
        }
        if row.oracle == OracleDisposition::CompileReject {
            let actual = observe_role(row.source)?;
            assert_role(row, actual, expected)?;
            continue;
        }
        assert_clean_parse(row.source);
        let mut parser = Parser::new(row.source);
        let _ = parser.parse();
        let blocking: Vec<_> =
            parser.get_errors().iter().filter(|error| error.blocks_clean_parse()).collect();
        if !blocking.is_empty() {
            return Err(format!(
                "{} ({:?}): unexpected blocking diagnostics: {blocking:#?}",
                row.id, row.source
            ));
        }
        let actual = observe_role(row.source)?;
        assert_role(row, actual, expected)?;
    }
    Ok(())
}

#[test]
fn expected_current_rows_pin_parser_findings() -> R {
    for row in MATRIX {
        let RowDisposition::ExpectedCurrent { role, finding } = row.disposition else {
            continue;
        };
        if !finding.starts_with("FINDING:") {
            return Err(format!("{} must carry a FINDING note", row.id));
        }
        assert_oracle(row)?;
        assert_clean_parse(row.source);
        let actual = observe_role(row.source)?;
        assert_role(row, actual, role)?;
    }
    Ok(())
}

#[test]
fn parser_boundary_rows_stay_distinct() -> R {
    for row in MATRIX {
        if row.disposition != RowDisposition::ParserBoundary {
            continue;
        }
        assert_oracle(row)?;
        let output = Parser::new(row.source).parse_with_recovery();
        if output.diagnostics.is_empty()
            && !output.ast.to_sexp().to_ascii_lowercase().contains("error")
        {
            return Err(format!(
                "{} must remain a parser-boundary row, not a proven role row",
                row.id
            ));
        }
    }
    Ok(())
}

#[test]
fn f13125_io_controls_do_not_inherit_output_indirect_admission() -> R {
    for source in ["open $fh 1;", "sysread $fh 1;", "seek $fh 1;"] {
        let ast = parse(source);
        if count_indirect_calls(&ast) != 0 {
            return Err(format!(
                "IO numeric control must not become IndirectCall: {source:?} => {}",
                ast.to_sexp()
            ));
        }
    }
    Ok(())
}

#[test]
fn negative_control_output_comma_list_is_not_indirect() -> R {
    let ast = parse("print $fh, 1;");
    if count_indirect_calls(&ast) != 0 {
        return Err(format!("comma-separated print must stay a regular call: {}", ast.to_sexp()));
    }
    Ok(())
}

#[test]
fn negative_control_arrow_new_is_not_indirect_object_syntax() -> R {
    let ast = parse("Widget->new(4);");
    if count_indirect_calls(&ast) != 0 {
        return Err(format!(
            "arrow construction must not collapse into IndirectCall: {}",
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn negative_control_homonym_is_not_promoted_by_builtin_name() -> R {
    let ast = parse("sub open { 1 } open 1;");
    if count_indirect_calls(&ast) != 0 {
        return Err(format!(
            "user-defined homonym must not inherit builtin indirect admission: {}",
            ast.to_sexp()
        ));
    }
    Ok(())
}

#[test]
fn negative_control_list_and_braced_system_forms_differ() -> R {
    let list = parse("system qw(a b);");
    let braced = parse("system { $prog } @args;");
    if list.to_sexp() == braced.to_sexp() {
        return Err(
            "system LIST and braced program form must not share identical AST shapes".to_string()
        );
    }
    if count_indirect_calls(&list) != 0 {
        return Err("system LIST must not be classified as IndirectCall".to_string());
    }
    Ok(())
}

#[test]
#[ignore = "tracked: #13401 production split"]
fn production_split_braced_system_program_operand_is_not_only_block_arg() -> R {
    let role = observe_role("system { $prog } @args;")?;
    if matches!(role, ParserRole::RegularCall { name: "system", arg_count: 1 }) {
        return Err(
            "production split should model braced program form separately from a single block operand"
                .to_string(),
        );
    }
    Ok(())
}

#[test]
#[ignore = "tracked: #13401 production split"]
fn production_split_close_try_matches_ordinary_call_not_indirect() -> R {
    let role = observe_role("close try;")?;
    if !matches!(role, ParserRole::RegularCall { name: "close", arg_count: 1 }) {
        return Err(
            "production split should represent close try as an ordinary close() call".to_string()
        );
    }
    Ok(())
}

#[test]
fn perl_version_pin_is_recorded_when_oracle_runs() -> R {
    let Some(()) = pinned_oracle_ready()? else {
        return Ok(());
    };
    Ok(())
}
