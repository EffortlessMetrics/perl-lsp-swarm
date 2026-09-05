//! Contract tests for `MutationStructuredValue.v1` (#11327, S0).
//!
//! Negative controls: bare Perl-looking text without the `json:` prefix stays
//! unsupported; duplicate object keys are rejected (never last-wins); depth,
//! node, entry, scalar-byte, aggregate-byte, digit, and exponent budgets are
//! enforced (aggregate over decoded data bytes, exponents via checked
//! accumulation); integers outside the exact bounded range are refused.
//! Positive controls: nested finite trees parse; multi-byte UTF-8 string data
//! and object keys round-trip exactly; scalars admit no fresh referent while
//! arrays/objects map to fresh ARRAY/HASH referents; fingerprints derive from
//! canonical value serialization and discriminate changed content.
//!
//! #11328 falsifier pins: complete consumption refuses trailing text and
//! JSON-looking comments; strict JSON `\uXXXX` escapes (with exact surrogate
//! pairing) decode to inert string data while raw controls stay illegal;
//! Perl-looking text stays inert inside strings and bare at the prefix gate;
//! an adversarial corpus is total, panic-free, and deterministic under
//! repetition; every refusal path is pure (identical repeated results, no
//! backend seam to observe).
//!
//! Scalar-authority receipt (#11328 required test 10): this module adds only
//! the `json:`-prefixed structured branch beside the scalar
//! `MutationValueText.v1` surface (#10745/#8364). It re-exports no scalar
//! parsing symbol and touches no scalar path; the full perl-dap suite stays
//! green as the standing scalar-behavior authority.

use perl_dap::mutation::{
    FreshReferentKind, MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION, StructuredMutationLimits,
    StructuredRefusal, StructuredValue, fresh_referent_kind, parse_structured_mutation,
    structured_payload,
};
use perl_source_identity::ContentDigest;
use std::fmt::Write as _;

type TestResult<T = ()> = Result<T, String>;

fn parse(text: &str) -> TestResult<StructuredValue> {
    let envelope = parse_structured_mutation(text, &StructuredMutationLimits::default())
        .map_err(|error| error.to_string())?;
    assert_eq!(envelope.schema_version(), MUTATION_STRUCTURED_VALUE_SCHEMA_VERSION);
    Ok(envelope.value().clone())
}

fn parse_envelope(text: &str) -> TestResult<perl_dap::mutation::MutationStructuredValueV1> {
    parse_structured_mutation(text, &StructuredMutationLimits::default())
        .map_err(|error| error.to_string())
}

fn parse_refusal(text: &str) -> TestResult<StructuredRefusal> {
    parse_structured_mutation(text, &StructuredMutationLimits::default())
        .err()
        .ok_or_else(|| format!("expected {text:?} to be refused"))
}

#[test]
fn missing_json_prefix_refuses_bare_perl_looking_text() -> TestResult {
    for bare in ["[1, 2]", "{ \"a\": 1 }", "\\@array", "sub { 42 }", " $x ", "JSON:[1]"] {
        let error = parse_structured_mutation(bare, &StructuredMutationLimits::default())
            .err()
            .ok_or_else(|| format!("bare text {bare:?} must be refused"))?;
        assert_eq!(
            error,
            StructuredRefusal::MissingStructuredPrefix,
            "bare text {bare:?} must refuse with the prefix error"
        );
    }
    // The prefix is byte-exact and required at offset zero.
    assert_eq!(structured_payload("json:[]"), Ok("[]"));
    assert!(structured_payload(" json:[]").is_err());
    Ok(())
}

