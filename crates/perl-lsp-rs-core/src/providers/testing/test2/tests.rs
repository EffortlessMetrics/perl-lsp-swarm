//! Unit tests for the Test2 fact table (`test2_imports`).

use super::*;

#[test]
fn test2_imports_recognizes_bundles_and_tools() {
    assert!(is_test2_module("Test2::V0"));
    assert!(is_test2_module("Test2::V1"));
    assert!(is_test2_module("Test2::Bundle::Extended"));
    assert!(is_test2_module("Test2::Tools::Basic"));
    assert!(is_test2_module("Test2::Tools::Compare"));
    assert!(is_test2_module("Test2::Plugin::UTF8"));
    assert!(is_test2_module("Test2::API"));

    assert!(!is_test2_module("Test::More"));
    assert!(!is_test2_module("strict"));
    assert!(!is_test2_module("Moose"));
}

#[test]
fn test2_imports_bundle_classification() {
    assert!(is_test2_bundle("Test2::V0"));
    assert!(is_test2_bundle("Test2::V1"));
    assert!(is_test2_bundle("Test2::Bundle::Extended"));
    assert!(is_test2_bundle("Test2::Suite"));

    // Individual tool modules do NOT turn on strict/warnings.
    assert!(!is_test2_bundle("Test2::Tools::Basic"));
    assert!(!is_test2_bundle("Test2::Plugin::UTF8"));
    assert!(!is_test2_bundle("Test2::API"));
}

#[test]
fn test2_imports_v0_default_exports_cover_common_tools() {
    let defaults = module_default_exports("Test2::V0").expect("V0 has a default set");
    for expected in [
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
        "is",
        "isnt",
        "like",
        "unlike",
        "cmp_ok",
        "subtest",
        "isa_ok",
        "can_ok",
        "DOES_ok",
        "dies",
        "lives",
        "try_ok",
        "warns",
        "warning",
        "warnings",
        "no_warnings",
        "ref_ok",
        "mock",
    ] {
        assert!(defaults.contains(&expected), "V0 default set should export `{expected}`");
    }
    // `subtest_buffered` is renamed to `subtest` in V0 — the raw name is not a
    // V0 default export.
    assert!(!defaults.contains(&"subtest_buffered"));
}

#[test]
fn test2_v1_default_exports_only_t2_and_no_pragmas() {
    // Test2::V1's ONLY default export is the `T2()` handle, and it enables NO
    // pragmas by default — its tools are methods on the handle, not bare subs
    // (oracle: metacpan Test2::V1 — "Only 1 export by default: T2()", "NO
    // PRAGMAS ARE ENABLED BY DEFAULT").
    let defaults = module_default_exports("Test2::V1").expect("V1 has a default set");
    assert_eq!(defaults, &["T2"], "V1 default-exports only the T2 handle");

    let facts = Test2Facts::from_source("use Test2::V1;\n");
    assert!(facts.uses_test2_bundle(), "Test2::V1 is still a bundle module");
    assert_eq!((facts.strict, facts.warnings), (false, false), "V1 enables no pragmas by default");
    assert!(facts.is_imported("T2"), "V1 imports the T2 handle");
    assert!(!facts.is_imported("ok"), "V1 does not export bare ok by default");
    assert!(!facts.is_imported("is"), "V1 does not export bare is by default");
    assert!(!facts.is_imported("subtest"), "V1 does not export bare subtest by default");
}

#[test]
fn test2_v1_import_option_brings_in_the_full_bare_set() {
    // `-import` (and its `-i` shorthand) imports all tools as bare subs, like V0.
    for src in ["use Test2::V1 -import;\n", "use Test2::V1 -i;\n"] {
        let facts = Test2Facts::from_source(src);
        assert!(facts.is_imported("ok"), "{src:?}: -import brings in bare ok");
        assert!(facts.is_imported("is"), "{src:?}: -import brings in bare is");
        assert!(facts.is_imported("subtest"), "{src:?}: -import brings in bare subtest");
    }
}

#[test]
fn test2_v1_pragmas_require_an_explicit_option() {
    // V1 strict/warnings only via -strict/-warnings/-p/-pragmas.
    let pragmas = Test2Facts::from_source("use Test2::V1 -pragmas;\n");
    assert_eq!((pragmas.strict, pragmas.warnings), (true, true), "-pragmas enables both");
    let strict_only = Test2Facts::from_source("use Test2::V1 -strict;\n");
    assert_eq!(
        (strict_only.strict, strict_only.warnings),
        (true, false),
        "-strict enables strict only"
    );
}

#[test]
fn test2_v1_grouped_short_flags() {
    // `use Test2::V1 -ipP;` — grouped short flags: -i (import) + -p (pragmas)
    // + -P (plugins). Oracle: metacpan Test2::V1 SYNOPSIS.
    let facts = Test2Facts::from_source("use Test2::V1 -ipP;\n");
    assert!(facts.is_imported("ok"), "grouped -i imports the bare set");
    assert!(facts.is_imported("is"));
    assert!(facts.is_imported("subtest"));
    assert_eq!((facts.strict, facts.warnings), (true, true), "grouped -p enables pragmas");

    // `-P` alone (plugins) is neither import nor pragmas.
    let plugins_only = Test2Facts::from_source("use Test2::V1 -P;\n");
    assert!(!plugins_only.is_imported("ok"), "-P (plugins) does not import the bare set");
    assert_eq!((plugins_only.strict, plugins_only.warnings), (false, false));
}

