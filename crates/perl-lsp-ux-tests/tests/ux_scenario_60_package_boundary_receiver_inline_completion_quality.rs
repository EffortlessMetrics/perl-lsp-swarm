//! Scenario 60 - package-boundary receiver inline-completion quality proof.
//!
//! This receipt exercises `$self->` inline completion across a small
//! multi-package workspace. It verifies that receiver ghost text stays tied to
//! the package already on screen and does not leak methods from neighboring
//! packages or generic constructor guesses.

// UX receipt tests intentionally write structured receipts to stderr for --nocapture logs.
#![allow(clippy::print_stderr)]

use anyhow::{Context, Result};
use perl_lsp_ux_tests::{
    LspEvent, ScenarioConfig, UxCiTier, UxComponent, UxHarness, binary_available,
    missing_binary_skip, run_ux_scenario,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const SCENARIO_FILE: &str = "ux_scenario_60_package_boundary_receiver_inline_completion_quality.rs";

const MODEL_PATH: &str = "lib/App/Model/User.pm";
const CONTROLLER_PATH: &str = "lib/App/Controller/User.pm";
const ORDER_PATH: &str = "lib/App/Model/Order.pm";
const SERVICE_PATH: &str = "lib/App/Service/UserDirectory.pm";

const MODEL_SOURCE: &str = r#"package App::Model::User;
use strict;
use warnings;

sub save {}
sub display_name {}

sub persist {
    my ($self) = @_;
    $self->
}

1;
"#;

const CONTROLLER_SOURCE: &str = r#"package App::Controller::User;
use strict;
use warnings;

sub render_user {}
sub redirect_to_profile {}

sub dispatch {
    my ($self) = @_;
    $self->
}

1;
"#;

const ORDER_SOURCE: &str = r#"package App::Model::Order;
use strict;
use warnings;

sub charge {}
sub ship {}

1;
"#;

const SERVICE_SOURCE: &str = r#"package App::Service::UserDirectory;
use strict;
use warnings;

sub lookup {}
sub rebuild_index {}

1;
"#;

#[derive(Debug, Serialize)]
struct PackageBoundaryProbeReport {
    name: &'static str,
    file: &'static str,
    trigger_kind: u8,
    candidate_count: usize,
    insert_texts: Vec<String>,
    expected_insert_texts: Vec<&'static str>,
    missing_expected_insert_texts: Vec<&'static str>,
    forbidden_insert_texts: Vec<&'static str>,
}

#[derive(Debug)]
struct PackageBoundaryProbe {
    name: &'static str,
    file: &'static str,
    source: &'static str,
    marker: &'static str,
    expected: &'static [&'static str],
    forbidden: &'static [&'static str],
}

fn create_harness() -> Result<UxHarness> {
    let mut config = ScenarioConfig::default()
        .with_file(MODEL_PATH, MODEL_SOURCE)
        .with_file(CONTROLLER_PATH, CONTROLLER_SOURCE)
        .with_file(ORDER_PATH, ORDER_SOURCE)
        .with_file(SERVICE_PATH, SERVICE_SOURCE);
    config.client_capability_overrides = json!({
        "textDocument": {
            "inlineCompletion": {
                "dynamicRegistration": true
            }
        }
    });

    UxHarness::new(config)
}

fn package_boundary_probes() -> Vec<PackageBoundaryProbe> {
    vec![
        PackageBoundaryProbe {
            name: "model_self_receiver_stays_in_model_package",
            file: MODEL_PATH,
            source: MODEL_SOURCE,
            marker: "$self->",
            expected: &["save()", "display_name()"],
            forbidden: &[
                "render_user()",
                "redirect_to_profile()",
                "charge()",
                "ship()",
                "lookup()",
                "rebuild_index()",
                "new()",
            ],
        },
        PackageBoundaryProbe {
            name: "controller_self_receiver_stays_in_controller_package",
            file: CONTROLLER_PATH,
            source: CONTROLLER_SOURCE,
            marker: "$self->",
            expected: &["render_user()", "redirect_to_profile()"],
            forbidden: &[
                "save()",
                "display_name()",
                "charge()",
                "ship()",
                "lookup()",
                "rebuild_index()",
                "new()",
            ],
        },
    ]
}

fn position_after(source: &str, needle: &str) -> Result<(u32, u32)> {
    let byte_offset =
        source.find(needle).with_context(|| format!("missing `{needle}`"))? + needle.len();
    position_from_byte_offset(source, byte_offset)
}

fn position_from_byte_offset(source: &str, byte_offset: usize) -> Result<(u32, u32)> {
    let prefix = source
        .get(..byte_offset)
        .with_context(|| format!("byte offset {byte_offset} is not a UTF-8 boundary"))?;
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let character = prefix.rsplit('\n').next().map(str::chars).map(Iterator::count).unwrap_or(0);
    Ok((u32::try_from(line)?, u32::try_from(character)?))
}

fn inline_registration_seen(events: &[LspEvent]) -> bool {
    events.iter().any(|event| {
        let LspEvent::Other { method, params } = event else {
            return false;
        };
        method == "client/registerCapability"
            && params.get("registrations").and_then(Value::as_array).into_iter().flatten().any(
                |registration| {
                    registration.get("method").and_then(Value::as_str)
                        == Some("textDocument/inlineCompletion")
                        && registration.get("id").and_then(Value::as_str)
                            == Some("perl-inlineCompletion")
                },
            )
    })
}

