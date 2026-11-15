//! Topology-derived executable budget check for the v0.18 release artifact set.
//!
//! This activates the existing `release_artifact_size` measurement modules as
//! a normal binary while preserving the older safe-ICF comparison example.
//! The `budget` command is deliberately a local/check receipt: release
//! eligibility still requires the strip, capability-manifest, exact handoff,
//! installed, and publication joins owned by #19435/#19436/#19438.

#![allow(clippy::print_stderr, clippy::print_stdout)]

use clap::{Args, Parser, Subcommand};
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

// The pre-existing modules also support the broader safe-ICF comparison
// example. This binary consumes only their shared identity/path substrate.
#[allow(dead_code)]
#[path = "release_artifact_size/measure.rs"]
mod measure;
#[allow(dead_code)]
#[path = "release_artifact_size/model.rs"]
mod model;
#[allow(dead_code)]
#[path = "release_artifact_size/policy.rs"]
mod policy;

pub(crate) use policy::{BINARY_NAMES, GOVERNED_TARGETS, REPOSITORY, SAFE_ICF_RUSTFLAGS};

const CHECK_NAME: &str = "release-executable-budget";
const SCHEMA_VERSION: &str = "release_executable_budget.v1";
const DEFAULT_HARD_CAP_BYTES: u64 = 10_485_760;
const CLAIM_BOUNDARY: &str = concat!(
    "topology-derived local measurement of exact executable files against the ",
    "v0.18 10 MiB per-executable cap; observed file evidence proves format and ",
    "architecture only, not target ABI; does not establish target-native strip ",
    "success, release-core capability parity, exact built-product handoff, ",
    "installed behavior, publication eligibility, or public verification"
);
const KNOWN_NON_EXECUTABLE_MEMBERS: &[&str] =
    &["README.md", "LICENSE-APACHE", "LICENSE-MIT", "SHA256SUMS.txt"];

#[derive(Debug, Parser)]
#[command(name = "release-artifact-size")]
#[command(about = "Validate release artifact size contracts")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Measure one topology target's native executables against the hard cap.
    Budget(BudgetArgs),
}

#[derive(Debug, Args)]
struct BudgetArgs {
    /// Exact release_topology.v1 JSON supplying target/member identity.
    #[arg(long)]
    topology: PathBuf,

    /// Rust target triple selected from the topology.
    #[arg(long)]
    target: String,

    /// Directory containing the exact post-build executable members.
    #[arg(long)]
    artifact_dir: PathBuf,

    /// Exact release-core capability manifest consumed by the candidate.
    /// Its digest is recorded as an input slot; #19437 owns semantic validation
    /// and #19438 owns the release-eligibility join.
    #[arg(long)]
    capability_manifest: Option<PathBuf>,

    /// Exact hard cap applied independently to every native executable.
    #[arg(long, default_value_t = DEFAULT_HARD_CAP_BYTES)]
    hard_cap_bytes: u64,

    /// JSON receipt output path.
    #[arg(long)]
    json: PathBuf,
}

