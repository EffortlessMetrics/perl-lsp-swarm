//! Direct native results against the public `perllsp --stdio` formatting route.
//!
//! The runner must supply `PERL_LSP_BIN` from an explicitly admitted product
//! build. This target never builds a binary, searches PATH for one, or skips a
//! missing instrument. The public explanation command supplies the server's own
//! receipt: an empty edit array alone cannot distinguish refusal from no change.
//! The existing `unit_routed_full` gate in `.ci/gate-policy.yaml` builds the
//! public product and exports `PERL_LSP_BIN` while running package integration tests.
//!
//! On Unix, working PATH traps observe the controlled native formatting journey.
//! Startup observations are retained separately; this is not a claim that every
//! LSP subsystem is process-free. Core tests own native-construction isolation.

use anyhow::{Context, Result, anyhow, ensure};
use perl_lsp_rs_core::hashing::fnv1a64;
use perl_lsp_rs_core::providers::formatting::FormatTextEdit;
use perl_lsp_rs_core::providers::provider_decision::ProviderDecisionProvider;
// This is a direct re-export of perl_tidy, not the compatibility executor.
use perl_lsp_rs_core::tooling::perltidy::{
    ExactSource, FormatContext, FormatDisposition, FormatEngine, FormatEvidenceState,
    FormatOutcome, FormatReasonCode, FormatRequest, FormatRequestTarget, FormatSourceIdentity,
    FormatterMode, NativeFormatConfig, NativeFormatter,
};
use perl_lsp_ux_tests::{FakeWorkspace, ScenarioConfig, UxClient};
use perl_position_tracking::{
    PositionEncoding, PositionMappingDisposition, WirePosition, WireRange, wire_range_to_bytes,
};
use perl_source_identity::{ContentDigest, SourceGeneration};
use serde_json::{Value, json};
use std::ops::Range;
use std::path::Path;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
// didOpen commits the first accepted generation at 1 (#11305). The deliberately
// different client version makes confusing version and generation observable.
const OPEN_GENERATION: u64 = 1;
const OPEN_VERSION: i32 = 17;

struct Case {
    name: &'static str,
    source: &'static str,
    expected: &'static str,
    disposition: FormatDisposition,
    reason: FormatReasonCode,
}

struct WireReplay {
    text: String,
    edits: Vec<(Range<usize>, String)>,
}

struct ObservedFormatting {
    outcome: FormatOutcome,
    application_config_fingerprint: String,
}

fn product_binary() -> Result<String> {
    let binary = std::env::var("PERL_LSP_BIN").context(
        "set PERL_LSP_BIN to the absolute perllsp artifact from the admitted candidate build; \
         this process proof never starts Cargo or skips a missing binary",
    )?;
    let path = Path::new(&binary);
    ensure!(path.is_absolute(), "PERL_LSP_BIN must be absolute: {binary:?}");
    ensure!(path.is_file(), "PERL_LSP_BIN is not a file: {binary:?}");
    Ok(binary)
}

fn scenario(formatting: Value) -> ScenarioConfig {
    ScenarioConfig {
        timeout: REQUEST_TIMEOUT,
        client_capability_overrides: json!({
            "general": {"positionEncodings": ["utf-16"]},
        }),
        initialization_options: json!({
            "perl": {
                "workspace": {"useSystemInc": false, "usePerl5lib": false},
                "critic": {"enabled": false, "engine": "native"},
                "formatting": formatting,
            },
        }),
        ..ScenarioConfig::default()
    }
}

