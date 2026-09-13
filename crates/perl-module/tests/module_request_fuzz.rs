//! Deterministic fuzz proof for validated module requests (#8497).
//!
//! Validation runs on untrusted source text, so arbitrary input must be
//! classified rather than trusted, and must never panic, slice mid-codepoint,
//! or produce a request that outruns its evidence.

use perl_module::{
    ModuleFilePath, ModuleName, ModuleRequest, is_lookup_safe_module_name, module_name_to_path,
};

fn next_u64(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

/// Alphabet weighted toward the characters that decide the grammar.
///
/// `:` and `'` are repeated so separator-shaped inputs occur often enough to
/// exercise the accepting side of the grammar, not just the rejecting side.
const ALPHABET: &str =
    "AbZy09_::''//\\..$@% \t\n\0-\"\u{3bb}\u{754c}\u{e9}\u{301}\u{200d}\u{a0}\u{1f600}";

fn fuzz_string(alphabet: &[char], state: &mut u64, max_len: usize) -> String {
    let len = (next_u64(state) as usize) % max_len;
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        let index = (next_u64(state) as usize) % alphabet.len();
        out.push(alphabet[index]);
    }
    out
}

#[test]
fn fuzz_module_requests_are_classified_without_panicking() {
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    let mut seed = 0x5EED_1234_ABCD_0001_u64;

    for _ in 0..20_000 {
        let text = fuzz_string(&alphabet, &mut seed, 24);

        let name = ModuleName::parse(&text);
        let file = ModuleFilePath::parse(&text);
        let bareword = ModuleRequest::bareword(&text);
        let quoted = ModuleRequest::quoted_require(&text);

        assert_eq!(
            name.is_ok(),
            bareword.is_ok(),
            "bareword classification must follow module-name validation for {text:?}"
        );
        assert_eq!(
            file.is_ok(),
            quoted.is_ok(),
            "quoted classification must follow file-path validation for {text:?}"
        );
        assert_eq!(
            name.is_ok(),
            is_lookup_safe_module_name(&text),
            "the lookup-safe predicate must stay the boolean projection for {text:?}"
        );

        if let Ok(name) = &name {
            assert!(!name.canonical().is_empty());
            assert!(
                !name.canonical().contains('\''),
                "a canonical name never keeps the legacy separator: {text:?}"
            );
            assert!(
                !name.canonical().contains('/') && !name.canonical().contains('\\'),
                "a validated name never carries a path separator: {text:?}"
            );
            // The relative path derived from a validated name stays relative.
            let relative = module_name_to_path(name.canonical());
            assert!(!relative.starts_with('/'), "derived path must stay relative: {relative:?}");
            assert!(
                !relative.split('/').any(|component| component == ".."),
                "derived path must never traverse: {relative:?}"
            );
        }

        if let Ok(file) = &file {
            assert_eq!(file.literal(), text, "the literal spelling is preserved exactly");
            assert!(
                !file.with_forward_separators().split('/').any(|component| component == ".."),
                "a validated file request never traverses: {text:?}"
            );
        }

        if let Ok(request) = &quoted {
            assert!(
                request.module_name().is_none(),
                "a quoted operand must never gain a module identity: {text:?}"
            );
        }
    }
}

#[test]
fn fuzz_validated_names_reparse_identically() {
    let alphabet: Vec<char> = ALPHABET.chars().collect();
    let mut seed = 0x5EED_1234_ABCD_0002_u64;

    for _ in 0..20_000 {
        let text = fuzz_string(&alphabet, &mut seed, 24);
        let Ok(name) = ModuleName::parse(&text) else {
            continue;
        };

        let reparsed = ModuleName::parse(name.canonical());
        assert_eq!(
            reparsed.as_ref().map(ModuleName::canonical),
            Ok(name.canonical()),
            "a canonical spelling must revalidate to itself: {text:?}"
        );
    }
}
