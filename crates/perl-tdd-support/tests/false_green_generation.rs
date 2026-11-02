//! False-green regression tests for TDD generation (#8154).

use perl_tdd_support::must_some;
use perl_tdd_support::must_with;
use perl_tdd_support::tdd_workflow::{TddConfig, TddWorkflow, TestType};
use perl_tdd_support::test_generator::{
    FalseGreenPolicy, GeneratedTestReadiness, TestFramework, TestGenerator, TestGeneratorOptions,
};
use std::collections::HashMap;
use std::process::Command;

fn assert_no_false_green(code: &str, readiness: GeneratedTestReadiness) {
    assert!(!code.contains("pass("), "must not emit pass(): {code}");
    assert!(!code.contains("ok(1"), "must not emit ok(1): {code}");
    if readiness != GeneratedTestReadiness::Runnable {
        assert!(
            code.contains("TODO: {") || code.contains("plan skip_all"),
            "draft/unsupported must encode TODO block or skip: {code}"
        );
    }
    let policy = FalseGreenPolicy;
    assert!(
        policy.check(code, readiness).is_empty(),
        "policy violations: {:?}\n{code}",
        policy.check(code, readiness)
    );
}

#[test]
fn test_generator_basic_without_oracle_is_draft() {
    let generator = TestGenerator::new(TestFramework::TestMore);
    let ast = perl_tdd_support::Node::new(
        perl_tdd_support::NodeKind::Subroutine {
            name: Some("zero".to_string()),
            name_span: None,
            declarator: None,
            signature: None,
            body: Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Block { statements: vec![] },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            )),
            attributes: vec![],
            prototype: None,
        },
        perl_tdd_support::SourceLocation { start: 0, end: 0 },
    );
    let tests = generator.generate_tests(&ast, "sub zero { 0 }");
    let basic = must_some(tests.iter().find(|t| t.name == "test_zero"));
    assert_eq!(basic.readiness, GeneratedTestReadiness::DraftNeedsOracle);
    assert_no_false_green(&basic.code, basic.readiness);
}

#[test]
fn test_generator_expected_value_still_draft_without_inputs() {
    let mut options = TestGeneratorOptions::default();
    options.expected_values.insert("add".to_string(), "3".to_string());
    let generator = TestGenerator::with_options(TestFramework::TestMore, options);
    let ast = perl_tdd_support::Node::new(
        perl_tdd_support::NodeKind::Subroutine {
            name: Some("add".to_string()),
            name_span: None,
            declarator: None,
            signature: None,
            body: Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Block { statements: vec![] },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            )),
            attributes: vec![],
            prototype: None,
        },
        perl_tdd_support::SourceLocation { start: 0, end: 0 },
    );
    let tests = generator.generate_tests(&ast, "sub add { }");
    let basic = must_some(tests.iter().find(|t| t.name == "test_add"));
    assert_eq!(basic.readiness, GeneratedTestReadiness::DraftNeedsOracle);
    assert_no_false_green(&basic.code, basic.readiness);
}

#[test]
fn test_generator_unsupported_framework_is_not_empty_runnable() {
    let generator = TestGenerator::new(TestFramework::Test2V0);
    let ast = perl_tdd_support::Node::new(
        perl_tdd_support::NodeKind::Subroutine {
            name: Some("fn".to_string()),
            name_span: None,
            declarator: None,
            signature: Some(Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Signature {
                    parameters: vec![perl_tdd_support::Node::new(
                        perl_tdd_support::NodeKind::MandatoryParameter {
                            variable: Box::new(perl_tdd_support::Node::new(
                                perl_tdd_support::NodeKind::Variable {
                                    name: "$x".to_string(),
                                    sigil: "$".to_string(),
                                },
                                perl_tdd_support::SourceLocation { start: 0, end: 0 },
                            )),
                        },
                        perl_tdd_support::SourceLocation { start: 0, end: 0 },
                    )],
                },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            ))),
            body: Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Block { statements: vec![] },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            )),
            attributes: vec![],
            prototype: None,
        },
        perl_tdd_support::SourceLocation { start: 0, end: 0 },
    );
    let tests = generator.generate_tests(&ast, "sub fn { }");
    assert!(!tests.is_empty());
    for test in tests {
        assert_ne!(test.readiness, GeneratedTestReadiness::Runnable);
        assert!(!test.code.trim().is_empty());
        assert_no_false_green(&test.code, test.readiness);
    }
}

