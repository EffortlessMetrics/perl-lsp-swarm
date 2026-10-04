//! Pin writer fragments as well as bytes: the analytical reuse hash consumes
//! each `write_str` fragment separately, so token boundaries are observable.

use perl_ast::{
    NativeDebugSexpLimits, NativeDebugSexpOmitted, NativeDebugSexpResult,
    NativeDebugSexpTruncation, NativeDebugSexpWork, Node, NodeKind, SourceLocation,
};
use std::fmt;

#[derive(Default)]
struct FragmentWriter(Vec<String>);

impl fmt::Write for FragmentWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.0.push(value.to_owned());
        Ok(())
    }
}

fn node(kind: NodeKind) -> Node {
    Node::new(kind, SourceLocation { start: 0, end: 1 })
}

#[test]
fn unquoted_atoms_keep_one_writer_fragment_per_token() {
    let mut out = FragmentWriter::default();
    let result = node(NodeKind::Number { value: "42".into() })
        .render_debug_sexp(&mut out, NativeDebugSexpLimits::unbounded());
    assert_eq!(out.0, ["(", "number", " ", "(", "value", " ", "42", ")", ")"]);
    assert_eq!(
        result,
        NativeDebugSexpResult::Complete {
            work: NativeDebugSexpWork {
                nodes_visited: 1,
                child_edges_visited: 0,
                max_depth: 0,
                bytes_written: "(number (value 42))".len(),
                work_units: 3,
            },
        }
    );
}

#[test]
fn escaped_atom_stays_in_one_fragment_and_byte_limit_rejects_it_whole() {
    let subject = node(NodeKind::Identifier { name: "a b\n".into() });
    let mut out = FragmentWriter::default();
    let result = subject.render_debug_sexp(&mut out, NativeDebugSexpLimits::unbounded());
    assert_eq!(out.0, ["(", "identifier", " ", "(", "name", " ", "\"a b\\n\"", ")", ")"]);
    assert!(matches!(result, NativeDebugSexpResult::Complete { .. }));

    let prefix = "(identifier (name ";
    let mut out = FragmentWriter::default();
    let result = subject.render_debug_sexp(
        &mut out,
        NativeDebugSexpLimits { max_bytes: Some(prefix.len() + 1), ..Default::default() },
    );
    assert_eq!(out.0, ["(", "identifier", " ", "(", "name", " "]);
    assert_eq!(
        result,
        NativeDebugSexpResult::Truncated {
            reason: NativeDebugSexpTruncation::ByteLimit { limit: prefix.len() + 1 },
            work: NativeDebugSexpWork {
                nodes_visited: 1,
                child_edges_visited: 0,
                max_depth: 0,
                bytes_written: prefix.len(),
                work_units: 2,
            },
            omitted: NativeDebugSexpOmitted::Unknown,
        }
    );
}

#[test]
fn depth_rejection_preserves_node_limit_precedence_and_payload_work() {
    let subject = node(NodeKind::Program {
        statements: (0..1024).map(|_| node(NodeKind::Number { value: "1".into() })).collect(),
    });
    for max_nodes in [None, Some(1)] {
        let mut out = FragmentWriter::default();
        let result = subject.render_debug_sexp(
            &mut out,
            NativeDebugSexpLimits {
                max_nodes,
                max_depth: Some(0),
                max_work: Some(2),
                ..Default::default()
            },
        );
        assert_eq!(out.0, ["(", "source_file"]);
        assert_eq!(
            result,
            NativeDebugSexpResult::Truncated {
                reason: if max_nodes.is_some() {
                    NativeDebugSexpTruncation::NodeLimit { limit: 1 }
                } else {
                    NativeDebugSexpTruncation::DepthLimit { limit: 0 }
                },
                work: NativeDebugSexpWork {
                    nodes_visited: 1,
                    child_edges_visited: 0,
                    max_depth: 0,
                    bytes_written: "(source_file".len(),
                    work_units: 2,
                },
                omitted: NativeDebugSexpOmitted::Unknown,
            }
        );
    }
}
