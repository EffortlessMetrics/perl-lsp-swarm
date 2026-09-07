//! Explicit field-trait argument identity for the compatibility class model.
//!
//! Controlling issue: #13449. The parser retains spellings such as
//! `reader(read_name)`, but the compatibility class model used to compare them
//! against exact bare strings, so every explicitly named `:param`, `:reader`,
//! `:accessor`, and `:mutator` was silently discarded. These regressions pin
//! the decoded identity for each trait family and its bounded failure states.

use perl_semantic_analyzer::{
    Parser,
    analysis::class_model::{ClassModel, ClassModelBuilder, FieldInfo},
};
use perl_tdd_support::{must, must_some};

fn model_for(source: &str, class_name: &str) -> ClassModel {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let models = ClassModelBuilder::new().build(&ast);
    must_some(models.into_iter().find(|model| model.name == class_name))
}

/// Build an `Object::Pad` class whose single field carries `attribute`.
fn object_pad_field(attribute: &str) -> (ClassModel, FieldInfo) {
    let source = format!("use Object::Pad;\nclass Probe {{\n    field $value :{attribute};\n}}\n");
    let model = model_for(&source, "Probe");
    let field = must_some(model.fields.first().cloned());
    (model, field)
}

fn synthesizes(model: &ClassModel, name: &str) -> bool {
    model.methods.iter().any(|method| method.synthetic && method.name == name)
}

#[test]
fn bare_traits_keep_their_existing_defaults() {
    let (model, field) = object_pad_field("reader");
    assert_eq!(field.reader.as_deref(), Some("value"));
    assert!(synthesizes(&model, "value"));

    let (model, field) = object_pad_field("writer");
    assert_eq!(field.writer.as_deref(), Some("set_value"));
    assert!(synthesizes(&model, "set_value"));

    let (model, field) = object_pad_field("accessor");
    assert_eq!(field.accessor.as_deref(), Some("value"));
    assert!(synthesizes(&model, "value"));

    let (model, field) = object_pad_field("mutator");
    assert_eq!(field.mutator.as_deref(), Some("value"));
    assert!(synthesizes(&model, "value"));

    let (_, field) = object_pad_field("param");
    assert!(field.param, "a bare :param still participates in the constructor");
    assert_eq!(field.param_name, None, "a bare :param spells no explicit name");
}

#[test]
fn explicit_static_names_are_retained_exactly() {
    let (model, field) = object_pad_field("reader(read_name)");
    assert_eq!(field.reader.as_deref(), Some("read_name"));
    assert!(synthesizes(&model, "read_name"));

    let (model, field) = object_pad_field("writer(write_name)");
    assert_eq!(field.writer.as_deref(), Some("write_name"));
    assert!(synthesizes(&model, "write_name"));

    let (model, field) = object_pad_field("accessor(access_name)");
    assert_eq!(field.accessor.as_deref(), Some("access_name"));
    assert!(synthesizes(&model, "access_name"));

    let (model, field) = object_pad_field("mutator(mutate_name)");
    assert_eq!(field.mutator.as_deref(), Some("mutate_name"));
    assert!(synthesizes(&model, "mutate_name"));

    let (_, field) = object_pad_field("param(external_name)");
    assert!(field.param, "a named :param still participates in the constructor");
    assert_eq!(field.param_name.as_deref(), Some("external_name"));
}

#[test]
fn an_explicit_name_replaces_rather_than_supplements_the_bare_default() {
    for (attribute, explicit, default) in [
        ("reader(read_name)", "read_name", "value"),
        ("writer(write_name)", "write_name", "set_value"),
        ("accessor(access_name)", "access_name", "value"),
        ("mutator(mutate_name)", "mutate_name", "value"),
    ] {
        let (model, _) = object_pad_field(attribute);
        assert!(synthesizes(&model, explicit), "`{attribute}` must synthesize `{explicit}`");
        assert!(
            !synthesizes(&model, default),
            "`{attribute}` must not also retain the default `{default}` identity"
        );
    }

    let (_, field) = object_pad_field("param(external_name)");
    assert_eq!(
        field.param_name.as_deref(),
        Some("external_name"),
        "a named :param must not fall back to the field name"
    );
}

