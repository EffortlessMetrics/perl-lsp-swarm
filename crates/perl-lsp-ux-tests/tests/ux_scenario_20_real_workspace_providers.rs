// Test infrastructure — allow test-friendly patterns used throughout this module.
// print_stderr is limited to the legacy direct-run missing-binary diagnostic.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

//! Scenario 20 — one assertion authority for the RealBaseline workspace.
//!
//! This target exercises completion, definition, hover, and diagnostics against
//! one four-file CPAN-style fixture. It deliberately does **not** keep the old
//! receipt-first pattern where a static missing or wrong answer was printed as a
//! "known gap" and returned `Ok(())` beside a hard regression-lock twin.
//!
//! ## Operational dispositions
//!
//! | Role | Meaning in this target |
//! |---|---|
//! | `RequiredKnownAnswer` | A static checked fixture has one externally falsifiable answer. Missing, malformed, unrelated, or wrong-root output fails. |
//! | `ExpectedBoundary` | A dynamic Perl construct may legitimately produce no exact result, but any result must stay inside the declared bounded class. |
//! | `InfrastructureControl` | The cell proves fixture, protocol-shape, or crash-safety behavior and cannot count as provider usefulness. |
//!
//! `CASE_DISPOSITIONS` is a deterministic review aid for #10012. The durable
//! selected-case policy is compiled later by #10020; this file is not a second
//! operational registry.
//!
//! The two old module-reference soft receipts are retired here. One targeted the
//! wrong source line and both accepted empty output as success. Scenario 21 owns
//! the real cross-file references regression oracle with a non-empty, cross-file
//! hard assertion.

use perl_lsp_ux_tests::{LspEvent, ScenarioConfig, UxHarness, binary_available};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use url::Url;

const APP_PM: &str = r#"package RealBaseline::App;
use strict;
use warnings;
use parent 'RealBaseline::Base';
use RealBaseline::Util qw(helper alias);

sub new {
    my ($class, %args) = @_;
    return bless \%args, $class;
}

sub run {
    my ($self) = @_;
    helper($self->name);
    alias($self->shared);
    return $self->shared;
}

sub name {
    return $_[0]->{name};
}

1;
"#;

const BASE_PM: &str = r#"package RealBaseline::Base;
use strict;
use warnings;

sub shared {
    return 'shared';
}

sub reset {
    return 1;
}

1;
"#;

const UTIL_PM: &str = r#"package RealBaseline::Util;
use strict;
use warnings;
use Exporter 'import';

our @EXPORT_OK = qw(helper alias);

sub helper {
    return shift;
}

*alias = \&helper;

sub bounce {
    goto &helper;
}

1;
"#;

const SCRIPT_PL: &str = r#"use strict;
use warnings;
use lib 'lib';
use RealBaseline::App;

my $app = RealBaseline::App->new(name => 'demo');
$app->run;
"#;

const SETTLEMENT_ATTEMPTS: usize = 5;
const SETTLEMENT_DELAY: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Scenario20Role {
    RequiredKnownAnswer,
    ExpectedBoundary,
    InfrastructureControl,
}

const CASE_DISPOSITIONS: &[(&str, Scenario20Role)] = &[
    (
        "scenario_20_fixture_exists_on_disk",
        Scenario20Role::InfrastructureControl,
    ),
    (
        "scenario_20_completion_items_valid_shape_in_base_pm",
        Scenario20Role::InfrastructureControl,
    ),
    (
        "scenario_20_completion_module_prefix_surfaces_real_baseline_app_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_completion_imported_symbol_helper_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_goto_definition_parent_class_resolves_to_base_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_goto_definition_inherited_method_shared_base_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_goto_definition_imported_helper_to_util_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_goto_definition_static_new_to_app_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_goto_definition_typeglob_alias_dynamic_boundary",
        Scenario20Role::ExpectedBoundary,
    ),
    (
        "scenario_20_hover_sub_shared_in_base_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_hover_module_import_in_app_pm_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_hover_inherited_method_call_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_hover_sub_helper_valid_shape_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_diagnostics_no_false_pl701_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_diagnostics_missing_module_fires_pl701_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_diagnostics_typeglob_alias_no_false_positive_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
    (
        "scenario_20_diagnostics_notification_received_for_all_files_hard_assert",
        Scenario20Role::RequiredKnownAnswer,
    ),
];

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate has parent")
        .join("perl-workspace")
        .join("tests")
        .join("fixtures")
        .join("semantic_real_workspace")
        .join("cpan_style")
}

