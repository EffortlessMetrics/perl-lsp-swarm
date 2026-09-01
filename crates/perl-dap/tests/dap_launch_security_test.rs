use perl_dap::DapMessage;
use perl_dap::DebugAdapter;
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

    // #8656: a launch is admitted only through explicit startup authority, so
    // this test trusts the temp workspace at startup.
    adapter.set_launch_authority(perl_dap::LaunchAuthority::resolve(
        &perl_dap::LaunchAuthorityStartup {
            trusted_roots: vec![workspace_root.clone()],
            allow_unbounded: None,
        },
    )?);

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
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    // Script lives outside the workspace
    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // Set the workspace root through the config seam (simulating DapServer::new wiring)
    adapter.set_workspace_root(workspace_dir.path().to_path_buf());

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
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let workspace_dir = tempfile::tempdir()?;
    let script = workspace_dir.path().join("test.pl");
    fs::write(&script, "print 'ok';")?;

    adapter.set_workspace_root(workspace_dir.path().to_path_buf());

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

/// A `workspaceRoot` field in the launch args can no longer create a boundary
/// when no server-configured root or startup authority is present (#8656):
/// the launch is refused outright instead of trusting launch-controlled roots.
#[test]
fn test_launch_workspace_root_field_rejects_outside_script()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // No set_workspace_root() call — root comes only from the launch field
    let args = json!({
        "program": must_some(outside_script.to_str()),
        "workspaceRoot": must_some(workspace_dir.path().to_str()),
        "args": []
    });

    let response = adapter.handle_request(2, "launch", Some(args));

    match response {
        DapMessage::Response { success, message, .. } => {
            assert!(!success, "Launch with a launch-args-only root must be rejected");
            let msg = message.unwrap_or_default();
            assert!(
                msg.contains("cannot create one"),
                "Expected launch-args-cannot-create-authority refusal, got: {msg}"
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
    let mut adapter = DebugAdapter::new();
    initialize_adapter(&mut adapter);

    let workspace_dir = tempfile::tempdir()?;
    let outside_dir = tempfile::tempdir()?;

    let outside_script = outside_dir.path().join("attack.pl");
    fs::write(&outside_script, "print 'bad';")?;

    // Configure a narrow server root
    adapter.set_workspace_root(workspace_dir.path().to_path_buf());

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