#[test]
fn an_empty_argument_does_not_generate_the_bare_default() {
    for (attribute, default) in [
        ("reader()", "value"),
        ("writer()", "set_value"),
        ("accessor()", "value"),
        ("mutator()", "value"),
    ] {
        let (model, field) = object_pad_field(attribute);
        assert!(
            !synthesizes(&model, default),
            "`{attribute}` must not generate the bare default `{default}`"
        );
        assert_eq!(field.reader, None, "`{attribute}` leaves no reader");
        assert_eq!(field.writer, None, "`{attribute}` leaves no writer");
        assert_eq!(field.accessor, None, "`{attribute}` leaves no accessor");
        assert_eq!(field.mutator, None, "`{attribute}` leaves no mutator");
    }

    let (_, field) = object_pad_field("param()");
    assert!(!field.param, "an empty :param argument does not admit a constructor input");
    assert_eq!(field.param_name, None);
}

/// Non-static arguments that the parser still retains as attributes.
///
/// An *unclosed* spelling such as `:reader(` never reaches the class model —
/// the parser does not retain a field for it at all — so the decoder's
/// handling of that form is pinned by its own unit tests rather than here.
#[test]
fn malformed_and_dynamic_arguments_stay_bounded() {
    for attribute in
        ["reader($dyn)", "reader(1bad)", "writer($dyn)", "accessor(a-b)", "mutator(get())"]
    {
        let (model, field) = object_pad_field(attribute);
        assert_eq!(field.reader, None, "`{attribute}` must not produce a reader");
        assert_eq!(field.writer, None, "`{attribute}` must not produce a writer");
        assert_eq!(field.accessor, None, "`{attribute}` must not produce an accessor");
        assert_eq!(field.mutator, None, "`{attribute}` must not produce a mutator");
        assert!(
            model.methods.iter().all(|method| !method.synthetic),
            "`{attribute}` must not synthesize any method"
        );
    }

    for attribute in ["param($dyn)", "param(1bad)"] {
        let (_, field) = object_pad_field(attribute);
        assert!(!field.param, "`{attribute}` must not admit a constructor input");
        assert_eq!(field.param_name, None, "`{attribute}` must not invent a parameter name");
    }
}

#[test]
fn an_unknown_extension_trait_is_not_a_known_language_trait() {
    for attribute in ["Custom", "Custom(read_name)", "Reader", "readers", "myreader"] {
        let (model, field) = object_pad_field(attribute);
        assert!(!field.param, "`{attribute}` must not be read as :param");
        assert_eq!(field.reader, None, "`{attribute}` must not be read as :reader");
        assert_eq!(field.writer, None, "`{attribute}` must not be read as :writer");
        assert_eq!(field.accessor, None, "`{attribute}` must not be read as :accessor");
        assert_eq!(field.mutator, None, "`{attribute}` must not be read as :mutator");
        assert!(
            model.methods.iter().all(|method| !method.synthetic),
            "`{attribute}` must not synthesize any method"
        );
    }
}

#[test]
fn the_parser_preserved_spelling_survives_into_the_compatibility_model() {
    for attribute in ["reader(read_name)", "param(external_name)", "reader($dyn)", "Custom(x)"] {
        let (_, field) = object_pad_field(attribute);
        assert_eq!(
            field.attributes,
            vec![attribute.to_owned()],
            "the raw spelling must remain available for later diagnostics"
        );
    }
}

