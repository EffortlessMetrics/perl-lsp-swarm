use perl_ast::{Node, NodeKind, SourceLocation};

fn loc() -> SourceLocation {
    SourceLocation::new(0, 0)
}

fn leaf(name: &str) -> Node {
    Node::new(NodeKind::Identifier { name: name.to_string() }, loc())
}

fn build_cases() -> Vec<(Node, &'static str, usize)> {
    vec![
        (Node::new(NodeKind::Program { statements: vec![leaf("stmt")] }, loc()), "Program", 1),
        (
            Node::new(NodeKind::ExpressionStatement { expression: Box::new(leaf("expr")) }, loc()),
            "ExpressionStatement",
            1,
        ),
        (
            Node::new(
                NodeKind::VariableDeclaration {
                    declarator: "my".to_string(),
                    variable: Box::new(leaf("var")),
                    attributes: vec!["shared".to_string()],
                    initializer: Some(Box::new(leaf("init"))),
                },
                loc(),
            ),
            "VariableDeclaration",
            2,
        ),
        (
            Node::new(
                NodeKind::VariableListDeclaration {
                    declarator: "my".to_string(),
                    variables: vec![leaf("a"), leaf("b")],
                    attributes: vec!["locked".to_string()],
                    initializer: Some(Box::new(leaf("init"))),
                },
                loc(),
            ),
            "VariableListDeclaration",
            3,
        ),
        (
            Node::new(NodeKind::Variable { sigil: "$".to_string(), name: "x".to_string() }, loc()),
            "Variable",
            0,
        ),
        (
            Node::new(
                NodeKind::VariableWithAttributes {
                    variable: Box::new(leaf("attr_var")),
                    attributes: vec!["shared".to_string()],
                },
                loc(),
            ),
            "VariableWithAttributes",
            1,
        ),
        (
            Node::new(
                NodeKind::Assignment {
                    lhs: Box::new(leaf("lhs")),
                    rhs: Box::new(leaf("rhs")),
                    op: "=".to_string(),
                },
                loc(),
            ),
            "Assignment",
            2,
        ),
        (
            Node::new(
                NodeKind::Binary {
                    op: "+".to_string(),
                    left: Box::new(leaf("left")),
                    right: Box::new(leaf("right")),
                },
                loc(),
            ),
            "Binary",
            2,
        ),
        (
            Node::new(
                NodeKind::Ternary {
                    condition: Box::new(leaf("cond")),
                    then_expr: Box::new(leaf("then")),
                    else_expr: Box::new(leaf("else")),
                },
                loc(),
            ),
            "Ternary",
            3,
        ),
        (
            Node::new(
                NodeKind::Unary { op: "!".to_string(), operand: Box::new(leaf("operand")) },
                loc(),
            ),
            "Unary",
            1,
        ),
        (Node::new(NodeKind::Diamond, loc()), "Diamond", 0),
        (Node::new(NodeKind::Ellipsis, loc()), "Ellipsis", 0),
        (Node::new(NodeKind::Undef, loc()), "Undef", 0),
        (
            Node::new(NodeKind::Readline { filehandle: Some("STDIN".to_string()) }, loc()),
            "Readline",
            0,
        ),
        (Node::new(NodeKind::Glob { pattern: "*.pl".to_string() }, loc()), "Glob", 0),
        (Node::new(NodeKind::Typeglob { name: "main::foo".to_string() }, loc()), "Typeglob", 0),
        (Node::new(NodeKind::Number { value: "42".to_string() }, loc()), "Number", 0),
        (
            Node::new(NodeKind::String { value: "value".to_string(), interpolated: true }, loc()),
            "String",
            0,
        ),
        (
            Node::new(
                NodeKind::Heredoc {
                    delimiter: "EOF".to_string(),
                    content: "content".to_string(),
                    interpolated: false,
                    indented: false,
                    command: false,
                    body_span: Some(loc()),
                },
                loc(),
            ),
            "Heredoc",
            0,
        ),
        (
            Node::new(NodeKind::ArrayLiteral { elements: vec![leaf("one"), leaf("two")] }, loc()),
            "ArrayLiteral",
            2,
        ),
        (
            Node::new(NodeKind::HashLiteral { pairs: vec![(leaf("key"), leaf("value"))] }, loc()),
            "HashLiteral",
            2,
        ),
        (Node::new(NodeKind::Block { statements: vec![leaf("stmt")] }, loc()), "Block", 1),
        (Node::new(NodeKind::Eval { block: Box::new(leaf("block")) }, loc()), "Eval", 1),
        (Node::new(NodeKind::Do { block: Box::new(leaf("block")) }, loc()), "Do", 1),
        (Node::new(NodeKind::Defer { block: Box::new(leaf("block")) }, loc()), "Defer", 1),
        (
            Node::new(
                NodeKind::Try {
                    body: Box::new(leaf("body")),
                    catch_blocks: vec![(Some("$err".to_string()), Box::new(leaf("catch")))],
                    finally_block: Some(Box::new(leaf("finally"))),
                },
                loc(),
            ),
            "Try",
            3,
        ),
        (
            Node::new(
                NodeKind::If {
                    condition: Box::new(leaf("cond")),
                    then_branch: Box::new(leaf("then")),
                    elsif_branches: vec![(
                        Box::new(leaf("elsif_cond")),
                        Box::new(leaf("elsif_body")),
                    )],
                    else_branch: Some(Box::new(leaf("else"))),
                },
                loc(),
            ),
            "If",
            5,
        ),
        (
            Node::new(
                NodeKind::LabeledStatement {
                    label: "OUTER".to_string(),
                    statement: Box::new(leaf("stmt")),
                },
                loc(),
            ),
            "LabeledStatement",
            1,
        ),
        (
            Node::new(
                NodeKind::While {
                    condition: Box::new(leaf("cond")),
                    body: Box::new(leaf("body")),
                    continue_block: Some(Box::new(leaf("continue"))),
                },
                loc(),
            ),
            "While",
            3,
        ),
        (
            Node::new(
                NodeKind::Tie {
                    variable: Box::new(leaf("var")),
                    package: Box::new(leaf("pkg")),
                    args: vec![leaf("arg")],
                },
                loc(),
            ),
            "Tie",
            3,
        ),
        (Node::new(NodeKind::Untie { variable: Box::new(leaf("var")) }, loc()), "Untie", 1),
        (
            Node::new(
                NodeKind::For {
                    init: Some(Box::new(leaf("init"))),
                    condition: Some(Box::new(leaf("cond"))),
                    update: Some(Box::new(leaf("update"))),
                    body: Box::new(leaf("body")),
                    continue_block: Some(Box::new(leaf("continue"))),
                },
                loc(),
            ),
            "For",
            5,
        ),
        (
            Node::new(
                NodeKind::Foreach {
                    variable: Box::new(leaf("var")),
                    list: Box::new(leaf("list")),
                    body: Box::new(leaf("body")),
                    continue_block: Some(Box::new(leaf("continue"))),
                },
                loc(),
            ),
            "Foreach",
            4,
        ),
        (
            Node::new(
                NodeKind::Given { expr: Box::new(leaf("expr")), body: Box::new(leaf("body")) },
                loc(),
            ),
            "Given",
            2,
        ),
        (
            Node::new(
                NodeKind::When { condition: Box::new(leaf("cond")), body: Box::new(leaf("body")) },
                loc(),
            ),
            "When",
            2,
        ),
        (Node::new(NodeKind::Default { body: Box::new(leaf("body")) }, loc()), "Default", 1),
        (
            Node::new(
                NodeKind::StatementModifier {
                    statement: Box::new(leaf("stmt")),
                    modifier: "if".to_string(),
                    condition: Box::new(leaf("cond")),
                },
                loc(),
            ),
            "StatementModifier",
            2,
        ),
        (
            Node::new(
                NodeKind::Subroutine {
                    name: Some("helper".to_string()),
                    name_span: Some(loc()),
                    prototype: Some(Box::new(Node::new(
                        NodeKind::Prototype { content: "$".to_string() },
                        loc(),
                    ))),
                    signature: Some(Box::new(Node::new(
                        NodeKind::Signature {
                            parameters: vec![Node::new(
                                NodeKind::MandatoryParameter { variable: Box::new(leaf("param")) },
                                loc(),
                            )],
                        },
                        loc(),
                    ))),
                    attributes: vec!["lvalue".to_string()],
                    body: Box::new(leaf("body")),
                },
                loc(),
            ),
            "Subroutine",
            3,
        ),
        (Node::new(NodeKind::Prototype { content: "$$".to_string() }, loc()), "Prototype", 0),
        (
            Node::new(
                NodeKind::Signature {
                    parameters: vec![Node::new(
                        NodeKind::MandatoryParameter { variable: Box::new(leaf("param")) },
                        loc(),
                    )],
                },
                loc(),
            ),
            "Signature",
            1,
        ),
        (
            Node::new(NodeKind::MandatoryParameter { variable: Box::new(leaf("var")) }, loc()),
            "MandatoryParameter",
            1,
        ),
        (
            Node::new(
                NodeKind::OptionalParameter {
                    variable: Box::new(leaf("var")),
                    default_value: Box::new(leaf("default")),
                },
                loc(),
            ),
            "OptionalParameter",
            2,
        ),
        (
            Node::new(NodeKind::SlurpyParameter { variable: Box::new(leaf("var")) }, loc()),
            "SlurpyParameter",
            1,
        ),
        (
            Node::new(NodeKind::NamedParameter { variable: Box::new(leaf("var")) }, loc()),
            "NamedParameter",
            1,
        ),
        (
            Node::new(
                NodeKind::Method {
                    name: "run".to_string(),
                    signature: Some(Box::new(Node::new(
                        NodeKind::Signature { parameters: vec![] },
                        loc(),
                    ))),
                    attributes: vec!["method".to_string()],
                    body: Box::new(leaf("body")),
                },
                loc(),
            ),
            "Method",
            2,
        ),
        (Node::new(NodeKind::Return { value: Some(Box::new(leaf("value"))) }, loc()), "Return", 1),
        (
            Node::new(
                NodeKind::LoopControl { op: "next".to_string(), label: Some("OUTER".to_string()) },
                loc(),
            ),
            "LoopControl",
            0,
        ),
        (Node::new(NodeKind::Goto { target: Box::new(leaf("target")) }, loc()), "Goto", 1),
        (
            Node::new(
                NodeKind::MethodCall {
                    object: Box::new(leaf("object")),
                    method: "call".to_string(),
                    args: vec![leaf("arg")],
                },
                loc(),
            ),
            "MethodCall",
            2,
        ),
        (
            Node::new(
                NodeKind::FunctionCall { name: "say".to_string(), args: vec![leaf("arg")] },
                loc(),
            ),
            "FunctionCall",
            1,
        ),
        (
            Node::new(
                NodeKind::IndirectCall {
                    method: "new".to_string(),
                    object: Box::new(leaf("object")),
                    args: vec![leaf("arg")],
                },
                loc(),
            ),
            "IndirectCall",
            2,
        ),
        (
            Node::new(
                NodeKind::Regex {
                    pattern: "foo".to_string(),
                    replacement: Some("bar".to_string()),
                    modifiers: "g".to_string(),
                    has_embedded_code: false,
                },
                loc(),
            ),
            "Regex",
            0,
        ),
        (
            Node::new(
                NodeKind::Match {
                    expr: Box::new(leaf("expr")),
                    pattern: "foo".to_string(),
                    modifiers: "i".to_string(),
                    has_embedded_code: false,
                    negated: false,
                },
                loc(),
            ),
            "Match",
            1,
        ),
        (
            Node::new(
                NodeKind::Substitution {
                    expr: Box::new(leaf("expr")),
                    pattern: "foo".to_string(),
                    replacement: "bar".to_string(),
                    modifiers: "g".to_string(),
                    has_embedded_code: false,
                    negated: false,
                },
                loc(),
            ),
            "Substitution",
            1,
        ),
        (
            Node::new(
                NodeKind::Transliteration {
                    expr: Box::new(leaf("expr")),
                    search: "abc".to_string(),
                    replace: "xyz".to_string(),
                    modifiers: "d".to_string(),
                    negated: false,
                },
                loc(),
            ),
            "Transliteration",
            1,
        ),
        (
            Node::new(
                NodeKind::Package {
                    name: "Main".to_string(),
                    name_span: loc(),
                    block: Some(Box::new(leaf("body"))),
                },
                loc(),
            ),
            "Package",
            1,
        ),
        (
            Node::new(
                NodeKind::Use {
                    module: "strict".to_string(),
                    args: vec!["vars".to_string()],
                    has_filter_risk: false,
                },
                loc(),
            ),
            "Use",
            0,
        ),
        (
            Node::new(
                NodeKind::No {
                    module: "warnings".to_string(),
                    args: vec!["once".to_string()],
                    has_filter_risk: false,
                },
                loc(),
            ),
            "No",
            0,
        ),
        (
            Node::new(
                NodeKind::PhaseBlock {
                    phase: "BEGIN".to_string(),
                    phase_span: Some(loc()),
                    block: Box::new(leaf("body")),
                },
                loc(),
            ),
            "PhaseBlock",
            1,
        ),
        (
            Node::new(
                NodeKind::DataSection {
                    marker: "__DATA__".to_string(),
                    body: Some("payload".to_string()),
                },
                loc(),
            ),
            "DataSection",
            0,
        ),
        (
            Node::new(
                NodeKind::Class {
                    name: "Example".to_string(),
                    parents: vec![],
                    body: Box::new(leaf("body")),
                },
                loc(),
            ),
            "Class",
            1,
        ),
        (
            Node::new(
                NodeKind::Format { name: "STDOUT".to_string(), body: "body".to_string() },
                loc(),
            ),
            "Format",
            0,
        ),
        (Node::new(NodeKind::Identifier { name: "name".to_string() }, loc()), "Identifier", 0),
        (
            Node::new(
                NodeKind::Error {
                    message: "broken".to_string(),
                    expected: vec![],
                    found: None,
                    partial: Some(Box::new(leaf("partial"))),
                },
                loc(),
            ),
            "Error",
            1,
        ),
        (Node::new(NodeKind::MissingExpression, loc()), "MissingExpression", 0),
        (Node::new(NodeKind::MissingStatement, loc()), "MissingStatement", 0),
        (Node::new(NodeKind::MissingIdentifier, loc()), "MissingIdentifier", 0),
        (Node::new(NodeKind::MissingBlock, loc()), "MissingBlock", 0),
        (Node::new(NodeKind::UnknownRest, loc()), "UnknownRest", 0),
    ]
}

