//! Route/architecture containment guards for withdrawn hard-coded
//! missing-import edits (issue #10690).
//!
//! Two production routes previously turned a hard-coded function→module
//! spelling table into enabled `use <module>;` edits:
//!
//! 1. the enhanced global route (`add_missing_imports` →
//!    `find_undefined_functions` → `guess_module_for_function` →
//!    package-blind preamble insertion), and
//! 2. the PL109 UnquotedBareword diagnostic route
//!    (`fix_import_for_bareword_function` → the same table).
//!
//! Until #790/#8948 land exact candidate planning, hard-coded name affinity is
//! not candidate identity and not edit authorization. These tests fail if any
//! production route offers an affinity-derived import edit again, or if any
//! production source re-wires the withdrawn authority.

use perl_lsp_rs_core::providers::code_actions::EnhancedCodeActionsProvider;
use perl_lsp_rs_core::providers::code_actions::{CodeAction, CodeActionKind, CodeActionsProvider};
use perl_lsp_rs_core::providers::diagnostics::{Diagnostic, DiagnosticSeverity};

/// Package-first source calling a table-mapped function. The withdrawn route
/// inserted `use Data::Dumper;` *before* `package App;` — importing into
/// `main` while the call lives in `App`.
const PACKAGE_FIRST_DUMPER: &str =
    "package App;\nuse strict;\nuse warnings;\nmy $value = dumper($value);\n1;\n";

fn parse(source: &str) -> Result<perl_parser_core::Node, String> {
    let mut parser = perl_parser_core::Parser::new(source);
    parser.parse().map_err(|error| format!("fixture source must parse: {error:?}"))
}

fn enhanced_actions(source: &str) -> Result<Vec<CodeAction>, String> {
    let ast = parse(source)?;
    Ok(EnhancedCodeActionsProvider::new(source.to_string())
        .get_enhanced_refactoring_actions(&ast, (0, source.len())))
}

fn v2_actions_with_pl109(source: &str, symbol: &str) -> Result<Vec<CodeAction>, String> {
    let start = source.find(symbol).ok_or_else(|| format!("fixture must contain {symbol:?}"))?;
    let end = start + symbol.len();
    let diagnostic = Diagnostic {
        range: (start, end),
        severity: DiagnosticSeverity::Error,
        code: Some("PL109".to_string()),
        message: format!(
            "Bareword '{symbol}' is not allowed under 'use strict' -- quote it as '{symbol}' or use it as a subroutine call"
        ),
        related_information: Vec::new(),
        tags: Vec::new(),
        suggestion: None,
    };
    let ast = parse(source)?;
    Ok(CodeActionsProvider::new(source.to_string()).get_code_actions(
        &ast,
        (0, source.len()),
        &[diagnostic],
    ))
}

fn workspace_root() -> Result<std::path::PathBuf, String> {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(2)
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| "integration tests always run inside the workspace tree".to_string())
}

fn assert_no_affinity_import_edit(actions: &[CodeAction]) {
    for action in actions {
        assert_ne!(
            action.title, "Add missing imports",
            "the withdrawn enhanced missing-import action (#10690) must not be offered; got {actions:?}"
        );
        assert!(
            !action.title.starts_with("Import '"),
            "the withdrawn PL109 import action (#10690) must not be offered; got {actions:?}"
        );
        for edit in &action.edit.changes {
            let inserts_use_line =
                edit.new_text.lines().any(|line| line.trim_start().starts_with("use "));
            // The missing-pragma quick fix legitimately inserts exactly these
            // pragma texts; any other `use` insertion is affinity-derived.
            let legitimate_pragma_insertion = matches!(
                edit.new_text.as_str(),
                "use strict;\n"
                    | "use warnings;\n"
                    | "use strict;\nuse warnings;\n"
                    | "use strict;\nuse warnings;\n\n"
            );
            assert!(
                !inserts_use_line || legitimate_pragma_insertion,
                "action {:?} carries an import-insertion edit ({:?}); hard-coded affinity must not authorize edits (#10690)",
                action.title,
                edit.new_text
            );
        }
    }
}

#[test]
fn enhanced_route_cannot_offer_table_derived_import_for_dumper() -> Result<(), String> {
    let actions = enhanced_actions(PACKAGE_FIRST_DUMPER)?;
    assert_no_affinity_import_edit(&actions);
    Ok(())
}

