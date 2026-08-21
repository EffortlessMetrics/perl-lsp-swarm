//! Route/architecture containment guards for the withdrawn legacy
//! organize-imports organizer (issue #8305).
//!
//! The line-oriented import sorter (`collect_imports` → `sort_imports` →
//! `find_imports_range` → whole-interval replacement) is withdrawn from live
//! authority until issue #8319 admits a bounded source-preserving cohort and
//! issue #10696 lands the proven cutover. These tests fail if any production
//! route offers the legacy edit again, or if any production source re-wires the
//! withdrawn sorter pipeline.

use perl_lsp_rs_core::providers::code_actions::{CodeAction, EnhancedCodeActionsProvider};

/// Source with executable statements between import-looking lines. The legacy
/// organizer replaced the whole first-to-last import interval, destroying the
/// middle statements.
const EXECUTABLE_BETWEEN_IMPORTS: &str =
    "use warnings;\nmy $middle = 41;\nprint \"$middle\\n\";\nuse strict;\n";

fn enhanced_actions(source: &str, range: (usize, usize)) -> Result<Vec<CodeAction>, String> {
    let mut parser = perl_parser_core::Parser::new(source);
    let ast = parser.parse().map_err(|error| format!("fixture source must parse: {error:?}"))?;
    Ok(EnhancedCodeActionsProvider::new(source.to_string())
        .get_enhanced_refactoring_actions(&ast, range))
}

fn byte_offset_of_line(source: &str, line: usize) -> usize {
    source
        .match_indices('\n')
        .map(|(offset, _)| offset)
        .chain(std::iter::once(source.len()))
        .scan(0usize, move |seen, end| {
            let start = *seen;
            *seen = end + 1;
            Some((start, end))
        })
        .nth(line)
        .map(|(start, _)| start)
        .unwrap_or(source.len())
}

#[test]
fn enhanced_provider_never_offers_organize_imports_with_executable_statement_between_imports()
-> Result<(), String> {
    let source = EXECUTABLE_BETWEEN_IMPORTS;
    let actions = enhanced_actions(source, (0, source.len()))?;

    assert!(
        actions.iter().all(|action| action.title != "Organize imports"),
        "no action may reuse the withdrawn organizer title; got {:?}",
        actions.iter().map(|action| &action.title).collect::<Vec<_>>()
    );

    // No returned edit may span across the executable middle statements.
    let middle_start = byte_offset_of_line(source, 1);
    let middle_end = byte_offset_of_line(source, 3);
    for action in &actions {
        for edit in &action.edit.changes {
            let spans_middle =
                edit.location.start <= middle_start && edit.location.end >= middle_end;
            assert!(
                !spans_middle,
                "edit from {:?} spans executable statements between import-looking lines",
                action.title
            );
        }
    }
    Ok(())
}

#[test]
fn unfiltered_requests_on_import_heavy_source_contain_no_legacy_organizer_edit()
-> Result<(), String> {
    let source = "use JSON;\nuse Data::Dumper;\nuse warnings;\nuse File::Path;\nuse strict;\nuse lib './lib';\n\nprint \"test\\n\";\n";
    let actions = enhanced_actions(source, (0, source.len()))?;

    assert!(
        actions.iter().all(|action| action.title != "Organize imports"),
        "filtered or unfiltered clients must not receive the withdrawn organizer: {actions:?}"
    );
    Ok(())
}

#[test]
fn no_production_route_references_the_withdrawn_organizer() -> Result<(), String> {
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
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
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
            let content = match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(_) => continue,
            };
            scanned += 1;
            for (needle, explanation) in WITHDRAWN_ROUTE_PATTERNS {
                if content.contains(needle) {
                    offenders.push(format!("{relative}: {explanation}"));
                }
            }
        }
    }

    assert!(scanned > 100, "source scan must traverse the workspace crates");
    assert!(
        offenders.is_empty(),
        "withdrawn organize-imports routes reappeared (restoration belongs to #8319/#10696):\n{}",
        offenders.join("\n")
    );
    Ok(())
}

/// Byte patterns whose presence under any `crates/*/src` path means the
/// withdrawn sorter regained a production reference.
const WITHDRAWN_ROUTE_PATTERNS: &[(&str, &str)] = &[
    ("organize_imports", "references the withdrawn organize_imports symbol"),
    ("import_management::collect_imports", "re-wires the withdrawn line collector into production"),
    (
        "import_management::{collect_imports",
        "re-wires the withdrawn line collector into production",
    ),
    ("import_management::sort_imports", "re-wires the withdrawn bucket sorter into production"),
    ("import_management::{sort_imports", "re-wires the withdrawn bucket sorter into production"),
    (
        "import_management::find_imports_range",
        "re-wires the withdrawn broad-range locator into production",
    ),
    (
        "import_management::{find_imports_range",
        "re-wires the withdrawn broad-range locator into production",
    ),
];