#[derive(Debug, Deserialize)]
struct ReleaseTopology {
    schema: u64,
    frozen_product_sha: String,
    binary_targets: Vec<TopologyTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct TopologyTarget {
    target: String,
    required_members: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum BudgetState {
    Pass,
    Failed,
    NotProven,
    InstrumentFailed,
}

impl BudgetState {
    fn is_blocking(self) -> bool {
        self != Self::Pass
    }
}

#[derive(Debug, Serialize)]
struct Receipt {
    check: &'static str,
    schema_version: &'static str,
    state: BudgetState,
    claim_boundary: &'static str,
    hard_cap_bytes: u64,
    topology: TopologyEvidence,
    capability_manifest: Option<FileIdentity>,
    subject: model::SubjectIdentity,
    artifacts: BTreeMap<String, ArtifactEvidence>,
    total_executable_bytes: u64,
    limitations: Vec<String>,
}

#[derive(Debug, Serialize)]
struct TopologyEvidence {
    path: String,
    sha256: String,
    schema: u64,
    frozen_product_sha: String,
    target: String,
    required_members: Vec<String>,
    executable_members: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FileIdentity {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct ExecutableIdentity {
    format: &'static str,
    architecture: &'static str,
}

#[derive(Debug, Serialize)]
struct ArtifactEvidence {
    member: String,
    path: String,
    bytes: Option<u64>,
    sha256: Option<String>,
    executable_identity: Option<ExecutableIdentity>,
    format_architecture_matches: bool,
    within_hard_cap: bool,
    state: BudgetState,
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();
    match cli.command {
        Command::Budget(args) => run_budget(args),
    }
}

fn run_budget(args: BudgetArgs) -> Result<()> {
    if args.hard_cap_bytes == 0 {
        bail!("--hard-cap-bytes must be greater than zero");
    }

    let root = measure::project_root()?;
    let topology_path = measure::resolve_path(&root, &args.topology);
    let topology_bytes = fs::read(&topology_path)
        .with_context(|| format!("reading release topology {}", topology_path.display()))?;
    let topology: ReleaseTopology = serde_json::from_slice(&topology_bytes)
        .with_context(|| format!("parsing release topology {}", topology_path.display()))?;

    let matching_targets = topology
        .binary_targets
        .iter()
        .filter(|row| row.target == args.target)
        .cloned()
        .collect::<Vec<_>>();
    let target = match matching_targets.as_slice() {
        [target] => target.clone(),
        rows => {
            bail!(
                "release topology must contain exactly one `{}` target row; found {}",
                args.target,
                rows.len()
            )
        }
    };
    let executable_members = executable_members(&target.required_members)?;

    let mut limitations = Vec::new();
    let subject = measure::subject_identity(&root, &args.target, "", "", &mut limitations)?;
    let topology_schema_supported = topology.schema == 1;
    if !topology_schema_supported {
        limitations
            .push(format!("unsupported release topology schema {}; expected 1", topology.schema));
    }
    let source_identity_matches = topology.frozen_product_sha == subject.git_sha;
    if !source_identity_matches {
        limitations.push(format!(
            "topology frozen_product_sha {} does not match current source {}",
            topology.frozen_product_sha, subject.git_sha
        ));
    }
    limitations.push(
        "file header evidence proves executable format and architecture, not target ABI; #19438 must bind the exact build-plan target"
            .to_string(),
    );

    let capability_manifest = match args.capability_manifest.as_ref() {
        Some(path) => match file_identity(&root, path) {
            Ok(identity) => Some(identity),
            Err(error) => {
                limitations.push(format!("capability manifest identity unavailable: {error}"));
                None
            }
        },
        None => {
            limitations.push(
                "release-core capability manifest digest is not attached; #19437/#19438 must join product parity before release eligibility"
                    .to_string(),
            );
            None
        }
    };

    let artifact_dir = measure::resolve_path(&root, &args.artifact_dir);
    let mut artifacts = BTreeMap::new();
    let mut total_executable_bytes = 0_u64;
    for member in &executable_members {
        let path = artifact_dir.join(member);
        let evidence = match file_identity_absolute(&root, &path) {
            Ok(identity) => {
                total_executable_bytes = total_executable_bytes.saturating_add(identity.bytes);
                let executable_identity = detect_executable_identity(&path)?;
                let format_architecture_matches = executable_identity
                    .as_ref()
                    .is_some_and(|observed| target_matches_identity(&args.target, observed));
                if !format_architecture_matches {
                    limitations.push(format!(
                        "required executable `{member}` does not match the format and architecture for `{}`",
                        args.target
                    ));
                }
                let within_hard_cap = identity.bytes <= args.hard_cap_bytes;
                let state = if !within_hard_cap {
                    BudgetState::Failed
                } else if format_architecture_matches {
                    BudgetState::Pass
                } else {
                    BudgetState::NotProven
                };
                ArtifactEvidence {
                    member: member.clone(),
                    path: identity.path,
                    bytes: Some(identity.bytes),
                    sha256: Some(identity.sha256),
                    executable_identity,
                    format_architecture_matches,
                    within_hard_cap,
                    state,
                }
            }
            Err(error) => {
                limitations.push(format!("required executable `{member}` unavailable: {error}"));
                ArtifactEvidence {
                    member: member.clone(),
                    path: measure::display_path(&root, &path),
                    bytes: None,
                    sha256: None,
                    executable_identity: None,
                    format_architecture_matches: false,
                    within_hard_cap: false,
                    state: BudgetState::NotProven,
                }
            }
        };
        artifacts.insert(member.clone(), evidence);
    }

    let state = classify_state(
        artifacts.values().map(|artifact| artifact.state),
        topology_schema_supported,
        source_identity_matches,
        subject.tree_clean,
    );

    limitations.sort();
    limitations.dedup();
    let receipt = Receipt {
        check: CHECK_NAME,
        schema_version: SCHEMA_VERSION,
        state,
        claim_boundary: CLAIM_BOUNDARY,
        hard_cap_bytes: args.hard_cap_bytes,
        topology: TopologyEvidence {
            path: measure::display_path(&root, &topology_path),
            sha256: sha256_bytes(&topology_bytes),
            schema: topology.schema,
            frozen_product_sha: topology.frozen_product_sha,
            target: args.target,
            required_members: target.required_members,
            executable_members,
        },
        capability_manifest,
        subject,
        artifacts,
        total_executable_bytes,
        limitations,
    };

    write_receipt(&root, &args.json, &receipt)?;
    println!(
        "release executable budget: target={} state={:?} cap={} total={}",
        receipt.topology.target,
        receipt.state,
        receipt.hard_cap_bytes,
        receipt.total_executable_bytes
    );
    if receipt.state.is_blocking() {
        bail!(
            "release executable budget is {:?}; see {}",
            receipt.state,
            measure::display_path(&root, &measure::resolve_path(&root, &args.json))
        );
    }
    Ok(())
}

fn executable_members(required_members: &[String]) -> Result<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut executables = Vec::new();
    for member in required_members {
        if !seen.insert(member) {
            bail!("duplicate release topology member `{member}`");
        }
        let file_name = Path::new(member)
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| color_eyre::eyre::eyre!("invalid topology member `{member}`"))?;
        if KNOWN_NON_EXECUTABLE_MEMBERS.contains(&file_name) {
            continue;
        }
        executables.push(member.clone());
    }
    if executables.is_empty() {
        bail!("release topology target contains no executable members");
    }
    Ok(executables)
}

fn classify_state(
    artifact_states: impl Iterator<Item = BudgetState>,
    topology_schema_supported: bool,
    source_identity_matches: bool,
    tree_clean: bool,
) -> BudgetState {
    let states = artifact_states.collect::<Vec<_>>();
    if states.iter().any(|state| *state == BudgetState::InstrumentFailed) {
        return BudgetState::InstrumentFailed;
    }
    if states.iter().any(|state| *state == BudgetState::Failed) {
        return BudgetState::Failed;
    }
    if states.iter().any(|state| *state == BudgetState::NotProven)
        || !topology_schema_supported
        || !source_identity_matches
        || !tree_clean
    {
        return BudgetState::NotProven;
    }
    BudgetState::Pass
}

fn file_identity(root: &Path, path: &Path) -> Result<FileIdentity> {
    let path = measure::resolve_path(root, path);
    file_identity_absolute(root, &path)
}

fn file_identity_absolute(root: &Path, path: &Path) -> Result<FileIdentity> {
    let metadata = fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("hashing {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(FileIdentity {
        path: measure::display_path(root, path),
        bytes: metadata.len(),
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn detect_executable_identity(path: &Path) -> Result<Option<ExecutableIdentity>> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut bytes = vec![0_u8; 4096];
    let read = file
        .read(&mut bytes)
        .with_context(|| format!("reading executable header {}", path.display()))?;
    bytes.truncate(read);

    if bytes.starts_with(b"\x7fELF") {
        let machine = match bytes.get(5).copied() {
            Some(1) => read_u16_le(&bytes, 18),
            Some(2) => read_u16_be(&bytes, 18),
            _ => None,
        };
        return Ok(Some(ExecutableIdentity {
            format: "elf",
            architecture: architecture_from_elf(machine),
        }));
    }

    if bytes.starts_with(b"MZ") {
        let pe_offset = read_u32_le(&bytes, 0x3c).and_then(|value| usize::try_from(value).ok());
        let machine = pe_offset.and_then(|offset| {
            let signature = bytes.get(offset..offset.saturating_add(4))?;
            if signature != b"PE\0\0" {
                return None;
            }
            read_u16_le(&bytes, offset.saturating_add(4))
        });
        return Ok(Some(ExecutableIdentity {
            format: "pe",
            architecture: architecture_from_pe(machine),
        }));
    }

    let magic = bytes.get(0..4);
    if magic == Some(&[0xcf, 0xfa, 0xed, 0xfe]) {
        return Ok(Some(ExecutableIdentity {
            format: "mach-o",
            architecture: architecture_from_mach(read_u32_le(&bytes, 4)),
        }));
    }
    if magic == Some(&[0xfe, 0xed, 0xfa, 0xcf]) {
        return Ok(Some(ExecutableIdentity {
            format: "mach-o",
            architecture: architecture_from_mach(read_u32_be(&bytes, 4)),
        }));
    }

    Ok(None)
}

fn expected_identity(target: &str) -> Option<(&'static str, &'static str)> {
    let architecture = if target.starts_with("x86_64-") {
        "x86_64"
    } else if target.starts_with("aarch64-") {
        "aarch64"
    } else {
        return None;
    };
    let format = if target.contains("windows") {
        "pe"
    } else if target.contains("apple-darwin") {
        "mach-o"
    } else if target.contains("linux") {
        "elf"
    } else {
        return None;
    };
    Some((format, architecture))
}

fn target_matches_identity(target: &str, observed: &ExecutableIdentity) -> bool {
    expected_identity(target)
        .is_some_and(|(format, architecture)| observed.format == format && observed.architecture == architecture)
}

fn architecture_from_elf(machine: Option<u16>) -> &'static str {
    match machine {
        Some(62) => "x86_64",
        Some(183) => "aarch64",
        _ => "unknown",
    }
}

fn architecture_from_pe(machine: Option<u16>) -> &'static str {
    match machine {
        Some(0x8664) => "x86_64",
        Some(0xaa64) => "aarch64",
        _ => "unknown",
    }
}

fn architecture_from_mach(cpu_type: Option<u32>) -> &'static str {
    match cpu_type {
        Some(0x01000007) => "x86_64",
        Some(0x0100000c) => "aarch64",
        _ => "unknown",
    }
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([*value.first()?, *value.get(1)?]))
}

