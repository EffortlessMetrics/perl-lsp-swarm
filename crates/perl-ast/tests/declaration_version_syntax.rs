//! Discriminating proof for the shared declaration VERSION source contract (#10716).
//!
//! These tests fail realistic wrong implementations:
//! - collapsing decimal and v-string into one "version" form
//! - normalizing the spelling, so `1.230` and `1.23` compare equal
//! - dropping the raw spelling or the exact range and reconstructing one
//!   from the other
//! - letting a recovered reading claim exactness
//! - treating a recovered/unknown version as absence
//! - accepting a range that does not cover the spelling, or an inverted range
//!
//! They do not cover parser population (#11089), package/class node layout
//! (#10753 / #10762), or any version comparison, ordering, or activation
//! semantics — none of which belong to `perl-ast`.

use perl_ast::SourceLocation;
use perl_ast::ast::{
    DeclarationVersionDisposition, DeclarationVersionForm, DeclarationVersionSyntax,
    DeclarationVersionSyntaxError,
};

fn make(
    form: DeclarationVersionForm,
    raw: &str,
    start: usize,
    end: usize,
) -> Result<DeclarationVersionSyntax, DeclarationVersionSyntaxError> {
    DeclarationVersionSyntax::new(form, raw, SourceLocation { start, end })
}

fn raw_of(
    value: &Result<DeclarationVersionSyntax, DeclarationVersionSyntaxError>,
) -> Result<String, DeclarationVersionSyntaxError> {
    value.as_ref().map(|v| v.raw().to_string()).map_err(|e| *e)
}

// DVS-001 — decimal and v-string are different source forms, and a recovered
// reading of the same text is a third, distinct value.
#[test]
fn decimal_vstring_and_recovered_readings_never_collapse() {
    // `1.002003` and `v1.2.3` are the same version to Perl's semantics.
    // They must remain different *source* values here.
    let decimal = make(DeclarationVersionForm::Decimal, "1.002003", 0, 8);
    let vstring = make(DeclarationVersionForm::VString, "v1.2.3", 0, 6);
    assert!(decimal.is_ok());
    assert!(vstring.is_ok());
    assert_ne!(decimal, vstring);

    // Identical spelling and geometry, different form: still distinct, because
    // an exact reading must never compare equal to a recovered one.
    let exact = make(DeclarationVersionForm::VString, "v5", 0, 2);
    let recovered = make(DeclarationVersionForm::RecoveredOrUnknown, "v5", 0, 2);
    assert!(exact.is_ok());
    assert!(recovered.is_ok());
    assert_ne!(exact, recovered);
}

// DVS-002 — the raw spelling is retained byte for byte; no normalization.
#[test]
fn spelling_is_retained_byte_for_byte() {
    let padded = make(DeclarationVersionForm::Decimal, "1.230", 0, 5);
    let bare = make(DeclarationVersionForm::Decimal, "1.23", 0, 4);
    assert_eq!(raw_of(&padded), Ok("1.230".to_string()));
    assert_eq!(raw_of(&bare), Ok("1.23".to_string()));
    assert_ne!(padded, bare);

    // Leading zeros are spelling, not value.
    let leading = make(DeclarationVersionForm::Decimal, "0.001", 0, 5);
    assert_eq!(raw_of(&leading), Ok("0.001".to_string()));
}

// DVS-003 — the exact range is retained, and it agrees with real source text.
#[test]
fn range_is_exact_and_agrees_with_source_text() {
    let cases: [(&str, DeclarationVersionForm, usize, usize); 3] = [
        ("package Demo 1.23;", DeclarationVersionForm::Decimal, 13, 17),
        ("package Demo v1.2.3;", DeclarationVersionForm::VString, 13, 19),
        ("class Demo 0.001 { }", DeclarationVersionForm::Decimal, 11, 16),
    ];

    for (source, form, start, end) in cases {
        let spelling = &source[start..end];
        let value = make(form, spelling, start, end);
        assert_eq!(raw_of(&value), Ok(spelling.to_string()));
        assert_eq!(
            value.as_ref().map(|v| (v.range().start, v.range().end)),
            Ok((start, end)),
            "range must survive construction for {source}"
        );
        // Fidelity oracle: the retained spelling is the source slice, so the
        // spelling and the range cannot have been reconstructed from each other.
        assert_eq!(
            value.as_ref().map(|v| v.raw() == &source[v.range().start..v.range().end]),
            Ok(true)
        );
    }
}

