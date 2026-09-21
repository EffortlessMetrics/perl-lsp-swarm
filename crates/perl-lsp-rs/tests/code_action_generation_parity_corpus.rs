//! Code-action provider-generation parity corpus (#9188).
//!
//! Every fixture here drives the real server through `LspHarness` and asserts a
//! hand-written expectation against the **published** `textDocument/codeAction`
//! payload. No fixture calls a provider directly and no expectation is captured
//! from a provider's own output, so this corpus stays valid proof after #9189
//! changes routing and #9190 deletes a generation.
//!
//! Fixture identities are the `cac-parity-*` constants below. They are claimed
//! by rows in `policy/code-action-generation-ledger.toml`, and
//! `cargo xtask check-code-action-generation-ledger` fails when the two sets
//! disagree in either direction.
//!
//! The corpus covers the outcome classes #9188 requires: successful, disabled /
//! refused, stale, ambiguous, malformed, and legitimate-empty.

mod support;

use serde_json::{Value, json};
use support::lsp_harness::LspHarness;

type TestResult = Result<(), Box<dyn std::error::Error>>;

// --- fixture identities -----------------------------------------------------

const SUCCESS_DIAGNOSTIC_ROUTED: &str = "cac-parity-diagnostic-routed-quickfix-edit";
const SUCCESS_PRAGMA: &str = "cac-parity-pragma-duplicate-authority-is-user-visible";
const SUCCESS_CRITIC_SAFE_ONLY: &str = "cac-parity-critic-quickfix-safe-only";
const SUCCESS_FIX_ALL: &str = "cac-parity-source-fixall-aggregates-after-dedupe";
const SUCCESS_ENHANCED_COMBINED_PRAGMA: &str = "cac-parity-enhanced-combined-pragma-fix";
const SUCCESS_UTF8_PRAGMA: &str = "cac-parity-utf8-pragma-only-for-non-ascii-source";
const SUCCESS_REWRITE_TRANSFORM: &str = "cac-parity-enhanced-rewrite-transform-publishes-an-edit";
const IDENTITY_V2_DIAGNOSTIC: &str = "cac-parity-v2-attaches-originating-diagnostic";
const IDENTITY_EXPLAIN: &str = "cac-parity-explain-diagnostic-command-only";
const IDENTITY_TEST_GENERATION: &str = "cac-parity-test-generation-command-only";
const DISABLED_EXTRACT: &str = "cac-parity-disabled-extract-requires-selection";
const REFUSED_NO_DISABLED_SUPPORT: &str = "cac-parity-refused-without-disabled-support";
const STALE_SUPERSEDED_VERSION: &str = "cac-parity-stale-superseded-document-version";
const AMBIGUOUS_EXTRACT_SELECTION: &str = "cac-parity-extract-variable-requires-selection";
const AMBIGUOUS_DUPLICATE_COLLAPSED: &str = "cac-parity-duplicate-authority-collapsed";
const RECOVERY_KEEPS_AST_PATH: &str = "cac-parity-parse-error-recovery-keeps-ast-path";
const EMPTY_OUT_OF_RANGE_SOURCE: &str = "cac-parity-legitimate-empty-out-of-range-source-action";
const EMPTY_KIND_FILTER: &str = "cac-parity-kind-filter-excludes-other-families";
const EMPTY_UNKNOWN_DOCUMENT: &str = "cac-parity-unknown-document-is-empty-not-error";

// --- shared corpus sources --------------------------------------------------

/// Declares nothing under `use strict`, so the diagnostic-routed generations
/// have a real finding to answer.
const UNDECLARED_VARIABLE: &str = "use strict;\nuse warnings;\n\n$undeclared = 1;\n";

/// Carries neither pragma, so the pragma family has an answer.
const NO_PRAGMAS: &str = "my $value = 1;\nprint $value;\n";

/// Carries real parse errors. The v3 parser recovers rather than failing, so
/// this stays on the AST path — see `parse_errors_stay_on_the_ast_path`.
const PARSE_ERRORS: &str = "sub broken { my $x = ; if ( { $x\n";

/// Produces a PL109 unquoted-bareword diagnostic. The explain generation only
/// answers PL701 and PL109, so a PL103 source leaves it silent.
const UNQUOTED_BAREWORD: &str = "use strict;\nuse warnings;\nmy $x = SOMEBAREWORD;\n";

