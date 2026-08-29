//! Closure-ownership contract for unresolved LSP 3.18 surfaces.
//!
//! The changelog-totality test proves every official addition is represented
//! in the generated matrix. This sibling contract proves that every matrix row
//! which is negative-gated, not applicable, or partly unclaimed has either an
//! implementation owner or an accepted disposition with recorded evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::{fs, path::PathBuf};

const MATRIX_PATH: &str = "docs/specs/lsp-318-conformance-matrix.md";
const LEDGER_PATH: &str = "docs/specs/lsp-318-closure-ownership.md";
const CLOSED_DISPOSITIONS: &[&str] = &["implementation-owner", "accepted-disposition"];
const LEDGER_HEADER: &[&str] =
    &["ID", "Matrix feature", "Disposition", "Owner", "Evidence", "Dependency", "Rationale"];
const EXPECTED_LEDGER_IDS: &[&str] = &[
    "command-tooltip-non-codelens",
    "generated-code-action-tags",
    "markdown-command-theme-icons",
    "multi-range-formatting",
    "notebook-318-additions",
    "relative-pattern-document-selector",
    "string-value-object-form",
];

#[derive(Debug)]
struct LedgerRow<'a> {
    id: &'a str,
    feature: &'a str,
    disposition: &'a str,
    owner: &'a str,
    evidence: &'a str,
    dependency: &'a str,
    rationale: &'a str,
}

fn project_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn read(relative: &str) -> Result<String, String> {
    let path = project_root().join(relative);
    fs::read_to_string(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn table_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    let body = trimmed.strip_prefix('|').unwrap_or(trimmed);
    let body = body.strip_suffix('|').unwrap_or(body);
    let mut cells = Vec::new();
    let mut start = 0;
    let mut in_code_span = false;
    let mut escaped = false;

    for (index, ch) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '`' {
            in_code_span = !in_code_span;
            continue;
        }
        if ch == '|' && !in_code_span {
            cells.push(body[start..index].trim());
            start = index + ch.len_utf8();
        }
    }
    cells.push(body[start..].trim());
    cells
}

fn table_rows(source: &str, expected_cells: usize) -> Result<Vec<Vec<&str>>, String> {
    let mut rows = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            continue;
        }
        let cells = table_cells(trimmed);
        if cells.iter().all(|cell| cell.chars().all(|ch| ch == '-' || ch == ' ')) {
            continue;
        }
        if cells.len() != expected_cells {
            return Err(format!(
                "malformed Markdown table row with {} cells, expected {expected_cells}: {line}",
                cells.len()
            ));
        }
        rows.push(cells);
    }
    Ok(rows)
}
fn parse_ledger(ledger: &str) -> Result<Vec<LedgerRow<'_>>, String> {
    let mut rows = table_rows(ledger, 7)?;
    if rows.first().map(Vec::as_slice) != Some(LEDGER_HEADER) {
        return Err("closure ledger must keep the reviewed six-column header".to_owned());
    }
    rows.remove(0);

    Ok(rows
        .into_iter()
        .map(|cells| LedgerRow {
            id: cells[0],
            feature: cells[1],
            disposition: cells[2],
            owner: cells[3],
            evidence: cells[4],
            dependency: cells[5],
            rationale: cells[6],
        })
        .collect())
}

fn parse_matrix(matrix: &str) -> Result<BTreeMap<&str, Vec<&str>>, String> {
    let mut rows = table_rows(matrix, 10)?;
    if rows.first().and_then(|cells| cells.first()).copied() != Some("Feature") {
        return Err("LSP 3.18 matrix must keep Feature as its first table column".to_owned());
    }
    rows.remove(0);

    let mut by_feature = BTreeMap::new();
    for cells in rows {
        let feature = cells[0];
        if by_feature.insert(feature, cells).is_some() {
            return Err(format!("duplicate LSP 3.18 matrix feature `{feature}`"));
        }
    }
    Ok(by_feature)
}

fn has_issue_reference(value: &str) -> bool {
    value.char_indices().any(|(index, ch)| {
        ch == '#'
            && value
                .get(index + 1..)
                .and_then(|rest| rest.chars().next())
                .is_some_and(|next| next.is_ascii_digit())
    })
}

fn is_single_issue_reference(value: &str) -> bool {
    value
        .trim()
        .strip_prefix('#')
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit()))
}

