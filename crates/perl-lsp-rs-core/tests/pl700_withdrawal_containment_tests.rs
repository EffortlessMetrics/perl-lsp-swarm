//! Containment proof for the withdrawn PL700 prose-driven whole-line removal
//! edit (issue #11079).
//!
//! Until the exact replacement trains land (#1719 explicit-symbol removal,
//! #8322 complete module-load assessment), a PL700 diagnostic's code, message
//! prose, or line geometry grants no authority to mutate import statements.
//!
//! These tests were written against unmodified `main` and proven failing
//! before the production route was deleted (shift-left). Any mutation that
//! re-couples PL700 to import-edit authority must fail here and fail
//! `no_production_route_references_the_withdrawn_pl700_edit`.
//!
//! Exact-process coverage lives in
//! `crates/perllsp/tests/lsp_pl700_withdrawal_process.rs`; the defect class is
//! production routing, so provider-unit coverage alone is insufficient there.

use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::{must, must_some};

fn make_diag(start: usize, end: usize, code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        range: (start, end),
        severity: DiagnosticSeverity::Hint,
        code: Some(code.to_string()),
        message: message.to_string(),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    }
}

fn pl700_diag(source: &str, needle: &str, module: &'static str) -> Diagnostic {
    let start = must_some(source.find(needle));
    let end = start + needle.len();
    make_diag(start, end, "PL700", &format!("Module '{module}' appears to be unused"))
}

fn actions_for(source: &str, diags: &[Diagnostic]) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());
    provider.get_code_actions(&ast, (0, source.len()), diags)
}

/// Apply every edit from every action, highest offset first.
fn apply_all(source: &str, actions: &[CodeAction]) -> String {
    let mut edits =
        actions.iter().flat_map(|action| action.edit.changes.clone()).collect::<Vec<_>>();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.location.start));
    let mut out = source.to_string();
    for edit in edits {
        out.replace_range(edit.location.start..edit.location.end, &edit.new_text);
    }
    out
}

fn reject_withdrawn_import_edits(
    source: &str,
    actions: &[CodeAction],
    protected: &[&str],
    forbidden_titles: &[&str],
) -> Result<(), String> {
    for action in actions {
        if action.kind == CodeActionKind::QuickFix
            && action.diagnostics.iter().any(|code| code == "PL700")
        {
            return Err(format!("a quick fix still claims the withdrawn PL700 family: {action:?}"));
        }
        for marker in forbidden_titles {
            if action.title.contains(marker) {
                return Err(format!(
                    "an action reused the withdrawn import-removal presentation \
                     ('{marker}'): {action:?}"
                ));
            }
        }
    }
    let rewritten = apply_all(source, actions);
    for fragment in protected {
        if !rewritten.contains(fragment) {
            return Err(format!(
                "applying the returned edits destroyed protected source bytes \
                 ({fragment:?}); result: {rewritten:?}"
            ));
        }
    }
    Ok(())
}

#[test]
fn pl700_cannot_delete_line_with_explicit_import_list() -> Result<(), String> {
    // foo-only unusedness: the prose names one symbol, but `bar` stays
    // imported by the same load. Whole-line deletion destroys `bar`'s import.
    let source = "use strict;\nuse M qw(foo bar);\nsub call { bar(); }\n";
    let diag = pl700_diag(source, "use M qw(foo bar);", "M");
    let actions = actions_for(source, &[diag]);

    reject_withdrawn_import_edits(source, &actions, &["use M qw(foo bar);"], &["Remove unused"])
}

#[test]
fn pl700_cannot_destroy_registration_comment() -> Result<(), String> {
    let source = "use M; # registration\nmy $x = 1;\n";
    let diag = pl700_diag(source, "use M;", "M");
    let actions = actions_for(source, &[diag]);

    reject_withdrawn_import_edits(source, &actions, &["# registration"], &["Remove unused"])
}

#[test]
fn pl700_prose_cannot_retarget_or_delete_the_diagnosed_source() -> Result<(), String> {
    // The message names B::Second while the diagnosed line loads A::First.
    // Prose-derived authority would present -- and delete -- across that lie.
    let source = "use A::First;\nmy $x = 1;\n";
    let start = must_some(source.find("use A::First;"));
    let diag = make_diag(
        start,
        start + "use A::First;".len(),
        "PL700",
        "Module 'B::Second' appears to be unused",
    );
    let actions = actions_for(source, &[diag]);

    reject_withdrawn_import_edits(
        source,
        &actions,
        &["use A::First;"],
        &["B::Second", "Remove unused"],
    )
}

