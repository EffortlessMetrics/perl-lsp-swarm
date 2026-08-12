//! Focused public-surface coverage audit for the native `perl-dap` crate.
//!
//! Domain-specific behavior remains covered by the dedicated protocol,
//! breakpoint, variable, lifecycle, transport, security, and real-session test
//! targets. This file keeps a compact cross-section of public serde and native
//! server API contracts without encoding removed PLS bridge behavior.

use perl_dap::breakpoints::{BreakpointHitOutcome, BreakpointRecord, BreakpointStore};
use perl_dap::protocol::*;
use perl_dap::{
    AttachConfiguration, DapConfig, DapMessage, DapMode, DapServer, DapSocketBindError,
    DebugAdapter, LaunchConfiguration,
};
use serde_json::json;
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use tempfile::NamedTempFile;

// ── Protocol serde ─────────────────────────────────────────────────

#[test]
fn request_response_and_event_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request {
        seq: 1,
        msg_type: "request".to_string(),
        command: "initialize".to_string(),
        arguments: Some(json!({"clientId": "vscode", "adapterId": "perl-rs"})),
    };
    let request: Request = serde_json::from_str(&serde_json::to_string(&request)?)?;
    assert_eq!(request.seq, 1);
    assert_eq!(request.command, "initialize");
    assert!(request.arguments.is_some());

    let response = Response {
        seq: 2,
        msg_type: "response".to_string(),
        request_seq: 1,
        success: true,
        command: "initialize".to_string(),
        message: None,
        body: Some(json!({"supportsConfigurationDoneRequest": true})),
    };
    let response: Response = serde_json::from_str(&serde_json::to_string(&response)?)?;
    assert!(response.success);
    assert_eq!(response.request_seq, 1);
    assert!(response.body.is_some());

    let event = Event {
        seq: 3,
        msg_type: "event".to_string(),
        event: "stopped".to_string(),
        body: Some(json!({"reason": "breakpoint", "threadId": 1})),
    };
    let event: Event = serde_json::from_str(&serde_json::to_string(&event)?)?;
    assert_eq!(event.event, "stopped");
    assert!(event.body.is_some());
    Ok(())
}

#[test]
fn optional_protocol_fields_are_omitted() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request {
        seq: 1,
        msg_type: "request".to_string(),
        command: "configurationDone".to_string(),
        arguments: None,
    };
    assert!(!serde_json::to_string(&request)?.contains("arguments"));

    let event = Event {
        seq: 2,
        msg_type: "event".to_string(),
        event: "initialized".to_string(),
        body: None,
    };
    assert!(!serde_json::to_string(&event)?.contains("body"));
    Ok(())
}

#[test]
fn breakpoint_and_capabilities_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let source_breakpoint = SourceBreakpoint {
        line: 42,
        column: Some(5),
        condition: Some("$x > 10".to_string()),
        hit_condition: Some(">= 3".to_string()),
        log_message: Some("hit {$x}".to_string()),
    };
    let source_breakpoint: SourceBreakpoint =
        serde_json::from_str(&serde_json::to_string(&source_breakpoint)?)?;
    assert_eq!(source_breakpoint.line, 42);
    assert_eq!(source_breakpoint.condition.as_deref(), Some("$x > 10"));

    let capabilities = Capabilities {
        supports_configuration_done_request: Some(true),
        supports_evaluate_for_hovers: Some(true),
        supports_conditional_breakpoints: Some(true),
        supports_hit_conditional_breakpoints: Some(true),
        supports_log_points: Some(true),
        supports_exception_options: Some(true),
        supports_exception_filter_options: Some(true),
        supports_terminate_request: Some(true),
        supports_inline_values: Some(false),
        supports_function_breakpoints: Some(true),
        supports_set_variable: Some(true),
        supports_value_formatting_options: Some(false),
        support_terminate_debuggee: Some(true),
        supports_step_back: Some(false),
        supports_data_breakpoints: Some(false),
        exception_breakpoint_filters: Some(vec![ExceptionBreakpointFilter {
            filter: "die".to_string(),
            label: "Break on die/croak".to_string(),
            default: Some(false),
        }]),
    };
    let capabilities: Capabilities =
        serde_json::from_str(&serde_json::to_string(&capabilities)?)?;
    assert_eq!(capabilities.supports_configuration_done_request, Some(true));
    assert_eq!(capabilities.supports_data_breakpoints, Some(false));
    Ok(())
}

