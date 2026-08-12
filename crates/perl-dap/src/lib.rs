//! Native Debug Adapter Protocol implementation for Perl.
//!
//! `perl-dap` is the Rust DAP server shipped with `perl-lsp`. It speaks DAP over
//! stdio or TCP, launches or attaches to Perl debug sessions, validates
//! breakpoints with the native parser stack, and serves stack, variable,
//! evaluation, and execution-control requests to DAP-capable editors.
//!
//! # Product boundary
//!
//! The supported first-mile runtime is the native [`DapServer`] and
//! [`DebugAdapter`] path. A local Perl interpreter is required for the debuggee;
//! parser, lexer, protocol, and adapter support code are compiled into the
//! shipped binary.
//!
//! Historical `Perl::LanguageServer` bridge code is retained only as a
//! deprecated library compatibility and conformance surface. It is not exposed
//! by the shipped `perl-dap` CLI and is not required for native launch or attach.
//!
//! Optional external debugger peers, such as `Devel::ptkdb`, integrate through
//! the backend-neutral peer protocol while `perl-dap` remains the DAP server.
//! Those peers are not bundled or required for the native path.
//!
//! # Running the server
//!
//! Native stdio mode, used when an editor launches the adapter:
//!
//! ```text
//! perl-dap --stdio
//! ```
//!
//! Native TCP mode:
//!
//! ```text
//! perl-dap --socket --port 13603
//! ```
//!
//! # Programmatic launch
//!
//! ```no_run
//! use perl_dap::{DapConfig, DapMode, DapServer};
//!
//! # fn main() -> anyhow::Result<()> {
//! let config = DapConfig {
//!     log_level: "info".to_string(),
//!     mode: DapMode::Native,
//!     workspace_root: None,
//! };
//! let mut server = DapServer::new(config)?;
//! server.run()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Launch configuration
//!
//! ```no_run
//! use perl_dap::LaunchConfiguration;
//! use std::collections::HashMap;
//! use std::path::PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = LaunchConfiguration {
//!     program: PathBuf::from("script.pl"),
//!     args: vec!["--verbose".to_string()],
//!     cwd: Some(PathBuf::from("/workspace")),
//!     env: HashMap::new(),
//!     perl_path: None,
//!     include_paths: vec![PathBuf::from("lib")],
//! };
//! config.validate()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Attach configuration
//!
//! ```no_run
//! use perl_dap::AttachConfiguration;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let config = AttachConfiguration {
//!     host: "localhost".to_string(),
//!     port: 13603,
//!     timeout_ms: Some(5000),
//!     stop_on_entry: None,
//! };
//! config.validate()?;
//! # Ok(())
//! # }
//! ```
//!
//! # Native foundations
//!
//! - [`debug_adapter`] owns DAP request routing and session state.
//! - [`breakpoint`] and [`breakpoint_oracle`] provide parser-backed breakpoint
//!   truth.
//! - [`backend`] defines the backend-neutral execution seam.
//! - [`model`] carries canonical debugger facts across native and optional peer
//!   backends.
//! - [`protocol`] carries DAP wire types.
//! - [`platform`], [`shell`], and [`security`] own process, path, and admission
//!   boundaries.

#![warn(missing_docs)]
#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]

/// Deprecated compatibility bridge for historical `Perl::LanguageServer` integrations.
#[deprecated(
    note = "legacy Perl::LanguageServer compatibility; use the native DapServer/DebugAdapter path"
)]
pub mod bridge_adapter;
/// Launch and attach configuration structures for DAP debugging sessions.
pub mod configuration;
/// Debug Adapter Protocol (DAP) implementation for Perl debugging.
pub mod debug_adapter;
/// DAP feature catalog and capability gating helpers.
pub mod feature_catalog;

// Wave H collapsed modules (in DAG order — command_args before platform, platform before shell, value before variables)
/// Explicit public API re-exports from all collapsed satellite modules.
pub mod api;
/// AST-based breakpoint validation (from perl-dap-breakpoint).
pub mod breakpoint;
/// Platform-aware shell argument formatting (from perl-dap-command-args).
pub mod command_args;
/// DAP launch and attach configuration types (from perl-dap-config).
pub mod config;
/// Safe expression evaluation validation (from perl-dap-eval).
pub mod eval;
/// Cross-platform utilities for Perl path resolution and environment setup (from perl-dap-platform).
pub mod platform;
/// Security validation and hardening (from perl-dap-security).
pub mod security;
/// Shell-specific helpers for Perl DAP process launch (from perl-dap-shell).
pub mod shell;
/// Stack trace parsing and frame classification (from perl-dap-stack).
pub mod stack;
/// Shared DAP session model types (from perl-dap-types).
pub mod types;
/// Shared Perl value model for DAP parser and renderer (from perl-dap-value).
pub mod value;
/// Variable parsing and rendering for Perl DAP (from perl-dap-variables).
pub mod variables;

