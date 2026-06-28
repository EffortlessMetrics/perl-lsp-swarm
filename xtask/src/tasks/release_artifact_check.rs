//! Validate that produced release archives ship the binaries downstream DAP
//! consumers depend on (notably `perl-dap` alongside `perllsp`).
//!
//! The release workflow (`.github/workflows/release.yml`) builds and packages
//! both binaries, but nothing proves that contract holds for a given `dist/`
//! before the release is published. This check closes that gap: it reads the
//! contract from `docs/reference/downstream-dap-integrations.json` and verifies
//! every archive against it.
//!
//! Archive layout produced by `release.yml` (mirrored by the fixtures):
//! each `perllsp-<version>-<triple>{.tar.gz,.zip}` unpacks to a
//! `perllsp-<version>-<triple>/` directory containing the LSP and DAP binaries,
//! a per-archive `SHA256SUMS.txt`, and the license/readme files. A consolidated
//! top-level `SHA256SUMS` lists every archive by basename.

use color_eyre::eyre::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::utils::project_root;

const DEFAULT_CONTRACT_REL: &str = "docs/reference/downstream-dap-integrations.json";

/// CLI configuration for `cargo xtask release artifact-check`.
pub struct Config {
    /// Directory holding the release archives and consolidated `SHA256SUMS`.
    pub dist: PathBuf,
    /// Optional override for the contract JSON (defaults to the in-repo file).
    pub contract: Option<PathBuf>,
    /// If set, every archive name must contain this version string.
    pub version: Option<String>,
    /// Permit a `dist/` that does not cover every contract target triple.
    pub allow_partial: bool,
}

#[derive(Debug, Deserialize)]
struct Contract {
    archive_name_pattern: String,
    consolidated_checksums_file: String,
    platforms: BTreeMap<String, PlatformSpec>,
    targets: Vec<TargetSpec>,
}

#[derive(Debug, Deserialize)]
struct PlatformSpec {
    required_binaries: Vec<String>,
    ext: String,
    require_executable_bit: bool,
}

#[derive(Debug, Deserialize)]
struct TargetSpec {
    triple: String,
    platform: String,
}

/// A single file entry inside an archive (directories are filtered out).
#[derive(Debug, Clone)]
struct ArchiveEntry {
    /// Final path component, e.g. `perl-dap` from `perllsp-1.2.3-.../perl-dap`.
    base_name: String,
    /// Unix permission bits (0 when the archive does not record them, e.g. zip
    /// produced on Windows).
    mode: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct Violation {
    location: String,
    message: String,
}

pub fn run(cfg: Config) -> Result<()> {
    let root = project_root()?;
    let contract_path = cfg.contract.clone().unwrap_or_else(|| root.join(DEFAULT_CONTRACT_REL));
    let contract = load_contract(&contract_path)?;

    let violations =
        validate_dist(&cfg.dist, &contract, cfg.version.as_deref(), cfg.allow_partial)?;

    if violations.is_empty() {
        println!(
            "Release artifact check passed: every archive in {} ships the required binaries.",
            cfg.dist.display()
        );
        return Ok(());
    }

    eprintln!("RELEASE ARTIFACT VIOLATIONS:");
    eprintln!("{}", "=".repeat(60));
    for v in &violations {
        eprintln!("  {}: {}", v.location, v.message);
    }
    eprintln!("{}", "=".repeat(60));
    bail!("release artifact check failed with {} violation(s)", violations.len())
}

fn load_contract(path: &Path) -> Result<Contract> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading DAP integration contract {}", path.display()))?;
    let contract: Contract = serde_json::from_str(&text)
        .with_context(|| format!("parsing DAP integration contract {}", path.display()))?;
    Ok(contract)
}

