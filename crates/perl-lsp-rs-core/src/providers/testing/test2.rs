//! Test2 framework awareness — static fact tables for Test2 bundles and tools.
//!
//! This module teaches the LSP about the [Test2](https://metacpan.org/pod/Test2::V0)
//! testing framework: which symbols a `use Test2::V0;` (or a `Test2::Tools::*`
//! module) brings into scope, and whether the import turns on `strict`/`warnings`.
//! It is a *static* fact table, not a Test2 runtime — it does not execute Perl.
//!
//! # Provenance (external-truth gate)
//!
//! The export lists below are verified against the canonical Test2-Suite source
//! rather than reasoned from the diff:
//!
//! - `Test2::V0` default `@EXPORT` and the `use Test2::Tools::* qw/.../;` lines —
//!   `Test-More/Test2-Suite` `lib/Test2/V0.pm`.
//! - Per-tool exports — `lib/Test2/Tools/{Basic,Compare,Subtest,Exception,
//!   Warnings,Class,...}.pm`.
//! - Import-list grammar (`!name` exclusion, `:DEFAULT`/`:ALL` tags,
//!   `name => {-as => 'alias'}` renames, `-prefix`/`-postfix`) — `exodist/Importer`.
//! - `strict`/`warnings` default and the `-no_strict` / `-no_warnings` /
//!   `-no_pragmas` opt-outs — the `Test2::V0` POD.
//! - `Test2::V1` default export (`T2()` only), its pragma model (none by
//!   default; `-strict`/`-warnings`/`-p`/`-pragmas` opt-in), `-import`/`-i`
//!   (bring in the full bare set), and grouped short flags (`-ipP`) — the
//!   `Test2::V1` POD.
//!
//! # Scope model (documented simplification)
//!
//! When an import list explicitly selects symbols (positive barewords, a `qw//`
//! list, or a rename), only those symbols are considered imported — matching
//! `Importer`. Otherwise the module's full default set is used. Positive names
//! are trusted verbatim (added to scope even if not in our table), which keeps
//! the LSP from emitting false "unknown subroutine" diagnostics for tools we do
//! not enumerate. Exclusions and renames are applied on top of the default set.

use regex::Regex;
use std::collections::BTreeSet;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Per-tool export constants (traceable to individual Test2::Tools::* modules).
// ---------------------------------------------------------------------------

/// `Test2::Tools::Basic` — plan/assert/control primitives.
const BASIC: &[&str] = &[
    "ok",
    "pass",
    "fail",
    "diag",
    "note",
    "todo",
    "skip",
    "plan",
    "skip_all",
    "done_testing",
    "bail_out",
];

/// `Test2::Tools::Compare` — the comparison/check DSL as re-exported by
/// `Test2::V0` (the module's own default `@EXPORT` is only `is`/`like`; the rest
/// are `@EXPORT_OK` that V0 pulls in by name).
const COMPARE: &[&str] = &[
    "is",
    "like",
    "isnt",
    "unlike",
    "match",
    "mismatch",
    "validator",
    "hash",
    "array",
    "bag",
    "object",
    "meta",
    "meta_check",
    "number",
    "float",
    "rounded",
    "within",
    "string",
    "subset",
    "bool",
    "check_isa",
    "number_lt",
    "number_le",
    "number_ge",
    "number_gt",
    "in_set",
    "not_in_set",
    "check_set",
    "item",
    "field",
    "call",
    "call_list",
    "call_hash",
    "prop",
    "check",
    "all_items",
    "all_keys",
    "all_vals",
    "all_values",
    "etc",
    "end",
    "filter_items",
    "T",
    "F",
    "D",
    "DF",
    "E",
    "DNE",
    "FDNE",
    "U",
    "L",
    "event",
    "fail_events",
    "exact_ref",
];

/// The `Test2::Tools::Compare` module's *own* default export set (used when the
/// tool module is imported standalone rather than via a bundle).
const COMPARE_OWN_DEFAULT: &[&str] = &["is", "like"];

/// `Test2::Tools::ClassicCompare` — the `Test::More`-style operator compare.
const CLASSIC_COMPARE: &[&str] = &["cmp_ok"];

/// `Test2::Tools::Warnings`.
const WARNINGS: &[&str] = &["warns", "warning", "warnings", "no_warnings"];

/// `Test2::Tools::Class`.
const CLASS: &[&str] = &["can_ok", "isa_ok", "DOES_ok"];

/// `Test2::Tools::Exception`.
const EXCEPTION: &[&str] = &["dies", "lives", "try_ok"];

/// `Test2::Tools::Defer`.
const DEFER: &[&str] = &["def", "do_def"];

/// `Test2::Tools::Mock`.
const MOCK: &[&str] = &["mock", "mocked"];

/// `Test2::Tools::Ref`.
const REF: &[&str] = &["ref_ok", "ref_is", "ref_is_not"];

/// `Test2::Tools::Encoding`.
const ENCODING: &[&str] = &["set_encoding"];

/// `Test2::Tools::Exports`.
const EXPORTS: &[&str] = &["imported_ok", "not_imported_ok"];

/// `Test2::Tools::Refcount`.
const REFCOUNT: &[&str] = &["is_refcount", "is_oneref", "refcount"];

/// `Test2::Tools::Event`.
const EVENT: &[&str] = &["gen_event"];

/// `Test2::API` symbols re-exported by `Test2::V0`.
const API: &[&str] = &["intercept", "context"];

/// `Test2::Tools::Subtest` — the module's *own* default exports. `Test2::V0`
/// renames `subtest_buffered` to the familiar `subtest`, so a bundle exposes
/// `subtest` while the standalone tool exposes the `*_streamed`/`*_buffered`
/// pair.
const SUBTEST_OWN: &[&str] = &["subtest_streamed", "subtest_buffered"];

/// The `subtest` name as exposed by the `Test2::V0` bundle.
const SUBTEST_BUNDLE: &[&str] = &["subtest"];

/// The complete `Test2::V0` default `@EXPORT` set, composed from the tool
/// modules the bundle pulls in. This is the single source of truth for
/// "what does `use Test2::V0;` put in scope". `Test2::V1` reuses this set only
/// under an explicit `-import`/`-i` option.
static V0_DEFAULT: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut v: Vec<&'static str> = Vec::new();
    for group in [
        BASIC,
        COMPARE,
        CLASSIC_COMPARE,
        WARNINGS,
        CLASS,
        EXCEPTION,
        DEFER,
        MOCK,
        REF,
        ENCODING,
        EXPORTS,
        REFCOUNT,
        EVENT,
        API,
        SUBTEST_BUNDLE,
    ] {
        v.extend_from_slice(group);
    }
    v.sort_unstable();
    v.dedup();
    v
});

/// `Test2::V1`'s sole default export: the `T2()` handle. Unlike `Test2::V0`,
/// `Test2::V1` does NOT export the tools as bare subs by default — they are
/// methods on the returned handle (e.g. `T2->ok(...)`, `T2->is(...)`). The bare
/// set is imported only under `-import`/`-i`. Oracle: metacpan `Test2::V1`
/// ("Only 1 export by default: T2()").
const V1_DEFAULT: &[&str] = &["T2"];

// ---------------------------------------------------------------------------
// Module classification.
// ---------------------------------------------------------------------------

/// Whether `module` is any Test2 module the LSP has awareness of.
pub fn is_test2_module(module: &str) -> bool {
    is_test2_bundle(module)
        || module.starts_with("Test2::Tools::")
        || module.starts_with("Test2::Plugin::")
        || module == "Test2::API"
}

/// Whether `module` is a Test2 *bundle* module. Bundles are the recommended
/// entry points (`Test2::V0`, `Test2::V1`, `Test2::Bundle::*`). Note that being
/// a bundle does **not** imply pragmas are on by default — `Test2::V0` enables
/// them by default while `Test2::V1` does not (see `resolve_import`).
pub fn is_test2_bundle(module: &str) -> bool {
    matches!(module, "Test2::V0" | "Test2::V1" | "Test2::Suite")
        || module.starts_with("Test2::Bundle::")
}

