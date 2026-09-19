//! Fact emitters for the `ripr-perl-facts-v1` packet (#9271 split of the
//! former single-file `emitter.rs`, itself relocated here behavior-preserving
//! from `perl-lsp-rs::ripr_facts_emitter` — see the crate root docs).
//!
//! Each submodule owns one packet fact family and exposes exactly one
//! `pub(crate) fn emit_*` entry point that `crate::packet::build_ripr_facts_packet`
//! calls; everything else here is `pub(crate)` only where a sibling submodule
//! needs it (never part of this crate's public API):
//!
//! - [`ids`] — shared id/digest recipes (`owner_fact_id`, `fnv1a_hash`, …).
//! - [`discovery`] — the `.t`/`.pm`/Perl-source filesystem walks every other
//!   submodule reuses.
//! - [`oracles`] — the assertion-call → `oracle.kind`/`strength` lookup table.
//! - [`test_facts`] — `tests[]` + `oracles[]` (Campaign 31 Phase B PR 6).
//! - [`relations`] — `relations[]` (`direct_owner_call` / `file_proximity`,
//!   Phase B PR 7).
//! - [`boundaries`] — `dynamic_boundaries[]` + `verify_commands[]` (fused;
//!   Phase B PR 8).
//! - [`changes`] — diff-owned `changes[]` (#3293 PR 5).
//! - [`owners`] — `files[]` + `owners[]` (fused; #3293 PR 3).
//!
//! `boundaries::emit_boundaries_and_commands` and `owners::emit_files_and_owners`
//! each emit two fact arrays from one shared file walk/parse; the issue's
//! target shape suggested finer per-array modules (`files.rs`/`owners.rs`,
//! `verify.rs`), but splitting those functions would restructure — not
//! move — working code, so they stay fused. See the #9271 PR notes.

mod boundaries;
mod changes;
mod discovery;
mod ids;
mod oracles;
mod owners;
mod relations;
mod test_facts;

pub(crate) use boundaries::emit_boundaries_and_commands;
pub(crate) use changes::emit_changes_from_diff;
pub(crate) use owners::emit_files_and_owners;
pub(crate) use relations::emit_relations_and_discriminators;
pub(crate) use test_facts::emit_tests_and_oracles;