#[test]
fn v1_short_flag_predicate_boundary_discriminators() {
    // Direct unit tests for the three char-class terms in v1_short_flag's split
    // predicate (`c.is_ascii_alphanumeric() || c == '_' || c == '-'`), each
    // constructed so flipping that term's truth value would flip the result.

    // `c == '-'`: a leading `-` must be kept (not treated as a delimiter) so the
    // flag token survives intact. Without this term, `-i` would split into an
    // empty token and `i` (no leading `-` to strip), and `i` alone would never
    // match because `strip_prefix('-')` requires the leading dash.
    assert!(v1_short_flag("-i", 'i'), "leading '-' must stay attached to the flag letters");

    // `c == '_'`: an embedded underscore must also be kept, so `-i_p` stays one
    // token ("i_p" after stripping the dash) and fails the all-known-letters
    // check (the '_' is not in {i,p,P,x}). If underscore were instead treated as
    // a delimiter, `-i_p` would split into `-i` and `p`, and `-i` alone would
    // match the import flag — a different (wrong) result.
    assert!(!v1_short_flag("-i_p", 'i'), "embedded '_' must not act as a delimiter");
    assert!(!v1_short_flag("-i_p", 'p'), "embedded '_' must not act as a delimiter");

    // `c.is_ascii_alphanumeric()`: an embedded digit must also be kept (kept
    // together via alphanumeric, not just alphabetic), so `-i2p` stays one token
    // ("i2p") and fails the all-known-letters check (the '2' is not in
    // {i,p,P,x}). If digits were treated as delimiters instead, `-i2p` would
    // split into `-i` and `p`, and `-i` alone would match — a different (wrong)
    // result.
    assert!(!v1_short_flag("-i2p", 'i'), "embedded digit must not act as a delimiter");
    assert!(!v1_short_flag("-i2p", 'p'), "embedded digit must not act as a delimiter");

    // Sanity: the true-match path still works once the token is exactly the
    // known short-flag letters (no interfering '_'/digit).
    assert!(v1_short_flag("-ip", 'p'), "clean grouped short flags still match");
}

#[test]
fn v1_import_all_predicate_exact_module_match_boundary() {
    // `resolve_import`'s `v1_import_all` boundary (test2.rs:324) is
    // `module == "Test2::V1" && (...)`. Hold the raw args fixed at `-import`
    // and flip only the module name: for Test2::V1 this pulls in the full bare
    // tool set (V0_DEFAULT); for any other Test2 module, `-import` is not a
    // recognized option and has no effect on that module's own default set.
    // This proves the predicate needs an *exact* "Test2::V1" match, not "any
    // bundle" or "any Test2 module reachable via is_test2_module".
    let v1 = resolve_import("Test2::V1", "-import").expect("Test2::V1 recognized");
    assert!(v1.symbols.contains("subtest"), "-import brings the full bare set into Test2::V1");
    assert!(v1.symbols.contains("ok"));

    let basic =
        resolve_import("Test2::Tools::Basic", "-import").expect("Test2::Tools::Basic recognized");
    assert!(
        !basic.symbols.contains("subtest"),
        "-import has no special meaning outside Test2::V1; Test2::Tools::Basic keeps its own          fixed default set"
    );
}

#[test]
fn v1_pragma_default_predicate_exact_module_match_boundary() {
    // `resolve_import`'s bundle-pragma boundary (test2.rs:336) is
    // `module == "Test2::V1"`. Hold the raw args fixed (empty import list) and
    // flip only the module name between two bundles: Test2::V1 (no pragmas by
    // default) and Test2::Suite (pragmas on by default, like Test2::V0). This
    // proves the predicate needs an *exact* "Test2::V1" match, not "any
    // bundle".
    let v1 = resolve_import("Test2::V1", "").expect("Test2::V1 recognized");
    assert_eq!(
        v1.pragmas,
        Some(Test2Pragmas { strict: false, warnings: false }),
        "Test2::V1 enables no pragmas by default"
    );

    let suite = resolve_import("Test2::Suite", "").expect("Test2::Suite recognized");
    assert_eq!(
        suite.pragmas,
        Some(Test2Pragmas { strict: true, warnings: true }),
        "Test2::Suite (a bundle that is not Test2::V1) enables both pragmas by default"
    );
}

#[test]
fn test2_imports_plain_use_v0_scope_and_pragmas() {
    let facts = Test2Facts::from_source("use Test2::V0;\nok(1);\ndone_testing;\n");
    assert!(facts.uses_test2());
    assert!(facts.uses_test2_bundle());
    assert!(facts.is_imported("ok"));
    assert!(facts.is_imported("done_testing"));
    assert!(facts.is_imported("is"));
    assert!(facts.is_imported("subtest"));
    // Plain `use Test2::V0;` turns strict + warnings on.
    assert!(facts.strict);
    assert!(facts.warnings);
    assert!(!facts.is_imported("find_user"), "unrelated names are not imported");
}

#[test]
fn test2_imports_no_strict_option_disables_strict_only() {
    let facts = Test2Facts::from_source("use Test2::V0 -no_strict => 1;\nok(1);\n");
    assert!(!facts.strict, "-no_strict disables strict");
    assert!(facts.warnings, "-no_strict leaves warnings on");
    assert!(facts.is_imported("ok"));
}

#[test]
fn test2_imports_no_warnings_option_disables_warnings_only() {
    let facts = Test2Facts::from_source("use Test2::V0 -no_warnings;\nok(1);\n");
    assert!(facts.strict);
    assert!(!facts.warnings);
}