fn create_harness() -> anyhow::Result<UxHarness> {
    UxHarness::new(
        ScenarioConfig::default()
            .with_file("lib/RealBaseline/App.pm", APP_PM)
            .with_file("lib/RealBaseline/Base.pm", BASE_PM)
            .with_file("lib/RealBaseline/Util.pm", UTIL_PM)
            .with_file("script/real-baseline.pl", SCRIPT_PL),
    )
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn uri_matches_file(uri: &str, expected_uri: &str, expected_path: &Path) -> bool {
    if uri == expected_uri {
        return true;
    }

    let Some(path) = Url::parse(uri).ok().and_then(|url| url.to_file_path().ok()) else {
        return false;
    };
    canonical(&path) == canonical(expected_path)
}

fn position_is_valid(position: &Value) -> bool {
    position.get("line").and_then(Value::as_u64).is_some()
        && position.get("character").and_then(Value::as_u64).is_some()
}

fn range_is_valid(range: &Value) -> bool {
    range.get("start").is_some_and(position_is_valid)
        && range.get("end").is_some_and(position_is_valid)
}

fn validate_location_entry(entry: &Value) -> anyhow::Result<()> {
    if let Some(target_uri) = entry.get("targetUri") {
        anyhow::ensure!(
            target_uri.is_string(),
            "LocationLink targetUri must be a string: {entry:?}"
        );
        anyhow::ensure!(
            entry.get("targetRange").is_some_and(range_is_valid),
            "LocationLink targetRange must be valid: {entry:?}"
        );
        if let Some(selection) = entry.get("targetSelectionRange") {
            anyhow::ensure!(
                range_is_valid(selection),
                "LocationLink targetSelectionRange must be valid: {entry:?}"
            );
        }
        return Ok(());
    }

    anyhow::ensure!(
        entry.get("uri").is_some_and(Value::is_string),
        "Location uri must be a string: {entry:?}"
    );
    anyhow::ensure!(
        entry.get("range").is_some_and(range_is_valid),
        "Location range must be valid: {entry:?}"
    );
    Ok(())
}

fn entry_uri(entry: &Value) -> Option<&str> {
    entry.get("targetUri").or_else(|| entry.get("uri")).and_then(Value::as_str)
}

fn entry_start_line(entry: &Value) -> Option<u64> {
    entry
        .get("targetSelectionRange")
        .or_else(|| entry.get("targetRange"))
        .or_else(|| entry.get("range"))?
        .get("start")?
        .get("line")?
        .as_u64()
}

fn assert_definition_target(
    definitions: &[Value],
    harness: &UxHarness,
    relative_path: &str,
    expected_line: u64,
    subject: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !definitions.is_empty(),
        "{subject} returned no definition after bounded settlement"
    );

    for entry in definitions {
        validate_location_entry(entry)?;
    }

    let expected_uri = harness.workspace.uri(relative_path);
    let expected_path = harness.workspace.path(relative_path);
    anyhow::ensure!(
        definitions.iter().any(|entry| {
            entry_uri(entry).is_some_and(|uri| {
                uri_matches_file(uri, &expected_uri, &expected_path)
                    && entry_start_line(entry) == Some(expected_line)
            })
        }),
        "{subject} must resolve to {relative_path} line {expected_line}; got {definitions:?}"
    );

    Ok(())
}

fn append_hover_text(value: &Value, output: &mut String) -> anyhow::Result<()> {
    match value {
        Value::String(text) => output.push_str(text),
        Value::Array(items) => {
            for item in items {
                append_hover_text(item, output)?;
                output.push('\n');
            }
        }
        Value::Object(map) => {
            let text = map.get("value").and_then(Value::as_str).ok_or_else(|| {
                anyhow::anyhow!("hover content object must contain string `value`: {value:?}")
            })?;
            output.push_str(text);
        }
        _ => anyhow::bail!("unsupported hover contents shape: {value:?}"),
    }
    Ok(())
}

fn hover_text(hover: &Value) -> anyhow::Result<String> {
    let contents = hover
        .get("contents")
        .ok_or_else(|| anyhow::anyhow!("hover result is missing contents: {hover:?}"))?;
    let mut text = String::new();
    append_hover_text(contents, &mut text)?;
    anyhow::ensure!(!text.trim().is_empty(), "hover contents must not be empty");
    Ok(text)
}