/// Replay every returned edit through the shared coordinate owner and an
/// independent string applicator. Reject malformed, inexact, overlapping or
/// reordered edits before touching the string; never clamp a wire position.
fn replay_wire_edits(source: &str, edits: &[FormatTextEdit]) -> Result<WireReplay> {
    let mut mapped = Vec::with_capacity(edits.len());
    for edit in edits {
        let mapping = wire_range_to_bytes(
            source,
            WireRange {
                start: WirePosition {
                    line: edit.range.start.line,
                    character: edit.range.start.character,
                },
                end: WirePosition {
                    line: edit.range.end.line,
                    character: edit.range.end.character,
                },
            },
            PositionEncoding::Utf16,
        );
        ensure!(
            mapping.disposition() == PositionMappingDisposition::Exact,
            "process returned inexact UTF-16 edit: {edit:?}, {mapping:?}"
        );
        let bytes = mapping.byte_range().ok_or_else(|| anyhow!("exact edit lacks byte range"))?;
        mapped.push((bytes, edit.new_text.clone()));
    }
    for pair in mapped.windows(2) {
        let left = &pair[0].0;
        let right = &pair[1].0;
        ensure!(left.end <= right.start, "process edits overlap or are reordered");
        ensure!(
            left.start != right.start || (!left.is_empty() && !right.is_empty()),
            "process edits have an ambiguous insertion boundary"
        );
    }
    let mut text = source.to_string();
    for (range, replacement) in mapped.iter().rev() {
        text.replace_range(range.clone(), replacement);
    }
    Ok(WireReplay { text, edits: mapped })
}

fn response_result(response: &Value) -> Result<&Value> {
    ensure!(response.get("error").is_none(), "process returned an error: {response}");
    response.get("result").ok_or_else(|| anyhow!("process response lacks result: {response}"))
}

#[test]
fn formatting_provider_round_trips_published_explanation_vocabulary() -> Result<()> {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/provider_decision.v1.schema.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(&schema_path)?)?;
    let providers = schema
        .pointer("/$defs/provider/enum")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("published schema lacks its provider vocabulary"))?;
    let formatting = serde_json::to_value(ProviderDecisionProvider::Formatting)?;
    assert_eq!(formatting, "formatting");
    ensure!(providers.contains(&formatting), "published schema must accept formatting");
    for value in providers {
        let provider: ProviderDecisionProvider = serde_json::from_value(value.clone())?;
        assert_eq!(serde_json::to_value(provider)?, *value);
    }
    for pointer in [
        "/properties/provider/$ref",
        "/$defs/request_receipt/properties/provider/$ref",
        "/$defs/copyable_payload/properties/provider/$ref",
    ] {
        assert_eq!(schema.pointer(pointer).and_then(Value::as_str), Some("#/$defs/provider"));
    }
    let unsupported = json!("not_a_provider");
    assert!(!providers.contains(&unsupported));
    assert!(serde_json::from_value::<ProviderDecisionProvider>(unsupported).is_err());
    Ok(())
}

fn assert_formatting_policy_defaults(explanation: &Value) -> Result<()> {
    let copyable = explanation
        .get("copyable_payload")
        .filter(|payload| payload.is_object())
        .ok_or_else(|| anyhow!("explanation lacks its copyable payload"))?;
    for policy in [explanation, copyable] {
        for (field, expected) in [
            ("provider", "formatting"),
            ("decision", "blocked"),
            ("reason", "missing_fact"),
            ("fact_source", "unknown"),
            ("confidence", "low"),
            ("freshness", "unknown"),
            ("fallback", "no_edit"),
        ] {
            assert_eq!(policy[field], expected, "policy field {field}");
        }
        assert_eq!(policy["dynamic_boundary"], false);
        for field in ["actual_engine", "format_outcome", "source_generation"] {
            ensure!(policy.get(field).is_none(), "policy cannot invent {field}: {policy}");
        }
        assert!(policy["receipt_id"].is_null());
        assert!(policy["scenario"].is_null());
    }
    Ok(())
}

fn assert_no_retained_formatting_receipt(client: &UxClient) -> Result<()> {
    // A fresh process has no formatting evidence. The same public request used
    // after formatting must succeed without inventing an execution or a receipt.
    let response = client.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{"provider": "formatting"}],
        }),
        REQUEST_TIMEOUT,
    )?;
    let explanation = response_result(&response)?;
    assert_formatting_policy_defaults(explanation)?;
    assert!(explanation.get("request_receipt").is_none());
    assert!(explanation["copyable_payload"]["request_receipt"].is_null());
    let message = explanation["user_message"]
        .as_str()
        .ok_or_else(|| anyhow!("formatting explanation lacks its user message"))?;
    assert!(message.starts_with("No request evidence is attached.\nProvider policy summary:\n"));
    assert!(message.contains("Formatting blocked."));
    assert!(message.contains("Fallback: no edit."));
    assert_eq!(explanation["copyable_payload"]["user_message"], explanation["user_message"]);
    Ok(())
}

