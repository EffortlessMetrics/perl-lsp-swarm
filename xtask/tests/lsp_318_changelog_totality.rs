//! Totality contract for the official LSP 3.18 changelog.
//!
//! The generated conformance matrix already closes every row it contains. This
//! test guards the other failure mode: omitting a complete 3.18 addition from
//! the matrix while the remaining rows continue to pass their claim checks.

use std::{collections::BTreeSet, fs, path::PathBuf};

const MATRIX_PATH: &str = "docs/specs/lsp-318-conformance-matrix.md";
const CLOSED_STATUSES: &[&str] =
    &["implemented+tested+documented", "negative-gated+documented", "not-applicable+documented"];

const EXPECTED_CHANGELOG_IDS: &[&str] = &[
    "inline_completions",
    "dynamic_text_document_content",
    "folding_range_refresh",
    "multi_range_formatting",
    "workspace_edit_snippets",
    "relative_patterns_in_document_filters",
    "code_action_kind_documentation",
    "nullable_signature_help_active_parameter",
    "command_tooltips",
    "workspace_edit_metadata",
    "text_document_edit_snippets",
    "debug_message_kind",
    "code_lens_resolve_properties",
    "completion_list_apply_kind",
];

#[derive(Debug)]
struct ChangelogSurface {
    id: &'static str,
    row_markers: &'static [&'static str],
    required_text: &'static [&'static str],
}

// Canonical source: microsoft/language-server-protocol,
// _specifications/lsp/3.18/specification.md, Change Log, 3.18.0 (06/04/2026).
// Keep this inventory at exactly one entry per official changelog bullet. A
// single matrix row may satisfy more than one bullet only when the protocol
// models those bullets through the same wire type, as with SnippetTextEdit.
const OFFICIAL_CHANGELOG_SURFACES: &[ChangelogSurface] = &[
    ChangelogSurface {
        id: "inline_completions",
        row_markers: &[
            "Standard inline completion",
            "`selectedCompletionInfo` inline context",
            "Object-form `StringValue` inline insert text",
        ],
        required_text: &[],
    },
    ChangelogSurface {
        id: "dynamic_text_document_content",
        row_markers: &[
            "`workspace/textDocumentContent`",
            "`workspace/textDocumentContent/refresh`",
        ],
        required_text: &[],
    },
    ChangelogSurface {
        id: "folding_range_refresh",
        row_markers: &["`workspace/foldingRange/refresh`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "multi_range_formatting",
        row_markers: &["Multi-range formatting"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "workspace_edit_snippets",
        row_markers: &["`SnippetTextEdit` workspace edits"],
        required_text: &[],
    },
    // The watcher row owns the explicit document-selector non-claim; watcher
    // support does not substitute for the official document-filter surface.
    ChangelogSurface {
        id: "relative_patterns_in_document_filters",
        row_markers: &["RelativePattern watcher registrations", "Notebook 3.18 additions"],
        required_text: &["Document-selector `RelativePattern` support remains unclaimed."],
    },
    ChangelogSurface {
        id: "code_action_kind_documentation",
        row_markers: &["`CodeAction.documentation`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "nullable_signature_help_active_parameter",
        row_markers: &["`SignatureHelp.activeParameter = null`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "command_tooltips",
        row_markers: &["`Command.tooltip`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "workspace_edit_metadata",
        row_markers: &["`ApplyWorkspaceEditParams.metadata`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "text_document_edit_snippets",
        row_markers: &["`SnippetTextEdit` workspace edits"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "debug_message_kind",
        row_markers: &["`MessageType.Debug`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "code_lens_resolve_properties",
        row_markers: &["`CodeLens.resolveSupport.properties`"],
        required_text: &[],
    },
    ChangelogSurface {
        id: "completion_list_apply_kind",
        row_markers: &["`CompletionList.applyKind`"],
        required_text: &[],
    },
];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn row_cells<'a>(matrix: &'a str, feature: &str) -> Result<Vec<&'a str>, String> {
    let matching_rows: Vec<Vec<_>> = matrix
        .lines()
        .filter(|line| line.starts_with("| "))
        .map(|line| line.trim_matches('|').split('|').map(str::trim).collect())
        .filter(|cells: &Vec<&str>| cells.first().copied() == Some(feature))
        .collect();

    if matching_rows.len() != 1 {
        return Err(format!(
            "matrix feature `{feature}` must resolve to exactly one row, found {}",
            matching_rows.len()
        ));
    }

    let cells = matching_rows[0].clone();
    if cells.len() != 10 {
        return Err(format!(
            "matrix feature `{feature}` resolved to a malformed {}-cell row",
            cells.len()
        ));
    }

    Ok(cells)
}

#[test]
fn official_lsp_318_changelog_is_totally_classified() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        OFFICIAL_CHANGELOG_SURFACES.len(),
        14,
        "the June 4, 2026 LSP 3.18 changelog contains exactly 14 additions"
    );

    let matrix = fs::read_to_string(project_root().join(MATRIX_PATH))?;
    let mut ids = BTreeSet::new();

    let expected_ids: BTreeSet<_> = EXPECTED_CHANGELOG_IDS.iter().copied().collect();

    for surface in OFFICIAL_CHANGELOG_SURFACES {
        assert!(ids.insert(surface.id), "duplicate official LSP 3.18 changelog id: {}", surface.id);
        assert!(
            !surface.row_markers.is_empty(),
            "official LSP 3.18 changelog surface `{}` has no matrix classification",
            surface.id
        );

        for &marker in surface.row_markers {
            let cells = row_cells(&matrix, marker)?;
            let status = cells[5];
            assert!(
                CLOSED_STATUSES.contains(&status),
                "official LSP 3.18 surface `{}` maps to open status `{status}` through `{marker}`",
                surface.id
            );
        }

        for &marker in surface.required_text {
            assert!(
                matrix.contains(marker),
                "official LSP 3.18 surface `{}` lost required boundary text `{marker}`",
                surface.id
            );
        }
    }

    assert_eq!(
        ids, expected_ids,
        "official LSP 3.18 changelog inventory IDs changed or went missing"
    );
    Ok(())
}

#[test]
fn every_lsp_318_matrix_row_uses_a_closed_status() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = fs::read_to_string(project_root().join(MATRIX_PATH))?;
    let mut checked_rows = 0_usize;

    for line in matrix.lines().filter(|line| line.starts_with("| ")) {
        let cells: Vec<_> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.first().copied() == Some("Feature") || cells.first().copied() == Some("---") {
            continue;
        }
        if cells.len() != 10 {
            return Err(format!("malformed LSP 3.18 matrix row: {line}").into());
        }

        checked_rows += 1;
        assert!(
            CLOSED_STATUSES.contains(&cells[5]),
            "LSP 3.18 matrix row `{}` uses open or unknown status `{}`",
            cells[0],
            cells[5]
        );
    }

    assert!(
        checked_rows >= OFFICIAL_CHANGELOG_SURFACES.len(),
        "LSP 3.18 matrix unexpectedly shrank to {checked_rows} rows"
    );
    Ok(())
}