fn hover_with_retry<F>(
    harness: &UxHarness,
    relative_path: &str,
    line: u32,
    character: u32,
    useful: F,
) -> anyhow::Result<Value>
where
    F: Fn(&str) -> bool,
{
    let mut last_text = None;
    for attempt in 1..=SETTLEMENT_ATTEMPTS {
        if let Some(hover) = harness.hover(relative_path, line, character)? {
            let text = hover_text(&hover)?;
            if useful(&text) {
                return Ok(hover);
            }
            last_text = Some(text);
        }

        if attempt < SETTLEMENT_ATTEMPTS {
            std::thread::sleep(SETTLEMENT_DELAY);
        }
    }

    anyhow::bail!(
        "hover at {relative_path}:{line}:{character} never produced the required subject after \
         {SETTLEMENT_ATTEMPTS} attempts; last text: {last_text:?}"
    )
}

fn completion_labels_with_retry<F>(
    harness: &UxHarness,
    relative_path: &str,
    line: u32,
    character: u32,
    useful: F,
) -> anyhow::Result<Vec<String>>
where
    F: Fn(&str) -> bool,
{
    let mut last_labels = Vec::new();
    for attempt in 1..=SETTLEMENT_ATTEMPTS {
        let labels = harness.completion_labels(relative_path, line, character)?;
        if labels.iter().any(|label| useful(label)) {
            return Ok(labels);
        }
        last_labels = labels;

        if attempt < SETTLEMENT_ATTEMPTS {
            std::thread::sleep(SETTLEMENT_DELAY);
        }
    }

    anyhow::bail!(
        "completion at {relative_path}:{line}:{character} never produced the required candidate \
         after {SETTLEMENT_ATTEMPTS} attempts; last labels: {last_labels:?}"
    )
}

fn diagnostic_code(diag: &Value) -> Option<String> {
    diag.get("code").and_then(|code| {
        code.as_str().map(str::to_owned).or_else(|| code.as_u64().map(|value| value.to_string()))
    })
}

fn has_pl701(diags: &[Value]) -> bool {
    diags.iter().any(|diag| matches!(diagnostic_code(diag).as_deref(), Some("PL701" | "701")))
}

fn validate_diagnostics(diags: &[Value]) -> anyhow::Result<()> {
    for diag in diags {
        anyhow::ensure!(
            diag.get("range").is_some_and(range_is_valid),
            "diagnostic must contain a valid range: {diag:?}"
        );
        anyhow::ensure!(
            diag.get("message").is_some_and(Value::is_string),
            "diagnostic must contain a string message: {diag:?}"
        );
        if let Some(severity) = diag.get("severity") {
            let value = severity.as_u64().unwrap_or(0);
            anyhow::ensure!(
                (1..=4).contains(&value),
                "diagnostic severity must be in 1..=4: {diag:?}"
            );
        }
    }
    Ok(())
}

fn current_diagnostic_uris(harness: &UxHarness) -> BTreeSet<String> {
    harness
        .peek_notifications()
        .into_iter()
        .filter_map(|event| match event {
            LspEvent::Diagnostics { uri, .. } => Some(uri),
            _ => None,
        })
        .collect()
}

fn wait_for_diagnostic_uris(
    harness: &UxHarness,
    expected: &BTreeSet<String>,
    timeout: Duration,
) -> BTreeSet<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let seen = current_diagnostic_uris(harness);
        if expected.iter().all(|uri| seen.contains(uri)) || Instant::now() >= deadline {
            return seen;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn scenario_20_case_disposition_table_is_unique_and_complete() {
    let mut names = BTreeSet::new();
    for (name, _) in CASE_DISPOSITIONS {
        assert!(names.insert(*name), "duplicate Scenario 20 disposition for {name}");
    }

    assert_eq!(
        names.len(),
        17,
        "Scenario 20 review denominator changed; update #10012 disposition evidence deliberately"
    );
    assert_eq!(
        CASE_DISPOSITIONS
            .iter()
            .filter(|(_, role)| *role == Scenario20Role::RequiredKnownAnswer)
            .count(),
        14
    );
    assert_eq!(
        CASE_DISPOSITIONS
            .iter()
            .filter(|(_, role)| *role == Scenario20Role::ExpectedBoundary)
            .count(),
        1
    );
    assert_eq!(
        CASE_DISPOSITIONS
            .iter()
            .filter(|(_, role)| *role == Scenario20Role::InfrastructureControl)
            .count(),
        2
    );
}

#[test]
fn scenario_20_fixture_exists_on_disk() {
    let root = fixture_root();
    assert!(root.exists(), "real-workspace fixture directory must exist at: {}", root.display());

    for (relative, expected) in [
        ("lib/RealBaseline/App.pm", APP_PM),
        ("lib/RealBaseline/Base.pm", BASE_PM),
        ("lib/RealBaseline/Util.pm", UTIL_PM),
        ("script/real-baseline.pl", SCRIPT_PL),
    ] {
        let path = root.join(relative);
        let actual = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("fixture file must be readable at {}: {error}", path.display()));
        assert_eq!(
            actual.replace("\r\n", "\n"),
            expected,
            "inlined Scenario 20 fixture drifted from {}",
            path.display()
        );
    }
}

