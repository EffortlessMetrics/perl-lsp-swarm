//! Executable case discovery for the UX train (`ux_case_inventory.v1`).
//!
//! This module answers exactly one question:
//!
//! ```text
//! under this exact Cargo/toolchain/feature profile,
//! which test executables exist,
//! which exact libtest cases do they contain,
//! and what stable identity belongs to each case?
//! ```
//!
//! It deliberately does **not** decide which cases should run, what their
//! semantic role is, whether a failure is quarantined, or whether the UX gate
//! passes. Those are the operational-policy, execution, and run-verdict
//! authorities that consume this inventory (see `#9879`).
//!
//! # Why a trait instead of direct process execution
//!
//! Every impure step — compiling test targets, asking an executable for its
//! case list, digesting a file — goes through [`UxDiscoveryCommands`]. The
//! discovery algorithm itself never touches the filesystem, never enumerates
//! Rust source files, and never guesses. That makes every negative control in
//! this module a plain deterministic unit test over injected command output,
//! and makes "source scanning substituted for executable discovery"
//! structurally impossible rather than merely discouraged.
//!
//! # Identity
//!
//! Case identity is [`UxCaseId`]: an escaped `package::kind::target::test`
//! tuple. Numeric `ux_scenario_NN` filename prefixes are display metadata
//! only — they collide in the current suite (three `ux_scenario_18_*` targets,
//! four `ux_scenario_19_*`) and can never be identity.

use crate::taxonomy::UxCiTier;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

// ── Constants ────────────────────────────────────────────────────────────

/// Schema identifier emitted in every inventory document.
pub const UX_CASE_INVENTORY_SCHEMA: &str = "ux_case_inventory.v1";

/// Producer identifier emitted in every inventory document.
pub const UX_CASE_INVENTORY_PRODUCER: &str = "perl-lsp-ux-tests::case_inventory";

/// Schema identifier for the tombstone that replaces a stale inventory while
/// a refresh is in flight or after one has failed.
///
/// A consumer that reads the canonical path must treat any document whose
/// `schema` is not [`UX_CASE_INVENTORY_SCHEMA`] as "no current inventory".
pub const UX_CASE_INVENTORY_INVALID_SCHEMA: &str = "ux_case_inventory_invalid.v1";

/// The only Cargo package whose test executables may enter the denominator.
pub const UX_INVENTORY_PACKAGE: &str = "perl-lsp-ux-tests";

/// Features selected by the `pr` operational profile.
///
/// Declared independently of every other profile: a profile is an explicit
/// feature population, never an inherited one.
pub const PR_PROFILE_FEATURES: &[&str] = &[];

/// Features selected by the `nightly` operational profile.
pub const NIGHTLY_PROFILE_FEATURES: &[&str] = &["integration-test"];

/// Features selected by the `release` operational profile.
///
/// Spelled out rather than derived from [`NIGHTLY_PROFILE_FEATURES`] so that
/// widening nightly can never silently widen release.
pub const RELEASE_PROFILE_FEATURES: &[&str] = &["integration-test"];

/// Exact feature population selected by one operational profile.
#[must_use]
pub fn profile_features(tier: UxCiTier) -> &'static [&'static str] {
    match tier {
        UxCiTier::Pr => PR_PROFILE_FEATURES,
        UxCiTier::Nightly => NIGHTLY_PROFILE_FEATURES,
        UxCiTier::Release => RELEASE_PROFILE_FEATURES,
    }
}

/// Stable lowercase name of one operational profile.
#[must_use]
pub fn profile_name(tier: UxCiTier) -> &'static str {
    match tier {
        UxCiTier::Pr => "pr",
        UxCiTier::Nightly => "nightly",
        UxCiTier::Release => "release",
    }
}

/// Parse an operational profile name, rejecting anything unknown.
///
/// # Errors
///
/// Returns [`UxDiscoveryFailure::UnknownProfile`] for any name outside the
/// declared `pr` / `nightly` / `release` set.
pub fn parse_profile(name: &str) -> Result<UxCiTier, UxDiscoveryFailure> {
    match name {
        "pr" => Ok(UxCiTier::Pr),
        "nightly" => Ok(UxCiTier::Nightly),
        "release" => Ok(UxCiTier::Release),
        other => Err(UxDiscoveryFailure::UnknownProfile { name: other.to_string() }),
    }
}

// ── Failure semantics ────────────────────────────────────────────────────

/// Every distinct way discovery can fail to produce a complete inventory.
///
/// These are kept separate on purpose: a missing executable, a malformed case
/// list, and a stale wrong-profile artifact are different facts, and none of
/// them may be rendered as "zero cases".
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UxDiscoveryFailure {
    /// A profile name outside the declared set was requested.
    UnknownProfile {
        /// The rejected profile name.
        name: String,
    },
    /// The Cargo compile/metadata invocation itself failed.
    CargoInvocationFailed {
        /// Exact argv that was invoked.
        argv: Vec<String>,
        /// Process exit status, when one was observed.
        status: Option<i32>,
        /// Bounded excerpt of the failing output.
        detail: String,
    },
    /// A line of Cargo JSON output could not be understood.
    MalformedCargoMessage {
        /// 1-based line number within the captured stdout.
        line_number: usize,
        /// Why the line was rejected.
        reason: String,
    },
    /// Cargo reported no test artifacts at all for the package.
    NoTestArtifacts {
        /// The package that was expected to produce test artifacts.
        package: String,
    },
    /// Cargo named a test executable that is not present on disk.
    TestArtifactMissing {
        /// Target identity whose executable is missing.
        target: String,
        /// Path Cargo reported.
        path: String,
    },
    /// Two messages for one target and executable disagree on their metadata.
    ContradictoryArtifact {
        /// Target identity with contradictory messages.
        target: String,
        /// What disagreed.
        detail: String,
    },
    /// Two different executables were reported for the same target.
    DuplicateArtifact {
        /// Target identity with conflicting artifacts.
        target: String,
        /// First executable path seen.
        first: String,
        /// Second, conflicting executable path.
        second: String,
    },
    /// An artifact's resolved features do not match the requested profile.
    WrongProfileArtifact {
        /// Target identity of the stale artifact.
        target: String,
        /// Feature population the profile selected.
        expected_features: Vec<String>,
        /// Feature population Cargo actually reported.
        actual_features: Vec<String>,
    },
    /// Invoking a test executable's list mode failed.
    ListCommandFailed {
        /// Target identity whose list mode failed.
        target: String,
        /// Exact argv that was invoked.
        argv: Vec<String>,
        /// Process exit status, when one was observed.
        status: Option<i32>,
        /// Bounded excerpt of the failing output.
        detail: String,
    },
    /// A line of libtest list output did not match the terse format.
    MalformedListOutput {
        /// Target identity whose list output was rejected.
        target: String,
        /// 1-based line number within the captured output.
        line_number: usize,
        /// The rejected line.
        line: String,
    },
    /// The libtest trailing `N tests, M benchmarks` summary was absent.
    ///
    /// This is the control that stops a different runner's output — or a
    /// truncated capture — from being read as a legitimate empty target.
    MissingListSummary {
        /// Target identity whose list output had no summary.
        target: String,
    },
    /// The libtest summary counts disagree with the parsed listing, per kind.
    ListCountMismatch {
        /// Target identity whose counts disagree.
        target: String,
        /// Test count libtest declared in its summary.
        declared_tests: usize,
        /// Test count actually parsed from the listing.
        parsed_tests: usize,
        /// Benchmark count libtest declared in its summary.
        declared_benchmarks: usize,
        /// Benchmark count actually parsed from the listing.
        parsed_benchmarks: usize,
    },
    /// The executable changed on disk between being digested and being listed.
    ///
    /// A concurrent build can replace a test binary mid-discovery, which would
    /// otherwise record executable B's cases under executable A's digest.
    ExecutableChangedDuringDiscovery {
        /// Target identity whose executable moved.
        target: String,
        /// Digest observed before listing.
        before: String,
        /// Digest observed after listing.
        after: String,
    },
    /// Two executable cases normalized to one case identity.
    DuplicateCaseId {
        /// The colliding identity.
        case_id: String,
        /// First target claiming it.
        first_target: String,
        /// Second target claiming it.
        second_target: String,
    },
    /// A test executable's content digest could not be computed.
    DigestUnavailable {
        /// Target identity whose digest failed.
        target: String,
        /// Why the digest could not be produced.
        reason: String,
    },
    /// The discovery instrument itself failed in a way that is not a subject fact.
    InstrumentFailure {
        /// Why the instrument failed.
        reason: String,
    },
}