#[test]
fn enhanced_route_stays_withdrawn_when_call_is_locally_defined() -> Result<(), String> {
    // A locally defined same-name callable is one of the counterexamples that
    // made the table wrong; withdrawal must hold regardless.
    let source = "package App;\nsub dumper { return 1; }\nmy $value = dumper($value);\n1;\n";
    let actions = enhanced_actions(source)?;
    assert_no_affinity_import_edit(&actions);
    Ok(())
}

#[test]
fn enhanced_route_stays_withdrawn_for_explicit_empty_import_form() -> Result<(), String> {
    // `use Encode ();` does not make decode() visible; the table must neither
    // suppress nor edit here.
    let source = "use Encode ();\nuse strict;\nuse warnings;\nmy $text = decode($bytes);\n";
    let actions = enhanced_actions(source)?;
    assert_no_affinity_import_edit(&actions);
    Ok(())
}

#[test]
fn pl109_route_cannot_offer_table_derived_import_but_keeps_quote_fixes() -> Result<(), String> {
    let source = "use strict;\nuse warnings;\nmy $name = basename($path);\n";
    let actions = v2_actions_with_pl109(source, "basename")?;

    assert_no_affinity_import_edit(&actions);

    // Collateral control: PL109's independently justified fixes survive.
    assert!(
        actions.iter().any(|a| a.title == "Quote 'basename' with single quotes"),
        "PL109 single-quote fix must remain available; got {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a.kind == CodeActionKind::QuickFix),
        "PL109 quick fixes must remain available; got {actions:?}"
    );
    Ok(())
}

#[test]
fn pl109_route_cannot_offer_import_for_uppercase_filehandle_bareword() -> Result<(), String> {
    // Uppercase barewords keep their filehandle fix and never get imports.
    // The source deliberately lacks `use warnings;` so the unrelated
    // missing-pragma family must also stay available.
    let source = "use strict;\nopen FH, $path or die $!;\n";
    let actions = v2_actions_with_pl109(source, "FH")?;

    assert_no_affinity_import_edit(&actions);
    assert!(
        actions.iter().any(|a| a.title == "Declare 'FH' as filehandle"),
        "PL109 filehandle fix must remain available; got {actions:?}"
    );
    assert!(
        actions.iter().any(|a| a.title.contains("Add missing pragmas")),
        "the unrelated missing-pragma fix family must remain available; got {actions:?}"
    );
    Ok(())
}

#[test]
fn no_production_route_references_the_withdrawn_import_authority() -> Result<(), String> {
    let crates_dir = workspace_root()?.join("crates");

    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    let mut stack = vec![crates_dir.clone()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|error| {
            format!("failed to read source directory {}: {error}", dir.display())
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                format!("failed to inspect an entry in {}: {error}", dir.display())
            })?;
            let path = entry.path();
            if entry
                .file_type()
                .map_err(|error| {
                    format!("failed to inspect source path {}: {error}", path.display())
                })?
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
            let content = std::fs::read_to_string(&path).map_err(|error| {
                format!("failed to read production source {}: {error}", path.display())
            })?;
            scanned += 1;
            for (needle, explanation) in WITHDRAWN_IMPORT_AUTHORITY_PATTERNS {
                if content.contains(needle) {
                    offenders.push(format!("{relative}: {explanation}"));
                }
            }
            if contains_withdrawn_import_helper_invocation(&content) {
                offenders.push(format!(
                    "{relative}: invokes the retained package-blind insertion helper as production authority"
                ));
            }
            for (needle, inventoried_path, explanation) in PINNED_WITHDRAWN_AUTHORITY_PATTERNS {
                if content.contains(needle) && relative != *inventoried_path {
                    offenders.push(format!("{relative}: {explanation}"));
                }
            }
        }
    }

    assert!(scanned > 100, "source scan must traverse the workspace crates");
    assert!(
        offenders.is_empty(),
        "withdrawn hard-coded missing-import authority reappeared (restoration belongs to #790/#8948):\n{}",
        offenders.join("\n")
    );
    Ok(())
}