#[test]
fn test2_imports_no_pragmas_disables_both() {
    let facts = Test2Facts::from_source("use Test2::V0 -no_pragmas;\nok(1);\n");
    assert!(!facts.strict);
    assert!(!facts.warnings);
}

#[test]
fn test2_imports_exclusion_removes_symbol() {
    let resolved = resolve_import("Test2::V0", "'!ok'").expect("recognized module");
    assert!(!resolved.symbols.contains("ok"), "!ok excludes ok");
    assert!(resolved.symbols.contains("is"), "other defaults remain");
    // Exclusion alone keeps pragmas on.
    assert_eq!(resolved.pragmas, Some(Test2Pragmas { strict: true, warnings: true }));
}

#[test]
fn test2_imports_rename_as_installs_alias_not_original() {
    let resolved =
        resolve_import("Test2::V0", "ok => {-as => 'my_ok'}").expect("recognized module");
    assert!(resolved.symbols.contains("my_ok"), "alias my_ok is imported");
    assert!(!resolved.symbols.contains("ok"), "original ok is not imported after rename");
    // A rename is an explicit selection: only the alias is in scope.
    assert!(!resolved.symbols.contains("is"), "explicit selection replaces the default set");
}

#[test]
fn test2_imports_default_tag_keeps_full_set_with_rename() {
    let resolved =
        resolve_import("Test2::V0", "':DEFAULT', ok => {-as => 'my_ok'}").expect("recognized");
    assert!(resolved.symbols.contains("my_ok"));
    assert!(resolved.symbols.contains("is"), ":DEFAULT restores the default set");
}

#[test]
fn test2_imports_explicit_qw_list_replaces_default() {
    let resolved = resolve_import("Test2::V0", "qw/ok is/").expect("recognized module");
    assert!(resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("is"));
    assert!(!resolved.symbols.contains("like"), "explicit list does not pull in the rest");
}

#[test]
fn test2_imports_prefix_rename() {
    let resolved = resolve_import("Test2::V0", "ok => {-prefix => 'my_'}").expect("recognized");
    assert!(resolved.symbols.contains("my_ok"));
    assert!(!resolved.symbols.contains("ok"));
}

#[test]
fn test2_imports_standalone_tool_has_no_pragmas() {
    let facts = Test2Facts::from_source("use Test2::Tools::Basic;\nok(1);\n");
    assert!(facts.is_imported("ok"));
    assert!(facts.is_imported("done_testing"));
    // A bare tool import does not enable strict/warnings.
    assert!(!facts.strict);
    assert!(!facts.warnings);
}

#[test]
fn test2_imports_compare_standalone_default_is_narrow() {
    // Standalone Test2::Tools::Compare only default-exports is/like.
    let defaults = module_default_exports("Test2::Tools::Compare").expect("known tool");
    assert_eq!(defaults, &["is", "like"]);
    // But an explicit import of an EXPORT_OK symbol is trusted verbatim.
    let resolved = resolve_import("Test2::Tools::Compare", "qw/hash array/").expect("recognized");
    assert!(resolved.symbols.contains("hash"));
    assert!(resolved.symbols.contains("array"));
}

#[test]
fn test2_imports_subtest_tool_standalone_names() {
    let defaults = module_default_exports("Test2::Tools::Subtest").expect("known tool");
    assert!(defaults.contains(&"subtest_streamed"));
    assert!(defaults.contains(&"subtest_buffered"));
    // The bundle-level `subtest` alias is not a standalone-tool default name.
    assert!(!defaults.contains(&"subtest"));
}

#[test]
fn test2_imports_ignores_commented_out_use() {
    let facts = Test2Facts::from_source("# use Test2::V0;\nmy $x = 1;\n");
    assert!(!facts.uses_test2(), "commented-out use must not register");
    assert!(!facts.strict);
}

#[test]
fn test2_imports_multiline_import_list() {
    let src = "use Test2::V0 qw(\n    ok\n    is\n);\n";
    let facts = Test2Facts::from_source(src);
    assert!(facts.is_imported("ok"));
    assert!(facts.is_imported("is"));
    assert!(!facts.is_imported("like"));
    assert!(facts.strict, "bundle still enables strict with an explicit list");
}

#[test]
fn test2_imports_non_test2_module_returns_none() {
    assert!(resolve_import("Test::More", "").is_none());
    assert!(resolve_import("strict", "").is_none());
}

#[test]
fn test2_imports_target_option_does_not_drop_exports() {
    let resolved = resolve_import("Test2::V0", "-target => 'Foo::Bar'").expect("recognized");
    // -target is an option, not a symbol selection, so the default set stays.
    assert!(resolved.symbols.contains("ok"));
    assert!(resolved.symbols.contains("is"));
    assert_eq!(resolved.pragmas, Some(Test2Pragmas { strict: true, warnings: true }));
}

#[test]
fn expand_qw_only_fires_on_word_boundary_and_real_delimiter() {
    // Genuine qw list expands to space-separated words.
    assert_eq!(expand_qw("qw(ok is like)"), " ok is like ");
    assert_eq!(expand_qw("qw{ok is}"), " ok is ");
    // `qw` preceded by a word char (`eqw`) is not the operator — must stay intact.
    assert_eq!(expand_qw("eqw(x)"), "eqw(x)");
    // A word char immediately after `qw` is not a delimiter (`qwoo` is a bareword).
    assert_eq!(expand_qw("qwoo"), "qwoo");
    // A symbol name embedding `qw` at a word boundary is left intact.
    assert_eq!(expand_qw("my_qw"), "my_qw");
}