fn wait_for_inline_registration(harness: &UxHarness) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if inline_registration_seen(&harness.client.peek_events()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn probe_inline_receiver(
    harness: &UxHarness,
    probe: &PackageBoundaryProbe,
) -> Result<PackageBoundaryProbeReport> {
    let (line, character) = position_after(probe.source, probe.marker)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let items = loop {
        let items = harness.inline_completion_with_trigger_kind(probe.file, line, character, 1)?;
        for item in &items {
            anyhow::ensure!(
                item.get("insertText").and_then(Value::as_str).is_some(),
                "inline item must include insertText: {item:?}"
            );
        }
        let insert_texts = insert_texts_for(&items);
        if probe
            .expected
            .iter()
            .all(|expected| insert_texts.iter().any(|actual| actual == expected))
            || Instant::now() >= deadline
        {
            break items;
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let insert_texts = insert_texts_for(&items);
    let missing_expected_insert_texts = probe
        .expected
        .iter()
        .copied()
        .filter(|expected| !insert_texts.iter().any(|actual| actual == expected))
        .collect::<Vec<_>>();
    let forbidden_insert_texts = probe
        .forbidden
        .iter()
        .copied()
        .filter(|forbidden| insert_texts.iter().any(|actual| actual == forbidden))
        .collect::<Vec<_>>();

    Ok(PackageBoundaryProbeReport {
        name: probe.name,
        file: probe.file,
        trigger_kind: 1,
        candidate_count: items.len(),
        insert_texts,
        expected_insert_texts: probe.expected.to_vec(),
        missing_expected_insert_texts,
        forbidden_insert_texts,
    })
}

fn insert_texts_for(items: &[Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get("insertText").and_then(Value::as_str).map(str::to_string))
        .collect()
}

#[test]
fn scenario_60_package_boundary_receiver_inline_completion_quality_receipt() {
    run_ux_scenario(
        "package_boundary_receiver_inline_completion_quality",
        SCENARIO_FILE,
        "scenario_60_package_boundary_receiver_inline_completion_quality_receipt",
        UxCiTier::Pr,
        Some(UxComponent::Completion),
        |recorder| {
            if !binary_available() {
                return Err(missing_binary_skip().into());
            }

            let harness = create_harness()?;
            for (path, source) in [
                (MODEL_PATH, MODEL_SOURCE),
                (CONTROLLER_PATH, CONTROLLER_SOURCE),
                (ORDER_PATH, ORDER_SOURCE),
                (SERVICE_PATH, SERVICE_SOURCE),
            ] {
                harness.open_file(path, source)?;
            }
            std::thread::sleep(Duration::from_millis(500));

            recorder.mark_request_start("dynamic_inline_registration");
            let dynamic_registration_seen = wait_for_inline_registration(&harness);
            if dynamic_registration_seen {
                recorder.mark_first_useful_result("dynamic_inline_registration");
            }

            let probes = package_boundary_probes();
            let mut reports = Vec::new();
            for probe in &probes {
                recorder.mark_request_start(probe.name);
                let report = probe_inline_receiver(&harness, probe)?;
                if report.missing_expected_insert_texts.is_empty()
                    && report.forbidden_insert_texts.is_empty()
                {
                    recorder.mark_first_useful_result(probe.name);
                }
                reports.push(report);
            }

            let missing_expected = reports
                .iter()
                .filter(|report| !report.missing_expected_insert_texts.is_empty())
                .map(|report| report.name)
                .collect::<Vec<_>>();
            let forbidden_hits = reports
                .iter()
                .filter(|report| !report.forbidden_insert_texts.is_empty())
                .map(|report| report.name)
                .collect::<Vec<_>>();

            let receipt = json!({
                "schema_version": 1,
                "receipt": "package_boundary_receiver_inline_completion_quality",
                "workspace_fixture": "multi-package App workspace with model, controller, order, and service packages",
                "claim_boundary": "project-shaped inline receiver package-boundary quality receipt only; no provider behavior change, support-tier promotion, source mirror, release action, or AI behavior",
                "dynamic_registration_seen": dynamic_registration_seen,
                "missing_expected": missing_expected,
                "forbidden_hits": forbidden_hits,
                "reports": reports,
            });
            eprintln!(
                "package_boundary_receiver_inline_completion_quality_receipt={}",
                serde_json::to_string_pretty(&receipt)?
            );

            recorder
                .check("dynamic inline registration was observed", dynamic_registration_seen)?;
            recorder.check(
                "all package-boundary receiver probes returned candidates",
                reports.iter().all(|report| report.candidate_count > 0),
            )?;
            recorder.check(
                "model receiver inline completion stayed in App::Model::User",
                reports.iter().any(|report| {
                    report.name == "model_self_receiver_stays_in_model_package"
                        && report.missing_expected_insert_texts.is_empty()
                        && report.forbidden_insert_texts.is_empty()
                }),
            )?;
            recorder.check(
                "controller receiver inline completion stayed in App::Controller::User",
                reports.iter().any(|report| {
                    report.name == "controller_self_receiver_stays_in_controller_package"
                        && report.missing_expected_insert_texts.is_empty()
                        && report.forbidden_insert_texts.is_empty()
                }),
            )?;
            recorder.check(
                "receiver inline completion avoided unrelated package and generic methods",
                reports.iter().all(|report| report.forbidden_insert_texts.is_empty()),
            )?;

            harness.assert_no_crash();
            Ok(())
        },
    );
}
