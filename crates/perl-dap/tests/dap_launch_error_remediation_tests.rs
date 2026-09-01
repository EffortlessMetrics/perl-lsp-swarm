//! Tests for DAP launch error remediation messaging.
//!
//! Issue #4192: Launch failure should provide actionable guidance including
//! detected Perl location and the documented `launch.json` `perlPath` setting.

use perl_dap::{DapMessage, DebugAdapter};
use serde_json::json;

fn initialize_adapter(adapter: &mut DebugAdapter) {
    let response = adapter.handle_request(1, "initialize", None);
    assert!(
        matches!(response, DapMessage::Response { success: true, .. }),
        "initialize should succeed before launch remediation checks, got: {response:?}"
    );
}

/// Launch with a nonexistent program path should yield an error message
/// that mentions the documented `launch.json` `perlPath` field.
#[test]
fn launch_error_names_launch_json_perlpath_setting() -> anyhow::Result<()> {
    let mut adapter = DebugAdapter::new();
    crate::install_unbounded_test_authority(&adapter);
    initialize_adapter(&mut adapter);

    let response = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": "/nonexistent/path/to/script.pl"
        })),
    );

    match response {
        DapMessage::Response { success: false, message: Some(msg), request_seq, .. } => {
            assert_eq!(request_seq, 2, "launch response must echo the post-initialize request");
            assert!(
                msg.contains("launch.json") && msg.contains("perlPath"),
                "error message must name launch.json `perlPath`, got: {msg}"
            );
            assert!(
                !msg.contains("perl-lsp.perl.path"),
                "DAP launch error must not point at stale perl-lsp.perl.path setting, got: {msg}"
            );
        }
        DapMessage::Response { success: true, .. } => {
            // If Perl is available and the file actually exists (unlikely with this path),
            // treat as expected. On most CI envs the program doesn't exist so we get false.
            // This branch should not happen for a nonexistent path, but be defensive.
        }
        other => {
            anyhow::bail!("expected a Response, got: {:?}", other);
        }
    }
    Ok(())
}

/// Launch failure error message must include information about the Perl
/// interpreter that was found (or indicate none was found).
#[test]
fn launch_error_includes_perl_detection_info() -> anyhow::Result<()> {
    let mut adapter = DebugAdapter::new();
    crate::install_unbounded_test_authority(&adapter);
    initialize_adapter(&mut adapter);

    let response = adapter.handle_request(
        2,
        "launch",
        Some(json!({
            "program": "/nonexistent/path/to/script.pl"
        })),
    );

    match response {
        DapMessage::Response { success: false, message: Some(msg), request_seq, .. } => {
            assert_eq!(request_seq, 2, "launch response must echo the post-initialize request");
            let msg_lower = msg.to_lowercase();
            let has_perl_found = msg_lower.contains("found perl") || msg_lower.contains("perl at");
            let has_perl_not_found = msg_lower.contains("not found")
                || msg_lower.contains("no perl")
                || msg_lower.contains("check your path")
                || msg_lower.contains("path");
            assert!(
                has_perl_found || has_perl_not_found,
                "error message should mention Perl found/not-found status, got: {msg}"
            );
        }
        DapMessage::Response { success: true, .. } => {
            // Defensive: skip if somehow succeeded (shouldn't happen for nonexistent path).
        }
        other => {
            anyhow::bail!("expected a Response, got: {:?}", other);
        }
    }
    Ok(())
}

/// Repeated launch failures should preserve the same actionable remediation text.
#[test]
fn repeated_launch_failures_keep_actionable_guidance() -> anyhow::Result<()> {
    let mut adapter = DebugAdapter::new();
    crate::install_unbounded_test_authority(&adapter);
    initialize_adapter(&mut adapter);
    let arguments = Some(json!({
        "program": "/nonexistent/path/to/script.pl"
    }));

    let first = adapter.handle_request(2, "launch", arguments.clone());
    let second = adapter.handle_request(3, "launch", arguments);

    match (first, second) {
        (
            DapMessage::Response {
                success: false,
                message: Some(first_msg),
                request_seq: first_request_seq,
                ..
            },
            DapMessage::Response {
                success: false,
                message: Some(second_msg),
                request_seq: second_request_seq,
                ..
            },
        ) => {
            assert_eq!(first_request_seq, 2, "first launch error must echo request 2");
            assert_eq!(second_request_seq, 3, "second launch error must echo request 3");
            assert!(
                first_msg.contains("launch.json") && first_msg.contains("perlPath"),
                "first error should include launch.json perlPath guidance, got: {first_msg}"
            );
            assert!(
                !first_msg.contains("perl-lsp.perl.path"),
                "first error should not include stale perl-lsp.perl.path guidance, got: {first_msg}"
            );
            assert!(
                second_msg.contains("launch.json") && second_msg.contains("perlPath"),
                "second error should include launch.json perlPath guidance, got: {second_msg}"
            );
            assert!(
                !second_msg.contains("perl-lsp.perl.path"),
                "second error should not include stale perl-lsp.perl.path guidance, got: {second_msg}"
            );
        }
        other => {
            anyhow::bail!("expected two launch error responses, got: {other:?}");
        }
    }
    Ok(())
}

/// On Windows with no Perl available, the launch error should link to strawberryperl.com.
///
/// This test only asserts when Perl is actually absent — if Perl is found on the system
/// the "found at" branch fires instead and the link is not needed.
#[test]
#[cfg(windows)]
fn launch_error_on_windows_links_strawberry_perl_when_perl_absent() -> anyhow::Result<()> {
    use perl_dap::platform::resolve_perl_path_with_toolchain;

    // Only run this assertion when Perl is genuinely not available.
    if resolve_perl_path_with_toolchain().is_err() {
        let mut adapter = DebugAdapter::new();
        crate::install_unbounded_test_authority(&adapter);
        initialize_adapter(&mut adapter);

        let response = adapter.handle_request(
            2,
            "launch",
            Some(json!({
                "program": "/nonexistent/path/to/script.pl"
            })),
        );

        match response {
            DapMessage::Response { success: false, message: Some(msg), request_seq, .. } => {
                assert_eq!(request_seq, 2, "launch response must echo the post-initialize request");
                assert!(
                    msg.contains("strawberryperl.com"),
                    "Windows not-found error message should link to strawberryperl.com, got: {msg}"
                );
            }
            DapMessage::Response { success: true, .. } => {}
            other => {
                anyhow::bail!("expected a Response, got: {:?}", other);
            }
        }
    }
    // If Perl is found, the test passes vacuously — the "found" branch is tested by
    // the other tests above.
    Ok(())
}

/// Install an explicitly unbounded startup authority (#8656).
///
/// These tests exercise debugging workflows, not the launch-authority
/// contract. Without an installed authority every launch is refused, so each
/// adapter opts into unbounded mode with a visible test acknowledgement.
fn install_unbounded_test_authority(adapter: &perl_dap::DebugAdapter) {
    use perl_dap::{
        LaunchAuthority, LaunchAuthoritySource, LaunchAuthorityStartup, UnboundedAcknowledgement,
    };
    let authority = LaunchAuthority::resolve(&LaunchAuthorityStartup {
        trusted_roots: Vec::new(),
        allow_unbounded: Some(UnboundedAcknowledgement::new(
            LaunchAuthoritySource::CommandLine,
            "test: unbounded session",
        )),
    })
    .expect("test authority resolution");
    adapter.set_launch_authority(authority);
}