/// Non-ASCII content with no `use utf8`, so the UTF-8 pragma family has an
/// answer. `NO_PRAGMAS` is its negative control: pure ASCII, no UTF-8 action.
const NON_ASCII: &str = "my $s = \"caf\u{e9} na\u{ef}ve\";\nprint $s;\n";

/// A hardcoded (non-portable) shebang, so the shebang source action has an
/// answer when the requested range covers the first line.
const HARDCODED_SHEBANG: &str =
    "#!/usr/bin/perl\nuse strict;\nuse warnings;\nmy $v = 1;\nprint $v;\n";

/// A subroutine plus an extractable binary expression.
const EXTRACTABLE: &str =
    "use strict;\nuse warnings;\n\nsub compute {\n    my $total = 2 + 3;\n    return $total;\n}\n";

// --- helpers ----------------------------------------------------------------

fn harness_with(capabilities: Option<Value>) -> Result<LspHarness, String> {
    let mut harness = LspHarness::new_without_initialize();
    harness.initialize(capabilities)?;
    harness.barrier();
    Ok(harness)
}

fn disabled_support_capabilities() -> Value {
    json!({ "textDocument": { "codeAction": { "disabledSupport": true } } })
}

fn code_actions(
    harness: &mut LspHarness,
    uri: &str,
    range: ((u32, u32), (u32, u32)),
    only: Option<&[&str]>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let ((start_line, start_char), (end_line, end_char)) = range;
    let mut context = json!({ "diagnostics": [], "triggerKind": 1 });
    if let Some(kinds) = only {
        context["only"] = json!(kinds);
    }

    let response = harness.request(
        "textDocument/codeAction",
        json!({
            "textDocument": { "uri": uri },
            "range": {
                "start": { "line": start_line, "character": start_char },
                "end": { "line": end_line, "character": end_char },
            },
            "context": context,
        }),
    )?;

    let actions = response
        .as_array()
        .ok_or_else(|| format!("expected a code action array, got: {response}"))?;
    Ok(actions.clone())
}

fn title(action: &Value) -> &str {
    action.get("title").and_then(Value::as_str).unwrap_or_default()
}

fn kind(action: &Value) -> &str {
    action.get("kind").and_then(Value::as_str).unwrap_or_default()
}

fn edits_for(action: &Value, uri: &str) -> Vec<Value> {
    action
        .get("edit")
        .and_then(|edit| edit.get("changes"))
        .and_then(|changes| changes.get(uri))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn titles(actions: &[Value]) -> Vec<&str> {
    actions.iter().map(title).collect()
}

// --- successful -------------------------------------------------------------

/// A diagnostic-routed quick fix reaches the client with a concrete edit.
#[test]
fn diagnostic_routed_quickfix_carries_an_edit() -> TestResult {
    let uri = "file:///cac_parity_diagnostic_routed.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, UNDECLARED_VARIABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((3, 0), (3, 15)), Some(&["quickfix"]))?;

    let with_edits =
        actions.iter().filter(|action| !edits_for(action, uri).is_empty()).collect::<Vec<_>>();
    assert!(
        !with_edits.is_empty(),
        "{SUCCESS_DIAGNOSTIC_ROUTED}: expected at least one quickfix carrying an edit, got {:?}",
        titles(&actions)
    );

    for action in &actions {
        assert_eq!(
            kind(action),
            "quickfix",
            "{SUCCESS_DIAGNOSTIC_ROUTED}: context.only was quickfix but a {:?} action was published",
            kind(action)
        );
        assert!(
            !title(action).is_empty(),
            "{SUCCESS_DIAGNOSTIC_ROUTED}: every published action must carry a title"
        );
    }

    Ok(())
}

/// Pragma duplicate authority is **user-visible**, not merely internal.
///
/// `dedupe_code_actions` keys on `(kind, title, edit, command)`, so it collapses
/// only byte-identical actions. Three generations insert `use strict` under
/// three different titles — `missing_pragmas`, the native critic, and the
/// enhanced generation's combined fix — and a client sees all three at once.
///
/// This fixture freezes that as the current fact rather than asserting the
/// tidier claim that the family "answers once". If #9189 resolves the
/// duplication, this fixture must fail and the ledger must be updated with it;
/// that coupling is the point.
#[test]
fn pragma_duplicate_authority_is_visible_and_uncollapsed() -> TestResult {
    let uri = "file:///cac_parity_pragma.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((0, 0), (1, 12)), Some(&["quickfix"]))?;

    // Semantic duplication: count actions whose edit actually inserts the
    // pragma, not actions carrying one exact title.
    let inserts_strict = actions
        .iter()
        .filter(|action| {
            edits_for(action, uri).iter().any(|edit| {
                edit.get("newText")
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.contains("use strict"))
            })
        })
        .map(title)
        .collect::<Vec<_>>();

    assert!(
        inserts_strict.len() >= 2,
        "{SUCCESS_PRAGMA}: expected several generations to publish a `use strict` insertion, got {inserts_strict:?}"
    );

    // Distinct titles are exactly why dedupe does not collapse them.
    let mut distinct = inserts_strict.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        inserts_strict.len(),
        "{SUCCESS_PRAGMA}: two identical titles survived dedupe_code_actions: {inserts_strict:?}"
    );

    // What dedupe DOES guarantee: no two published actions are byte-identical.
    let mut identities = actions
        .iter()
        .map(|action| (kind(action).to_string(), title(action).to_string(), edits_for(action, uri)))
        .map(|identity| format!("{identity:?}"))
        .collect::<Vec<_>>();
    let published = identities.len();
    identities.sort();
    identities.dedup();
    assert_eq!(
        identities.len(),
        published,
        "{SUCCESS_PRAGMA}: a byte-identical duplicate survived dedupe_code_actions: {:?}",
        titles(&actions)
    );

    Ok(())
}

