//! Test support utilities for LSP integration tests
// Test support module — eprintln!/println! are used for test diagnostics.
#![allow(clippy::print_stderr, clippy::print_stdout)]

pub mod bdd_diagnostics;
pub mod client_caps;
pub mod env_guard;
pub mod lsp_client;
pub mod lsp_harness;
pub mod lsp_ux_harness;
pub mod message_framing;
pub mod notification_queue;
pub mod test_helpers;
pub mod test_workspace;
pub mod ux_bdd;

/// Resolve the canonical public product binary, building it on demand for
/// implementation-crate process tests that Cargo does not associate with the
/// `perllsp` package's binary target.
// Shared support is compiled separately by tests that do not all spawn a process.
#[allow(dead_code)]
pub fn product_binary_path() -> Result<String, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("PERL_LSP_BIN") {
        return Ok(path);
    }

    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("perl-lsp-rs must live below the workspace root")?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| workspace.join("target"));
    let binary = target.join("debug").join(if cfg!(windows) { "perllsp.exe" } else { "perllsp" });
    if !binary.is_file() {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = std::process::Command::new(cargo)
            .current_dir(workspace)
            .args(["build", "-p", "perllsp", "--bin", "perllsp", "--locked"])
            .status()?;
        if !status.success() {
            return Err("building canonical perllsp test binary failed".into());
        }
    }
    if !binary.is_file() {
        return Err(format!("canonical perllsp test binary missing: {}", binary.display()).into());
    }
    Ok(binary.to_string_lossy().into_owned())
}

// Re-export test helpers for convenience in test files that use `support::*`
// NOTE: test_helpers module exists but may not be used in all test contexts
#[allow(unused_imports)]
pub use test_helpers::*;

// Re-export Phase 1 stabilization helpers for easy access
#[allow(unused_imports)]
pub use lsp_harness::{handshake_initialize, shutdown_graceful, spawn_lsp};

// Re-export types that tests may need
#[allow(unused_imports)]
pub use lsp_harness::{LspHarness, TempWorkspace, TestContext};