/// The default export set for a known Test2 module, or `None` if the module is
/// a Test2 module we recognize structurally but have no enumerated export table
/// for (e.g. a plugin, or an unfamiliar bundle). `None` means "trust explicit
/// imports, otherwise unknown" — callers should not emit unknown-sub
/// diagnostics for such modules.
pub fn module_default_exports(module: &str) -> Option<&'static [&'static str]> {
    // `Test2::V0` re-exports its tools as bare subs — the recommended default set.
    if module == "Test2::V0" {
        return Some(V0_DEFAULT.as_slice());
    }
    // `Test2::V1`'s only *default* export is the `T2()` handle; the bare set is
    // pulled in only under `-import`/`-i` (handled in `resolve_import`). Oracle:
    // metacpan `Test2::V1`.
    if module == "Test2::V1" {
        return Some(V1_DEFAULT);
    }
    let group: &'static [&'static str] = match module {
        "Test2::Tools::Basic" => BASIC,
        "Test2::Tools::Compare" => COMPARE_OWN_DEFAULT,
        "Test2::Tools::ClassicCompare" => CLASSIC_COMPARE,
        "Test2::Tools::Warnings" => WARNINGS,
        "Test2::Tools::Class" => CLASS,
        "Test2::Tools::Exception" => EXCEPTION,
        "Test2::Tools::Defer" => DEFER,
        "Test2::Tools::Mock" => MOCK,
        "Test2::Tools::Ref" => REF,
        "Test2::Tools::Encoding" => ENCODING,
        "Test2::Tools::Exports" => EXPORTS,
        "Test2::Tools::Refcount" => REFCOUNT,
        "Test2::Tools::Event" => EVENT,
        "Test2::Tools::Subtest" => SUBTEST_OWN,
        "Test2::API" => API,
        _ => return None,
    };
    Some(group)
}

/// The reviewed export-plus-export-ok set for a known Test2 module.
///
/// Most currently modeled modules use the same reviewed set for defaults and
/// `:ALL`. `Test2::Tools::Compare` is the important exception: standalone
/// default imports are only `is`/`like`, while the already-reviewed `COMPARE`
/// table records its complete known menu. Unknown/custom modules remain
/// `None` rather than receiving invented names.
fn module_all_exports(module: &str) -> Option<&'static [&'static str]> {
    match module {
        "Test2::Tools::Compare" => Some(COMPARE),
        _ => module_default_exports(module),
    }
}

// ---------------------------------------------------------------------------
// Import resolution.
// ---------------------------------------------------------------------------

/// The `strict`/`warnings` pragma effect an import applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Test2Pragmas {
    /// `strict` is turned on by this import.
    pub strict: bool,
    /// `warnings` is turned on by this import.
    pub warnings: bool,
}

/// The resolved effect of a single Test2 `use` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImport {
    /// Symbols brought into scope by this import.
    pub symbols: BTreeSet<String>,
    /// Pragma effect, present only for bundle imports.
    pub pragmas: Option<Test2Pragmas>,
}

/// Internal result carrying analysis completeness alongside the stable public
/// import shape. The completeness bit is deliberately not part of
/// [`ResolvedImport`]'s public API: this crate is currently 0.17.x, and adding
/// a public field would break downstream struct-literal construction.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedImportAnalysis {
    resolved: ResolvedImport,
    analysis_limited: bool,
}

/// Match `name => { ... -as => 'alias' ... }` renames in an import list.
///
/// `None` when the pattern cannot be compiled. Recognition is a bounded
/// compatibility bridge, not an invariant: a recognizer that fails to build
/// must degrade the affected statement, never abort the language server.
static RENAME_AS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(\w+)\s*=>\s*\{[^}]*?-as\s*=>\s*['"]?(\w+)['"]?[^}]*?\}"#).ok());

/// Match `name => { ... -prefix => 'p' ... }` / `-postfix` renames.
///
/// `None` when the pattern cannot be compiled; see [`RENAME_AS`].
static RENAME_FIX: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(\w+)\s*=>\s*\{[^}]*?-(prefix|postfix)\s*=>\s*['"]?(\w+)['"]?[^}]*?\}"#).ok()
});

/// The transform-option names Importer consumes inside a `name => { ... }`
/// rename map. These are recognized by *role* (an option token immediately
/// followed by `=>`), never as a bareword blacklist: a legitimate exported
/// symbol may share the spelling elsewhere in the import list.
const TRANSFORM_OPTIONS: [&str; 3] = ["-as", "-prefix", "-postfix"];

/// Whether `text` still contains Test2 transform-option syntax.
///
/// This is a deliberately conservative, regex-free detector used only to
/// decide whether to *fail closed*. Over-detection costs a statement its
/// symbols; under-detection would let transform bytes reach the bareword
/// scanner and fabricate imports from syntax atoms. The safe bias is therefore
/// to over-detect.
///
/// It must cover the recognizers: anything [`RENAME_AS`] or [`RENAME_FIX`]
/// matches has to be detected here, or `resolve_import` would take the
/// no-transform path and bareword-scan text those patterns own. Both patterns
/// place the option after `[^}]*?`, which admits a preceding word character,
/// so this deliberately does *not* require a word boundary before the token —
/// only that the token is not a prefix of a longer word (`-aside`) and that it
/// sits in option position. `covers_every_recognizer_match` pins that.
fn contains_transform_syntax(text: &str) -> bool {
    count_transform_options(text) > 0
}

/// How many transform options sit in option position in `text`.
///
/// Shares one role predicate with [`contains_transform_syntax`] so the
/// presence test and the arity test cannot drift apart.
fn count_transform_options(text: &str) -> usize {
    let masked = mask_data_values(text).masked;
    let mut total = 0;

    for option in TRANSFORM_OPTIONS {
        let mut rest = masked.as_str();
        while let Some(offset) = rest.find(option) {
            let after = &rest[offset + option.len()..];
            // The token must not merely open a longer word (`-aside`), and must
            // sit in option position — followed by a fat comma, optionally
            // through a closing quote so the quoted spelling (`{'-as' => ...}`)
            // is recognized too.
            let tail = after.strip_prefix(['\'', '"']).unwrap_or(after);
            if !after.starts_with(|next: char| next.is_alphanumeric() || next == '_')
                && tail.trim_start().starts_with("=>")
            {
                total += 1;
            }
            rest = &rest[offset + option.len()..];
        }
    }
    total
}

/// Whether one recognizer-matched `name => { ... }` entry carries exactly one
/// transform option.
///
/// Each recognizer reads a single option and claims the whole entry, never
/// seeing what else the map holds. A map carrying two options therefore
/// produces one alias per matching recognizer — two symbols for an entry
/// Importer installs under exactly one name (`{-as => 'a', -prefix => 'p'}`
/// yielded both `a` and `p_ok`), or one alias built from a single option when
/// the real name composes both (`{-prefix => 'p', -postfix => 's'}` yielded
/// `p_ok`). Either way a name that does not exist is published, and nothing
/// downstream can catch it: the entry's span is stripped, so the residual scan
/// sees nothing left to object to.
///
/// Which single name Importer composes for such a map is not decided here.
/// This resolver only refuses to guess.
fn entry_carries_one_transform_option(span: &str) -> bool {
    count_transform_options(span) == 1
}

/// Blank the Perl values whose bytes are data rather than option syntax, so
/// the detector's role predicate is not tripped by an option-shaped payload.
///
/// Two forms are masked, each replaced by spaces so surrounding structure and
/// token boundaries survive:
///
/// * complete quote-like expressions (`q{}`, `qq{}`, `qx{}`, `qw//`, `m//`,
///   `s///`, `tr///`, `y///`, `qr//`), which `-target` legitimately takes and
///   which the rest of this module already treats as opaque;
/// * ordinary single- and double-quoted strings.
///
/// A value whose entire payload is one option name is **not** masked, because
/// it is an option key rather than data: `{'-as' => ...}` and `{q{-as} => ...}`
/// evaluate to the same key. A quote-like key is re-emitted as the bare option
/// so the role predicate can see the following `=>`; the span is padded back to
/// its original character length so nothing else shifts.
///
/// Masking a genuine option key would hide it from *both* the detector and the
/// recognizers — the recognizers do not accept the quote-like spelling either —
/// leaving no disagreement for `scan_import_transforms` to fail closed on, and
/// the bareword scan would then report the map's atoms as imports.
///
/// An unterminated string is left visible, so malformed text still reaches the
/// conservative detector rather than being silently swallowed.
fn mask_data_values(text: &str) -> MaskedArgs {
    let mut masked = String::with_capacity(text.len());
    let mut undecidable_key = false;
    let mut index = 0usize;

    while index < text.len() {
        if let Some(end) = quote_like_expression_end(text, index)
            && end > index
        {
            let span = &text[index..end];
            match quote_like_option_key(span) {
                Some(option) => {
                    masked.push_str(option);
                    for _ in option.chars().count()..span.chars().count() {
                        masked.push(' ');
                    }
                }
                None => {
                    if let Some((operator, payload)) = string_quote_payload(span)
                        && !quoted_payload_is_literal(payload, operator == "qq")
                        && is_key_position(text, end)
                    {
                        undecidable_key = true;
                    }
                    blank_into(&mut masked, span);
                }
            }
            index = end;
            continue;
        }

        let Some(current) = text[index..].chars().next() else {
            break;
        };

        // A bare `/.../` match carries no operator, so the quote-like scan above
        // cannot see it, yet its payload is data exactly like `m{...}`. Perl
        // resolves `/` by parse state; here the discriminator is whether a term
        // can start, which keeps division (`$a / $b`) visible.
        if current == '/'
            && bare_match_can_start(&text[..index])
            && let Some(end) = bare_match_end(text, index)
        {
            blank_into(&mut masked, &text[index..end]);
            index = end;
            continue;
        }

        // `$\``, `$'` and `$"` are punctuation-named variables, not delimiters.
        // Pairing them would mask everything up to the next such variable,
        // hiding real transform syntax in between.
        if matches!(current, '\'' | '"' | '`') && text[..index].ends_with('$') {
            masked.push(current);
            index += current.len_utf8();
            continue;
        }

        if current != '\'' && current != '"' && current != '`' {
            masked.push(current);
            index += current.len_utf8();
            continue;
        }

        let body_start = index + current.len_utf8();
        let mut cursor = body_start;
        let mut escaped = false;
        let mut close = None;
        while let Some(next) = text[cursor..].chars().next() {
            if escaped {
                escaped = false;
            } else if next == '\\' {
                escaped = true;
            } else if next == current {
                close = Some(cursor);
                break;
            }
            cursor += next.len_utf8();
        }

        let Some(close) = close else {
            masked.push_str(&text[index..]);
            break;
        };

        let end = close + current.len_utf8();
        let inner = &text[body_start..close];
        // A backtick expression runs a command and yields its output, never the
        // literal option text, so it can never be an option key.
        if current != '`' && TRANSFORM_OPTIONS.iter().any(|option| inner.trim() == *option) {
            masked.push_str(&text[index..end]);
        } else {
            if current != '`'
                && !quoted_payload_is_literal(inner, current == '"')
                && is_key_position(text, end)
            {
                undecidable_key = true;
            }
            blank_into(&mut masked, &text[index..end]);
        }
        index = end;
    }

    MaskedArgs { masked, undecidable_key }
}