impl UxDiscoveryFailure {
    /// Stable snake_case discriminator, safe to record in downstream receipts.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownProfile { .. } => "unknown_profile",
            Self::CargoInvocationFailed { .. } => "cargo_invocation_failed",
            Self::MalformedCargoMessage { .. } => "malformed_cargo_message",
            Self::NoTestArtifacts { .. } => "no_test_artifacts",
            Self::TestArtifactMissing { .. } => "test_artifact_missing",
            Self::ContradictoryArtifact { .. } => "contradictory_artifact",
            Self::DuplicateArtifact { .. } => "duplicate_artifact",
            Self::WrongProfileArtifact { .. } => "wrong_profile_artifact",
            Self::ListCommandFailed { .. } => "list_command_failed",
            Self::MalformedListOutput { .. } => "malformed_list_output",
            Self::MissingListSummary { .. } => "missing_list_summary",
            Self::ListCountMismatch { .. } => "list_count_mismatch",
            Self::ExecutableChangedDuringDiscovery { .. } => "executable_changed_during_discovery",
            Self::DuplicateCaseId { .. } => "duplicate_case_id",
            Self::DigestUnavailable { .. } => "digest_unavailable",
            Self::InstrumentFailure { .. } => "instrument_failure",
        }
    }
}

impl fmt::Display for UxDiscoveryFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProfile { name } => {
                write!(f, "unknown discovery profile `{name}` (expected pr, nightly, or release)")
            }
            Self::CargoInvocationFailed { argv, status, detail } => write!(
                f,
                "cargo invocation failed (status {status:?}): {} — {detail}",
                argv.join(" ")
            ),
            Self::MalformedCargoMessage { line_number, reason } => {
                write!(f, "malformed cargo JSON on line {line_number}: {reason}")
            }
            Self::NoTestArtifacts { package } => {
                write!(f, "cargo reported no test artifacts for package `{package}`")
            }
            Self::TestArtifactMissing { target, path } => {
                write!(f, "test executable for `{target}` is missing at `{path}`")
            }
            Self::ContradictoryArtifact { target, detail } => {
                write!(f, "contradictory cargo messages for `{target}`: {detail}")
            }
            Self::DuplicateArtifact { target, first, second } => write!(
                f,
                "target `{target}` reported two different executables: `{first}` and `{second}`"
            ),
            Self::WrongProfileArtifact { target, expected_features, actual_features } => write!(
                f,
                "target `{target}` was built with features {actual_features:?} but the profile selects {expected_features:?}"
            ),
            Self::ListCommandFailed { target, argv, status, detail } => write!(
                f,
                "listing cases for `{target}` failed (status {status:?}): {} — {detail}",
                argv.join(" ")
            ),
            Self::MalformedListOutput { target, line_number, line } => write!(
                f,
                "malformed libtest list output for `{target}` on line {line_number}: `{line}`"
            ),
            Self::MissingListSummary { target } => write!(
                f,
                "libtest list output for `{target}` has no `N tests, M benchmarks` summary"
            ),
            Self::ListCountMismatch {
                target,
                declared_tests,
                parsed_tests,
                declared_benchmarks,
                parsed_benchmarks,
            } => write!(
                f,
                "libtest declared {declared_tests} tests and {declared_benchmarks} benchmarks for `{target}` but {parsed_tests} tests and {parsed_benchmarks} benchmarks were parsed"
            ),
            Self::ExecutableChangedDuringDiscovery { target, before, after } => write!(
                f,
                "the executable for `{target}` changed during discovery: {before} before listing, {after} after"
            ),
            Self::DuplicateCaseId { case_id, first_target, second_target } => write!(
                f,
                "case id `{case_id}` is claimed by both `{first_target}` and `{second_target}`"
            ),
            Self::DigestUnavailable { target, reason } => {
                write!(f, "could not digest the executable for `{target}`: {reason}")
            }
            Self::InstrumentFailure { reason } => {
                write!(f, "discovery instrument failed: {reason}")
            }
        }
    }
}

impl std::error::Error for UxDiscoveryFailure {}

// ── Case identity ────────────────────────────────────────────────────────

/// Stable identity for one executable libtest case.
///
/// Encoded as four percent-escaped components joined by `::`:
///
/// ```text
/// <package>::<target_kind>::<target_name>::<test_name>
/// ```
///
/// Escaping `%` and `:` inside each component makes the join injective, so two
/// distinct cases can never normalize to one identity — including the same
/// test name in two different targets.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UxCaseId {
    encoded: String,
}

fn escape_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3A"),
            other => out.push(other),
        }
    }
    out
}

fn unescape_component(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let bytes: Vec<char> = raw.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == '%' {
            let hex: String = bytes.get(index + 1..index + 3)?.iter().collect();
            match hex.as_str() {
                "25" => out.push('%'),
                "3A" => out.push(':'),
                _ => return None,
            }
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Some(out)
}

impl UxCaseId {
    /// Build a case identity from its four components.
    #[must_use]
    pub fn new(package: &str, target_kind: &str, target_name: &str, test_name: &str) -> Self {
        let encoded = [package, target_kind, target_name, test_name]
            .iter()
            .map(|component| escape_component(component))
            .collect::<Vec<_>>()
            .join("::");
        Self { encoded }
    }

    /// Borrow the encoded identity string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.encoded
    }

    /// Recover the four components from an encoded identity.
    ///
    /// Returns `None` when the encoding is not a well-formed four-component
    /// identity. Round-tripping is what proves the encoding is injective.
    #[must_use]
    pub fn components(&self) -> Option<[String; 4]> {
        let parts: Vec<&str> = self.encoded.split("::").collect();
        let [package, kind, target, test]: [&str; 4] = parts.try_into().ok()?;
        Some([
            unescape_component(package)?,
            unescape_component(kind)?,
            unescape_component(target)?,
            unescape_component(test)?,
        ])
    }
}