#[test]
fn expand_qw_does_not_panic_on_non_ascii() {
    // `raw[i..]` must never split a multi-byte codepoint (critic-path panic).
    assert_eq!(expand_qw("qw(ok café)"), " ok café ");
    // Non-ASCII outside a qw list is copied through unchanged.
    assert_eq!(expand_qw("-target => 'Café::Módulo'"), "-target => 'Café::Módulo'");
    // A multi-byte char immediately before `qw` still parses safely.
    assert_eq!(expand_qw("café qw(ok)"), "café  ok ");
    // A non-ASCII byte must never be treated as a qw delimiter: doing so would
    // set `close` to a lead byte and slice `raw` mid-codepoint (panic). Here the
    // qw is left unexpanded rather than crashing.
    assert_eq!(expand_qw("qw é a é b"), "qw é a é b");
}

#[test]
fn test2_non_ascii_target_does_not_panic() {
    // Reaches expand_qw via the critic path; must not crash on valid UTF-8.
    let facts = Test2Facts::from_source("use Test2::V0 -target => 'Café::Módulo';\n");
    assert!(facts.is_imported("ok"));
    assert_eq!((facts.strict, facts.warnings), (true, true));
}

#[test]
fn test2_empty_import_list_imports_nothing_and_no_pragmas() {
    // `use Test2::V0 ();` loads the module but does not call import(): no
    // symbols, and no strict/warnings are provided.
    let resolved = resolve_import("Test2::V0", "()").expect("module still recognized");
    assert!(resolved.symbols.is_empty(), "empty () import must import no symbols");
    assert_eq!(resolved.pragmas, None, "empty () import provides no pragmas");

    let facts = Test2Facts::from_source("use Test2::V0 ();\n");
    assert!(facts.modules.iter().any(|m| m == "Test2::V0"), "module is still loaded");
    assert!(!facts.is_imported("ok"), "no default exports for ()");
    assert_eq!((facts.strict, facts.warnings), (false, false), "() provides no pragmas");
}

#[test]
fn use_statements_survive_escaped_quotes() {
    // An escaped quote inside a double-quoted string must not close the string
    // early; otherwise the `;` terminator is missed and the following `use`
    // statement is never seen. The second Test2 module only appears if the
    // first statement terminated at the right `;`.
    let src = "use Test2::V0 -target => \"a\\\"b\";\nuse Test2::Tools::Basic;\n";
    let facts = Test2Facts::from_source(src);
    assert!(facts.modules.iter().any(|m| m == "Test2::V0"), "modules: {:?}", facts.modules);
    assert!(
        facts.modules.iter().any(|m| m == "Test2::Tools::Basic"),
        "second statement lost after escaped quote: {:?}",
        facts.modules
    );
}

// ---------------------------------------------------------------------------
// Transform-recognizer failure boundary (#13690)
//
// `-as`/`-prefix`/`-postfix` recognition is a bounded compatibility bridge.
// These regressions pin two things its previous `unreachable!()` initializers
// could not: recognizer failure must not abort the server, and it must not
// degrade into bareword scanning that reports transform syntax as imports.
// ---------------------------------------------------------------------------

/// Resolve with both transform recognizers forced unavailable.
fn resolve_without_recognizers(module: &str, raw_args: &str) -> ResolvedImportAnalysis {
    resolve_import_with(module, raw_args, None, None).expect("recognized module")
}

fn resolve_with_analysis(module: &str, raw_args: &str) -> ResolvedImportAnalysis {
    resolve_import_with(module, raw_args, RENAME_AS.as_ref(), RENAME_FIX.as_ref())
        .expect("recognized module")
}

#[test]
fn test2_unavailable_as_recognizer_leaks_no_alias_or_option_atoms() {
    let resolved = resolve_without_recognizers("Test2::V0", "ok => {-as => 'my_ok'}");

    for leaked in ["my_ok", "ok", "as", "-as"] {
        assert!(
            !resolved.resolved.symbols.contains(leaked),
            "{leaked} must not be imported when the -as recognizer is unavailable, got {:?}",
            resolved.resolved.symbols
        );
    }
    assert!(resolved.resolved.symbols.is_empty(), "unresolved transform syntax proves no symbol");
    assert!(resolved.analysis_limited, "degraded analysis is explicit");
}

#[test]
fn test2_unavailable_prefix_recognizer_leaks_no_mapping_atoms() {
    let resolved = resolve_without_recognizers("Test2::V0", "ok => {-prefix => 'my_'}");

    for leaked in ["my_ok", "ok", "my_", "prefix", "-prefix"] {
        assert!(
            !resolved.resolved.symbols.contains(leaked),
            "{leaked} must not be imported when the -prefix recognizer is unavailable"
        );
    }
    assert!(resolved.analysis_limited);
}

#[test]
fn test2_unavailable_postfix_recognizer_leaks_no_mapping_atoms() {
    let resolved = resolve_without_recognizers("Test2::V0", "ok => {-postfix => '_mine'}");

    for leaked in ["ok_mine", "ok", "_mine", "postfix", "-postfix"] {
        assert!(
            !resolved.resolved.symbols.contains(leaked),
            "{leaked} must not be imported when the -postfix recognizer is unavailable"
        );
    }
    assert!(resolved.analysis_limited);
}

#[test]
fn test2_malformed_transform_does_not_fall_through_to_bareword_scan() {
    // Unclosed rename map: the recognizer cannot match, so its bytes remain.
    // The bareword scan must not run over them (it would report `ok` and
    // `my_ok` as imported).
    let resolved = resolve_with_analysis("Test2::V0", "ok => {-as => 'my_ok'");

    assert!(!resolved.resolved.symbols.contains("ok"), "malformed transform must not import `ok`");
    assert!(
        !resolved.resolved.symbols.contains("my_ok"),
        "malformed transform must not import `my_ok`"
    );
    assert!(resolved.resolved.symbols.is_empty());
    assert!(resolved.analysis_limited, "malformed transform is a limited analysis");
}