/// The result of masking one statement's import arguments.
struct MaskedArgs {
    /// The text with data payloads blanked, for the option-role predicate.
    masked: String,
    /// A quoted key sat in option position but could not be compared as
    /// written, so whether it names a transform option is unknown. Resolution
    /// must fail closed rather than pick a reading.
    undecidable_key: bool,
}

/// The quote-like operators that evaluate to their literal payload text, and
/// so can spell an option key. Longest first, so `qq`/`qw` are not read as `q`
/// followed by a delimiter.
///
/// Deliberately excludes the operators that evaluate to something else:
/// `qr` yields a compiled pattern, `qx` runs a command and yields its output,
/// and `m`/`s`/`tr`/`y` yield a match or substitution result. None of them
/// produces the literal option text, so none can be an option key.
const STRING_QUOTE_OPERATORS: [&str; 3] = ["qq", "qw", "q"];

/// The transform option a quote-like expression evaluates to, when its entire
/// payload is exactly one option name (`q{-as}`, `qq[-prefix]`), else `None`.
///
/// Only single-segment string-yielding forms can produce a bare option key;
/// `s///`-style two-segment expressions never trim to one option name, and the
/// non-string operators are rejected by [`STRING_QUOTE_OPERATORS`].
fn quote_like_option_key(span: &str) -> Option<&'static str> {
    let (_, inner) = string_quote_payload(span)?;
    TRANSFORM_OPTIONS.iter().copied().find(|option| inner.trim() == *option)
}

/// The operator and literal payload of a string-producing quote-like span.
///
/// `None` for anything else — a non-string operator, or text that is not a
/// quote-like expression at all.
fn string_quote_payload(span: &str) -> Option<(&'static str, &str)> {
    let operator = STRING_QUOTE_OPERATORS.iter().copied().find(|operator| {
        span.strip_prefix(*operator)
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
    })?;
    let after_operator = span[operator.len()..].trim_start();
    let mut chars = after_operator.chars();
    let open = chars.next()?;
    let close = match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        other => other,
    };
    let inner = chars.as_str().strip_suffix(close)?;
    Some((operator, inner))
}

/// Whether a quoted payload can be compared against an option name as written.
///
/// Perl evaluates escapes, and (in an interpolating quote) variables, before
/// the hash key exists: `"\x2das"` *is* the key `-as`, and `"${sigil}as"` may
/// be. This module evaluates neither, so a payload carrying either construct
/// cannot be classified as data or as syntax — only guessed at.
///
/// `qw` is listed as interpolating-safe because it never interpolates, and `q`
/// and `'` only honor `\\` and `\'`; a backslash still makes them undecidable
/// here rather than worth a second escape model.
fn quoted_payload_is_literal(payload: &str, interpolating: bool) -> bool {
    !payload.contains('\\') && !(interpolating && (payload.contains('$') || payload.contains('@')))
}

/// Whether the text after a quoted span puts it in option-key position.
fn is_key_position(text: &str, end: usize) -> bool {
    text.get(end..).is_some_and(|rest| rest.trim_start().starts_with("=>"))
}

/// Whether a `/` at the end of `before` opens a bare match rather than
/// dividing what precedes it.
///
/// Perl resolves this from parse state. Inside an import list the workable
/// discriminator is the preceding *token*, not just its last character, and the
/// decisive property is the sigil rather than the spelling:
///
/// * a sigilled variable (`$count /`, `$? /`) is a complete value, so `/`
///   divides it;
/// * a closer or quote likewise ends a term (`f(1) /`, `'Foo' /`);
/// * a bare word is a function or operator call (`grep /.../`, `abs /.../`),
///   so `/` opens its first argument.
///
/// The bare-word case deliberately does *not* consult a list of known
/// operators. Perl has no fixed set here — any sub name can appear — and an
/// allowlist silently misreads every name it omits, dropping that statement's
/// imports.
///
/// Two bare-word shapes are decidable and treated as values: a numeric literal,
/// and a `__FILE__`-style compile-time token. A *named* constant (`PI / 2`) is
/// not decidable here — it is spelled exactly like a sub call — so it falls to
/// the call default and its statement fails closed. That costs completions,
/// never correctness: `scan_import_transforms` refuses when the recognizer
/// still claims a span this masked, so an ambiguous constant cannot turn into
/// a fabricated import. Resolving it properly needs the symbol table that the
/// canonical adapter migration brings.
fn bare_match_can_start(before: &str) -> bool {
    let trimmed = before.trim_end();
    let Some(last) = trimmed.chars().next_back() else {
        return true;
    };

    // A bare sigil means the `/` is itself the variable's name (`$/`), not a
    // match opener.
    if matches!(last, '$' | '@' | '%') {
        return false;
    }

    // A punctuation-named variable (`$?`, `$!`, `@-`) is a complete term, so a
    // following `/` divides it.
    let mut reversed = trimmed.chars().rev();
    reversed.next();
    if !last.is_alphanumeric() && last != '_' && matches!(reversed.next(), Some('$' | '@' | '%')) {
        return false;
    }

    if last.is_alphanumeric() || last == '_' {
        let word_start = trimmed
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(trimmed.len(), |(offset, _)| offset);
        // A sigil makes it a variable (`$grep /`).
        let sigil = trimmed[..word_start].chars().next_back();
        if matches!(sigil, Some('$' | '@' | '%' | '&')) {
            return false;
        }
        let word = &trimmed[word_start..];
        // A numeric literal is a value, never a call.
        if word.starts_with(|c: char| c.is_ascii_digit()) {
            return false;
        }
        // Perl's compile-time tokens are values, never calls. The set is
        // closed: `__helper__` is an ordinary subroutine name, and the general
        // bareword rule above already treats those as calls. Excluding the
        // whole `__NAME__` *shape* would make this the one place a spelling,
        // rather than structure, decided the question.
        if matches!(word, "__FILE__" | "__LINE__" | "__PACKAGE__" | "__SUB__" | "__CLASS__") {
            return false;
        }
        return true;
    }

    // Closers and quotes end a term, so a following `/` divides it.
    !matches!(last, ')' | ']' | '}' | '\'' | '"')
}

/// End offset just past a bare `/.../` match and any trailing flags, or `None`
/// when the expression is unterminated (left visible for the conservative path).
fn bare_match_end(text: &str, start: usize) -> Option<usize> {
    let mut cursor = start + '/'.len_utf8();
    let mut escaped = false;
    while let Some(next) = text[cursor..].chars().next() {
        if escaped {
            escaped = false;
        } else if next == '\\' {
            escaped = true;
        } else if next == '/' {
            let mut end = cursor + next.len_utf8();
            while let Some(flag) = text[end..].chars().next() {
                if !flag.is_ascii_alphabetic() {
                    break;
                }
                end += flag.len_utf8();
            }
            return Some(end);
        }
        cursor += next.len_utf8();
    }
    None
}

/// Append one space per character of `span`, keeping token boundaries intact
/// while erasing the payload.
fn blank_into(masked: &mut String, span: &str) {
    for _ in span.chars() {
        masked.push(' ');
    }
}

/// Outcome of scanning one import list for transform syntax.
enum TransformScan {
    /// No transform syntax observed; the ordinary bareword scan may run over
    /// the import text unchanged.
    None,
    /// Transform syntax recognized and fully consumed. `stripped` is the
    /// import text with every recognized transform span removed.
    Recognized { renames: Vec<(String, String)>, stripped: String },
    /// Transform syntax is present but was not fully recognized — either the
    /// recognizer could not be constructed, or it left transform bytes behind
    /// (malformed or unsupported form). The affected statement must fail
    /// closed rather than resume bareword scanning over those bytes.
    Unresolved,
}