// DVS-004 — a range that does not cover the spelling is rejected.
#[test]
fn range_that_does_not_cover_the_spelling_is_rejected() {
    assert_eq!(
        make(DeclarationVersionForm::Decimal, "1.23", 13, 18),
        Err(DeclarationVersionSyntaxError::RangeLengthMismatch { raw_len: 4, range_len: 5 })
    );
    assert_eq!(
        make(DeclarationVersionForm::Decimal, "1.23", 13, 16),
        Err(DeclarationVersionSyntaxError::RangeLengthMismatch { raw_len: 4, range_len: 3 })
    );
    // Recovery does not relax the geometry check.
    assert_eq!(
        make(DeclarationVersionForm::RecoveredOrUnknown, "1.2.", 0, 9),
        Err(DeclarationVersionSyntaxError::RangeLengthMismatch { raw_len: 4, range_len: 9 })
    );
}

// DVS-005 — an inverted range is rejected, and rejected before any length
// arithmetic that would underflow.
#[test]
fn inverted_range_is_rejected_without_arithmetic_underflow() {
    assert_eq!(
        make(DeclarationVersionForm::Decimal, "1.23", 17, 13),
        Err(DeclarationVersionSyntaxError::InvertedRange { start: 17, end: 13 })
    );
    assert_eq!(
        make(DeclarationVersionForm::RecoveredOrUnknown, "", 9, 4),
        Err(DeclarationVersionSyntaxError::InvertedRange { start: 9, end: 4 })
    );
}

// DVS-006 — an exact form cannot be zero-width; a zero-width reading is only
// representable as recovered.
#[test]
fn exact_forms_require_a_spelling_but_recovery_may_be_zero_width() {
    assert_eq!(
        make(DeclarationVersionForm::Decimal, "", 5, 5),
        Err(DeclarationVersionSyntaxError::EmptyExactSpelling {
            form: DeclarationVersionForm::Decimal
        })
    );
    assert_eq!(
        make(DeclarationVersionForm::VString, "", 5, 5),
        Err(DeclarationVersionSyntaxError::EmptyExactSpelling {
            form: DeclarationVersionForm::VString
        })
    );
    let zero_width = make(DeclarationVersionForm::RecoveredOrUnknown, "", 5, 5);
    assert!(zero_width.is_ok());
    assert_eq!(raw_of(&zero_width), Ok(String::new()));
}

// DVS-007 — recovery is never exact, and the disposition is derived from the
// form rather than stored beside it.
#[test]
fn recovered_readings_are_never_exact() {
    assert_eq!(DeclarationVersionForm::Decimal.disposition(), DeclarationVersionDisposition::Exact);
    assert_eq!(DeclarationVersionForm::VString.disposition(), DeclarationVersionDisposition::Exact);
    assert_ne!(
        DeclarationVersionForm::RecoveredOrUnknown.disposition(),
        DeclarationVersionDisposition::Exact
    );

    let recovered = make(DeclarationVersionForm::RecoveredOrUnknown, "1.2.3", 0, 5);
    assert_eq!(
        recovered.as_ref().map(DeclarationVersionSyntax::disposition),
        Ok(DeclarationVersionDisposition::Recovered)
    );
    assert_eq!(recovered.as_ref().map(DeclarationVersionSyntax::is_exact), Ok(false));

    let exact = make(DeclarationVersionForm::Decimal, "1.23", 0, 4);
    assert_eq!(exact.as_ref().map(DeclarationVersionSyntax::is_exact), Ok(true));
}

// DVS-008 — an unreadable version is not an absent version.
#[test]
fn an_unknown_version_is_not_absence() {
    let absent: Option<DeclarationVersionSyntax> = None;
    let unknown = make(DeclarationVersionForm::RecoveredOrUnknown, "", 12, 12).ok();

    assert!(unknown.is_some());
    assert_ne!(absent, unknown);

    // The recovered reading still carries where the parser looked.
    assert_eq!(unknown.as_ref().map(|v| v.range().start), Some(12));
}

