//! Historical `perl_kwalitee.v1` compatibility and migration authority.
//!
//! The current crate name predates the separation between CPAN distribution
//! Kwalitee and repository/product release readiness. This module freezes that
//! historical meaning, validates a one-to-one migration ledger for every
//! legacy indicator, and provides the only supported reader for old receipts.
//!
//! New distribution-Kwalitee work must not add indicators to this frozen
//! catalog or reinterpret these receipts as `distribution_kwalitee`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::indicator::{
    CATALOG, EvalSource, EvidenceRef, IndicatorScope, IndicatorSpec, IndicatorStatus,
    KwaliteeIndicator,
};
use crate::profile::KwaliteeProfile;
use crate::receipt::{KwaliteeReceipt, KwaliteeVerdict, RECEIPT_KIND, SCHEMA_VERSION};

/// Schema version for the migration-ledger file.
pub const LEGACY_MIGRATION_SCHEMA_VERSION: u32 = 1;
/// Historical receipt domain.
pub const LEGACY_DOMAIN: &str = "mixed_repository_product_release_readiness";
/// Historical receipt status.
pub const LEGACY_STATUS: &str = "compatibility_read_only";
/// Replacement architecture.
pub const LEGACY_REPLACEMENT: &str = "independent_release_readiness_rails";

const LEDGER_TOML: &str = include_str!("../legacy_indicator_migrations.toml");
#[cfg(test)]
const LEGACY_RECEIPT_FIXTURE: &str = include_str!("../fixtures/legacy_receipt_v1.json");
const FROZEN_LEGACY_INDICATOR_IDS: &[&str] = &[
    "manifest.workspace_member_declared",
    "manifest.publish_policy_clean",
    "license.declared",
    "product_surface.native_only",
    "dap.cli_native_only",
    "release.native_binaries_present",
    "release.no_external_tooling",
    "release.checksums_valid",
    "formatter.native_default",
    "critic.native_default",
    "critic.run_critic_registry_parity",
    "quality.no_new_severe_gaps",
    "docs.status_current",
    "formatter.corpus_idempotent",
    "critic.no_false_positives",
    "formatter.perltidy_compat_no_external_only",
    "critic.perlcritic_compat_no_external_only",
];
#[cfg(test)]
const GENERATED_MARKDOWN: &str = include_str!("../../../docs/reference/PERL_KWALITEE_MIGRATION.md");

/// Destination domain for one frozen legacy indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyDestinationRail {
    /// Native product posture and configured defaults.
    NativeProduct,
    /// RIPR, coverage, conformance, corpus, and rule proof.
    EngineeringEvidence,
    /// Candidate archive, checksum, and subject integrity.
    ReleaseIntegrity,
    /// Candidate policy, metadata, status, and lineage governance.
    ReleaseGovernance,
    /// Installed packaged-product behavior.
    InstalledAcceptance,
    /// Removed from release readiness and owned by an ordinary gate.
    Retired,
}

impl LegacyDestinationRail {
    fn as_str(self) -> &'static str {
        match self {
            LegacyDestinationRail::NativeProduct => "native_product",
            LegacyDestinationRail::EngineeringEvidence => "engineering_evidence",
            LegacyDestinationRail::ReleaseIntegrity => "release_integrity",
            LegacyDestinationRail::ReleaseGovernance => "release_governance",
            LegacyDestinationRail::InstalledAcceptance => "installed_acceptance",
            LegacyDestinationRail::Retired => "retired",
        }
    }
}

/// How the historical proposition changes during migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyMigrationAction {
    /// Preserve the proposition under its correct domain authority.
    Transfer,
    /// Replace it with stronger candidate- or artifact-bound evidence.
    Replace,
    /// Keep only the narrower proposition actually established by the evidence.
    Narrow,
    /// Remove a bootstrap or ordinary-gate proposition from release readiness.
    Retire,
}

impl LegacyMigrationAction {
    fn as_str(self) -> &'static str {
        match self {
            LegacyMigrationAction::Transfer => "transfer",
            LegacyMigrationAction::Replace => "replace",
            LegacyMigrationAction::Narrow => "narrow",
            LegacyMigrationAction::Retire => "retire",
        }
    }
}

