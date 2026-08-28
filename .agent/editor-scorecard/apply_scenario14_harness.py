#!/usr/bin/env python3
"""Apply the reviewed Scenario 14 oracle repairs deterministically."""

from pathlib import Path

PATH = Path("crates/perl-lsp-ux-tests/tests/ux_scenario_14_inc_conformance.rs")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one anchor, found {count}")
    return text.replace(old, new, 1)


def remove_between(text: str, start: str, end: str, replacement: str, label: str) -> str:
    start_index = text.find(start)
    if start_index < 0:
        raise SystemExit(f"{label}: start anchor not found")
    end_index = text.find(end, start_index)
    if end_index < 0:
        raise SystemExit(f"{label}: end anchor not found")
    if text.find(start, start_index + 1) >= 0:
        raise SystemExit(f"{label}: start anchor is not unique")
    return text[:start_index] + replacement + text[end_index:]


text = PATH.read_text(encoding="utf-8")

text = replace_once(
    text,
    """//! | `scenario_14_findbin_relative` | `use FindBin; use lib "$FindBin::Bin/lib"` | resolves |""",
    """//! | `scenario_14_findbin_relative` | `use FindBin; use lib "$FindBin::Bin/lib"` | environment-dependent; consumers agree |""",
    "FindBin contract table",
)

text = replace_once(
    text,
    """    assert!(
        pl701_absent,
        "Expected no PL701 for GreetModule when includePaths=['lib'] is configured.\\n\\
         diagnostics: {:?}",
        diags
    );

    // Hover result shape check (if non-null).
""",
    """    assert!(
        pl701_absent,
        "Expected no PL701 for GreetModule when includePaths=['lib'] is configured.\\n\\
         diagnostics: {:?}",
        diags
    );
    assert!(
        completion_ok,
        "Expected completion to include GreetModule via includePaths=['lib']; labels={:?}",
        completion_labels(&completions)
    );

    // Hover result shape check (if non-null).
""",
    "relative include completion oracle",
)

text = replace_once(
    text,
    """    assert!(
        pl701_absent,
        "Expected no PL701 for LexicalModule when 'use lib lib' is in source.\\n\\
         diagnostics: {:?}",
        diags
    );

    if let Some(hover) = hover_result {
""",
    """    assert!(
        pl701_absent,
        "Expected no PL701 for LexicalModule when 'use lib lib' is in source.\\n\\
         diagnostics: {:?}",
        diags
    );
    assert!(
        completion_ok,
        "Expected completion to include LexicalModule through in-source 'use lib'; labels={:?}",
        completion_labels(&completions)
    );

    if let Some(hover) = hover_result {
""",
    "use lib completion oracle",
)

text = replace_once(
    text,
    """        "external_include_paths_unauthorized",
        !pl701_fires,
""",
    """        "external_include_paths_unauthorized",
        pl701_fires,
""",
    "external include PL701 receipt polarity",
)

text = replace_once(
    text,
    """    // Consistency check.
    if def_resolves && !pl701_absent {
        return Err(format!(
            "Consumer inconsistency (findbin_relative): goto-def resolved but PL701 fired.\\n\\
             goto-def: {:?}\\n\\
             diagnostics: {:?}",
            defs, diags
        ));
    }
    if !def_resolves && pl701_absent {
        // Both agree module doesn't resolve — log but don't fail the consistency test.
        // FindBin resolution may be in degraded mode in some environments.
        eprintln!(
            "INFO scenario_14_findbin_relative: both consumers agree module does not resolve \
             (def empty + no PL701). FindBin resolution may be in degraded mode."
        );
    }

    // We assert consistency but tolerate FindBin not resolving end-to-end in the
    // UX harness (it's environment-dependent). What we MUST NOT see is divergence.
""",
    """    // FindBin support is environment-dependent, so the terminal contract is
    // consumer agreement rather than unconditional resolution. A module is
    // considered resolved only when diagnostics, completion, and definition agree.
    if def_resolves != pl701_absent {
        return Err(format!(
            "Consumer inconsistency (findbin_relative): goto-def and PL701 disagree.\\n\\
             goto-def: {:?}\\n\\
             diagnostics: {:?}",
            defs, diags
        ));
    }
    assert_eq!(
        completion_ok,
        def_resolves,
        "Consumer inconsistency (findbin_relative): completion and goto-definition disagree.\\n\\
         completion labels: {:?}\\n\\
         goto-def: {:?}",
        completion_labels(&completions),
        defs
    );
    if !def_resolves {
        eprintln!(
            "INFO scenario_14_findbin_relative: diagnostics, completion, and goto-definition \
             consistently report the environment-dependent capability as unavailable."
        );
    }

    // Tolerate FindBin not resolving end-to-end in the UX harness, but never
    // tolerate disagreement between the consumers that report resolution.
""",
    "FindBin symmetric consistency oracle",
)

