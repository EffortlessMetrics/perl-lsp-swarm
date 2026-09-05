//! Representative cardinality and grammar-name witnesses for check-mode parity.
//!
//! Fully populated samples come from [`crate::node_kind_fixtures`]. This module
//! adds the extra forms that expose absent optional fields, empty repeated
//! fields, and grammar-name input pairs. It does not replace production
//! constructors.

use super::GrammarInputWitness;
use crate::{Node, NodeKind, SourceLocation};

fn loc() -> SourceLocation {
    SourceLocation { start: 0, end: 0 }
}

fn dummy() -> Node {
    Node::new(NodeKind::Undef, loc())
}

fn boxed() -> Box<Node> {
    Box::new(dummy())
}

fn text() -> String {
    "fixture".to_string()
}

/// Extra representatives that expose optional-absent and repeated-empty forms.
#[must_use]
pub fn cardinality_forms() -> Vec<Node> {
    vec![
        Node::new(NodeKind::Program { statements: vec![] }, loc()),
        Node::new(
            NodeKind::VariableDeclaration {
                declarator: text(),
                variable: boxed(),
                attributes: vec![],
                initializer: None,
            },
            loc(),
        ),
        Node::new(
            NodeKind::VariableListDeclaration {
                declarator: text(),
                variables: vec![],
                attributes: vec![],
                initializer: None,
            },
            loc(),
        ),
        Node::new(NodeKind::NestedVariableList { items: vec![] }, loc()),
        Node::new(NodeKind::ChainedComparison { operands: vec![], ops: vec![] }, loc()),
        Node::new(NodeKind::ArrayLiteral { elements: vec![] }, loc()),
        Node::new(NodeKind::HashLiteral { pairs: vec![] }, loc()),
        Node::new(NodeKind::Block { statements: vec![] }, loc()),
        Node::new(
            NodeKind::Try { body: boxed(), catch_blocks: vec![], finally_block: None },
            loc(),
        ),
        Node::new(
            NodeKind::If {
                condition: boxed(),
                then_branch: boxed(),
                elsif_branches: vec![],
                else_branch: None,
                keyword: None,
            },
            loc(),
        ),
        Node::new(
            NodeKind::While {
                condition: boxed(),
                body: boxed(),
                continue_block: None,
                keyword: None,
            },
            loc(),
        ),
        Node::new(NodeKind::Tie { variable: boxed(), package: boxed(), args: vec![] }, loc()),
        Node::new(
            NodeKind::For {
                init: None,
                condition: None,
                update: None,
                body: boxed(),
                continue_block: None,
            },
            loc(),
        ),
        Node::new(
            NodeKind::Foreach {
                variable: boxed(),
                list: boxed(),
                body: boxed(),
                continue_block: None,
            },
            loc(),
        ),
        Node::new(
            NodeKind::Subroutine {
                name: Some(text()),
                name_span: None,
                declarator: None,
                prototype: None,
                signature: None,
                attributes: vec![],
                body: boxed(),
            },
            loc(),
        ),
        Node::new(NodeKind::Signature { parameters: vec![] }, loc()),
        Node::new(
            NodeKind::NamedParameter {
                variable: boxed(),
                external_name: text(),
                default_operator: None,
                default_value: None,
                required: true,
            },
            loc(),
        ),
        Node::new(
            NodeKind::Method {
                name: text(),
                name_span: None,
                signature: None,
                attributes: vec![],
                body: boxed(),
            },
            loc(),
        ),
        Node::new(NodeKind::Return { value: None }, loc()),
        Node::new(NodeKind::MethodCall { object: boxed(), method: text(), args: vec![] }, loc()),
        Node::new(NodeKind::IndirectCall { method: text(), object: boxed(), args: vec![] }, loc()),
        Node::new(NodeKind::Package { name: text(), name_span: loc(), block: None }, loc()),
        Node::new(
            NodeKind::Error { message: text(), expected: vec![], found: None, partial: None },
            loc(),
        ),
    ]
}