fn read_u16_be(bytes: &[u8], offset: usize) -> Option<u16> {
    let value = bytes.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_be_bytes([*value.first()?, *value.get(1)?]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([*value.first()?, *value.get(1)?, *value.get(2)?, *value.get(3)?]))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes([*value.first()?, *value.get(1)?, *value.get(2)?, *value.get(3)?]))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn write_receipt(root: &Path, path: &Path, receipt: &Receipt) -> Result<()> {
    let path = measure::resolve_path(root, path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating receipt directory {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(receipt).context("serializing budget receipt")?;
    bytes.push(b'\n');
    fs::write(&path, bytes).with_context(|| format!("writing receipt {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        BudgetState, ExecutableIdentity, architecture_from_elf, architecture_from_mach,
        architecture_from_pe, classify_state, executable_members, target_matches_identity,
    };
    use color_eyre::eyre::Result;

    #[test]
    fn topology_members_drive_the_complete_native_denominator() -> Result<()> {
        let members = vec![
            "perllsp".to_string(),
            "perl-dap".to_string(),
            "perl-tidy".to_string(),
            "README.md".to_string(),
            "LICENSE-APACHE".to_string(),
            "LICENSE-MIT".to_string(),
            "SHA256SUMS.txt".to_string(),
        ];
        let executables = executable_members(&members)?;
        assert_eq!(executables, ["perllsp", "perl-dap", "perl-tidy"]);
        Ok(())
    }

    #[test]
    fn one_oversized_executable_fails_the_complete_set() {
        let state =
            classify_state([BudgetState::Pass, BudgetState::Failed].into_iter(), true, true, true);
        assert_eq!(state, BudgetState::Failed);
    }

    #[test]
    fn missing_companion_is_not_proven() {
        let missing_companion = classify_state(
            [BudgetState::Pass, BudgetState::NotProven].into_iter(),
            true,
            true,
            true,
        );
        assert_eq!(missing_companion, BudgetState::NotProven);
    }

    #[test]
    fn dirty_or_cross_sha_subject_cannot_pass() {
        assert_eq!(
            classify_state([BudgetState::Pass].into_iter(), true, false, true),
            BudgetState::NotProven
        );
        assert_eq!(
            classify_state([BudgetState::Pass].into_iter(), true, true, false),
            BudgetState::NotProven
        );
    }

    #[test]
    fn executable_architecture_mapping_is_exact() {
        assert_eq!(architecture_from_elf(Some(62)), "x86_64");
        assert_eq!(architecture_from_elf(Some(183)), "aarch64");
        assert_eq!(architecture_from_pe(Some(0x8664)), "x86_64");
        assert_eq!(architecture_from_pe(Some(0xaa64)), "aarch64");
        assert_eq!(architecture_from_mach(Some(0x01000007)), "x86_64");
        assert_eq!(architecture_from_mach(Some(0x0100000c)), "aarch64");
    }

    #[test]
    fn format_and_architecture_do_not_cross_operating_system_or_cpu() {
        let linux_x64 = ExecutableIdentity { format: "elf", architecture: "x86_64" };
        assert!(target_matches_identity("x86_64-unknown-linux-gnu", &linux_x64));
        assert!(!target_matches_identity("aarch64-unknown-linux-gnu", &linux_x64));
        assert!(!target_matches_identity("x86_64-pc-windows-msvc", &linux_x64));
    }
}
