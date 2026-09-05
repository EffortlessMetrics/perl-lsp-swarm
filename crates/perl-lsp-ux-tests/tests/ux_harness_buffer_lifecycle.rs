// Test infrastructure — allow test-friendly patterns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anyhow::Result;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness, binary_available};
use std::time::Duration;

const FILE: &str = "live.pl";
const DISK_SENTINEL: &str = "sub disk_symbol { 1 }\n";
const INITIAL_BUFFER: &str = "sub initial_buffer_symbol { 1 }\n";
const CHANGED_BUFFER: &str = "sub changed_buffer_symbol { 1 }\n";
const REOPENED_BUFFER: &str = "sub reopened_buffer_symbol { 1 }\n";

#[test]
fn buffer_only_open_change_close_reopen_preserves_disk_and_resets_version() -> Result<()> {
    if !binary_available() {
        eprintln!("SKIP ux_harness_buffer_lifecycle: perl-lsp binary not found");
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(10), ..Default::default() }
            .with_file(FILE, DISK_SENTINEL),
    )?;

    assert_eq!(harness.tracked_document_version(FILE), None);
    assert!(harness.change_editor_buffer_full(FILE, CHANGED_BUFFER).is_err());
    assert!(harness.close_editor_buffer(FILE).is_err());

    harness.open_editor_buffer(FILE, INITIAL_BUFFER)?;
    assert_eq!(harness.tracked_document_version(FILE), Some(1));
    assert!(harness.open_editor_buffer(FILE, INITIAL_BUFFER).is_err());
    assert_eq!(std::fs::read_to_string(harness.workspace.path(FILE))?, DISK_SENTINEL);

    assert_eq!(harness.change_editor_buffer_full(FILE, CHANGED_BUFFER)?, 2);
    assert_eq!(harness.tracked_document_version(FILE), Some(2));
    assert_eq!(std::fs::read_to_string(harness.workspace.path(FILE))?, DISK_SENTINEL);

    harness.close_editor_buffer(FILE)?;
    assert_eq!(harness.tracked_document_version(FILE), None);
    assert!(harness.close_editor_buffer(FILE).is_err());

    harness.open_editor_buffer(FILE, REOPENED_BUFFER)?;
    assert_eq!(harness.tracked_document_version(FILE), Some(1));
    assert_eq!(harness.change_editor_buffer_full(FILE, CHANGED_BUFFER)?, 2);
    assert_eq!(harness.tracked_document_version(FILE), Some(2));
    assert_eq!(std::fs::read_to_string(harness.workspace.path(FILE))?, DISK_SENTINEL);

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn explicit_language_id_open_joins_version_ownership() -> Result<()> {
    if !binary_available() {
        eprintln!(
            "SKIP explicit_language_id_open_joins_version_ownership: perl-lsp binary not found"
        );
        return Ok(());
    }

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(10), ..Default::default() }
            .with_file(FILE, DISK_SENTINEL),
    )?;

    harness.open_file_with_language_id(FILE, INITIAL_BUFFER, "perl")?;
    assert_eq!(harness.tracked_document_version(FILE), Some(1));

    harness.close_editor_buffer(FILE)?;
    assert_eq!(harness.tracked_document_version(FILE), None);
    assert!(harness.close_editor_buffer(FILE).is_err());

    harness.assert_no_crash();
    Ok(())
}
