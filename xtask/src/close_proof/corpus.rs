//! Immutable offline regression corpus for the close-proof schema train.
//!
//! Fixtures live under `.ci/close-proof-contract/fixtures/` and are addressed
//! by a content-addressed manifest (`corpus_manifest.json`). Verification:
//!
//! 1. the fixture file set matches the manifest membership exactly;
//! 2. every file's SHA-256 equals its manifest digest;
//! 3. every fixture parses against the strict schemas;
//! 4. each case's packet validates against its contract and the recorded
//!    packet verdict agrees with the fixture's expected outcomes.
//!
//! Note on step 4: agreement between a fixture's expectations and its own
//! recorded verdict proves internal consistency and schema validity only.
//! Independently recomputing those outcomes is the CP03 evaluator's job
//! (#10382), which replays this same corpus against its own decisions.
//!
//! Honest immutability boundary: a writer who regenerates both a fixture and
//! its manifest can still produce a self-consistent tree; repository history
//! remains the final authority over reviewed mutation. What this layer proves
//! is accidental-corruption resistance and deterministic regeneration (a
//! second generation produces no diff).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use super::contract::IssueContract;
use super::packet::validate_packet_against_contract;
use super::{
    CloseProofError, ISSUE_CONTRACT_SCHEMA_V1, IssueCloseOutcome, PrScopeOutcome, canonical_json,
    content_digest_hex, corpus_root, is_digest_hex, is_stable_token,
};
use crate::close_proof::model::{CLOSE_PACKET_SCHEMA_V1, ClosePacket};

pub const FIXTURE_SCHEMA_V1: &str = "close_proof_contract_fixture.v1";
pub const CORPUS_MANIFEST_SCHEMA_V1: &str = "close_proof_corpus_manifest.v1";

/// Bounded captured context proving where a historical subject came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureProvenance {
    pub captured_at: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub subject_shas: Vec<String>,
    pub boundary: String,
}

/// One close attempt recorded against the fixture's contract, with expected
/// outcomes for both independent result surfaces.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureCase {
    pub case_id: String,
    pub description: String,
    pub packet: ClosePacket,
    pub expected_pr_scope: PrScopeOutcome,
    pub expected_issue_close: IssueCloseOutcome,
}

/// A regression fixture document: one issue contract plus recorded close
/// attempts and their expected dispositions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureDocument {
    pub schema_version: String,
    pub provenance: FixtureProvenance,
    pub contract: IssueContract,
    pub cases: Vec<FixtureCase>,
}

impl FixtureDocument {
    pub fn from_json_str(json: &str) -> Result<Self, CloseProofError> {
        serde_json::from_str(json).map_err(|error| CloseProofError::Schema {
            field: "close_proof_contract_fixture".to_string(),
            message: error.to_string(),
        })
    }

    /// Deterministic serialization; a second generation produces no diff.
    pub fn to_canonical_json(&self) -> Result<String, CloseProofError> {
        canonical_json(self)
    }

    /// Validate the document's own integrity: strict schema versions, bounded
    /// provenance, unique case IDs, valid contract, and every case packet
    /// validating against that contract with verdicts matching expectations.
    pub fn verify(&self) -> Result<(), CloseProofError> {
        if self.schema_version != FIXTURE_SCHEMA_V1 {
            return Err(CloseProofError::Schema {
                field: "schema_version".to_string(),
                message: format!("expected `{FIXTURE_SCHEMA_V1}`, found `{}`", self.schema_version),
            });
        }
        if self.provenance.captured_at.trim().is_empty()
            || self.provenance.boundary.trim().is_empty()
        {
            return Err(CloseProofError::Schema {
                field: "provenance".to_string(),
                message: "captured_at and boundary are required".to_string(),
            });
        }
        for source in self.provenance.sources.iter().chain(&self.provenance.subject_shas) {
            if source.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "provenance.sources".to_string(),
                    message: "provenance entries must not be empty".to_string(),
                });
            }
        }
        if self.contract.schema_version != ISSUE_CONTRACT_SCHEMA_V1 {
            return Err(CloseProofError::Schema {
                field: "contract.schema_version".to_string(),
                message: format!(
                    "expected `{ISSUE_CONTRACT_SCHEMA_V1}`, found `{}`",
                    self.contract.schema_version
                ),
            });
        }
        if self.cases.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "fixtures must record at least one close-attempt case".to_string(),
            });
        }
        let mut seen_cases = std::collections::BTreeSet::new();
        for case in &self.cases {
            if !is_stable_token(&case.case_id) {
                return Err(CloseProofError::Schema {
                    field: "cases.case_id".to_string(),
                    message: format!("`{}` is not a stable case token", case.case_id),
                });
            }
            if !seen_cases.insert(case.case_id.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!("duplicate case id `{}`", case.case_id),
                });
            }
            if case.description.trim().is_empty() {
                return Err(CloseProofError::Schema {
                    field: "cases.description".to_string(),
                    message: format!("case `{}` has an empty description", case.case_id),
                });
            }
            if case.packet.schema_version != CLOSE_PACKET_SCHEMA_V1 {
                return Err(CloseProofError::Schema {
                    field: "cases.packet.schema_version".to_string(),
                    message: format!(
                        "expected `{CLOSE_PACKET_SCHEMA_V1}`, found `{}`",
                        case.packet.schema_version
                    ),
                });
            }
            validate_packet_against_contract(&case.packet, &self.contract)?;
            if case.packet.verdict.pr_scope != case.expected_pr_scope {
                return Err(CloseProofError::Corpus {
                    message: format!(
                        "case `{}` expects pr_scope {:?} but its packet records {:?}",
                        case.case_id, case.expected_pr_scope, case.packet.verdict.pr_scope
                    ),
                });
            }
            if case.packet.verdict.issue_close != case.expected_issue_close {
                return Err(CloseProofError::Corpus {
                    message: format!(
                        "case `{}` expects issue_close {:?} but its packet records {:?}",
                        case.case_id, case.expected_issue_close, case.packet.verdict.issue_close
                    ),
                });
            }
        }
        Ok(())
    }
}

