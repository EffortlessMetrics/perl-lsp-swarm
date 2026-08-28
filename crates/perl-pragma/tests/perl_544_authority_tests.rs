use perl_ast::SourceLocation;
use perl_ast::ast::{Node, NodeKind};
use perl_pragma::{
    CompileTimePragmaEnvironment, PerlVersion, PragmaState, features_enabled_by_version,
};

fn loc(start: usize, end: usize) -> SourceLocation {
    SourceLocation { start, end }
}

fn use_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node::new(
        NodeKind::Use {
            module: module.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            has_filter_risk: false,
        },
        loc(start, end),
    )
}

fn no_node(module: &str, args: &[&str], start: usize, end: usize) -> Node {
    Node::new(
        NodeKind::No {
            module: module.to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            has_filter_risk: false,
        },
        loc(start, end),
    )
}

fn program(statements: Vec<Node>, end: usize) -> Node {
    Node::new(NodeKind::Program { statements }, loc(0, end))
}

#[test]
fn perl_544_bundle_is_explicit_and_excludes_enhanced_xx() {
    let expected = vec![
        "bitwise",
        "current_sub",
        "evalbytes",
        "fc",
        "isa",
        "module_true",
        "postderef_qq",
        "say",
        "signatures",
        "state",
        "try",
        "unicode_eval",
        "unicode_strings",
    ];

    let perl_542 = features_enabled_by_version(PerlVersion::new(5, 42));
    let perl_544 = features_enabled_by_version(PerlVersion::new(5, 44));

    assert_eq!(perl_544, expected);
    assert_eq!(perl_544, perl_542, "Perl 5.44 keeps the 5.42 bundle membership");
    assert!(
        !perl_544.contains(&"enhanced_xx"),
        "enhanced_xx remains opt-in under Perl 5.44",
    );
}

#[test]
fn explicit_feature_pragmas_toggle_enhanced_xx() {
    let ast = program(
        vec![
            use_node("feature", &["'enhanced_xx'"], 0, 28),
            no_node("feature", &["'enhanced_xx'"], 40, 67),
        ],
        67,
    );
    let environment = CompileTimePragmaEnvironment::build(&ast);

    assert!(environment.snapshot_at(30).has_feature("enhanced_xx"));
    assert!(!environment.snapshot_at(67).has_feature("enhanced_xx"));
}

#[test]
fn feature_all_and_experimental_admit_enhanced_xx() {
    let feature_all = program(vec![use_node("feature", &["':all'"], 0, 20)], 20);
    let experimental = program(
        vec![use_node("experimental", &["'enhanced_xx'"], 0, 34)],
        34,
    );

    assert!(
        CompileTimePragmaEnvironment::build(&feature_all)
            .snapshot_at(20)
            .has_feature("enhanced_xx"),
    );
    assert!(
        CompileTimePragmaEnvironment::build(&experimental)
            .snapshot_at(34)
            .has_feature("enhanced_xx"),
    );
}

#[test]
fn use_version_is_retained_in_snapshot() {
    let ast = program(vec![use_node("v5.44", &[], 0, 10)], 10);

    assert_eq!(
        CompileTimePragmaEnvironment::build(&ast).snapshot_at(10).perl_version(),
        Some(PerlVersion::new(5, 44)),
    );
}

#[test]
fn require_version_does_not_change_lexical_pragma_state() {
    let ast = program(
        vec![Node::new(
            NodeKind::FunctionCall {
                name: "require".to_string(),
                args: vec![Node::new(
                    NodeKind::VString { value: "v5.44".to_string() },
                    loc(8, 13),
                )],
            },
            loc(0, 14),
        )],
        14,
    );
    let environment = CompileTimePragmaEnvironment::build(&ast);
    let snapshot = environment.snapshot_at(14);

    assert_eq!(snapshot.perl_version(), None);
    assert!(!snapshot.strict_enabled());
    assert!(!snapshot.warnings_enabled());
    assert_eq!(snapshot.state(), &PragmaState::default());
}

#[test]
fn conditional_version_target_retains_version_authority() {
    let ast = program(
        vec![use_node("if", &["$]", ">=", "5.044", "v5.44"], 0, 30)],
        30,
    );

    assert_eq!(
        CompileTimePragmaEnvironment::build(&ast)
            .snapshot_at(30)
            .perl_version(),
        Some(PerlVersion::new(5, 44)),
    );
}

#[test]
fn nested_version_declaration_restores_outer_authority() {
    let inner = Node::new(
        NodeKind::Block { statements: vec![use_node("v5.44", &[], 20, 30)] },
        loc(15, 50),
    );
    let ast = program(vec![use_node("v5.42", &[], 0, 10), inner], 50);
    let environment = CompileTimePragmaEnvironment::build(&ast);

    assert_eq!(
        environment.snapshot_at(12).perl_version(),
        Some(PerlVersion::new(5, 42))
    );
    assert_eq!(
        environment.snapshot_at(35).perl_version(),
        Some(PerlVersion::new(5, 44))
    );
    assert_eq!(
        environment.snapshot_at(50).perl_version(),
        Some(PerlVersion::new(5, 42))
    );
}
