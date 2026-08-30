//! Typed release-candidate security contract (`release_candidate_security.v1`)
//! and the topology-derived audit rail inventory.
//!
//! Schema and subject-inventory definition only (#9427): this module owns the
//! closed contract shape, the required rail vocabulary, and fail-closed
//! validation. It executes no scanner, runs no candidate audit, accepts no
//! risk, and changes no `ship_candidate` policy — those belong to the exact
//! security execution this contract is a prerequisite for.

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Schema identity for the contract governed here. Serialized receipts must
/// carry exactly this string; anything else is a different contract.
pub const CONTRACT_SCHEMA: &str = "release_candidate_security.v1";

#[derive(Debug, Parser)]
#[command(
    name = "candidate-security-contract",
    about = "Validate a release_candidate_security.v1 contract document"
)]
struct Cli {
    /// Path to the contract JSON document.
    #[arg(long)]
    contract: PathBuf,
}

/// The closed candidate-security contract. Every field is required and unknown
/// fields are rejected, so an under-specified or over-specified document fails
/// instead of silently narrowing the audited surface.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CandidateSecurityContract {
    pub schema_version: String,
    /// Exact subject identities the candidate audit binds. Every identity is
    /// mandatory so an omitted lockfile/artifact digest cannot pass silently;
    /// the container digest is the one exception because it is required only
    /// when `container_required` says the topology demands a container.
    pub subjects: SubjectIdentities,
    /// Authoritative producer identities rails may reference. A rail naming a
    /// tool absent from this set fails validation.
    pub tools: BTreeSet<ToolIdentity>,
    /// Whether the release topology requires a container for this candidate.
    pub container_required: bool,
    /// One row per topology-required audit rail, in deterministic order.
    pub rails: Vec<SecurityRail>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SubjectIdentities {
    pub release_repo_sha: String,
    pub candidate_packet_digest: String,
    pub cargo_lock_hash: String,
    pub extension_package_hash: String,
    pub extension_lock_hash: String,
    pub topology_digest: String,
    pub crate_archive_digest: String,
    pub vsix_digest: String,
    pub checksums_digest: String,
    pub sbom_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_digest: Option<String>,
    pub workflow_run_id: String,
    pub workflow_attempt: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    pub tool: String,
    pub version: String,
    /// Config identity the tool ran under (file digest or pinned reference).
    pub config_identity: String,
    /// Database identity when the tool consults one (e.g. advisory DB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database_identity: Option<String>,
}

/// Required rail vocabulary. This is the topology-derived inventory: every
/// release candidate audit must carry exactly one row per name here.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RailName {
    RustDependenciesPolicy,
    ExtensionDependencies,
    PackagedSubjects,
    ContainerWhenRequired,
}

/// Rail outcome. `pass` is never a default: it requires applicability evidence
/// and cannot coexist with an unresolved review finding.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum RailStatus {
    Pass,
    Failed,
    NotProven,
    NotApplicable,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecurityRail {
    pub rail: RailName,
    pub status: RailStatus,
    /// Tool name from `CandidateSecurityContract::tools` that produced the
    /// result. An undeclared producer is an unknown tool identity.
    pub authoritative_producer: String,
    /// Topology/reachability evidence for the applicability decision. Required
    /// for `pass` and `not_applicable`; omission there is the
    /// "applicability defaults to pass" hazard this contract forbids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability_evidence: Option<String>,
    /// Rule naming the exact subjects this rail covers.
    pub subject_rule: String,
    /// Identity/schema the producer's output must carry to be checkable.
    pub required_output_identity: String,
    pub output_schema: String,
    /// Findings stay attached to their rail; dispositions remain review
    /// metadata and never upgrade the rail status.
    pub findings: Vec<SecurityFinding>,
    pub owner: String,
    pub claim_boundary: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecurityFinding {
    pub finding_id: String,
    pub summary: String,
    pub disposition: FindingDisposition,
}

/// Disposition vocabulary for findings. These values preserve the distinction
/// between review effect (a human disposition record) and claim effect (the
/// rail status): no disposition here flips a rail to `pass`, and `validate`
/// rejects a `pass` rail that still carries an open review finding.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FindingDisposition {
    NeedsReview,
    AcceptedWithDisposition,
    Rejected,
    Remediated,
}

