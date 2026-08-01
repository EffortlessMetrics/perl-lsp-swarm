use color_eyre::eyre::{Context, Result, bail, eyre};
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tar::Archive;

use super::model::{
    ArchiveEvidence, ComparisonEvidence, DecisionFacts, DecisionPolicy, EmbeddedArtifact,
    FileArtifact, Recommendation, SmokeEvidence, SmokeStatus, SubjectIdentity, ToolIdentity,
    VariantEvidence, component_growth_exceeds, decide, repeat_requirement_satisfied, size_delta,
    smokes_pass,
};
use super::{BINARY_NAMES, GOVERNED_TARGETS, REPOSITORY, SAFE_ICF_RUSTFLAGS};

#[derive(Debug, Deserialize)]
struct SmokeSuccessShape {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    outcome: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    binary: Option<String>,
}

pub(crate) fn project_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| eyre!("xtask manifest directory has no repository parent"))
}

pub(crate) fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
}

pub(crate) fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn subject_identity(
    root: &Path,
    target: &str,
    baseline_rustflags: &str,
    candidate_rustflags: &str,
    limitations: &mut Vec<String>,
) -> Result<SubjectIdentity> {
    let rustc = capture("rustc", &["-vV"]).unwrap_or_else(|| "unknown".to_string());
    let cargo = capture("cargo", &["--version"]).unwrap_or_else(|| "unknown".to_string());
    let host = parse_rustc_host(&rustc).unwrap_or_else(|| "unknown".to_string());
    let git_sha =
        capture_in(root, "git", &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let tree_clean =
        capture_in(root, "git", &["status", "--porcelain"]).is_some_and(|value| value.is_empty());
    let workspace_version = workspace_version(root).unwrap_or_else(|error| {
        limitations.push(format!("workspace version unavailable: {error}"));
        "unknown".to_string()
    });
    let cargo_lock_sha256 = measure_sha256(&root.join("Cargo.lock")).unwrap_or_else(|error| {
        limitations.push(format!("Cargo.lock identity unavailable: {error}"));
        "unknown".to_string()
    });
    let rust_lld = rust_lld_identity(&host).unwrap_or_else(|error| {
        limitations.push(format!("bundled rust-lld identity unavailable: {error}"));
        None
    });

    if rustc == "unknown" {
        limitations.push("rustc identity is unknown".to_string());
    }
    if cargo == "unknown" {
        limitations.push("Cargo identity is unknown".to_string());
    }
    if git_sha == "unknown" {
        limitations.push("Git HEAD identity is unknown".to_string());
    }
    if !tree_clean {
        limitations.push("source tree is not clean".to_string());
    }

    let mut environment = BTreeMap::new();
    for name in [
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "ImageOS",
        "ImageVersion",
        "RUNNER_OS",
        "RUNNER_ARCH",
        "MACOSX_DEPLOYMENT_TARGET",
        "CARGO_TARGET_DIR",
        "CARGO_INCREMENTAL",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        if let Ok(value) = std::env::var(name) {
            environment.insert(name.to_string(), value);
        }
    }

    Ok(SubjectIdentity {
        repository: REPOSITORY,
        git_sha,
        tree_clean,
        target: target.to_string(),
        host,
        workspace_version,
        cargo_lock_sha256,
        rustc,
        cargo,
        rust_lld,
        profile: "release",
        baseline_rustflags: baseline_rustflags.to_string(),
        candidate_rustflags: candidate_rustflags.to_string(),
        environment,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_variant(
    root: &Path,
    directory: &Path,
    archive: &Path,
    lsp_smoke: &Path,
    dap_smoke: &Path,
    source_sha: &str,
    label: &str,
    limitations: &mut Vec<String>,
) -> VariantEvidence {
    let directory = resolve_path(root, directory);
    let mut binaries = BTreeMap::new();
    for name in BINARY_NAMES {
        let path = directory.join(name);
        match measure_file(root, &path) {
            Ok(artifact) => {
                binaries.insert(name.to_string(), artifact);
            }
            Err(error) => {
                limitations.push(format!("{label} binary `{name}` unavailable: {error}"));
            }
        }
    }

    let archive_path = resolve_path(root, archive);
    let archive = match measure_archive(root, &archive_path, &binaries) {
        Ok(value) => Some(value),
        Err(error) => {
            limitations.push(format!("{label} archive evidence unavailable: {error}"));
            None
        }
    };

    let lsp_expected = binaries.get("perllsp");
    let dap_expected = binaries.get("perl-dap");
    let lsp_smoke = load_smoke(root, lsp_smoke, &format!("{label} LSP"), lsp_expected, limitations);
    let dap_smoke = load_smoke(root, dap_smoke, &format!("{label} DAP"), dap_expected, limitations);

    if !is_full_git_sha(source_sha) {
        limitations.push(format!("{label} artifacts declare no full 40-character source SHA"));
    }

    VariantEvidence {
        directory: display_path(root, &directory),
        source_sha: normalize_git_sha(source_sha),
        binaries,
        archive,
        lsp_smoke,
        dap_smoke,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compare_variants(
    baseline: &VariantEvidence,
    candidate: &VariantEvidence,
    subject: &SubjectIdentity,
    policy: &DecisionPolicy,
    target: &str,
    repeat_confirmed: bool,
    limitations: &mut Vec<String>,
) -> ComparisonEvidence {
    let mut binaries = BTreeMap::new();
    let mut baseline_total = 0_u64;
    let mut candidate_total = 0_u64;
    let mut structural_parity = true;
    let mut target_architecture_match = true;
    let mut component_growth_within_policy = true;

    for name in BINARY_NAMES {
        let Some(baseline_artifact) = baseline.binaries.get(name) else {
            structural_parity = false;
            continue;
        };
        let Some(candidate_artifact) = candidate.binaries.get(name) else {
            structural_parity = false;
            continue;
        };
        baseline_total = baseline_total.saturating_add(baseline_artifact.bytes);
        candidate_total = candidate_total.saturating_add(candidate_artifact.bytes);
        let delta = size_delta(baseline_artifact.bytes, candidate_artifact.bytes);
        if component_growth_exceeds(&delta, policy) {
            component_growth_within_policy = false;
        }
        if baseline_artifact.file_description.is_none()
            || candidate_artifact.file_description.is_none()
            || baseline_artifact.file_description != candidate_artifact.file_description
        {
            structural_parity = false;
            limitations.push(format!("`{name}` file identity differs or is unavailable"));
        }
        if !baseline_artifact
            .file_description
            .as_deref()
            .is_some_and(|value| target_matches_file_description(target, value))
            || !candidate_artifact
                .file_description
                .as_deref()
                .is_some_and(|value| target_matches_file_description(target, value))
        {
            target_architecture_match = false;
            limitations.push(format!("`{name}` does not match target `{target}`"));
        }
        binaries.insert(name.to_string(), delta);
    }

    let combined = size_delta(baseline_total, candidate_total);
    let archive = match (&baseline.archive, &candidate.archive) {
        (Some(left), Some(right)) => size_delta(left.artifact.bytes, right.artifact.bytes),
        _ => size_delta(0, 0),
    };
    let material_reduction = combined.reduction_bytes >= policy.minimum_reduction_bytes
        && combined.reduction_basis_points >= policy.minimum_reduction_basis_points;

    let source_identity_bound =
        source_identity_bound(&subject.git_sha, &baseline.source_sha, &candidate.source_sha);
    if !source_identity_bound {
        limitations.push(
            "baseline and candidate artifacts are not bound to one declared source SHA matching \
             the measured checkout"
                .to_string(),
        );
    }

    let repeat_satisfied =
        repeat_requirement_satisfied(&combined, policy, material_reduction, repeat_confirmed);
    if !repeat_satisfied {
        limitations.push(format!(
            "combined reduction of {} bp is below the {} bp repeat-confirmation threshold and no \
             confirming repeat measurement was declared",
            combined.reduction_basis_points, policy.repeat_required_below_basis_points
        ));
    }

    ComparisonEvidence {
        binaries,
        combined,
        archive,
        structural_parity,
        target_architecture_match,
        baseline_archive_identity: baseline
            .archive
            .as_ref()
            .is_some_and(|value| value.matches_directory),
        candidate_archive_identity: candidate
            .archive
            .as_ref()
            .is_some_and(|value| value.matches_directory),
        baseline_smokes_pass: smokes_pass(baseline),
        candidate_smokes_pass: smokes_pass(candidate),
        source_identity_bound,
        material_reduction,
        component_growth_within_policy,
        repeat_confirmed,
        repeat_requirement_satisfied: repeat_satisfied,
    }
}

/// Artifacts are source-bound only when both variants declare the same full
/// source SHA and that SHA is the checkout the receipt is labelled with.
///
/// This is a declaration check, not a cryptographic build attestation: it stops
/// a receipt from labelling artifacts with an unrelated checkout SHA, but it
/// cannot prove the declared SHA actually produced the measured bytes.
pub(crate) fn source_identity_bound(
    subject_sha: &str,
    baseline_sha: &str,
    candidate_sha: &str,
) -> bool {
    if !is_full_git_sha(subject_sha)
        || !is_full_git_sha(baseline_sha)
        || !is_full_git_sha(candidate_sha)
    {
        return false;
    }
    let subject = normalize_git_sha(subject_sha);
    subject == normalize_git_sha(baseline_sha) && subject == normalize_git_sha(candidate_sha)
}

fn is_full_git_sha(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 40 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_git_sha(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn recommend(
    subject: &SubjectIdentity,
    baseline: &VariantEvidence,
    candidate: &VariantEvidence,
    comparison: &ComparisonEvidence,
    target: &str,
    baseline_rustflags: &str,
    candidate_rustflags: &str,
    limitations: &mut Vec<String>,
) -> Recommendation {
    let complete_artifacts = BINARY_NAMES.iter().all(|name| {
        baseline.binaries.contains_key(*name) && candidate.binaries.contains_key(*name)
    }) && baseline.archive.is_some()
        && candidate.archive.is_some();
    let subject_complete = subject.tree_clean
        && subject.git_sha != "unknown"
        && subject.workspace_version != "unknown"
        && subject.cargo_lock_sha256 != "unknown"
        && subject.rustc != "unknown"
        && subject.cargo != "unknown"
        && subject.rust_lld.is_some();
    let facts = DecisionFacts {
        baseline_smokes_pass: comparison.baseline_smokes_pass,
        candidate_smoke_failed: matches!(candidate.lsp_smoke.status, SmokeStatus::Fail)
            || matches!(candidate.dap_smoke.status, SmokeStatus::Fail),
        candidate_smokes_pass: comparison.candidate_smokes_pass,
        structural_parity: comparison.structural_parity,
        target_architecture_match: comparison.target_architecture_match,
        baseline_archive_identity: comparison.baseline_archive_identity,
        candidate_archive_identity: comparison.candidate_archive_identity,
        baseline_smoke_identity: baseline.lsp_smoke.binary_matches
            && baseline.dap_smoke.binary_matches,
        candidate_smoke_identity: candidate.lsp_smoke.binary_matches
            && candidate.dap_smoke.binary_matches,
        complete_artifacts,
        subject_complete,
        source_identity_bound: comparison.source_identity_bound,
        governed_target: GOVERNED_TARGETS.contains(&target),
        // #5432 measures each target on its own native runner. A host that is
        // not the measured target means the artifacts were cross-built and the
        // smoke receipts cannot have run on the claimed platform.
        host_matches_target: subject.host == target,
        baseline_flags_clean: baseline_rustflags.trim().is_empty(),
        candidate_flags_exact: candidate_rustflags.trim() == SAFE_ICF_RUSTFLAGS,
        material_reduction: comparison.material_reduction,
        component_growth_within_policy: comparison.component_growth_within_policy,
        repeat_requirement_satisfied: comparison.repeat_requirement_satisfied,
    };
    let recommendation = decide(&facts);

    if !facts.baseline_smokes_pass {
        limitations.push(
            "baseline runtime smoke did not pass; candidate effect cannot be isolated".to_string(),
        );
    } else if facts.candidate_smoke_failed {
        limitations.push("candidate runtime smoke failed".to_string());
    } else if !facts.candidate_smokes_pass {
        limitations.push("candidate runtime smoke is missing or invalid".to_string());
    } else if !facts.target_architecture_match {
        limitations.push("measured binaries do not match the declared target".to_string());
    } else if !facts.structural_parity {
        limitations.push("baseline and candidate binary structures differ".to_string());
    } else if facts.baseline_smoke_identity && !facts.candidate_smoke_identity {
        limitations.push("candidate smoke is not bound to the measured binary".to_string());
    } else if facts.baseline_archive_identity && !facts.candidate_archive_identity {
        limitations
            .push("candidate archive does not contain the measured extracted binaries".to_string());
    } else if !facts.complete_artifacts
        || !facts.subject_complete
        || !facts.source_identity_bound
        || !facts.baseline_smoke_identity
        || !facts.candidate_smoke_identity
    {
        limitations.push("required artifact or subject identity is incomplete".to_string());
    } else if !facts.host_matches_target {
        limitations.push(format!(
            "measurement host `{}` is not the measured target `{target}`; safe-ICF evidence must \
             be produced on the native runner",
            subject.host
        ));
    } else if !facts.governed_target {
        limitations.push(format!(
            "safe-ICF adoption policy is limited to the governed targets {}",
            GOVERNED_TARGETS.join(", ")
        ));
    } else if !facts.baseline_flags_clean {
        limitations.push("baseline rustflags are not empty".to_string());
    } else if !facts.candidate_flags_exact {
        limitations
            .push("candidate rustflags do not match the governed safe-ICF policy".to_string());
    } else if !facts.component_growth_within_policy {
        limitations.push("one or more binaries exceed the component growth ceiling".to_string());
    }

    recommendation
}

fn measure_file(root: &Path, path: &Path) -> Result<FileArtifact> {
    let metadata =
        fs::metadata(path).with_context(|| format!("reading metadata for {}", path.display()))?;
    if !metadata.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    Ok(FileArtifact {
        path: display_path(root, path),
        bytes: metadata.len(),
        sha256: measure_sha256(path)?,
        file_description: capture_file_description(path),
    })
}

fn measure_archive(
    root: &Path,
    path: &Path,
    directory_binaries: &BTreeMap<String, FileArtifact>,
) -> Result<ArchiveEvidence> {
    if !path.to_string_lossy().ends_with(".tar.gz") {
        bail!("{} is not a .tar.gz archive", path.display());
    }
    let artifact = measure_file(root, path)?;
    let embedded_binaries = read_archive_binaries(path)?;
    let matches_directory = BINARY_NAMES.iter().all(|name| {
        let embedded = embedded_binaries.get(*name);
        let extracted = directory_binaries.get(*name);
        matches!(
            (embedded, extracted),
            (Some(left), Some(right))
                if left.sha256 == right.sha256 && left.bytes == right.bytes
        )
    });

    Ok(ArchiveEvidence { artifact, embedded_binaries, matches_directory })
}

fn read_archive_binaries(path: &Path) -> Result<BTreeMap<String, EmbeddedArtifact>> {
    let file = File::open(path).with_context(|| format!("opening archive {}", path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    let mut binaries = BTreeMap::new();

    for entry in archive.entries().context("reading archive entries")? {
        let mut entry = entry.context("reading archive entry")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry.path().context("reading archive entry path")?.into_owned();
        let Some(base_name) = entry_path.file_name().and_then(OsStr::to_str).map(ToOwned::to_owned)
        else {
            continue;
        };
        if !BINARY_NAMES.contains(&base_name.as_str()) {
            continue;
        }
        if binaries.contains_key(&base_name) {
            bail!("archive contains duplicate `{base_name}` entries");
        }
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let mut bytes_count = 0_u64;
        loop {
            let read = entry
                .read(&mut buffer)
                .with_context(|| format!("reading `{base_name}` from archive"))?;
            if read == 0 {
                break;
            }
            bytes_count = bytes_count
                .checked_add(u64::try_from(read).context("archive read length exceeds u64")?)
                .ok_or_else(|| eyre!("archive member length exceeds u64"))?;
            hasher.update(&buffer[..read]);
        }
        binaries.insert(
            base_name,
            EmbeddedArtifact { bytes: bytes_count, sha256: hex_lower(&hasher.finalize()) },
        );
    }

    for name in BINARY_NAMES {
        if !binaries.contains_key(name) {
            bail!("archive is missing required binary `{name}`");
        }
    }
    Ok(binaries)
}

fn load_smoke(
    root: &Path,
    path: &Path,
    label: &str,
    expected_binary: Option<&FileArtifact>,
    limitations: &mut Vec<String>,
) -> SmokeEvidence {
    let path = resolve_path(root, path);
    let display = display_path(root, &path);
    let raw = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) => {
            limitations.push(format!("{label} smoke receipt missing: {error}"));
            return SmokeEvidence {
                path: display,
                status: SmokeStatus::Missing,
                observed_status: None,
                binary: None,
                binary_matches: false,
            };
        }
    };
    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            limitations.push(format!("{label} smoke receipt is invalid JSON: {error}"));
            return SmokeEvidence {
                path: display,
                status: SmokeStatus::Invalid,
                observed_status: None,
                binary: None,
                binary_matches: false,
            };
        }
    };
    let shape: SmokeSuccessShape = match serde_json::from_value(value) {
        Ok(value) => value,
        Err(error) => {
            limitations.push(format!("{label} smoke receipt has unsupported shape: {error}"));
            return SmokeEvidence {
                path: display,
                status: SmokeStatus::Invalid,
                observed_status: None,
                binary: None,
                binary_matches: false,
            };
        }
    };
    let observed_status = shape
        .status
        .or(shape.outcome)
        .or_else(|| shape.success.map(|success| if success { "pass" } else { "fail" }.to_string()));
    let normalized_status = observed_status.as_deref().map(str::to_ascii_lowercase);
    let status = match normalized_status.as_deref() {
        Some("pass" | "passed" | "success" | "ok") => SmokeStatus::Pass,
        Some("fail" | "failed" | "error") => SmokeStatus::Fail,
        Some(_) | None => SmokeStatus::Invalid,
    };
    if status == SmokeStatus::Invalid {
        limitations.push(format!("{label} smoke receipt has no recognized terminal status"));
    }
    let binary = shape.binary.map(|value| normalize_reported_path(root, &value));
    let binary_matches = match (binary.as_deref(), expected_binary) {
        (Some(observed), Some(expected)) => observed == expected.path,
        _ => false,
    };
    if !binary_matches {
        limitations.push(format!("{label} smoke receipt is not bound to the measured binary path"));
    }
    SmokeEvidence { path: display, status, observed_status, binary, binary_matches }
}

fn normalize_reported_path(root: &Path, value: &str) -> String {
    let path = Path::new(value);
    display_path(root, &resolve_path(root, path))
}

/// Match only the exact governed triples. Prefix matching would accept an
/// arbitrary vendor triple such as `aarch64-anything-apple-darwin`.
fn target_matches_file_description(target: &str, description: &str) -> bool {
    match target {
        "aarch64-apple-darwin" => description.contains("arm64") || description.contains("aarch64"),
        "x86_64-apple-darwin" => description.contains("x86_64"),
        _ => false,
    }
}

fn workspace_version(root: &Path) -> Result<String> {
    let raw = fs::read_to_string(root.join("Cargo.toml")).context("reading root Cargo.toml")?;
    let value: toml::Value = raw.parse().context("parsing root Cargo.toml")?;
    value
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| eyre!("workspace.package.version is missing"))
}

fn parse_rustc_host(rustc_verbose: &str) -> Option<String> {
    rustc_verbose.lines().find_map(|line| line.strip_prefix("host: ").map(ToOwned::to_owned))
}

fn rust_lld_identity(host: &str) -> Result<Option<ToolIdentity>> {
    if host == "unknown" {
        return Ok(None);
    }
    let Some(sysroot) = capture("rustc", &["--print", "sysroot"]) else {
        return Ok(None);
    };
    let path =
        PathBuf::from(sysroot).join("lib").join("rustlib").join(host).join("bin").join("rust-lld");
    if !path.is_file() {
        return Ok(None);
    }
    let version = capture_path(&path, &["-flavor", "darwin", "--version"])
        .or_else(|| capture_path(&path, &["--version"]))
        .unwrap_or_else(|| "unknown".to_string());
    Ok(Some(ToolIdentity { version, sha256: measure_sha256(&path)? }))
}

fn capture(program: &str, args: &[&str]) -> Option<String> {
    capture_command(Command::new(program), args)
}

fn capture_in(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.current_dir(root);
    capture_command(command, args)
}

fn capture_path(program: &Path, args: &[&str]) -> Option<String> {
    capture_command(Command::new(program), args)
}

fn capture_command(mut command: Command, args: &[&str]) -> Option<String> {
    let output = command.args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn capture_file_description(path: &Path) -> Option<String> {
    let output = Command::new("file").arg("-b").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn measure_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

/// `sha2` digests are byte arrays with no `LowerHex` implementation; render
/// them the way the other xtask hashing paths already do.
fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        output.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        GOVERNED_TARGETS, hex_lower, source_identity_bound, target_matches_file_description,
    };
    use sha2::{Digest, Sha256};

    const SHA_A: &str = "0123456789abcdef0123456789abcdef01234567";
    const SHA_B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn source_identity_binds_only_when_every_declared_sha_agrees() {
        assert!(source_identity_bound(SHA_A, SHA_A, SHA_A));
        assert!(source_identity_bound(SHA_A, &SHA_A.to_ascii_uppercase(), SHA_A));
    }

    #[test]
    fn source_identity_is_unbound_when_variants_came_from_different_shas() {
        // Baseline from SHA A and candidate from SHA B, compared from a clean
        // checkout at a third SHA, must not earn an artifact-bound receipt.
        let checkout = "fedcba9876543210fedcba9876543210fedcba98";
        assert!(!source_identity_bound(checkout, SHA_A, SHA_B));
        assert!(!source_identity_bound(checkout, SHA_A, SHA_A));
        assert!(!source_identity_bound(SHA_A, SHA_A, SHA_B));
    }

    #[test]
    fn source_identity_is_unbound_without_a_full_sha() {
        assert!(!source_identity_bound(SHA_A, "", SHA_A));
        assert!(!source_identity_bound(SHA_A, "0123456", SHA_A));
        assert!(!source_identity_bound("unknown", SHA_A, SHA_A));
        assert!(!source_identity_bound(SHA_A, "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", SHA_A));
    }

    #[test]
    fn digests_render_as_lowercase_hex() {
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        // Known SHA-256 of the empty input.
        assert_eq!(
            hex_lower(&Sha256::new().finalize()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn adoption_is_restricted_to_the_exact_governed_triples() {
        assert!(GOVERNED_TARGETS.contains(&"aarch64-apple-darwin"));
        assert!(GOVERNED_TARGETS.contains(&"x86_64-apple-darwin"));
        assert!(!GOVERNED_TARGETS.contains(&"aarch64-anything-apple-darwin"));
        assert!(!GOVERNED_TARGETS.contains(&"aarch64-apple-ios"));
    }

    #[test]
    fn file_description_matching_rejects_ungoverned_triples() {
        assert!(target_matches_file_description(
            "aarch64-apple-darwin",
            "Mach-O 64-bit executable arm64"
        ));
        assert!(target_matches_file_description(
            "x86_64-apple-darwin",
            "Mach-O 64-bit executable x86_64"
        ));
        // Suffix/prefix matching previously accepted this fabricated triple.
        assert!(!target_matches_file_description(
            "aarch64-anything-apple-darwin",
            "Mach-O 64-bit executable arm64"
        ));
        assert!(!target_matches_file_description(
            "aarch64-unknown-linux-gnu",
            "ELF 64-bit LSB pie executable, ARM aarch64"
        ));
    }
}