impl fmt::Display for UxCaseId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encoded)
    }
}

impl Serialize for UxCaseId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.encoded)
    }
}

impl<'de> Deserialize<'de> for UxCaseId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        let candidate = Self { encoded };
        // A typed identity that cannot be decomposed is not an identity. B02
        // consumes these; rejecting here keeps a malformed id from travelling
        // any further than the document it arrived in.
        if candidate.components().is_none() {
            return Err(serde::de::Error::custom(format!(
                "`{}` is not a well-formed ux case id (expected four escaped `::`-joined components)",
                candidate.encoded
            )));
        }
        Ok(candidate)
    }
}

/// Cargo target kind that produced a test executable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxTargetKind {
    /// Unit/contract tests compiled into the package library target.
    Lib,
    /// An integration test target under `tests/`.
    Test,
    /// A binary target compiled with `--test`.
    Bin,
    /// A benchmark target.
    Bench,
    /// An example target compiled with `--test`.
    Example,
    /// Any other Cargo target kind, retained verbatim.
    Other(String),
}

impl UxTargetKind {
    /// Stable string form used inside case and target identities.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Lib => "lib",
            Self::Test => "test",
            Self::Bin => "bin",
            Self::Bench => "bench",
            Self::Example => "example",
            Self::Other(raw) => raw.as_str(),
        }
    }

    fn from_cargo_kind(raw: &str) -> Self {
        match raw {
            "lib" | "rlib" | "dylib" | "cdylib" | "staticlib" | "proc-macro" => Self::Lib,
            "test" => Self::Test,
            "bin" => Self::Bin,
            "bench" => Self::Bench,
            "example" => Self::Example,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Case kind reported by libtest's terse listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxCaseKind {
    /// An ordinary `#[test]` case.
    Test,
    /// A `#[bench]` case.
    Benchmark,
}

// ── Cargo artifact parsing ───────────────────────────────────────────────

/// One test executable reported by `cargo test --no-run --message-format=json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTestArtifact {
    /// Cargo package id exactly as reported. Machine-local: it embeds the
    /// absolute checkout path, so it never reaches the durable projection.
    pub package_id: String,
    /// Durable name, version, and source role parsed from the package id.
    pub package_identity: CargoPackageIdentity,
    /// Package name extracted from the package id.
    pub package_name: String,
    /// Cargo target name.
    pub target_name: String,
    /// Cargo target kind.
    pub target_kind: UxTargetKind,
    /// Basename of the target's source path (display metadata only).
    pub source_file: Option<String>,
    /// Absolute path to the compiled test executable.
    pub executable: PathBuf,
    /// Resolved feature population reported for the artifact, sorted.
    pub features: Vec<String>,
}

impl CargoTestArtifact {
    /// Stable `package::kind::target` identity for this artifact.
    #[must_use]
    pub fn target_identity(&self) -> String {
        [self.package_name.as_str(), self.target_kind.as_str(), self.target_name.as_str()]
            .iter()
            .map(|component| escape_component(component))
            .collect::<Vec<_>>()
            .join("::")
    }
}

/// Where Cargo resolved a package from, as a durable role rather than a locator.
///
/// The raw package id embeds the absolute checkout path, which cannot appear in
/// a portable projection; the role plus name and version is the durable part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxPackageSource {
    /// A workspace/path dependency.
    WorkspacePath,
    /// A registry dependency.
    Registry,
    /// A git dependency.
    Git,
    /// A source Cargo spelled in a way this parser does not classify.
    Unknown,
}

/// Name, version, and source role parsed out of a Cargo package id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoPackageIdentity {
    /// Package name.
    pub name: String,
    /// Package version, when the id carries one.
    pub version: Option<String>,
    /// Durable source role.
    pub source: UxPackageSource,
}

/// Parse a Cargo package id into its durable parts.
///
/// Handles three spellings; the absolute locator itself is deliberately not
/// returned, being machine-local detail.
///
/// 1. `path+file:///…#name@version` — the name is explicit.
/// 2. `path+file:///…/name#version` — the **implicit** form, where the name is
///    taken from the last path segment. This is only sound because Cargo emits
///    it exclusively when the package name equals the directory name, and
///    switches to form 1 the moment they differ. A hand-written or
///    wrapper-produced id of this shape with a mismatched directory would
///    therefore yield the directory name; the mismatch is not detectable from
///    the id alone, which is why the package filter in
///    [`parse_cargo_test_artifacts`] is the guard that keeps a wrong name out
///    of the denominator rather than silently renaming a target.
/// 3. `name version (source)` — the legacy form.
#[must_use]
pub fn parse_package_id(package_id: &str) -> Option<CargoPackageIdentity> {
    let source = if package_id.starts_with("path+") {
        UxPackageSource::WorkspacePath
    } else if package_id.starts_with("registry+") {
        UxPackageSource::Registry
    } else if package_id.starts_with("git+") {
        UxPackageSource::Git
    } else {
        UxPackageSource::Unknown
    };

    if let Some((locator, fragment)) = package_id.rsplit_once('#') {
        if let Some((name, version)) = fragment.rsplit_once('@') {
            return Some(CargoPackageIdentity {
                name: name.to_string(),
                version: Some(version.to_string()),
                source,
            });
        }
        // `…/crates/perl-lsp-ux-tests#0.1.0` — the name is the last path segment.
        if fragment.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
            let name = locator.trim_end_matches('/').rsplit('/').next()?;
            return Some(CargoPackageIdentity {
                name: name.to_string(),
                version: Some(fragment.to_string()),
                source,
            });
        }
        return Some(CargoPackageIdentity { name: fragment.to_string(), version: None, source });
    }

    let mut parts = package_id.split_whitespace();
    let name = parts.next()?.to_string();
    let version = parts.next().map(str::to_string);
    let source = if package_id.contains("(path+") {
        UxPackageSource::WorkspacePath
    } else if package_id.contains("(registry+") {
        UxPackageSource::Registry
    } else if package_id.contains("(git+") {
        UxPackageSource::Git
    } else {
        source
    };
    Some(CargoPackageIdentity { name, version, source })
}

