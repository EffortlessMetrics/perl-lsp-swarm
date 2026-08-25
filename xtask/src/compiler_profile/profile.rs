//! In-memory compiler profile definitions, imports, and closure validation.
//!
//! [`CompilerProfileDefinition`] owns a canonically ordered row map plus its
//! evidence records and limitations. [`CompilerProfileDefinition::validate`]
//! enforces the closure law locally: every required applicable row is
//! conjunctive across its declared proof axes, closed dispositions never
//! demand evidence, and no cross-satisfaction survives the class/family
//! matrix, stage floors, or work requirements.
//!
//! [`CompilerProfileDefinition::validate_closure`] additionally enforces the
//! import law against a caller-supplied registry: an importing profile binds
//! an exact lower-profile id/version/content digest and must preserve every
//! imported row and limitation unchanged.
//!
//! There is deliberately no readiness score anywhere in this module: validity
//! is a boolean closure result, never a weighted aggregate.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use super::CompilerProfileError;
use super::dimensions::ProofAxis;
use super::dimensions::encode_set;
use super::fingerprint::CanonWriter;
use super::fingerprint::CanonicalEncode;
use super::identity::CompilerProfileId;
use super::identity::CompilerProfileRowId;
use super::identity::CompilerProfileVersion;
use super::identity::ProfileContentDigest;
use super::requirements::AllowedLimitation;
use super::requirements::EvidenceRecord;
use super::rows::AxisRejection;
use super::rows::CompilerProfileRow;

/// Resolved dependency view supplied by callers for import validation.
/// Keys are exact `(profile id, version)` pairs.
pub type ProfileRegistry<'a> =
    BTreeMap<(CompilerProfileId, CompilerProfileVersion), &'a CompilerProfileDefinition>;

/// Reference to an exact lower profile: identity, version, and content digest.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompilerProfileImport {
    /// Imported profile identity.
    pub imported_profile: CompilerProfileId,
    /// Imported profile version.
    pub imported_version: CompilerProfileVersion,
    /// Exact semantic content digest the importer was built against.
    pub content_digest: ProfileContentDigest,
}

impl CompilerProfileImport {
    /// Validated constructor binding all three identity components exactly.
    pub fn new(
        imported_profile: CompilerProfileId,
        imported_version: CompilerProfileVersion,
        content_digest: ProfileContentDigest,
    ) -> Result<Self, CompilerProfileError> {
        Ok(Self { imported_profile, imported_version, content_digest })
    }
}

impl CanonicalEncode for CompilerProfileImport {
    fn encode(&self, writer: &mut CanonWriter) {
        self.imported_profile.encode(writer);
        self.imported_version.encode(writer);
        self.content_digest.encode(writer);
    }
}

/// A complete maintained compiler operating profile held purely in memory.
///
/// Construction is validated; [`CompilerProfileDefinition::validate`] remains
/// the authoritative closure check so that any post-construction mutation can
/// be caught by revalidation. All collections are ordered, so iteration and
/// fingerprints are independent of insertion order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerProfileDefinition {
    /// Stable profile identity.
    pub profile_id: CompilerProfileId,
    /// Profile version selector.
    pub version: CompilerProfileVersion,
    /// What claim boundary this profile expresses.
    pub purpose: String,
    /// Exact lower profiles imported by this profile.
    pub imports: BTreeSet<CompilerProfileImport>,
    /// Rows keyed by their own stable ids (canonical order).
    pub rows: BTreeMap<CompilerProfileRowId, CompilerProfileRow>,
    /// Observed evidence records backing declared axes.
    pub evidence: BTreeSet<EvidenceRecord>,
    /// Profile-level allowed limitations.
    pub limitations: BTreeSet<AllowedLimitation>,
}

impl CompilerProfileDefinition {
    /// Validated constructor. Enforces key/row agreement, evidence-to-spec
    /// pairing, and non-empty purpose. Disposition closure is enforced by
    /// [`CompilerProfileDefinition::validate`].
    pub fn new(
        profile_id: CompilerProfileId,
        version: CompilerProfileVersion,
        purpose: &str,
        imports: BTreeSet<CompilerProfileImport>,
        rows: BTreeMap<CompilerProfileRowId, CompilerProfileRow>,
        evidence: BTreeSet<EvidenceRecord>,
        limitations: BTreeSet<AllowedLimitation>,
    ) -> Result<Self, CompilerProfileError> {
        if purpose.trim().is_empty() {
            return Err(CompilerProfileError::Structure {
                message: "profile purpose must not be empty".to_string(),
            });
        }
        for (key, row) in &rows {
            if key != &row.row_id {
                return Err(CompilerProfileError::Identity {
                    message: format!(
                        "row map key {} does not match row id {}",
                        key.as_str(),
                        row.row_id.as_str()
                    ),
                });
            }
        }
        for record in &evidence {
            Self::pair_record(&rows, record)?;
        }
        Ok(Self {
            profile_id,
            version,
            purpose: purpose.to_string(),
            imports,
            rows,
            evidence,
            limitations,
        })
    }

