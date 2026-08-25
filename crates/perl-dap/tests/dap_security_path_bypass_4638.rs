//! Regression tests for DAP security issue #4638: validate_source_path bypass
//! when workspace_root is None.
//!
//! Before the fix, the `None` arm of `validate_source_path` returned
//! `Ok(PathBuf::from(path))` with no validation, allowing arbitrary
//! path-traversal and absolute-path escapes during the pre-launch window.
//!
//! After the fix, the `None` arm rejects:
//! - Paths containing `Component::ParentDir` (`..`)
//!   while still allowing legitimate relative and absolute paths through with a
//!   warning (no workspace boundary is known, so absolute paths outside the CWD
//!   are warned but not hard-rejected — temp files and explicit user paths are
//!   legitimate pre-launch use cases).

use anyhow::Result;
use perl_dap::debug_adapter::{DapMessage, DebugAdapter};
use serde_json::json;

type TestResult = Result<()>;

/// Helper: send a gotoTargets request with the given source path and return
/// whether the response indicates success or failure.
fn goto_targets_path_result(
    adapter: &mut DebugAdapter,
    path: &str,
) -> Result<(bool, Option<String>)> {
    let args = json!({
        "source": { "path": path },
        "line": 1
    });
    let response = adapter.handle_request(1, "gotoTargets", Some(args));
    match response {
        DapMessage::Response { success, message, .. } => Ok((success, message)),
        other => anyhow::bail!("expected Response, got {other:?}"),
    }
}

/// Helper: send a breakpointLocations request with the given source path and
/// return whether the response indicates success or failure.
fn breakpoint_locations_path_result(
    adapter: &mut DebugAdapter,
    path: &str,
) -> Result<(bool, Option<String>)> {
    let args = json!({
        "source": { "path": path },
        "line": 1
    });
    let response = adapter.handle_request(1, "breakpointLocations", Some(args));
    match response {
        DapMessage::Response { success, message, .. } => Ok((success, message)),
        other => anyhow::bail!("expected Response, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Parent-directory traversal must be rejected (#4638)
// ---------------------------------------------------------------------------

#[test]
fn test_parent_traversal_rejected_without_workspace_goto_targets() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, _msg) = goto_targets_path_result(&mut adapter, "../../../etc/passwd")?;
    assert!(!success, "parent-directory traversal must be rejected without workspace root");
    Ok(())
}

#[test]
fn test_parent_traversal_rejected_without_workspace_breakpoint_locations() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, _msg) =
        breakpoint_locations_path_result(&mut adapter, "../../../../../../tmp/sensitive")?;
    assert!(!success, "parent-directory traversal must be rejected without workspace root");
    Ok(())
}

#[test]
fn test_mixed_traversal_rejected_without_workspace() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, _) = goto_targets_path_result(&mut adapter, "src/../../etc/shadow")?;
    assert!(
        !success,
        "paths with ParentDir components must be rejected even if they start with a normal component"
    );
    Ok(())
}

#[test]
fn test_rooted_traversal_rejected_without_workspace() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, _) = goto_targets_path_result(&mut adapter, "/../../../etc/passwd")?;
    assert!(!success, "rooted paths with ParentDir components must be rejected");
    Ok(())
}

// ---------------------------------------------------------------------------
// Absolute paths outside CWD are warned but allowed (#4638)
// ---------------------------------------------------------------------------
// When no workspace root is set, we cannot definitively reject absolute paths
// outside the CWD because temp files and explicit user paths are legitimate
// pre-launch use cases.  The ParentDir hard-rejection is the primary security
// fix; absolute paths get an elevated warning.

#[test]
fn test_absolute_path_outside_cwd_allowed_without_workspace() -> TestResult {
    // Absolute paths outside CWD should be allowed (with a warning) when no
    // workspace root is set — they are not traversal attacks.
    let mut adapter = DebugAdapter::new();

    #[cfg(windows)]
    let outside_path = "C:\\Windows\\System32\\config\\SAM";
    #[cfg(not(windows))]
    let outside_path = "/etc/shadow";

    let (success, _) = goto_targets_path_result(&mut adapter, outside_path)?;
    // May succeed or fail for other reasons, but must NOT be rejected by the
    // path-validation layer for being absolute.
    if !success {
        let (_, msg) = goto_targets_path_result(&mut adapter, outside_path)?;
        if let Some(m) = msg {
            let lower = m.to_lowercase();
            assert!(
                !lower.contains("traversal") && !lower.contains("path validation"),
                "absolute path without '..' should not be rejected by path validation: {m}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Legitimate paths must still be accepted (#4638 — no false positives)
// ---------------------------------------------------------------------------

#[test]
fn test_simple_relative_path_accepted_without_workspace() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, _) = goto_targets_path_result(&mut adapter, "src/main.pl")?;
    // May succeed or fail depending on breakpoint validation, but must NOT be
    // rejected by the path-validation layer itself.  If it fails, the failure
    // message should not mention "path validation" or "traversal".
    if !success {
        // If it fails, it should not be due to path validation
        let (_, msg) = goto_targets_path_result(&mut adapter, "src/main.pl")?;
        if let Some(m) = msg {
            let lower = m.to_lowercase();
            assert!(
                !lower.contains("traversal") && !lower.contains("path validation"),
                "relative path without '..' should not be rejected by path validation: {m}"
            );
        }
    }
    Ok(())
}

#[test]
fn test_dot_relative_path_accepted_without_workspace() -> TestResult {
    let mut adapter = DebugAdapter::new();
    // A path with a CurDir component (".") but no ParentDir should be accepted.
    let (success, _) = goto_targets_path_result(&mut adapter, "./lib/utils.pl")?;
    if !success {
        let (_, msg) = goto_targets_path_result(&mut adapter, "./lib/utils.pl")?;
        if let Some(m) = msg {
            let lower = m.to_lowercase();
            assert!(
                !lower.contains("traversal") && !lower.contains("path validation"),
                "relative path with only '.' should not be rejected by path validation: {m}"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Error messages must be informative (#4638)
// ---------------------------------------------------------------------------

#[test]
fn test_traversal_error_message_mentions_path_validation() -> TestResult {
    let mut adapter = DebugAdapter::new();
    let (success, msg) = goto_targets_path_result(&mut adapter, "../../../etc/passwd")?;
    assert!(!success, "traversal path must be rejected");
    let msg = msg.unwrap_or_default();
    assert!(
        msg.to_lowercase().contains("path validation")
            || msg.to_lowercase().contains("traversal")
            || msg.to_lowercase().contains("parent-directory"),
        "error message should explain the path validation failure: {msg}"
    );
    Ok(())
}