#[test]
fn pl700_range_inside_multiline_directive_cannot_expand_to_deletion() -> Result<(), String> {
    let source = "use M qw(\n    foo,\n    bar,\n);\nmy $x = 1;\n";

    // Sub-line range strictly inside the directive: expanding to line
    // geometry deletes a live import-list element.
    let bar_start = must_some(source.find("bar,"));
    let sub_line =
        vec![make_diag(bar_start, bar_start + 4, "PL700", "Module 'M' appears to be unused")];
    let actions = actions_for(source, &sub_line);
    reject_withdrawn_import_edits(source, &actions, &["bar,", "foo,", ");"], &["Remove unused"])?;

    // Range spanning the directive opening into its interior: line expansion
    // would swallow the header and list elements alike.
    let span_end = must_some(source.find("bar,"));
    let spanning = vec![make_diag(0, span_end + 4, "PL700", "Module 'M' appears to be unused")];
    let actions = actions_for(source, &spanning);
    reject_withdrawn_import_edits(source, &actions, &["foo,", "bar,", ");"], &["Remove unused"])
}

#[test]
fn pl700_withdrawal_is_omission_not_enabled_noop_or_disabled_stub() -> Result<(), String> {
    // Refusal must not stand in as an enabled empty edit, a no-op rewrite of
    // the same bytes, or a disabled stub carrying executable data.
    let source = "use M;\nmy $x = 1;\n";
    let diag = pl700_diag(source, "use M;", "M");
    let actions = actions_for(source, &[diag]);

    for action in &actions {
        let touches_import = action.edit.changes.iter().any(|edit| {
            let covered = &source[edit.location.start..edit.location.end];
            covered.contains("use M;")
        });
        if touches_import && action.edit.changes.iter().all(|edit| edit.new_text.is_empty()) {
            return Err(format!("an enabled empty edit stands in for refusal: {action:?}"));
        }
        let noop_rewrite = touches_import
            && action.edit.changes.iter().filter(|edit| edit.new_text.is_empty()).count()
                == action.edit.changes.len();
        if noop_rewrite {
            return Err(format!("a no-op edit stands in for refusal: {action:?}"));
        }
    }
    Ok(())
}

#[test]
fn unrelated_proven_quick_fixes_survive_alongside_pl700_refusal() -> Result<(), String> {
    let source = "use strict;\nuse warnings;\nuse POSIX;\nmy $unused = 1;\n";
    let unused_start = must_some(source.find("$unused"));
    let pl102 = make_diag(
        unused_start,
        unused_start + "$unused".len(),
        "native.variables.unused_lexical",
        "Lexical variable '$unused' is declared but never used",
    );
    let pl700 = pl700_diag(source, "use POSIX;", "POSIX");
    let actions = actions_for(source, &[pl102, pl700]);

    reject_withdrawn_import_edits(
        source,
        &actions,
        &["use POSIX;"],
        &["Remove unused 'use", "Remove unused import"],
    )?;

    assert!(
        actions.iter().any(|action| action.title.contains("Remove unused variable")),
        "unrelated unused-variable fixes must remain available: {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );
    Ok(())
}

/// Byte patterns whose presence means the withdrawn edit regained production
/// authority. Restoration belongs to #1719/#8322, never to a revert.
const WITHDRAWN_ROUTE_PATTERNS: &[(&str, &str)] =
    &[("fix_unused_import", "references the withdrawn fix_unused_import symbol")];

#[test]
fn no_production_route_references_the_withdrawn_pl700_edit() -> Result<(), String> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(2)
        .ok_or_else(|| "integration tests always run inside the workspace tree".to_string())?;
    let crates_dir = workspace_root.join("crates");

    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    let mut stack = vec![crates_dir.clone()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("bad entry in {}: {error}", dir.display()))?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?
                .is_dir()
            {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let relative = path
                .strip_prefix(&crates_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !relative.contains("/src/") {
                continue;
            }
            let content = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            scanned += 1;
            for (needle, explanation) in WITHDRAWN_ROUTE_PATTERNS {
                if content.contains(needle) {
                    offenders.push(format!("{relative}: {explanation}"));
                }
            }
            // The routing file is the mutation surface for the diagnostic
            // dispatch table: any renewed mention of the diagnostic family
            // there is the first step of restoration, even under a renamed
            // fix function.
            if relative.ends_with("providers/code_actions/diagnostic_routes.rs")
                && content.contains("UnusedImport")
            {
                offenders
                    .push(format!("{relative}: routes the withdrawn UnusedImport family again"));
            }
        }
    }

    assert!(scanned > 100, "source scan must traverse the workspace crates");
    assert!(
        offenders.is_empty(),
        "withdrawn PL700 removal routes reappeared \
         (restoration belongs to #1719/#8322, not a revert):\n{}",
        offenders.join("\n")
    );
    Ok(())
}
