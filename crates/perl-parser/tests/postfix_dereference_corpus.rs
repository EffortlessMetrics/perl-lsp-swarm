//! Public-parser and governed-corpus proof for the complete postfix-dereference
//! family (#13763).

use perl_parser::{Node, NodeKind, Parser};
use std::fs;
use std::path::{Path, PathBuf};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone, Copy)]
enum ExpectedShape<'a> {
    Unary { op: &'a str, receiver: &'a str },
    Binary { op: &'a str, receiver: &'a str, selector: &'a str },
    HashSlice { receiver: &'a str, selector: &'a str },
}

#[derive(Clone, Copy)]
struct MatrixCase<'a> {
    text: &'a str,
    shape: ExpectedShape<'a>,
}

const MATRIX: &[MatrixCase<'static>] = &[
    MatrixCase {
        text: "$sref->$*",
        shape: ExpectedShape::Unary { op: "->$*", receiver: "$sref" },
    },
    MatrixCase {
        text: "$aref->$#*",
        shape: ExpectedShape::Unary { op: "->$#*", receiver: "$aref" },
    },
    MatrixCase {
        text: "$aref->@*",
        shape: ExpectedShape::Unary { op: "->@*", receiver: "$aref" },
    },
    MatrixCase {
        text: "$aref->@[0, 2]",
        shape: ExpectedShape::Binary {
            op: "->@[]",
            receiver: "$aref",
            selector: "0, 2",
        },
    },
    MatrixCase {
        text: "$href->@{'alpha', 'beta'}",
        shape: ExpectedShape::HashSlice {
            receiver: "$href",
            selector: "'alpha', 'beta'",
        },
    },
    MatrixCase {
        text: "$href->%*",
        shape: ExpectedShape::Unary { op: "->%*", receiver: "$href" },
    },
    MatrixCase {
        text: "$href->%{qw(alpha beta)}",
        shape: ExpectedShape::Binary {
            op: "->%{}",
            receiver: "$href",
            selector: "qw(alpha beta)",
        },
    },
    MatrixCase {
        text: "$cref->&*",
        shape: ExpectedShape::Unary { op: "->&*", receiver: "$cref" },
    },
    MatrixCase {
        text: "$gref->**",
        shape: ExpectedShape::Unary { op: "->**", receiver: "$gref" },
    },
];

#[test]
fn project_fixture_is_discovered_and_emits_the_exact_postfix_matrix() -> TestResult {
    let workspace_root = fs::canonicalize(workspace_root())?;
    let fixture_path = workspace_root.join("test_corpus/postfix_dereference_matrix.pl");
    let source = fs::read_to_string(&fixture_path)?;
    let corpus_paths = perl_corpus::files::CorpusPaths::from_root(workspace_root);

    assert!(
        perl_corpus::files::get_test_files_from(&corpus_paths).contains(&fixture_path),
        "the exact checkout fixture must belong to the explicitly rooted project corpus"
    );

    let ast = parse_clean(&source)?;
    for case in MATRIX {
        let node = exact_node(&ast, &source, case.text)?;
        assert_shape(node, &source, case.shape)?;
        let expected_children = match case.shape {
            ExpectedShape::Unary { receiver, .. } => vec![receiver],
            ExpectedShape::Binary { receiver, selector, .. }
            | ExpectedShape::HashSlice { receiver, selector } => vec![receiver, selector],
        };
        let actual_children = node
            .children()
            .into_iter()
            .map(|child| node_source(child, &source))
            .collect::<Option<Vec<_>>>()
            .ok_or("a matrix child had an out-of-bounds source range")?;
        assert_eq!(actual_children, expected_children, "{} child traversal drifted", case.text);
    }

    assert_chained_unicode_hash_slice(&ast, &source)?;
    assert_slice_lvalues(&ast, &source)?;
    Ok(())
}

#[test]
fn matrix_geometry_is_byte_exact_under_crlf_source() -> TestResult {
    let fixture_path = workspace_root().join("test_corpus/postfix_dereference_matrix.pl");
    let source = fs::read_to_string(fixture_path)?.replace('\n', "\r\n");
    let ast = parse_clean(&source)?;

    for expected in ["$sref->$*", "$aref->$#*", "$href->@{'alpha', 'beta'}", "$gref->**"] {
        let node = exact_node(&ast, &source, expected)?;
        assert_eq!(node_source(node, &source), Some(expected));
    }
    Ok(())
}