#[test]
fn nodekind_cases_all_have_display_and_sexp_receipts() {
    for (node, expected_name, _) in build_cases() {
        assert_eq!(node.kind.to_string(), expected_name, "Display drifted for {expected_name}");

        let sexp = node.to_sexp();
        assert!(!sexp.is_empty(), "{expected_name} produced an empty S-expression");
        assert!(sexp.starts_with('('), "{expected_name} S-expression must start with '(': {sexp}");
        assert!(sexp.ends_with(')'), "{expected_name} S-expression must end with ')': {sexp}");

        let open_parens = sexp.chars().filter(|ch| *ch == '(').count();
        let close_parens = sexp.chars().filter(|ch| *ch == ')').count();
        assert_eq!(
            open_parens, close_parens,
            "{expected_name} S-expression has unbalanced parentheses: {sexp}"
        );
    }
}

#[test]
fn nodekind_children_helpers_agree_for_every_case() {
    for (node, expected_name, expected_children) in build_cases() {
        let children = node.children();
        assert_eq!(
            node.child_count(),
            expected_children,
            "child_count drifted for {expected_name}"
        );
        assert_eq!(
            children.len(),
            node.child_count(),
            "children() and child_count() disagree for {expected_name}"
        );
        assert!(
            node.count_nodes() > expected_children,
            "count_nodes should include {expected_name} and at least its direct children"
        );

        let first_from_vec = children.first().copied().map(|child| child.kind.kind_name());
        let first_from_helper = node.first_child().map(|child| child.kind.kind_name());
        assert_eq!(
            first_from_helper, first_from_vec,
            "first_child disagrees with children().first() for {expected_name}"
        );

        let last_from_vec = children.last().copied().map(|child| child.kind.kind_name());
        let last_from_helper = node.last_child().map(|child| child.kind.kind_name());
        assert_eq!(
            last_from_helper, last_from_vec,
            "last_child disagrees with children().last() for {expected_name}"
        );
    }
}