#[test]
fn wire_replay_rejects_bad_geometry_and_ambiguous_edit_sets() -> Result<()> {
    let source = "a😀\r\nb";
    let edit = |start: (u32, u32), end: (u32, u32)| -> Result<FormatTextEdit> {
        Ok(serde_json::from_value(json!({
            "range": {
                "start": {"line": start.0, "character": start.1},
                "end": {"line": end.0, "character": end.1},
            },
            "newText": "X",
        }))?)
    };
    let valid = replay_wire_edits(source, &[edit((0, 1), (0, 3))?])?;
    assert_eq!(valid.text, "aX\r\nb");
    assert_eq!(valid.edits[0].0, 1..5);
    for bad in [
        vec![edit((0, 2), (0, 3))?], // Middle of the astral surrogate pair.
        vec![edit((0, 0), (0, 4))?], // Past the CRLF line's content end.
        vec![edit((2, 0), (2, 0))?], // Nonexistent line.
        vec![edit((0, 3), (0, 1))?], // Reversed endpoints.
        vec![edit((0, 0), (0, 3))?, edit((0, 1), (0, 3))?], // Overlap.
        vec![edit((0, 1), (0, 3))?, edit((0, 0), (0, 1))?], // Reordered.
        vec![edit((0, 1), (0, 1))?, edit((0, 1), (0, 1))?], // Ambiguous inserts.
    ] {
        ensure!(replay_wire_edits(source, &bad).is_err(), "invalid wire edits passed: {bad:?}");
    }
    Ok(())
}