#[test]
fn nested_finite_tree_parses_with_scalars() -> TestResult {
    let value = parse(r#"json:{"a": [1, -2, null, true, false, "s"], "b": {"c": 1.5e2}}"#)?;
    let StructuredValue::Object(entries) = &value else {
        return Err("expected object root".to_string());
    };
    assert_eq!(entries.len(), 2);
    let StructuredValue::Array(items) = &entries[0].1 else {
        return Err("expected array under key a".to_string());
    };
    assert_eq!(items[0], StructuredValue::Integer(1));
    assert_eq!(items[1], StructuredValue::Integer(-2));
    assert_eq!(items[2], StructuredValue::Null);
    assert_eq!(items[3], StructuredValue::Bool(true));
    assert_eq!(items[4], StructuredValue::Bool(false));
    assert_eq!(items[5], StructuredValue::String("s".to_string()));
    assert!(matches!(value, StructuredValue::Object(_)));
    Ok(())
}

#[test]
fn duplicate_object_keys_are_rejected_not_last_wins() -> TestResult {
    let error = parse_refusal(r#"json:{"k": 1, "k": 2}"#)?;
    assert_eq!(error, StructuredRefusal::DuplicateKey { key: "k".to_string() });
    Ok(())
}

#[test]
fn depth_budget_is_enforced() -> TestResult {
    let mut deep = String::from("json:");
    for _ in 0..=17 {
        deep.push('[');
    }
    for _ in 0..=17 {
        deep.push(']');
    }
    let error = parse_refusal(&deep)?;
    assert_eq!(error, StructuredRefusal::DepthExceeded { limit: 16 });
    Ok(())
}

#[test]
fn node_budget_is_enforced() -> TestResult {
    let limits = StructuredMutationLimits { max_nodes: 4, ..StructuredMutationLimits::default() };
    let error = parse_structured_mutation("json:[1,2,3,4,5]", &limits)
        .err()
        .ok_or("over-wide tree must fail")?;
    assert_eq!(error, StructuredRefusal::TooManyNodes { limit: 4 });
    Ok(())
}

#[test]
fn entry_budget_is_enforced_per_container() -> TestResult {
    let limits = StructuredMutationLimits { max_entries: 3, ..StructuredMutationLimits::default() };
    let error = parse_structured_mutation(r#"json:{"a":1,"b":2,"c":3,"d":4}"#, &limits)
        .err()
        .ok_or("over-wide object must fail")?;
    assert_eq!(error, StructuredRefusal::TooManyEntries { limit: 3 });
    Ok(())
}

#[test]
fn integer_out_of_exact_range_is_refused_without_float_fallback() -> TestResult {
    let mut beyond = String::from("json:-9");
    let _ = write!(beyond, "{}", "9".repeat(25));
    let error = parse_structured_mutation(&beyond, &StructuredMutationLimits::default())
        .err()
        .ok_or("beyond-i64 integer must be refused")?;
    assert_eq!(error, StructuredRefusal::IntegerOutOfRange);
    Ok(())
}

#[test]
fn digit_and_exponent_budgets_are_enforced() -> TestResult {
    let limits = StructuredMutationLimits {
        max_significant_digits: 4,
        max_absolute_exponent: 8,
        ..StructuredMutationLimits::default()
    };
    let too_many_digits = parse_structured_mutation("json:1.000001e0", &limits)
        .err()
        .ok_or("digit budget must bind")?;
    assert_eq!(too_many_digits, StructuredRefusal::TooManyDigits { limit: 4 });

    let exponent_too_large =
        parse_structured_mutation("json:1e9", &limits).err().ok_or("exponent budget must bind")?;
    assert_eq!(exponent_too_large, StructuredRefusal::ExponentTooLarge { limit: 8 });

    let within =
        parse_structured_mutation("json:1.5e-8", &limits).map_err(|error| error.to_string())?;
    let expected = perl_dap::mutation::ExactDecimal::admitted("1.5e-8")
        .ok_or("canonical 1.5e-8 must be admissible")?;
    assert_eq!(within.value(), &StructuredValue::Decimal(expected.clone()));
    assert_eq!(expected.canonical(), "1.5e-8");
    Ok(())
}

#[test]
fn scalars_admit_no_fresh_referent_arrays_and_objects_do() -> TestResult {
    for scalar in ["json:null", "json:true", "json:3", r#"json:"text""#, "json:1.25"] {
        let value = parse(scalar)?;
        assert_eq!(fresh_referent_kind(&value), None, "scalars must not create a fresh referent");
    }
    let array = parse("json:[]")?;
    let hash = parse("json:{}")?;
    assert_eq!(fresh_referent_kind(&array), Some(FreshReferentKind::Array));
    assert_eq!(fresh_referent_kind(&hash), Some(FreshReferentKind::Hash));
    Ok(())
}

#[test]
fn ordering_is_deterministic_and_fingerprints_stable() -> TestResult {
    let first = parse_envelope(r#"json:{"z": 1, "a": [true, null]}"#)?;
    let second = parse_envelope(r#"json:{"z": 1, "a": [true, null]}"#)?;
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_ne!(first.fingerprint(), &ContentDigest::of_bytes(b""));
    // Object entry order is preserved as written (deterministic receipt-safe
    // ordering), not silently re-sorted.
    let StructuredValue::Object(entries) = first.value() else {
        return Err("expected object".to_string());
    };
    assert_eq!(entries[0].0, "z");
    assert_eq!(entries[1].0, "a");
    Ok(())
}

#[test]
fn fingerprint_derives_from_canonical_value_serialization() -> TestResult {
    // Distinct admitted content must never share a digest; the constant
    // empty-content sentinel made this pass vacuously before.
    let one = parse_envelope("json:1")?;
    let two = parse_envelope("json:2")?;
    assert_ne!(one.fingerprint(), two.fingerprint());
    assert_eq!(
        one.fingerprint(),
        &ContentDigest::of_bytes(
            serde_json::to_string(one.value()).map_err(|error| error.to_string())?.as_bytes()
        )
    );

    // Identical content reproduces the digest deterministically.
    let again = parse_envelope("json:2")?;
    assert_eq!(two.fingerprint(), again.fingerprint());

    // Documented entry order participates: same members, different order.
    let z_then_a = parse_envelope(r#"json:{"z": 1, "a": 2}"#)?;
    let a_then_z = parse_envelope(r#"json:{"a": 2, "z": 1}"#)?;
    assert_ne!(z_then_a.fingerprint(), a_then_z.fingerprint());
    Ok(())
}

#[test]
fn multibyte_utf8_strings_and_keys_round_trip_exactly() -> TestResult {
    let value = parse(r#"json:"café 🦀""#)?;
    assert_eq!(value, StructuredValue::String("café 🦀".to_string()));

    let object = parse(r#"json:{"é": "ü"}"#)?;
    let StructuredValue::Object(entries) = &object else {
        return Err("expected object root".to_string());
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "é");
    assert_eq!(entries[0].1, StructuredValue::String("ü".to_string()));
    Ok(())
}

#[test]
fn oversized_exponent_refuses_without_overflowing() -> TestResult {
    for literal in ["json:1e18446744073709551616", "json:1e99999999999999999999"] {
        let error = parse_structured_mutation(literal, &StructuredMutationLimits::default())
            .err()
            .ok_or_else(|| format!("{literal} must refuse"))?;
        assert_eq!(
            error,
            StructuredRefusal::ExponentTooLarge { limit: 4_096 },
            "{literal} must map checked overflow to the exponent refusal"
        );
    }
    Ok(())
}

#[test]
fn aggregate_budget_counts_decoded_data_bytes_not_entry_counts() -> TestResult {
    // The canonical falsifier from review: with a one-byte aggregate budget,
    // entry-count accounting charges zero and lets this payload through.
    let tight =
        StructuredMutationLimits { max_aggregate_bytes: 1, ..StructuredMutationLimits::default() };

    let object_payload = parse_structured_mutation(r#"json:{"key":"arbitrarily long"}"#, &tight)
        .err()
        .ok_or("string payload and key bytes must be charged against the aggregate")?;
    assert_eq!(object_payload, StructuredRefusal::AggregateTooLarge { limit: 1 });

    let array_payload = parse_structured_mutation(r#"json:["arbitrarily long"]"#, &tight)
        .err()
        .ok_or("array string bytes must be charged against the aggregate")?;
    assert_eq!(array_payload, StructuredRefusal::AggregateTooLarge { limit: 1 });

    // Numeric text is charged too: one digit fits a two-byte budget only
    // because container delimiters are part of decoded data as well.
    let two_bytes =
        StructuredMutationLimits { max_aggregate_bytes: 2, ..StructuredMutationLimits::default() };
    let single_digit = parse_structured_mutation("json:[7]", &two_bytes)
        .err()
        .ok_or("delimiters plus numeric text must reach the two-byte budget")?;
    assert_eq!(single_digit, StructuredRefusal::AggregateTooLarge { limit: 2 });

    // A realistic small payload still admits under a generous budget.
    let generous = StructuredMutationLimits {
        max_aggregate_bytes: 65_536,
        ..StructuredMutationLimits::default()
    };
    let admitted = parse_structured_mutation(r#"json:{"key":"value"}"#, &generous)
        .map_err(|error| error.to_string())?;
    assert!(matches!(admitted.value(), StructuredValue::Object(_)));
    Ok(())
}

#[test]
fn raw_control_characters_are_rejected_inside_strings() -> TestResult {
    // Raw control characters (U+0000–U+001F) are illegal in a json: string
    // and must arrive through the escape branch instead.
    for raw in ["json:\"a\nb\"", "json:\"a\tb\"", "json:\"a\0b\"", "json:\"a\u{1f}b\""] {
        let error = parse_refusal(raw)?;
        assert!(
            matches!(error, StructuredRefusal::InvalidSyntax { .. }),
            "raw control character in {raw:?} must refuse as invalid syntax"
        );
    }
    // Keys are strings too: the same rule binds object keys.
    let raw_key = parse_refusal("json:{\"a\nb\": 1}")?;
    assert!(matches!(raw_key, StructuredRefusal::InvalidSyntax { .. }));
    // The escaped spellings stay legal through the escape branch.
    let escaped = parse(r#"json:"a\nb""#)?;
    assert_eq!(escaped, StructuredValue::String("a\nb".to_string()));
    Ok(())
}

#[test]
fn malformed_number_forms_are_rejected() -> TestResult {
    // None of these is a valid JSON number.
    for malformed in ["json:-e1", "json:-.1", "json:1.", "json:01", "json:+1", "json:."] {
        let error = parse_refusal(malformed)?;
        assert!(
            matches!(error, StructuredRefusal::InvalidSyntax { .. }),
            "{malformed:?} must refuse as invalid syntax, got {error:?}"
        );
    }
    // The neighboring valid forms still admit.
    for valid in ["json:-0.5", "json:0.5", "json:0", "json:-0", "json:1.5e-8", "json:1e2"] {
        parse(valid).map_err(|error| format!("valid number {valid:?} refused: {error}"))?;
    }
    Ok(())
}

#[test]
fn empty_containers_and_literals_are_charged_to_the_aggregate() -> TestResult {
    let zero =
        StructuredMutationLimits { max_aggregate_bytes: 0, ..StructuredMutationLimits::default() };
    for payload in ["json:[]", "json:{}", "json:null", "json:true", "json:false"] {
        let error = parse_structured_mutation(payload, &zero)
            .err()
            .ok_or_else(|| format!("{payload} must not admit under a zero aggregate budget"))?;
        assert_eq!(
            error,
            StructuredRefusal::AggregateTooLarge { limit: 0 },
            "{payload} delimiters/literal bytes must charge the aggregate"
        );
    }
    // A budget that exactly covers the two delimiters admits the empty array.
    let two =
        StructuredMutationLimits { max_aggregate_bytes: 2, ..StructuredMutationLimits::default() };
    let empty_array =
        parse_structured_mutation("json:[]", &two).map_err(|error| error.to_string())?;
    assert!(matches!(empty_array.value(), StructuredValue::Array(_)));
    // Four bytes cover `null`.
    let four =
        StructuredMutationLimits { max_aggregate_bytes: 4, ..StructuredMutationLimits::default() };
    let null = parse_structured_mutation("json:null", &four).map_err(|error| error.to_string())?;
    assert_eq!(null.value(), &StructuredValue::Null);
    Ok(())
}

#[test]
fn widened_limits_beyond_the_pinned_profile_are_refused() -> TestResult {
    let widened = StructuredMutationLimits {
        max_input_bytes: usize::MAX,
        ..StructuredMutationLimits::default()
    };
    let error = parse_structured_mutation("json:1", &widened)
        .err()
        .ok_or("a widened profile must not be admitted under the v1 schema identity")?;
    assert_eq!(error, StructuredRefusal::LimitsExceedPinnedProfile);

    // Tighter-than-pinned profiles remain admissible (boundary tests rely on
    // them); every widened budget refuses the same way.
    let tightened =
        StructuredMutationLimits { max_nodes: 2, ..StructuredMutationLimits::default() };
    parse_structured_mutation("json:[1]", &tightened).map_err(|error| error.to_string())?;
    for widened in [
        StructuredMutationLimits {
            max_scalar_bytes: 32_769,
            ..StructuredMutationLimits::default()
        },
        StructuredMutationLimits {
            max_aggregate_bytes: 65_537,
            ..StructuredMutationLimits::default()
        },
        StructuredMutationLimits { max_depth: 17, ..StructuredMutationLimits::default() },
        StructuredMutationLimits { max_nodes: 1_025, ..StructuredMutationLimits::default() },
        StructuredMutationLimits { max_entries: 513, ..StructuredMutationLimits::default() },
        StructuredMutationLimits {
            max_significant_digits: 257,
            ..StructuredMutationLimits::default()
        },
        StructuredMutationLimits {
            max_absolute_exponent: 4_097,
            ..StructuredMutationLimits::default()
        },
    ] {
        let error = parse_structured_mutation("json:1", &widened)
            .err()
            .ok_or("widened budget must refuse")?;
        assert_eq!(error, StructuredRefusal::LimitsExceedPinnedProfile);
    }
    Ok(())
}

#[test]
fn exact_decimal_construction_is_checked() -> TestResult {
    for invalid in ["1.", ".1", "01", "-", "-e1", "1e", "1.5e+", "+1", ""] {
        assert!(
            perl_dap::mutation::ExactDecimal::admitted(invalid).is_none(),
            "non-canonical {invalid:?} must not be admissible"
        );
    }
    for valid in ["0", "-0", "1.5", "-0.5", "1e2", "1.5e-8", "12"] {
        assert!(
            perl_dap::mutation::ExactDecimal::admitted(valid).is_some(),
            "canonical {valid:?} must be admissible"
        );
    }
    Ok(())
}

#[test]
fn complete_consumption_refuses_trailing_text_and_comments() -> TestResult {
    // Any non-whitespace byte after one complete value must refuse as invalid
    // syntax at the exact stopping offset; JSON has no comment grammar.
    let trailing = parse_refusal("json:[1] trailing")?;
    assert_eq!(trailing, StructuredRefusal::InvalidSyntax { offset: 4 });
    let object_trailing = parse_refusal(r#"json:{"a":1} x"#)?;
    assert_eq!(object_trailing, StructuredRefusal::InvalidSyntax { offset: 8 });
    let line_comment = parse_refusal("json:[1] // comment")?;
    assert_eq!(line_comment, StructuredRefusal::InvalidSyntax { offset: 4 });
    let block_comment = parse_refusal("json:[/*c*/1]")?;
    assert_eq!(block_comment, StructuredRefusal::InvalidSyntax { offset: 1 });

    // Trailing whitespace is not trailing text: completion skips it.
    let whitespace_only = parse("json:[1]   ").map_err(|error| error.to_string())?;
    assert!(matches!(whitespace_only, StructuredValue::Array(_)));
    Ok(())
}

#[test]
fn escaped_control_and_unicode_escapes_decode_to_inert_data() -> TestResult {
    // Strict JSON escapes decode to inert string DATA; they are never
    // interpreted as Perl syntax or delimiters.
    let cases = [
        (r#"json:"\n""#, "\n"),
        (r#"json:"\t""#, "\t"),
        (r#"json:"\r""#, "\r"),
        (r#"json:"\b\f""#, "\u{8}\u{c}"),
        (r#"json:"\u0000""#, "\0"),
        (r#"json:"\u0001""#, "\u{1}"),
        (r#"json:"\u001f""#, "\u{1f}"),
        (r#"json:"A\u00e9B""#, "A\u{e9}B"),
        (r#"json:"\u00C9""#, "\u{c9}"),
        (r#"json:"\ud83d\ude00""#, "\u{1f600}"),
    ];
    for (text, expected) in cases {
        let value = parse(text).map_err(|error| format!("{text:?} refused: {error}"))?;
        assert_eq!(
            value,
            StructuredValue::String(expected.to_string()),
            "{text:?} must decode to inert data"
        );
    }

    // An escape-produced delimiter stays data: it never terminates a string
    // or opens structure, and an escaped backslash never starts an escape.
    let quoted = parse(r#"json:"\u0022""#).map_err(|error| error.to_string())?;
    assert_eq!(quoted, StructuredValue::String("\"".to_string()));
    let two_quoted = parse(r#"json:["\u0022\u0022"]"#).map_err(|error| error.to_string())?;
    assert_eq!(
        two_quoted,
        StructuredValue::Array(vec![StructuredValue::String("\"\"".to_string())])
    );
    let literal_backslash_n = parse(r#"json:"a\u005Cnb""#).map_err(|error| error.to_string())?;
    assert_eq!(literal_backslash_n, StructuredValue::String("a\\nb".to_string()));

    // The paired-escape spelling and the direct UTF-8 spelling are the same
    // admitted value down to the fingerprint.
    let escaped = parse_envelope(r#"json:"\ud83d\ude00""#)?;
    let direct = parse_envelope("json:\"\u{1f600}\"")?;
    assert_eq!(escaped, direct);
    Ok(())
}

#[test]
fn invalid_unicode_escapes_refuse_deterministically() -> TestResult {
    // Offsets are payload-relative: the parser runs on the text after the
    // `json:` prefix is stripped, so the opening quote sits at 0 and the first
    // escape backslash at 1. A syntactically valid but non-low-surrogate
    // continuation refuses at the pair's leading backslash; only a *malformed*
    // continuation hex4 reports the continuation backslash (first + 6).
    for (malformed, expected_offset) in [
        (r#"json:"\u12""#, 1),
        (r#"json:"\uzzzz""#, 1),
        (r#"json:"\ud800""#, 1),
        (r#"json:"\udbff""#, 1),
        (r#"json:"\udc00""#, 1),
        (r#"json:"\udfff""#, 1),
        (r#"json:"\ud800\ud800""#, 1),
        (r#"json:"\udbff\uffff""#, 1),
        // Malformed continuation hex4 (non-hex, then truncated): these are the
        // only shapes that report the continuation backslash at 1 + 6 = 7.
        (r#"json:"\ud800\uzzzz""#, 7),
        (r#"json:"\ud800\u12""#, 7),
        (r#"json:"\ud800x""#, 1),
        (r#"json:"\ud83d\n""#, 1),
        (r#"json:"\ud83d""#, 1),
    ] {
        let error = parse_refusal(malformed)?;
        assert_eq!(
            error,
            StructuredRefusal::InvalidSyntax { offset: expected_offset },
            "{malformed:?} must report the offending escape backslash"
        );
    }
    Ok(())
}

#[test]
fn perl_looking_text_stays_inert_in_strings_and_bare_at_prefix_gate() -> TestResult {
    let inert_cases = [
        (r#"json:["%{$x} = @INC"]"#, "%{$x} = @INC"),
        (r#"json:["\\&undef"]"#, "\\&undef"),
        (r#"json:["sub f { @@ }"]"#, "sub f { @@ }"),
    ];
    for (text, expected) in inert_cases {
        let value = parse(text).map_err(|error| format!("{text:?} refused: {error}"))?;
        let StructuredValue::Array(items) = &value else {
            return Err(format!("{text:?} must parse to an array"));
        };
        let [StructuredValue::String(decoded)] = items.as_slice() else {
            return Err(format!("{text:?} must hold exactly one string"));
        };
        assert_eq!(decoded, expected, "Perl-looking text must stay inert string data");
    }
    for bare in [
        "%{$x} = @INC",
        "\\&undef",
        "sub f { @@ }",
        "@INC",
        "keys %hash",
        "$x =~ y/a/b/",
        "sort { $a <=> $b } @list",
    ] {
        let error = parse_refusal(bare)?;
        assert_eq!(
            error,
            StructuredRefusal::MissingStructuredPrefix,
            "bare Perl text {bare:?} must refuse at the prefix gate"
        );
    }
    Ok(())
}

#[test]
fn adversarial_corpus_is_total_and_deterministic() -> TestResult {
    let corpus: [(&str, bool); 15] = [
        ("", false),
        ("json:", false),
        (r#"json:"\ud800""#, false),
        (r#"json:"\udfff""#, false),
        (r#"json:"\ud800\ud800""#, false),
        (r#"json:"\udbff\uffff""#, false),
        (r#"json:"\udc00\ud800""#, false),
        (r#"json:"\u0000""#, true),
        ("json:-", false),
        ("json:01", false),
        ("json:1e", false),
        (r#"json:"unterminated"#, false),
        (r#"{"a": [1, {"b": null}]}"#, false),
        (r#"json:["🦀", "café"]"#, true),
        ("json:null", true),
    ];
    for (text, admits) in corpus {
        let limits = StructuredMutationLimits::default();
        let first = parse_structured_mutation(text, &limits);
        let second = parse_structured_mutation(text, &limits);
        assert_eq!(first, second, "repeated parses of {text:?} must be identical");
        assert_eq!(first.is_ok(), admits, "{text:?} must classify deterministically");
    }

    let built: [(String, bool); 5] = [
        (format!("json:{}", "[".repeat(64)), false),
        (format!("json:{}", "9".repeat(500)), false),
        (format!("json:0.{}", "1".repeat(300)), false),
        (format!("json:1e{}", "9".repeat(40)), false),
        (format!("json:\"{}\"", "🦀".repeat(40)), true),
    ];
    for (text, admits) in built {
        let limits = StructuredMutationLimits::default();
        let first = parse_structured_mutation(&text, &limits);
        let second = parse_structured_mutation(&text, &limits);
        assert_eq!(first, second, "repeated parses of {text:?} must be identical");
        assert_eq!(first.is_ok(), admits, "{text:?} must classify deterministically");
    }

    // Multi-byte scalars straddling a tightened byte budget refuse typed
    // instead of splitting or panicking; the exact boundary still admits.
    let scalar_two =
        StructuredMutationLimits { max_scalar_bytes: 2, ..StructuredMutationLimits::default() };
    let aggregate_one =
        StructuredMutationLimits { max_aggregate_bytes: 1, ..StructuredMutationLimits::default() };
    let aggregate_two =
        StructuredMutationLimits { max_aggregate_bytes: 2, ..StructuredMutationLimits::default() };
    let aggregate_three =
        StructuredMutationLimits { max_aggregate_bytes: 3, ..StructuredMutationLimits::default() };
    let budgeted: [(StructuredMutationLimits, &str, bool); 6] = [
        (scalar_two, r#"json:"é""#, true),
        (scalar_two, r#"json:"€""#, false),
        (aggregate_one, r#"json:"a""#, true),
        (aggregate_one, r#"json:"ab""#, false),
        (aggregate_two, r#"json:"€""#, false),
        (aggregate_three, r#"json:"€""#, true),
    ];
    for (limits, text, admits) in budgeted {
        let first = parse_structured_mutation(text, &limits);
        let second = parse_structured_mutation(text, &limits);
        assert_eq!(first, second, "repeated parses of {text:?} must be identical");
        assert_eq!(first.is_ok(), admits, "{text:?} must classify deterministically");
    }
    Ok(())
}

#[test]
fn refusal_paths_are_pure_repeated_calls_are_identical() -> TestResult {
    // Required-test 12 receipt (#11328): there is no backend seam by design.
    // Every public entry point takes only shared references and returns owned
    // data; the module holds no statics or interior mutability, so no state is
    // observable between calls. The compile-time signature pin below fails to
    // compile if any free function ever takes a `&mut` parameter, and the
    // repeated-call identities prove each refusal is a pure function of its
    // inputs (zero backend/state calls on every refusal path).
    type ParseFn = fn(
        &str,
        &StructuredMutationLimits,
    )
        -> Result<perl_dap::mutation::MutationStructuredValueV1, StructuredRefusal>;
    type PayloadFn = fn(&str) -> Result<&str, StructuredRefusal>;
    type ReferentKindFn = fn(&StructuredValue) -> Option<FreshReferentKind>;
    let shared_reference_signatures_only: (ParseFn, PayloadFn, ReferentKindFn) =
        (parse_structured_mutation, structured_payload, fresh_referent_kind);
    let _ = shared_reference_signatures_only;

    let widened = StructuredMutationLimits {
        max_input_bytes: usize::MAX,
        ..StructuredMutationLimits::default()
    };
    let nodes_two =
        StructuredMutationLimits { max_nodes: 2, ..StructuredMutationLimits::default() };
    let zero_aggregate =
        StructuredMutationLimits { max_aggregate_bytes: 0, ..StructuredMutationLimits::default() };
    let scalar_one =
        StructuredMutationLimits { max_scalar_bytes: 1, ..StructuredMutationLimits::default() };
    let entries_one =
        StructuredMutationLimits { max_entries: 1, ..StructuredMutationLimits::default() };
    let digits_one = StructuredMutationLimits {
        max_significant_digits: 1,
        ..StructuredMutationLimits::default()
    };
    let exponent_one = StructuredMutationLimits {
        max_absolute_exponent: 1,
        ..StructuredMutationLimits::default()
    };

    let long_input = format!("json:{}", "1".repeat(70_000));
    let deep_input = format!("json:{}", "[".repeat(18));
    let beyond_integer = format!("json:-9{}", "9".repeat(25));

    let cases: Vec<(StructuredMutationLimits, String, StructuredRefusal)> = vec![
        (
            StructuredMutationLimits::default(),
            "bare Perl-looking text".to_string(),
            StructuredRefusal::MissingStructuredPrefix,
        ),
        (
            StructuredMutationLimits::default(),
            long_input,
            StructuredRefusal::InputTooLarge { limit: 65_536 },
        ),
        (widened, "json:1".to_string(), StructuredRefusal::LimitsExceedPinnedProfile),
        (
            StructuredMutationLimits::default(),
            deep_input,
            StructuredRefusal::DepthExceeded { limit: 16 },
        ),
        (nodes_two, "json:[1,2,3]".to_string(), StructuredRefusal::TooManyNodes { limit: 2 }),
        (zero_aggregate, "json:[]".to_string(), StructuredRefusal::AggregateTooLarge { limit: 0 }),
        (scalar_one, r#"json:"ab""#.to_string(), StructuredRefusal::ScalarTooLarge { limit: 1 }),
        (entries_one, "json:[1,2]".to_string(), StructuredRefusal::TooManyEntries { limit: 1 }),
        (
            StructuredMutationLimits::default(),
            r#"json:{"k":1,"k":2}"#.to_string(),
            StructuredRefusal::DuplicateKey { key: "k".to_string() },
        ),
        (digits_one, "json:1.25".to_string(), StructuredRefusal::TooManyDigits { limit: 1 }),
        (exponent_one, "json:1e2".to_string(), StructuredRefusal::ExponentTooLarge { limit: 1 }),
        (StructuredMutationLimits::default(), beyond_integer, StructuredRefusal::IntegerOutOfRange),
        (
            StructuredMutationLimits::default(),
            "json:[1] x".to_string(),
            StructuredRefusal::InvalidSyntax { offset: 4 },
        ),
    ];
    for (limits, text, expected) in cases {
        let first = parse_structured_mutation(&text, &limits);
        let second = parse_structured_mutation(&text, &limits);
        let refusal = first.as_ref().err().ok_or_else(|| format!("{text:?} must be refused"))?;
        assert_eq!(refusal, &expected, "{text:?} must refuse with the pinned variant");
        assert_eq!(
            first, second,
            "{text:?} must produce identical refusals on repeated pure calls"
        );
    }

    // The admitted path is equally repeatable: same input, same envelope.
    let admitted_first = parse_structured_mutation(
        r#"json:{"a":[true,null]}"#,
        &StructuredMutationLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let admitted_second = parse_structured_mutation(
        r#"json:{"a":[true,null]}"#,
        &StructuredMutationLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(admitted_first, admitted_second);
    Ok(())
}
