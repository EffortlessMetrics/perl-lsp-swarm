//! Constants shared by the `release_artifact_size` measurement instrument and
//! by the contract proof for the read-only macOS shadow measurement lane.
//!
//! These values are the single authority for issue #5432's measurement subject.
//! The shadow workflow must reproduce them exactly: a lane that builds another
//! triple, or that passes link flags the instrument does not recognise, cannot
//! produce evidence the instrument will accept.
//!
//! Each consumer uses a subset: the instrument does not need the runner map or
//! the workflow path, and the contract proof does not need the repository name.
#![allow(dead_code)]

/// Repository the receipt is claimed for.
pub(crate) const REPOSITORY: &str = "EffortlessMetrics/perl-lsp-swarm";

/// The exact candidate link flags issue #5432 measures. `measure.rs` requires
/// the declared candidate flags to equal this string, so the shadow lane must
/// build the candidate with precisely these flags and nothing else.
pub(crate) const SAFE_ICF_RUSTFLAGS: &str =
    "-C linker=rust-lld -C linker-flavor=ld64.lld -C link-arg=--icf=safe";

/// The release binaries compared by a measurement.
pub(crate) const BINARY_NAMES: [&str; 2] = ["perllsp", "perl-dap"];

/// The exact native macOS target triples governed by issue #5432. Adoption is
/// restricted to these; no other triple may earn `adopt`.
pub(crate) const GOVERNED_TARGETS: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/// Native runner label for each governed target.
///
/// `measure.rs` rejects a measurement whose `rustc` host is not the measured
/// target, so each triple must be measured on its own native runner. These are
/// the same images `.github/workflows/release.yml` builds the macOS release
/// artifacts on.
pub(crate) const GOVERNED_TARGET_RUNNERS: [(&str, &str); 2] =
    [("aarch64-apple-darwin", "macos-14"), ("x86_64-apple-darwin", "macos-15-intel")];

/// Path of the read-only shadow measurement lane that produces #5432 evidence.
pub(crate) const SHADOW_WORKFLOW_PATH: &str = ".github/workflows/release-artifact-size-shadow.yml";