#[test]
fn tdd_workflow_generators_do_not_false_green() {
    let wf = TddWorkflow::new(TddConfig::default());
    let cases = [
        (TestType::Basic, GeneratedTestReadiness::DraftNeedsOracle),
        (TestType::EdgeCase, GeneratedTestReadiness::DraftNeedsOracle),
        (TestType::ErrorHandling, GeneratedTestReadiness::DraftNeedsOracle),
        (TestType::Performance, GeneratedTestReadiness::DraftNeedsOracle),
        (TestType::Integration, GeneratedTestReadiness::DraftNeedsSetup),
    ];
    for (kind, readiness) in cases {
        let tc = wf.generate_test_for_function("svc", &["$x".to_string()], kind);
        assert_eq!(tc.readiness, readiness);
        assert_no_false_green(&tc.code, tc.readiness);
    }
}

#[test]
fn integration_template_never_uses_pass() {
    let wf = TddWorkflow::new(TddConfig::default());
    let tc = wf.generate_test_for_function("setup", &[], TestType::Integration);
    assert!(!tc.code.contains("pass("));
    assert!(tc.code.contains("PENDING"));
}

#[test]
fn policy_negative_fixture_rejects_known_false_green_classes() {
    let policy = FalseGreenPolicy;
    let runnable_pass = policy.check("pass('x');", GeneratedTestReadiness::Runnable);
    assert!(runnable_pass.iter().any(|v| v.rule == "unconditional_pass"));

    let eval_pass =
        policy.check("eval { 1 }; pass('y');", GeneratedTestReadiness::DraftNeedsOracle);
    assert!(eval_pass.iter().any(|v| v.rule == "eval_then_pass"));

    let draft_missing = policy.check("ok(1);", GeneratedTestReadiness::DraftNeedsOracle);
    assert!(draft_missing.iter().any(|v| v.rule == "draft_missing_todo"));
}

#[test]
fn generated_draft_prove_is_not_all_green_without_todo() {
    if Command::new("perl").arg("-v").output().is_err() {
        return;
    }
    let wf = TddWorkflow::new(TddConfig::default());
    let tc = wf.generate_test_for_function("setup", &[], TestType::Integration);
    let dir = must_with(tempfile::tempdir(), "tempdir for prove fixture");
    let path = dir.path().join("integration.t");
    must_with(std::fs::write(&path, &tc.code), "write generated perl for prove fixture");

    let output = must_with(
        Command::new("prove").arg("-v").arg(&path).output(),
        "run prove on generated integration draft",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TODO") || !output.status.success(),
        "integration draft must not be all-green without TODO; stdout={stdout}"
    );
}

#[test]
fn generated_basic_draft_parses_under_perl_c() {
    if Command::new("perl").arg("-v").output().is_err() {
        return;
    }
    let generator = TestGenerator::new(TestFramework::TestMore);
    let ast = perl_tdd_support::Node::new(
        perl_tdd_support::NodeKind::Subroutine {
            name: Some("add".to_string()),
            name_span: None,
            declarator: None,
            signature: None,
            body: Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Block { statements: vec![] },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            )),
            attributes: vec![],
            prototype: None,
        },
        perl_tdd_support::SourceLocation { start: 0, end: 0 },
    );
    let tests = generator.generate_tests(&ast, "sub add { }");
    let basic = must_some(tests.iter().find(|t| t.name == "test_add"));
    let dir = must_with(tempfile::tempdir(), "tempdir for perl -c");
    let path = dir.path().join("basic.t");
    must_with(std::fs::write(&path, &basic.code), "write generated basic draft for perl -c");
    let output = must_with(
        Command::new("perl").arg("-c").arg(&path).output(),
        "perl -c on generated basic draft",
    );
    assert!(
        output.status.success(),
        "generated basic draft must parse: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn options_map_does_not_change_draft_without_oracle_key() {
    let _options: TestGeneratorOptions =
        TestGeneratorOptions { expected_values: HashMap::new(), ..TestGeneratorOptions::default() };
    let generator = TestGenerator::new(TestFramework::TestMore);
    let ast = perl_tdd_support::Node::new(
        perl_tdd_support::NodeKind::Subroutine {
            name: Some("mystery".to_string()),
            name_span: None,
            declarator: None,
            signature: None,
            body: Box::new(perl_tdd_support::Node::new(
                perl_tdd_support::NodeKind::Block { statements: vec![] },
                perl_tdd_support::SourceLocation { start: 0, end: 0 },
            )),
            attributes: vec![],
            prototype: None,
        },
        perl_tdd_support::SourceLocation { start: 0, end: 0 },
    );
    let tests = generator.generate_tests(&ast, "sub mystery { }");
    let basic = must_some(tests.iter().find(|t| t.name == "test_mystery"));
    assert_eq!(basic.readiness, GeneratedTestReadiness::DraftNeedsOracle);
}
