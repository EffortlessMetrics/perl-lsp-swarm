#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{
    CompileTimePragmaEnvironment, ImportIntoSource, ImportIntoTarget, find_import_into_calls,
};

fn node(kind: NodeKind, start: usize, end: usize) -> Node {
    Node::new(
        NodeKind::ExpressionStatement {
            expression: Box::new(Node::new(kind, SourceLocation::new(start, end))),
        },
        SourceLocation::new(start, end),
    )
}

fn identifier(name: &str, start: usize) -> Node {
    Node::new(
        NodeKind::Identifier { name: name.into() },
        SourceLocation::new(start, start + name.len()),
    )
}

#[test]
fn recognizes_static_pragma_import_into_and_caller_depth() {
    let call = Node::new(
        NodeKind::MethodCall {
            object: Box::new(identifier("strict", 0)),
            method: "import::into".into(),
            args: vec![Node::new(
                NodeKind::Number { value: "1".into() },
                SourceLocation::new(20, 21),
            )],
        },
        SourceLocation::new(0, 22),
    );
    let ast = Node::new(NodeKind::Program { statements: vec![call] }, SourceLocation::new(0, 22));

    assert_eq!(
        find_import_into_calls(&ast),
        vec![perl_pragma::ImportIntoCall {
            range: 0..22,
            source: ImportIntoSource::Package("strict".into()),
            target: ImportIntoTarget::CallerDepth(1),
        }]
    );
}

#[test]
fn preserves_dynamic_source_and_target_boundaries() {
    let call = node(
        NodeKind::MethodCall {
            object: Box::new(identifier("$module", 0)),
            method: "import::into".into(),
            args: vec![identifier("caller", 20)],
        },
        0,
        26,
    );
    let ast = Node::new(NodeKind::Program { statements: vec![call] }, SourceLocation::new(0, 26));
    let observed = &find_import_into_calls(&ast)[0];

    assert_eq!(observed.source, ImportIntoSource::Dynamic);
    assert_eq!(observed.target, ImportIntoTarget::Dynamic);
}

#[test]
fn recognizes_literal_destination_package() {
    let call = Node::new(
        NodeKind::MethodCall {
            object: Box::new(identifier("warnings", 0)),
            method: "import::into".into(),
            args: vec![Node::new(
                NodeKind::String { value: "My::Package".into(), interpolated: false },
                SourceLocation::new(20, 33),
            )],
        },
        SourceLocation::new(0, 34),
    );
    let ast = Node::new(NodeKind::Program { statements: vec![call] }, SourceLocation::new(0, 34));

    assert_eq!(
        find_import_into_calls(&ast)[0].target,
        ImportIntoTarget::Package("My::Package".into())
    );
}

#[test]
fn production_environment_retains_observations() {
    let call = Node::new(
        NodeKind::MethodCall {
            object: Box::new(identifier("strict", 0)),
            method: "import::into".into(),
            args: vec![Node::new(
                NodeKind::Number { value: "1".into() },
                SourceLocation::new(20, 21),
            )],
        },
        SourceLocation::new(0, 22),
    );
    let ast = Node::new(NodeKind::Program { statements: vec![call] }, SourceLocation::new(0, 22));
    let environment = CompileTimePragmaEnvironment::build(&ast);

    assert_eq!(environment.import_into_calls(), &find_import_into_calls(&ast));
}