#[test]
fn scenario_20_completion_items_valid_shape_in_base_pm() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    let items = harness.completion("lib/RealBaseline/Base.pm", 4, 4)?;

    for item in &items {
        let map = item
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("completion item must be an object: {item:?}"))?;
        let has_user_text = ["label", "insertText", "filterText"]
            .iter()
            .any(|field| map.get(*field).is_some_and(Value::is_string));
        anyhow::ensure!(
            has_user_text,
            "completion item must contain string label, insertText, or filterText: {item:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_completion_module_prefix_surfaces_real_baseline_app_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("script/real-baseline.pl", SCRIPT_PL)?;
    let labels = completion_labels_with_retry(
        &harness,
        "script/real-baseline.pl",
        3,
        17,
        |label| label == "App" || label.contains("RealBaseline::App"),
    )?;

    assert!(
        labels.iter().any(|label| label == "App" || label.contains("RealBaseline::App")),
        "completion for `RealBaseline::` must surface RealBaseline::App: {labels:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_completion_imported_symbol_helper_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let labels = completion_labels_with_retry(
        &harness,
        "lib/RealBaseline/App.pm",
        13,
        7,
        |label| label.contains("helper"),
    )?;

    assert!(
        labels.iter().any(|label| label.contains("helper")),
        "imported helper must appear in completion labels: {labels:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_goto_definition_parent_class_resolves_to_base_pm_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    let definitions = harness.definition_with_retry(
        "lib/RealBaseline/App.pm",
        3,
        12,
        SETTLEMENT_ATTEMPTS,
        SETTLEMENT_DELAY,
    )?;

    assert_definition_target(
        &definitions,
        &harness,
        "lib/RealBaseline/Base.pm",
        0,
        "goto-definition on parent class RealBaseline::Base",
    )?;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_goto_definition_inherited_method_shared_base_pm_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    let definitions = harness.definition_with_retry(
        "lib/RealBaseline/App.pm",
        15,
        18,
        SETTLEMENT_ATTEMPTS,
        SETTLEMENT_DELAY,
    )?;

    assert_definition_target(
        &definitions,
        &harness,
        "lib/RealBaseline/Base.pm",
        4,
        "goto-definition on inherited method shared",
    )?;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_goto_definition_imported_helper_to_util_pm_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let definitions = harness.definition_with_retry(
        "lib/RealBaseline/App.pm",
        13,
        4,
        SETTLEMENT_ATTEMPTS,
        SETTLEMENT_DELAY,
    )?;

    assert_definition_target(
        &definitions,
        &harness,
        "lib/RealBaseline/Util.pm",
        7,
        "goto-definition on imported helper",
    )?;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_goto_definition_static_new_to_app_pm_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("script/real-baseline.pl", SCRIPT_PL)?;
    let definitions = harness.definition_with_retry(
        "script/real-baseline.pl",
        5,
        36,
        SETTLEMENT_ATTEMPTS,
        SETTLEMENT_DELAY,
    )?;

    assert_definition_target(
        &definitions,
        &harness,
        "lib/RealBaseline/App.pm",
        6,
        "goto-definition on RealBaseline::App->new",
    )?;
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_goto_definition_typeglob_alias_dynamic_boundary() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let definitions = harness.definition_with_retry(
        "lib/RealBaseline/Util.pm",
        11,
        1,
        3,
        SETTLEMENT_DELAY,
    )?;

    let expected_uri = harness.workspace.uri("lib/RealBaseline/Util.pm");
    let expected_path = harness.workspace.path("lib/RealBaseline/Util.pm");
    for entry in &definitions {
        validate_location_entry(entry)?;
        let uri = entry_uri(entry)
            .ok_or_else(|| anyhow::anyhow!("definition result has no target URI: {entry:?}"))?;
        anyhow::ensure!(
            uri_matches_file(uri, &expected_uri, &expected_path),
            "typeglob alias boundary must not fabricate a target outside Util.pm: {definitions:?}"
        );
        anyhow::ensure!(
            matches!(entry_start_line(entry), Some(7 | 11)),
            "typeglob alias boundary may target helper or the alias assignment only: {definitions:?}"
        );
    }

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_hover_sub_shared_in_base_pm_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    let hover = hover_with_retry(
        &harness,
        "lib/RealBaseline/Base.pm",
        4,
        4,
        |text| text.contains("shared"),
    )?;

    assert!(hover_text(&hover)?.contains("shared"));
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_hover_module_import_in_app_pm_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let hover = hover_with_retry(
        &harness,
        "lib/RealBaseline/App.pm",
        4,
        4,
        |text| text.contains("RealBaseline::Util") || text.contains("Util"),
    )?;

    let text = hover_text(&hover)?;
    assert!(text.contains("RealBaseline::Util") || text.contains("Util"));
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_hover_inherited_method_call_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    let hover = hover_with_retry(
        &harness,
        "lib/RealBaseline/App.pm",
        15,
        18,
        |text| text.contains("shared") || text.contains("Base"),
    )?;

    let text = hover_text(&hover)?;
    assert!(text.contains("shared") || text.contains("Base"));
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_hover_sub_helper_valid_shape_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let hover = hover_with_retry(
        &harness,
        "lib/RealBaseline/Util.pm",
        7,
        4,
        |text| text.contains("helper"),
    )?;

    assert!(hover_text(&hover)?.contains("helper"));
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_diagnostics_no_false_pl701_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    harness.open_file("lib/RealBaseline/Base.pm", BASE_PM)?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let diagnostics =
        harness.wait_for_diagnostics("lib/RealBaseline/App.pm", Duration::from_secs(5));

    validate_diagnostics(&diagnostics)?;
    anyhow::ensure!(
        !has_pl701(&diagnostics),
        "PL701 must not fire for modules present in the workspace: {diagnostics:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_diagnostics_missing_module_fires_pl701_hard_assert() -> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness =
        UxHarness::new(ScenarioConfig::default().with_file("lib/RealBaseline/App.pm", APP_PM))?;
    harness.open_file("lib/RealBaseline/App.pm", APP_PM)?;
    let diagnostics =
        harness.wait_for_diagnostics("lib/RealBaseline/App.pm", Duration::from_secs(5));

    validate_diagnostics(&diagnostics)?;
    anyhow::ensure!(
        has_pl701(&diagnostics),
        "PL701 must fire when RealBaseline::Base and RealBaseline::Util are absent: {diagnostics:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_diagnostics_typeglob_alias_no_false_positive_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    harness.open_file("lib/RealBaseline/Util.pm", UTIL_PM)?;
    let diagnostics =
        harness.wait_for_diagnostics("lib/RealBaseline/Util.pm", Duration::from_secs(5));

    validate_diagnostics(&diagnostics)?;
    let false_positive = diagnostics.iter().any(|diag| {
        matches!(diagnostic_code(diag).as_deref(), Some("PL304" | "304"))
            && diag
                .get("message")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("alias"))
    });
    anyhow::ensure!(
        !false_positive,
        "PL304 must not treat typeglob-created alias as a missing subroutine: {diagnostics:?}"
    );
    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_20_diagnostics_notification_received_for_all_files_hard_assert()
-> anyhow::Result<()> {
    if !binary_available() {
        eprintln!("SKIP scenario_20: perl-lsp binary not found");
        return Ok(());
    }

    let harness = create_harness()?;
    for (relative, source) in [
        ("lib/RealBaseline/App.pm", APP_PM),
        ("lib/RealBaseline/Base.pm", BASE_PM),
        ("lib/RealBaseline/Util.pm", UTIL_PM),
        ("script/real-baseline.pl", SCRIPT_PL),
    ] {
        harness.open_file(relative, source)?;
    }

    let expected: BTreeSet<String> = [
        "lib/RealBaseline/App.pm",
        "lib/RealBaseline/Base.pm",
        "lib/RealBaseline/Util.pm",
        "script/real-baseline.pl",
    ]
    .into_iter()
    .map(|relative| harness.workspace.uri(relative))
    .collect();
    let seen = wait_for_diagnostic_uris(&harness, &expected, Duration::from_secs(2));

    let missing: Vec<&String> = expected.iter().filter(|uri| !seen.contains(*uri)).collect();
    anyhow::ensure!(
        missing.is_empty(),
        "every opened file must receive a diagnostics publication; missing {missing:?}, seen {seen:?}"
    );
    harness.assert_no_crash();
    Ok(())
}