#[test]
fn stack_scope_variable_and_evaluate_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let stack = StackTraceResponseBody {
        stack_frames: vec![ProtocolStackFrame {
            id: 1,
            name: "main::run".to_string(),
            source: Some(Source {
                path: Some("/workspace/script.pl".to_string()),
                name: Some("script.pl".to_string()),
            }),
            line: 42,
            column: 1,
            end_line: None,
            end_column: None,
        }],
        total_frames: Some(1),
    };
    let stack: StackTraceResponseBody = serde_json::from_str(&serde_json::to_string(&stack)?)?;
    assert_eq!(stack.stack_frames.len(), 1);
    assert_eq!(stack.total_frames, Some(1));

    let scopes = ScopesResponseBody {
        scopes: vec![Scope {
            name: "Locals".to_string(),
            presentation_hint: Some("locals".to_string()),
            variables_reference: 11,
            expensive: false,
            named_variables: Some(1),
            indexed_variables: Some(0),
        }],
    };
    let scopes: ScopesResponseBody = serde_json::from_str(&serde_json::to_string(&scopes)?)?;
    assert_eq!(scopes.scopes[0].named_variables, Some(1));

    let variables = VariablesResponseBody {
        variables: vec![ProtocolVariable {
            name: "$x".to_string(),
            value: "42".to_string(),
            type_: Some("integer".to_string()),
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            evaluate_name: Some("$x".to_string()),
        }],
        total_variables: Some(1),
    };
    let variables: VariablesResponseBody =
        serde_json::from_str(&serde_json::to_string(&variables)?)?;
    assert_eq!(variables.variables[0].evaluate_name.as_deref(), Some("$x"));

    let evaluate = EvaluateResponseBody {
        result: "42".to_string(),
        type_: Some("integer".to_string()),
        variables_reference: 0,
    };
    let evaluate: EvaluateResponseBody =
        serde_json::from_str(&serde_json::to_string(&evaluate)?)?;
    assert_eq!(evaluate.result, "42");
    Ok(())
}

#[test]
fn representative_request_arguments_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let launch = LaunchRequestArguments {
        program: "/workspace/script.pl".to_string(),
        args: Some(vec!["--verbose".to_string()]),
        cwd: Some("/workspace".to_string()),
        env: Some(HashMap::from([("DEBUG".to_string(), "1".to_string())])),
        perl_path: Some("/usr/bin/perl".to_string()),
        stop_on_entry: Some(true),
    };
    let launch: LaunchRequestArguments = serde_json::from_str(&serde_json::to_string(&launch)?)?;
    assert_eq!(launch.program, "/workspace/script.pl");
    assert_eq!(launch.stop_on_entry, Some(true));

    let attach = AttachRequestArguments {
        process_id: Some(1234),
        host: Some("localhost".to_string()),
        port: Some(13603),
        timeout: Some(5000),
        stop_on_entry: None,
    };
    let attach: AttachRequestArguments = serde_json::from_str(&serde_json::to_string(&attach)?)?;
    assert_eq!(attach.process_id, Some(1234));
    assert_eq!(attach.port, Some(13603));

    let set_variable = SetVariableArguments {
        variables_reference: 11,
        name: "$x".to_string(),
        value: "43".to_string(),
    };
    let set_variable: SetVariableArguments =
        serde_json::from_str(&serde_json::to_string(&set_variable)?)?;
    assert_eq!(set_variable.name, "$x");

    let set_expression = SetExpressionArguments {
        expression: "$hash{key}".to_string(),
        value: "\"new value\"".to_string(),
        frame_id: Some(1),
    };
    let set_expression: SetExpressionArguments =
        serde_json::from_str(&serde_json::to_string(&set_expression)?)?;
    assert_eq!(set_expression.frame_id, Some(1));
    Ok(())
}

// ── DapMessage ─────────────────────────────────────────────────────