/// One indicator disposition from the checked-in migration ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyIndicatorMigration {
    /// Stable indicator ID from the frozen catalog.
    pub legacy_id: String,
    /// Destination rail, or [`LegacyDestinationRail::Retired`].
    pub destination_rail: LegacyDestinationRail,
    /// Stable destination proposition when the row remains active.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_id: Option<String>,
    /// Migration treatment.
    pub action: LegacyMigrationAction,
    /// GitHub issue that owns the destination or retirement.
    pub owner_issue: u64,
    /// Existing producer or evidence surface.
    pub evidence_source: String,
    /// Reproduction command for the historical proposition.
    pub reproduce: String,
    /// Mechanical condition required before the legacy row may retire.
    pub removal_condition: String,
    /// Claim-boundary note.
    pub note: String,
}

/// Complete checked-in migration ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyMigrationLedger {
    /// Ledger schema version.
    pub schema_version: u32,
    /// Historical receipt kind.
    pub legacy_kind: String,
    /// Historical receipt schema.
    pub legacy_schema_version: u32,
    /// Historical domain.
    pub domain: String,
    /// Compatibility status.
    pub status: String,
    /// Replacement architecture.
    pub replacement: String,
    /// One row for every frozen catalog indicator, in catalog order.
    pub indicator: Vec<LegacyIndicatorMigration>,
}

/// Historical catalog fields joined with one migration row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyIndicatorRecord {
    /// Stable historical indicator ID.
    pub legacy_id: String,
    /// Historical title.
    pub title: String,
    /// Historical mandatory/advisory state.
    pub mandatory: bool,
    /// Historical score weight.
    pub score_weight: u8,
    /// Historical evidence-source class.
    pub source: String,
    /// Historical profile scope.
    pub scope: String,
    /// Destination rail.
    pub destination_rail: LegacyDestinationRail,
    /// Stable destination proposition, when retained.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_id: Option<String>,
    /// Migration treatment.
    pub action: LegacyMigrationAction,
    /// Owning GitHub issue.
    pub owner_issue: u64,
    /// Existing producer or evidence surface.
    pub evidence_source: String,
    /// Reproduction command.
    pub reproduce: String,
    /// Retirement condition.
    pub removal_condition: String,
    /// Claim-boundary note.
    pub note: String,
}

