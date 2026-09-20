//! Totality contract for the official LSP 3.18 changelog.
//!
//! The generated conformance matrix already closes every row it contains. This
//! test guards the other failure mode: omitting a complete 3.18 addition from
//! the matrix while the remaining rows continue to pass their claim checks.
//!
//! The authoritative bullet inventory is *not* handwritten here. It is parsed
//! from the vendored upstream specification fixture
//! (`fixtures/lsp-318-changelog/specification.md`, provenance and pinned
//! revision in the sibling `PROVENANCE.md`), so a fake replacement, a missing
//! bullet, or an invented bullet in the handwritten mapping below cannot
//! false-green: every parsed bullet must be classified exactly once, and every
//! classification must carry a verbatim upstream bullet.

use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::PathBuf};

const MATRIX_PATH: &str = "docs/specs/lsp-318-conformance-matrix.md";
const SPEC_FIXTURE_PATH: &str = "xtask/tests/fixtures/lsp-318-changelog/specification.md";
const PROVENANCE_PATH: &str = "xtask/tests/fixtures/lsp-318-changelog/PROVENANCE.md";
const PINNED_SPEC_REVISION: &str = "2cbcf18d991d3564af08fcbf5eb8b8af546a3e71";
const PINNED_SPEC_SECTION_HEADING: &str = "3.18.0 (06/04/2026)";
const CLOSED_STATUSES: &[&str] =
    &["implemented+tested+documented", "negative-gated+documented", "not-applicable+documented"];

#[derive(Debug)]
struct ChangelogSurface {
    /// Verbatim bullet text from the pinned upstream changelog fixture.
    bullet: &'static str,
    /// Matrix rows whose Status cells classify (part of) this bullet.
    row_markers: &'static [&'static str],
    /// Boundary text the matrix must retain for halves of the bullet that are
    /// explicit non-claims rather than matrix rows.
    required_text: &'static [&'static str],
}

