use perl_dap::DapMessage;
use perl_dap::DebugAdapter;
use perl_dap::security::WorkspaceAuthority;
use perl_tdd_support::must_some;
use serde_json::json;
use std::fs;

fn initialize_adapter(adapter: &mut DebugAdapter) {
    let response = adapter.handle_request(1, "initialize", None);
    assert!(
        matches!(response, DapMessage::Response { success: true, .. }),
        "initialize should succeed before launch security checks, got: {response:?}"
    );
}

#[test]
fn test_launch_accepts_cwd_independently_of_script_location()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    // Create separate directories for the script and the working directory
    let scripts_dir = tempfile::tempdir()?;
    let work_dir = tempfile::tempdir()?;

    let script_path = scripts_dir.path().join("program.pl");
    fs::write(&script_path, "print 'hello';")?;

    // Launch with cwd set to a different directory than where the script is located
    // This should succeed - cwd is the execution directory, not the workspace boundary
    let args = json!({
        "program": must_some(script_path.to_str()),
        "cwd": must_some(work_dir.path().to_str()),
        "args": []
    });

    // Handle launch request (seq 2; initialize used seq 1)
    let response = adapter.handle_request(2, "launch", Some(args));

    // Verify response
    match response {
        DapMessage::Response { success, message, .. } => {
            // The launch should succeed because both script and cwd are valid paths
            if !success {
                // Some failures are ok (e.g., perl not found), but not workspace-related failures
                let msg = message.unwrap_or_default();
                assert!(
                    !msg.contains("outside your workspace") && !msg.contains("outside workspace"),
                    "Unexpected workspace rejection with valid paths: {}",
                    msg
                );
            }
        }
        _ => return Err("Expected Response message".into()),
    }
    Ok(())
}

#[test]
fn test_launch_allows_valid_path() -> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    // Create a temporary workspace directory
    let temp_dir = tempfile::tempdir()?;
    let workspace_root = temp_dir.path().to_path_buf();

    // Create a file *inside* the workspace
    let inside_script = workspace_root.join("good.pl");
    fs::write(&inside_script, "print 'good';")?;

    // Construct launch arguments
    let args = json!({
        "program": must_some(inside_script.to_str()),
        "cwd": must_some(workspace_root.to_str()),
        "args": []
    });

    // Handle launch request
    let response = adapter.handle_request(2, "launch", Some(args));

    // Verify response
    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(success, "Launch of in-workspace script must succeed, got: {:?}", message);
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

/// Regression test for the bypass where workspace_root = program.parent() made every
/// launch self-validating. With a real workspace root configured, a script in a
/// different directory must be rejected before Perl is spawned.
#[test]
fn test_configured_workspace_root_rejects_outside_script() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    // Script lives outside the workspace
    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // Establish the trusted root through the startup seam (as DapServer::new does)
    let mut adapter = DebugAdapter::with_workspace_authority(WorkspaceAuthority::from_startup(
        &[workspace_dir.path().to_path_buf()],
        false,
    )?);
    initialize_adapter(&mut adapter);

    let args = json!({
        "program": must_some(outside_script.to_str()),
        "args": []
    });

    let response = adapter.handle_request(2, "launch", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Launch of out-of-workspace script must be rejected");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("outside your workspace") || msg.contains("outside workspace"),
                "Expected workspace-boundary rejection, got: {msg}"
            );
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