#[test]
fn test2_recognizers_available_preserve_transform_semantics() {
    // Negative control for the fail-closed guard: with recognizers available,
    // every currently supported transform form still resolves exactly. A
    // guard that always failed closed would fail this test.
    let as_form = resolve_with_analysis("Test2::V0", "ok => {-as => 'my_ok'}");
    assert!(as_form.resolved.symbols.contains("my_ok"));
    assert!(!as_form.analysis_limited);

    let prefix = resolve_with_analysis("Test2::V0", "ok => {-prefix => 'my_'}");
    assert!(prefix.resolved.symbols.contains("my_ok"));
    assert!(!prefix.analysis_limited);

    let postfix = resolve_with_analysis("Test2::V0", "ok => {-postfix => '_mine'}");
    assert!(postfix.resolved.symbols.contains("ok_mine"));
    assert!(!postfix.analysis_limited);

    let with_tag = resolve_with_analysis("Test2::V0", "':DEFAULT', ok => {-as => 'my_ok'}");
    assert!(with_tag.resolved.symbols.contains("my_ok"));
    assert!(with_tag.resolved.symbols.contains("is"));
    assert!(!with_tag.analysis_limited);
}

#[test]
fn test2_ordinary_imports_are_unaffected_by_the_transform_guard() {
    // No transform syntax: the ordinary fallback still runs, and an
    // unavailable recognizer is irrelevant to statements that never use it.
    for (module, args) in
        [("Test2::V0", ""), ("Test2::Tools::Compare", "qw/is like/"), ("Test2::V0", "':ALL'")]
    {
        let available = resolve_import(module, args).expect("recognized module");
        let unavailable = resolve_without_recognizers(module, args);
        assert_eq!(
            available, unavailable.resolved,
            "no-transform statement `use {module} {args};` must not depend on the recognizer"
        );
    }

    let ordinary = resolve_import("Test2::Tools::Compare", "qw/is like/").expect("recognized");
    assert!(ordinary.symbols.contains("is"));
    assert!(ordinary.symbols.contains("like"));
}

#[test]
fn test2_option_shaped_symbol_is_retained_when_actually_imported() {
    // The guard recognizes transform options by role (token followed by `=>`),
    // not by a bareword blacklist. A legitimate export sharing the spelling
    // must survive.
    let bareword = resolve_with_analysis("Test2::V0", "qw/ok as prefix postfix/");
    for kept in ["ok", "as", "prefix", "postfix"] {
        assert!(bareword.resolved.symbols.contains(kept), "{kept} is an ordinary import entry");
    }
    assert!(!bareword.analysis_limited, "no transform syntax is present");

    // An option-looking atom that is not in option position (no `=>`) is not
    // transform syntax either.
    let not_option = resolve_with_analysis("Test2::V0", "qw/ok/, '-as'");
    assert!(!not_option.analysis_limited, "`-as` without `=>` is not transform syntax");
    assert!(not_option.resolved.symbols.contains("ok"));
}

#[test]
fn test2_transform_detection_requires_option_position() {
    assert!(contains_transform_syntax("ok => {-as => 'my_ok'}"));
    assert!(contains_transform_syntax("ok => {-prefix => 'my_'}"));
    assert!(contains_transform_syntax("ok => {-postfix => '_x'}"));
    // The quoted option spelling is the same syntax.
    assert!(contains_transform_syntax("ok => {'-as' => 'my_ok'}"));
    assert!(contains_transform_syntax("ok => {\"-prefix\" => 'my_'}"));
    // An option-shaped substring in a quoted target is data, not transform
    // syntax.  The detector must not fail closed on an ordinary target value.
    assert!(!contains_transform_syntax("ok => {target => '-as => my_ok'}"));
    assert!(!contains_transform_syntax("ok => {target => \"-prefix => my_\"}"));
    // Not in option position.
    assert!(!contains_transform_syntax("qw/ok as/"));
    assert!(!contains_transform_syntax("'-as'"));
    // A longer word merely opening with an option spelling is not the option.
    assert!(!contains_transform_syntax("-aside => 1"));
    // `-no_as` has no literal `-as` token at all.
    assert!(!contains_transform_syntax("-no_as => 1"));
}