fn contains_withdrawn_import_helper_invocation(source: &str) -> bool {
    const METHOD: &str = "find_import_insert_position";
    const RETAINED_DECLARATION: &str = "pub fn find_import_insert_position(&self) -> usize {";

    let mut search_from = 0;
    while let Some(relative_offset) = source[search_from..].find(METHOD) {
        let offset = search_from + relative_offset;
        let method_end = offset + METHOD.len();
        if source[method_end..].trim_start().starts_with('(') {
            let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
            let line = source[line_start..].lines().next().map(str::trim).unwrap_or_default();
            if line != RETAINED_DECLARATION {
                return true;
            }
        }
        search_from = method_end;
    }

    false
}

/// Byte patterns whose presence under any `crates/*/src` path means the
/// withdrawn hard-coded import authority regained a production reference.
const WITHDRAWN_IMPORT_AUTHORITY_PATTERNS: &[(&str, &str)] = &[
    ("guess_module_for_function", "references the withdrawn hard-coded function-to-module table"),
    ("add_missing_imports", "references the withdrawn enhanced global missing-import action"),
    (
        "find_undefined_functions",
        "references the withdrawn table-driven undefined-function detector",
    ),
    ("fix_import_for_bareword_function", "references the withdrawn PL109 diagnostic import fix"),
    (
        "create_add_missing_imports_action",
        "re-creates the withdrawn compatibility empty-edit placeholder",
    ),
];

/// Byte patterns pinned to their single inventoried home. The parser-side
/// import optimizer (`perl-parser::refactor::import_optimizer`) is
/// dispositioned as withdrawn-authority-equivalent in
/// `.spec/10690-missing-import-containment/context.md`: compiled and publicly
/// re-exported, but no production request path reaches it today; restoration
/// belongs to #790/#8948. Occurrences inside that one file are the inventory
/// itself; any occurrence under any other `crates/*/src` path means the
/// affinity authority was restored or re-wired toward a live surface.
const PINNED_WITHDRAWN_AUTHORITY_PATTERNS: &[(&str, &str, &str)] = &[(
    "get_known_module_exports",
    "perl-parser/src/refactor/import_optimizer.rs",
    "reaches the withdrawn parser-side hard-coded module-export affinity table outside its inventoried home",
)];

#[test]
fn recurrence_guard_rejects_receiver_qualified_and_ufcs_helper_calls() {
    let declaration = r#"
        pub fn find_import_insert_position(&self) -> usize {
            0
        }
    "#;
    assert!(!contains_withdrawn_import_helper_invocation(declaration));

    let receiver_call = r#"
        helpers.find_import_insert_position()
    "#;
    assert!(contains_withdrawn_import_helper_invocation(receiver_call));

    let qualified_call = r#"
        Helpers::find_import_insert_position(&helpers);
    "#;
    assert!(contains_withdrawn_import_helper_invocation(qualified_call));

    let ufcs_call = r#"
        <Helpers as ImportHelpers>::find_import_insert_position(&helpers);
    "#;
    assert!(contains_withdrawn_import_helper_invocation(ufcs_call));
}

#[test]
fn feature_catalog_does_not_advertise_automatic_missing_import_insertion() -> Result<(), String> {
    let catalog = std::fs::read_to_string(workspace_root()?.join("features.toml"))
        .map_err(|error| format!("failed to read root features.toml: {error}"))?;

    let description = code_action_row_description(&catalog)
        .ok_or_else(|| "features.toml must keep an lsp.code_action feature row".to_string())?;

    for needle in ["missing import", "missing-import", "add missing"] {
        assert!(
            !description.to_lowercase().contains(needle),
            "the lsp.code_action catalog row must not advertise automatic missing-import insertion (#10690); got: {description}"
        );
    }
    Ok(())
}

/// Extract the `description` of the `id = "lsp.code_action"` feature row from
/// the raw catalog text without adding a TOML dependency to dev-deps.
fn code_action_row_description(catalog: &str) -> Option<String> {
    let mut in_row = false;
    for line in catalog.lines() {
        if line.trim_start().starts_with('[') {
            in_row = false;
        }
        if line.trim() == "id = \"lsp.code_action\"" {
            in_row = true;
            continue;
        }
        if in_row && let Some(description) = line.trim().strip_prefix("description =") {
            return Some(description.trim_matches('"').to_string());
        }
    }
    None
}
