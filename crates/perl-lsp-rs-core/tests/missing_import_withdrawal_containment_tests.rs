//! Containment proof for the withdrawn hard-coded missing-import edits
//! (issue #10690).
//!
//! Until #790 lands exact candidate planning, no production route may turn a
//! hard-coded function→module affinity — or a PL109 presentation — into an
//! import-edit authority. The enhanced global route and the PL109 diagnostic
//! import fix are both withdrawn; neither may return without failing these
//! guards.
//!
//! These tests were written against unmodified `main` and proven failing before
//! the production routes were deleted (shift-left). Exact-process coverage
//! lives in `crates/perllsp/tests/lsp_missing_import_withdrawal_process.rs`.

use perl_lsp_rs_core::providers::code_actions::{
    CodeAction, CodeActionsProvider, EnhancedCodeActionsProvider,
};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};
use perl_parser::Parser;
use perl_tdd_support::{must, must_some};

fn pl109_diag(source: &str, needle: &str) -> Diagnostic {
    let start = must_some(source.find(needle));
    Diagnostic {
        range: (start, start + needle.len()),
        severity: DiagnosticSeverity::Error,
        code: Some("PL109".to_string()),
        message: format!(
            "Bareword '{needle}' is not allowed under 'use strict' -- quote it as '{needle}' or use it as a subroutine call"
        ),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    }
}

fn provider_actions(source: &str, diags: &[Diagnostic]) -> Vec<CodeAction> {
    let mut parser = Parser::new(source);
    let ast = must(parser.parse());
    let provider = CodeActionsProvider::new(source.to_string());
    provider.get_code_actions(&ast, (0, source.len()), diags)
}

fn enhanced_actions(source: &str) -> Result<Vec<CodeAction>, String> {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("fixture source must parse: {error:?}"))?;
    Ok(EnhancedCodeActionsProvider::new(source.to_string())
        .get_enhanced_refactoring_actions(&ast, (0, source.len())))
}

const WITHDRAWN_TABLE_MODULES: &[&str] =
    &["Data::Dumper", "Encode", "File::Basename", "File::Path", "File::Slurp", "JSON"];

fn reject_import_authority(actions: &[CodeAction]) -> Result<(), String> {
    for action in actions {
        if action.title.starts_with("Import '") || action.title.contains("Add missing imports") {
            return Err(format!("withdrawn import presentation returned: {action:?}"));
        }
        for module in WITHDRAWN_TABLE_MODULES {
            for edit in &action.edit.changes {
                if edit.new_text.contains(&format!("use {module};")) {
                    return Err(format!(
                        "an edit inserts the table directive 'use {module};': {action:?}"
                    ));
                }
            }
        }
    }
    Ok(())
}

// ===========================================================================
// Route B: PL109 presentation cannot produce import edits
// ===========================================================================

#[test]
fn pl109_route_cannot_feed_an_import_edit_while_quoting_survives() -> Result<(), String> {
    let source = "use strict;\nmy $name = basename($path);\nmy $data = decode_json($raw);\n";
    let diags =
        vec![pl109_diag(source, "basename($path)"), pl109_diag(source, "decode_json($raw)")];
    let actions = provider_actions(source, &diags);

    reject_import_authority(&actions)?;

    assert!(
        actions.iter().any(|action| action.title.contains("with single quotes")),
        "surviving PL109 quoting fixes must remain available: {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn mapped_spellings_are_inert_across_the_whole_withdrawn_table() -> Result<(), String> {
    for spelling in
        ["dumper", "encode", "decode", "basename", "dirname", "mkpath", "rmtree", "slurp"]
    {
        let source = format!("use strict;\nmy $r = {spelling}($in);\n");
        let actions = provider_actions(&source, &[pl109_diag(&source, spelling)]);
        reject_import_authority(&actions).map_err(|error| format!("{spelling}: {error}"))?;
    }
    Ok(())
}

#[test]
fn local_or_imported_collisions_never_reauthorize_the_mapping() -> Result<(), String> {
    // The call precedes the definition so the diagnosed range covers the bare
    // spelling; affinity must not fire even when a same-named local sub exists.
    let local = "use strict;\nmy $d = dirname($p);\nsub dirname { }\n";
    reject_import_authority(&provider_actions(local, &[pl109_diag(local, "dirname")]))?;

    // The module is already imported; no duplicate directive may appear.
    let imported = "use strict;\nuse JSON;\nmy $d = decode_json($t);\n";
    reject_import_authority(&provider_actions(imported, &[pl109_diag(imported, "decode_json")]))?;
    Ok(())
}

#[test]
fn refusal_is_omission_not_an_enabled_noop_stand_in() -> Result<(), String> {
    let source = "use strict;\nmy $out = dumper($hash);\n";
    let actions = provider_actions(source, &[pl109_diag(source, "dumper")]);

    for action in &actions {
        for edit in &action.edit.changes {
            if edit.new_text.is_empty() && edit.location.start == edit.location.end {
                return Err(format!("an enabled empty edit stands in for refusal: {action:?}"));
            }
            let covered = &source[edit.location.start..edit.location.end];
            if edit.new_text == covered {
                return Err(format!("a no-op rewrite stands in for refusal: {action:?}"));
            }
        }
    }
    Ok(())
}

// ===========================================================================
// Route A: the enhanced provider cannot offer missing-import actions
// ===========================================================================

#[test]
fn enhanced_provider_never_offers_missing_import_actions() -> Result<(), String> {
    // Undefined function that the withdrawn table used to map to JSON.
    let source =
        "use strict;\nuse warnings;\nmy $data = decode_json($text);\nprint \"$data\\n\";\n";
    let actions = enhanced_actions(source)?;

    reject_import_authority(&actions)?;
    assert!(!actions.is_empty(), "unrelated enhanced refactoring families must remain available");
    Ok(())
}

// ===========================================================================
// Restoration gate: any re-wiring of the affinity routes fails the scan
// ===========================================================================

/// Byte patterns whose presence under any `crates/*/src` path means a
/// withdrawn route regained production authority. Restoration belongs to
/// #790/#8948, never to a revert.
const WITHDRAWN_ROUTE_PATTERNS: &[(&str, &str)] = &[
    (
        "guess_module_for_function",
        "re-wires the withdrawn hard-coded function-to-module affinity table",
    ),
    (
        "fix_import_for_bareword_function",
        "re-wires the withdrawn PL109 name-affinity import quick fix",
    ),
    ("add_missing_imports", "re-wires the withdrawn enhanced global missing-import route"),
    ("find_undefined_functions", "re-wires the withdrawn affinity-driven undefined-function scan"),
    (
        "create_add_missing_imports_action",
        "re-wires the withdrawn compatibility missing-import placeholder",
    ),
];

#[test]
fn no_production_route_references_the_withdrawn_affinity_routes() -> Result<(), String> {
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
            // dispatch table. Its surviving PL109 arm must stay keyed to the
            // quoting/filehandle family only; a second import-bearing call in
            // that arm is restoration even under a renamed helper.
            if relative.ends_with("providers/code_actions/diagnostic_routes.rs")
                && content.contains("Import '")
            {
                offenders
                    .push(format!("{relative}: routes a name-affinity import presentation again"));
            }
        }
    }

    assert!(scanned > 100, "source scan must traverse the workspace crates");
    assert!(
        offenders.is_empty(),
        "withdrawn missing-import routes reappeared \
         (restoration belongs to #790/#8948, not a revert):\n{}",
        offenders.join("\n")
    );
    Ok(())
}
