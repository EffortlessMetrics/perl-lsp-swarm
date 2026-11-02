//! Regression tests for workspace-bound launch execution admission (#14601).

use perl_dap::DebugAdapter;
use perl_lsp_rs_core::config::{PerlToolchainProfile, WorkspaceConfig};
use perl_tdd_support::must_some;
use serde_json::json;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn initialize(adapter: &mut DebugAdapter) {
    let response = adapter.handle_request(1, "initialize", None);
    assert!(
        matches!(response, perl_dap::DapMessage::Response { success: true, .. }),
        "initialize must succeed, got: {response:?}"
    );
}

fn toolchain_perl() -> Option<std::path::PathBuf> {
    PerlToolchainProfile::resolve(&WorkspaceConfig::default())
        .map(|profile| profile.into_perl_binary())
}

#[test]
fn workspace_bound_refuses_unadmitted_interpreter_path() -> Result<(), Box<dyn std::error::Error>> {
    let Some(admitted) = toolchain_perl() else {
        eprintln!("skip: no toolchain perl on this runner");
        return Ok(());
    };

    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let script = workspace.path().join("ok.pl");
    fs::write(&script, "print 1;\n")?;

    let fake_perl = outside.path().join("perl");
    fs::write(&fake_perl, b"#!/bin/sh\nexit 0\n")?;
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&fake_perl)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_perl, perms)?;
    }

    let startup = perl_dap::LaunchAuthorityStartup {
        trusted_roots: vec![workspace.path().to_path_buf()],
        allow_unbounded: None,
        execution: perl_dap::LaunchExecutionStartup {
            admitted_interpreters: vec![admitted],
            allow_launch_code_loading_env: false,
        },
    };

    let mut adapter = DebugAdapter::new();
    initialize(&mut adapter);
    adapter.set_launch_authority(perl_dap::LaunchAuthority::resolve(&startup)?);

    let args = json!({
        "program": must_some(script.to_str()),
        "perlPath": must_some(fake_perl.to_str()),
    });
    let response = adapter.handle_request(2, "launch", Some(args));
    match response {
        perl_dap::DapMessage::Response { success, message, .. } => {
            assert!(!success, "unadmitted interpreter must be refused");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("not admitted") || msg.contains("Could not resolve"),
                "expected execution refusal, got: {msg}"
            );
        }
        other => return Err(format!("expected Response, got: {other:?}").into()),
    }
    Ok(())
}

#[test]
fn workspace_bound_refuses_launch_perl5lib_without_startup_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let Some(admitted) = toolchain_perl() else {
        eprintln!("skip: no toolchain perl on this runner");
        return Ok(());
    };

    let workspace = tempfile::tempdir()?;
    let script = workspace.path().join("ok.pl");
    fs::write(&script, "print 1;\n")?;

    let startup = perl_dap::LaunchAuthorityStartup {
        trusted_roots: vec![workspace.path().to_path_buf()],
        allow_unbounded: None,
        execution: perl_dap::LaunchExecutionStartup {
            admitted_interpreters: vec![admitted.clone()],
            allow_launch_code_loading_env: false,
        },
    };

    let mut adapter = DebugAdapter::new();
    initialize(&mut adapter);
    adapter.set_launch_authority(perl_dap::LaunchAuthority::resolve(&startup)?);

    let args = json!({
        "program": must_some(script.to_str()),
        "perlPath": admitted.to_string_lossy(),
        "env": {"PERL5LIB": "/outside/lib"},
    });
    let response = adapter.handle_request(2, "launch", Some(args));
    match response {
        perl_dap::DapMessage::Response { success, message, .. } => {
            assert!(!success, "PERL5LIB must be refused without startup admission");
            let msg = message.unwrap_or_default();
            assert!(msg.contains("PERL5LIB"), "expected env refusal, got: {msg}");
        }
        other => return Err(format!("expected Response, got: {other:?}").into()),
    }
    Ok(())
}