/// Extract `-as`/`-prefix`/`-postfix` renames and strip their spans.
///
/// Passing `None` for either recognizer models that recognizer being
/// unavailable; this is the seam the fail-closed regressions drive.
fn scan_import_transforms(
    raw_args: &str,
    rename_as: Option<&Regex>,
    rename_fix: Option<&Regex>,
) -> TransformScan {
    // An option key whose evaluated text is unknown is invisible to both
    // instruments: the recognizers never match its spelling, and the detector
    // compares it literally. Neither disagrees, so the bareword scan would run
    // over a real rename map and report the original and the alias as imports.
    if mask_data_values(raw_args).undecidable_key {
        return TransformScan::Unresolved;
    }

    if !contains_transform_syntax(raw_args) {
        // The detector masks option-shaped text inside ordinary quoted values,
        // but the regex bridge does not: it can still match across a quoted
        // value (`ok => {target => '-as => my_ok'}`). Taking the no-transform
        // path there would bareword-scan a span a recognizer owns and report
        // its structural atoms — the container key, and an original that a
        // rename would have removed — as imported symbols. Where the two
        // disagree, fail closed rather than let the cheaper instrument win.
        let recognizer_claims_span = rename_as.is_some_and(|re| re.is_match(raw_args))
            || rename_fix.is_some_and(|re| re.is_match(raw_args));
        if recognizer_claims_span {
            return TransformScan::Unresolved;
        }
        return TransformScan::None;
    }

    // Transform syntax is present, so both recognizers are load-bearing for
    // this statement. An unavailable recognizer is instrument failure, not
    // evidence that the syntax is absent.
    let (Some(rename_as), Some(rename_fix)) = (rename_as, rename_fix) else {
        return TransformScan::Unresolved;
    };

    let mut renames: Vec<(String, String)> = Vec::new();
    for caps in rename_as.captures_iter(raw_args) {
        if !entry_carries_one_transform_option(caps.get(0).map_or("", |span| span.as_str())) {
            return TransformScan::Unresolved;
        }
        if let (Some(name), Some(alias)) = (caps.get(1), caps.get(2)) {
            if !capture_covers_whole_value(raw_args, alias.start(), alias.end()) {
                return TransformScan::Unresolved;
            }
            renames.push((name.as_str().to_string(), alias.as_str().to_string()));
        }
    }
    for caps in rename_fix.captures_iter(raw_args) {
        if !entry_carries_one_transform_option(caps.get(0).map_or("", |span| span.as_str())) {
            return TransformScan::Unresolved;
        }
        if let (Some(name), Some(kind), Some(fix)) = (caps.get(1), caps.get(2), caps.get(3)) {
            if !capture_covers_whole_value(raw_args, fix.start(), fix.end()) {
                return TransformScan::Unresolved;
            }
            let base = name.as_str();
            let alias = if kind.as_str() == "prefix" {
                format!("{}{}", fix.as_str(), base)
            } else {
                format!("{}{}", base, fix.as_str())
            };
            renames.push((base.to_string(), alias));
        }
    }

    // Remove matched rename spans so the remaining scan does not see the raw
    // `name => { ... }` text.
    let mut stripped = rename_as.replace_all(raw_args, " ").into_owned();
    stripped = rename_fix.replace_all(&stripped, " ").into_owned();

    // Residual transform syntax means the recognizers did not consume every
    // transform span (malformed, truncated, or an unsupported form). Those
    // leftover bytes are exactly what would otherwise be scanned as barewords.
    if contains_transform_syntax(&stripped) {
        return TransformScan::Unresolved;
    }

    TransformScan::Recognized { renames, stripped }
}

/// Whether a recognizer's captured transform value covers the whole Perl value
/// it was taken from.
///
/// Both patterns read a value as `['"]?(\w+)['"]?`, which happily matches a
/// *prefix* of anything longer, and the trailing `[^}]*?` then swallows the
/// rest: `-as => "my_\x6fk"` captured `my_`, and `-as => 'my ok'` captured
/// `my`. Either published a name that does not exist, as a clean result.
///
/// A quoted value must therefore end at its closing quote, and a bare one at a
/// value terminator — `\w+` is greedy, so anything else following it is more
/// of the value that the capture did not take.
fn capture_covers_whole_value(raw_args: &str, start: usize, end: usize) -> bool {
    let opened_with = raw_args[..start].chars().next_back();
    let follows = raw_args[end..].chars().next();
    match opened_with {
        Some(quote @ ('\'' | '"')) => follows == Some(quote),
        _ => follows.is_none_or(|next| next.is_whitespace() || matches!(next, ',' | '}' | ')')),
    }
}

/// Resolve the imported symbols and pragma effect of a single Test2 `use`
/// statement, given the module name and the raw import-argument text (whatever
/// appears between the module name and the terminating `;`).
///
/// Returns `None` if `module` is not a recognized Test2 module.
pub fn resolve_import(module: &str, raw_args: &str) -> Option<ResolvedImport> {
    resolve_import_with(module, raw_args, RENAME_AS.as_ref(), RENAME_FIX.as_ref())
        .map(|analysis| analysis.resolved)
}