fn optional_absence_cases() -> Vec<(Node, &'static str, usize, &'static str)> {
    vec![
        (
            Node::new(
                NodeKind::VariableDeclaration {
                    declarator: "my".to_string(),
                    variable: Box::new(leaf("var")),
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
            "VariableDeclaration",
            1,
            "(my_declaration (identifier var))",
        ),
        (
            Node::new(
                NodeKind::VariableListDeclaration {
                    declarator: "my".to_string(),
                    variables: vec![leaf("a"), leaf("b")],
                    attributes: vec![],
                    initializer: None,
                },
                loc(),
            ),
            "VariableListDeclaration",
            2,
            "(my_declaration ((identifier a) (identifier b)))",
        ),
        (
            Node::new(
                NodeKind::Try {
                    body: Box::new(leaf("body")),
                    catch_blocks: vec![],
                    finally_block: None,
                },
                loc(),
            ),
            "Try",
            1,
            "(try (identifier body))",
        ),
        (
            Node::new(
                NodeKind::If {
                    condition: Box::new(leaf("cond")),
                    then_branch: Box::new(leaf("then")),
                    elsif_branches: vec![],
                    else_branch: None,
                },
                loc(),
            ),
            "If",
            2,
            "(if (identifier cond) (identifier then))",
        ),
        (
            Node::new(
                NodeKind::While {
                    condition: Box::new(leaf("cond")),
                    body: Box::new(leaf("body")),
                    continue_block: None,
                },
                loc(),
            ),
            "While",
            2,
            "(while (identifier cond) (identifier body))",
        ),
        (
            Node::new(
                NodeKind::For {
                    init: None,
                    condition: None,
                    update: None,
                    body: Box::new(leaf("body")),
                    continue_block: None,
                },
                loc(),
            ),
            "For",
            1,
            "(for () () () (identifier body))",
        ),
        (
            Node::new(
                NodeKind::Foreach {
                    variable: Box::new(leaf("var")),
                    list: Box::new(leaf("list")),
                    body: Box::new(leaf("body")),
                    continue_block: None,
                },
                loc(),
            ),
            "Foreach",
            3,
            "(foreach (identifier var) (identifier list) (identifier body))",
        ),
        (
            Node::new(
                NodeKind::Subroutine {
                    name: Some("demo".to_string()),
                    name_span: None,
                    prototype: None,
                    signature: None,
                    attributes: vec![],
                    body: Box::new(leaf("body")),
                },
                loc(),
            ),
            "Subroutine",
            1,
            "(sub demo ()(identifier body))",
        ),
        (
            Node::new(
                NodeKind::Method {
                    name: "demo".to_string(),
                    signature: None,
                    attributes: vec![],
                    body: Box::new(leaf("body")),
                },
                loc(),
            ),
            "Method",
            1,
            "(method_declaration_statement (bareword) (block (identifier body)))",
        ),
        (Node::new(NodeKind::Return { value: None }, loc()), "Return", 0, "(return)"),
        (
            Node::new(
                NodeKind::Package { name: "Main".to_string(), name_span: loc(), block: None },
                loc(),
            ),
            "Package",
            0,
            "(package Main)",
        ),
        (
            Node::new(
                NodeKind::Error {
                    message: "broken".to_string(),
                    expected: vec![],
                    found: None,
                    partial: None,
                },
                loc(),
            ),
            "Error",
            0,
            "(ERROR \"broken\")",
        ),
        (Node::new(NodeKind::Readline { filehandle: None }, loc()), "Readline", 0, "(readline)"),
        (
            Node::new(
                NodeKind::Use { module: "strict".to_string(), args: vec![], has_filter_risk: true },
                loc(),
            ),
            "Use",
            0,
            "(use strict (risk:filter))",
        ),
        (
            Node::new(
                NodeKind::No {
                    module: "warnings".to_string(),
                    args: vec![],
                    has_filter_risk: true,
                },
                loc(),
            ),
            "No",
            0,
            "(no warnings (risk:filter))",
        ),
        (
            Node::new(NodeKind::DataSection { marker: "__END__".to_string(), body: None }, loc()),
            "DataSection",
            0,
            "(data_section __END__)",
        ),
    ]
}

#[test]
fn nodekind_optional_absence_paths_preserve_child_counts_and_sexp() {
    for (node, expected_name, expected_children, expected_sexp) in optional_absence_cases() {
        assert_eq!(node.kind.kind_name(), expected_name);
        assert_eq!(
            node.children().len(),
            expected_children,
            "optional absence child count drifted for {expected_name}"
        );
        assert_eq!(
            node.to_sexp(),
            expected_sexp,
            "optional absence S-expression drifted for {expected_name}"
        );
    }
}

#[test]
fn nodekind_cases_match_all_kind_names_set() {
    let mut case_names =
        build_cases().into_iter().map(|(_, expected_name, _)| expected_name).collect::<Vec<_>>();
    case_names.sort_unstable();

    let mut canonical = NodeKind::ALL_KIND_NAMES.to_vec();
    canonical.sort_unstable();

    assert_eq!(case_names, canonical);
}

#[test]
fn nodekind_cases_cover_kind_names_and_child_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let cases = build_cases();
    assert_eq!(cases.len(), NodeKind::ALL_KIND_NAMES.len());

    for (node, expected_name, expected_children) in cases {
        assert_eq!(node.kind.kind_name(), expected_name);
        assert_eq!(
            node.children().len(),
            expected_children,
            "unexpected children for {expected_name}"
        );

        match expected_children {
            0 => assert!(
                node.first_child().is_none(),
                "{expected_name} unexpectedly had a first child"
            ),
            _ => {
                assert!(node.first_child().is_some(), "{expected_name} should expose a first child")
            }
        }
    }

    Ok(())
}