/// Backend abstraction (`DebugBackend`) and its implementations.
pub mod backend;
/// Breakpoint oracle: reusable breakpoint truth layer over the AST validator.
pub mod breakpoint_oracle;
/// Canonical, backend-neutral Perl debug model shared by all debug backends.
pub mod model;
/// The Perl Debugger Peer Protocol spoken to external engines (ptkdb-first).
pub mod peer_protocol;
/// `.ptkdbrc` bootstrap/fallback rendering for `Devel::ptkdb`.
pub mod ptkdb_bootstrap;
/// Frozen debug-session packet builder (the stable external handoff format).
pub mod session_plan;

/// Breakpoint storage and management for the DAP adapter.
pub mod breakpoints;
/// Inline value extraction for DAP `inlineValues` requests.
pub mod inline_values;
/// DAP protocol types following the JSON-RPC 2.0 message format.
pub mod protocol;
/// DAP server lifecycle, configuration, and operating mode.
pub mod server;
/// TCP-based attachment to running Perl debugger processes.
pub mod tcp_attach;

/// Type-safe variablesReference codec — retiring the #1219 ID/ref-space collision class.
pub mod var_ref {
    pub use crate::debug_adapter::var_ref::{ScopeKind, VariableReference, VariableReferenceError};
}

// Re-export codec types at crate root for ergonomic use in tests and consumer crates.
pub use debug_adapter::var_ref::{ScopeKind, VariableReference, VariableReferenceError};

/// Deprecated compatibility re-exports for historical PLS bridge consumers.
#[allow(deprecated)]
#[deprecated(
    note = "legacy Perl::LanguageServer compatibility; use the native DapServer/DebugAdapter path"
)]
pub use bridge_adapter::{BridgeAdapter, DapBridgeEnvConfig};
pub use configuration::{
    AttachConfiguration, LaunchConfiguration, create_attach_json_snippet,
    create_launch_json_snippet,
};
pub use debug_adapter::{DapMessage, DebugAdapter};
pub use server::{DapConfig, DapMode, DapServer, DapSocketBindError};

pub use breakpoints::{BreakpointRecord, BreakpointStore, interpolate_logpoint_message};
pub use protocol::{
    AttachRequestArguments, Breakpoint, BreakpointLocation, BreakpointLocationsArguments,
    BreakpointLocationsResponseBody, CancelArguments, Capabilities, CompletionItem,
    CompletionsArguments, CompletionsResponseBody, ContinueArguments, ContinueResponseBody,
    DataBreakpoint, DataBreakpointInfoArguments, DataBreakpointInfoResponseBody,
    DisconnectArguments, EvaluateArguments, EvaluateResponseBody, Event, ExceptionBreakpointFilter,
    ExceptionDetails, ExceptionFilterOption, ExceptionInfoArguments, ExceptionInfoResponseBody,
    FunctionBreakpoint, GotoArguments, GotoTarget, GotoTargetsArguments, GotoTargetsResponseBody,
    InitializeRequestArguments, LaunchRequestArguments, LoadedSourcesResponseBody, Module,
    ModulesArguments, ModulesResponseBody, NextArguments, PauseArguments, ProtocolStackFrame,
    ProtocolVariable, Request, Response, RestartArguments, RestartFrameArguments, Scope,
    ScopesArguments, ScopesResponseBody, SetBreakpointsArguments, SetBreakpointsResponseBody,
    SetDataBreakpointsArguments, SetDataBreakpointsResponseBody, SetExceptionBreakpointsArguments,
    SetExpressionArguments, SetExpressionResponseBody, SetFunctionBreakpointsArguments,
    SetVariableArguments, SetVariableResponseBody, Source, SourceArguments, SourceBreakpoint,
    SourceResponseBody, StackTraceArguments, StackTraceResponseBody, StepInArguments, StepInTarget,
    StepInTargetsArguments, StepInTargetsResponseBody, StepOutArguments, TerminateArguments,
    TerminateThreadsArguments, Thread, ThreadsResponseBody, VariablesArguments,
    VariablesResponseBody,
};