/// Only critic findings carrying a Safe, non-empty fix become quick fixes, so
/// no published critic action may be an empty edit.
///
/// The non-emptiness precondition is load-bearing: without it this fixture
/// passes when the critic generation publishes nothing at all, which is the
/// regression it exists to catch.
#[test]
fn critic_quickfixes_are_never_published_without_an_edit() -> TestResult {
    let uri = "file:///cac_parity_critic.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((0, 0), (1, 12)), Some(&["quickfix"]))?;

    let critic = actions
        .iter()
        .filter(|action| {
            action.get("diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .get("code")
                        .and_then(Value::as_str)
                        .is_some_and(|code| code.starts_with("native."))
                })
            })
        })
        .collect::<Vec<_>>();

    assert!(
        !critic.is_empty(),
        "{SUCCESS_CRITIC_SAFE_ONLY}: the native critic generation published no quick fix for a source missing both pragmas, so this fixture would prove nothing: {:?}",
        titles(&actions)
    );

    for action in critic {
        assert!(
            !edits_for(action, uri).is_empty(),
            "{SUCCESS_CRITIC_SAFE_ONLY}: critic quick fix {:?} was published without an edit; only Safe non-empty fixes may become quick fixes",
            title(action)
        );
    }

    Ok(())
}

/// The enhanced generation is a pragma publisher too, and the only one that
/// offers a single combined fix. `missing_pragmas` emits one action per
/// pragma; retiring the enhanced route would remove the one-click form from
/// the `quickfix` kind entirely.
#[test]
fn enhanced_generation_publishes_a_combined_pragma_fix() -> TestResult {
    let uri = "file:///cac_parity_enhanced_pragma.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((0, 0), (1, 12)), Some(&["quickfix"]))?;

    let combined = actions
        .iter()
        .find(|action| title(action).starts_with("Add missing pragmas ("))
        .ok_or_else(|| {
            format!(
                "{SUCCESS_ENHANCED_COMBINED_PRAGMA}: no combined pragma quickfix was published: {:?}",
                titles(&actions)
            )
        })?;

    // One action, one edit, inserting both pragmas — that is what makes it
    // distinct from the canonical generation's per-pragma actions.
    let edits = edits_for(combined, uri);
    assert_eq!(
        edits.len(),
        1,
        "{SUCCESS_ENHANCED_COMBINED_PRAGMA}: expected a single combined edit, got {edits:?}"
    );
    let new_text = edits[0].get("newText").and_then(Value::as_str).unwrap_or_default();
    assert!(
        new_text.contains("use strict;") && new_text.contains("use warnings;"),
        "{SUCCESS_ENHANCED_COMBINED_PRAGMA}: combined fix must insert both pragmas, got {new_text:?}"
    );

    // The canonical per-pragma actions must still be present alongside it, or
    // this is no longer the duplicate-authority situation the ledger records.
    assert!(
        actions.iter().any(|action| title(action) == "Add use strict;"),
        "{SUCCESS_ENHANCED_COMBINED_PRAGMA}: the canonical per-pragma generation stopped publishing: {:?}",
        titles(&actions)
    );

    Ok(())
}