/// [`resolve_import`] with the transform recognizers injected.
///
/// Production always passes the compiled statics. Tests pass `None` to force
/// an unavailable recognizer without process-global resettable state.
fn resolve_import_with(
    module: &str,
    raw_args: &str,
    rename_as: Option<&Regex>,
    rename_fix: Option<&Regex>,
) -> Option<ResolvedImportAnalysis> {
    if !is_test2_module(module) {
        return None;
    }

    // `use Test2::V0 ();` — an explicit empty import list. Perl does not call
    // `import`, so no symbols are imported and (for bundles) no strict/warnings
    // pragmas are applied. The module is still loaded, so return an empty import
    // rather than `None`.
    let trimmed_args = raw_args.trim();
    if trimmed_args.starts_with('(')
        && trimmed_args.ends_with(')')
        && trimmed_args[1..trimmed_args.len() - 1].trim().is_empty()
    {
        return Some(ResolvedImportAnalysis {
            resolved: ResolvedImport { symbols: BTreeSet::new(), pragmas: None },
            analysis_limited: false,
        });
    }

    let bundle = is_test2_bundle(module);

    // `Test2::V1` reaches V0 parity (the full bare tool set) only under an
    // explicit `-import` long option or an `i` short flag (standalone `-i` or
    // grouped, e.g. `-ipP` — the "work like V0" form). A plain `use Test2::V1;`
    // brings in only the `T2()` handle. Oracle: metacpan `Test2::V1`.
    let v1_import_all = module == "Test2::V1"
        && (args_contains_option(raw_args, "import") || v1_short_flag(raw_args, 'i'));
    let default_set =
        if v1_import_all { Some(V0_DEFAULT.as_slice()) } else { module_default_exports(module) };
    let all_set =
        if v1_import_all { Some(V0_DEFAULT.as_slice()) } else { module_all_exports(module) };

    // Pragma resolution (bundles only). Most bundles (`Test2::V0`, `Test2::Suite`,
    // `Test2::Bundle::*`) enable strict/warnings by default and opt OUT via
    // `-no_strict`/`-no_warnings`/`-no_pragmas`. `Test2::V1` is the exception: it
    // enables NO pragmas by default and opts IN via `-pragmas`/`-p` (grouped or
    // standalone), `-strict`, or `-warnings`. Oracle: metacpan `Test2::V1` ("NO
    // PRAGMAS ARE ENABLED BY DEFAULT").
    let pragmas = if bundle {
        if module == "Test2::V1" {
            let all = args_contains_option(raw_args, "pragmas") || v1_short_flag(raw_args, 'p');
            Some(Test2Pragmas {
                strict: all || args_contains_option(raw_args, "strict"),
                warnings: all || args_contains_option(raw_args, "warnings"),
            })
        } else {
            let no_pragmas = args_contains_option(raw_args, "no_pragmas");
            let no_strict = no_pragmas || args_contains_option(raw_args, "no_strict");
            let no_warnings = no_pragmas || args_contains_option(raw_args, "no_warnings");
            Some(Test2Pragmas { strict: !no_strict, warnings: !no_warnings })
        }
    } else {
        None
    };

    // Extract renames first (and strip their spans so their bareword names are
    // not double-counted as positive imports).
    let (renames, stripped) = match scan_import_transforms(raw_args, rename_as, rename_fix) {
        TransformScan::None => (Vec::new(), raw_args.to_string()),
        TransformScan::Recognized { renames, stripped } => (renames, stripped),
        TransformScan::Unresolved => {
            // Transform syntax was declared but not interpreted. Resuming the
            // bareword scan here would report `-as`, alias names, and mapping
            // keys/values as imported symbols. Fail closed instead: prove no
            // symbol rather than invent one. Pragma resolution is independent
            // of the import list, so it stays exact.
            return Some(ResolvedImportAnalysis {
                resolved: ResolvedImport { symbols: BTreeSet::new(), pragmas },
                analysis_limited: true,
            });
        }
    };

    let atoms = tokenize_import_args(&stripped);
    let target_option_supported = matches!(module, "Test2::V0" | "Test2::V1");
    let mut atom_index = 0;
    let mut target_helpers: BTreeSet<String> = BTreeSet::new();

    let mut positives: Vec<String> = Vec::new();
    let mut exclusions: BTreeSet<String> = BTreeSet::new();
    let mut include_default_tag = false;
    let mut include_all_tag = false;

    while let Some(atom) = atoms.get(atom_index) {
        atom_index += 1;
        // Keep quote delimiters in tokenizer atoms so `-target` can preserve
        // Perl's distinction between a quoted string and an unquoted literal.
        // Other import entries still use their unquoted spelling for matching.
        let atom = strip_quotes(atom.trim());
        if atom.is_empty() {
            continue;
        }
        if let Some(rest) = atom.strip_prefix('!') {
            // Exclusion: `!name` (pattern/tag exclusions are ignored — high
            // precision over completeness).
            if is_bareword(rest) {
                exclusions.insert(rest.to_string());
            }
            continue;
        }
        if let Some(tag) = atom.strip_prefix(':') {
            match tag.to_ascii_lowercase().as_str() {
                "default" => include_default_tag = true,
                "all" => include_all_tag = true,
                _ => {}
            }
            continue;
        }
        if atom.starts_with('-') {
            // Test2::V0 and Test2::V1 consume the value after `-target` before
            // export processing. The flat atom view must do the same or a
            // single-segment package name/hash key looks like an imported sub.
            if target_option_supported && atom == "-target" {
                let mut brace_depth = 0_isize;
                let mut saw_hash = false;
                let mut expect_key = true;
                let mut nested_value_depth = None;
                let mut hash_closed = false;
                let mut value_expression_unprovable = false;
                let mut pending_helpers: BTreeSet<String> = BTreeSet::new();
                while let Some(value) = atoms.get(atom_index) {
                    atom_index += 1;
                    if saw_hash && brace_depth == 1 && value == "," {
                        // Commas preserved by tokenization delimit hash pairs;
                        // whitespace inside a value must not change pairing.
                        expect_key = true;
                        continue;
                    }
                    if is_quote_like_operator(value) {
                        // Quote-like operators are expressions, even when
                        // their first token resembles a bareword. At the
                        // scalar boundary they own the target and leave
                        // CLASS unproven. Inside a hash, however, they are a
                        // value (or a dynamic key), so consume the complete
                        // expression and keep scanning the enclosing hash;
                        // stopping here would leak the remaining keys and
                        // values into ordinary import processing.
                        atom_index = consume_quote_like_target(&atoms, atom_index - 1, value);
                        if saw_hash {
                            continue;
                        }
                        break;
                    }
                    let (opens, closes) = count_unquoted_braces(value);
                    if is_dynamic_target_atom(value) {
                        atom_index = consume_dynamic_target_expression(&atoms, atom_index - 1);
                        if saw_hash {
                            continue;
                        }
                        break;
                    }
                    if !saw_hash && opens == 0 {
                        // Parentheses and unary-plus may be separated from a
                        // hash opener by whitespace (`( { ... } )` and
                        // `+ { ... }`). They are structural only when the
                        // nearby tokens prove that this is a hash target;
                        // otherwise the first atom is the scalar target.
                        if is_structural_target_atom(value) {
                            if target_starts_hash(&atoms, atom_index - 1) {
                                continue;
                            }
                            if value == "+" {
                                // A separated unary-plus target owns its
                                // operand too. Otherwise `+ 'Foo'` would
                                // stop here and the outer export scan would
                                // incorrectly import `Foo`.
                                atom_index =
                                    consume_unwrapped_target_expression(&atoms, atom_index - 1);
                                break;
                            }
                            if value == "(" {
                                if let Some((next_index, truthy)) =
                                    consume_parenthesized_scalar(&atoms, atom_index - 1)
                                {
                                    atom_index = next_index;
                                    if truthy {
                                        target_helpers.insert("CLASS".to_string());
                                    }
                                    break;
                                }
                                // An unclosed parenthesized target owns the
                                // remainder of the option expression. Do not
                                // resume ordinary export scanning and leak
                                // its values or barewords.
                                atom_index = atoms.len();
                                break;
                            }
                        }
                        if is_bareword(value)
                            && atoms.get(atom_index).map(String::as_str) == Some("(")
                        {
                            atom_index = consume_parenthesized_expression(&atoms, atom_index);
                            break;
                        }
                        if scalar_target_is_truthy(value) {
                            target_helpers.insert("CLASS".to_string());
                        }
                        break;
                    }
                    let previous_depth = brace_depth;
                    saw_hash |= opens > 0;
                    brace_depth += opens;
                    if previous_depth == 1 && opens > 0 && !expect_key {
                        // A nested hash is one value of the enclosing hash.
                        // Keep the outer key/value parity unchanged until the
                        // complete nested structure closes.
                        nested_value_depth = Some(brace_depth);
                    }
                    // Unary-plus and parenthesized hashrefs leave wrapper
                    // punctuation attached to the first/last atom. These
                    // structural atoms must not consume a key/value slot.
                    let candidate =
                        strip_quotes(value.trim_matches(['+', '{', '}', '(', ')']).trim());
                    if brace_depth == 1 && !candidate.is_empty() {
                        if expect_key && is_bareword(candidate) {
                            pending_helpers.insert(candidate.to_string());
                            expect_key = false;
                        } else if !expect_key && LIST_ARITY_OPERATORS.contains(&candidate) {
                            // A list operator's argument list consumes its own
                            // comma-separated terms, so a top-level comma after
                            // it is an argument separator, not a hash-pair
                            // separator (`join '-', 'Widget'`). The remaining
                            // pairing is a guess: fail closed for the whole
                            // hash instead of inventing helpers (#13305).
                            value_expression_unprovable = true;
                        }
                    }
                    brace_depth -= closes;
                    if nested_value_depth.is_some_and(|depth| brace_depth < depth) {
                        nested_value_depth = None;
                        expect_key = true;
                    }
                    if saw_hash && brace_depth <= 0 {
                        hash_closed = true;
                        break;
                    }
                }
                if hash_closed && !value_expression_unprovable {
                    target_helpers.extend(pending_helpers);
                }
            }
            // Other import options are flags whose effects are handled
            // elsewhere; no option token is a positive symbol.
            continue;
        }
        if is_bareword(atom) {
            positives.push(atom.to_string());
        }
    }

    let rename_aliases: Vec<String> = renames.iter().map(|(_, alias)| alias.clone()).collect();

    // Decide the base set. Explicit local-name selections replace the default
    // unless a tag requests a reviewed set as well. Importer supplies automatic
    // `:DEFAULT` and `:ALL` tags; asking for `:ALL` must not suppress every
    // known import merely because the tag itself is explicit.
    let has_local_selection = !positives.is_empty() || !renames.is_empty();
    let use_default = !has_local_selection || include_default_tag || include_all_tag;
    let requested_base_set = if include_all_tag { all_set } else { default_set };

    let base_symbols: BTreeSet<String> = if use_default {
        requested_base_set
            .into_iter()
            .flat_map(|symbols| symbols.iter().copied())
            .map(str::to_string)
            .collect()
    } else {
        BTreeSet::new()
    };
    let mut symbols = base_symbols.clone();
    for name in &positives {
        symbols.insert(name.clone());
    }
    for alias in &rename_aliases {
        symbols.insert(alias.clone());
    }
    // A rename replaces the original only when that original was not also
    // requested independently by a tag or a positive import entry. Importer
    // expands `:DEFAULT`/`:ALL` into their own entries before applying the
    // renamed entry, so both local names remain installed in that composition.
    for (orig, _) in &renames {
        if !positives.iter().any(|positive| positive == orig) && !base_symbols.contains(orig) {
            symbols.remove(orig);
        }
    }
    for excluded in &exclusions {
        symbols.remove(excluded);
    }
    // Test2::Tools::Target installs these helpers separately from the export
    // list, so they neither suppress defaults nor participate in exclusions.
    for helper in target_helpers {
        symbols.insert(helper);
    }

    Some(ResolvedImportAnalysis {
        resolved: ResolvedImport { symbols, pragmas },
        analysis_limited: false,
    })
}

/// Aggregate Test2 facts for an entire source file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Test2Facts {
    /// Test2 modules imported in the file (in source order).
    pub modules: Vec<String>,
    /// All symbols imported from Test2 modules.
    pub imported_symbols: BTreeSet<String>,
    /// Whether some Test2 bundle turned on `strict`.
    pub strict: bool,
    /// Whether some Test2 bundle turned on `warnings`.
    pub warnings: bool,
}

