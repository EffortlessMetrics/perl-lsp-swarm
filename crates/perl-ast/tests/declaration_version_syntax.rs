//! Discriminating proof for the shared declaration VERSION source contract (#10716).
//!
//! These tests fail realistic wrong implementations:
//! - collapsing decimal and v-string into one "version" form
//! - normalizing the spelling, so `1.230` and `1.23` compare equal
//! - accepting a spelling that is not what the range actually covers
//! - deriving the range from the spelling instead of the other way round
//! - letting a recovered reading claim exactness
//! - treating a recovered/unknown version as absence
//! - accepting an inverted, out-of-bounds, or char-splitting range
//!
//! They do not cover parser population (#11089), package/class node layout
//! (#10753 / #10762), or any version comparison, ordering, or activation
//! semantics — none of which belong to `perl-ast`.

use perl_ast::SourceLocation;
use perl_ast::ast::{
    DeclarationVersionDisposition, DeclarationVersionForm, DeclarationVersionSyntax,
    DeclarationVersionSyntaxError,
};

fn from(
    form: DeclarationVersionForm,
    source: &str,
    start: usize,
    end: usize,
) -> Result<DeclarationVersionSyntax, DeclarationVersionSyntaxError> {
    DeclarationVersionSyntax::from_source(form, source, SourceLocation { start, end })
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
    let decimal = from(DeclarationVersionForm::Decimal, "package A 1.002003;", 10, 18);
    let vstring = from(DeclarationVersionForm::VString, "package A v1.2.3;", 10, 16);
    assert_eq!(raw_of(&decimal), Ok("1.002003".to_string()));
    assert_eq!(raw_of(&vstring), Ok("v1.2.3".to_string()));
    assert_ne!(decimal, vstring);

    // Identical spelling and geometry, different form: still distinct, because
    // an exact reading must never compare equal to a recovered one.
    let source = "package A v1.2.3;";
    let exact = from(DeclarationVersionForm::VString, source, 10, 16);
    let recovered = from(DeclarationVersionForm::RecoveredOrUnknown, source, 10, 16);
    assert_eq!(raw_of(&exact), Ok("v1.2.3".to_string()));
    assert_eq!(raw_of(&recovered), Ok("v1.2.3".to_string()));
    assert_ne!(exact, recovered);
}

// DVS-002 — the spelling is retained byte for byte; no normalization.
#[test]
fn spelling_is_retained_byte_for_byte() {
    let padded = from(DeclarationVersionForm::Decimal, "package A 1.230;", 10, 15);
    let bare = from(DeclarationVersionForm::Decimal, "package A 1.23;", 10, 14);
    assert_eq!(raw_of(&padded), Ok("1.230".to_string()));
    assert_eq!(raw_of(&bare), Ok("1.23".to_string()));
    assert_ne!(padded, bare);

    // Leading zeros are spelling, not value.
    let leading = from(DeclarationVersionForm::Decimal, "package A 0.001;", 10, 15);
    assert_eq!(raw_of(&leading), Ok("0.001".to_string()));
}

// DVS-003 — the spelling is derived from source geometry, never supplied.
//
// Every expectation below is an independently written literal, not a slice of
// the same `source` taken with the same offsets. Moving the range moves the
// text, which is what proves the range is load-bearing rather than a label
// carried alongside a string.
#[test]
fn spelling_is_derived_from_the_range_not_supplied() {
    let source = "package Demo 1.23;";

    assert_eq!(raw_of(&from(DeclarationVersionForm::Decimal, source, 13, 17)), Ok("1.23".into()));
    assert_eq!(raw_of(&from(DeclarationVersionForm::Decimal, source, 13, 16)), Ok("1.2".into()));
    // Under the recovered form — the only one that admits arbitrary text —
    // the geometry alone decides the spelling, including spans that are not
    // version-shaped at all.
    let recovered = DeclarationVersionForm::RecoveredOrUnknown;
    assert_eq!(raw_of(&from(recovered, source, 14, 17)), Ok(".23".into()));
    assert_eq!(raw_of(&from(recovered, source, 8, 12)), Ok("Demo".into()));
    assert_eq!(raw_of(&from(recovered, source, 0, 7)), Ok("package".into()));

    // The retained range is the one supplied, not one recomputed from the text.
    let value = from(DeclarationVersionForm::Decimal, source, 13, 17);
    assert_eq!(value.as_ref().map(|v| (v.range().start, v.range().end)), Ok((13, 17)));
}