/// The topology-required rail inventory, in canonical order.
pub fn required_rails() -> [RailName; 4] {
    [
        RailName::RustDependenciesPolicy,
        RailName::ExtensionDependencies,
        RailName::PackagedSubjects,
        RailName::ContainerWhenRequired,
    ]
}

/// Validate `contract` fully, failing closed on every structural hazard the
/// issue names. Empty result means the contract is well-formed; it makes no
/// claim about any real candidate.
pub fn validate_contract(contract: &CandidateSecurityContract) -> Result<()> {
    if contract.schema_version != CONTRACT_SCHEMA {
        bail!(
            "unknown schema_version {:?}; expected {:?}",
            contract.schema_version,
            CONTRACT_SCHEMA
        );
    }

    let subjects = &contract.subjects;
    for (name, value) in [
        ("release_repo_sha", &subjects.release_repo_sha),
        ("candidate_packet_digest", &subjects.candidate_packet_digest),
        ("cargo_lock_hash", &subjects.cargo_lock_hash),
        ("extension_package_hash", &subjects.extension_package_hash),
        ("extension_lock_hash", &subjects.extension_lock_hash),
        ("topology_digest", &subjects.topology_digest),
        ("crate_archive_digest", &subjects.crate_archive_digest),
        ("vsix_digest", &subjects.vsix_digest),
        ("checksums_digest", &subjects.checksums_digest),
        ("sbom_digest", &subjects.sbom_digest),
        ("workflow_run_id", &subjects.workflow_run_id),
        ("workflow_attempt", &subjects.workflow_attempt),
    ] {
        if value.trim().is_empty() {
            bail!("required subject identity {name} is omitted");
        }
    }

    if contract.container_required && subjects.container_digest.is_none() {
        bail!("topology requires a container but container_digest is omitted");
    }

    for tool in &contract.tools {
        if tool.tool.trim().is_empty() || tool.version.trim().is_empty() {
            bail!("tool identity {:?} is missing a name or version", tool.tool);
        }
        if tool.config_identity.trim().is_empty() {
            bail!("tool {:?} has no config identity", tool.tool);
        }
    }

    let mut seen = BTreeSet::new();
    for rail in &contract.rails {
        if !seen.insert(rail.rail) {
            bail!("duplicate authoritative rail {:?}", rail_name_label(rail.rail));
        }
    }
    for required in required_rails() {
        if !seen.contains(&required) {
            bail!(
                "required topology rail {:?} is absent from the inventory",
                rail_name_label(required)
            );
        }
    }

    for rail in &contract.rails {
        if !contract.tools.iter().any(|tool| tool.tool == rail.authoritative_producer) {
            bail!(
                "rail {:?} names producer {:?} which is not a declared tool identity",
                rail_name_label(rail.rail),
                rail.authoritative_producer
            );
        }
        if rail.owner.trim().is_empty() {
            bail!("rail {:?} has no owner", rail_name_label(rail.rail));
        }
        if rail.subject_rule.trim().is_empty() {
            bail!("rail {:?} names no subject rule", rail_name_label(rail.rail));
        }
        if rail.required_output_identity.trim().is_empty() || rail.output_schema.trim().is_empty() {
            bail!(
                "rail {:?} is missing its required output identity or schema",
                rail_name_label(rail.rail)
            );
        }
        if rail.claim_boundary.trim().is_empty() {
            bail!("rail {:?} has no claim boundary", rail_name_label(rail.rail));
        }

        let evidence = rail.applicability_evidence.as_deref().map(str::trim).unwrap_or("");
        match rail.status {
            RailStatus::Pass | RailStatus::NotApplicable if evidence.is_empty() => bail!(
                "rail {:?} claims {:?} without applicability evidence; applicability never defaults to pass",
                rail_name_label(rail.rail),
                rail_status_label(rail.status)
            ),
            RailStatus::Pass => {
                if rail.rail == RailName::ContainerWhenRequired
                    && subjects.container_digest.is_none()
                {
                    bail!(
                        "container rail passes while container_digest is omitted; \
                         omission is not a pass"
                    );
                }
                if rail
                    .findings
                    .iter()
                    .any(|finding| finding.disposition == FindingDisposition::NeedsReview)
                {
                    bail!(
                        "rail {:?} passes while carrying an unresolved needs_review finding; \
                         dispositions are review metadata, not risk acceptance",
                        rail_name_label(rail.rail)
                    );
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn rail_name_label(rail: RailName) -> &'static str {
    match rail {
        RailName::RustDependenciesPolicy => "rust_dependencies_policy",
        RailName::ExtensionDependencies => "extension_dependencies",
        RailName::PackagedSubjects => "packaged_subjects",
        RailName::ContainerWhenRequired => "container_when_required",
    }
}

fn rail_status_label(status: RailStatus) -> &'static str {
    match status {
        RailStatus::Pass => "pass",
        RailStatus::Failed => "failed",
        RailStatus::NotProven => "not_proven",
        RailStatus::NotApplicable => "not_applicable",
    }
}

/// Build the deterministic baseline inventory for one candidate: every
/// topology-required rail present with explicit subjects, producers, and
/// boundaries. Values are placeholders that name their rule, not audit
/// results — this constructor exists so the inventory shape is executable and
/// provable before any scanner runs.
pub fn baseline_inventory() -> CandidateSecurityContract {
    let mut tools = BTreeSet::new();
    tools.insert(ToolIdentity {
        tool: "cargo_deny".to_string(),
        version: "pinned-by-workflow".to_string(),
        config_identity: "deny.toml@HEAD".to_string(),
        database_identity: Some("advisory-db@pinned".to_string()),
    });
    tools.insert(ToolIdentity {
        tool: "npm_audit".to_string(),
        version: "pinned-by-workflow".to_string(),
        config_identity: "package-lock.json@candidate".to_string(),
        database_identity: None,
    });

    let rail = |rail: RailName,
                producer: &str,
                subject_rule: &str,
                output_schema: &str,
                evidence: Option<String>,
                status: RailStatus| SecurityRail {
        rail,
        status,
        authoritative_producer: producer.to_string(),
        applicability_evidence: evidence,
        subject_rule: subject_rule.to_string(),
        required_output_identity: format!("{subject_rule} report digest"),
        output_schema: output_schema.to_string(),
        findings: Vec::new(),
        owner: "issue-9427".to_string(),
        claim_boundary: "schema/inventory only; no scanner execution in this claim".to_string(),
    };

    CandidateSecurityContract {
        schema_version: CONTRACT_SCHEMA.to_string(),
        subjects: SubjectIdentities {
            release_repo_sha: "<release repo sha>".to_string(),
            candidate_packet_digest: "<candidate packet digest>".to_string(),
            cargo_lock_hash: "<Cargo.lock hash>".to_string(),
            extension_package_hash: "<extension package hash>".to_string(),
            extension_lock_hash: "<extension lock hash>".to_string(),
            topology_digest: "<topology digest>".to_string(),
            crate_archive_digest: "<crate archive digest>".to_string(),
            vsix_digest: "<VSIX digest>".to_string(),
            checksums_digest: "<checksums digest>".to_string(),
            sbom_digest: "<SBOM digest>".to_string(),
            container_digest: Some("<container digest>".to_string()),
            workflow_run_id: "<workflow run>".to_string(),
            workflow_attempt: "<attempt>".to_string(),
        },
        tools,
        container_required: true,
        rails: vec![
            rail(
                RailName::RustDependenciesPolicy,
                "cargo_deny",
                "workspace Cargo.lock rust dependency set",
                "rust_dependencies_policy.v1",
                Some("topology: rust candidate is always in scope".to_string()),
                RailStatus::NotProven,
            ),
            rail(
                RailName::ExtensionDependencies,
                "npm_audit",
                "vscode-extension package-lock dependency set",
                "extension_dependencies.v1",
                Some("topology: VSIX ships the extension lockfile".to_string()),
                RailStatus::NotProven,
            ),
            rail(
                RailName::PackagedSubjects,
                "cargo_deny",
                "crate archive, VSIX, checksums, and SBOM digests",
                "packaged_subjects.v1",
                Some("topology: packaged subjects are the release payload".to_string()),
                RailStatus::NotProven,
            ),
            rail(
                RailName::ContainerWhenRequired,
                "cargo_deny",
                "container image digest when topology requires a container",
                "container_subject.v1",
                Some("topology: container required for this candidate".to_string()),
                RailStatus::NotProven,
            ),
        ],
    }
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let text = std::fs::read_to_string(&cli.contract)
        .with_context(|| format!("reading contract {}", cli.contract.display()))?;
    let contract: CandidateSecurityContract = serde_json::from_str(&text)
        .with_context(|| format!("parsing contract {}", cli.contract.display()))?;
    validate_contract(&contract)?;
    println!("candidate security contract {} is closed and valid", cli.contract.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::eyre;

    fn contract() -> CandidateSecurityContract {
        baseline_inventory()
    }

    #[test]
    fn baseline_inventory_is_valid_and_deterministic() -> Result<()> {
        validate_contract(&contract())?;
        let left = serde_json::to_string(&contract())?;
        let right = serde_json::to_string(&contract())?;
        assert_eq!(left, right, "identical inputs must serialize identically");
        Ok(())
    }

    #[test]
    fn rejects_unknown_schema_version() -> Result<()> {
        let mut candidate = contract();
        candidate.schema_version = "release_candidate_security.v2".to_string();
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("unknown schema_version"));
        Ok(())
    }

    #[test]
    fn rejects_required_topology_rail_absent_from_inventory() -> Result<()> {
        let mut candidate = contract();
        candidate.rails.retain(|rail| rail.rail != RailName::PackagedSubjects);
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("absent from the inventory"));
        Ok(())
    }

    #[test]
    fn rejects_container_silently_omitted_while_required() -> Result<()> {
        let mut candidate = contract();
        candidate.subjects.container_digest = None;
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("container_digest is omitted"));
        Ok(())
    }

    #[test]
    fn rejects_omitted_lockfile_subject_identity() -> Result<()> {
        let mut candidate = contract();
        candidate.subjects.cargo_lock_hash = "  ".to_string();
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("cargo_lock_hash"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_tool_identity_on_a_rail() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].authoritative_producer = "undeclared_scanner".to_string();
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("not a declared tool identity"));
        Ok(())
    }

    #[test]
    fn rejects_not_applicable_without_topology_evidence() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[3].status = RailStatus::NotApplicable;
        candidate.rails[3].applicability_evidence = None;
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("without applicability evidence"));
        Ok(())
    }

    #[test]
    fn rejects_duplicate_authoritative_rail() -> Result<()> {
        let mut candidate = contract();
        let duplicate = candidate.rails[0].clone();
        candidate.rails.push(duplicate);
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("duplicate authoritative rail"));
        Ok(())
    }

    #[test]
    fn rejects_pass_with_unresolved_review_finding() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].status = RailStatus::Pass;
        candidate.rails[0].findings.push(SecurityFinding {
            finding_id: "F-1".to_string(),
            summary: "advisory still open".to_string(),
            disposition: FindingDisposition::NeedsReview,
        });
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("invalid contract unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("review metadata, not risk acceptance"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_document_fields() -> Result<()> {
        let text = r#"{"schema_version":"release_candidate_security.v1","extra":true}"#;
        let parsed: Result<CandidateSecurityContract, _> = serde_json::from_str(text);
        assert!(parsed.is_err(), "closed contract must deny unknown fields");
        Ok(())
    }
}