fn unresolved_matrix_features<'a>(matrix: &BTreeMap<&'a str, Vec<&'a str>>) -> BTreeSet<&'a str> {
    matrix
        .iter()
        .filter_map(|(&feature, cells)| {
            let status = cells[5];
            let notes = cells[9].to_ascii_lowercase();
            let unresolved =
                matches!(status, "negative-gated+documented" | "not-applicable+documented")
                    || notes.contains("remains unclaimed");
            unresolved.then_some(feature)
        })
        .collect()
}

#[test]
fn markdown_table_parser_handles_indentation_trailing_space_and_code_pipes()
-> Result<(), Box<dyn std::error::Error>> {
    let rows = table_rows(
        "  | ID | Feature | Disposition | Owner | Evidence | Dependency | Rationale |  \n  | --- | --- | --- | --- | --- | --- | --- |\n  | row | feature | accepted-disposition | n/a | compare `a | b` (#123). | none | material rationale here |  ",
        7,
    )?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][4], "compare `a | b` (#123).");
    Ok(())
}

#[test]
fn issue_reference_detection_accepts_markdown_and_sentence_punctuation() {
    assert!(has_issue_reference("[#1234]."));
    assert!(has_issue_reference("PR #42)"));
    assert!(!has_issue_reference("hash #"));
    assert!(!has_issue_reference("no issue reference"));
}

#[test]
fn implementation_owner_is_separate_from_historical_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let rows = parse_ledger(
        "| ID | Matrix feature | Disposition | Owner | Evidence | Dependency | Rationale |\n| --- | --- | --- | --- | --- | --- | --- |\n| row | feature | implementation-owner | #123 | boundary #9 | none | material rationale here |",
    )?;
    assert_eq!(rows[0].owner, "#123");
    assert_eq!(rows[0].evidence, "boundary #9");
    Ok(())
}
#[test]
fn unresolved_lsp_318_surfaces_have_closure_owners() -> Result<(), Box<dyn std::error::Error>> {
    let ledger_text = read(LEDGER_PATH)?;
    let ledger = parse_ledger(&ledger_text)?;
    let matrix_text = read(MATRIX_PATH)?;
    let matrix = parse_matrix(&matrix_text)?;

    let expected_ids: BTreeSet<_> = EXPECTED_LEDGER_IDS.iter().copied().collect();
    assert_eq!(
        expected_ids.len(),
        EXPECTED_LEDGER_IDS.len(),
        "reviewed closure-ledger IDs must be unique"
    );

    let mut observed_ids = BTreeSet::new();
    let mut observed_features = BTreeSet::new();
    for row in &ledger {
        assert!(observed_ids.insert(row.id), "duplicate closure-ledger ID `{}`", row.id);
        assert!(
            row.id.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'),
            "closure-ledger ID `{}` must use lowercase kebab-case tokens",
            row.id
        );
        assert!(
            CLOSED_DISPOSITIONS.contains(&row.disposition),
            "closure-ledger row `{}` uses unknown disposition `{}`",
            row.id,
            row.disposition
        );
        assert!(
            observed_features.insert(row.feature),
            "matrix feature `{}` has more than one closure-ledger row",
            row.feature
        );
        assert!(
            matrix.contains_key(row.feature),
            "closure-ledger feature `{}` does not resolve to an exact matrix row",
            row.feature
        );
        assert!(
            has_issue_reference(row.evidence),
            "closure-ledger row `{}` must name issue or PR evidence",
            row.id
        );
        assert!(
            !row.dependency.trim().is_empty(),
            "closure-ledger row `{}` must state a dependency or `none`",
            row.id
        );
        assert!(
            row.rationale.trim().len() >= 24,
            "closure-ledger row `{}` needs a material rationale",
            row.id
        );

        if row.disposition == "implementation-owner" {
            assert!(
                is_single_issue_reference(row.owner),
                "implementation-owner row `{}` must pin exactly one owning issue in Owner",
                row.id
            );
        }
    }

    assert_eq!(
        observed_ids, expected_ids,
        "checked closure-ledger IDs diverge from the reviewed denominator"
    );

    let unresolved = unresolved_matrix_features(&matrix);
    assert_eq!(
        observed_features, unresolved,
        "every negative, not-applicable, or partly unclaimed matrix feature must have exactly one closure-ledger decision"
    );
    Ok(())
}