/// The UTF-8 pragma family has exactly one producer, and it is conditional on
/// the source actually containing non-ASCII content. The ASCII case is the
/// negative control: without it, an implementation that always offered the
/// action would pass.
#[test]
fn utf8_pragma_action_is_offered_only_for_non_ascii_source() -> TestResult {
    let non_ascii_uri = "file:///cac_parity_utf8.pl";
    let mut harness = harness_with(None)?;
    harness.open(non_ascii_uri, NON_ASCII)?;
    harness.barrier();

    let actions = code_actions(&mut harness, non_ascii_uri, ((0, 0), (1, 8)), Some(&["quickfix"]))?;
    let utf8 =
        actions.iter().find(|action| title(action) == "Add UTF-8 support").ok_or_else(|| {
            format!(
                "{SUCCESS_UTF8_PRAGMA}: no UTF-8 pragma action for non-ASCII source: {:?}",
                titles(&actions)
            )
        })?;

    let edits = edits_for(utf8, non_ascii_uri);
    let new_text = edits
        .first()
        .and_then(|edit| edit.get("newText"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        new_text.contains("use utf8;"),
        "{SUCCESS_UTF8_PRAGMA}: the UTF-8 action must insert `use utf8;`, got {new_text:?}"
    );

    // Negative control: pure ASCII, same pragma-less shape, no UTF-8 action.
    let ascii_uri = "file:///cac_parity_ascii.pl";
    harness.open(ascii_uri, NO_PRAGMAS)?;
    harness.barrier();
    let ascii_actions =
        code_actions(&mut harness, ascii_uri, ((0, 0), (1, 12)), Some(&["quickfix"]))?;
    assert!(
        ascii_actions.iter().all(|action| title(action) != "Add UTF-8 support"),
        "{SUCCESS_UTF8_PRAGMA}: a UTF-8 action was offered for pure-ASCII source: {:?}",
        titles(&ascii_actions)
    );

    Ok(())
}

/// `source.fixAll` aggregates the deduplicated set, so it never republishes a
/// pragma insertion that a collapsed duplicate already contributed.
///
/// Asserting presence first is load-bearing: an `at most one` assertion alone
/// is satisfied by zero, so removing the aggregate entirely would have passed.
#[test]
fn source_fix_all_aggregates_after_dedupe() -> TestResult {
    let uri = "file:///cac_parity_fix_all.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((0, 0), (1, 12)), None)?;

    // Precondition: this source yields several distinct quick fixes with edits,
    // which is what the aggregate is built from.
    let quickfixes_with_edits = actions
        .iter()
        .filter(|action| kind(action) == "quickfix" && !edits_for(action, uri).is_empty())
        .count();
    assert!(
        quickfixes_with_edits >= 2,
        "{SUCCESS_FIX_ALL}: expected at least two edit-carrying quick fixes to aggregate, got {quickfixes_with_edits}: {:?}",
        titles(&actions)
    );

    let fix_all =
        actions.iter().filter(|action| kind(action) == "source.fixAll").collect::<Vec<_>>();
    assert_eq!(
        fix_all.len(),
        1,
        "{SUCCESS_FIX_ALL}: expected exactly one source.fixAll action, got {}: {:?}",
        fix_all.len(),
        titles(&actions)
    );

    let edits = edits_for(fix_all[0], uri);
    assert!(
        !edits.is_empty(),
        "{SUCCESS_FIX_ALL}: the aggregate published no edit: {:?}",
        fix_all[0]
    );

    let mut keys = edits
        .iter()
        .map(|edit| (edit.get("range").map(ToString::to_string), edit.get("newText").cloned()))
        .collect::<Vec<_>>();
    let before = keys.len();
    keys.sort_by_key(|key| format!("{key:?}"));
    keys.dedup();
    assert_eq!(
        keys.len(),
        before,
        "{SUCCESS_FIX_ALL}: source.fixAll merged duplicate edits: {edits:?}"
    );

    Ok(())
}

/// The enhanced generation's rewrite transforms are a family of their own, and
/// the duplicate-authority fixture never exercised them: selecting `2 + 3`
/// yields an extract action, not a rewrite. This drives one directly.
#[test]
fn enhanced_rewrite_transform_publishes_an_edit() -> TestResult {
    let uri = "file:///cac_parity_rewrite.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, "my $s = 1;\nprint $s;\n")?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((1, 0), (1, 8)), Some(&["refactor.rewrite"]))?;

    let transform = actions.first().ok_or_else(|| {
        format!("{SUCCESS_REWRITE_TRANSFORM}: the rewrite family published nothing")
    })?;
    assert_eq!(
        kind(transform),
        "refactor.rewrite",
        "{SUCCESS_REWRITE_TRANSFORM}: wrong kind for {:?}",
        title(transform)
    );
    assert!(
        !edits_for(transform, uri).is_empty(),
        "{SUCCESS_REWRITE_TRANSFORM}: rewrite action {:?} carried no edit",
        title(transform)
    );

    Ok(())
}

// --- action identity without an edit ----------------------------------------

/// The explain generation publishes a command-only action; it must never carry
/// a workspace edit.
///
/// It answers only PL701 and PL109, so this uses an unquoted-bareword source.
/// An earlier version of this fixture used a PL103 source and was therefore
/// vacuous: the filter matched nothing and the loop asserted nothing.
#[test]
fn explain_diagnostic_action_is_command_only() -> TestResult {
    let uri = "file:///cac_parity_explain.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, UNQUOTED_BAREWORD)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((2, 0), (2, 21)), None)?;

    let explain =
        actions.iter().filter(|action| title(action).starts_with("Explain")).collect::<Vec<_>>();
    assert!(
        !explain.is_empty(),
        "{IDENTITY_EXPLAIN}: no explain action was published for a PL109 source, so this fixture would prove nothing: {:?}",
        titles(&actions)
    );

    for action in explain {
        assert!(
            action.get("edit").is_none(),
            "{IDENTITY_EXPLAIN}: explain action {:?} must not carry a workspace edit",
            title(action)
        );
        assert!(
            action.get("command").is_some(),
            "{IDENTITY_EXPLAIN}: explain action {:?} must carry a command",
            title(action)
        );
    }

    Ok(())
}

/// The test generator publishes a command-only action per discovered
/// subroutine.
#[test]
fn test_generation_action_is_command_only() -> TestResult {
    let uri = "file:///cac_parity_test_generation.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, EXTRACTABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((3, 0), (6, 1)), None)?;

    let generated = actions
        .iter()
        .filter(|action| title(action).starts_with("Generate test for"))
        .collect::<Vec<_>>();
    assert!(
        !generated.is_empty(),
        "{IDENTITY_TEST_GENERATION}: expected a test generation action, got {:?}",
        titles(&actions)
    );

    for action in generated {
        assert_eq!(kind(action), "source", "{IDENTITY_TEST_GENERATION}: wrong kind");
        assert!(
            action.get("edit").is_none(),
            "{IDENTITY_TEST_GENERATION}: {:?} must not carry a workspace edit",
            title(action)
        );
        assert!(
            action.get("command").is_some(),
            "{IDENTITY_TEST_GENERATION}: {:?} must carry a command",
            title(action)
        );
    }

    Ok(())
}

/// The V2 generation is the only one that links a quick fix back to the
/// diagnostic that produced it. This fixture is the retirement blocker
/// `canonical_route_omits_diagnostic_association` in executable form: it must
/// keep passing after #9189 picks a canonical route, or the association was
/// silently dropped.
#[test]
fn a_diagnostic_routed_quickfix_is_associated_with_its_diagnostic() -> TestResult {
    let uri = "file:///cac_parity_association.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, UNDECLARED_VARIABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((3, 0), (3, 15)), Some(&["quickfix"]))?;

    // The critic generation also attaches a diagnostic, so a bare "something
    // has a diagnostics array" assertion would pass even if the
    // diagnostic-routed association disappeared. Critic findings publish
    // `native.*` rule ids (or `Perl::Critic::*` under the legacy engine), so
    // requiring a stable `PL*` code keeps this fixture bound to the
    // diagnostic-routed generation it is evidence for.
    let associated = actions
        .iter()
        .filter(|action| {
            action.get("diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .get("code")
                        .and_then(Value::as_str)
                        .is_some_and(|code| code.starts_with("PL"))
                })
            })
        })
        .collect::<Vec<_>>();

    assert!(
        !associated.is_empty(),
        "{IDENTITY_V2_DIAGNOSTIC}: no published quick fix carried its originating PL diagnostic; the diagnostic-to-fix link #4205 consumers rely on is gone. Published: {:?}",
        titles(&actions)
    );

    for action in associated {
        let diagnostics = action
            .get("diagnostics")
            .and_then(Value::as_array)
            .ok_or("diagnostics array disappeared")?;
        for diagnostic in diagnostics {
            assert!(
                diagnostic.get("code").is_some(),
                "{IDENTITY_V2_DIAGNOSTIC}: associated diagnostic has no code: {diagnostic}"
            );
            assert!(
                diagnostic.get("range").is_some(),
                "{IDENTITY_V2_DIAGNOSTIC}: associated diagnostic has no range: {diagnostic}"
            );
        }
    }

    Ok(())
}