/// Validate every archive in `dist` against `contract`. Returns the list of
/// violations (empty == pass). Returns `Err` only when validation cannot run
/// at all (missing `dist`, unreadable archive).
fn validate_dist(
    dist: &Path,
    contract: &Contract,
    expected_version: Option<&str>,
    allow_partial: bool,
) -> Result<Vec<Violation>> {
    if !dist.is_dir() {
        bail!("dist directory does not exist: {}", dist.display());
    }

    let mut violations = Vec::new();

    // Known extensions, longest first, so `.tar.gz` wins over a hypothetical `.gz`.
    let mut exts: Vec<&str> = contract.platforms.values().map(|p| p.ext.as_str()).collect();
    exts.sort_by_key(|e| std::cmp::Reverse(e.len()));
    exts.dedup();

    let triples: Vec<&str> = contract.targets.iter().map(|t| t.triple.as_str()).collect();

    // Collect the archives present in dist, tracked by triple.
    let mut seen_triples: BTreeSet<String> = BTreeSet::new();
    let mut archive_basenames: Vec<String> = Vec::new();

    let entries =
        fs::read_dir(dist).with_context(|| format!("reading dist directory {}", dist.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", dist.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((triple, ext, version)) = parse_archive_name(file_name, &triples, &exts) else {
            continue;
        };
        archive_basenames.push(file_name.to_string());

        if let Some(expected) = expected_version
            && version != expected
        {
            violations.push(Violation {
                location: file_name.to_string(),
                message: format!(
                    "archive version `{version}` does not match expected release version `{expected}`"
                ),
            });
        }

        let Some(platform) = platform_for_triple(contract, &triple) else {
            violations.push(Violation {
                location: file_name.to_string(),
                message: format!("triple `{triple}` is not declared in the contract"),
            });
            continue;
        };
        let Some(spec) = contract.platforms.get(platform) else {
            violations.push(Violation {
                location: file_name.to_string(),
                message: format!("platform `{platform}` is not declared in the contract"),
            });
            continue;
        };

        if ext != spec.ext {
            violations.push(Violation {
                location: file_name.to_string(),
                message: format!(
                    "archive extension `{ext}` does not match expected `{}` for {platform}",
                    spec.ext
                ),
            });
        }

        let archive_entries = read_archive_entries(&path, &ext)
            .with_context(|| format!("reading archive {}", path.display()))?;
        check_required_binaries(file_name, &archive_entries, spec, &mut violations);

        seen_triples.insert(triple);
    }

    // Completeness: every contract target should have an archive.
    if !allow_partial {
        for target in &contract.targets {
            if !seen_triples.contains(&target.triple) {
                let pattern = contract
                    .archive_name_pattern
                    .replace("{version}", expected_version.unwrap_or("<version>"))
                    .replace("{triple}", &target.triple)
                    .replace(
                        "{ext}",
                        contract
                            .platforms
                            .get(&target.platform)
                            .map(|p| p.ext.as_str())
                            .unwrap_or(""),
                    );
                violations.push(Violation {
                    location: dist.display().to_string(),
                    message: format!(
                        "no archive found for target `{}` (expected `{pattern}`)",
                        target.triple
                    ),
                });
            }
        }
    }

    if archive_basenames.is_empty() {
        violations.push(Violation {
            location: dist.display().to_string(),
            message: "no release archives found (expected perllsp-<version>-<triple>.{tar.gz,zip})"
                .to_string(),
        });
        return Ok(violations);
    }

    // Consolidated checksums: every archive must be listed (and, when the
    // digest is present, it must match the file on disk).
    check_consolidated_checksums(dist, contract, &archive_basenames, &mut violations)?;

    Ok(violations)
}

/// Parse `perllsp-<version>-<triple><ext>` into `(triple, ext, version)`.
/// Returns `None` for files that are not release archives.
fn parse_archive_name(
    file_name: &str,
    triples: &[&str],
    exts: &[&str],
) -> Option<(String, String, String)> {
    const PREFIX: &str = "perllsp-";
    let ext = exts.iter().copied().find(|e| file_name.ends_with(*e))?;
    let core = file_name.strip_suffix(ext)?;
    let rest = core.strip_prefix(PREFIX)?; // "<version>-<triple>"

    // Triples contain hyphens, so match the longest known triple suffix that is
    // preceded by a `-`.
    let mut best: Option<&str> = None;
    for triple in triples {
        if let Some(version_part) = rest.strip_suffix(triple)
            && version_part.ends_with('-')
            && best.is_none_or(|b| triple.len() > b.len())
        {
            best = Some(triple);
        }
    }
    let triple = best?;
    let version = rest.strip_suffix(triple)?.strip_suffix('-')?;
    if version.is_empty() {
        return None;
    }
    Some((triple.to_string(), ext.to_string(), version.to_string()))
}

fn platform_for_triple<'a>(contract: &'a Contract, triple: &str) -> Option<&'a str> {
    contract.targets.iter().find(|t| t.triple == triple).map(|t| t.platform.as_str())
}

fn check_required_binaries(
    location: &str,
    entries: &[ArchiveEntry],
    spec: &PlatformSpec,
    violations: &mut Vec<Violation>,
) {
    for required in &spec.required_binaries {
        let matches: Vec<&ArchiveEntry> =
            entries.iter().filter(|e| &e.base_name == required).collect();
        if matches.is_empty() {
            violations.push(Violation {
                location: location.to_string(),
                message: format!("missing required binary `{required}`"),
            });
            continue;
        }
        if spec.require_executable_bit && !matches.iter().any(|e| e.mode & 0o111 != 0) {
            violations.push(Violation {
                location: location.to_string(),
                message: format!("binary `{required}` is present but not marked executable"),
            });
        }
    }
}