/// Fail-closed legacy-ledger and receipt errors.
#[derive(Debug, Error)]
pub enum LegacyCompatibilityError {
    /// The checked-in TOML could not be decoded.
    #[error("invalid legacy migration ledger TOML: {0}")]
    InvalidLedgerToml(#[source] toml::de::Error),
    /// Ledger metadata no longer describes the frozen receipt.
    #[error("legacy migration ledger metadata mismatch: {0}")]
    LedgerMetadata(String),
    /// Ledger IDs no longer match the complete frozen catalog in order.
    #[error(
        "legacy migration ledger/catalog mismatch: expected [{expected}], observed [{observed}]"
    )]
    CatalogMismatch {
        /// Frozen catalog sequence.
        expected: String,
        /// Ledger sequence.
        observed: String,
    },
    /// A migration row has an invalid destination/action combination.
    #[error("invalid migration destination for `{legacy_id}`: {reason}")]
    InvalidDestination {
        /// Historical indicator ID.
        legacy_id: String,
        /// Validation reason.
        reason: String,
    },
    /// A validated row could not be joined back to the catalog.
    #[error("legacy indicator `{0}` is missing from the frozen catalog")]
    MissingCatalogIndicator(String),
    /// Historical receipt JSON could not be decoded.
    #[error("invalid legacy receipt JSON: {0}")]
    InvalidReceiptJson(#[source] serde_json::Error),
    /// Receipt kind is not the frozen legacy kind.
    #[error("expected legacy receipt kind `{expected}`, observed `{observed}`")]
    WrongReceiptKind {
        /// Required kind.
        expected: &'static str,
        /// Observed kind or `missing`.
        observed: String,
    },
    /// Receipt schema is not the frozen legacy schema.
    #[error("unsupported legacy receipt schema: expected {expected}, observed {observed}")]
    UnsupportedReceiptSchema {
        /// Required schema.
        expected: u32,
        /// Observed schema or `missing`.
        observed: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenLegacyReceiptV1 {
    kind: String,
    schema_version: u32,
    generated_at: String,
    commit: String,
    profile: KwaliteeProfile,
    score: u8,
    verdict: KwaliteeVerdict,
    mandatory_passed: bool,
    mandatory_failed_count: usize,
    mandatory_unverified_count: usize,
    warning_count: usize,
    unverified_count: usize,
    indicators: Vec<FrozenLegacyIndicatorV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenLegacyIndicatorV1 {
    id: String,
    area: String,
    title: String,
    mandatory: bool,
    status: IndicatorStatus,
    score_weight: u8,
    evidence: Vec<FrozenLegacyEvidenceV1>,
    remediation: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FrozenLegacyEvidenceV1 {
    kind: String,
    value: String,
}

impl From<FrozenLegacyReceiptV1> for KwaliteeReceipt {
    fn from(receipt: FrozenLegacyReceiptV1) -> Self {
        Self {
            kind: receipt.kind,
            schema_version: receipt.schema_version,
            generated_at: receipt.generated_at,
            commit: receipt.commit,
            profile: receipt.profile,
            score: receipt.score,
            verdict: receipt.verdict,
            mandatory_passed: receipt.mandatory_passed,
            mandatory_failed_count: receipt.mandatory_failed_count,
            mandatory_unverified_count: receipt.mandatory_unverified_count,
            warning_count: receipt.warning_count,
            unverified_count: receipt.unverified_count,
            indicators: receipt.indicators.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FrozenLegacyIndicatorV1> for KwaliteeIndicator {
    fn from(indicator: FrozenLegacyIndicatorV1) -> Self {
        Self {
            id: indicator.id,
            area: indicator.area,
            title: indicator.title,
            mandatory: indicator.mandatory,
            status: indicator.status,
            score_weight: indicator.score_weight,
            evidence: indicator.evidence.into_iter().map(Into::into).collect(),
            remediation: indicator.remediation,
        }
    }
}

impl From<FrozenLegacyEvidenceV1> for EvidenceRef {
    fn from(evidence: FrozenLegacyEvidenceV1) -> Self {
        Self { kind: evidence.kind, value: evidence.value }
    }
}

/// Parse and validate the checked-in migration ledger.
pub fn legacy_migration_ledger() -> Result<LegacyMigrationLedger, LegacyCompatibilityError> {
    let ledger: LegacyMigrationLedger =
        toml::from_str(LEDGER_TOML).map_err(LegacyCompatibilityError::InvalidLedgerToml)?;
    validate_legacy_migration_ledger(&ledger)?;
    Ok(ledger)
}

/// Validate exact metadata, population, order, and destination invariants.
pub fn validate_legacy_migration_ledger(
    ledger: &LegacyMigrationLedger,
) -> Result<(), LegacyCompatibilityError> {
    if ledger.schema_version != LEGACY_MIGRATION_SCHEMA_VERSION {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "schema_version must be {LEGACY_MIGRATION_SCHEMA_VERSION}, observed {}",
            ledger.schema_version
        )));
    }
    if ledger.legacy_kind != RECEIPT_KIND {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "legacy_kind must be {RECEIPT_KIND}, observed {}",
            ledger.legacy_kind
        )));
    }
    if ledger.legacy_schema_version != SCHEMA_VERSION {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "legacy_schema_version must be {SCHEMA_VERSION}, observed {}",
            ledger.legacy_schema_version
        )));
    }
    if ledger.domain != LEGACY_DOMAIN {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "domain must be {LEGACY_DOMAIN}, observed {}",
            ledger.domain
        )));
    }
    if ledger.status != LEGACY_STATUS {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "status must be {LEGACY_STATUS}, observed {}",
            ledger.status
        )));
    }
    if ledger.replacement != LEGACY_REPLACEMENT {
        return Err(LegacyCompatibilityError::LedgerMetadata(format!(
            "replacement must be {LEGACY_REPLACEMENT}, observed {}",
            ledger.replacement
        )));
    }

    let live = CATALOG.iter().map(|spec| spec.id).collect::<Vec<_>>();
    if live != FROZEN_LEGACY_INDICATOR_IDS {
        return Err(LegacyCompatibilityError::CatalogMismatch {
            expected: FROZEN_LEGACY_INDICATOR_IDS.join(", "),
            observed: live.join(", "),
        });
    }
    let expected = FROZEN_LEGACY_INDICATOR_IDS;
    let observed = ledger.indicator.iter().map(|row| row.legacy_id.as_str()).collect::<Vec<_>>();
    if expected != observed {
        return Err(LegacyCompatibilityError::CatalogMismatch {
            expected: expected.join(", "),
            observed: observed.join(", "),
        });
    }

    for row in &ledger.indicator {
        match (row.destination_rail, row.action, row.destination_id.as_deref()) {
            (LegacyDestinationRail::Retired, LegacyMigrationAction::Retire, None) => {}
            (LegacyDestinationRail::Retired, _, _) => {
                return Err(LegacyCompatibilityError::InvalidDestination {
                    legacy_id: row.legacy_id.clone(),
                    reason: "retired rows must use action=retire and omit destination_id"
                        .to_string(),
                });
            }
            (_, LegacyMigrationAction::Retire, _) => {
                return Err(LegacyCompatibilityError::InvalidDestination {
                    legacy_id: row.legacy_id.clone(),
                    reason: "action=retire requires destination_rail=retired".to_string(),
                });
            }
            (_, _, Some(destination_id)) if destination_id.contains('.') => {}
            (_, _, Some(_)) => {
                return Err(LegacyCompatibilityError::InvalidDestination {
                    legacy_id: row.legacy_id.clone(),
                    reason: "destination_id must use a dotted stable identity".to_string(),
                });
            }
            (_, _, None) => {
                return Err(LegacyCompatibilityError::InvalidDestination {
                    legacy_id: row.legacy_id.clone(),
                    reason: "active destination rows require destination_id".to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Return the frozen catalog joined with its migration dispositions.
pub fn legacy_indicator_records() -> Result<Vec<LegacyIndicatorRecord>, LegacyCompatibilityError> {
    let ledger = legacy_migration_ledger()?;
    ledger
        .indicator
        .into_iter()
        .map(|row| {
            let spec =
                CATALOG.iter().find(|spec| spec.id == row.legacy_id.as_str()).ok_or_else(|| {
                    LegacyCompatibilityError::MissingCatalogIndicator(row.legacy_id.clone())
                })?;
            Ok(record_from(spec, row))
        })
        .collect()
}

/// Decode one historical receipt and reject kind/schema drift explicitly.
pub fn read_legacy_receipt(bytes: &[u8]) -> Result<KwaliteeReceipt, LegacyCompatibilityError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(LegacyCompatibilityError::InvalidReceiptJson)?;

    let observed_kind = value.get("kind").and_then(serde_json::Value::as_str).unwrap_or("missing");
    if observed_kind != RECEIPT_KIND {
        return Err(LegacyCompatibilityError::WrongReceiptKind {
            expected: RECEIPT_KIND,
            observed: observed_kind.to_string(),
        });
    }

    let observed_schema = value.get("schema_version").and_then(serde_json::Value::as_u64);
    if observed_schema != Some(u64::from(SCHEMA_VERSION)) {
        return Err(LegacyCompatibilityError::UnsupportedReceiptSchema {
            expected: SCHEMA_VERSION,
            observed: observed_schema
                .map(|schema| schema.to_string())
                .unwrap_or_else(|| "missing".to_string()),
        });
    }

    let receipt: FrozenLegacyReceiptV1 =
        serde_json::from_value(value).map_err(LegacyCompatibilityError::InvalidReceiptJson)?;
    Ok(receipt.into())
}

/// Render the checked-in human migration reference from the two authorities.
pub fn render_legacy_migration_markdown() -> Result<String, LegacyCompatibilityError> {
    let records = legacy_indicator_records()?;
    let mut out = String::from(
        "# Legacy `perl_kwalitee.v1` migration\n\n\
         > Generated from `crates/perl-kwalitee/legacy_indicator_migrations.toml` and the frozen\n\
         > legacy catalog. Do not edit this table independently.\n\n\
         ## Contract\n\n\
         - Legacy receipt kind: `perl_kwalitee`\n\
         - Legacy schema: `1`\n\
         - Historical domain: `mixed_repository_product_release_readiness`\n\
         - Status: compatibility-read-only; closed to new indicators\n\
         - Replacement: independent release-readiness rails plus the native Rust `perl-kwalitee` analyser\n\n\
         Historical receipts remain readable. They are not `distribution_kwalitee` receipts and\n\
         cannot authorize a current release candidate.\n\n\
         ## Indicator disposition\n\n\
         | Legacy indicator | Title | Mandatory | Weight | Source | Scope | Destination | Action | Owner | Reproduce |\n\
         |---|---|---:|---:|---|---|---|---|---|---|\n",
    );

    for record in records {
        let destination = match &record.destination_id {
            Some(destination_id) => {
                format!("`{}` / `{}`", record.destination_rail.as_str(), destination_id)
            }
            None => format!("`{}`", record.destination_rail.as_str()),
        };
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | `{}` | `{}` | {} | `{}` | #{} | `{}` |\n",
            record.legacy_id,
            sanitize_cell(&record.title),
            if record.mandatory { "yes" } else { "no" },
            record.score_weight,
            record.source,
            record.scope,
            destination,
            record.action.as_str(),
            record.owner_issue,
            sanitize_cell(&record.reproduce),
        ));
    }

    out.push_str(
        "\n## Interpretation\n\n\
         - `transfer` preserves the proposition while moving it to its correct domain authority.\n\
         - `replace` requires stronger candidate- or artifact-bound evidence before the legacy row can retire.\n\
         - `narrow` keeps only the bounded proposition actually established by the evidence.\n\
         - `retire` removes a bootstrap or ordinary-gate check from release readiness.\n\n\
         No row migrates into the CPANTS-compatible Kwalitee score. The native analyser has its\n\
         own catalog, input identity, scoring contract, and conformance fixtures.\n",
    );

    Ok(out)
}

fn record_from(
    spec: &'static IndicatorSpec,
    row: LegacyIndicatorMigration,
) -> LegacyIndicatorRecord {
    LegacyIndicatorRecord {
        legacy_id: row.legacy_id,
        title: spec.title.to_string(),
        mandatory: spec.mandatory,
        score_weight: spec.weight,
        source: source_name(spec.source).to_string(),
        scope: scope_name(spec.scope).to_string(),
        destination_rail: row.destination_rail,
        destination_id: row.destination_id,
        action: row.action,
        owner_issue: row.owner_issue,
        evidence_source: row.evidence_source,
        reproduce: row.reproduce,
        removal_condition: row.removal_condition,
        note: row.note,
    }
}

fn source_name(source: EvalSource) -> &'static str {
    match source {
        EvalSource::Native => "native",
        EvalSource::ReadinessReceipt => "readiness_receipt",
        EvalSource::QualityGateReceipt => "quality_gate_receipt",
        EvalSource::NightlyReceipt => "nightly_receipt",
        EvalSource::External => "external",
    }
}

fn scope_name(scope: IndicatorScope) -> &'static str {
    match scope {
        IndicatorScope::All => "all",
        IndicatorScope::ReleaseOnly => "release_only",
        IndicatorScope::NightlyOnly => "nightly_only",
    }
}

