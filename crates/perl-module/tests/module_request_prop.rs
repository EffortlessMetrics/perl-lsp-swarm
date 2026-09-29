//! Property proof for validated module requests (#8497).
//!
//! The load-bearing property is the **grammar-equivalence oracle**:
//! `is_lookup_safe_module_name` was reimplemented on top of `ModuleName::parse`,
//! and it gates production reference extraction
//! (`perl-module/src/reference/mod.rs`). If the new validated constructor
//! accepted or rejected one string differently from the predicate it replaced,
//! go-to-definition behaviour would change silently. `legacy_lookup_safe_oracle`
//! below is a verbatim copy of the pre-#8497 implementation and is the negative
//! control for that refactor.

use perl_module::{
    LegacySeparatorProfile, ModuleFilePath, ModuleName, ModuleRequest, PackageSeparatorForm,
    is_lookup_safe_module_name, normalize_package_separator,
};
use proptest::prelude::*;

/// Verbatim copy of `is_lookup_safe_module_name` as it stood before #8497.
///
/// This is the oracle, not a helper: it must never be "fixed" to agree with the
/// implementation. If it diverges, the refactor changed behaviour.
fn legacy_lookup_safe_oracle(module_name: &str) -> bool {
    fn is_perl_word_char(ch: char) -> bool {
        static WORD_RE: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
        let Some(regex) = WORD_RE.get_or_init(|| regex::Regex::new(r"^\w$").ok()).as_ref() else {
            return false;
        };

        regex.is_match(ch.encode_utf8(&mut [0; 4]))
    }
    fn is_identifier_start(ch: char) -> bool {
        ch == '_' || (unicode_ident::is_xid_start(ch) && is_perl_word_char(ch))
    }
    fn is_identifier_continue(ch: char) -> bool {
        ch == '_' || (unicode_ident::is_xid_continue(ch) && is_perl_word_char(ch))
    }
    fn is_module_identifier_segment(segment: &str) -> bool {
        let mut chars = segment.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        is_identifier_start(first) && chars.all(is_identifier_continue)
    }

    if module_name.is_empty() {
        return false;
    }
    if module_name
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '\\' | '$' | '@' | '%'))
    {
        return false;
    }

    let normalized = normalize_package_separator(module_name);
    normalized.split("::").all(|part| part != ".." && is_module_identifier_segment(part))
}

/// Hand-picked strings that sit exactly on the grammar's decision boundaries.
///
/// A random generator over a hostile alphabet is *not* discriminating here:
/// almost every such string is rejected by both implementations, so equivalence
/// holds vacuously. These cases exercise the accepting side and the near-misses
/// where a drift would actually change go-to-definition behaviour.
const BOUNDARY_CORPUS: &[&str] = &[
    // accepted
    "Foo",
    "Foo::Bar",
    "Foo::Bar::Baz",
    "strict",
    "_",
    "_Private::Util",
    "Foo9",
    "C::Foo",
    "X::Y::Z",
    "λ::Ж",
    "界",
    "Foo::Bar\u{0301}",
    // legacy separator — accepted, and the profile default must not change that
    "Foo'Bar",
    "A'B'C",
    "A'B::C",
    "Foo::Bar'Baz",
    // near-misses
    "",
    "9Foo",
    "Foo::",
    "::Foo",
    "Foo::::Bar",
    "Foo:Bar",
    "a:b",
    "C:",
    "C:foo",
    "C:/foo",
    "..",
    ".",
    "Foo::..::Bar",
    "Foo::.",
    "../../etc/passwd",
    "$Foo",
    "@Foo",
    "%Foo",
    "Foo Bar",
    "Foo\u{00A0}Bar",
    "Foo\tBar",
    "Foo\nBar",
    "Foo\0Bar",
    "Foo/Bar",
    "Foo\\Bar",
    "Foo\u{200D}Bar",
    "Foo\u{1F600}",
    "'Foo",
    "Foo'",
];

fn valid_segment_strategy() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec!['A', 'z', '_', 'λ', 'Ж', '界', 'é']),
        proptest::collection::vec(
            prop::sample::select(vec!['A', 'z', '0', '9', '_', 'λ', '界', '\u{0301}']),
            0..=4,
        ),
    )
        .prop_map(|(head, tail)| {
            let mut segment = head.to_string();
            segment.extend(tail);
            segment
        })
}

/// Names built from mostly-valid segments joined by mostly-valid separators, so
/// the accepting side of the grammar is exercised as often as the rejecting one.
fn near_valid_name_strategy() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(
            prop_oneof![
                8 => valid_segment_strategy(),
                1 => prop::sample::select(vec!["", ".", "..", "$x", "C:", "9a", "a b"])
                    .prop_map(str::to_string),
            ],
            1..=4,
        ),
        proptest::collection::vec(
            prop_oneof![
                6 => Just("::"),
                4 => Just("'"),
                1 => Just(":"),
                1 => Just("/"),
                1 => Just("."),
            ],
            0..=3,
        ),
    )
        .prop_map(|(segments, separators)| {
            let mut name = segments[0].clone();
            for (separator, segment) in separators.iter().zip(segments.iter().skip(1)) {
                name.push_str(separator);
                name.push_str(segment);
            }
            name
        })
}

