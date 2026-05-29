//! Property tests for the shared `PerlValue` debugger value model.
//!
//! These tests complement the example-based serde tests by generating nested
//! arrays, hashes, references, objects, tied values, and scalar leaves. The
//! core invariant is that every generated value keeps its shape across JSON
//! serialization and continues to report stable DAP renderer metadata.

use perl_dap::value::PerlValue;
use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;

fn arb_debugger_string() -> impl Strategy<Value = String> {
    "[\\PC]{0,32}".prop_map(String::from)
}

fn arb_class_name() -> impl Strategy<Value = String> {
    proptest::collection::vec("[A-Z][A-Za-z0-9_]{0,8}", 1..=3).prop_map(|parts| parts.join("::"))
}

fn arb_leaf_perl_value() -> impl Strategy<Value = PerlValue> {
    prop_oneof![
        Just(PerlValue::Undef),
        arb_debugger_string().prop_map(PerlValue::Scalar),
        (-1_000_000i32..1_000_000i32).prop_map(|value| PerlValue::Number(f64::from(value))),
        any::<i64>().prop_map(PerlValue::Integer),
        proptest::option::of(arb_debugger_string()).prop_map(|name| PerlValue::Code { name }),
        arb_debugger_string().prop_map(PerlValue::Glob),
        arb_debugger_string().prop_map(PerlValue::Regex),
        arb_debugger_string().prop_map(PerlValue::Error),
        (arb_debugger_string(), proptest::option::of(0usize..10_000usize))
            .prop_map(|(summary, total_count)| PerlValue::Truncated { summary, total_count },),
    ]
}

fn arb_perl_value() -> impl Strategy<Value = PerlValue> {
    arb_leaf_perl_value().prop_recursive(4, 64, 4, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..=4).prop_map(PerlValue::Array),
            proptest::collection::vec((arb_debugger_string(), inner.clone()), 0..=4)
                .prop_map(PerlValue::Hash),
            inner.clone().prop_map(|value| PerlValue::Reference(Box::new(value))),
            (arb_class_name(), inner.clone())
                .prop_map(|(class, value)| PerlValue::Object { class, value: Box::new(value) }),
            (arb_class_name(), proptest::option::of(inner)).prop_map(|(class, value)| {
                PerlValue::Tied { class, value: value.map(Box::new) }
            }),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    #[test]
    fn prop_perl_value_json_roundtrip(value in arb_perl_value()) {
        let json = serde_json::to_string(&value)
            .map_err(|error| TestCaseError::fail(format!("serialize failed: {error}")))?;
        let roundtrip: PerlValue = serde_json::from_str(&json)
            .map_err(|error| TestCaseError::fail(format!("deserialize failed: {error}")))?;
        let second_json = serde_json::to_string(&roundtrip)
            .map_err(|error| TestCaseError::fail(format!("re-serialize failed: {error}")))?;

        prop_assert_eq!(&roundtrip, &value);
        prop_assert_eq!(&second_json, &json);
    }

    #[test]
    fn prop_perl_value_renderer_metadata_matches_variant(value in arb_perl_value()) {
        match &value {
            PerlValue::Undef => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "undef");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Scalar(_) | PerlValue::Number(_) | PerlValue::Integer(_) => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "SCALAR");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Array(elements) => {
                prop_assert!(value.is_expandable());
                prop_assert_eq!(value.type_name(), "ARRAY");
                prop_assert_eq!(value.child_count(), Some(elements.len()));
            }
            PerlValue::Hash(entries) => {
                prop_assert!(value.is_expandable());
                prop_assert_eq!(value.type_name(), "HASH");
                prop_assert_eq!(value.child_count(), Some(entries.len()));
            }
            PerlValue::Reference(_) => {
                prop_assert!(value.is_expandable());
                prop_assert_eq!(value.type_name(), "REF");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Object { .. } => {
                prop_assert!(value.is_expandable());
                prop_assert_eq!(value.type_name(), "OBJECT");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Code { .. } => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "CODE");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Glob(_) => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "GLOB");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Regex(_) => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "Regexp");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Tied { .. } => {
                prop_assert!(value.is_expandable());
                prop_assert_eq!(value.type_name(), "TIED");
                prop_assert_eq!(value.child_count(), None);
            }
            PerlValue::Truncated { total_count, .. } => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "...");
                prop_assert_eq!(value.child_count(), *total_count);
            }
            PerlValue::Error(_) => {
                prop_assert!(!value.is_expandable());
                prop_assert_eq!(value.type_name(), "ERROR");
                prop_assert_eq!(value.child_count(), None);
            }
        }
    }

    #[test]
    fn prop_perl_value_constructors_preserve_inputs(
        scalar in arb_debugger_string(),
        elements in proptest::collection::vec(arb_leaf_perl_value(), 0..=8),
        entries in proptest::collection::vec((arb_debugger_string(), arb_leaf_perl_value()), 0..=8),
        referent in arb_leaf_perl_value(),
        class in arb_class_name(),
        object_value in arb_leaf_perl_value(),
    ) {
        prop_assert_eq!(PerlValue::scalar(scalar.clone()), PerlValue::Scalar(scalar));
        prop_assert_eq!(PerlValue::array(elements.clone()), PerlValue::Array(elements));
        prop_assert_eq!(PerlValue::hash(entries.clone()), PerlValue::Hash(entries));
        prop_assert_eq!(PerlValue::reference(referent.clone()), PerlValue::Reference(Box::new(referent)));
        prop_assert_eq!(
            PerlValue::object(class.clone(), object_value.clone()),
            PerlValue::Object { class, value: Box::new(object_value) },
        );
    }
}