// --- disabled / refused -----------------------------------------------------

/// A zero-width selection cannot extract, and a client that declared
/// `disabledSupport` is told so explicitly rather than shown nothing.
#[test]
fn zero_width_selection_publishes_a_disabled_extract_action() -> TestResult {
    let uri = "file:///cac_parity_disabled.pl";
    let mut harness = harness_with(Some(disabled_support_capabilities()))?;
    harness.open(uri, EXTRACTABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((4, 16), (4, 16)), None)?;

    let disabled =
        actions.iter().find(|action| action.get("disabled").is_some()).ok_or_else(|| {
            format!("{DISABLED_EXTRACT}: no disabled action was published: {:?}", titles(&actions))
        })?;

    assert_eq!(kind(disabled), "refactor.extract", "{DISABLED_EXTRACT}: wrong kind");
    let reason = disabled
        .pointer("/disabled/reason")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{DISABLED_EXTRACT}: disabled action carries no reason"))?;
    assert!(
        !reason.trim().is_empty(),
        "{DISABLED_EXTRACT}: a disabled action must name its blocker"
    );
    assert!(
        disabled.get("edit").is_none(),
        "{DISABLED_EXTRACT}: a disabled action must not carry an applicable edit"
    );

    Ok(())
}

/// Negative control for the fixture above. Without the capability the server
/// refuses silently instead of publishing an action the client cannot render.
#[test]
fn zero_width_selection_publishes_nothing_without_disabled_support() -> TestResult {
    let uri = "file:///cac_parity_refused.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, EXTRACTABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((4, 16), (4, 16)), None)?;

    assert!(
        actions.iter().all(|action| action.get("disabled").is_none()),
        "{REFUSED_NO_DISABLED_SUPPORT}: a disabled action was published to a client that did not declare disabledSupport: {:?}",
        titles(&actions)
    );

    // An *enabled* extract action carries no `disabled` field either, so the
    // assertion above alone would pass if the server started publishing one for
    // a zero-width selection. The refusal is that no extract action appears.
    assert!(
        actions.iter().all(|action| kind(action) != "refactor.extract"),
        "{REFUSED_NO_DISABLED_SUPPORT}: an extract action was published for a zero-width selection to a client without disabledSupport: {:?}",
        titles(&actions)
    );

    Ok(())
}