/// One content-addressed corpus member.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    pub file: String,
    pub sha256: String,
}

/// Content-addressed membership roster for the immutable regression corpus.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusManifest {
    pub schema_version: String,
    pub corpus_id: String,
    pub fixtures: Vec<ManifestEntry>,
}

impl CorpusManifest {
    pub fn from_json_str(json: &str) -> Result<Self, CloseProofError> {
        serde_json::from_str(json).map_err(|error| CloseProofError::Schema {
            field: "corpus_manifest".to_string(),
            message: error.to_string(),
        })
    }

    pub fn validate(&self) -> Result<(), CloseProofError> {
        if self.schema_version != CORPUS_MANIFEST_SCHEMA_V1 {
            return Err(CloseProofError::Schema {
                field: "schema_version".to_string(),
                message: format!(
                    "expected `{CORPUS_MANIFEST_SCHEMA_V1}`, found `{}`",
                    self.schema_version
                ),
            });
        }
        if !is_stable_token(&self.corpus_id) {
            return Err(CloseProofError::Schema {
                field: "corpus_id".to_string(),
                message: format!("`{}` is not a stable corpus token", self.corpus_id),
            });
        }
        if self.fixtures.is_empty() {
            return Err(CloseProofError::Coverage {
                message: "the corpus manifest must list at least one fixture".to_string(),
            });
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in &self.fixtures {
            if !entry.file.starts_with("fixtures/") || !entry.file.ends_with(".json") {
                return Err(CloseProofError::Schema {
                    field: "fixtures.file".to_string(),
                    message: format!("`{}` must address `fixtures/*.json`", entry.file),
                });
            }
            if !seen.insert(entry.file.as_str()) {
                return Err(CloseProofError::Coverage {
                    message: format!("duplicate manifest entry `{}`", entry.file),
                });
            }
            if !is_digest_hex(&entry.sha256) {
                return Err(CloseProofError::Digest {
                    message: format!(
                        "manifest digest for `{}` is not 64 lowercase hex",
                        entry.file
                    ),
                });
            }
        }
        Ok(())
    }
}

/// Load and validate `corpus_manifest.json` from the corpus root.
pub fn load_corpus_manifest() -> Result<CorpusManifest, CloseProofError> {
    let path = corpus_root().join("corpus_manifest.json");
    let raw = read_file(&path)?;
    let manifest = CorpusManifest::from_json_str(&raw)?;
    manifest.validate()?;
    Ok(manifest)
}

/// Verify the whole regression corpus; returns the number of fixtures checked.
pub fn verify_corpus() -> Result<usize, CloseProofError> {
    let manifest = load_corpus_manifest()?;
    verify_corpus_at(&corpus_root(), &manifest)
}

pub(crate) fn verify_corpus_at(
    root: &Path,
    manifest: &CorpusManifest,
) -> Result<usize, CloseProofError> {
    manifest.validate()?;
    let fixtures_dir = root.join("fixtures");
    let mut on_disk: Vec<String> = fs::read_dir(&fixtures_dir)
        .map_err(|error| CloseProofError::Corpus {
            message: format!("cannot read corpus fixtures directory: {error}"),
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .map(|name| format!("fixtures/{name}"))
        .collect();
    on_disk.sort();
    let mut listed: Vec<&str> = manifest.fixtures.iter().map(|entry| entry.file.as_str()).collect();
    listed.sort_unstable();
    if on_disk.iter().map(String::as_str).collect::<Vec<_>>() != listed {
        return Err(CloseProofError::Corpus {
            message: format!(
                "corpus membership drifted from the manifest: disk has [{}], manifest lists [{}]",
                on_disk.join(", "),
                listed.join(", ")
            ),
        });
    }

    for entry in &manifest.fixtures {
        let path = root.join(&entry.file);
        let raw = read_file(&path)?;
        let actual_digest = sha256_file(raw.as_bytes());
        if actual_digest != entry.sha256 {
            return Err(CloseProofError::Corpus {
                message: format!(
                    "fixture `{}` does not match its immutable manifest digest",
                    entry.file
                ),
            });
        }
        let document = FixtureDocument::from_json_str(&raw)?;
        document.verify()?;
        // Deterministic second generation: re-serializing what was parsed must
        // reproduce the committed bytes exactly.
        let regenerated = document.to_canonical_json()?;
        if regenerated != normalize_newlines(&raw) {
            return Err(CloseProofError::Corpus {
                message: format!(
                    "fixture `{}` is not canonically serialized; regeneration would diff",
                    entry.file
                ),
            });
        }
    }
    Ok(manifest.fixtures.len())
}

pub(crate) fn read_file(path: &Path) -> Result<String, CloseProofError> {
    fs::read_to_string(path).map_err(|error| CloseProofError::Corpus {
        message: format!("cannot read `{}`: {error}", path.display()),
    })
}

fn sha256_file(bytes: &[u8]) -> String {
    content_digest_hex(bytes)
}

fn normalize_newlines(raw: &str) -> String {
    raw.replace("\r\n", "\n")
}