#[test]
fn dap_message_variants_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let messages = [
        DapMessage::Request {
            seq: 1,
            command: "initialize".to_string(),
            arguments: Some(json!({"adapterId": "perl-rs"})),
        },
        DapMessage::Response {
            seq: 2,
            request_seq: 1,
            success: true,
            command: "initialize".to_string(),
            body: Some(json!({"supportsConfigurationDoneRequest": true})),
            message: None,
        },
        DapMessage::Event {
            seq: 3,
            event: "initialized".to_string(),
            body: None,
        },
    ];

    for message in messages {
        let json = serde_json::to_string(&message)?;
        let decoded: DapMessage = serde_json::from_str(&json)?;
        assert_eq!(decoded.message_type(), message.message_type());
    }
    Ok(())
}

// ── Native server ──────────────────────────────────────────────────

#[test]
fn dap_mode_is_native_only() {
    let mode = DapMode::default();
    assert_eq!(mode, DapMode::Native);
    assert_eq!(mode.clone(), DapMode::Native);
    assert_eq!(format!("{mode:?}"), "Native");
}

#[test]
fn dap_server_creation_preserves_native_config() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from("/workspace");
    let config = DapConfig {
        log_level: "info".to_string(),
        mode: DapMode::Native,
        workspace_root: Some(root.clone()),
    };
    let server = DapServer::new(config)?;
    assert_eq!(server.config.mode, DapMode::Native);
    assert_eq!(server.config.log_level, "info");
    assert_eq!(server.config.workspace_root, Some(root));
    Ok(())
}

#[test]
fn dap_server_socket_preserves_bind_error_marker() -> Result<(), Box<dyn std::error::Error>> {
    let occupied = TcpListener::bind(("127.0.0.1", 0))?;
    let port = occupied.local_addr()?.port();
    let config =
        DapConfig { log_level: "info".to_string(), mode: DapMode::Native, workspace_root: None };
    let mut server = DapServer::new(config)?;

    let error = server.run_socket(port).expect_err("occupied port must fail before accept");
    let marker = error
        .downcast_ref::<DapSocketBindError>()
        .ok_or_else(|| io::Error::other("missing DAP bind marker"))?;
    assert_eq!(marker.port, port);
    let source = error
        .downcast_ref::<io::Error>()
        .ok_or_else(|| io::Error::other("missing underlying io error"))?;
    assert_eq!(source.kind(), io::ErrorKind::AddrInUse);
    Ok(())
}

#[test]
fn debug_adapter_constructs() {
    let _adapter = DebugAdapter::new();
}

// ── Breakpoints ────────────────────────────────────────────────────

fn create_test_perl_file() -> Result<(NamedTempFile, String), Box<dyn std::error::Error>> {
    let mut file = NamedTempFile::with_suffix(".pl")?;
    writeln!(file, "use strict;")?;
    writeln!(file, "my $x = 1;")?;
    writeln!(file, "print $x;")?;
    file.flush()?;
    let path = file.path().to_string_lossy().into_owned();
    Ok((file, path))
}

#[test]
fn breakpoint_record_projects_protocol_fields() {
    let record = BreakpointRecord {
        id: 42,
        line: 2,
        column: Some(1),
        condition: Some("$x > 0".to_string()),
        hit_condition: None,
        log_message: None,
        hit_count: 3,
        verified: true,
        message: Some("verified".to_string()),
    };
    let protocol = record.to_protocol();
    assert_eq!(protocol.id, 42);
    assert_eq!(protocol.line, 2);
    assert!(protocol.verified);
    assert_eq!(protocol.message.as_deref(), Some("verified"));
}

#[test]
fn breakpoint_hit_outcome_default_is_empty() {
    let outcome = BreakpointHitOutcome::default();
    assert!(!outcome.matched);
    assert!(!outcome.should_stop);
    assert!(outcome.log_messages.is_empty());
}