// The mapping below is the only handwritten surface here, and it is checked in
// both directions against the parsed fixture inventory: a renamed, duplicated,
// or missing matrix row classification fails, and so does a bullet that gains
// or loses its single classification. One matrix row may classify more than
// one bullet only through `SHARED_ROW_ALLOWLIST`, with the protocol reason
// documented there.
const OFFICIAL_CHANGELOG_SURFACES: &[ChangelogSurface] = &[
    ChangelogSurface {
        bullet: "Added inline completions support.",
        row_markers: &[
            "Standard inline completion",
            "`selectedCompletionInfo` inline context",
            "Object-form `StringValue` inline insert text",
        ],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Added dynamic text document content support.",
        row_markers: &[
            "`workspace/textDocumentContent`",
            "`workspace/textDocumentContent/refresh`",
        ],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Added refresh support for folding ranges.",
        row_markers: &["`workspace/foldingRange/refresh`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support to format multiple ranges at once.",
        row_markers: &["Multi-range formatting"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support for snippets in workspace edits.",
        row_markers: &["`SnippetTextEdit` workspace edits"],
        required_text: &[],
    },
    // Both halves of the upstream bullet are represented explicitly: the
    // notebook-document-filter half is classified through the not-applicable
    // notebook row, while the document-filter half is a documented non-claim
    // guarded by required boundary text instead of a matrix row. The
    // implemented 3.17 file-watcher RelativePattern surface must not
    // substitute for either half.
    ChangelogSurface {
        bullet: "Relative Pattern support for document filters and notebook document filters.",
        row_markers: &["Notebook 3.18 additions"],
        required_text: &["Document-selector `RelativePattern` support remains unclaimed."],
    },
    ChangelogSurface {
        bullet: "Support for code action kind documentation.",
        row_markers: &["`CodeAction.documentation`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Add support for `activeParameter` on `SignatureHelp` and `SignatureInformation` being `null`.",
        row_markers: &["`SignatureHelp.activeParameter = null`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support tooltips for `Command`.",
        row_markers: &["`Command.tooltip`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support for meta data information on workspace edits.",
        row_markers: &["`ApplyWorkspaceEditParams.metadata`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support for snippets in text document edits.",
        row_markers: &["`SnippetTextEdit` workspace edits"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Support for debug message kind.",
        row_markers: &["`MessageType.Debug`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Client capability to enumerate properties that can be resolved for code lenses.",
        row_markers: &["`CodeLens.resolveSupport.properties`"],
        required_text: &[],
    },
    ChangelogSurface {
        bullet: "Added support for `completionList.applyKind` to determine how values from `completionList.itemDefaults` and `completion` are combined.",
        row_markers: &["`CompletionList.applyKind`"],
        required_text: &[],
    },
];

// Rows may classify more than one changelog bullet only when the protocol
// models those bullets through the same wire type: both snippet bullets are
// carried by the single `SnippetTextEdit` workspace-edit row.
const SHARED_ROW_ALLOWLIST: &[(&str, usize)] = &[("`SnippetTextEdit` workspace edits", 2)];

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn read(relative: &str) -> String {
    let path = project_root().join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}

/// Parse the pinned fixture's provenance header far enough to detect a stale
/// or hand-edited vendored copy.
fn assert_provenance_is_current(provenance: &str) {
    assert!(
        provenance.contains(PINNED_SPEC_REVISION),
        "provenance must pin the vendored specification revision {PINNED_SPEC_REVISION}"
    );
    assert!(
        provenance.contains("byte-for-byte copy"),
        "provenance must state that the fixture is a byte-for-byte upstream copy"
    );
}

/// Extract the bullet list of the pinned `3.18.0` Change Log section from the
/// vendored upstream specification.
fn parse_pinned_changelog_bullets(specification: &str) -> Vec<String> {
    let heading_line = specification
        .lines()
        .find(|line| line.contains(PINNED_SPEC_SECTION_HEADING) && line.starts_with("#### "))
        .unwrap_or_else(|| {
            panic!(
                "the vendored specification must contain the pinned {PINNED_SPEC_SECTION_HEADING} change-log heading"
            )
        });

    let mut bullets = Vec::new();
    let mut after_heading = false;
    for line in specification.lines() {
        if line == heading_line {
            after_heading = true;
            continue;
        }
        if !after_heading {
            continue;
        }
        if let Some(bullet) = line.strip_prefix("* ") {
            bullets.push(bullet.trim().to_owned());
        } else if !line.trim().is_empty() {
            break;
        }
    }
    bullets
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

/// Owner-module cells mix backticked repo paths with free-form prose; every
/// backticked path-shaped token must exist so a mapped classification cannot
/// silently outlive its owner.
fn assert_owner_modules_exist(cells: &[&str], marker: &str) {
    for segment in cells[7].split('`').skip(1).step_by(2) {
        let owner = segment.trim();
        if owner.is_empty() || owner.contains(char::is_whitespace) || !owner.contains('/') {
            continue;
        }
        assert!(
            project_root().join(owner).exists(),
            "matrix row `{marker}` declares owner module `{owner}` that does not exist"
        );
    }
}

fn classification_plan(matrix: &str) -> Result<BTreeMap<&'static str, Vec<&'static str>>, String> {
    let mut rows_to_bullets: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
    for surface in OFFICIAL_CHANGELOG_SURFACES {
        assert!(
            !surface.row_markers.is_empty() || !surface.required_text.is_empty(),
            "official LSP 3.18 changelog bullet `{}` has no classification",
            surface.bullet
        );

        for &marker in surface.row_markers {
            let cells = row_cells(matrix, marker)?;
            let status = cells[5];
            assert!(
                CLOSED_STATUSES.contains(&status),
                "official LSP 3.18 surface `{}` maps to open status `{status}` through `{marker}`",
                surface.bullet
            );
            if status == "implemented+tested+documented" {
                assert!(
                    !cells[6].trim().is_empty(),
                    "matrix row `{marker}` claims implemented proof without naming any"
                );
                assert_owner_modules_exist(&cells, marker);
            }
            rows_to_bullets.entry(marker).or_default().push(surface.bullet);
        }

        for &text in surface.required_text {
            assert!(
                matrix.contains(text),
                "official LSP 3.18 surface `{}` lost required boundary text `{text}`",
                surface.bullet
            );
        }
    }
    Ok(rows_to_bullets)
}

#[test]
fn vendored_specification_matches_its_recorded_provenance_hash() {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let provenance = read(PROVENANCE_PATH);
    let recorded = provenance
        .lines()
        .find_map(|line| line.trim().strip_prefix("- SHA-256: `"))
        .and_then(|rest| rest.strip_suffix('`'))
        .expect("provenance must record the vendored specification SHA-256");
    let mut digest = Sha256::new();
    digest.update(read(SPEC_FIXTURE_PATH).as_bytes());
    let observed = digest.finalize().iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    });

    assert_eq!(
        recorded.to_ascii_lowercase(),
        observed,
        "the vendored specification no longer matches the revision pinned in provenance"
    );
}

#[test]
fn official_lsp_318_changelog_is_totally_classified() -> Result<(), Box<dyn std::error::Error>> {
    let provenance = read(PROVENANCE_PATH);
    assert_provenance_is_current(&provenance);

    let specification = read(SPEC_FIXTURE_PATH);
    let bullets = parse_pinned_changelog_bullets(&specification);
    assert_eq!(
        bullets.len(),
        14,
        "the pinned June 4, 2026 LSP 3.18 changelog must keep exactly 14 bullets"
    );

    let matrix = read(MATRIX_PATH);
    let rows_to_bullets = classification_plan(&matrix)?;

    // Every handwritten classification must carry a real upstream bullet, and
    // every upstream bullet must be classified exactly once.
    let parsed: BTreeSet<&str> = bullets.iter().map(String::as_str).collect();
    let mut classified: BTreeSet<&str> = BTreeSet::new();
    for surface in OFFICIAL_CHANGELOG_SURFACES {
        assert!(
            parsed.contains(surface.bullet),
            "handwritten classification bullet `{}` is not a verbatim pinned changelog bullet",
            surface.bullet
        );
        assert!(
            classified.insert(surface.bullet),
            "duplicate classification for official LSP 3.18 changelog bullet: {}",
            surface.bullet
        );
    }
    assert_eq!(
        classified, parsed,
        "handwritten classifications and pinned changelog bullets diverge"
    );

    // A matrix row classifies more than one bullet only through the
    // documented protocol-level allowlist.
    for (marker, bullets_on_row) in &rows_to_bullets {
        if bullets_on_row.len() > 1 {
            let allowed = SHARED_ROW_ALLOWLIST
                .iter()
                .any(|(shared, count)| shared == marker && *count == bullets_on_row.len());
            assert!(
                allowed,
                "matrix row `{marker}` unexpectedly classifies {} changelog bullets: {:?}",
                bullets_on_row.len(),
                bullets_on_row
            );
        }
    }
    let shared_rows: usize = rows_to_bullets.values().filter(|bullets| bullets.len() > 1).count();
    assert_eq!(
        shared_rows,
        SHARED_ROW_ALLOWLIST.len(),
        "the shared-row allowlist must stay exactly as large as the shared rows in use"
    );
    Ok(())
}

#[test]
fn every_lsp_318_matrix_row_uses_a_closed_status() -> Result<(), Box<dyn std::error::Error>> {
    let matrix = read(MATRIX_PATH);
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