/// Parse `cargo test --no-run --message-format=json` stdout into test artifacts.
///
/// Only `compiler-artifact` messages with `profile.test == true`, a non-null
/// `executable`, and a package name equal to `package` are retained; every
/// other package is filtered out of the denominator. Non-JSON lines are
/// tolerated (Cargo interleaves human progress output on stdout in some
/// configurations) but a JSON object whose `reason` is `compiler-artifact` and
/// whose shape cannot be understood is rejected.
///
/// # Errors
///
/// Returns [`UxDiscoveryFailure::MalformedCargoMessage`] when an artifact
/// message is structurally unusable, and
/// [`UxDiscoveryFailure::DuplicateArtifact`] when one target reports two
/// different executables.
pub fn parse_cargo_test_artifacts(
    stdout: &str,
    package: &str,
) -> Result<Vec<CargoTestArtifact>, UxDiscoveryFailure> {
    let mut by_identity: BTreeMap<String, CargoTestArtifact> = BTreeMap::new();

    for (index, line) in stdout.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(value) => value,
            // A line that opens as a JSON object is a Cargo message; failing to
            // parse it would silently drop whichever target it described.
            Err(error) if trimmed.starts_with('{') => {
                return Err(UxDiscoveryFailure::MalformedCargoMessage {
                    line_number,
                    reason: format!("unparseable cargo JSON object: {error}"),
                });
            }
            // Cargo interleaves human progress output on stdout in some
            // configurations; that is not a message and carries no target.
            Err(_) => continue,
        };
        if value.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        if value.get("profile").and_then(|profile| profile.get("test"))
            != Some(&serde_json::Value::Bool(true))
        {
            continue;
        }
        let Some(executable) = value.get("executable").and_then(serde_json::Value::as_str) else {
            continue;
        };

        let package_id =
            value.get("package_id").and_then(serde_json::Value::as_str).ok_or_else(|| {
                UxDiscoveryFailure::MalformedCargoMessage {
                    line_number,
                    reason: "compiler-artifact has no package_id".to_string(),
                }
            })?;
        let package_identity = parse_package_id(package_id).ok_or_else(|| {
            UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: format!("could not extract a package identity from `{package_id}`"),
            }
        })?;
        let package_name = package_identity.name.clone();
        if package_name != package {
            continue;
        }

        let target =
            value.get("target").ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: "compiler-artifact has no target".to_string(),
            })?;
        let target_name = target
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: "compiler-artifact target has no name".to_string(),
            })?
            .to_string();
        let raw_kind = target
            .get("kind")
            .and_then(serde_json::Value::as_array)
            .and_then(|kinds| kinds.first())
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: format!("compiler-artifact target `{target_name}` has no kind"),
            })?;
        let source_file =
            target.get("src_path").and_then(serde_json::Value::as_str).and_then(|path| {
                Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned())
            });

        // An unknown feature population must never normalize into an apparently
        // exact one. A missing key, a non-array value, or a non-string element
        // would otherwise become the empty list, which the `pr` profile accepts
        // as its own exact selection.
        let raw_features =
            value.get("features").ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: format!("compiler-artifact target `{target_name}` has no features array"),
            })?;
        let feature_items =
            raw_features.as_array().ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                line_number,
                reason: format!(
                    "compiler-artifact target `{target_name}` has a non-array features value"
                ),
            })?;
        let mut features: Vec<String> = Vec::with_capacity(feature_items.len());
        for item in feature_items {
            let feature =
                item.as_str().ok_or_else(|| UxDiscoveryFailure::MalformedCargoMessage {
                    line_number,
                    reason: format!(
                        "compiler-artifact target `{target_name}` has a non-string feature entry"
                    ),
                })?;
            features.push(feature.to_string());
        }
        features.sort();
        features.dedup();

        let artifact = CargoTestArtifact {
            package_id: package_id.to_string(),
            package_identity,
            package_name,
            target_name,
            target_kind: UxTargetKind::from_cargo_kind(raw_kind),
            source_file,
            executable: PathBuf::from(executable),
            features,
        };
        let identity = artifact.target_identity();

        match by_identity.get(&identity) {
            // Cargo repeats an artifact message for fresh targets; a repeat is
            // the same fact only when it agrees in full. Comparing the path
            // alone would let two contradictory messages resolve by arrival
            // order instead of failing closed.
            Some(existing) if *existing == artifact => {}
            Some(existing) if existing.executable == artifact.executable => {
                return Err(UxDiscoveryFailure::ContradictoryArtifact {
                    target: identity,
                    detail: format!(
                        "two messages for the same executable disagree: features {:?} vs {:?}, package id `{}` vs `{}`",
                        existing.features,
                        artifact.features,
                        existing.package_id,
                        artifact.package_id
                    ),
                });
            }
            Some(existing) => {
                return Err(UxDiscoveryFailure::DuplicateArtifact {
                    target: identity,
                    first: existing.executable.to_string_lossy().into_owned(),
                    second: artifact.executable.to_string_lossy().into_owned(),
                });
            }
            None => {
                by_identity.insert(identity, artifact);
            }
        }
    }

    Ok(by_identity.into_values().collect())
}

// ── libtest list parsing ─────────────────────────────────────────────────

/// One case reported by a libtest executable's terse listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedCase {
    /// Full module-qualified libtest name.
    pub test_name: String,
    /// Whether libtest called it a test or a benchmark.
    pub kind: UxCaseKind,
}

/// Parse `--list` output from one libtest executable.
///
/// The format is `<name>: test` / `<name>: benchmark` lines followed by a
/// `N tests, M benchmarks` summary. Both the per-line shape and the trailing
/// summary are required: an output with no summary is rejected rather than read
/// as an empty target, which is what stops a different runner's output, a
/// truncated capture, or a silent tool substitution from shrinking the
/// denominator to zero.
///
/// Note that `--list --format terse` prints the same case lines but **omits**
/// the summary, which is why [`LIST_ARGV_SUFFIX`] uses libtest's default list
/// format: dropping the summary would drop the only cross-check that the
/// listing is complete.
///
/// # Errors
///
/// Returns [`UxDiscoveryFailure::MalformedListOutput`],
/// [`UxDiscoveryFailure::MissingListSummary`], or
/// [`UxDiscoveryFailure::ListCountMismatch`].
pub fn parse_libtest_list(
    target: &str,
    output: &str,
) -> Result<Vec<ListedCase>, UxDiscoveryFailure> {
    let mut cases: Vec<ListedCase> = Vec::new();
    let mut summary: Option<(usize, usize)> = None;

    for (index, line) in output.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim_end_matches(['\r']);
        if trimmed.trim().is_empty() {
            continue;
        }
        if let Some(counts) = parse_list_summary(trimmed) {
            if summary.is_some() {
                return Err(UxDiscoveryFailure::MalformedListOutput {
                    target: target.to_string(),
                    line_number,
                    line: trimmed.to_string(),
                });
            }
            summary = Some(counts);
            continue;
        }
        // A case line must come before the summary; anything after it is noise.
        if summary.is_some() {
            return Err(UxDiscoveryFailure::MalformedListOutput {
                target: target.to_string(),
                line_number,
                line: trimmed.to_string(),
            });
        }
        let Some((name, kind)) = trimmed.rsplit_once(": ") else {
            return Err(UxDiscoveryFailure::MalformedListOutput {
                target: target.to_string(),
                line_number,
                line: trimmed.to_string(),
            });
        };
        let kind = match kind {
            "test" => UxCaseKind::Test,
            "benchmark" => UxCaseKind::Benchmark,
            _ => {
                return Err(UxDiscoveryFailure::MalformedListOutput {
                    target: target.to_string(),
                    line_number,
                    line: trimmed.to_string(),
                });
            }
        };
        if name.is_empty() {
            return Err(UxDiscoveryFailure::MalformedListOutput {
                target: target.to_string(),
                line_number,
                line: trimmed.to_string(),
            });
        }
        cases.push(ListedCase { test_name: name.to_string(), kind });
    }

    let Some((declared_tests, declared_benchmarks)) = summary else {
        return Err(UxDiscoveryFailure::MissingListSummary { target: target.to_string() });
    };
    // Compared per kind, not as one total: `foo: benchmark` under a
    // `1 test, 0 benchmarks` summary is contradictory evidence about the runner
    // shape, and a combined comparison would accept it and record the wrong
    // kind.
    let parsed_tests = cases.iter().filter(|case| case.kind == UxCaseKind::Test).count();
    let parsed_benchmarks = cases.len() - parsed_tests;
    if declared_tests != parsed_tests || declared_benchmarks != parsed_benchmarks {
        return Err(UxDiscoveryFailure::ListCountMismatch {
            target: target.to_string(),
            declared_tests,
            parsed_tests,
            declared_benchmarks,
            parsed_benchmarks,
        });
    }

    Ok(cases)
}