#[test]
fn breakpoint_store_replace_clear_and_invalid_input() -> Result<(), Box<dyn std::error::Error>> {
    let (_file, path) = create_test_perl_file()?;
    let store = BreakpointStore::new();
    assert!(store.is_empty());

    let valid = SetBreakpointsArguments {
        source: Source { path: Some(path.clone()), name: None },
        breakpoints: Some(vec![SourceBreakpoint {
            line: 2,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
        }]),
        source_modified: None,
    };
    let resolved = store.set_breakpoints(&valid);
    assert_eq!(resolved.len(), 1);
    assert!(!store.is_empty());

    let invalid = SetBreakpointsArguments {
        source: Source { path: Some(path.clone()), name: None },
        breakpoints: Some(vec![SourceBreakpoint {
            line: -1,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
        }]),
        source_modified: None,
    };
    let resolved = store.set_breakpoints(&invalid);
    assert_eq!(resolved.len(), 1);
    assert!(!resolved[0].verified);

    store.clear_all();
    assert!(store.is_empty());
    Ok(())
}

#[test]
fn breakpoint_condition_and_hit_condition_validation() -> Result<(), Box<dyn std::error::Error>> {
    let (_file, path) = create_test_perl_file()?;
    let store = BreakpointStore::new();

    for breakpoint in [
        SourceBreakpoint {
            line: 2,
            column: None,
            condition: Some("1\nprint 'injected'".to_string()),
            hit_condition: None,
            log_message: None,
        },
        SourceBreakpoint {
            line: 2,
            column: None,
            condition: None,
            hit_condition: Some("not_a_number".to_string()),
            log_message: None,
        },
    ] {
        let args = SetBreakpointsArguments {
            source: Source { path: Some(path.clone()), name: None },
            breakpoints: Some(vec![breakpoint]),
            source_modified: None,
        };
        let resolved = store.set_breakpoints(&args);
        assert_eq!(resolved.len(), 1);
        assert!(!resolved[0].verified);
    }
    Ok(())
}

// ── Inline values and catalog ──────────────────────────────────────

#[test]
fn inline_values_handle_empty_range_and_variables() {
    assert!(perl_dap::inline_values::collect_inline_values("", 1, 1).is_empty());
    assert!(
        perl_dap::inline_values::collect_inline_values("my $x = 1;\n", 100, 200).is_empty()
    );

    let values = perl_dap::inline_values::collect_inline_values("my $total = $x + $y;", 1, 1);
    assert!(values.iter().any(|value| value.text.contains("$total")));
    assert!(values.iter().all(|value| value.line >= 1 && value.column >= 1));
}

#[test]
fn feature_catalog_has_expected_native_entries() {
    for id in [
        "dap.core",
        "dap.breakpoints.basic",
        "dap.breakpoints.hit_condition",
        "dap.breakpoints.logpoints",
        "dap.completions",
        "dap.exceptions.die",
        "dap.exceptions.warn",
        "dap.inline_values",
        "dap.modules",
        "dap.watchpoints",
    ] {
        assert!(perl_dap::feature_catalog::has_feature(id), "missing feature {id}");
    }
    assert!(!perl_dap::feature_catalog::has_feature("nonexistent.feature.xyz"));
    assert!(!perl_dap::feature_catalog::advertised_features().is_empty());
}

// ── Launch / attach configuration ──────────────────────────────────

#[test]
fn launch_config_resolves_relative_program() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = LaunchConfiguration {
        program: PathBuf::from("script.pl"),
        args: vec![],
        cwd: None,
        env: HashMap::new(),
        perl_path: None,
        include_paths: vec![],
    };
    config.resolve_paths(&PathBuf::from("/workspace"))?;
    assert_eq!(config.program, PathBuf::from("/workspace/script.pl"));
    assert!(config.cwd.is_none());
    Ok(())
}

#[test]
fn attach_config_accepts_port_bounds_and_rejects_timeout_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    for port in [1, 65535] {
        AttachConfiguration {
            host: "localhost".to_string(),
            port,
            timeout_ms: Some(5000),
            stop_on_entry: None,
        }
        .validate()?;
    }

    AttachConfiguration {
        host: "localhost".to_string(),
        port: 13603,
        timeout_ms: Some(300_000),
        stop_on_entry: None,
    }
    .validate()?;

    let invalid = AttachConfiguration {
        host: "localhost".to_string(),
        port: 13603,
        timeout_ms: Some(300_001),
        stop_on_entry: None,
    };
    assert!(invalid.validate().is_err());
    Ok(())
}