    fn pair_record(
        rows: &BTreeMap<CompilerProfileRowId, CompilerProfileRow>,
        record: &EvidenceRecord,
    ) -> Result<(), CompilerProfileError> {
        let Some(row) = rows.get(&record.row_id) else {
            return Err(CompilerProfileError::Structure {
                message: format!(
                    "evidence record {} references unknown row {}",
                    record.record_id,
                    record.row_id.as_str()
                ),
            });
        };
        let Some(spec) = row.axis_specs.get(&record.axis) else {
            return Err(CompilerProfileError::Structure {
                message: format!(
                    "evidence record {} backs axis ({:?} at {:?}) that row {} never declares",
                    record.record_id,
                    record.axis.family,
                    record.axis.stage,
                    record.row_id.as_str()
                ),
            });
        };
        let rejection =
            spec.accept_record(record.class, record.tier, record.stage_observed, record.work);
        match rejection {
            Ok(()) => Ok(()),
            Err(reason) => Err(Self::rejection_error(record.row_id.as_str(), &record.axis, reason)),
        }
    }

    fn rejection_error(row: &str, axis: &ProofAxis, reason: AxisRejection) -> CompilerProfileError {
        let axis_name = format!("{:?} at {:?}", axis.family, axis.stage);
        match reason {
            AxisRejection::ClassNotAccepted => CompilerProfileError::CrossSatisfaction {
                row: row.to_string(),
                detail: format!("evidence cannot satisfy axis {axis_name}: class not accepted"),
            },
            AxisRejection::TierBelowFloor => CompilerProfileError::EvidenceTierBelowFloor {
                row: row.to_string(),
                detail: format!("provenance tier below the floor demanded by axis {axis_name}"),
            },
            AxisRejection::StageBelowFloor => CompilerProfileError::StageUnderflow {
                row: row.to_string(),
                detail: format!("observation stage below the floor demanded by axis {axis_name}"),
            },
            AxisRejection::WorkContextMismatch => CompilerProfileError::WorkMismatch {
                row: row.to_string(),
                detail: format!(
                    "work context does not match the context demanded by axis {axis_name}"
                ),
            },
            AxisRejection::WorkBelowMinimum => CompilerProfileError::WorkMismatch {
                row: row.to_string(),
                detail: format!("performed work below the minimum demanded by axis {axis_name}"),
            },
        }
    }

    /// Display name combining identity and version (`compiler_x.v1`).
    pub fn full_name(&self) -> String {
        format!("{}.{}", self.profile_id.as_str(), self.version.as_str())
    }

    /// Number of rows carried by this profile.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Look up one row by id.
    pub fn row(&self, row_id: &CompilerProfileRowId) -> Option<&CompilerProfileRow> {
        self.rows.get(row_id)
    }

    /// Deterministic 64-bit semantic fingerprint. Independent of insertion
    /// order; sensitive to every semantic field.
    pub fn semantic_fingerprint(&self) -> u64 {
        let mut writer = CanonWriter::new();
        self.encode(&mut writer);
        writer.finish_fingerprint()
    }

    /// Hex-rendered content digest of [`Self::semantic_fingerprint`].
    pub fn content_digest_hex(&self) -> String {
        ProfileContentDigest::from_fingerprint(self.semantic_fingerprint()).into_inner()
    }

    /// Authoritative local closure check. Required rows are conjunctive: each
    /// declared axis needs its own conforming evidence record, and general
    /// semantic support may never rest on source-stage-only observations.
    ///
    /// Scaffold note (commit 1): only structural checks are active here; the
    /// per-axis required-evidence closure lands with the domain-model commit.
    pub fn validate(&self) -> Result<(), CompilerProfileError> {
        if self.purpose.trim().is_empty() {
            return Err(CompilerProfileError::Structure {
                message: "profile purpose must not be empty".to_string(),
            });
        }
        for (key, row) in &self.rows {
            if key != &row.row_id {
                return Err(CompilerProfileError::Identity {
                    message: format!(
                        "row map key {} does not match row id {}",
                        key.as_str(),
                        row.row_id.as_str()
                    ),
                });
            }
            row.validate_row()?;
        }
        for record in &self.evidence {
            Self::pair_record(&self.rows, record)?;
        }
        Ok(())
    }

    /// Validates this profile plus its transitive import closure against
    /// `registry`: digests must match exactly and every imported row and
    /// limitation must be preserved verbatim.
    ///
    /// Scaffold note (commit 1): the closure law is pinned by the falsifiers
    /// in `tests/compiler_profile_contract.rs` and lands with the
    /// domain-model commit; this scaffold accepts any resolvable shape.
    pub fn validate_closure<'a>(
        &'a self,
        _registry: &ProfileRegistry<'a>,
    ) -> Result<(), CompilerProfileError> {
        self.validate()
    }
}

impl CanonicalEncode for CompilerProfileDefinition {
    fn encode(&self, writer: &mut CanonWriter) {
        self.profile_id.encode(writer);
        self.version.encode(writer);
        writer.str_field("purpose", &self.purpose);
        for import in &self.imports {
            writer.tag("import");
            import.encode(writer);
        }
        for row in self.rows.values() {
            writer.tag("row");
            row.encode(writer);
        }
        for record in &self.evidence {
            writer.tag("evidence");
            record.encode(writer);
        }
        writer.tag("limitations");
        encode_set(&self.limitations, writer);
    }
}