#[test]
fn test2_no_recognizer_owned_span_reaches_the_bareword_scan() {
    // Load-bearing safety property, stated where it actually bites: for any
    // import text a recognizer matches, `resolve_import` must never fall
    // through to the ordinary bareword scan. That scan would report the span's
    // structural atoms — the container key, and an original a rename removes —
    // as imported symbols, which is the exact leak this change closes.
    //
    // Two ways the property can break, both represented below:
    //   * a detector boundary rule narrower than `[^}]*?` (word-char prefixes);
    //   * the detector and the recognizers disagreeing about quoted values.
    let corpus = [
        "ok => {-as => 'my_ok'}",
        "ok => { -as => 'my_ok' }",
        "ok => {x-as => 'y'}",
        "ok => {1-as => 'y'}",
        "ok => {a-prefix => 'p'}",
        "ok => {z-postfix => 's'}",
        "ok => {-prefix => 'my_'}",
        "ok => {-postfix => '_x'}",
        "':DEFAULT', ok => {-as => 'my_ok'}",
        "ok => {-as=>'my_ok'}",
        "ok => {other => 1, -as => 'my_ok'}",
        // Option-shaped text inside a quoted value: the detector treats it as
        // data, the regex bridge still matches across it. The disagreement
        // must resolve fail-closed, never into a bareword scan.
        "ok => {target => '-as => my_ok'}",
        "ok => {target => \"-prefix => my_\"}",
    ];

    let rename_as = RENAME_AS.as_ref().expect("static -as pattern compiles");
    let rename_fix = RENAME_FIX.as_ref().expect("static -prefix/-postfix pattern compiles");

    let leaked: Vec<(&str, TransformScan)> = corpus
        .into_iter()
        .filter(|case| rename_as.is_match(case) || rename_fix.is_match(case))
        .map(|case| (case, scan_import_transforms(case, Some(rename_as), Some(rename_fix))))
        .filter(|(_, scan)| matches!(scan, TransformScan::None))
        .collect();

    let uncovered: Vec<&str> = leaked.into_iter().map(|(case, _)| case).collect();

    assert!(
        uncovered.is_empty(),
        "a recognizer owns these spans but the scan resolved to None, \
         so they would reach the bareword scan: {uncovered:?}"
    );
}

#[test]
fn test2_quoted_option_shaped_value_never_imports_container_atoms() {
    // The concrete regression the property above prevents. `target` is a hash
    // key and `ok` is the rename's original, which a real `-as` removes from
    // scope; neither is an imported symbol under any reading. Reporting them —
    // and reporting them as a clean result — is a fabricated fact.
    for args in ["ok => {target => '-as => my_ok'}", "ok => {target => \"-prefix => my_\"}"] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(
            !resolved.resolved.symbols.contains("target"),
            "{args}: container key must never be an imported symbol, got {:?}",
            resolved.resolved.symbols
        );
        assert!(
            !resolved.resolved.symbols.contains("ok"),
            "{args}: rename original must not be imported, got {:?}",
            resolved.resolved.symbols
        );
        assert!(resolved.analysis_limited, "{args}: an unresolved span is never a clean result");
    }
}

#[test]
fn test2_quoted_transform_option_fails_closed_instead_of_fabricating_imports() {
    // `{'-as' => 'my_ok'}` is transform syntax the recognizer does not accept
    // (its pattern requires the bare `-as` token). Before the transform
    // boundary existed this fell through to the bareword scan and reported
    // BOTH `ok` and `my_ok` as imported — the original is not even in scope
    // after a rename, so that was a fabricated fact reported as clean.
    let resolved = resolve_with_analysis("Test2::V0", "ok => {'-as' => 'my_ok'}");

    assert!(!resolved.resolved.symbols.contains("ok"), "renamed original must not be imported");
    assert!(!resolved.resolved.symbols.contains("my_ok"), "unparsed alias must not be imported");
    assert!(resolved.resolved.symbols.is_empty());
    assert!(resolved.analysis_limited, "an uninterpreted transform is never reported as clean");
}

#[test]
fn test2_limited_analysis_is_distinguishable_from_a_proven_empty_import() {
    // `use Test2::V0 ();` genuinely imports nothing — that is a proven fact.
    let proven_empty = resolve_with_analysis("Test2::V0", "()");
    assert!(proven_empty.resolved.symbols.is_empty());
    assert!(!proven_empty.analysis_limited, "an explicit empty import list is proven, not limited");

    // A failed transform also yields no symbols, but must not be readable as
    // the same proven fact.
    let limited = resolve_with_analysis("Test2::V0", "ok => {-as => 'my_ok'");
    assert!(limited.resolved.symbols.is_empty());
    assert!(limited.analysis_limited);

    assert_ne!(
        proven_empty, limited,
        "a completeness-sensitive consumer must be able to tell these apart"
    );
}

#[test]
fn test2_facts_propagate_limited_analysis_across_a_file() {
    let analyzed = Test2Facts::from_source_with_analysis(
        "use Test2::V0;\nuse Test2::Tools::Compare qw/is/, ok => {-as => 'my_ok';\n",
    );
    assert!(analyzed.facts.uses_test2());
    assert!(analyzed.analysis_limited, "one failed statement makes the file's import set unproven");
    // The clean statement's facts survive; the failed one contributes nothing.
    assert!(analyzed.facts.is_imported("ok"), "the clean Test2::V0 bundle still resolves");

    let clean = Test2Facts::from_source_with_analysis("use Test2::V0;\n");
    assert!(!clean.analysis_limited);
}

#[test]
fn test2_transform_resolution_is_deterministic_across_repeated_calls() {
    // The recognizers are compiled once and hold no resettable state; repeated
    // resolution of the same statement is byte-identical.
    let first = resolve_with_analysis("Test2::V0", "ok => {-as => 'my_ok'}");
    for _ in 0..5 {
        let again = resolve_with_analysis("Test2::V0", "ok => {-as => 'my_ok'}");
        assert_eq!(first, again);
    }

    let failed_first = resolve_without_recognizers("Test2::V0", "ok => {-as => 'my_ok'}");
    for _ in 0..5 {
        let failed_again = resolve_without_recognizers("Test2::V0", "ok => {-as => 'my_ok'}");
        assert_eq!(failed_first, failed_again);
    }
}