// --- stale ------------------------------------------------------------------

/// Actions are computed against the current document generation. After a full
/// replacement the superseded text must not still be answerable.
#[test]
fn actions_follow_the_current_document_version() -> TestResult {
    let uri = "file:///cac_parity_stale.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let before = code_actions(&mut harness, uri, ((0, 0), (0, 0)), Some(&["quickfix"]))?;
    assert!(
        before.iter().any(|action| title(action).contains("use strict")),
        "{STALE_SUPERSEDED_VERSION}: expected a use strict fix for the pragma-less version, got {:?}",
        titles(&before)
    );

    harness.change_full(uri, 2, "use strict;\nuse warnings;\nmy $value = 1;\n")?;
    harness.barrier();

    let after = code_actions(&mut harness, uri, ((0, 0), (0, 0)), Some(&["quickfix"]))?;
    assert!(
        !after.iter().any(|action| title(action).contains("use strict")),
        "{STALE_SUPERSEDED_VERSION}: the superseded version still answered with {:?}",
        titles(&after)
    );

    Ok(())
}

// --- ambiguous --------------------------------------------------------------

/// LSP 3.16 §3.16.2: an enabled `refactor.extract` requires a real selection.
/// A cursor position is ambiguous about what to extract, so no enabled extract
/// action may be published.
#[test]
fn cursor_position_publishes_no_enabled_extract_action() -> TestResult {
    let uri = "file:///cac_parity_ambiguous.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, EXTRACTABLE)?;
    harness.barrier();

    // Positive control first: the same document and the same expression DO
    // yield an enabled extract once the selection is real. Without this, the
    // absence assertion below would pass against a server that never offers
    // extraction at all.
    let selected =
        code_actions(&mut harness, uri, ((4, 16), (4, 21)), Some(&["refactor.extract"]))?;
    assert!(
        selected.iter().any(|action| action.get("disabled").is_none()),
        "{AMBIGUOUS_EXTRACT_SELECTION}: a real selection published no enabled extract action, so the zero-width case proves nothing: {:?}",
        titles(&selected)
    );

    let actions = code_actions(&mut harness, uri, ((4, 16), (4, 16)), Some(&["refactor.extract"]))?;

    for action in &actions {
        assert!(
            action.get("disabled").is_some(),
            "{AMBIGUOUS_EXTRACT_SELECTION}: enabled extract action {:?} was published for a zero-width selection",
            title(action)
        );
    }

    Ok(())
}