/// A script inside the configured workspace root must be accepted by the boundary check.
#[test]
fn test_configured_workspace_root_accepts_inside_script() -> Result<(), Box<dyn std::error::Error>>
{
    let workspace_dir = tempfile::tempdir()?;
    let script = workspace_dir.path().join("test.pl");
    fs::write(&script, "print 'ok';")?;

    let mut adapter = DebugAdapter::with_workspace_authority(WorkspaceAuthority::from_startup(
        &[workspace_dir.path().to_path_buf()],
        false,
    )?);
    initialize_adapter(&mut adapter);

    let args = json!({
        "program": must_some(script.to_str()),
        "args": []
    });

    let response = adapter.handle_request(2, "launch", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(success, "Launch of in-workspace script must succeed, got: {:?}", message);
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

/// A `workspaceRoot` field in the launch args provides the boundary when no
/// server-configured root is present. Scripts outside the declared root are rejected.
#[test]
fn test_launch_workspace_root_field_rejects_outside_script()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // No startup authority — the boundary comes only from the launch field
    let args = json!({
        "program": must_some(outside_script.to_str()),
        "workspaceRoot": must_some(workspace_dir.path().to_str()),
        "args": []
    });

    let response = adapter.handle_request(2, "launch", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Launch of out-of-workspace script must be rejected");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("outside your workspace") || msg.contains("outside workspace"),
                "Expected workspace-boundary rejection, got: {msg}"
            );
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

/// A `workspaceRoot` launch field must not widen a server-configured root.
/// An attacker supplying a broad `workspaceRoot` in a launch request must not
/// bypass the tighter server-configured boundary.
#[test]
fn test_launch_workspace_root_field_cannot_widen_server_root()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // Configure a narrow trusted root
    let mut adapter = DebugAdapter::with_workspace_authority(WorkspaceAuthority::from_startup(
        &[workspace_dir.path().to_path_buf()],
        false,
    )?);
    initialize_adapter(&mut adapter);

    // Client attempts to widen the boundary by supplying the parent of both dirs
    let broad_root = outside_dir.path().to_str().unwrap_or("/");
    let args = json!({
        "program": must_some(outside_script.to_str()),
        "workspaceRoot": broad_root,
        "args": []
    });

    let response = adapter.handle_request(2, "launch", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(
                !success,
                "A client-supplied workspaceRoot must not widen the server-configured root"
            );
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("outside your workspace") || msg.contains("outside workspace"),
                "Expected workspace-boundary rejection, got: {msg}"
            );
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Session isolation (#14587)
//
// The adapter used to write each launch's effective root back over its single
// `workspace_root` field. These tests pin the separation between the trusted
// startup authority and the per-session boundary derived from it.
// ---------------------------------------------------------------------------

/// Assert a launch was not rejected for workspace-boundary reasons.
///
/// The launch may still fail for unrelated environment reasons (no `perl` on
/// PATH, for instance); only a boundary rejection falsifies these tests.
fn assert_workspace_accepted(response: &DapMessage, what: &str) {
    let DapMessage::Response { success, message, .. } = response else {
        unreachable!("handle_request always answers a request with a Response ({what})")
    };
    if !*success {
        let msg = message.clone().unwrap_or_default();
        assert!(
            !msg.contains("outside your workspace")
                && !msg.contains("outside workspace")
                && !msg.contains("cannot widen"),
            "{what} must not be rejected by the workspace boundary, got: {msg}"
        );
    }
}

/// A narrowing `workspaceRoot` must confine only the launch that sent it.
///
/// Before the split, launch 1's `workspaceRoot` (`<ws>/sub`) was written back
/// over the adapter's trusted root, so launch 2 — which sends no
/// `workspaceRoot` at all — was confined to `<ws>/sub` and its sibling script
/// was rejected. The trusted grant must survive the first launch intact.
#[test]
fn a_narrowing_launch_root_does_not_confine_the_next_session()
-> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = tempfile::tempdir()?;
    let inner_dir = workspace_dir.path().join("sub");
    fs::create_dir_all(&inner_dir)?;

    let inner_script = inner_dir.join("inner.pl");
    fs::write(&inner_script, "print 'inner';")?;
    let sibling_script = workspace_dir.path().join("sibling.pl");
    fs::write(&sibling_script, "print 'sibling';")?;

    let authority = WorkspaceAuthority::from_startup(&[workspace_dir.path().to_path_buf()], false)?;
    let mut adapter = DebugAdapter::with_workspace_authority(authority.clone());
    initialize_adapter(&mut adapter);

    // Session 1 narrows to <ws>/sub.
    let first = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": must_some(inner_script.to_str()),
            "workspaceRoot": must_some(inner_dir.to_str()),
            "args": []
        })),
    );
    assert_workspace_accepted(&first, "a script inside the narrowed root");

    // The trusted authority is unchanged by that narrowing.
    assert_eq!(
        adapter.workspace_authority(),
        &authority,
        "a launch argument must not rewrite the adapter's trusted authority"
    );

    // Session 2 sends no workspaceRoot, so it is governed by the trusted root
    // again — not by session 1's narrowing.
    let second = adapter.handle_request(
        3,
        "launch",
        Some(json!({
            "program": must_some(sibling_script.to_str()),
            "args": []
        })),
    );
    assert_workspace_accepted(
        &second,
        "a script under the trusted root after an earlier narrowed session",
    );
    Ok(())
}