fn exercise_case(
    client: &UxClient,
    workspace: &FakeWorkspace,
    case: &Case,
    config: &NativeFormatConfig,
    options: Value,
) -> Result<ObservedFormatting> {
    let uri = workspace.uri(&format!("{}.pl", case.name));
    let normalized_uri = perl_uri::uri_key(&uri);
    let uri_hash = format!("{:016x}", fnv1a64(normalized_uri.as_bytes()));

    // Derive both exact content and generation from the input we send, never
    // from the process result that this test is meant to challenge. A URI is a
    // provenance label, not a canonical LogicalSourceId.
    let mut identity = FormatSourceIdentity::content_only(case.source);
    identity.generation = SourceGeneration::known(OPEN_GENERATION.to_string());
    let exact = ExactSource::with_identity(case.source, identity)?;
    let context = FormatContext::new(Some(normalized_uri), Some(OPEN_GENERATION));
    let direct =
        NativeFormatter::new().format(&exact, &FormatRequest::document(&exact), config, &context);
    assert_eq!(direct.outcome.disposition, case.disposition, "{}", case.name);
    assert_eq!(direct.outcome.reason, case.reason, "{}", case.name);
    assert_eq!(direct.result.formatted, case.expected, "{}", case.name);
    assert_eq!(direct.apply_exact(&exact)?, case.expected, "{}", case.name);

    client.notify(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": uri, "languageId": "perl", "version": OPEN_VERSION,
                "text": case.source,
            },
        }),
    )?;
    let response = client.request(
        "textDocument/formatting",
        json!({
            "textDocument": {"uri": uri, "version": OPEN_VERSION},
            "options": options,
        }),
        REQUEST_TIMEOUT,
    )?;
    let edits: Vec<FormatTextEdit> = serde_json::from_value(response_result(&response)?.clone())
        .context("formatting must return a well-formed edit array")?;
    let replayed = replay_wire_edits(case.source, &edits)?;
    assert_eq!(replayed.text, direct.result.formatted, "{}", case.name);
    assert_eq!(replayed.edits.len(), direct.result.edits.len(), "{}", case.name);
    for ((range, replacement), native) in replayed.edits.iter().zip(&direct.result.edits) {
        assert_eq!(range, &(native.range.start()..native.range.end()), "{}", case.name);
        assert_eq!(replacement, &native.new_text, "{}", case.name);
    }

    // Serialized request/receipt pairs keep the latest trace attributable to
    // this case. No caller receipt is supplied, and no request_id selector is
    // used: the current formatting receipt does not carry a request ID.
    let explanation = client.request(
        "workspace/executeCommand",
        json!({
            "command": "perl.explainProviderDecision",
            "arguments": [{"provider": "formatting"}],
        }),
        REQUEST_TIMEOUT,
    )?;
    let explanation = response_result(&explanation)?;
    // The default policy remains distinct from an attached actual outcome,
    // including successful formatting and the no-change case below.
    assert_formatting_policy_defaults(explanation)?;
    let receipt = explanation
        .get("request_receipt")
        .filter(|receipt| receipt.is_object())
        .ok_or_else(|| anyhow!("explanation did not attach the live formatting receipt"))?;
    assert_eq!(receipt["provider"], "formatting");
    assert_eq!(receipt["provider_action"], "textDocument/formatting");
    assert_eq!(receipt["source_id_hash"], uri_hash);
    assert_eq!(receipt["source_generation"], OPEN_GENERATION);
    assert_eq!(receipt["document_version"], OPEN_VERSION);
    assert_eq!(receipt["result_count"], edits.len());
    assert_eq!(receipt["configured_enabled"], true);
    assert_eq!(receipt["configured_mode"], "native");
    assert_eq!(receipt["requested_mode"], "native");
    assert_eq!(receipt["effective_mode"], "native");
    assert_eq!(receipt["actual_engine"], "native");
    assert_eq!(receipt["freshness"], "fresh");
    let outcome_value = receipt.get("format_outcome").ok_or_else(|| anyhow!("missing outcome"))?;
    assert_eq!(outcome_value["identity"]["source_id_hash"], uri_hash);
    ensure!(outcome_value["identity"].get("source_id").is_none(), "raw source label escaped");
    let outcome: FormatOutcome = serde_json::from_value(outcome_value.clone())?;

    assert_eq!(outcome.disposition, direct.outcome.disposition);
    assert_eq!(outcome.reason, direct.outcome.reason);
    assert_eq!(receipt["reason"], serde_json::to_value(direct.outcome.reason)?);
    assert_eq!(outcome.source, *exact.identity());
    assert_eq!(outcome.source.content, ContentDigest::of_bytes(case.source.as_bytes()));
    assert_eq!(outcome.source.generation, SourceGeneration::known(OPEN_GENERATION.to_string()));
    assert!(outcome.source.logical_source.is_none(), "URI must not become canonical source ID");
    assert_eq!(outcome.identity.content_digest, exact.identity().content.to_string());
    assert_eq!(outcome.identity.source_generation, Some(OPEN_GENERATION));
    assert_eq!(outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(outcome.identity.requested_mode, FormatterMode::Native);
    assert_eq!(outcome.target, FormatRequestTarget::Document);
    assert_eq!(outcome.admitted, Some(exact.whole_document()));
    assert_eq!(outcome.admitted, direct.outcome.admitted);
    assert_eq!(outcome.widening, None);
    assert_eq!(outcome.coverage, direct.outcome.coverage);
    assert_eq!(outcome.execution, direct.outcome.execution);
    assert_eq!(outcome.change, direct.outcome.change);
    assert_eq!(outcome.change.edit_count, edits.len());
    assert_eq!(outcome.safety, direct.outcome.safety);
    assert_eq!(outcome.next_action, direct.outcome.next_action);
    ensure!(!outcome.identity.config_fingerprint.is_empty(), "missing adapter policy identity");
    assert_ne!(
        outcome.identity.config_fingerprint, direct.outcome.identity.config_fingerprint,
        "adapter identity includes complete editor options, unlike pure native config"
    );
    let application_config_fingerprint = receipt["config_fingerprint"]
        .as_str()
        .filter(|fingerprint| !fingerprint.is_empty())
        .ok_or_else(|| anyhow!("missing application configuration identity"))?
        .to_string();

    match case.disposition {
        FormatDisposition::Applied => {
            assert!(!edits.is_empty(), "applied case cannot pass through empty edits");
            assert_ne!(replayed.text, case.source);
            assert_eq!(receipt["decision"], "acted");
            assert_eq!(receipt["fallback"], "none");
        }
        FormatDisposition::NoChange => {
            assert!(edits.is_empty());
            assert_eq!(replayed.text, case.source);
            assert_eq!(receipt["decision"], "acted");
            assert_eq!(receipt["fallback"], "none");
        }
        FormatDisposition::Refused => {
            assert!(edits.is_empty());
            assert_eq!(replayed.text, case.source);
            assert_eq!(receipt["decision"], "blocked");
            assert_eq!(receipt["fallback"], "no_edit");
        }
        FormatDisposition::FailedOrNotProven => {
            return Err(anyhow!("this fixture cohort does not manufacture instrument failures"));
        }
    }
    if case.name == "partial_literal_preservation" {
        assert_eq!(outcome.safety.literal_preservation, FormatEvidenceState::PreservedPartially);
    }
    if case.name == "crlf_astral_true_eof" {
        let last = edits.last().ok_or_else(|| anyhow!("missing astral document edit"))?;
        assert_eq!((last.range.end.line, last.range.end.character), (1, 4));
        assert_ne!(last.range.end.character as usize, "# 😀".len());
    }
    client.notify("textDocument/didClose", json!({"textDocument": {"uri": uri}}))?;
    Ok(ObservedFormatting { outcome, application_config_fingerprint })
}