/// Crate-private facts plus the completeness signal needed by
/// completeness-sensitive production consumers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Test2FactsAnalysis {
    pub facts: Test2Facts,
    pub analysis_limited: bool,
}

impl Test2Facts {
    /// Whether the file imports any Test2 module at all.
    pub fn uses_test2(&self) -> bool {
        !self.modules.is_empty()
    }

    /// Whether an imported Test2 bundle turns on the named pragma. Only
    /// `strict` and `warnings` are provided by Test2 bundles; every other
    /// feature returns `false`.
    pub fn provides_pragma(&self, feature: &str) -> bool {
        match feature {
            "strict" => self.strict,
            "warnings" => self.warnings,
            _ => false,
        }
    }

    /// Whether the file imports any Test2 *bundle* (`Test2::V0`, etc.).
    pub fn uses_test2_bundle(&self) -> bool {
        self.modules.iter().any(|m| is_test2_bundle(m))
    }

    /// Whether `name` is a symbol imported from Test2 in this file.
    pub fn is_imported(&self, name: &str) -> bool {
        self.imported_symbols.contains(name)
    }

    /// Scan `source` for Test2 `use` statements and aggregate their effects.
    pub fn from_source(source: &str) -> Self {
        Self::from_source_with_analysis(source).facts
    }

    /// Scan source while retaining the internal completeness result. Public
    /// callers continue to receive the stable [`Test2Facts`] shape.
    pub(crate) fn from_source_with_analysis(source: &str) -> Test2FactsAnalysis {
        let mut facts = Test2Facts::default();
        let mut analysis_limited = false;
        for stmt in use_statements(source) {
            let Some((module, args)) = parse_use_statement(&stmt) else {
                continue;
            };
            let Some(resolved) =
                resolve_import_with(&module, &args, RENAME_AS.as_ref(), RENAME_FIX.as_ref())
            else {
                continue;
            };
            facts.modules.push(module);
            analysis_limited |= resolved.analysis_limited;
            for sym in resolved.resolved.symbols {
                facts.imported_symbols.insert(sym);
            }
            if let Some(pragmas) = resolved.resolved.pragmas {
                facts.strict |= pragmas.strict;
                facts.warnings |= pragmas.warnings;
            }
        }
        Test2FactsAnalysis { facts, analysis_limited }
    }
}

// ---------------------------------------------------------------------------
// Source scanning helpers.
// ---------------------------------------------------------------------------

/// Whether `s` is a plain Perl identifier (bareword), optionally quoted by the
/// caller before this check.
/// Operators whose argument lists consume comma-separated terms (perlop list
/// operators). A top-level comma after one of these, in a target-hash value
/// position, is an argument separator rather than a hash-pair separator, which
/// makes the remaining key/value pairing unprovable at the atom level.
const LIST_ARITY_OPERATORS: [&str; 14] = [
    "join", "sprintf", "printf", "split", "map", "grep", "sort", "push", "unshift", "splice",
    "reverse", "say", "die", "warn",
];

fn is_bareword(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether an import-option flag (`no_strict`, `no_warnings`, ...) appears in the
/// raw args, in either `-flag` or `-flag => 1` form.
fn args_contains_option(raw_args: &str, flag: &str) -> bool {
    let needle = format!("-{flag}");
    raw_args.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).any(|tok| {
        // Match the exact flag token, not a prefix (`-no_strict` must not match
        // a hypothetical `-no_strictness`).
        tok == needle
    })
}

/// Whether the Test2::V1 short flag `flag_char` is set — either as a standalone
/// `-c` option or inside a grouped short-flag token such as `-ipP`. A grouped
/// token is `-` followed only by known V1 short-flag letters (`i`=import,
/// `p`=pragmas, `P`=plugins, `x`), which distinguishes it from long options like
/// `-import` or `-strict` (whose other letters are not short flags). Oracle:
/// metacpan `Test2::V1` SYNOPSIS (`use Test2::V1 -ipP;`).
fn v1_short_flag(raw_args: &str, flag_char: char) -> bool {
    raw_args.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-')).any(|tok| {
        tok.strip_prefix('-').is_some_and(|rest| {
            !rest.is_empty()
                && rest.chars().all(|c| matches!(c, 'i' | 'p' | 'P' | 'x'))
                && rest.contains(flag_char)
        })
    })
}

/// Split raw import-argument text into classifiable atoms. Handles `qw//`
/// lists, quoted strings, and comma / fat-comma separated barewords.
fn tokenize_import_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let expanded = expand_qw(raw);
    // Normalize separators only outside quoted strings. A quoted scalar is one
    // Perl atom even when it contains commas, braces, or escaped delimiters.
    let pieces = split_import_pieces(&expanded);
    for (piece_index, piece) in pieces.iter().enumerate() {
        let piece = replace_unquoted_fat_commas(piece);
        out.extend(split_import_piece(&piece));
        if piece_index + 1 < pieces.len() {
            out.push(",".to_string());
        }
    }
    out
}

/// Split import arguments on commas outside quoted strings. Hash commas are
/// intentionally separators too; `split_import_piece` preserves each quoted
/// key/value atom after this pass.
fn split_import_pieces(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut paren_depth = 0usize;

    let mut index = 0;
    while index < raw.len() {
        if quote.is_none()
            && let Some(end) = quote_like_expression_end(raw, index)
        {
            current.push_str(&raw[index..end]);
            index = end;
            continue;
        }
        let Some(ch) = raw[index..].chars().next() else { break };
        index += ch.len_utf8();
        if let Some(delimiter) = quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            current.push(ch);
            quote = Some(ch);
        } else if ch == '(' {
            paren_depth += 1;
            current.push(ch);
        } else if ch == ')' {
            paren_depth = paren_depth.saturating_sub(1);
            current.push(ch);
        } else if ch == ',' && paren_depth == 0 {
            out.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    out.push(current);
    out
}

/// Return the byte offset after a Perl quote-like expression beginning at
/// `start`, or `None` when the text does not contain a complete expression.
/// Keeping this expression opaque is essential before comma splitting: commas
/// inside `q#...#`, `s/.../.../`, and similar forms are not import separators.
fn quote_like_expression_end(raw: &str, start: usize) -> Option<usize> {
    let bytes = raw.as_bytes();
    if start > 0
        && bytes
            .get(start.saturating_sub(1))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        return None;
    }
    let operators = [b"tr".as_slice(), b"qq", b"qx", b"qr", b"qw", b"q", b"m", b"s", b"y"];
    let operator = operators
        .iter()
        .find(|operator| bytes.get(start..start + operator.len()) == Some(*operator))?;
    let mut delimiter = start + operator.len();
    while bytes.get(delimiter).is_some_and(u8::is_ascii_whitespace) {
        delimiter += 1;
    }
    let open = *bytes.get(delimiter)?;
    if open.is_ascii_alphanumeric() || open == b'_' || open.is_ascii_whitespace() || open == b'=' {
        return None;
    }
    let close = match open {
        b'(' => b')',
        b'{' => b'}',
        b'[' => b']',
        b'<' => b'>',
        other => other,
    };
    let paired = matches!(open, b'(' | b'{' | b'[' | b'<');
    let segment_end = find_quote_like_segment_end(bytes, delimiter + 1, open, close, paired)?;
    let mut end = segment_end;
    if matches!(*operator, b"s" | b"tr" | b"y") {
        while bytes.get(end).is_some_and(u8::is_ascii_whitespace) {
            end += 1;
        }
        // Non-paired delimiters are reused after the first segment (`s/a/b/`);
        // paired forms repeat their opener (`s{a}{b}`).
        let second_open = if paired { *bytes.get(end)? } else { open };
        let second_close = match second_open {
            b'(' => b')',
            b'{' => b'}',
            b'[' => b']',
            b'<' => b'>',
            other => other,
        };
        let second_paired = matches!(second_open, b'(' | b'{' | b'[' | b'<');
        let second_start = if paired { end + 1 } else { end };
        end = find_quote_like_segment_end(
            bytes,
            second_start,
            second_open,
            second_close,
            second_paired,
        )?;
    }
    if matches!(*operator, b"m" | b"qr" | b"s" | b"tr" | b"y") {
        while bytes.get(end).is_some_and(u8::is_ascii_alphabetic) {
            end += 1;
        }
    }
    Some(end)
}

fn find_quote_like_segment_end(
    bytes: &[u8],
    mut index: usize,
    open: u8,
    close: u8,
    paired: bool,
) -> Option<usize> {
    let mut depth = if paired { 1 } else { 0 };
    let mut escaped = false;
    while let Some(&byte) = bytes.get(index) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if paired && byte == open {
            depth += 1;
        } else if byte == close {
            if !paired || depth == 1 {
                return Some(index + 1);
            }
            depth -= 1;
        }
        index += 1;
    }
    None
}

