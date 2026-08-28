//! Perl 5.42 native-class and Object::Pad writer generation regressions.

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
fn named_writer_is_profile_bound_but_object_pad_supported() {
    let mut parser = Parser::new("use v5.40; class TooOld { field $value :writer(write_value); }");
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "TooOld"));
    let field = must_some(model.fields.first());

    assert_eq!(field.writer, None, "unsupported native named writer must stay non-exact");
    assert!(
        !model.methods.iter().any(|method| method.synthetic && method.name == "write_value"),
        "unsupported native named writer must not synthesize a method"
    );

    let mut parser =
        Parser::new("use Object::Pad; class Extension { field $value :writer(write_value); }");
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Extension"));
    let field = must_some(model.fields.first());

    assert_eq!(field.writer.as_deref(), Some("write_value"));
    assert!(model.methods.iter().any(|method| method.synthetic && method.name == "write_value"));
}

#[test]
fn native_bare_writer_requires_perl_5_42() {
    let mut parser = Parser::new("use v5.40; class TooOld { field $value :writer; }");
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "TooOld"));
    let field = must_some(model.fields.first());

    assert_eq!(field.writer, None, "bare native writer is unsupported before Perl 5.42");
    assert!(
        !model.methods.iter().any(|method| method.synthetic && method.name == "set_value"),
        "unsupported bare writer must not synthesize the default method"
    );
}

#[test]
fn native_writer_is_rejected_for_non_scalar_fields() {
    let mut parser = Parser::new(
        r#"
use v5.42;
class Collection {
    field @values :writer;
    field %metadata :writer(write_metadata);
    field $valid_scalar :writer(invalid-name);
}
"#,
    );
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Collection"));

    assert!(
        !model.fields.iter().any(|field| field.name == "values" || field.name == "metadata"),
        "non-scalar field declarations must be rejected before entering the writer boundary"
    );
    assert!(
        !model.methods.iter().any(|method| method.synthetic),
        "array/hash fields must not synthesize scalar writer methods"
    );

    let scalar = must_some(model.fields.iter().find(|field| field.name == "valid_scalar"));
    assert_eq!(
        scalar.writer, None,
        "a valid scalar field must still reject an invalid named-writer identifier"
    );
    assert!(
        !model.methods.iter().any(|method| method.synthetic && method.name == "invalid-name"),
        "an invalid named-writer identifier must not synthesize a method"
    );
}

#[test]
fn native_writer_respects_explicit_method_declared_before_field() {
    let mut parser = Parser::new(
        r#"
use v5.42;
class Collision {
    method set_value { }
    field $value :writer;
}
"#,
    );
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Collision"));
    let field = must_some(model.fields.iter().find(|field| field.name == "value"));

    assert_eq!(field.writer, None, "an explicit method must reserve the writer name");
    assert_eq!(
        model.methods.iter().filter(|method| method.name == "set_value").count(),
        1,
        "the explicit method must not be duplicated by synthetic writer generation"
    );
    assert!(
        model.methods.iter().any(|method| method.name == "set_value" && !method.synthetic),
        "the surviving method must retain its explicit declaration"
    );
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
fn generated_writer_does_not_collide_with_synthetic_reader() {
    let mut parser = Parser::new(
        r#"
use Object::Pad;
class Collision {
    field $reader :reader;
    field $writer :writer(reader);
}
"#,
    );
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    let model = must_some(models.iter().find(|model| model.name == "Collision"));
    let reader_field = must_some(model.fields.iter().find(|field| field.name == "reader"));
    let writer_field = must_some(model.fields.iter().find(|field| field.name == "writer"));

    assert_eq!(reader_field.reader.as_deref(), Some("reader"));
    assert_eq!(writer_field.writer, None, "synthetic reader must own the colliding name");
    assert_eq!(
        model.methods.iter().filter(|method| method.name == "reader").count(),
        1,
        "collision must not publish duplicate synthetic members"
    );
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