// DVS-004 — a range past the end of the source is rejected.
#[test]
fn range_past_the_end_of_source_is_rejected() {
    let source = "package Demo 1.23;";
    assert_eq!(source.len(), 18);
    assert_eq!(
        from(DeclarationVersionForm::Decimal, source, 13, 25),
        Err(DeclarationVersionSyntaxError::RangeOutOfBounds { start: 13, end: 25, source_len: 18 })
    );
    // Recovery does not relax the geometry check.
    assert_eq!(
        from(DeclarationVersionForm::RecoveredOrUnknown, source, 13, 19),
        Err(DeclarationVersionSyntaxError::RangeOutOfBounds { start: 13, end: 19, source_len: 18 })
    );
    // The exact end of the source is in bounds.
    assert_eq!(
        raw_of(&from(DeclarationVersionForm::RecoveredOrUnknown, source, 13, 18)),
        Ok("1.23;".into())
    );
}

// DVS-005 — an inverted range is rejected, and rejected before any bounds or
// slicing arithmetic that would underflow or mis-report.
#[test]
fn inverted_range_is_rejected_before_bounds_arithmetic() {
    let source = "package Demo 1.23;";
    assert_eq!(
        from(DeclarationVersionForm::Decimal, source, 17, 13),
        Err(DeclarationVersionSyntaxError::InvertedRange { start: 17, end: 13 })
    );
    // Inverted *and* out of bounds: the inversion must win, so the diagnostic
    // names the real defect rather than a derived one.
    assert_eq!(
        from(DeclarationVersionForm::RecoveredOrUnknown, source, 25, 13),
        Err(DeclarationVersionSyntaxError::InvertedRange { start: 25, end: 13 })
    );
}

// DVS-006 — an exact form cannot cover zero bytes; a zero-width reading is
// only representable as recovered.
#[test]
fn exact_forms_require_a_spelling_but_recovery_may_be_zero_width() {
    let source = "package Demo 1.23;";
    assert_eq!(
        from(DeclarationVersionForm::Decimal, source, 13, 13),
        Err(DeclarationVersionSyntaxError::EmptyExactSpelling {
            form: DeclarationVersionForm::Decimal
        })
    );
    assert_eq!(
        from(DeclarationVersionForm::VString, source, 13, 13),
        Err(DeclarationVersionSyntaxError::EmptyExactSpelling {
            form: DeclarationVersionForm::VString
        })
    );
    let zero_width = from(DeclarationVersionForm::RecoveredOrUnknown, source, 13, 13);
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

    let source = "package Demo 1.2.3;";
    let recovered = from(DeclarationVersionForm::RecoveredOrUnknown, source, 13, 18);
    assert_eq!(
        recovered.as_ref().map(DeclarationVersionSyntax::disposition),
        Ok(DeclarationVersionDisposition::Recovered)
    );
    assert_eq!(recovered.as_ref().map(DeclarationVersionSyntax::is_exact), Ok(false));

    let exact = from(DeclarationVersionForm::Decimal, "package Demo 1.23;", 13, 17);
    assert_eq!(exact.as_ref().map(DeclarationVersionSyntax::is_exact), Ok(true));
}

// DVS-008 — an unreadable version is not an absent version.
#[test]
fn an_unknown_version_is_not_absence() {
    let absent: Option<DeclarationVersionSyntax> = None;
    let unknown = from(DeclarationVersionForm::RecoveredOrUnknown, "package Demo ;", 13, 13).ok();

    assert!(unknown.is_some());
    assert_ne!(absent, unknown);

    // The recovered reading still carries where the parser looked.
    assert_eq!(unknown.as_ref().map(|v| v.range().start), Some(13));
}