text = replace_once(
    text,
    """    assert!(
        pl701_absent,
        "scenario_14_perl5lib_env: PL701 should not fire when module resolves via PERL5LIB; diagnostics={diags:?}"
    );

    if let Some(hover) = hover_result {
""",
    """    assert!(
        pl701_absent,
        "scenario_14_perl5lib_env: PL701 should not fire when module resolves via PERL5LIB; diagnostics={diags:?}"
    );
    assert!(
        completion_ok,
        "scenario_14_perl5lib_env: completion should include SystemModule via PERL5LIB; labels={:?}",
        completion_labels(&completions)
    );

    if let Some(hover) = hover_result {
""",
    "PERL5LIB completion oracle",
)

text = replace_once(
    text,
    """    assert!(
        pl701_absent,
        "Expected no PL701 for Nested::Deep when includePaths=['lib'] is configured.\\n\\
         diagnostics: {:?}",
        diags
    );

    if let Some(hover) = hover_result {
""",
    """    assert!(
        pl701_absent,
        "Expected no PL701 for Nested::Deep when includePaths=['lib'] is configured.\\n\\
         diagnostics: {:?}",
        diags
    );
    assert!(
        completion_ok,
        "Expected completion to include Nested::Deep via includePaths=['lib']; labels={:?}",
        completion_labels(&completions)
    );

    if let Some(hover) = hover_result {
""",
    "nested module completion oracle",
)

text = remove_between(
    text,
    'const INCLUDE_MISSING_COMPLETION_SOURCE: &str = "\\\n',
    "/// Completion prefix fixture for the missing-module negative test.",
    "",
    "invalid incomplete-symbol fixture",
)

text = replace_once(
    text,
    """    let hover_result = harness.hover("fixture.pl", 2, 4).expect("hover must not error");

    // Completion check (negative): `MissingFromInclude` should NOT appear since
""",
    """    let hover_result = harness.hover("fixture.pl", 2, 4).expect("hover must not error");
    let hover_not_resolved = hover_is_not_resolved(&hover_result);

    // Completion check (negative): `MissingFromInclude` should NOT appear since
""",
    "missing module hover classification",
)

text = replace_once(
    text,
    """        completion_ok, // "ok" for negative = module absent from completion
        def_empty,
        hover_result.is_none(),
""",
    """        completion_ok, // "ok" for negative = module absent from completion
        def_empty,
        hover_not_resolved,
""",
    "missing module hover receipt",
)

text = replace_once(
    text,
    """    assert!(
        pl701_fires,
        "Expected PL701 for MissingFromInclude when module does not exist.\\n\\
         diagnostics: {:?}",
        diags
    );

    harness.assert_no_crash();
""",
    """    assert!(
        pl701_fires,
        "Expected PL701 for MissingFromInclude when module does not exist.\\n\\
         diagnostics: {:?}",
        diags
    );
    assert!(
        completion_ok,
        "Expected completion to keep MissingFromInclude absent; labels={:?}",
        completion_labels(&completions)
    );
    assert!(
        hover_not_resolved,
        "Expected hover to remain unresolved for MissingFromInclude; got {:?}",
        hover_result
    );

    harness.assert_no_crash();
""",
    "missing module completion and hover oracles",
)

text = remove_between(
    text,
    "#[test]\nfn scenario_14_include_path_missing_module_completion_consistency()",
    "// =============================================================================\n// Fixture 8: PERL5LIB completion gating is independent of useSystemInc",
    """// The former `scenario_14_include_path_missing_module_completion_consistency`
// repeated exact-symbol consumers against a completion-prefix fixture, which
// violates this file's fixture contract. Its valid completion-negative intent
// is enforced by `scenario_14_include_path_missing_module_consistency`, where
// completion uses a prefix and diagnostics/navigation/hover use an exact module.

""",
    "redundant invalid-prefix test",
)

PATH.write_text(text, encoding="utf-8")