/// Replace fat-comma operators outside quoted strings without changing text
/// such as `'=>` inside a target package name.
fn replace_unquoted_fat_commas(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut quote = None;
    let mut escaped = false;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(delimiter) = quote {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            out.push(ch);
            quote = Some(ch);
        } else if ch == '=' && chars.peek() == Some(&'>') {
            out.push(' ');
            out.push(' ');
            chars.next();
        } else {
            out.push(ch);
        }
    }
    out
}

/// Count braces that are structural rather than characters inside a quoted
/// Perl scalar. The result is used only for the conservative hash-target
/// recognizer, which must fail closed when the structure is not balanced.
fn count_unquoted_braces(raw: &str) -> (isize, isize) {
    let mut opens = 0;
    let mut closes = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0;

    while index < raw.len() {
        if quote.is_none()
            && let Some(end) = quote_like_expression_end(raw, index)
        {
            index = end;
            continue;
        }
        let Some(ch) = raw[index..].chars().next() else { break };
        index += ch.len_utf8();
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if ch == '{' {
            opens += 1;
        } else if ch == '}' {
            closes += 1;
        }
    }
    (opens, closes)
}

/// Split one import-argument piece while keeping quoted strings intact and
/// making wrapper punctuation independently classifiable. In particular,
/// `('Foo')` must become `(`, `'Foo'`, `)` rather than one opaque atom.
fn split_import_piece(piece: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut attached_parens = 0usize;

    let flush = |out: &mut Vec<String>, current: &mut String| {
        if !current.is_empty() {
            out.push(std::mem::take(current));
        }
    };

    let mut index = 0;
    while index < piece.len() {
        if quote.is_none()
            && current.is_empty()
            && let Some(end) = quote_like_expression_end(piece, index)
        {
            out.push(piece[index..end].to_string());
            index = end;
            continue;
        }
        let Some(ch) = piece[index..].chars().next() else { break };
        index += ch.len_utf8();
        if let Some(delimiter) = quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                current.push(ch);
                quote = Some(ch);
            }
            '(' | ')' | '{' | '}' => {
                let attached = matches!(ch, '(') && !current.is_empty() || attached_parens > 0;
                if attached {
                    current.push(ch);
                    if ch == '(' {
                        attached_parens += 1;
                    } else if ch == ')' {
                        attached_parens = attached_parens.saturating_sub(1);
                    }
                } else {
                    flush(&mut out, &mut current);
                    out.push(ch.to_string());
                }
            }
            ',' if attached_parens == 0 => {
                flush(&mut out, &mut current);
                out.push(",".to_string());
            }
            c if c.is_whitespace() && attached_parens == 0 => flush(&mut out, &mut current),
            _ => current.push(ch),
        }
    }
    flush(&mut out, &mut current);
    out
}

/// Expand every `qw/.../` (and `qw{}`, `qw()`, `qw[]`, `qw<>`) construct in
/// `raw` into a plain space-separated word list.
fn expand_qw(raw: &str) -> String {
    let mut out = String::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    // Invariant: `i` is always on a UTF-8 char boundary at the top of the loop
    // (we only ever advance past a whole char or past an ASCII delimiter byte),
    // so `raw[i..]` and the delimiter slices below never split a codepoint.
    // `qw`, its delimiters, and its closers are all ASCII, so the structural
    // scan uses byte predicates while non-ASCII content is copied char-wise.
    while i < bytes.len() {
        // Only treat `qw` as the quote-word operator on a word boundary, so
        // barewords like `qwerty` or `my_qw` are not misread as `qw`.
        let on_word_boundary =
            i == 0 || !matches!(bytes[i - 1], b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_');
        if on_word_boundary && bytes[i] == b'q' && bytes.get(i + 1) == Some(&b'w') {
            // Find the delimiter after optional whitespace (all ASCII).
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if let Some(&open) = bytes.get(j) {
                // A real delimiter is non-word, non-whitespace, and must be
                // ASCII: a non-ASCII byte here is a multi-byte lead/continuation
                // byte, and treating it as the delimiter would slice `raw`
                // mid-codepoint below and panic. (`qwords` would otherwise treat
                // `o` as the delimiter.)
                if open.is_ascii()
                    && !open.is_ascii_alphanumeric()
                    && open != b'_'
                    && !open.is_ascii_whitespace()
                {
                    let close = match open {
                        b'(' => b')',
                        b'{' => b'}',
                        b'[' => b']',
                        b'<' => b'>',
                        other => other,
                    };
                    if let Some(end_rel) = bytes[j + 1..].iter().position(|&b| b == close)
                        && !qw_is_target_value(raw, i)
                    {
                        // `j + 1` and `j + 1 + end_rel` sit on ASCII delimiter
                        // bytes, i.e. char boundaries, so slicing `raw` is safe.
                        let inner = &raw[j + 1..j + 1 + end_rel];
                        out.push(' ');
                        out.push_str(inner);
                        out.push(' ');
                        i = j + 1 + end_rel + 1;
                        continue;
                    }
                }
            }
        }
        // Copy the whole current char (handles multi-byte UTF-8 safely).
        let Some(ch) = raw[i..].chars().next() else { break };
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Keep a `qw` expression opaque when it is the value of `-target`.
///
/// `expand_qw` is useful for ordinary import lists, but expanding an empty
/// target (`qw{}`) would erase the target atom and make the following export
/// look like the target value. The target resolver must instead consume the
/// expression and fail closed.
fn qw_is_target_value(raw: &str, index: usize) -> bool {
    let Some(target_start) = raw[..index].rfind("-target") else {
        return false;
    };
    let before = &raw[target_start..index];
    if !before.contains("=>") {
        return false;
    }

    // A comma at depth zero terminates the target option; commas inside a
    // hash, call, or wrapper belong to its expression. Keep `qw` opaque in
    // all of those target-expression contexts so its words cannot be
    // mistaken for helpers or imports.
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0usize;
    for ch in before.chars() {
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if matches!(ch, '(' | '{' | '[') {
            depth = depth.saturating_add(1);
        } else if matches!(ch, ')' | '}' | ']') {
            depth = depth.saturating_sub(1);
        } else if (ch == ',' && depth == 0) || ch == ';' {
            return false;
        }
    }
    true
}

/// Strip surrounding single or double quotes from a token.
fn strip_quotes(tok: &str) -> &str {
    let tok = tok.trim();
    let bytes = tok.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'\'' || first == b'"') && first == last {
            return &tok[1..tok.len() - 1];
        }
    }
    tok
}

/// Whether `tok` is a complete single- or double-quoted token, including the
/// intentionally empty string literal.
fn is_quoted_token(tok: &str) -> bool {
    let tok = tok.trim();
    let bytes = tok.as_bytes();
    bytes.len() >= 2
        && (bytes[0] == b'\'' || bytes[0] == b'"')
        && bytes[0] == bytes[bytes.len() - 1]
}

/// Whether an atom contains only target-expression wrapper punctuation.
fn is_structural_target_atom(atom: &str) -> bool {
    !atom.is_empty() && atom.chars().all(|c| matches!(c, '+' | '{' | '}' | '(' | ')'))
}

/// Perl quote-like operators produce expressions, never package-name atoms.
fn is_quote_like_operator(atom: &str) -> bool {
    matches!(atom, "q" | "qq" | "qw" | "qx" | "m" | "qr" | "s" | "tr" | "y")
}

/// Consume the delimited expression following a quote-like operator.
fn consume_quote_like_target(atoms: &[String], start: usize, operator: &str) -> usize {
    let mut next = start.saturating_add(1);
    let consume_one = |atoms: &[String], index: &mut usize| {
        let Some(open) = atoms.get(*index).map(String::as_str) else {
            return;
        };
        let close = match open {
            "(" => ")",
            "{" => "}",
            "[" => "]",
            "<" => ">",
            _ => {
                *index = (*index).saturating_add(1);
                return;
            }
        };
        let mut depth = 0usize;
        while let Some(atom) = atoms.get(*index) {
            match atom.as_str() {
                value if value == open => depth = depth.saturating_add(1),
                value if value == close => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        *index = (*index).saturating_add(1);
                        return;
                    }
                }
                _ => {}
            }
            *index = (*index).saturating_add(1);
        }
    };

    consume_one(atoms, &mut next);
    if matches!(operator, "s" | "tr" | "y") {
        consume_one(atoms, &mut next);
    }
    next
}

/// Look ahead through separated wrapper punctuation for a hash opener.
fn target_starts_hash(atoms: &[String], start: usize) -> bool {
    atoms
        .iter()
        .skip(start + 1)
        .take(4)
        .take_while(|atom| is_structural_target_atom(atom))
        .any(|atom| atom.contains('{'))
}