/// Recognize libtest's `N tests, M benchmarks` summary line.
fn parse_list_summary(line: &str) -> Option<(usize, usize)> {
    let (tests_part, benchmarks_part) = line.trim().split_once(", ")?;
    let tests = tests_part.strip_suffix(" tests").or_else(|| tests_part.strip_suffix(" test"))?;
    let benchmarks = benchmarks_part
        .strip_suffix(" benchmarks")
        .or_else(|| benchmarks_part.strip_suffix(" benchmark"))?;
    Some((tests.parse().ok()?, benchmarks.parse().ok()?))
}

// ── Inventory documents ──────────────────────────────────────────────────

/// Working-tree cleanliness of the discovery subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxDirtyState {
    /// The working tree matched the recorded SHA exactly.
    Clean,
    /// The working tree carried uncommitted changes.
    Dirty,
    /// Cleanliness could not be established.
    Unknown,
}

/// Where a discovered executable sits relative to the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxExecutableRole {
    /// Inside the workspace's Cargo target directory.
    WorkspaceTarget,
    /// Inside the workspace but not under the target directory.
    WorkspaceOther,
    /// Outside the workspace, but under the declared Cargo target directory.
    ///
    /// This is the ordinary shape when `CARGO_TARGET_DIR` points outside the
    /// checkout, which `.cargo/config.local.toml.example` supports. The replay
    /// stays runnable because the path is recorded relative to
    /// `$CARGO_TARGET_DIR` rather than to the checkout.
    CargoTargetDir,
    /// Outside both the workspace root and the declared target directory.
    OutsideWorkspace,
}

/// Placeholder the replay argv uses for a Cargo-target-dir-relative executable.
pub const CARGO_TARGET_DIR_PLACEHOLDER: &str = "${CARGO_TARGET_DIR}";

/// A limitation that the inventory cannot resolve and must not hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxInventoryLimitation {
    /// libtest's terse listing does not mark which cases are `#[ignore]`d.
    ///
    /// Ignored cases are present in the denominator but indistinguishable here.
    /// Ignore state is an operational-policy fact, not a discovery fact.
    IgnoreStateNotObservable,
    /// At least one compiled target reported zero cases.
    ///
    /// Discovery records the fact; whether that is intentional belongs to the
    /// policy compiler.
    ZeroCaseTargetPresent,
    /// The subject's repository SHA could not be established.
    RepositoryShaUnknown,
    /// The subject's working-tree cleanliness could not be established.
    RepositoryDirtyStateUnknown,
    /// At least one executable lives outside both the workspace and the
    /// declared Cargo target directory.
    ///
    /// Its durable replay names the executable by file name only and therefore
    /// cannot locate it without the machine-local section.
    ReplayNotSelfContained,
}

/// Durable identity of one discovered test executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxExecutableIdentity {
    /// Where the executable sits relative to the workspace.
    pub role: UxExecutableRole,
    /// Slash-normalized workspace-relative path, when the executable is inside.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_relative_path: Option<String>,
    /// Slash-normalized `$CARGO_TARGET_DIR`-relative path, when the executable
    /// lives in an external target directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_dir_relative_path: Option<String>,
    /// Executable file name.
    pub file_name: String,
    /// `sha256:` content digest of the executable.
    pub digest: String,
}

/// Display-only metadata for one case. Never identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxCaseDisplay {
    /// Basename of the Cargo target's source file, when Cargo reported one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// Numeric `ux_scenario_NN` prefix, when the target name carries one.
    ///
    /// Duplicated across targets in the current suite; retained for humans
    /// only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_prefix: Option<String>,
}

/// One discovered executable case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxCase {
    /// Stable collision-free identity.
    pub case_id: UxCaseId,
    /// Full module-qualified libtest name.
    pub test_name: String,
    /// Test or benchmark.
    pub kind: UxCaseKind,
    /// Display metadata; never consulted for identity.
    pub display: UxCaseDisplay,
}

/// Every case belonging to one compiled test target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxTargetInventory {
    /// Stable `package::kind::target` identity.
    pub target_identity: String,
    /// Cargo target name.
    pub target_name: String,
    /// Cargo target kind.
    pub target_kind: UxTargetKind,
    /// Durable identity of the compiled executable.
    pub executable: UxExecutableIdentity,
    /// Exact argv used to list this target's cases, with the executable named
    /// by its durable workspace-relative role rather than an absolute path.
    pub list_argv: Vec<String>,
    /// Number of cases in this target.
    pub case_count: usize,
    /// Cases, sorted by case id.
    pub cases: Vec<UxCase>,
}

/// Exact subject the inventory describes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxDiscoverySubject {
    /// Package whose executables form the denominator.
    pub package: String,
    /// Package version, when at least one artifact was observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_version: Option<String>,
    /// Durable source role of the package.
    ///
    /// The raw Cargo package id embeds the absolute checkout path and is kept
    /// in [`UxLocalExecution`] instead.
    pub package_source: UxPackageSource,
    /// Repository SHA under discovery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_sha: Option<String>,
    /// Working-tree cleanliness at discovery time.
    pub repository_dirty_state: UxDirtyState,
    /// `sha256:` digest of `Cargo.lock`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_lock_digest: Option<String>,
    /// `sha256:` digest of the package manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_manifest_digest: Option<String>,
    /// Rust toolchain identification string.
    pub rust_toolchain: String,
    /// Host target triple.
    pub host_target: String,
    /// Cargo profile the executables were built under.
    pub cargo_profile: String,
    /// Operational profile name (`pr`, `nightly`, `release`).
    pub operational_profile: String,
    /// Exact feature population the operational profile selected, sorted.
    pub selected_features: Vec<String>,
    /// `sha256:` digest binding every field above into one subject identity.
    pub subject_digest: String,
}