/// A launch-supplied root must not become authority for a later session.
///
/// With no startup authority, launch 1's `workspaceRoot` used to be stored on
/// the adapter permanently, so launch 2 inherited a boundary established purely
/// by project-controlled launch data. Each session must resolve independently.
#[test]
fn an_unbounded_adapter_does_not_inherit_a_previous_launch_root()
-> Result<(), Box<dyn std::error::Error>> {
    let first_dir = tempfile::tempdir()?;
    let second_dir = tempfile::tempdir()?;

    let first_script = first_dir.path().join("first.pl");
    fs::write(&first_script, "print 'first';")?;
    let second_script = second_dir.path().join("second.pl");
    fs::write(&second_script, "print 'second';")?;

    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let first = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": must_some(first_script.to_str()),
            "workspaceRoot": must_some(first_dir.path().to_str()),
            "args": []
        })),
    );
    assert_workspace_accepted(&first, "a script inside its own launch-supplied root");

    // Session 2 is in a completely different directory and supplies no root.
    // The stale boundary from session 1 must not reject it.
    let second = adapter.handle_request(
        3,
        "launch",
        Some(json!({
            "program": must_some(second_script.to_str()),
            "args": []
        })),
    );
    assert_workspace_accepted(&second, "a later session with no launch-supplied root");
    Ok(())
}

/// A project-controlled field must not be able to grant unbounded launches.
///
/// Launch arguments are opened-project data. Only machine/user-owned adapter
/// startup configuration establishes authority, so a launch that asks for
/// unbounded access is simply not an authority input.
#[test]
fn a_launch_argument_cannot_grant_unbounded_access() -> Result<(), Box<dyn std::error::Error>> {
    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;
    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    let mut adapter = DebugAdapter::with_workspace_authority(WorkspaceAuthority::from_startup(
        &[workspace_dir.path().to_path_buf()],
        false,
    )?);
    initialize_adapter(&mut adapter);

    let response = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": must_some(outside_script.to_str()),
            "allowUnboundedWorkspace": true,
            "unbounded": true,
            "args": []
        })),
    );

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "a launch field must not unbind a workspace-bound adapter");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("outside your workspace") || msg.contains("outside workspace"),
                "Expected workspace-boundary rejection, got: {msg}"
            );
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}

/// Under multiple trusted roots, a launch root naming root B cannot smuggle in
/// a program that lives in root A.
///
/// `resolve_session_boundary` validates a launch root against the trust *set*
/// rather than against the program's owner, so this launch is admitted at that
/// gate and confined to root B — and the program, which is outside root B, is
/// then refused. Multi-root support is new in this change, so this asymmetry
/// gets an explicit end-to-end regression rather than being left to inference.
#[test]
fn a_launch_root_from_another_root_cannot_launch_an_outside_program()
-> Result<(), Box<dyn std::error::Error>> {
    let alpha_dir = tempfile::tempdir()?;
    let beta_dir = tempfile::tempdir()?;

    let alpha_script = alpha_dir.path().join("alpha.pl");
    fs::write(&alpha_script, "print 'alpha';")?;

    let mut adapter = DebugAdapter::with_workspace_authority(WorkspaceAuthority::from_startup(
        &[alpha_dir.path().to_path_buf(), beta_dir.path().to_path_buf()],
        false,
    )?);
    initialize_adapter(&mut adapter);

    let response = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": must_some(alpha_script.to_str()),
            "workspaceRoot": must_some(beta_dir.path().to_str()),
            "args": []
        })),
    );

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "a program outside the launch-selected root must not launch");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("outside your workspace") || msg.contains("outside workspace"),
                "Expected workspace-boundary rejection, got: {msg}"
            );
        }
        other => return Err(format!("Expected Response, got: {other:?}").into()),
    }
    Ok(())
}