fn sanitize_cell(value: &str) -> String {
    value.replace(['\r', '\n'], " ").replace('|', "\\|").replace('`', "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KwaliteeProfile, KwaliteeVerdict};

    fn sample_receipt() -> KwaliteeReceipt {
        KwaliteeReceipt {
            kind: RECEIPT_KIND.to_string(),
            schema_version: SCHEMA_VERSION,
            generated_at: String::new(),
            commit: "abcdef1".to_string(),
            profile: KwaliteeProfile::Pr,
            score: 100,
            verdict: KwaliteeVerdict::Pass,
            mandatory_passed: true,
            mandatory_failed_count: 0,
            mandatory_unverified_count: 0,
            warning_count: 0,
            unverified_count: 0,
            indicators: Vec::new(),
        }
    }

    #[test]
    fn ledger_accounts_for_the_frozen_catalog_exactly_once_and_in_order()
    -> Result<(), Box<dyn std::error::Error>> {
        let ledger = legacy_migration_ledger()?;
        assert_eq!(ledger.indicator.len(), CATALOG.len());
        assert_eq!(
            ledger.indicator.iter().map(|row| row.legacy_id.as_str()).collect::<Vec<_>>(),
            CATALOG.iter().map(|spec| spec.id).collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn generated_migration_reference_is_current() -> Result<(), Box<dyn std::error::Error>> {
        let generated = render_legacy_migration_markdown()?;
        assert_eq!(generated, GENERATED_MARKDOWN);
        Ok(())
    }

    #[test]
    fn legacy_reader_accepts_only_the_frozen_kind_and_schema()
    -> Result<(), Box<dyn std::error::Error>> {
        let receipt = sample_receipt();
        let encoded = serde_json::to_vec(&receipt)?;
        assert_eq!(read_legacy_receipt(&encoded)?, receipt);

        let mut wrong_schema = serde_json::to_value(&receipt)?;
        wrong_schema["schema_version"] = serde_json::json!(2);
        let encoded = serde_json::to_vec(&wrong_schema)?;
        assert!(matches!(
            read_legacy_receipt(&encoded),
            Err(LegacyCompatibilityError::UnsupportedReceiptSchema { .. })
        ));

        let mut wrong_kind = serde_json::to_value(&receipt)?;
        wrong_kind["kind"] = serde_json::json!("distribution_kwalitee");
        let encoded = serde_json::to_vec(&wrong_kind)?;
        assert!(matches!(
            read_legacy_receipt(&encoded),
            Err(LegacyCompatibilityError::WrongReceiptKind { .. })
        ));
        Ok(())
    }

    #[test]
    fn historical_v1_fixture_decodes_through_pinned_shape() -> Result<(), Box<dyn std::error::Error>>
    {
        let receipt = read_legacy_receipt(LEGACY_RECEIPT_FIXTURE.as_bytes())?;
        assert_eq!(receipt.kind, RECEIPT_KIND);
        assert_eq!(receipt.schema_version, SCHEMA_VERSION);
        assert_eq!(receipt.commit, "legacy-v1-fixture");
        assert_eq!(receipt.indicators.len(), 1);
        assert_eq!(receipt.indicators[0].id, "manifest.workspace_member_declared");
        Ok(())
    }

    #[test]
    fn legacy_reader_rejects_unknown_envelope_and_nested_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut unknown_envelope = serde_json::to_value(sample_receipt())?;
        unknown_envelope["future_authority"] = serde_json::json!(true);
        assert!(matches!(
            read_legacy_receipt(&serde_json::to_vec(&unknown_envelope)?),
            Err(LegacyCompatibilityError::InvalidReceiptJson(_))
        ));

        let mut receipt = sample_receipt();
        receipt.indicators.push(crate::KwaliteeIndicator {
            id: "manifest.workspace_member_declared".to_string(),
            area: "manifest".to_string(),
            title: "workspace member".to_string(),
            mandatory: true,
            status: crate::IndicatorStatus::Pass,
            score_weight: 1,
            evidence: vec![crate::EvidenceRef::file("Cargo.toml")],
            remediation: None,
        });
        let mut unknown_nested = serde_json::to_value(receipt)?;
        unknown_nested["indicators"][0]["future_status"] = serde_json::json!("pass");
        assert!(matches!(
            read_legacy_receipt(&serde_json::to_vec(&unknown_nested)?),
            Err(LegacyCompatibilityError::InvalidReceiptJson(_))
        ));
        Ok(())
    }

    #[test]
    fn active_destinations_are_dotted_and_retired_rows_have_no_destination()
    -> Result<(), Box<dyn std::error::Error>> {
        let ledger = legacy_migration_ledger()?;
        for row in ledger.indicator {
            match row.destination_rail {
                LegacyDestinationRail::Retired => {
                    assert_eq!(row.action, LegacyMigrationAction::Retire);
                    assert!(row.destination_id.is_none());
                }
                _ => {
                    assert_ne!(row.action, LegacyMigrationAction::Retire);
                    assert!(
                        row.destination_id
                            .as_deref()
                            .is_some_and(|destination| destination.contains('.'))
                    );
                }
            }
        }
        Ok(())
    }
}