/// Characters that sit on or near the grammar's decision boundaries.
const HOSTILE_ALPHABET: &str =
    "Az09_:'/\\.$@% \t\n\0-\"\u{3bb}\u{416}\u{754c}\u{e9}\u{301}\u{200d}\u{a0}\u{1f600}";

fn hostile_char_strategy() -> impl Strategy<Value = char> {
    prop::sample::select(HOSTILE_ALPHABET.chars().collect::<Vec<char>>())
}

fn hostile_string_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => near_valid_name_strategy(),
        1 => proptest::collection::vec(hostile_char_strategy(), 0..=12)
            .prop_map(|chars| chars.into_iter().collect::<String>()),
    ]
}

/// Exhaustive equivalence over the decision-boundary corpus.
///
/// This is the control that actually detects a grammar drift; it is deliberately
/// deterministic so a regression cannot hide behind a lucky seed.
#[test]
fn boundary_corpus_matches_the_pre_refactor_oracle() {
    for text in BOUNDARY_CORPUS {
        assert_eq!(
            is_lookup_safe_module_name(text),
            legacy_lookup_safe_oracle(text),
            "`is_lookup_safe_module_name` drifted for {text:?}"
        );
        assert_eq!(
            ModuleName::parse(text).is_ok(),
            legacy_lookup_safe_oracle(text),
            "`ModuleName::parse` drifted for {text:?}"
        );
    }
}

/// The corpus must actually exercise both sides of the decision.
#[test]
fn boundary_corpus_covers_accepted_and_rejected_names() {
    let accepted = BOUNDARY_CORPUS.iter().filter(|text| legacy_lookup_safe_oracle(text)).count();
    let rejected = BOUNDARY_CORPUS.len() - accepted;

    assert!(accepted >= 15, "corpus must exercise the accepting side, got {accepted}");
    assert!(rejected >= 15, "corpus must exercise the rejecting side, got {rejected}");
}

proptest! {
    /// The refactored predicate must accept exactly what the old one accepted.
    #[test]
    fn lookup_safe_predicate_matches_the_pre_refactor_oracle(text in hostile_string_strategy()) {
        prop_assert_eq!(
            is_lookup_safe_module_name(&text),
            legacy_lookup_safe_oracle(&text),
            "grammar drifted for {:?}",
            text
        );
    }

    /// The same equivalence, stated against the validated constructor directly.
    #[test]
    fn module_name_parse_matches_the_pre_refactor_oracle(text in hostile_string_strategy()) {
        prop_assert_eq!(
            ModuleName::parse(&text).is_ok(),
            legacy_lookup_safe_oracle(&text),
            "ModuleName::parse drifted for {:?}",
            text
        );
    }

    /// Arbitrary text is classified, never panics, and never resolves by accident.
    #[test]
    fn arbitrary_text_is_classified_without_panicking(text in ".{0,64}") {
        let bareword = ModuleRequest::bareword(&text);
        let quoted = ModuleRequest::quoted_require(&text);

        if let Ok(request) = &bareword {
            prop_assert!(request.module_name().is_some());
            prop_assert!(request.literal_file().is_none());
            prop_assert!(request.is_exact());
        }
        if let Ok(request) = &quoted {
            prop_assert!(request.literal_file().is_some());
            prop_assert!(
                request.module_name().is_none(),
                "a quoted operand must never gain a module identity"
            );
        }
        prop_assert_eq!(bareword.is_ok(), is_lookup_safe_module_name(&text));
    }

    /// A validated name round-trips through its canonical spelling.
    #[test]
    fn validated_names_round_trip_through_canonical_spelling(text in hostile_string_strategy()) {
        let Ok(name) = ModuleName::parse(&text) else {
            return Ok(());
        };
        let reparsed = ModuleName::parse(name.canonical());
        prop_assert_eq!(
            reparsed.map(|value| value.canonical().to_string()),
            Ok(name.canonical().to_string())
        );
        prop_assert_eq!(
            name.segments().collect::<Vec<_>>().join("::"),
            name.canonical().to_string()
        );
    }

    /// A literal file request preserves its bytes exactly.
    #[test]
    fn literal_file_requests_preserve_their_bytes(text in hostile_string_strategy()) {
        let Ok(path) = ModuleFilePath::parse(&text) else {
            return Ok(());
        };
        prop_assert_eq!(path.literal(), text.as_str());
    }

    /// The rejecting profile is strictly narrower than the accepting one.
    #[test]
    fn rejecting_profile_is_strictly_narrower(text in hostile_string_strategy()) {
        let accepting = ModuleName::parse_with_profile(&text, LegacySeparatorProfile::Accept);
        let rejecting = ModuleName::parse_with_profile(&text, LegacySeparatorProfile::Reject);

        if rejecting.is_ok() {
            prop_assert!(accepting.is_ok(), "rejecting profile must never widen the accept set");
        }
        if let Ok(name) = &accepting
            && name.separator_form() == PackageSeparatorForm::Canonical
        {
            prop_assert!(
                rejecting.is_ok(),
                "canonical spellings are unaffected by the legacy profile"
            );
        }
    }
}
