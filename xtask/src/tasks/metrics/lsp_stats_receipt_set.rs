//! Private, invocation-local compatibility receipt snapshot. This is not an
//! exact-run plan, durable receipt format, or verified filesystem bundle.

use color_eyre::eyre::{Context, Result, bail};
use perl_lsp_ux_tests::recorder::UxScenarioRunReceipt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub(super) enum ReceiptFamily {
    UxScenarioRun,
    GoldenEditorWorkload,
    UnrelatedJson,
}

pub(super) struct ClassifiedReceipt {
    pub family: ReceiptFamily,
    pub receipt: Option<UxScenarioRunReceipt>,
}

struct CapturedFile {
    path: PathBuf,
    size: u64,
    digest: [u8; 32],
    family: ReceiptFamily,
}

#[derive(Debug, PartialEq, Eq)]
struct CompatibilitySubject {
    sha: String,
    platform: String,
    tier: String,
    hosted: Option<(String, u32)>,
}

type LogicalCaseKey = (String, String, String, String);

pub(super) struct AdmittedLegacyUxReceiptSet {
    root: PathBuf,
    files: Vec<CapturedFile>,
    receipts: Vec<UxScenarioRunReceipt>,
    subject: Option<CompatibilitySubject>,
    case_keys: BTreeSet<LogicalCaseKey>,
}

/// The path set changed, or a selected file could not be captured. Callers must
/// retry as a new invocation, never fall back to the historical directory reader.
#[derive(Debug)]
pub(super) struct UnstableReceiptInput {
    root: PathBuf,
}

impl fmt::Display for UnstableReceiptInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unstable compatibility receipt input: {}", self.root.display())
    }
}

impl std::error::Error for UnstableReceiptInput {}

impl AdmittedLegacyUxReceiptSet {
    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn receipts(&self) -> &[UxScenarioRunReceipt] {
        &self.receipts
    }

    /// Hash ordered names, lengths, captured bytes and explicit dispositions.
    /// The root is retained separately so an enumeration permutation cannot
    /// change the digest, and moving identical input does not change its content.
    pub(super) fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"legacy-ux-receipt-snapshot\0");
        for file in &self.files {
            let name = file.path.as_os_str().as_encoded_bytes();
            hash.update((name.len() as u64).to_be_bytes());
            hash.update(name);
            hash.update(file.size.to_be_bytes());
            hash.update(file.digest);
            hash.update([match file.family {
                ReceiptFamily::UxScenarioRun => 0,
                ReceiptFamily::GoldenEditorWorkload => 1,
                ReceiptFamily::UnrelatedJson => 2,
            }]);
        }
        hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub(super) fn describe(&self) -> String {
        let subject = self
            .subject
            .as_ref()
            .map_or_else(|| "no UX subject".to_string(), |subject| format!("subject {subject:?}"));
        format!(
            "{} files, {} observed cases, {subject}, digest {}; v1 predeclared completeness is not proven",
            self.files.len(),
            self.case_keys.len(),
            self.digest()
        )
    }
}

pub(super) fn load(
    root: &Path,
    classify: impl FnMut(&Path, Value) -> Result<ClassifiedReceipt>,
) -> Result<AdmittedLegacyUxReceiptSet> {
    load_with_source(root, || selected_paths(root), |path| Ok(fs::read(path)?), classify)
}

/// The same acquisition algorithm is used by production and deterministic
/// mutation/read-count tests. The injected operations are private to this path.
pub(super) fn load_with_source(
    root: &Path,
    mut list: impl FnMut() -> Result<Vec<PathBuf>>,
    mut read: impl FnMut(&Path) -> Result<Vec<u8>>,
    mut classify: impl FnMut(&Path, Value) -> Result<ClassifiedReceipt>,
) -> Result<AdmittedLegacyUxReceiptSet> {
    let mut paths = list()?;
    paths.sort();
    let mut admitted = AdmittedLegacyUxReceiptSet {
        root: root.to_path_buf(),
        files: Vec::new(),
        receipts: Vec::new(),
        subject: None,
        case_keys: BTreeSet::new(),
    };
    for path in &paths {
        let raw = read(path)
            .with_context(|| format!("capturing receipt candidate: {}", path.display()))
            .map_err(|error| error.wrap_err(UnstableReceiptInput { root: root.to_path_buf() }))?;
        let value: Value = serde_json::from_slice(&raw)
            .with_context(|| format!("parsing JSON receipt candidate: {}", path.display()))?;
        let classified = classify(path, value)?;
        if let Some(receipt) = classified.receipt {
            let subject = subject_of(&receipt, path)?;
            if let Some(expected) = &admitted.subject {
                if expected != &subject {
                    bail!(
                        "mixed compatibility receipt subject in {}: expected {expected:?}, got {subject:?}",
                        path.display()
                    );
                }
            } else {
                admitted.subject = Some(subject);
            }
            let key = (
                receipt.workflow_id.clone(),
                receipt.scenario_file.clone(),
                receipt.test_name.clone(),
                serde_json::to_string(&receipt.ci_tier)?,
            );
            if !admitted.case_keys.insert(key) {
                bail!(
                    "duplicate logical UX case in {}: workflow `{}` scenario `{}` test `{}`",
                    path.display(),
                    receipt.workflow_id,
                    receipt.scenario_file,
                    receipt.test_name
                );
            }
            admitted.receipts.push(receipt);
        }
        admitted.files.push(CapturedFile {
            path: path.strip_prefix(root)?.to_path_buf(),
            size: raw.len() as u64,
            digest: Sha256::digest(&raw).into(),
            family: classified.family,
        });
    }
    let mut closing_paths = list()
        .map_err(|error| error.wrap_err(UnstableReceiptInput { root: root.to_path_buf() }))?;
    closing_paths.sort();
    if paths != closing_paths {
        return Err(UnstableReceiptInput { root: root.to_path_buf() }.into());
    }
    Ok(admitted)
}

pub(super) fn selected_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading receipts directory: {}", root.display()));
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| "reading receipt directory entry")?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            if !entry.file_type()?.is_file() {
                bail!("receipt candidate must be a regular file: {}", path.display());
            }
            paths.push(path);
        }
    }
    Ok(paths)
}

fn subject_of(receipt: &UxScenarioRunReceipt, path: &Path) -> Result<CompatibilitySubject> {
    let identity = &receipt.run_identity;
    let required = |value: &Option<String>, field: &str| -> Result<String> {
        match value {
            Some(value) if !value.trim().is_empty() => Ok(value.clone()),
            _ => bail!(
                "compatibility receipt identity not proven: missing or empty {field} in {}",
                path.display()
            ),
        }
    };
    let hosted = match (&identity.run_id, identity.attempt) {
        (None, None) => None,
        (Some(run), Some(attempt)) if !run.trim().is_empty() => Some((run.clone(), attempt)),
        _ => bail!(
            "partial hosted compatibility identity in {}: run_id and attempt must both be present or both absent",
            path.display()
        ),
    };
    Ok(CompatibilitySubject {
        sha: required(&identity.sha, "run_identity.sha")?,
        platform: required(&identity.platform, "run_identity.platform")?,
        tier: serde_json::to_string(&receipt.ci_tier)?,
        hosted,
    })
}