fn with_process(
    mut config: ScenarioConfig,
    exercise: impl FnOnce(&UxClient, &FakeWorkspace) -> Result<()>,
) -> Result<()> {
    let binary = product_binary()?;
    let workspace = FakeWorkspace::new()?;
    // The parameter remains mutable on platforms without executable shell traps.
    config.timeout = REQUEST_TIMEOUT;
    #[cfg(unix)]
    let probe = PathProbe::install(&mut config)?;
    let client = UxClient::spawn(&binary, &workspace, &config)?;
    let initialized = client.initialize_result();
    let capabilities = &response_result(&initialized)?["capabilities"];
    assert_eq!(capabilities["positionEncoding"], "utf-16");
    assert_eq!(capabilities["documentFormattingProvider"], true);
    ensure!(capabilities.get("documentRangeFormattingProvider").is_none());
    ensure!(capabilities.get("documentOnTypeFormattingProvider").is_none());
    #[cfg(unix)]
    let startup_calls = probe.startup_observation()?;
    assert_no_retained_formatting_receipt(&client)?;
    exercise(&client, &workspace)?;
    let shutdown = client.shutdown_and_exit(REQUEST_TIMEOUT)?;
    ensure!(shutdown.status.success(), "public process did not exit successfully");
    #[cfg(unix)]
    probe.assert_no_journey_calls(&startup_calls)?;
    Ok(())
}

#[test]
fn direct_native_and_public_stdio_share_document_behavior() -> Result<()> {
    // The server's actual default width is 80; the pure library default is 100.
    let config = NativeFormatConfig { line_width: 80, ..NativeFormatConfig::default() };
    with_process(scenario(json!({})), |client, workspace| {
        let cases = [
            Case {
                name: "applied",
                source: "my$x=1;\n",
                expected: "my $x = 1;\n",
                disposition: FormatDisposition::Applied,
                reason: FormatReasonCode::Applied,
            },
            Case {
                name: "no_change",
                source: "my $x = 1;\n",
                expected: "my $x = 1;\n",
                disposition: FormatDisposition::NoChange,
                reason: FormatReasonCode::AlreadyFormatted,
            },
            Case {
                name: "parse_refusal",
                source: "my $x = ;\n",
                expected: "my $x = ;\n",
                disposition: FormatDisposition::Refused,
                reason: FormatReasonCode::SourceParseError,
            },
            Case {
                name: "partial_literal_preservation",
                source: "my$x=1;\nmy $text = <<'END';\nvalue  \nEND\n",
                expected: "my $x = 1;\nmy $text = <<'END';\nvalue  \nEND\n",
                disposition: FormatDisposition::Applied,
                reason: FormatReasonCode::AppliedPreservingLiterals,
            },
            Case {
                name: "crlf_astral_true_eof",
                source: "my$x=1;\r\n# 😀",
                expected: "my $x = 1;\r\n# 😀",
                disposition: FormatDisposition::Applied,
                reason: FormatReasonCode::Applied,
            },
        ];
        for case in cases {
            exercise_case(
                client,
                workspace,
                &case,
                &config,
                json!({"tabSize": 4, "insertSpaces": true}),
            )
            .with_context(|| format!("public process parity case {}", case.name))?;
        }
        Ok(())
    })
}

