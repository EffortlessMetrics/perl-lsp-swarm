#![warn(missing_docs)]
#![cfg_attr(clippy, allow(missing_docs))]

//! Batch exporter for the `ripr-perl-facts-v1` packet.
//!
//! This crate owns the `ripr-facts` unit that previously lived inside the LSP
//! server runtime (`perl-lsp-rs::ripr_facts_emitter` + the packet-assembly
//! helpers in `perl-lsp-rs::cli`). It was relocated here behavior-preserving so
//! that RIPR fact production sits **below** the editor/LSP stack and **above**
//! the raw parser — see `README.md` for the dependency contract.
//!
//! Three entry points:
//!
//! - [`build_ripr_facts_packet`] is the **structured batch API** (#3293 PR 2):
//!   it validates a [`RiprFactsRequest`], runs the (currently conservative,
//!   string-scan) emitter, and returns the assembled `ripr-perl-facts-v1` packet
//!   as a [`serde_json::Value`] — no I/O.
//! - [`run_ripr_facts`] is the thin CLI wrapper the `perl-lsp` / `perllsp`
//!   `ripr-facts` subcommand calls: it forwards CLI-shaped args to the batch
//!   API, then validates the output path, writes the packet to disk, and maps
//!   the outcome to a process exit code.
//! - [`run_cli`] is the standalone `perl-ripr-facts` binary entry point. It
//!   accepts RIPR's managed-producer command shape, including `--diff`.
//!
//! Later slices replace the remaining string scans with fuller parser- and
//! semantic-backed extraction (using leaf crates like `perl-parser-core` /
//! `perl-symbol` that carry no forbidden dependencies — not `perl-workspace`,
//! which pulls `lsp-types`); this crate is the home for that work.
//!
//! ## Module layout (#9271)
//!
//! This facade re-exports the public API from four internal modules split out
//! of the former single-file `lib.rs` + `emitter.rs`, structure-only (no
//! behavior or packet-byte change):
//!
//! - [`request`] (private) — [`RiprFactsRequest`], [`RiprFactsError`], and the
//!   request-field validation ([`build_ripr_facts_packet`] and [`cli`] share).
//! - [`packet`] (private) — [`build_ripr_facts_packet`] itself: runs the
//!   emitters, binds relations to changes, computes `packet_fingerprint`.
//! - [`cli`] (private) — [`run_cli`], [`run_ripr_facts`],
//!   [`run_ripr_facts_with_diff`]: argv parsing, output-path validation, and
//!   the process-exit-code mapping around [`build_ripr_facts_packet`].
//! - [`emitter`] (private) — the fact emitters themselves, one submodule per
//!   packet fact family; see its module docs for the full breakdown.
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

mod cli;
mod emitter;
mod packet;
mod request;

pub use cli::{run_cli, run_ripr_facts, run_ripr_facts_with_diff};
pub use packet::build_ripr_facts_packet;
pub use request::{RiprFactsError, RiprFactsRequest};