#[test]
fn test2_quote_like_target_payload_is_data_not_transform_syntax() {
    // Review finding (#14651, three independent reviewers). `-target` takes
    // Perl quote-like values, and this module already treats them as opaque.
    // Before masking them, an option-shaped payload such as `q{-as => Foo}`
    // tripped the detector's role predicate, marked the whole statement
    // limited, and dropped every genuinely imported symbol — including the
    // default bundle set. Withholding valid imports is exactly the
    // user-visible failure the fail-closed bias is supposed to avoid.
    for args in [
        "-target => q{-as => Foo}, ok",
        "-target => qq{-prefix => Foo}, ok",
        "-target => qx{-postfix => Foo}, ok",
    ] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(
            !resolved.analysis_limited,
            "{args}: a quote-like payload is data, so analysis is not limited"
        );
        assert!(
            resolved.resolved.symbols.contains("ok"),
            "{args}: the explicit import must survive, got {:?}",
            resolved.resolved.symbols
        );
    }

    // Parity with the already-supported ordinary-quote spelling.
    let quoted = resolve_with_analysis("Test2::V0", "-target => '-as => Foo', ok");
    assert!(!quoted.analysis_limited);
    assert!(quoted.resolved.symbols.contains("ok"));
}

#[test]
fn test2_quote_like_masking_does_not_uncover_a_real_transform() {
    // The masking must not become a way to smuggle transform syntax past the
    // detector. A real rename beside a quote-like value is still recognized,
    // and a recognizer-owned span still fails closed rather than resolving.
    let real = resolve_with_analysis("Test2::V0", "-target => q{Foo}, ok => {-as => 'my_ok'}");
    assert!(!real.analysis_limited, "a recognized rename is not a limited analysis");
    assert!(real.resolved.symbols.contains("my_ok"), "the alias is installed");
    assert!(!real.resolved.symbols.contains("ok"), "the original is replaced by the alias");

    // The quoted-value disagreement case stays fail-closed (see
    // `test2_quoted_option_shaped_value_never_imports_container_atoms`).
    let disagreement = resolve_with_analysis("Test2::V0", "ok => {target => '-as => my_ok'}");
    assert!(disagreement.analysis_limited);
    assert!(disagreement.resolved.symbols.is_empty());
}

#[test]
fn test2_quote_like_option_key_is_syntax_not_data() {
    // Review finding (@devin-ai-integration on #14651). `q{-as}` evaluates to
    // the same option key as `'-as'`, so masking it as data hid the transform
    // from BOTH the detector and the recognizers. With neither seeing it there
    // is no disagreement for the fail-closed guard to catch, and the bareword
    // scan reported the map's atoms as imports:
    //
    //   ok => {q{-as} => 'my_ok'}  ->  symbols {"my_ok", "ok"}, limited=false
    //
    // Both are fabricated — `ok` is the rename's original, `my_ok` was never
    // parsed — and it claimed to be a clean result.
    for (args, alias) in [
        ("ok => {q{-as} => 'my_ok'}", "my_ok"),
        ("ok => {q{-prefix} => 'my_'}", "my_"),
        ("ok => {q{-postfix} => '_mine'}", "_mine"),
        ("ok => {qq[-as] => 'my_ok'}", "my_ok"),
    ] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(
            resolved.analysis_limited,
            "{args}: an uninterpreted option key is never a clean result"
        );
        assert!(
            !resolved.resolved.symbols.contains("ok"),
            "{args}: rename original must not be imported, got {:?}",
            resolved.resolved.symbols
        );
        assert!(
            !resolved.resolved.symbols.contains(alias),
            "{args}: unparsed alias must not be imported, got {:?}",
            resolved.resolved.symbols
        );
        assert!(resolved.resolved.symbols.is_empty());
    }

    // Control: the same quote-like operators carrying an ordinary payload stay
    // data, so `-target` values still resolve (the previous review finding).
    let payload = resolve_with_analysis("Test2::V0", "-target => q{-as => Foo}, ok");
    assert!(!payload.analysis_limited, "an option-shaped payload is still data");
    assert!(payload.resolved.symbols.contains("ok"));

    // Unit-level boundary for the key/payload split.
    assert_eq!(quote_like_option_key("q{-as}"), Some("-as"));
    assert_eq!(quote_like_option_key("qq[-prefix]"), Some("-prefix"));
    assert_eq!(quote_like_option_key("q{-as => Foo}"), None, "a payload is not a key");
    assert_eq!(quote_like_option_key("q{Foo}"), None);
}

#[test]
fn test2_non_string_quote_operators_are_not_option_keys() {
    // Review finding (@devin-ai-integration on #14651). The option-key rescue
    // originally accepted any quote-like operator, but only the string-yielding
    // ones evaluate to the literal option text. `qr{-as}` is a compiled
    // pattern, `qx{-as}` runs a command and yields its output, `m{-as}` yields
    // a match result — none of them is the key `-as`, so treating them as one
    // failed the statement closed and dropped valid imports.
    for operator in ["qr", "qx", "m"] {
        assert_eq!(
            quote_like_option_key(&format!("{operator}{{-as}}")),
            None,
            "{operator}{{-as}} does not evaluate to the option text"
        );
    }

    // The string-yielding operators still are keys.
    assert_eq!(quote_like_option_key("q{-as}"), Some("-as"));
    assert_eq!(quote_like_option_key("qq{-as}"), Some("-as"));
    assert_eq!(quote_like_option_key("qw{-as}"), Some("-as"));
    assert_eq!(quote_like_option_key("qq[-prefix]"), Some("-prefix"));

    // The operator must be the whole token: `qr`/`qx` must not be read as `q`
    // plus a delimiter, which is what would silently readmit them.
    assert_eq!(quote_like_option_key("qr[-as]"), None);
    assert_eq!(quote_like_option_key("qx[-as]"), None);

    // End to end: a non-string operator in key position is not a transform, so
    // the statement is not failed closed on its account.
    let regex_key = resolve_with_analysis("Test2::V0", "-target => qr{-as}, ok");
    assert!(!regex_key.analysis_limited, "a qr// value is not an option key");
    assert!(regex_key.resolved.symbols.contains("ok"), "the explicit import survives");
}