/// Exact commands that reproduce this inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxCanonicalReplay {
    /// Exact argv used to compile the test executables.
    pub compile_argv: Vec<String>,
    /// Argv suffix appended to each executable to list its cases.
    pub list_argv_suffix: Vec<String>,
}

/// Counts derived from the discovered rows, never asserted independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxInventoryTotals {
    /// Number of discovered test targets.
    pub target_count: usize,
    /// Number of discovered cases across every target.
    pub case_count: usize,
    /// Number of targets that reported zero cases.
    pub zero_case_target_count: usize,
    /// Per-target case counts, keyed by target identity.
    pub cases_per_target: BTreeMap<String, usize>,
}

/// Non-durable, machine-local execution detail.
///
/// Absolute paths and wall-clock timestamps live here and nowhere else, so the
/// durable projection stays portable and byte-stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxLocalExecution {
    /// When discovery ran, when the caller supplied a clock reading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    /// Raw Cargo package id, which embeds the absolute checkout path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_id: Option<String>,
    /// Absolute workspace root at discovery time.
    pub workspace_root: String,
    /// Absolute Cargo target directory at discovery time, when established.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cargo_target_root: Option<String>,
    /// Absolute executable path per target identity.
    pub target_executables: BTreeMap<String, String>,
}

/// The `ux_case_inventory.v1` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxCaseInventory {
    /// Schema identifier.
    pub schema: String,
    /// Producer identifier.
    pub producer: String,
    /// Exact subject this inventory describes.
    pub subject: UxDiscoverySubject,
    /// Discovered targets, sorted by target identity.
    pub targets: Vec<UxTargetInventory>,
    /// Derived counts.
    pub totals: UxInventoryTotals,
    /// Target identities that reported zero cases, sorted.
    pub zero_case_targets: Vec<String>,
    /// Limitations this inventory cannot resolve, sorted and deduplicated.
    pub limitations: Vec<UxInventoryLimitation>,
    /// Exact reproduction commands.
    pub canonical_replay: UxCanonicalReplay,
    /// `sha256:` digest over the durable projection.
    pub inventory_digest: String,
    /// Machine-local detail; excluded from the digest and the durable projection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_execution: Option<UxLocalExecution>,
}

impl UxCaseInventory {
    /// The durable projection: everything except machine-local detail and the
    /// digest field itself.
    ///
    /// This is what byte-determinism and [`Self::inventory_digest`] are defined
    /// over, so a changed timestamp or a different checkout path cannot move
    /// the inventory's identity, and a changed feature population cannot fail
    /// to move it.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::InstrumentFailure`] if the document cannot
    /// be serialized.
    pub fn durable_projection(&self) -> Result<serde_json::Value, UxDiscoveryFailure> {
        let mut value =
            serde_json::to_value(self).map_err(|error| UxDiscoveryFailure::InstrumentFailure {
                reason: format!("could not serialize inventory: {error}"),
            })?;
        if let Some(object) = value.as_object_mut() {
            object.remove("local_execution");
            object.remove("inventory_digest");
        }
        Ok(value)
    }

    /// Canonical bytes of the durable projection.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::InstrumentFailure`] if serialization fails.
    pub fn durable_bytes(&self) -> Result<Vec<u8>, UxDiscoveryFailure> {
        serde_json::to_vec(&self.durable_projection()?).map_err(|error| {
            UxDiscoveryFailure::InstrumentFailure {
                reason: format!("could not serialize durable projection: {error}"),
            }
        })
    }

    /// Recompute the digest and compare it to the recorded one.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::InstrumentFailure`] when the recorded
    /// digest does not match the durable projection.
    pub fn verify_digest(&self) -> Result<(), UxDiscoveryFailure> {
        let recomputed = sha256_hex(&self.durable_bytes()?);
        if recomputed == self.inventory_digest {
            return Ok(());
        }
        Err(UxDiscoveryFailure::InstrumentFailure {
            reason: format!(
                "inventory digest mismatch: recorded {}, recomputed {recomputed}",
                self.inventory_digest
            ),
        })
    }
}

/// Why the canonical inventory path currently holds no usable inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum UxInventoryInvalidState {
    /// A refresh is in flight; the previous inventory is no longer current.
    DiscoveryInProgress,
    /// A refresh ran and failed; there is no inventory for this subject.
    DiscoveryFailed,
}

/// Tombstone written over the canonical inventory path so that a failed refresh
/// cannot leave a previous run's inventory readable as this run's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UxCaseInventoryInvalid {
    /// Schema identifier — never [`UX_CASE_INVENTORY_SCHEMA`].
    pub schema: String,
    /// Producer identifier.
    pub producer: String,
    /// Operational profile the failed or in-flight refresh targeted.
    pub operational_profile: String,
    /// Why there is no usable inventory here.
    pub state: UxInventoryInvalidState,
    /// Stable discriminator of the discovery failure, when one occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    /// Human-readable failure detail, when one occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl UxCaseInventoryInvalid {
    /// Tombstone for a refresh that is about to start.
    #[must_use]
    pub fn in_progress(tier: UxCiTier) -> Self {
        Self {
            schema: UX_CASE_INVENTORY_INVALID_SCHEMA.to_string(),
            producer: UX_CASE_INVENTORY_PRODUCER.to_string(),
            operational_profile: profile_name(tier).to_string(),
            state: UxInventoryInvalidState::DiscoveryInProgress,
            failure_kind: None,
            detail: None,
        }
    }

    /// Tombstone for a refresh that failed.
    #[must_use]
    pub fn failed(tier: UxCiTier, failure: &UxDiscoveryFailure) -> Self {
        Self {
            schema: UX_CASE_INVENTORY_INVALID_SCHEMA.to_string(),
            producer: UX_CASE_INVENTORY_PRODUCER.to_string(),
            operational_profile: profile_name(tier).to_string(),
            state: UxInventoryInvalidState::DiscoveryFailed,
            failure_kind: Some(failure.kind().to_string()),
            detail: Some(failure.to_string()),
        }
    }
}

/// `sha256:`-prefixed hex digest of a byte slice.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hex: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

// ── Discovery ────────────────────────────────────────────────────────────

/// Every impure step discovery needs.
///
/// The discovery algorithm holds no filesystem or process access of its own, so
/// tests inject fixtures here and the production implementation lives in
/// `xtask`.
pub trait UxDiscoveryCommands {
    /// Compile the package's test targets and return raw Cargo JSON stdout.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::CargoInvocationFailed`] when Cargo fails.
    fn compile_test_targets(&self, argv: &[String]) -> Result<String, UxDiscoveryFailure>;

    /// Ask one test executable for its libtest case list.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::ListCommandFailed`] when the invocation
    /// fails.
    fn list_cases(
        &self,
        target_identity: &str,
        executable: &Path,
        argv: &[String],
    ) -> Result<String, UxDiscoveryFailure>;

    /// `sha256:` content digest of one test executable.
    ///
    /// # Errors
    ///
    /// Returns [`UxDiscoveryFailure::DigestUnavailable`] when the executable
    /// cannot be read.
    fn executable_digest(
        &self,
        target_identity: &str,
        executable: &Path,
    ) -> Result<String, UxDiscoveryFailure>;