// DVS-009 — multi-byte source before the version does not disturb geometry.
#[test]
fn multibyte_source_preserves_exact_byte_geometry() {
    let source = "package Café 1.23;";
    let start = 14;
    let end = 18;
    assert_eq!(&source[start..end], "1.23");

    let value = make(DeclarationVersionForm::Decimal, &source[start..end], start, end);
    assert_eq!(raw_of(&value), Ok("1.23".to_string()));
    assert_eq!(value.as_ref().map(|v| (v.range().start, v.range().end)), Ok((start, end)));

    // A version whose own spelling is compared by bytes, not chars.
    // "class " is 6 bytes; "Ünïcøde" is 10 bytes (Ü, ï, ø are two bytes each);
    // the separating space puts the v-string at bytes 17..23, not chars 14..20.
    let vstring_source = "class Ünïcøde v1.2.3 { }";
    let v_start = 17;
    let v_end = 23;
    assert_eq!(vstring_source.find("v1.2.3"), Some(v_start));
    assert_eq!(&vstring_source[v_start..v_end], "v1.2.3");
    let vstring =
        make(DeclarationVersionForm::VString, &vstring_source[v_start..v_end], v_start, v_end);
    assert_eq!(raw_of(&vstring), Ok("v1.2.3".to_string()));
}

// DVS-010 — the deterministic projection is form-tagged and never normalized.
#[test]
fn display_projection_is_deterministic_and_form_tagged() {
    let decimal = make(DeclarationVersionForm::Decimal, "1.23", 13, 17);
    let vstring = make(DeclarationVersionForm::VString, "v1.2.3", 13, 19);
    let recovered = make(DeclarationVersionForm::RecoveredOrUnknown, "1.2.3", 13, 18);

    assert_eq!(decimal.as_ref().map(ToString::to_string), Ok("decimal:1.23@13..17".to_string()));
    assert_eq!(vstring.as_ref().map(ToString::to_string), Ok("vstring:v1.2.3@13..19".to_string()));
    assert_eq!(
        recovered.as_ref().map(ToString::to_string),
        Ok("recovered:1.2.3@13..18".to_string())
    );

    // Same spelling, different form: the projection must still discriminate.
    let exact_v5 = make(DeclarationVersionForm::VString, "v5", 0, 2);
    let recovered_v5 = make(DeclarationVersionForm::RecoveredOrUnknown, "v5", 0, 2);
    assert_ne!(
        exact_v5.as_ref().map(ToString::to_string),
        recovered_v5.as_ref().map(ToString::to_string)
    );

    // Repeated rendering of one value is stable.
    assert_eq!(
        decimal.as_ref().map(ToString::to_string),
        decimal.as_ref().map(ToString::to_string)
    );
}

// DVS-011 — one type serves both declaration owners with no conversion.
#[test]
fn one_value_embeds_in_package_and_class_owners_without_conversion() {
    struct PackageDeclarationOwner {
        version: Option<DeclarationVersionSyntax>,
    }
    struct ClassDeclarationOwner {
        version: Option<DeclarationVersionSyntax>,
    }

    let value = make(DeclarationVersionForm::Decimal, "1.23", 13, 17).ok();
    assert!(value.is_some());

    let package_owner = PackageDeclarationOwner { version: value.clone() };
    let class_owner = ClassDeclarationOwner { version: value };

    assert_eq!(package_owner.version, class_owner.version);
    assert_eq!(
        package_owner.version.as_ref().map(DeclarationVersionSyntax::form),
        Some(DeclarationVersionForm::Decimal)
    );
}

// DVS-012 — rejection diagnostics name the offending geometry.
#[test]
fn rejection_diagnostics_are_actionable() {
    let inverted = DeclarationVersionSyntaxError::InvertedRange { start: 17, end: 13 };
    let mismatch = DeclarationVersionSyntaxError::RangeLengthMismatch { raw_len: 4, range_len: 5 };
    let empty =
        DeclarationVersionSyntaxError::EmptyExactSpelling { form: DeclarationVersionForm::VString };

    assert!(inverted.to_string().contains("17..13"));
    assert!(mismatch.to_string().contains('4') && mismatch.to_string().contains('5'));
    assert!(empty.to_string().contains("vstring"));
}
