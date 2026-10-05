#![expect(
    clippy::print_stderr,
    reason = "Scenario 07 reports a local non-execution reason when the required perllsp binary is unavailable."
)]

//! Scenario 07 — Multi-file workspace / cross-file navigation.
//!
//! Sets up a small Perl project (cpanfile, library modules, script).
//! Verifies multi-file open and a configured static module definition target.
//!
//! Acceptance criteria:
//! - All files open without crashing.
//! - `textDocument/definition` resolves the configured `lib/Counter.pm` target.
//! - Server remains responsive after workspace indexing.

use perl_lsp_ux_tests::binary_available;
use perl_lsp_ux_tests::{ScenarioConfig, UxHarness};
use std::time::Duration;

#[test]
fn scenario_07_multi_file_workspace_opens_without_crash() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_07: perl-lsp binary not found");
        return Ok(());
    }

    let module_a = "package MyProject::Utils;\nuse strict;\nuse warnings;\n\n\
                    sub greet { my ($self, $name) = @_; return \"Hello, $name!\"; }\n1;\n";
    let module_b = "package MyProject::Config;\nuse strict;\nuse warnings;\n\n\
                    our $VERSION = '1.0';\nsub get_setting { return 'default'; }\n1;\n";
    let script = "#!/usr/bin/env perl\nuse strict;\nuse warnings;\n\n\
                  use MyProject::Utils;\nuse MyProject::Config;\n\n\
                  my $utils = MyProject::Utils->new();\nprint $utils->greet('World');\n";

    let harness = UxHarness::new(
        ScenarioConfig::default()
            .with_file("lib/MyProject/Utils.pm", module_a)
            .with_file("lib/MyProject/Config.pm", module_b)
            .with_file("script.pl", script)
            .with_file("cpanfile", "requires 'Moo', '2.0';\n"),
    )
    .map_err(|error| format!("Failed to create multi-file harness: {error}"))?;

    harness
        .open_file("lib/MyProject/Utils.pm", module_a)
        .map_err(|error| format!("Utils.pm should open: {error}"))?;
    harness
        .open_file("lib/MyProject/Config.pm", module_b)
        .map_err(|error| format!("Config.pm should open: {error}"))?;
    harness
        .open_file("script.pl", script)
        .map_err(|error| format!("script.pl should open: {error}"))?;

    harness.assert_no_crash();
    Ok(())
}

#[test]
fn scenario_07_definition_resolves_configured_module_target() -> Result<(), String> {
    if !binary_available() {
        eprintln!("SKIP scenario_07: perl-lsp binary not found");
        return Ok(());
    }

    let module = "package Counter;\nuse strict;\nuse warnings;\n\n\
                  sub new { bless {count => 0}, shift }\n\
                  sub increment { $_[0]->{count}++ }\n\
                  sub value { $_[0]->{count} }\n1;\n";
    let script = "use strict;\nuse warnings;\n\nuse Counter;\n\n\
                  my $c = Counter->new();\n$c->increment();\nprint $c->value();\n";

    let harness = UxHarness::new(
        ScenarioConfig { timeout: Duration::from_secs(15), ..Default::default() }
            .with_file("lib/Counter.pm", module)
            .with_file("main.pl", script),
    )
    .map_err(|error| format!("Failed to create harness: {error}"))?;

    harness
        .open_file("lib/Counter.pm", module)
        .map_err(|error| format!("Counter.pm should open: {error}"))?;
    harness
        .open_file("main.pl", script)
        .map_err(|error| format!("main.pl should open: {error}"))?;

    let definitions = harness
        .definition_with_retry("main.pl", 3, 4, 5, Duration::from_millis(250))
        .map_err(|error| format!("definition request crashed server — UX regression: {error}"))?;
    assert!(!definitions.is_empty(), "configured Counter module must have a definition target");
    let expected = url::Url::parse(harness.root_uri())
        .and_then(|root| root.join("lib/Counter.pm"))
        .map_err(|error| format!("invalid configured Counter URI: {error}"))?;
    let mut found_expected = false;
    let read_position = |value: &serde_json::Value| -> Option<(u32, u32)> {
        Some((
            u32::try_from(value.get("line")?.as_u64()?).ok()?,
            u32::try_from(value.get("character")?.as_u64()?).ok()?,
        ))
    };
    let read_range = |value: &serde_json::Value| {
        let start = read_position(value.get("start")?)?;
        let end = read_position(value.get("end")?)?;
        (start <= end).then_some((start, end))
    };
    for definition in &definitions {
        let (uri, range) = if let Some(uri) = definition.get("uri") {
            (uri, definition.get("range"))
        } else {
            (
                definition
                    .get("targetUri")
                    .ok_or("definition is not a Location or LocationLink")?,
                definition.get("targetRange"),
            )
        };
        let range = range.and_then(read_range).ok_or("definition must include a valid range")?;
        if definition.get("uri").is_none() {
            let selection = definition
                .get("targetSelectionRange")
                .and_then(read_range)
                .ok_or("LocationLink must include a valid target selection range")?;
            assert!(
                range.0 <= selection.0 && selection.1 <= range.1,
                "LocationLink selection must lie within its target range"
            );
        }
        let actual = url::Url::parse(uri.as_str().ok_or("definition URI is not a string")?)
            .map_err(|error| format!("invalid definition URI: {error}"))?;
        found_expected |= actual == expected;
    }
    assert!(
        found_expected,
        "definition must resolve exact configured URI {expected}; got {definitions:?}"
    );

    harness.assert_no_crash();
    Ok(())
}