#[test]
fn test2_bare_match_payload_is_data_not_transform_syntax() {
    // Review finding (@chatgpt-codex-connector on #14651). A bare `/.../` match
    // carries no operator, so the quote-like scan cannot see it, yet its payload
    // is data exactly like `m{...}`. An option-shaped pattern therefore tripped
    // the role predicate and dropped the genuinely imported symbol:
    //
    //   -target => scalar(/-as => Foo/), ok  ->  limited=true, ok absent
    for args in [
        "-target => scalar(/-as => Foo/), ok",
        "-target => scalar(/-prefix => Foo/), ok",
        "-target => scalar(/-postfix => Foo/i), ok",
    ] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(!resolved.analysis_limited, "{args}: a match payload is data");
        assert!(
            resolved.resolved.symbols.contains("ok"),
            "{args}: the explicit import must survive, got {:?}",
            resolved.resolved.symbols
        );
    }

    // Division must stay visible: masking from a `/` that follows a term would
    // swallow real import text up to the next `/`.
    assert!(!bare_match_can_start("$count "), "a `/` after a variable is division");
    assert!(!bare_match_can_start("f(1) "), "a `/` after a closing paren is division");
    assert!(!bare_match_can_start("'Foo' "), "a `/` after a quoted term is division");
    assert!(bare_match_can_start("scalar("), "a `/` after an opener starts a match");
    assert!(bare_match_can_start("-target => "), "a `/` after a fat comma starts a match");
    assert!(bare_match_can_start(""), "a leading `/` starts a match");

    // The `qw//` list form is still consumed by the operator path, not by the
    // bare-match path, so ordinary imports are untouched.
    let list = resolve_with_analysis("Test2::Tools::Compare", "qw/is like/");
    assert!(!list.analysis_limited);
    assert!(list.resolved.symbols.contains("is") && list.resolved.symbols.contains("like"));
}

#[test]
fn test2_word_operator_prefixed_match_is_still_data() {
    // Review finding (@devin-ai-integration on #14651). The bare-match
    // discriminator looked only at the preceding character, so a match after a
    // word operator (`grep /.../`) read as division: the payload stayed visible,
    // tripped the role predicate, and dropped the valid import.
    for args in [
        "-target => scalar(grep /-as => Foo/, @values), ok",
        "-target => scalar(map /-prefix => Foo/, @values), ok",
        "-target => scalar(split /-postfix => Foo/, $text), ok",
    ] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(!resolved.analysis_limited, "{args}: a match payload is data");
        assert!(
            resolved.resolved.symbols.contains("ok"),
            "{args}: the explicit import must survive, got {:?}",
            resolved.resolved.symbols
        );
    }

    // The token, not its last character, decides.
    assert!(bare_match_can_start("scalar(grep "), "grep takes a pattern");
    assert!(bare_match_can_start("return "), "return takes a pattern");
    assert!(!bare_match_can_start("$grep "), "a sigil makes it a variable, not the operator");
    assert!(!bare_match_can_start("mygrep "), "a longer identifier is not the operator");
    assert!(!bare_match_can_start("count "), "an ordinary identifier divides");
}

#[test]
fn test2_backtick_command_payload_is_data_not_transform_syntax() {
    // Review finding (@chatgpt-codex-connector on #14651). Backticks are the
    // shorthand for `qx//`, but they carry no operator letters, so neither the
    // quote-like scan nor the quoted-string branch saw them. An option-shaped
    // command payload tripped the role predicate and dropped the valid import.
    for args in [
        "-target => `echo -as => Foo`, ok",
        "-target => `echo -prefix => Foo`, ok",
        "-target => `echo -postfix => Foo`, ok",
    ] {
        let resolved = resolve_with_analysis("Test2::V0", args);
        assert!(!resolved.analysis_limited, "{args}: a command payload is data");
        assert!(
            resolved.resolved.symbols.contains("ok"),
            "{args}: the explicit import must survive, got {:?}",
            resolved.resolved.symbols
        );
    }

    // A backtick expression yields command output, never the literal option
    // text, so unlike `'-as'` it can never be rescued as an option key.
    assert!(!contains_transform_syntax("ok => {`-as` => 'my_ok'}"));
}

#[test]
fn test2_punctuation_named_variable_divides() {
    // Review finding (@chatgpt-codex-connector on #14651). `$?` is a complete
    // term, so a following `/` is division; the character-only rule read the
    // `?` as punctuation and admitted a match, which would mask forward from a
    // division slash and swallow real import text.
    assert!(!bare_match_can_start("$? "), "$? is a term, so `/` divides");
    assert!(!bare_match_can_start("$! "), "$! is a term, so `/` divides");
    assert!(!bare_match_can_start("$_ "), "$_ is a term, so `/` divides");

    // Still a match after an opener or a word operator.
    assert!(bare_match_can_start("scalar("), "an opener still admits a match");
    assert!(bare_match_can_start("grep "), "a word operator still admits a match");

    let divided = resolve_with_analysis("Test2::V0", "-target => scalar($? / 2 + 3 / 4), ok");
    assert!(!divided.analysis_limited, "division must not be masked as a match");
    assert!(divided.resolved.symbols.contains("ok"));
}