/// Two generations answer the extract families for one request — the enhanced
/// generation runs once directly and once nested inside the original
/// generation — and the client must still see each action exactly once.
#[test]
fn overlapping_generations_publish_each_action_once() -> TestResult {
    let uri = "file:///cac_parity_duplicates.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, EXTRACTABLE)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((4, 16), (4, 21)), None)?;

    // Precondition: the extract families both generations answer must actually
    // be present, or "no duplicates" is satisfied by an empty response and this
    // fixture stops being evidence for the redundant_behavior rows it backs.
    assert!(
        actions.iter().any(|action| kind(action).starts_with("refactor.")),
        "{AMBIGUOUS_DUPLICATE_COLLAPSED}: no refactor action was published, so deduplication proves nothing here: {:?}",
        titles(&actions)
    );

    let mut identities = actions
        .iter()
        .map(|action| {
            (
                kind(action).to_string(),
                title(action).to_string(),
                action.get("edit").map(ToString::to_string),
                action.get("command").map(ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    let published = identities.len();
    identities.sort();
    identities.dedup();

    assert_eq!(
        identities.len(),
        published,
        "{AMBIGUOUS_DUPLICATE_COLLAPSED}: overlapping generations published a duplicate action: {:?}",
        titles(&actions)
    );

    Ok(())
}

// --- malformed --------------------------------------------------------------

/// Malformed source does **not** reach the degraded text generation.
///
/// The v3 recursive-descent parser recovers from the inputs a user actually
/// produces, so `ParsedSnapshot::ast()` stays `Some` and the AST-path
/// generations keep answering. The repository already records this for the
/// snapshot layer: no malformed input reliably forces `ast: None` (#3760).
///
/// This fixture pins that at the protocol surface, because it is what decides
/// how much the `text_fallback` generation is actually worth to #9190. If
/// recovery ever regressed, the AST-only families asserted here would vanish
/// and this fixture would fail — which is the signal, not a nuisance.
#[test]
fn parse_errors_stay_on_the_ast_path() -> TestResult {
    let uri = "file:///cac_parity_parse_errors.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, PARSE_ERRORS)?;
    harness.barrier();

    let actions = code_actions(&mut harness, uri, ((0, 0), (1, 0)), None)?;

    // `source.modernize` and test generation exist only on the AST path; the
    // degraded text generation cannot produce either.
    assert!(
        actions.iter().any(|action| kind(action) == "source.modernize"),
        "{RECOVERY_KEEPS_AST_PATH}: no source.modernize action, so the request did not stay on the AST path: {:?}",
        titles(&actions)
    );
    assert!(
        actions.iter().any(|action| title(action).starts_with("Generate test for")),
        "{RECOVERY_KEEPS_AST_PATH}: no test generation action, so the request did not stay on the AST path: {:?}",
        titles(&actions)
    );

    // Recovery must still route the parse error itself to an *associated*
    // quick fix. This extends `cac-parity-v2-attaches-originating-diagnostic`
    // to the PL00x parse-error family: verified by removing the V2 association,
    // which fails this assertion too.
    assert!(
        actions.iter().any(|action| {
            action.get("diagnostics").and_then(Value::as_array).is_some_and(|diagnostics| {
                diagnostics.iter().any(|diagnostic| {
                    diagnostic
                        .get("code")
                        .and_then(Value::as_str)
                        .is_some_and(|code| code.starts_with("PL00"))
                })
            })
        }),
        "{RECOVERY_KEEPS_AST_PATH}: no associated parse-error quick fix was published for source with real parse errors: {:?}",
        titles(&actions)
    );

    Ok(())
}

// --- legitimate empty -------------------------------------------------------

/// The shebang source action is offered only when the requested range covers
/// the first line. A later range is a legitimate empty answer, not a failure.
#[test]
fn source_action_outside_its_range_is_legitimately_absent() -> TestResult {
    let uri = "file:///cac_parity_out_of_range.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, HARDCODED_SHEBANG)?;
    harness.barrier();

    // Positive control: covering the first line DOES offer the fix. Without it
    // the absence assertion below would pass against a server that never offers
    // the shebang action at all.
    let in_range = code_actions(&mut harness, uri, ((0, 0), (0, 15)), Some(&["quickfix"]))?;
    assert!(
        in_range.iter().any(|action| title(action).to_lowercase().contains("shebang")),
        "{EMPTY_OUT_OF_RANGE_SOURCE}: no shebang action for a range covering the first line, so its absence elsewhere proves nothing: {:?}",
        titles(&in_range)
    );

    let actions = code_actions(&mut harness, uri, ((4, 0), (4, 9)), None)?;

    assert!(
        actions.iter().all(|action| !title(action).to_lowercase().contains("shebang")),
        "{EMPTY_OUT_OF_RANGE_SOURCE}: a shebang action was published for a range that excludes the first line: {:?}",
        titles(&actions)
    );

    Ok(())
}

/// `context.only` selects families. A filter no generation answers is an empty
/// array, not an error and not an unfiltered set.
#[test]
fn kind_filter_excludes_every_other_family() -> TestResult {
    let uri = "file:///cac_parity_kind_filter.pl";
    let mut harness = harness_with(None)?;
    harness.open(uri, NO_PRAGMAS)?;
    harness.barrier();

    let modernize =
        code_actions(&mut harness, uri, ((0, 0), (1, 12)), Some(&["source.modernize"]))?;
    // This fixture is the source.modernize route's evidence, so the family must
    // be present, not merely unpolluted.
    assert!(
        !modernize.is_empty(),
        "{EMPTY_KIND_FILTER}: the source.modernize family published nothing for a pragma-less source"
    );
    for action in &modernize {
        assert_eq!(
            kind(action),
            "source.modernize",
            "{EMPTY_KIND_FILTER}: {:?} leaked through a source.modernize filter",
            title(action)
        );
    }

    let inline = code_actions(&mut harness, uri, ((0, 0), (1, 12)), Some(&["refactor.inline"]))?;
    for action in &inline {
        assert_eq!(
            kind(action),
            "refactor.inline",
            "{EMPTY_KIND_FILTER}: {:?} leaked through a refactor.inline filter",
            title(action)
        );
    }

    Ok(())
}

/// A request for a document the server never opened is an empty answer, not a
/// protocol error and not a guess from an unrelated document.
#[test]
fn unknown_document_answers_empty_rather_than_failing() -> TestResult {
    let mut harness = harness_with(None)?;
    harness.open("file:///cac_parity_known.pl", NO_PRAGMAS)?;
    harness.barrier();

    let actions =
        code_actions(&mut harness, "file:///cac_parity_never_opened.pl", ((0, 0), (0, 0)), None)?;

    assert!(
        actions.is_empty(),
        "{EMPTY_UNKNOWN_DOCUMENT}: an unopened document answered with {:?}",
        titles(&actions)
    );

    Ok(())
}