// DVS-009 — multi-byte source preserves byte geometry, and a range that
// splits a character is rejected rather than silently truncated.
#[test]
fn multibyte_source_preserves_byte_geometry_and_rejects_split_characters() {
    let source = "package Café 1.23;";
    // "package " is 8 bytes; "Café" is 5 (é is two); the space puts the
    // version at bytes 14..18, not chars 11..15.
    assert_eq!(source.find("1.23"), Some(14));
    assert_eq!(raw_of(&from(DeclarationVersionForm::Decimal, source, 14, 18)), Ok("1.23".into()));

    // "class " is 6 bytes; "Ünïcøde" is 10 (Ü, ï, ø are two each).
    let vstring_source = "class Ünïcøde v1.2.3 { }";
    assert_eq!(vstring_source.find("v1.2.3"), Some(17));
    assert_eq!(
        raw_of(&from(DeclarationVersionForm::VString, vstring_source, 17, 23)),
        Ok("v1.2.3".into())
    );

    // Byte 6..7 lands inside "Ü".
    assert_eq!(
        from(DeclarationVersionForm::RecoveredOrUnknown, vstring_source, 6, 7),
        Err(DeclarationVersionSyntaxError::RangeNotOnCharBoundary { start: 6, end: 7 })
    );

    // A recovered reading may itself span multi-byte text. The retained
    // spelling is measured in bytes, not chars: "vé1" is 4 bytes over 3 chars,
    // so a char-counting length would disagree with this geometry.
    let recovered_source = "package Demo vé1;";
    assert_eq!(recovered_source.find("vé1"), Some(13));
    let recovered = from(DeclarationVersionForm::RecoveredOrUnknown, recovered_source, 13, 17);
    assert_eq!(raw_of(&recovered), Ok("vé1".into()));
    assert_eq!(recovered.as_ref().map(|v| v.raw().len()), Ok(4));
    assert_eq!(recovered.as_ref().map(|v| v.raw().chars().count()), Ok(3));
}