#[test]
fn explicit_native_policy_overrides_editor_style_with_distinct_adapter_identity() -> Result<()> {
    let config = NativeFormatConfig {
        line_width: 80,
        indent_width: 2,
        use_tabs: false,
        ..NativeFormatConfig::default()
    };
    with_process(
        scenario(json!({
            "engine": "native", "maximumLineLength": 80, "indentColumns": 2, "tabs": false,
        })),
        |client, workspace| {
            let case = Case {
                name: "explicit_native_indent",
                source: "sub f {\nmy$x=1;\n}\n",
                expected: "sub f {\n  my $x = 1;\n}\n",
                disposition: FormatDisposition::Applied,
                reason: FormatReasonCode::Applied,
            };
            let baseline = exercise_case(
                client,
                workspace,
                &case,
                &config,
                json!({"tabSize": 4, "insertSpaces": true}),
            )?;
            let contrary_options = exercise_case(
                client,
                workspace,
                &case,
                &config,
                json!({"tabSize": 8, "insertSpaces": false}),
            )?;
            assert_eq!(
                baseline.application_config_fingerprint,
                contrary_options.application_config_fingerprint
            );
            assert_ne!(
                baseline.outcome.identity.config_fingerprint,
                contrary_options.outcome.identity.config_fingerprint,
                "editor options remain bound even when explicit native style wins"
            );
            Ok(())
        },
    )
}

#[cfg(unix)]
struct PathProbe {
    _directory: tempfile::TempDir,
    log: std::path::PathBuf,
}

#[cfg(unix)]
impl PathProbe {
    fn install(config: &mut ScenarioConfig) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        let directory = tempfile::tempdir()?;
        let witness_log = directory.path().join("positive-controls.log");
        let log = directory.path().join("process-observations.log");
        let mut witnessed = Vec::new();
        for name in ["perl", "perltidy"] {
            let executable = directory.path().join(name);
            std::fs::write(
                &executable,
                "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$PERL_TIDY_PROCESS_PROBE_LOG\"\nexit 73\n",
            )?;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755))?;
            let status = Command::new(name)
                .env("PATH", directory.path())
                .env("PERL_TIDY_PROCESS_PROBE_LOG", &witness_log)
                .status()
                .with_context(|| format!("execute {name} PATH trap positive control"))?;
            assert_eq!(status.code(), Some(73), "{name} trap did not execute");
            witnessed.push(executable.to_string_lossy().into_owned());
        }
        let witness = std::fs::read_to_string(&witness_log)?;
        assert_eq!(witness.lines().map(ToOwned::to_owned).collect::<Vec<_>>(), witnessed);
        // Separate logs preserve both positive controls and every real-process
        // call. No reset can hide a startup probe or a late formatting probe.
        std::fs::write(&log, b"")?;
        config.path_restriction = Some(vec![directory.path().to_string_lossy().into_owned()]);
        config.extra_env.push((
            "PERL_TIDY_PROCESS_PROBE_LOG".to_string(),
            Some(log.to_string_lossy().into_owned()),
        ));
        Ok(Self { _directory: directory, log })
    }

    #[expect(
        clippy::print_stderr,
        reason = "report startup tool observations separately from the native formatting journey"
    )]
    fn startup_observation(&self) -> Result<Vec<u8>> {
        let startup = std::fs::read(&self.log)?;
        if !startup.is_empty() {
            eprintln!(
                "perllsp initialization PATH observations, excluded from native journey proof: {}",
                String::from_utf8_lossy(&startup)
            );
        }
        Ok(startup)
    }

    fn assert_no_journey_calls(&self, startup: &[u8]) -> Result<()> {
        let complete = std::fs::read(&self.log)?;
        ensure!(
            complete == startup,
            "native formatting journey added PATH tool calls; startup={:?}, complete={:?}",
            String::from_utf8_lossy(startup),
            String::from_utf8_lossy(&complete),
        );
        Ok(())
    }
}
