//! Scenario 55 - DBI receiver inline-completion quality proof.
//!
//! This test exercises deterministic DBI receiver inline completion through a
//! real stdio LSP process. It records whether `$dbh->` and `$sth->f` ghost text
//! uses DBI handle methods instead of generic constructor guesses.

// UX receipt tests intentionally write structured receipts to stderr for --nocapture logs.
#![allow(clippy::print_stderr)]

use anyhow::Result;
use perl_lsp_ux_tests::{QualityPollOutcome, ScenarioConfig, UxHarness, binary_available};
use serde_json::json;
use std::time::Duration;

const DBI_HANDLE_PATH: &str = "lib/Inline/DbiHandle.pl";
const DBI_STATEMENT_PATH: &str = "lib/Inline/DbiStatement.pl";

const DBI_HANDLE_SOURCE: &str = r#"use strict;
use warnings;
use DBI;

my $dbh = DBI->connect($dsn);
$dbh->
"#;

const DBI_STATEMENT_SOURCE: &str = r#"use strict;
use warnings;
use DBI;

my $dbh = DBI->connect($dsn);
my $sth = $dbh->prepare($sql);
$sth->f
"#;

const DBI_HANDLE_LINE: u32 = 5;
const DBI_HANDLE_CHARACTER: u32 = 6;
const DBI_STATEMENT_LINE: u32 = 6;
const DBI_STATEMENT_CHARACTER: u32 = 7;
const EXPECTED_HANDLE_INSERTS: &[&str] = &["prepare()", "do()", "disconnect()"];
const EXPECTED_STATEMENT_INSERTS: &[&str] = &["fetchrow_hashref()", "fetchrow_array()", "finish()"];
const FORBIDDEN_INSERTS: &[&str] = &["new()"];

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default()
        .with_file(DBI_HANDLE_PATH, DBI_HANDLE_SOURCE)
        .with_file(DBI_STATEMENT_PATH, DBI_STATEMENT_SOURCE);
    config.client_capability_overrides = json!({
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });

    UxHarness::new(config)
}

fn missing_expected<'a>(insert_texts: &[String], expected: &'a [&str]) -> Vec<&'a str> {
    expected
        .iter()
        .copied()
        .filter(|expected| !insert_texts.iter().any(|actual| actual == expected))
        .collect()
}

fn present_forbidden<'a>(insert_texts: &[String], forbidden: &'a [&str]) -> Vec<&'a str> {
    forbidden
        .iter()
        .copied()
        .filter(|forbidden| insert_texts.iter().any(|actual| actual == forbidden))
        .collect()
}

fn wait_for_expected_inserts(
    harness: &UxHarness,
    file: &str,
    line: u32,
    character: u32,
    expected: &[&str],
) -> Result<(Vec<String>, QualityPollOutcome)> {
    // The quality budget must tolerate analysis lag on cold CI runners:
    // post-diagnostics, completion facts can land after the previous 5s
    // window closed (observed as a one-probe zero on a refreshed-base run).
    // The poll carries its [`QualityPollOutcome`] into the assertion message
    // so a load-induced budget exhaustion is rendered with the deadline
    // marker the receipt classifier keys on, rather than reading like a
    // real provider regression (#16103).
    harness.poll_inline_completion_until_quality(
        file,
        line,
        character,
        Duration::from_secs(30),
        |insert_texts| missing_expected(insert_texts, expected).is_empty(),
    )
}

#[test]
fn scenario_55_dbi_receiver_inline_completion_quality_stdio() -> Result<()> {
    if !binary_available() {
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file(DBI_HANDLE_PATH, DBI_HANDLE_SOURCE)?;
    harness.open_file(DBI_STATEMENT_PATH, DBI_STATEMENT_SOURCE)?;
    // Synchronize on the server's own analysis-readiness signal instead of a
    // fixed sleep: on a cold CI runner the previous 250ms guess let completion
    // queries outrun the first analysis of the just-opened document, starving
    // the DBI semantic context for the whole poll window (#15870).
    let readiness = harness.wait_for_diagnostics(DBI_HANDLE_PATH, Duration::from_secs(30));
    if readiness.is_empty() {
        return Err(anyhow::anyhow!(
            "analysis readiness: no publishDiagnostics; completion probes would poll blind (#15899)"
        )
        .into());
    }
    let readiness = harness.wait_for_diagnostics(DBI_STATEMENT_PATH, Duration::from_secs(30));
    if readiness.is_empty() {
        return Err(anyhow::anyhow!(
            "analysis readiness: no publishDiagnostics; completion probes would poll blind (#15899)"
        )
        .into());
    }

    let (handle_insert_texts, handle_outcome) = wait_for_expected_inserts(
        &harness,
        DBI_HANDLE_PATH,
        DBI_HANDLE_LINE,
        DBI_HANDLE_CHARACTER,
        EXPECTED_HANDLE_INSERTS,
    )?;
    let handle_missing = missing_expected(&handle_insert_texts, EXPECTED_HANDLE_INSERTS);
    let handle_forbidden = present_forbidden(&handle_insert_texts, FORBIDDEN_INSERTS);

    let (statement_insert_texts, statement_outcome) = wait_for_expected_inserts(
        &harness,
        DBI_STATEMENT_PATH,
        DBI_STATEMENT_LINE,
        DBI_STATEMENT_CHARACTER,
        EXPECTED_STATEMENT_INSERTS,
    )?;
    let statement_missing = missing_expected(&statement_insert_texts, EXPECTED_STATEMENT_INSERTS);
    let statement_forbidden = present_forbidden(&statement_insert_texts, FORBIDDEN_INSERTS);

    assert!(
        !handle_insert_texts.is_empty(),
        "DBI database handle inline completion returned no candidates; {}",
        handle_outcome.describe()
    );
    assert!(
        handle_missing.is_empty(),
        "DBI database handle inline completion missed expected methods: {handle_missing:?}; actual: {handle_insert_texts:?}; {}",
        handle_outcome.describe()
    );
    assert!(
        handle_forbidden.is_empty(),
        "DBI database handle inline completion returned forbidden methods: {handle_forbidden:?}; {}",
        handle_outcome.describe()
    );
    assert!(
        !statement_insert_texts.is_empty(),
        "DBI statement handle inline completion returned no candidates; {}",
        statement_outcome.describe()
    );
    assert!(
        statement_missing.is_empty(),
        "DBI statement handle inline completion missed expected methods: {statement_missing:?}; actual: {statement_insert_texts:?}; {}",
        statement_outcome.describe()
    );
    assert!(
        statement_forbidden.is_empty(),
        "DBI statement handle inline completion returned forbidden methods: {statement_forbidden:?}; {}",
        statement_outcome.describe()
    );

    // Pin that neither poll exhausted its budget. A passing run must end in
    // `Matched`, never `Deadline`, or the helper that we expect to time
    // out under load has already done so on the well-trodden path. This
    // catches a regression in the deadline budget itself (#16103).
    assert_eq!(
        handle_outcome,
        QualityPollOutcome::Matched,
        "DBI handle poll exhausted the deadline on a passing run; {}",
        handle_outcome.describe()
    );
    assert_eq!(
        statement_outcome,
        QualityPollOutcome::Matched,
        "DBI statement poll exhausted the deadline on a passing run; {}",
        statement_outcome.describe()
    );

    harness.assert_no_crash();
    Ok(())
}
