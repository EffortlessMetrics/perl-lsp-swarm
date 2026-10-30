use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
pub struct SrpMicrocratesArgs {
    /// Optional output path (default: docs/SRP_MICROCRATES.md)
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct UnwiredScanArgs {
    /// Emit JSON to stdout instead of human-readable output
    #[arg(long)]
    pub json: bool,

    /// Exit 1 if any topology-flagged crates are found (Cargo dep scan, not product activation)
    #[arg(long)]
    pub check: bool,

    /// Name of the root LSP crate to check (default: perl-lsp-rs)
    #[arg(long, default_value = "perl-lsp-rs")]
    pub lsp_crate: String,
}

#[derive(Subcommand)]
pub enum SrpCommand {
    /// Generate SRP microcrate inventory and split-candidate report
    Microcrates(SrpMicrocratesArgs),

    /// Enforce crate layer-dependency constraints (leaf crates must not depend on higher layers).
    ///
    /// Current rules:
    ///   - perl-diagnostics must NOT depend on any perl-lsp-* crate.
    LayerCheck,

    /// Scan Cargo topology: crates with tests that are not a direct dep of perl-lsp-rs.
    ///
    /// This is not product activation. Class-contract checking is
    /// `cargo xtask activation check`. `--check` fails on topology flags only.
    UnwiredScan(UnwiredScanArgs),

    /// Check that test-bearing Rust files are reachable from their module tree.
    CheckTestWiring,
}
