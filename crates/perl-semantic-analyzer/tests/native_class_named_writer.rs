//! Perl 5.42 native-class writer generation regression.

use perl_semantic_analyzer::{
    Parser,
    analysis::{class_model::ClassModelBuilder, semantic::SemanticAnalyzer},
};
use perl_tdd_support::{must, must_some};

#[test]
fn native_class_named_writer_uses_explicit_method_name() {
    let mut parser = Parser::new(
        r#"
use v5.42;
class Counter {
    field $implicit :writer;
    field $named :writer(write_named);
}
"#,
    );
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Counter"));

    let implicit = must_some(model.fields.iter().find(|field| field.name == "implicit"));
    let named = must_some(model.fields.iter().find(|field| field.name == "named"));

    assert_eq!(implicit.writer.as_deref(), Some("set_implicit"));
    assert_eq!(named.writer.as_deref(), Some("write_named"));
    assert!(model.methods.iter().any(|method| method.synthetic && method.name == "set_implicit"));
    assert!(model.methods.iter().any(|method| method.synthetic && method.name == "write_named"));
    assert!(
        !model.methods.iter().any(|method| method.synthetic && method.name == "set_named"),
        "an explicitly named writer must not retain the default set_<field> identity"
    );
}

#[test]
fn named_writer_is_profile_bound_and_not_object_pad() {
    for (code, class_name) in [
        ("use v5.40; class TooOld { field $value :writer(write_value); }", "TooOld"),
        (
            "use Object::Pad; use v5.42; class Extension { field $value :writer(write_value); }",
            "Extension",
        ),
    ] {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        let models = ClassModelBuilder::new().build(&ast);
        let model = must_some(models.iter().find(|model| model.name == class_name));
        let field = must_some(model.fields.first());

        assert_eq!(field.writer, None, "unsupported named writer must stay non-exact");
        assert!(
            !model.methods.iter().any(|method| method.synthetic && method.name == "write_value"),
            "unsupported named writer must not synthesize a method"
        );
    }
}

#[test]
fn named_writer_attributes_are_fail_closed_and_source_ordered() {
    let mut parser = Parser::new(
        r#"
use v5.42;
class Deterministic {
    field $first :writer(shared);
    field $second :writer(shared);
    field $malformed :writer();
    field $ordered :writer(first_name) :writer(second_name);
    field $colliding :writer(explicit_name);
    method explicit_name { }
}
"#,
    );
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Deterministic"));

    assert_eq!(
        must_some(model.fields.iter().find(|field| field.name == "first")).writer.as_deref(),
        Some("shared")
    );
    assert_eq!(must_some(model.fields.iter().find(|field| field.name == "second")).writer, None);
    assert_eq!(must_some(model.fields.iter().find(|field| field.name == "malformed")).writer, None);
    assert_eq!(
        must_some(model.fields.iter().find(|field| field.name == "ordered")).writer.as_deref(),
        Some("first_name")
    );
    assert_eq!(must_some(model.fields.iter().find(|field| field.name == "colliding")).writer, None);

    let writers: Vec<_> = model
        .methods
        .iter()
        .filter(|method| method.synthetic && method.generated_kind.is_some())
        .map(|method| method.name.as_str())
        .collect();
    assert_eq!(writers, vec!["shared", "first_name"]);
}

#[test]
fn semantic_analyzer_reaches_named_writer_class_model() {
    let code = "use v5.42; class Counter { field $value :writer(write_value); }";
    let mut parser = Parser::new(code);
    let ast = must(parser.parse());
    let analyzer = SemanticAnalyzer::analyze_with_source(&ast, code);
    let model = must_some(analyzer.class_models.iter().find(|model| model.name == "Counter"));
    let field = must_some(model.fields.first());

    assert_eq!(field.writer.as_deref(), Some("write_value"));
    assert!(model.methods.iter().any(|method| method.synthetic && method.name == "write_value"));
}
