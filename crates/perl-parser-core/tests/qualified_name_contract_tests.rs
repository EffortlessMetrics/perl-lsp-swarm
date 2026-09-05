use perl_parser_core::qualified_name::{
    QualifiedNameError, container_name, is_valid_identifier_part, split_qualified_name,
    validate_perl_qualified_name,
};

#[test]
fn reports_exact_failure_identity_and_segment_index() {
    let cases = [
        ("", QualifiedNameError::EmptyName),
        ("$value", QualifiedNameError::LeadingSigil('$')),
        ("@items", QualifiedNameError::LeadingSigil('@')),
        ("%lookup", QualifiedNameError::LeadingSigil('%')),
        ("&handler", QualifiedNameError::LeadingSigil('&')),
        ("*glob", QualifiedNameError::LeadingSigil('*')),
        ("::Foo", QualifiedNameError::EmptySegment { index: 0 }),
        ("Foo::", QualifiedNameError::EmptySegment { index: 1 }),
        ("Foo::::Bar", QualifiedNameError::EmptySegment { index: 1 }),
        ("3Foo::Bar", QualifiedNameError::InvalidSegment { index: 0 }),
        ("Foo::Bar-Baz", QualifiedNameError::InvalidSegment { index: 1 }),
    ];

    for (name, expected) in cases {
        assert_eq!(
            validate_perl_qualified_name(name),
            Err(expected),
            "unexpected validation result for {name:?}"
        );
    }
}

#[test]
fn renders_actionable_failure_messages() {
    let cases = [
        (QualifiedNameError::EmptyName, "name is empty"),
        (QualifiedNameError::LeadingSigil('$'), "qualified name cannot start with sigil '$'"),
        (
            QualifiedNameError::EmptySegment { index: 2 },
            "segment 2 is empty (leading/trailing/double separator)",
        ),
        (QualifiedNameError::InvalidSegment { index: 3 }, "segment 3 is not a valid identifier"),
    ];

    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }
}

#[test]
fn splits_at_the_final_package_separator_without_validating() {
    assert_eq!(split_qualified_name("Foo::Bar::baz"), (Some("Foo::Bar"), "baz"));
    assert_eq!(split_qualified_name("::Foo"), (Some(""), "Foo"));
    assert_eq!(split_qualified_name("Foo::"), (Some("Foo"), ""));
    assert_eq!(container_name("Foo::Bar::baz"), Some("Foo::Bar"));
}

#[test]
fn distinguishes_identifier_start_and_continuation_rules() {
    for valid in ["_", "_9", "A9", "π2", "日本_2"] {
        assert!(is_valid_identifier_part(valid), "expected {valid:?} to be valid");
    }

    for invalid in ["9name", "name-with-dash", "name with space", "$name", "Foo::Bar"] {
        assert!(!is_valid_identifier_part(invalid), "expected {invalid:?} to be invalid");
    }
}

#[test]
fn preserves_unicode_segments_and_reports_separator_boundaries() {
    assert_eq!(validate_perl_qualified_name("Müller::日本::π2"), Ok(()));

    let cases = [
        ("Müller::", QualifiedNameError::EmptySegment { index: 1 }),
        ("日本::::π", QualifiedNameError::EmptySegment { index: 1 }),
        ("Müller::bad-name", QualifiedNameError::InvalidSegment { index: 1 }),
        ("π/β", QualifiedNameError::InvalidSegment { index: 0 }),
    ];

    for (name, expected) in cases {
        assert_eq!(validate_perl_qualified_name(name), Err(expected));
    }
}