#[test]
fn underscore_field_defaults_remain_unchanged_and_named_forms_are_not_stripped() {
    let source =
        "use Object::Pad;\nclass Probe {\n    field $_secret :reader :accessor :mutator;\n}\n";
    let model = model_for(source, "Probe");
    let field = must_some(model.fields.first().cloned());
    assert_eq!(
        field.reader.as_deref(),
        Some("secret"),
        "the bare default still strips a leading _"
    );
    assert_eq!(field.accessor.as_deref(), Some("secret"));
    assert_eq!(field.mutator.as_deref(), Some("secret"));

    // An explicit name is source identity, not a field name: it is used exactly
    // as spelled, including a leading underscore.
    let source = "use Object::Pad;\nclass Named {\n    field $value :reader(_hidden);\n}\n";
    let model = model_for(source, "Named");
    let field = must_some(model.fields.first().cloned());
    assert_eq!(field.reader.as_deref(), Some("_hidden"));
    assert!(synthesizes(&model, "_hidden"));
    assert!(!synthesizes(&model, "hidden"), "an explicit name is not re-normalized");
}

#[test]
fn every_trait_family_decodes_through_the_same_path() {
    // A single field carrying every family at once must resolve each one to
    // its explicit name — no family may be left on an incompatible parser.
    let source = "use Object::Pad;\nclass All {\n    field $value :param(p_name) :reader(r_name) :writer(w_name) :accessor(a_name) :mutator(m_name);\n}\n";
    let model = model_for(source, "All");
    let field = must_some(model.fields.first().cloned());

    assert!(field.param);
    assert_eq!(field.param_name.as_deref(), Some("p_name"));
    assert_eq!(field.reader.as_deref(), Some("r_name"));
    assert_eq!(field.writer.as_deref(), Some("w_name"));
    assert_eq!(field.accessor.as_deref(), Some("a_name"));
    assert_eq!(field.mutator.as_deref(), Some("m_name"));

    for name in ["r_name", "w_name", "a_name", "m_name"] {
        assert!(synthesizes(&model, name), "`{name}` must be synthesized");
    }
    for name in ["value", "set_value"] {
        assert!(!synthesizes(&model, name), "no default identity may survive alongside `{name}`");
    }
}

#[test]
fn two_fields_with_the_same_explicit_name_remain_distinguishable() {
    let source = "use Object::Pad;\nclass Clash {\n    field $first :reader(shared);\n    field $second :reader(shared);\n}\n";
    let model = model_for(source, "Clash");

    let first = must_some(model.fields.iter().find(|field| field.name == "first"));
    let second = must_some(model.fields.iter().find(|field| field.name == "second"));
    assert_eq!(first.reader.as_deref(), Some("shared"));
    assert_eq!(second.reader.as_deref(), Some("shared"));
    assert_ne!(
        first.location.start, second.location.start,
        "colliding readers stay distinguishable by source location for a later conflict owner"
    );
}

#[test]
fn a_named_param_is_the_constructor_keyword_the_field_name_is_not() {
    let source = "use Object::Pad;\nclass Ctor {\n    field $value :param(external_name);\n    field $plain :param;\n}\n";
    let model = model_for(source, "Ctor");

    let constructor: Vec<&str> = model.object_pad_constructor_param_names().collect();
    assert_eq!(
        constructor,
        vec!["external_name", "plain"],
        "an explicit :param name replaces the field name as the constructor keyword"
    );

    let fields: Vec<&str> = model.object_pad_param_field_names().collect();
    assert_eq!(fields, vec!["value", "plain"], "field identity remains separately available");
}

#[test]
fn a_named_param_is_not_admitted_by_decoding_a_non_param_trait() {
    // Negative control: decoding an argument must not turn a non-`:param`
    // trait into a constructor input.
    let source = "use Object::Pad;\nclass NoParam {\n    field $value :reader(external_name);\n}\n";
    let model = model_for(source, "NoParam");
    let field = must_some(model.fields.first().cloned());

    assert!(!field.param, ":reader must not admit a constructor input");
    assert_eq!(field.param_name, None);
    assert_eq!(model.object_pad_constructor_param_names().count(), 0);
}
