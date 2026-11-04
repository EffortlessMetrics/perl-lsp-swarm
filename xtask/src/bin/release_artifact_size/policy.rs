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

/// Receipt schema the instrument emits and that a confirming prior receipt
/// must carry. A different schema cannot confirm a borderline win.
pub(crate) const SCHEMA_VERSION: &str = "release_artifact_size.v1";

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

/// Production release workflow whose macOS rows may receive safe-ICF flags
/// only after the matching target earns `adopt`.
pub(crate) const RELEASE_WORKFLOW_PATH: &str = ".github/workflows/release.yml";

/// One controller disposition for a governed target (issue #16783).
///
/// `recommendation` is one of `adopt`, `do_not_adopt`, `reject`, `not_proven`.
/// Production linker flags may be applied only when it is `adopt`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct RecordedDisposition {
    pub target: &'static str,
    pub recommendation: &'static str,
    pub reason: &'static str,
}

/// Current #16783 dispositions. Both triples stay `not_proven` until a native
/// same-SHA receipt independently earns a different recommendation. This is
/// the production-binding authority: `release.yml` must not carry safe-ICF
/// flags for any target that is not `adopt` here.
pub(crate) const TARGET_DISPOSITIONS: [RecordedDisposition; 2] = [
    RecordedDisposition {
        target: "aarch64-apple-darwin",
        recommendation: "not_proven",
        reason: "no native same-SHA measurement has been dispatched",
    },
    RecordedDisposition {
        target: "x86_64-apple-darwin",
        recommendation: "not_proven",
        reason: "no native same-SHA measurement has been dispatched",
    },
];

/// Look up the recorded controller disposition for a target triple.
pub(crate) fn recorded_disposition(target: &str) -> Option<RecordedDisposition> {
    TARGET_DISPOSITIONS.into_iter().find(|row| row.target == target)
}