    /// Whether the compiled executable is present on disk.
    fn executable_exists(&self, executable: &Path) -> bool;
}

/// Everything the caller must establish about the subject before discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UxDiscoveryRequest {
    /// Operational profile to discover.
    pub tier: UxCiTier,
    /// Repository SHA, when it could be established.
    pub repository_sha: Option<String>,
    /// Working-tree cleanliness.
    pub repository_dirty_state: UxDirtyState,
    /// `sha256:` digest of `Cargo.lock`.
    pub cargo_lock_digest: Option<String>,
    /// `sha256:` digest of the package manifest.
    pub package_manifest_digest: Option<String>,
    /// Rust toolchain identification string.
    pub rust_toolchain: String,
    /// Host target triple.
    pub host_target: String,
    /// Cargo profile the executables are built under.
    pub cargo_profile: String,
    /// Absolute workspace root, used to normalize executable path roles.
    pub workspace_root: PathBuf,
    /// Absolute Cargo target directory, when the caller could establish one.
    ///
    /// Lets an executable under an external `CARGO_TARGET_DIR` keep a runnable
    /// durable replay instead of degrading to a bare file name.
    pub cargo_target_root: Option<PathBuf>,
    /// Wall-clock reading recorded in the machine-local section only.
    pub generated_at: Option<String>,
    /// Whether to emit the machine-local section at all.
    pub include_local_execution: bool,
}

impl UxDiscoveryRequest {
    /// A request with only the required subject facts, everything else unknown.
    #[must_use]
    pub fn new(tier: UxCiTier, workspace_root: PathBuf) -> Self {
        Self {
            tier,
            repository_sha: None,
            repository_dirty_state: UxDirtyState::Unknown,
            cargo_lock_digest: None,
            package_manifest_digest: None,
            rust_toolchain: "unknown".to_string(),
            host_target: "unknown".to_string(),
            cargo_profile: "test".to_string(),
            workspace_root,
            cargo_target_root: None,
            generated_at: None,
            include_local_execution: false,
        }
    }
}

/// Argv suffix appended to each test executable to list its cases.
///
/// Deliberately libtest's default list format rather than `--format terse`:
/// terse prints the same `<name>: test` lines but omits the trailing
/// `N tests, M benchmarks` summary, and that summary is the only cross-check
/// that the captured listing is complete. See [`parse_libtest_list`].
pub const LIST_ARGV_SUFFIX: &[&str] = &["--list"];

/// Exact `cargo` argv that compiles the discovery population for one profile.
#[must_use]
pub fn compile_argv(tier: UxCiTier) -> Vec<String> {
    let mut argv: Vec<String> = [
        "cargo",
        "test",
        "--locked",
        "--no-run",
        "--message-format=json",
        "-p",
        UX_INVENTORY_PACKAGE,
    ]
    .iter()
    .map(|part| (*part).to_string())
    .collect();
    let features = profile_features(tier);
    if !features.is_empty() {
        argv.push("--features".to_string());
        argv.push(features.join(","));
    }
    argv
}

/// Discover the exact executable case population for one operational profile.
///
/// # Errors
///
/// Returns the [`UxDiscoveryFailure`] describing the first state that would
/// otherwise have to be represented as a smaller or empty denominator.
pub fn discover_cases(
    commands: &dyn UxDiscoveryCommands,
    request: &UxDiscoveryRequest,
) -> Result<UxCaseInventory, UxDiscoveryFailure> {
    let compile_argv = compile_argv(request.tier);
    let stdout = commands.compile_test_targets(&compile_argv)?;
    let artifacts = parse_cargo_test_artifacts(&stdout, UX_INVENTORY_PACKAGE)?;

    if artifacts.is_empty() {
        return Err(UxDiscoveryFailure::NoTestArtifacts {
            package: UX_INVENTORY_PACKAGE.to_string(),
        });
    }

    // Sorted to match the artifact-side normalization: a feature population is
    // a set, and ordering must never decide whether an executable is stale.
    let mut expected_features: Vec<String> =
        profile_features(request.tier).iter().map(|feature| (*feature).to_string()).collect();
    expected_features.sort();
    expected_features.dedup();
    let list_suffix: Vec<String> =
        LIST_ARGV_SUFFIX.iter().map(|part| (*part).to_string()).collect();

    let mut targets: Vec<UxTargetInventory> = Vec::with_capacity(artifacts.len());
    let mut seen_case_ids: BTreeMap<String, String> = BTreeMap::new();
    let mut zero_case_targets: Vec<String> = Vec::new();
    let mut local_executables: BTreeMap<String, String> = BTreeMap::new();
    let mut package_id: Option<String> = None;
    let mut package_identity: Option<CargoPackageIdentity> = None;
    let mut replay_not_self_contained = false;

    for artifact in artifacts {
        let identity = artifact.target_identity();

        if artifact.features != expected_features {
            return Err(UxDiscoveryFailure::WrongProfileArtifact {
                target: identity,
                expected_features,
                actual_features: artifact.features,
            });
        }
        if !commands.executable_exists(&artifact.executable) {
            return Err(UxDiscoveryFailure::TestArtifactMissing {
                target: identity,
                path: artifact.executable.to_string_lossy().into_owned(),
            });
        }

        // The digest and the listing are two observations of a file a concurrent
        // build can replace between them. Bracketing the listing binds the cases
        // to the exact executable they came from instead of to whichever binary
        // happened to be on disk first.
        let digest = commands.executable_digest(&identity, &artifact.executable)?;
        let listing = commands.list_cases(&identity, &artifact.executable, &list_suffix)?;
        let digest_after = commands.executable_digest(&identity, &artifact.executable)?;
        if digest != digest_after {
            return Err(UxDiscoveryFailure::ExecutableChangedDuringDiscovery {
                target: identity,
                before: digest,
                after: digest_after,
            });
        }
        let listed = parse_libtest_list(&identity, &listing)?;

        let scenario_prefix = scenario_prefix(&artifact.target_name);
        let mut cases: Vec<UxCase> = Vec::with_capacity(listed.len());
        for case in listed {
            let case_id = UxCaseId::new(
                &artifact.package_name,
                artifact.target_kind.as_str(),
                &artifact.target_name,
                &case.test_name,
            );
            if let Some(first_target) = seen_case_ids.get(case_id.as_str()) {
                return Err(UxDiscoveryFailure::DuplicateCaseId {
                    case_id: case_id.as_str().to_string(),
                    first_target: first_target.clone(),
                    second_target: identity,
                });
            }
            seen_case_ids.insert(case_id.as_str().to_string(), identity.clone());
            cases.push(UxCase {
                case_id,
                test_name: case.test_name,
                kind: case.kind,
                display: UxCaseDisplay {
                    source_file: artifact.source_file.clone(),
                    scenario_prefix: scenario_prefix.clone(),
                },
            });
        }
        cases.sort_by(|left, right| left.case_id.cmp(&right.case_id));

        if cases.is_empty() {
            zero_case_targets.push(identity.clone());
        }
        if package_id.is_none() {
            package_id = Some(artifact.package_id.clone());
            package_identity = Some(artifact.package_identity.clone());
        }
        local_executables
            .insert(identity.clone(), artifact.executable.to_string_lossy().into_owned());

        let executable = executable_identity(
            &request.workspace_root,
            request.cargo_target_root.as_deref(),
            &artifact.executable,
            digest,
        );
        if executable.role == UxExecutableRole::OutsideWorkspace {
            replay_not_self_contained = true;
        }
        // The replay argv names the executable by its durable role, never by the
        // absolute path of whichever checkout happened to run discovery.
        let mut list_argv = vec![executable.replay_argv0()];
        list_argv.extend(list_suffix.iter().cloned());

        targets.push(UxTargetInventory {
            target_identity: identity,
            target_name: artifact.target_name,
            target_kind: artifact.target_kind,
            executable,
            list_argv,
            case_count: cases.len(),
            cases,
        });
    }

    targets.sort_by(|left, right| left.target_identity.cmp(&right.target_identity));
    zero_case_targets.sort();

    let mut limitations: BTreeSet<UxInventoryLimitation> = BTreeSet::new();
    limitations.insert(UxInventoryLimitation::IgnoreStateNotObservable);
    if !zero_case_targets.is_empty() {
        limitations.insert(UxInventoryLimitation::ZeroCaseTargetPresent);
    }
    if request.repository_sha.is_none() {
        limitations.insert(UxInventoryLimitation::RepositoryShaUnknown);
    }
    if request.repository_dirty_state == UxDirtyState::Unknown {
        limitations.insert(UxInventoryLimitation::RepositoryDirtyStateUnknown);
    }
    if replay_not_self_contained {
        limitations.insert(UxInventoryLimitation::ReplayNotSelfContained);
    }

    let cases_per_target: BTreeMap<String, usize> =
        targets.iter().map(|target| (target.target_identity.clone(), target.case_count)).collect();
    let totals = UxInventoryTotals {
        target_count: targets.len(),
        case_count: targets.iter().map(|target| target.case_count).sum(),
        zero_case_target_count: zero_case_targets.len(),
        cases_per_target,
    };

    let subject = build_subject(request, package_identity.as_ref(), &expected_features)?;

    let mut inventory = UxCaseInventory {
        schema: UX_CASE_INVENTORY_SCHEMA.to_string(),
        producer: UX_CASE_INVENTORY_PRODUCER.to_string(),
        subject,
        targets,
        totals,
        zero_case_targets,
        limitations: limitations.into_iter().collect(),
        canonical_replay: UxCanonicalReplay { compile_argv, list_argv_suffix: list_suffix },
        inventory_digest: String::new(),
        local_execution: request.include_local_execution.then(|| UxLocalExecution {
            generated_at: request.generated_at.clone(),
            package_id,
            workspace_root: normalize_path(&request.workspace_root),
            cargo_target_root: request.cargo_target_root.as_deref().map(normalize_path),
            target_executables: local_executables,
        }),
    };
    inventory.inventory_digest = sha256_hex(&inventory.durable_bytes()?);

    Ok(inventory)
}

