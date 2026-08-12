//! Focused public-surface coverage audit for native `perl-dap`.
//!
//! Deep protocol, breakpoint, variable, lifecycle, transport, and security
//! behavior remains in the dedicated test targets. This file keeps a compact
//! cross-section of public serde and native server contracts.

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

#[test]
fn protocol_envelopes_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let request = Request {
        seq: 1,
        msg_type: "request".to_string(),
        command: "initialize".to_string(),
        arguments: Some(json!({"clientId": "vscode", "adapterId": "perl-rs"})),
    };
    let request_json = serde_json::to_string(&request)?;
    let request: Request = serde_json::from_str(&request_json)?;
    assert_eq!(request.command, "initialize");

    let response = Response {
        seq: 2,
        msg_type: "response".to_string(),
        request_seq: 1,
        success: true,
        command: "initialize".to_string(),
        message: None,
        body: Some(json!({"supportsConfigurationDoneRequest": true})),
    };
    let response_json = serde_json::to_string(&response)?;
    let response: Response = serde_json::from_str(&response_json)?;
    assert!(response.success);
    assert_eq!(response.request_seq, 1);

    let event = Event {
        seq: 3,
        msg_type: "event".to_string(),
        event: "stopped".to_string(),
        body: Some(json!({"reason": "breakpoint", "threadId": 1})),
    };
    let event_json = serde_json::to_string(&event)?;
    let event: Event = serde_json::from_str(&event_json)?;
    assert_eq!(event.event, "stopped");
    Ok(())
}

#[test]
fn optional_envelope_fields_are_omitted() -> Result<(), Box<dyn std::error::Error>> {
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
fn core_protocol_payloads_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let breakpoint = SourceBreakpoint {
        line: 42,
        column: Some(5),
        condition: Some("$x > 10".to_string()),
        hit_condition: Some(">= 3".to_string()),
        log_message: Some("hit {$x}".to_string()),
    };
    let breakpoint: SourceBreakpoint =
        serde_json::from_str(&serde_json::to_string(&breakpoint)?)?;
    assert_eq!(breakpoint.line, 42);

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
    assert_eq!(stack.total_frames, Some(1));

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
            body: None,
            message: None,
        },
        DapMessage::Event { seq: 3, event: "initialized".to_string(), body: None },
    ];

    for message in messages {
        let encoded = serde_json::to_string(&message)?;
        let decoded: DapMessage = serde_json::from_str(&encoded)?;
        assert_eq!(std::mem::discriminant(&decoded), std::mem::discriminant(&message));
    }
    Ok(())
}

#[test]
fn dap_mode_and_server_are_native_only() -> Result<(), Box<dyn std::error::Error>> {
    let mode = DapMode::default();
    assert_eq!(mode, DapMode::Native);
    assert_eq!(format!("{mode:?}"), "Native");

    let root = PathBuf::from("/workspace");
    let config = DapConfig {
        log_level: "info".to_string(),
        mode: DapMode::Native,
        workspace_root: Some(root.clone()),
    };
    let server = DapServer::new(config)?;
    assert_eq!(server.config.mode, DapMode::Native);
    assert_eq!(server.config.workspace_root, Some(root));
    Ok(())
}

#[test]
fn native_socket_preserves_bind_error_identity() -> Result<(), Box<dyn std::error::Error>> {
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
fn breakpoint_store_and_projection_smoke() -> Result<(), Box<dyn std::error::Error>> {
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
    assert!(protocol.verified);

    let outcome = BreakpointHitOutcome::default();
    assert!(!outcome.matched);
    assert!(!outcome.should_stop);

    let (_file, path) = create_test_perl_file()?;
    let store = BreakpointStore::new();
    let args = SetBreakpointsArguments {
        source: Source { path: Some(path), name: None },
        breakpoints: Some(vec![SourceBreakpoint {
            line: 2,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
        }]),
        source_modified: None,
    };
    assert_eq!(store.set_breakpoints(&args).len(), 1);
    assert!(!store.is_empty());
    store.clear_all();
    assert!(store.is_empty());
    Ok(())
}

#[test]
fn inline_values_and_feature_catalog_smoke() {
    assert!(perl_dap::inline_values::collect_inline_values("", 1, 1).is_empty());
    let values = perl_dap::inline_values::collect_inline_values("my $total = $x + $y;", 1, 1);
    assert!(values.iter().any(|value| value.text.contains("$total")));

    for id in ["dap.core", "dap.breakpoints.basic", "dap.completions", "dap.modules"] {
        assert!(perl_dap::feature_catalog::has_feature(id), "missing feature {id}");
    }
    assert!(!perl_dap::feature_catalog::has_feature("nonexistent.feature.xyz"));
}

#[test]
fn launch_and_attach_configuration_bounds() -> Result<(), Box<dyn std::error::Error>> {
    let mut launch = LaunchConfiguration {
        program: PathBuf::from("script.pl"),
        args: vec![],
        cwd: None,
        env: HashMap::new(),
        perl_path: None,
        include_paths: vec![],
    };
    launch.resolve_paths(&PathBuf::from("/workspace"))?;
    assert_eq!(launch.program, PathBuf::from("/workspace/script.pl"));

    for port in [1, 65535] {
        AttachConfiguration {
            host: "localhost".to_string(),
            port,
            timeout_ms: Some(5000),
            stop_on_entry: None,
        }
        .validate()?;
    }

    let invalid = AttachConfiguration {
        host: "localhost".to_string(),
        port: 13603,
        timeout_ms: Some(300_001),
        stop_on_entry: None,
    };
    assert!(invalid.validate().is_err());
    Ok(())
}