/// Pairs that differ by one declared runtime grammar-name input.
#[must_use]
pub fn grammar_input_witnesses() -> Vec<GrammarInputWitness> {
    vec![
        GrammarInputWitness {
            kind_name: "VariableDeclaration",
            input: "declarator",
            left: Node::new(
                NodeKind::VariableDeclaration {
                    declarator: "my".to_string(),
                    variable: boxed(),
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
            right: Node::new(
                NodeKind::VariableDeclaration {
                    declarator: "our".to_string(),
                    variable: boxed(),
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "VariableListDeclaration",
            input: "declarator",
            left: Node::new(
                NodeKind::VariableListDeclaration {
                    declarator: "my".to_string(),
                    variables: vec![],
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
            right: Node::new(
                NodeKind::VariableListDeclaration {
                    declarator: "our".to_string(),
                    variables: vec![],
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "Assignment",
            input: "op",
            left: Node::new(
                NodeKind::Assignment { lhs: boxed(), rhs: boxed(), op: "=".to_string() },
                loc(),
            ),
            right: Node::new(
                NodeKind::Assignment { lhs: boxed(), rhs: boxed(), op: "+=".to_string() },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "Binary",
            input: "op",
            left: Node::new(
                NodeKind::Binary { op: "+".to_string(), left: boxed(), right: boxed() },
                loc(),
            ),
            right: Node::new(
                NodeKind::Binary { op: "-".to_string(), left: boxed(), right: boxed() },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "Unary",
            input: "op",
            left: Node::new(NodeKind::Unary { op: "-".to_string(), operand: boxed() }, loc()),
            right: Node::new(NodeKind::Unary { op: "!".to_string(), operand: boxed() }, loc()),
        },
        GrammarInputWitness {
            kind_name: "String",
            input: "interpolated",
            left: Node::new(NodeKind::String { value: text(), interpolated: false }, loc()),
            right: Node::new(NodeKind::String { value: text(), interpolated: true }, loc()),
        },
        GrammarInputWitness {
            kind_name: "Heredoc",
            input: "interpolated",
            left: heredoc(false, false, false),
            right: heredoc(true, false, false),
        },
        GrammarInputWitness {
            kind_name: "Heredoc",
            input: "indented",
            left: heredoc(false, false, false),
            right: heredoc(false, true, false),
        },
        GrammarInputWitness {
            kind_name: "Heredoc",
            input: "command",
            left: heredoc(false, false, false),
            right: heredoc(false, false, true),
        },
        GrammarInputWitness {
            kind_name: "If",
            input: "keyword",
            left: if_form(None),
            right: if_form(Some("unless".to_string())),
        },
        GrammarInputWitness {
            kind_name: "While",
            input: "keyword",
            left: while_form(None),
            right: while_form(Some("until".to_string())),
        },
        GrammarInputWitness {
            kind_name: "StatementModifier",
            input: "modifier",
            left: Node::new(
                NodeKind::StatementModifier {
                    statement: boxed(),
                    modifier: "if".to_string(),
                    condition: boxed(),
                },
                loc(),
            ),
            right: Node::new(
                NodeKind::StatementModifier {
                    statement: boxed(),
                    modifier: "unless".to_string(),
                    condition: boxed(),
                },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "Subroutine",
            input: "name",
            left: subroutine(Some(text())),
            right: subroutine(None),
        },
        GrammarInputWitness {
            kind_name: "LoopControl",
            input: "op",
            left: Node::new(NodeKind::LoopControl { op: "next".to_string(), label: None }, loc()),
            right: Node::new(NodeKind::LoopControl { op: "last".to_string(), label: None }, loc()),
        },
        GrammarInputWitness {
            kind_name: "FunctionCall",
            input: "name",
            left: Node::new(
                NodeKind::FunctionCall { name: "print".to_string(), args: vec![dummy()] },
                loc(),
            ),
            right: Node::new(
                NodeKind::FunctionCall { name: "foo".to_string(), args: vec![dummy()] },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "FunctionCall",
            input: "args",
            left: Node::new(
                NodeKind::FunctionCall { name: "foo".to_string(), args: vec![] },
                loc(),
            ),
            right: Node::new(
                NodeKind::FunctionCall { name: "foo".to_string(), args: vec![dummy()] },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "AmperCall",
            input: "args",
            left: Node::new(NodeKind::AmperCall { name: "foo".to_string(), args: vec![] }, loc()),
            right: Node::new(
                NodeKind::AmperCall { name: "foo".to_string(), args: vec![dummy()] },
                loc(),
            ),
        },
        GrammarInputWitness {
            kind_name: "Match",
            input: "negated",
            left: match_form(false),
            right: match_form(true),
        },
        GrammarInputWitness {
            kind_name: "PhaseBlock",
            input: "phase",
            left: Node::new(
                NodeKind::PhaseBlock {
                    phase: "BEGIN".to_string(),
                    phase_span: None,
                    block: boxed(),
                },
                loc(),
            ),
            right: Node::new(
                NodeKind::PhaseBlock { phase: "END".to_string(), phase_span: None, block: boxed() },
                loc(),
            ),
        },
    ]
}

fn heredoc(interpolated: bool, indented: bool, command: bool) -> Node {
    Node::new(
        NodeKind::Heredoc {
            delimiter: text(),
            content: text(),
            interpolated,
            indented,
            command,
            body_span: None,
        },
        loc(),
    )
}

fn if_form(keyword: Option<String>) -> Node {
    Node::new(
        NodeKind::If {
            condition: boxed(),
            then_branch: boxed(),
            elsif_branches: vec![],
            else_branch: None,
            keyword,
        },
        loc(),
    )
}

fn while_form(keyword: Option<String>) -> Node {
    Node::new(
        NodeKind::While { condition: boxed(), body: boxed(), continue_block: None, keyword },
        loc(),
    )
}

fn subroutine(name: Option<String>) -> Node {
    Node::new(
        NodeKind::Subroutine {
            name,
            name_span: None,
            declarator: None,
            prototype: None,
            signature: None,
            attributes: vec![],
            body: boxed(),
        },
        loc(),
    )
}

fn match_form(negated: bool) -> Node {
    Node::new(
        NodeKind::Match {
            expr: boxed(),
            pattern: text(),
            modifiers: String::new(),
            has_embedded_code: false,
            negated,
        },
        loc(),
    )
}