fn build_subject(
    request: &UxDiscoveryRequest,
    package_identity: Option<&CargoPackageIdentity>,
    selected_features: &[String],
) -> Result<UxDiscoverySubject, UxDiscoveryFailure> {
    let mut subject = UxDiscoverySubject {
        package: UX_INVENTORY_PACKAGE.to_string(),
        package_version: package_identity.and_then(|identity| identity.version.clone()),
        package_source: package_identity
            .map_or(UxPackageSource::Unknown, |identity| identity.source),
        repository_sha: request.repository_sha.clone(),
        repository_dirty_state: request.repository_dirty_state,
        cargo_lock_digest: request.cargo_lock_digest.clone(),
        package_manifest_digest: request.package_manifest_digest.clone(),
        rust_toolchain: request.rust_toolchain.clone(),
        host_target: request.host_target.clone(),
        cargo_profile: request.cargo_profile.clone(),
        operational_profile: profile_name(request.tier).to_string(),
        selected_features: selected_features.to_vec(),
        subject_digest: String::new(),
    };
    let mut identity =
        serde_json::to_value(&subject).map_err(|error| UxDiscoveryFailure::InstrumentFailure {
            reason: format!("could not serialize subject: {error}"),
        })?;
    if let Some(object) = identity.as_object_mut() {
        object.remove("subject_digest");
    }
    let bytes =
        serde_json::to_vec(&identity).map_err(|error| UxDiscoveryFailure::InstrumentFailure {
            reason: format!("could not serialize subject identity: {error}"),
        })?;
    subject.subject_digest = sha256_hex(&bytes);
    Ok(subject)
}

/// Display-only `ux_scenario_NN` prefix, when the target name carries one.
fn scenario_prefix(target_name: &str) -> Option<String> {
    let rest = target_name.strip_prefix("ux_scenario_")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn executable_identity(
    workspace_root: &Path,
    cargo_target_root: Option<&Path>,
    executable: &Path,
    digest: String,
) -> UxExecutableIdentity {
    let file_name = executable
        .file_name()
        .map_or_else(|| normalize_path(executable), |name| name.to_string_lossy().into_owned());

    if let Ok(relative) = executable.strip_prefix(workspace_root) {
        let relative = normalize_path(relative);
        let role = if relative.starts_with("target/") {
            UxExecutableRole::WorkspaceTarget
        } else {
            UxExecutableRole::WorkspaceOther
        };
        return UxExecutableIdentity {
            role,
            workspace_relative_path: Some(relative),
            target_dir_relative_path: None,
            file_name,
            digest,
        };
    }

    // `CARGO_TARGET_DIR` outside the checkout is a supported layout; recording
    // the path relative to that root keeps the replay runnable without leaking
    // an absolute path.
    if let Some(relative) = cargo_target_root.and_then(|root| executable.strip_prefix(root).ok()) {
        return UxExecutableIdentity {
            role: UxExecutableRole::CargoTargetDir,
            workspace_relative_path: None,
            target_dir_relative_path: Some(normalize_path(relative)),
            file_name,
            digest,
        };
    }

    UxExecutableIdentity {
        role: UxExecutableRole::OutsideWorkspace,
        workspace_relative_path: None,
        target_dir_relative_path: None,
        file_name,
        digest,
    }
}

impl UxExecutableIdentity {
    /// How the replay argv names this executable.
    ///
    /// Workspace-relative where possible, `$CARGO_TARGET_DIR`-relative for an
    /// external target directory, and the bare file name only when the
    /// executable is under neither root — which is the case
    /// [`UxInventoryLimitation::ReplayNotSelfContained`] declares.
    #[must_use]
    pub fn replay_argv0(&self) -> String {
        if let Some(path) = &self.workspace_relative_path {
            return path.clone();
        }
        if let Some(path) = &self.target_dir_relative_path {
            return format!("{CARGO_TARGET_DIR_PLACEHOLDER}/{path}");
        }
        self.file_name.clone()
    }
}

#[cfg(test)]
mod tests;