// DVS-010 — the deterministic projection is form-tagged and never normalized.
#[test]
fn display_projection_is_deterministic_and_form_tagged() {
    let decimal = from(DeclarationVersionForm::Decimal, "package Demo 1.23;", 13, 17);
    let vstring = from(DeclarationVersionForm::VString, "package Demo v1.2.3;", 13, 19);
    let recovered = from(DeclarationVersionForm::RecoveredOrUnknown, "package Demo 1.2.3;", 13, 18);

    assert_eq!(decimal.as_ref().map(ToString::to_string), Ok("decimal:1.23@13..17".to_string()));
    assert_eq!(vstring.as_ref().map(ToString::to_string), Ok("vstring:v1.2.3@13..19".to_string()));
    assert_eq!(
        recovered.as_ref().map(ToString::to_string),
        Ok("recovered:1.2.3@13..18".to_string())
    );

    // Same spelling, different form: the projection must still discriminate.
    let source = "package A v1.2.3;";
    let exact_v = from(DeclarationVersionForm::VString, source, 10, 16);
    let recovered_v = from(DeclarationVersionForm::RecoveredOrUnknown, source, 10, 16);
    assert_ne!(
        exact_v.as_ref().map(ToString::to_string),
        recovered_v.as_ref().map(ToString::to_string)
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

    let value = from(DeclarationVersionForm::Decimal, "package Demo 1.23;", 13, 17).ok();
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
    let out_of_bounds =
        DeclarationVersionSyntaxError::RangeOutOfBounds { start: 13, end: 25, source_len: 18 };
    let split = DeclarationVersionSyntaxError::RangeNotOnCharBoundary { start: 6, end: 7 };
    let empty =
        DeclarationVersionSyntaxError::EmptyExactSpelling { form: DeclarationVersionForm::VString };

    // Full expected strings, not digit co-occurrence: a message that swapped
    // which number it labels would still "contain" both digits, so substring
    // checks cannot prove attribution.
    assert_eq!(inverted.to_string(), "declaration VERSION range 17..13 ends before it starts");
    assert_eq!(
        out_of_bounds.to_string(),
        "declaration VERSION range 13..25 runs past the 18-byte source"
    );
    assert_eq!(split.to_string(), "declaration VERSION range 6..7 splits a multi-byte character");
    assert!(
        empty.to_string().starts_with("declaration VERSION form `vstring` requires a spelling")
    );
}

// DVS-013 — a caller cannot substitute a spelling for the one the range covers.
//
// The review counterexample on PR #13827: a producer supplies `9.99` against
// `package Demo 1.23;` at 13..17 — same length, wrong content. There is no
// constructor that accepts a spelling at all, so the value built at that range
// reports the source's own text and the substitution is unrepresentable.
#[test]
fn a_caller_cannot_substitute_a_spelling_for_the_covered_source() {
    let source = "package Demo 1.23;";
    let value = from(DeclarationVersionForm::Decimal, source, 13, 17);

    assert_eq!(raw_of(&value), Ok("1.23".to_string()));
    assert_ne!(raw_of(&value), Ok("9.99".to_string()));

    // Two different sources at the same range are different values, so the
    // range alone can never stand in for the text.
    let other = from(DeclarationVersionForm::Decimal, "package Demo 9.99;", 13, 17);
    assert_eq!(raw_of(&other), Ok("9.99".to_string()));
    assert_ne!(value, other);
}

// DVS-014 — an exact form only admits its own spelling; recovery is the only
// escape.
//
// Blocking review finding on PR #13827: exactness was derived from the enum
// tag alone, so `Decimal` over `v1.2.3` and `VString` over arbitrary text were
// both accepted as exact. A tag a caller simply asserts is not a checked
// invariant.
#[test]
fn exact_forms_reject_cross_tag_and_malformed_spellings() {
    // Decimal tag over a v-string spelling.
    assert_eq!(
        from(DeclarationVersionForm::Decimal, "package A v1.2.3;", 10, 16),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::Decimal,
            start: 10,
            end: 16
        })
    );
    // Decimal tag over a single-component v-string: rejected on the `v`
    // itself, not merely on the dot count.
    assert_eq!(
        from(DeclarationVersionForm::Decimal, "package A v5;", 10, 12),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::Decimal,
            start: 10,
            end: 12
        })
    );
    // Decimal tag over a three-part spelling, which is a v-string in Perl.
    assert_eq!(
        from(DeclarationVersionForm::Decimal, "package A 1.2.3;", 10, 15),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::Decimal,
            start: 10,
            end: 15
        })
    );
    // VString tag over arbitrary text.
    assert_eq!(
        from(DeclarationVersionForm::VString, "package Demo garbage;", 13, 20),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::VString,
            start: 13,
            end: 20
        })
    );
    // VString tag over a plain decimal, which is not a v-string.
    assert_eq!(
        from(DeclarationVersionForm::VString, "package A 1.23;", 10, 14),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::VString,
            start: 10,
            end: 14
        })
    );
    // A malformed exact spelling: trailing dot, no fractional digits.
    assert_eq!(
        from(DeclarationVersionForm::Decimal, "package A 1.;", 10, 12),
        Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm {
            form: DeclarationVersionForm::Decimal,
            start: 10,
            end: 12
        })
    );

    // Every rejected spelling above is representable as recovered — that is
    // the escape, and it is the only one.
    for (source, start, end) in [
        ("package A v1.2.3;", 10, 16),
        ("package A 1.2.3;", 10, 15),
        ("package Demo garbage;", 13, 20),
        ("package A 1.;", 10, 12),
    ] {
        assert!(
            from(DeclarationVersionForm::RecoveredOrUnknown, source, start, end).is_ok(),
            "recovery must admit {source}[{start}..{end}]"
        );
    }

    // The real spellings each form exists for are still accepted. Every row in
    // both tables below is the observed verdict of `perl -e 'package A <v>; 1;'`
    // on Perl 5.38.2 — see `.spec/10716-declaration-version-syntax/acceptance.md`.
    for (form, spelling) in [
        (DeclarationVersionForm::Decimal, "0"),
        (DeclarationVersionForm::Decimal, "1"),
        (DeclarationVersionForm::Decimal, "10"),
        (DeclarationVersionForm::Decimal, "0.0"),
        (DeclarationVersionForm::Decimal, "1.0"),
        (DeclarationVersionForm::Decimal, "1.23"),
        (DeclarationVersionForm::Decimal, "0.001"),
        (DeclarationVersionForm::Decimal, "5.036"),
        (DeclarationVersionForm::Decimal, "10.5"),
        (DeclarationVersionForm::VString, "v1.2.3"),
        (DeclarationVersionForm::VString, "v1.2.3.4"),
        (DeclarationVersionForm::VString, "v0.0.0"),
        // Leading zeros are rejected only in the first component.
        (DeclarationVersionForm::VString, "v1.02.3"),
    ] {
        let source = format!("package A {spelling};");
        assert!(
            from(form, &source, 10, 10 + spelling.len()).is_ok(),
            "perl accepts `package A {spelling};` so {form:?} must record it exactly"
        );
    }

    // Spellings Perl rejects in a declaration header are not exact readings,
    // whichever exact tag is offered.
    for (form, spelling) in [
        (DeclarationVersionForm::Decimal, "00"),
        (DeclarationVersionForm::Decimal, "01"),
        (DeclarationVersionForm::Decimal, "1_2"),
        (DeclarationVersionForm::Decimal, "1.23_45"),
        (DeclarationVersionForm::Decimal, ".5"),
        (DeclarationVersionForm::Decimal, "1.2.3.4"),
        // `v5` and `v1.2` have fewer than the three parts Perl requires.
        (DeclarationVersionForm::VString, "v5"),
        (DeclarationVersionForm::VString, "v1.2"),
        (DeclarationVersionForm::VString, "v"),
        (DeclarationVersionForm::VString, "vv1.2.3"),
        (DeclarationVersionForm::VString, "v01.2.3"),
        (DeclarationVersionForm::VString, "v1.2.3_4"),
        // A dotted-decimal must begin with `v`, so the bare form is not one.
        (DeclarationVersionForm::VString, "1.2.3"),
    ] {
        let source = format!("package A {spelling};");
        let end = 10 + spelling.len();
        assert_eq!(
            from(form, &source, 10, end),
            Err(DeclarationVersionSyntaxError::SpellingDoesNotMatchForm { form, start: 10, end }),
            "perl rejects `package A {spelling};` so {form:?} must not record it exactly"
        );
        assert!(
            from(DeclarationVersionForm::RecoveredOrUnknown, &source, 10, end).is_ok(),
            "`{spelling}` must remain representable as recovered"
        );
    }
}