/// Consume a whitespace-separated parenthesized scalar target.
///
/// The import tokenizer keeps wrapper punctuation as atoms, so `( 'Foo' )`
/// would otherwise make `(` look like the target and let `Foo` leak into the
/// export scan. Only a balanced wrapper containing one scalar atom is inferred;
/// other expressions are consumed but remain outside the truthiness boundary.
fn consume_parenthesized_scalar(atoms: &[String], start: usize) -> Option<(usize, bool)> {
    if atoms.get(start).map(String::as_str) != Some("(") {
        return None;
    }

    let mut depth = 0usize;
    let mut inner: Vec<&str> = Vec::new();
    let mut nested_expression = false;
    for (index, atom) in atoms.iter().enumerate().skip(start) {
        match atom.as_str() {
            "(" => {
                if depth > 0 {
                    nested_expression = true;
                }
                depth += 1;
            }
            ")" => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    let truthy =
                        !nested_expression && inner.len() == 1 && scalar_target_is_truthy(inner[0]);
                    return Some((index + 1, truthy));
                }
            }
            "{" | "}" if depth > 0 => nested_expression = true,
            _ if depth == 1 => inner.push(atom.as_str()),
            _ if depth > 1 => nested_expression = true,
            _ => {}
        }
    }
    None
}

/// Consume a call-like argument list without attempting to evaluate it.
fn consume_parenthesized_expression(atoms: &[String], start: usize) -> usize {
    let mut depth = 0usize;
    for (index, atom) in atoms.iter().enumerate().skip(start) {
        match atom.as_str() {
            "(" => depth += 1,
            ")" => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    atoms.len()
}

/// Consume the operand of a separated unary-plus target without inferring it.
///
/// The target option owns the complete expression even when its first wrapper
/// atom is separate from the operand (`+ 'Foo'` or `+ foo()`). The operand is
/// deliberately left outside the truthiness boundary: only a direct quoted
/// scalar or bare literal is proven by this resolver.
fn consume_unwrapped_target_expression(atoms: &[String], start: usize) -> usize {
    let mut operand = start.saturating_add(1);
    while atoms.get(operand).map(String::as_str) == Some("+") {
        operand = operand.saturating_add(1);
    }
    let Some(value) = atoms.get(operand).map(String::as_str) else {
        return atoms.len();
    };
    if is_quote_like_operator(value) {
        return consume_quote_like_target(atoms, operand, value);
    }
    if is_dynamic_target_atom(value) {
        return consume_dynamic_target_expression(atoms, operand);
    }
    if value == "(" {
        return consume_parenthesized_expression(atoms, operand);
    }
    if is_bareword(value) && atoms.get(operand + 1).map(String::as_str) == Some("(") {
        return consume_parenthesized_expression(atoms, operand + 1);
    }
    operand.saturating_add(1)
}

/// Whether an atom starts a dynamic Perl dereference whose value is not
/// statically provable by this resolver.
fn is_dynamic_target_atom(atom: &str) -> bool {
    matches!(atom.chars().next(), Some('$' | '@' | '%' | '&'))
}

/// Consume a dynamic dereference and its balanced subscript expression.
fn consume_dynamic_target_expression(atoms: &[String], start: usize) -> usize {
    let Some(open) = atoms.get(start.saturating_add(1)).map(String::as_str) else {
        return start.saturating_add(1);
    };
    if open != "{" {
        return start.saturating_add(1);
    }

    let consume_subscript = |atoms: &[String], start: usize| -> Option<usize> {
        let mut depth = 0usize;
        for (index, atom) in atoms.iter().enumerate().skip(start) {
            match atom.as_str() {
                "{" => depth = depth.saturating_add(1),
                "}" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index.saturating_add(1));
                    }
                }
                _ => {}
            }
        }
        None
    };

    let Some(mut next) = consume_subscript(atoms, start.saturating_add(1)) else {
        return atoms.len();
    };

    // Chained dereferences such as `$ENV{TARGET}->{other}` own each following
    // subscript. Consume those pairs before the enclosing target hash sees
    // their closing braces as structural hash delimiters.
    while atoms.get(next).map(String::as_str) == Some("->") {
        if atoms.get(next.saturating_add(1)).map(String::as_str) != Some("{") {
            break;
        }
        next = consume_subscript(atoms, next.saturating_add(1)).unwrap_or(atoms.len());
        if next == atoms.len() {
            return next;
        }
    }

    // A dereference can be part of a larger dynamic expression. Consume its
    // suffix through the target-option boundary so words after the closure do
    // not become ordinary imports or hash helpers.
    while let Some(atom) = atoms.get(next).map(String::as_str) {
        if matches!(atom, "," | "}" | ")") {
            break;
        }
        next = next.saturating_add(1);
    }
    next
}

/// Whether a scalar `-target` literal creates Test2::Tools::Target helpers.
///
/// Perl's false scalar values do not install the target helpers. Keep this
/// deliberately literal-only: dynamic expressions remain outside this
/// resolver's proof boundary rather than being guessed as truthy or falsey.
fn scalar_target_is_truthy(raw: &str) -> bool {
    let trimmed = raw.trim();
    // Quote-like operators are expressions, not bareword package names. The
    // import tokenizer may expose `q{...}`/`qq{...}` as `q`/`qq` followed by
    // delimiter atoms; fail closed here rather than inferring CLASS from the
    // operator name. The following delimiter atoms are structural and are not
    // eligible for ordinary export matching.
    if matches!(trimmed, "q" | "qq") {
        return false;
    }
    // A quoted non-empty string is truthy except for Perl's one false string,
    // "0". In particular, quoted spellings such as 'undef' and '0.0' must not
    // be confused with their unquoted false/dynamic counterparts.
    if is_quoted_token(trimmed) {
        let value = strip_quotes(trimmed);
        // Double-quoted interpolation is runtime-dependent, even when the
        // resulting value might look like a package name. Single-quoted
        // package values remain proven literals.
        if trimmed.starts_with('"') && (value.contains('$') || value.contains('@')) {
            return false;
        }
        return !value.is_empty() && value != "0";
    }

    if trimmed == "undef" || is_definitely_false_numeric(trimmed) {
        return false;
    }

    if trimmed.is_empty() || trimmed == "0" {
        return false;
    }
    // Variables and operators require evaluation. Do not guess their Perl
    // truthiness from source spelling.
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '$' | '@' | '%' | '&' | '+' | '-' | '(' | '{' | '['))
    {
        return false;
    }
    // A nonempty quoted string (other than Perl's false string `"0"`) and a
    // bare package name are safely established truthy literals. Numeric forms
    // not covered above remain deliberately outside this resolver's boundary.
    if is_bareword(trimmed) {
        return true;
    }
    false
}

/// Recognize only numeric spellings whose value is definitely false in Perl.
/// Other numeric-looking forms stay outside the inference boundary.
fn is_definitely_false_numeric(raw: &str) -> bool {
    let mut value = raw.trim();
    if let Some(rest) = value.strip_prefix(['+', '-']) {
        value = rest;
    }

    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        return !hex.is_empty()
            && hex.chars().all(|ch| ch.is_ascii_hexdigit())
            && hex.chars().all(|ch| ch == '0');
    }

    let (mantissa, exponent) =
        value.split_once(['e', 'E']).map_or((value, None), |parts| (parts.0, Some(parts.1)));
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !exponent.chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
    }

    let mut saw_digit = false;
    let mut saw_dot = false;
    for ch in mantissa.chars() {
        match ch {
            '.' if !saw_dot => saw_dot = true,
            '0' => saw_digit = true,
            _ => return false,
        }
    }
    saw_digit && mantissa.chars().all(|ch| ch == '0' || ch == '.')
}

/// Extract `use ...;` statements from Perl source, respecting quotes and `#`
/// comments so multi-line imports and commented-out lines are handled.
fn use_statements(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut in_comment = false;
    let mut escaped = false;
    for c in source.chars() {
        if in_comment {
            if c == '\n' {
                in_comment = false;
            }
            continue;
        }
        // A backslash inside a string escapes the next char, so an escaped
        // quote does not close the string (e.g. `use Foo "a\"b";`).
        if escaped {
            cur.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_single || in_double => {
                escaped = true;
                cur.push(c);
            }
            '#' if !in_single && !in_double => in_comment = true,
            '\'' if !in_double => {
                in_single = !in_single;
                cur.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                cur.push(c);
            }
            ';' if !in_single && !in_double => {
                let trimmed = cur.trim();
                if starts_with_keyword(trimmed, "use") {
                    out.push(trimmed.to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    out
}

/// Whether `stmt` begins with the bareword `keyword` followed by whitespace.
fn starts_with_keyword(stmt: &str, keyword: &str) -> bool {
    stmt.strip_prefix(keyword).is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// Parse a `use Module ...` statement into `(module, raw_args)`.
fn parse_use_statement(stmt: &str) -> Option<(String, String)> {
    let rest = stmt.strip_prefix("use")?;
    let rest = rest.trim_start();
    // Read the module name: identifier chars and `::`.
    let module: String =
        rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':').collect();
    if module.is_empty() {
        return None;
    }
    let args = rest[module.len()..].trim().to_string();
    Some((module, args))
}

#[cfg(test)]
mod tests;
