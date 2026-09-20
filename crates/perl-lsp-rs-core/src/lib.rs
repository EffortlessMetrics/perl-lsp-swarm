#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![deny(unsafe_code)]
#![cfg_attr(test, allow(clippy::print_stderr, clippy::print_stdout))]

/// Helpers for translating feature catalog entries into client capability checks.
pub mod capability_map;
/// Runtime configuration loading, validation, and compatibility adapters.
pub mod config;
/// Checked scope/precedence/validation authority consumed by configuration generations.
#[path = "configuration_authority/checked.rs"]
pub(crate) mod configuration_authority;
/// Crate-private, versioned configuration observation model (#10813); fixture
/// producers only until #10386 consumes it.
mod configuration_observation;
/// Parser for Perl::Critic output emitted by external lint runs.
pub mod critic_parser;
/// Registry-driven, native-first external-tool doctor projection.
pub mod external_tool_doctor;
/// Canonical policy roles and native replacements for external Perl tooling.
pub mod external_tools;
/// Feature catalog parsing and generation utilities shared by build/runtime code.
pub mod feature_catalog;
/// Feature model, identifiers, and registry plumbing for capability gating.
pub mod features;
/// Policy and governance APIs for feature profiles and rollout controls.
pub mod governance;
/// Hashing helpers shared by workspace tooling and verification pipelines.
pub mod hashing;
/// Performance-focused caches and allocation strategies for large workspaces.
pub mod performance;
/// Cross-platform interpreter and toolchain detection helpers.
pub mod platform;
/// Canonical runtime product, executable, build, and artifact identity packets.
pub mod product_identity;
/// JSON-RPC and LSP protocol types used across providers and transport layers.
pub mod protocol;
/// Language Server Protocol request/notification provider implementations.
pub mod providers;
/// Request lifecycle, scheduling, and runtime orchestration infrastructure.
pub mod runtime;
/// Ticket-owned fresh-full semantic construction cell (#12151).
pub mod semantic_construction;
/// Ticket-bound immutable file semantic snapshot envelope (#12150).
pub mod semantic_snapshot;
/// Integrations for external tools such as `perlcritic` and `perltidy`.
pub mod tooling;
/// Message framing and stream transport glue for stdio/socket communication.
pub mod transport;
/// URI parsing and conversion helpers used by protocol-facing components.
pub mod uri;
