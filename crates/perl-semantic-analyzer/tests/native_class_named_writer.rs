//! Perl 5.42 native-class writer generation regression.

use perl_semantic_analyzer::{Parser, analysis::class_model::ClassModelBuilder};
use perl_tdd_support::{must, must_some};

#[test]
fn native_class_named_writer_uses_explicit_method_name() {
    let mut parser = Parser::new(
        r#"
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