fn check_consolidated_checksums(
    dist: &Path,
    contract: &Contract,
    archive_basenames: &[String],
    violations: &mut Vec<Violation>,
) -> Result<()> {
    let checksums_path = dist.join(&contract.consolidated_checksums_file);
    if !checksums_path.is_file() {
        violations.push(Violation {
            location: dist.display().to_string(),
            message: format!("consolidated `{}` is missing", contract.consolidated_checksums_file),
        });
        return Ok(());
    }

    let text = fs::read_to_string(&checksums_path)
        .with_context(|| format!("reading {}", checksums_path.display()))?;
    let listed = parse_checksums(&text);

    for name in archive_basenames {
        match listed.get(name) {
            None => violations.push(Violation {
                location: contract.consolidated_checksums_file.clone(),
                message: format!("archive `{name}` is not listed in the consolidated checksums"),
            }),
            Some(expected_digest) => {
                let actual =
                    sha256_hex(&dist.join(name)).with_context(|| format!("hashing {name}"))?;
                if !expected_digest.eq_ignore_ascii_case(&actual) {
                    violations.push(Violation {
                        location: contract.consolidated_checksums_file.clone(),
                        message: format!(
                            "checksum mismatch for `{name}` (listed {expected_digest}, actual {actual})"
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

/// Parse `sha256sum`-style lines (`<hex>  <name>`) into a name -> digest map.
fn parse_checksums(text: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(digest) = parts.next() else { continue };
        let Some(name) = parts.next() else { continue };
        // Drop the binary-mode `*` marker and any leading `./`.
        let name = name.trim().trim_start_matches('*').trim_start_matches("./");
        map.insert(name.to_string(), digest.to_string());
    }
    map
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("opening {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 0x0f), 16).unwrap_or('0'));
    }
    s
}

fn read_archive_entries(path: &Path, ext: &str) -> Result<Vec<ArchiveEntry>> {
    if ext.ends_with(".zip") { read_zip_entries(path) } else { read_tar_gz_entries(path) }
}

fn read_tar_gz_entries(path: &Path) -> Result<Vec<ArchiveEntry>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut out = Vec::new();
    for entry in archive.entries().context("iterating tar entries")? {
        let entry = entry.context("reading tar entry")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let mode = entry.header().mode().unwrap_or(0);
        let path_in_tar = entry.path().context("decoding tar entry path")?;
        if let Some(base) = path_in_tar.file_name().and_then(|n| n.to_str()) {
            out.push(ArchiveEntry { base_name: base.to_string(), mode });
        }
    }
    Ok(out)
}

/// Read a ZIP central directory and return its file entries. Only the central
/// directory is parsed (entry names + external attributes); file data is never
/// decompressed. ZIP64 archives are not supported (release archives are small).
fn read_zip_entries(path: &Path) -> Result<Vec<ArchiveEntry>> {
    let data = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const CD_SIG: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];

    // Locate the End Of Central Directory record by scanning backwards.
    if data.len() < 22 {
        bail!("file too small to be a zip archive: {}", path.display());
    }
    let max_back = data.len().saturating_sub(22);
    let mut eocd: Option<usize> = None;
    for i in (0..=max_back).rev() {
        if data[i..i + 4] == EOCD_SIG {
            eocd = Some(i);
            break;
        }
    }
    let Some(eocd) = eocd else {
        bail!("zip End Of Central Directory record not found: {}", path.display());
    };

    let total_entries = read_u16(&data, eocd + 10)?;
    let cd_offset = read_u32(&data, eocd + 16)? as usize;

    let mut out = Vec::new();
    let mut pos = cd_offset;
    for _ in 0..total_entries {
        if pos + 46 > data.len() || data[pos..pos + 4] != CD_SIG {
            break;
        }
        let external_attrs = read_u32(&data, pos + 38)?;
        let name_len = read_u16(&data, pos + 28)? as usize;
        let extra_len = read_u16(&data, pos + 30)? as usize;
        let comment_len = read_u16(&data, pos + 32)? as usize;
        let name_start = pos + 46;
        let name_end = name_start + name_len;
        if name_end > data.len() {
            break;
        }
        let raw_name = String::from_utf8_lossy(&data[name_start..name_end]);
        let normalized = raw_name.replace('\\', "/");
        // Directory entries end in `/`.
        if !normalized.ends_with('/') {
            let base = normalized.rsplit('/').next().unwrap_or(&normalized).to_string();
            let mode = (external_attrs >> 16) & 0xffff;
            out.push(ArchiveEntry { base_name: base, mode });
        }
        pos = name_end + extra_len + comment_len;
    }
    Ok(out)
}

fn read_u16(data: &[u8], at: usize) -> Result<u16> {
    match data.get(at..at + 2) {
        Some([a, b]) => Ok(u16::from_le_bytes([*a, *b])),
        _ => bail!("zip: truncated u16 field at offset {at}"),
    }
}

fn read_u32(data: &[u8], at: usize) -> Result<u32> {
    match data.get(at..at + 4) {
        Some([a, b, c, d]) => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => bail!("zip: truncated u32 field at offset {at}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_contract() -> Contract {
        let json = r#"{
            "archive_name_pattern": "perllsp-{version}-{triple}{ext}",
            "consolidated_checksums_file": "SHA256SUMS",
            "platforms": {
                "unix": { "required_binaries": ["perllsp", "perl-dap"], "ext": ".tar.gz", "require_executable_bit": true },
                "windows": { "required_binaries": ["perllsp.exe", "perl-dap.exe"], "ext": ".zip", "require_executable_bit": false }
            },
            "targets": [
                { "triple": "x86_64-unknown-linux-gnu", "platform": "unix" },
                { "triple": "x86_64-pc-windows-msvc", "platform": "windows" }
            ]
        }"#;
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn parses_unix_archive_name() {
        let triples = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
        let exts = [".tar.gz", ".zip"];
        let parsed =
            parse_archive_name("perllsp-0.16.0-x86_64-unknown-linux-gnu.tar.gz", &triples, &exts);
        assert_eq!(
            parsed,
            Some((
                "x86_64-unknown-linux-gnu".to_string(),
                ".tar.gz".to_string(),
                "0.16.0".to_string()
            ))
        );
    }

    #[test]
    fn parses_windows_zip_name() {
        let triples = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
        let exts = [".tar.gz", ".zip"];
        let parsed =
            parse_archive_name("perllsp-1.2.3-x86_64-pc-windows-msvc.zip", &triples, &exts);
        assert_eq!(
            parsed,
            Some(("x86_64-pc-windows-msvc".to_string(), ".zip".to_string(), "1.2.3".to_string()))
        );
    }

    #[test]
    fn ignores_non_archive_files() {
        let triples = ["x86_64-unknown-linux-gnu"];
        let exts = [".tar.gz", ".zip"];
        assert_eq!(parse_archive_name("SHA256SUMS", &triples, &exts), None);
        assert_eq!(parse_archive_name("README.md", &triples, &exts), None);
        assert_eq!(
            parse_archive_name("perllsp-0.1.0-unknown-triple.tar.gz", &triples, &exts),
            None
        );
    }

    #[test]
    fn missing_dap_binary_is_a_violation() {
        let spec = PlatformSpec {
            required_binaries: vec!["perllsp".to_string(), "perl-dap".to_string()],
            ext: ".tar.gz".to_string(),
            require_executable_bit: true,
        };
        let entries = vec![ArchiveEntry { base_name: "perllsp".to_string(), mode: 0o755 }];
        let mut violations = Vec::new();
        check_required_binaries("ok.tar.gz", &entries, &spec, &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("perl-dap"));
    }

    #[test]
    fn non_executable_unix_binary_is_a_violation() {
        let spec = PlatformSpec {
            required_binaries: vec!["perl-dap".to_string()],
            ext: ".tar.gz".to_string(),
            require_executable_bit: true,
        };
        let entries = vec![ArchiveEntry { base_name: "perl-dap".to_string(), mode: 0o644 }];
        let mut violations = Vec::new();
        check_required_binaries("ok.tar.gz", &entries, &spec, &mut violations);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("not marked executable"));
    }

    #[test]
    fn windows_binary_does_not_require_exec_bit() {
        let spec = PlatformSpec {
            required_binaries: vec!["perl-dap.exe".to_string()],
            ext: ".zip".to_string(),
            require_executable_bit: false,
        };
        let entries = vec![ArchiveEntry { base_name: "perl-dap.exe".to_string(), mode: 0 }];
        let mut violations = Vec::new();
        check_required_binaries("ok.zip", &entries, &spec, &mut violations);
        assert!(violations.is_empty());
    }

    #[test]
    fn parse_checksums_handles_modes_and_prefixes() {
        let text = "abc123  perllsp-1.0.0-x86_64-unknown-linux-gnu.tar.gz\n\
                    def456 *perllsp-1.0.0-x86_64-pc-windows-msvc.zip\n";
        let map = parse_checksums(text);
        assert_eq!(
            map.get("perllsp-1.0.0-x86_64-unknown-linux-gnu.tar.gz"),
            Some(&"abc123".to_string())
        );
        assert_eq!(
            map.get("perllsp-1.0.0-x86_64-pc-windows-msvc.zip"),
            Some(&"def456".to_string())
        );
    }

    #[test]
    fn contract_platforms_present() {
        let contract = test_contract();
        assert!(contract.platforms.contains_key("unix"));
        assert!(contract.platforms.contains_key("windows"));
        assert_eq!(platform_for_triple(&contract, "x86_64-pc-windows-msvc"), Some("windows"));
    }
}