// DVS-015 — the one-line projection survives recovered text containing
// newlines, tabs, and other control characters.
//
// Review finding on PR #13827: a recovered reading may cover arbitrary source,
// so an unescaped newline would split one value across two log or receipt
// records while the doc comment promised one line.
#[test]
fn display_projection_escapes_control_characters_in_recovered_text() {
    // A real newline, tab and carriage return in the covered span.
    let source = "package A 1\n2\t3\r4;";
    let value = from(DeclarationVersionForm::RecoveredOrUnknown, source, 10, 17);
    assert_eq!(raw_of(&value), Ok("1\n2\t3\r4".to_string()));

    let rendered = value.as_ref().map(ToString::to_string);
    assert_eq!(rendered, Ok("recovered:1\\n2\\t3\\r4@10..17".to_string()));
    // The whole projection really is one line.
    assert_eq!(rendered.as_ref().map(|text| text.lines().count()), Ok(1));

    // Other control characters get a deterministic escape too.
    let nul_source = "package A a\u{0}b;";
    assert_eq!(
        from(DeclarationVersionForm::RecoveredOrUnknown, nul_source, 10, 13)
            .as_ref()
            .map(ToString::to_string),
        Ok("recovered:a\\u{0}b@10..13".to_string())
    );

    // The escape character itself is escaped, so the rendering is unambiguous:
    // a literal two-character `\n` in source does not render as a newline escape.
    let literal_source = "package A a\\nb;";
    assert_eq!(
        from(DeclarationVersionForm::RecoveredOrUnknown, literal_source, 10, 14)
            .as_ref()
            .map(ToString::to_string),
        Ok("recovered:a\\\\nb@10..14".to_string())
    );

    // `raw()` keeps the real bytes; only the projection escapes.
    assert_eq!(value.as_ref().map(|v| v.raw().contains('\n')), Ok(true));

    // Ordinary version spellings are untouched by the escaping.
    assert_eq!(
        from(DeclarationVersionForm::Decimal, "package A 1.23;", 10, 14)
            .as_ref()
            .map(ToString::to_string),
        Ok("decimal:1.23@10..14".to_string())
    );
}