fn parse_clean(source: &str) -> Result<Node, String> {
    let mut parser = Parser::new(source);
    let output = parser.parse_with_recovery();
    if !output.diagnostics.is_empty() {
        return Err(format!(
            "postfix corpus retained {} diagnostic(s): {:?}",
            output.diagnostics.len(),
            output.diagnostics
        ));
    }
    Ok(output.ast)
}

fn exact_node<'a>(ast: &'a Node, source: &str, expected: &str) -> Result<&'a Node, String> {
    let mut found = Vec::new();
    collect_exact(ast, source, expected, &mut found);
    if found.len() != 1 {
        return Err(format!(
            "expected one public AST node spanning {expected:?}, found {}\n{}",
            found.len(),
            ast.to_sexp()
        ));
    }
    found
        .into_iter()
        .next()
        .ok_or_else(|| format!("the unique node for {expected:?} was not retained"))
}

fn collect_exact<'a>(node: &'a Node, source: &str, expected: &str, found: &mut Vec<&'a Node>) {
    if node_source(node, source) == Some(expected) {
        found.push(node);
    }
    for child in node.children() {
        collect_exact(child, source, expected, found);
    }
}

fn assert_shape(node: &Node, source: &str, expected: ExpectedShape<'_>) -> Result<(), String> {
    match expected {
        ExpectedShape::Unary { op, receiver } => {
            let NodeKind::Unary { op: actual_op, operand } = &node.kind else {
                return Err(format!("expected Unary({op}), got {}", node.kind.kind_name()));
            };
            if actual_op != op || node_source(operand, source) != Some(receiver) {
                return Err(format!(
                    "unexpected Unary row: op={actual_op:?}, receiver={:?}",
                    node_source(operand, source)
                ));
            }
        }
        ExpectedShape::Binary { op, receiver, selector } => {
            let NodeKind::Binary { op: actual_op, left, right } = &node.kind else {
                return Err(format!("expected Binary({op}), got {}", node.kind.kind_name()));
            };
            if actual_op != op
                || node_source(left, source) != Some(receiver)
                || node_source(right, source) != Some(selector)
            {
                return Err(format!(
                    "unexpected Binary row: op={actual_op:?}, receiver={:?}, selector={:?}",
                    node_source(left, source),
                    node_source(right, source)
                ));
            }
        }
        ExpectedShape::HashSlice { receiver, selector } => {
            let NodeKind::HashSlice { target, keys } = &node.kind else {
                return Err(format!("expected HashSlice, got {}", node.kind.kind_name()));
            };
            if node_source(target, source) != Some(receiver)
                || node_source(keys, source) != Some(selector)
            {
                return Err(format!(
                    "unexpected HashSlice row: receiver={:?}, selector={:?}",
                    node_source(target, source),
                    node_source(keys, source)
                ));
            }
        }
    }
    Ok(())
}

fn assert_chained_unicode_hash_slice(ast: &Node, source: &str) -> Result<(), String> {
    let node = exact_node(ast, source, "$object->{payload}->@{'東京', 'alpha'}")?;
    let NodeKind::HashSlice { target, keys } = &node.kind else {
        return Err(format!("expected chained HashSlice, got {}", node.kind.kind_name()));
    };
    if !matches!(&target.kind, NodeKind::Binary { op, .. } if op == "->{}")
        || node_source(target, source) != Some("$object->{payload}")
        || node_source(keys, source) != Some("'東京', 'alpha'")
    {
        return Err("chained Unicode HashSlice geometry drifted".to_string());
    }
    Ok(())
}

fn assert_slice_lvalues(ast: &Node, source: &str) -> Result<(), String> {
    for expected_lhs in ["$aref->@[1, 3]", "$href->@{qw(alpha beta)}"] {
        let node = exact_node(ast, source, expected_lhs)?;
        let mut is_assignment_lhs = false;
        find_parent_assignment(ast, node, &mut is_assignment_lhs);
        if !is_assignment_lhs {
            return Err(format!("{expected_lhs} was not retained as an assignment lhs"));
        }
    }
    Ok(())
}

fn find_parent_assignment(node: &Node, expected_lhs: &Node, found: &mut bool) {
    if let NodeKind::Assignment { lhs, .. } = &node.kind
        && std::ptr::eq(lhs.as_ref(), expected_lhs)
    {
        *found = true;
    }
    for child in node.children() {
        if !*found {
            find_parent_assignment(child, expected_lhs, found);
        }
    }
}

fn node_source<'a>(node: &Node, source: &'a str) -> Option<&'a str> {
    source.get(node.location.start..node.location.end)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}
