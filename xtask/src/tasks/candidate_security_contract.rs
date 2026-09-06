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
    /// Crate package identity (the built package artifact), bound separately
    /// from the crate archive per #9427's "crate package/archive" parallel
    /// with "extension package/lock hashes".
    pub crate_package_digest: String,
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
        ("crate_package_digest", &subjects.crate_package_digest),
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

    // The container pair is one closed state: required with a non-blank
    // digest, or not required with no digest. Either half alone leaves the
    // subject set carrying an identity the topology does not govern.
    let container_digest = subjects.container_digest.as_deref().map(str::trim);
    match (contract.container_required, container_digest) {
        (true, None) => {
            bail!("topology requires a container but container_digest is omitted")
        }
        (true, Some("")) => {
            bail!("topology requires a container but container_digest is blank")
        }
        (false, Some(_)) => {
            bail!("container_digest is present while the topology does not require a container")
        }
        _ => {}
    }

    let mut tool_names = BTreeSet::new();
    for tool in &contract.tools {
        if tool.tool.trim().is_empty() || tool.version.trim().is_empty() {
            bail!("tool identity {:?} is missing a name or version", tool.tool);
        }
        if tool.config_identity.trim().is_empty() {
            bail!("tool {:?} has no config identity", tool.tool);
        }
        // "unknown tool/database identity accepted" must fail closed: a blank
        // database identity is neither a real identity nor an absent one.
        if let Some(database_identity) = &tool.database_identity
            && database_identity.trim().is_empty()
        {
            bail!("tool {:?} declares a blank database identity", tool.tool);
        }
        // Two declared identities under one name leave every rail reference
        // ambiguous about which tool produced its result.
        if !tool_names.insert(tool.tool.as_str()) {
            bail!("duplicate tool identity {:?}; a rail's producer must be unambiguous", tool.tool);
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
    // The rail vector defines the serialized bytes and therefore every digest
    // over them, so one canonical order is part of the closed contract:
    // permutations of the same inventory must not produce different documents.
    let rail_sequence: Vec<RailName> = contract.rails.iter().map(|rail| rail.rail).collect();
    if rail_sequence != required_rails().to_vec() {
        bail!("rails are not in the canonical required_rails() order");
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
        // A finding with a blank id or summary is structurally
        // unidentifiable: no stable identifier to cross-reference and nothing
        // a downstream reader can act on, whatever its disposition claims.
        for finding in &rail.findings {
            if finding.finding_id.trim().is_empty() || finding.summary.trim().is_empty() {
                bail!(
                    "rail {:?} carries a finding with a blank id or summary",
                    rail_name_label(rail.rail)
                );
            }
        }

        let evidence = rail.applicability_evidence.as_deref().map(str::trim).unwrap_or("");
        match rail.status {
            RailStatus::Pass | RailStatus::NotApplicable if evidence.is_empty() => bail!(
                "rail {:?} claims {:?} without applicability evidence; applicability never defaults to pass",
                rail_name_label(rail.rail),
                rail_status_label(rail.status)
            ),
            // A not_proven rail that does not say what is unproven is
            // structurally a pass with a deferred proof obligation, so it
            // carries the same evidence duty.
            RailStatus::NotProven if evidence.is_empty() => bail!(
                "rail {:?} claims not_proven without applicability evidence",
                rail_name_label(rail.rail)
            ),
            // A failed rail must say why it failed: findings name the failures
            // and evidence explains why no pass was reached.
            RailStatus::Failed if evidence.is_empty() && rail.findings.is_empty() => bail!(
                "rail {:?} claims failed with neither findings nor applicability evidence",
                rail_name_label(rail.rail)
            ),
            RailStatus::NotApplicable
                if rail.rail == RailName::ContainerWhenRequired && contract.container_required =>
            {
                bail!(
                    "container rail claims not_applicable while the topology requires a container"
                )
            }
            RailStatus::Pass => {
                if rail.rail == RailName::ContainerWhenRequired
                    && subjects.container_digest.is_none()
                {
                    bail!(
                        "container rail passes while container_digest is omitted; \
                         omission is not a pass"
                    );
                }
                for finding in &rail.findings {
                    match finding.disposition {
                        FindingDisposition::NeedsReview => bail!(
                            "rail {:?} passes while carrying an unresolved needs_review finding; \
                             dispositions are review metadata, not risk acceptance",
                            rail_name_label(rail.rail)
                        ),
                        FindingDisposition::Rejected => bail!(
                            "rail {:?} passes while carrying a rejected finding; \
                             a rejected finding contradicts the pass claim",
                            rail_name_label(rail.rail)
                        ),
                        FindingDisposition::AcceptedWithDisposition
                        | FindingDisposition::Remediated => {}
                    }
                }
            }
            RailStatus::Failed
                if rail
                    .findings
                    .iter()
                    .any(|finding| finding.disposition == FindingDisposition::Remediated) =>
            {
                bail!(
                    "rail {:?} claims failed while carrying a remediated finding; \
                     a remediated finding belongs on a rail that no longer fails",
                    rail_name_label(rail.rail)
                );
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
            crate_package_digest: "<crate package digest>".to_string(),
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
                "crate package, crate archive, VSIX, checksums, and SBOM digests",
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

/// Contract documents are small closed JSON files. Anything larger, missing,
/// or not a regular file is refused up front — before it can occupy validator
/// memory or fail with a confusing mid-parse error.
const MAX_CONTRACT_BYTES: u64 = 1024 * 1024;

fn load_contract(path: &std::path::Path) -> Result<CandidateSecurityContract> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("reading contract {}", path.display()))?;
    if !metadata.is_file() {
        bail!("contract path {} is not a file", path.display());
    }
    if metadata.len() > MAX_CONTRACT_BYTES {
        bail!(
            "contract {} is {} bytes; the closed-contract limit is {MAX_CONTRACT_BYTES} bytes",
            path.display(),
            metadata.len()
        );
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading contract {}", path.display()))?;
    let contract: CandidateSecurityContract = serde_json::from_str(&text)
        .with_context(|| format!("parsing contract {}", path.display()))?;
    Ok(contract)
}

pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let contract = load_contract(&cli.contract)?;
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
        // The rejection must be attributable to `deny_unknown_fields`, not to
        // missing required fields: the same document without the extra field
        // must parse, so one injected top-level field is the only delta
        // between the accepted and rejected inputs.
        let mut value = serde_json::to_value(contract())?;
        serde_json::from_value::<CandidateSecurityContract>(value.clone()).map_err(|error| {
            eyre!("baseline document must parse before the unknown field is added: {error}")
        })?;
        value
            .as_object_mut()
            .ok_or_else(|| eyre!("contract must serialize to a JSON object"))?
            .insert("extra".to_string(), serde_json::Value::Bool(true));
        let parsed: Result<CandidateSecurityContract, _> = serde_json::from_value(value);
        assert!(parsed.is_err(), "closed contract must deny unknown fields");
        Ok(())
    }

    #[test]
    fn rejects_blank_tool_database_identity() -> Result<()> {
        // Issue #9427's "unknown tool/database identity accepted" control: a
        // whitespace-only database identity is neither a real identity nor an
        // absent one, so it must fail closed.
        let mut candidate = contract();
        candidate.tools = candidate
            .tools
            .iter()
            .map(|tool| {
                let mut modified = tool.clone();
                if modified.tool == "cargo_deny" {
                    modified.database_identity = Some("   ".to_string());
                }
                modified
            })
            .collect();
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("blank database identity unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("database identity"));
        Ok(())
    }

    #[test]
    fn rejects_document_missing_crate_package_digest() -> Result<()> {
        // Issue #9427 binds "crate package/archive" as two subject identities
        // (parallel to "extension package/lock hashes"), so a document whose
        // subject set omits the crate package digest must fail closed.
        let mut value = serde_json::to_value(contract())?;
        let removed = value
            .as_object_mut()
            .ok_or_else(|| eyre!("contract must serialize to a JSON object"))?
            .get_mut("subjects")
            .and_then(|subjects| subjects.as_object_mut())
            .and_then(|subjects| subjects.remove("crate_package_digest"))
            .is_some();
        assert!(removed, "baseline inventory must carry crate_package_digest");
        let parsed: Result<CandidateSecurityContract, _> = serde_json::from_value(value);
        let error = match parsed {
            Ok(document) => match validate_contract(&document) {
                Ok(()) => {
                    return Err(eyre!("document missing crate_package_digest unexpectedly passed"));
                }
                Err(error) => error,
            },
            Err(error) => error.into(),
        };
        assert!(error.to_string().contains("crate_package_digest"));
        Ok(())
    }

    #[test]
    fn rejects_blank_container_digest_while_required() -> Result<()> {
        let mut candidate = contract();
        candidate.subjects.container_digest = Some("   ".to_string());
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("blank container digest unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("container_digest is blank"));
        Ok(())
    }

    #[test]
    fn rejects_container_digest_while_not_required() -> Result<()> {
        let mut candidate = contract();
        candidate.container_required = false;
        let error = match validate_contract(&candidate) {
            Ok(()) => {
                return Err(eyre!(
                    "ungoverned container digest unexpectedly passed while not required"
                ));
            }
            Err(error) => error,
        };
        assert!(error.to_string().contains("does not require a container"));
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_duplicate_tool_identity() -> Result<()> {
        // Two declared identities under one tool name leave every rail's
        // authoritative_producer reference ambiguous.
        let mut candidate = contract();
        candidate.tools.insert(ToolIdentity {
            tool: "cargo_deny".to_string(),
            version: "a-different-version".to_string(),
            config_identity: "deny.toml@elsewhere".to_string(),
            database_identity: None,
        });
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("duplicate tool identity unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("duplicate tool identity"));
        Ok(())
    }

    #[test]
    fn rejects_noncanonical_rail_order() -> Result<()> {
        // Same inventory, different serialized bytes: a permutation of the
        // required rails must not validate, or identical contracts could
        // produce different digests.
        let mut candidate = contract();
        candidate.rails.swap(0, 1);
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("noncanonical rail order unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("canonical required_rails() order"));
        Ok(())
    }

    #[test]
    fn rejects_not_applicable_container_while_required() -> Result<()> {
        // A required container cannot be skipped with not_applicable, even
        // with non-blank evidence text attached.
        let mut candidate = contract();
        candidate.rails[3].status = RailStatus::NotApplicable;
        candidate.rails[3].applicability_evidence = Some("unrelated note".to_string());
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("not_applicable container unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("requires a container"));
        Ok(())
    }

    #[test]
    fn rejects_blank_finding_identity_fields() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].findings.push(SecurityFinding {
            finding_id: "   ".to_string(),
            summary: String::new(),
            disposition: FindingDisposition::AcceptedWithDisposition,
        });
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("blank finding fields unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("blank id or summary"));
        Ok(())
    }

    #[test]
    fn rejects_not_proven_rail_without_evidence() -> Result<()> {
        // not_proven that never says what is unproven is structurally a pass
        // with a deferred proof obligation.
        let mut candidate = contract();
        candidate.rails[0].applicability_evidence = None;
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("evidence-free not_proven rail unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("not_proven without applicability evidence"));
        Ok(())
    }

    #[test]
    fn rejects_failed_rail_without_findings_or_evidence() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].status = RailStatus::Failed;
        candidate.rails[0].applicability_evidence = None;
        candidate.rails[0].findings = Vec::new();
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("unexplained failed rail unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("neither findings nor applicability evidence"));
        Ok(())
    }

    #[test]
    fn rejects_pass_rail_carrying_rejected_finding() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].status = RailStatus::Pass;
        candidate.rails[0].findings.push(SecurityFinding {
            finding_id: "F-2".to_string(),
            summary: "human rejected this finding".to_string(),
            disposition: FindingDisposition::Rejected,
        });
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("pass rail with rejected finding unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("rejected finding"));
        Ok(())
    }

    #[test]
    fn rejects_failed_rail_carrying_remediated_finding() -> Result<()> {
        let mut candidate = contract();
        candidate.rails[0].status = RailStatus::Failed;
        candidate.rails[0].findings.push(SecurityFinding {
            finding_id: "F-3".to_string(),
            summary: "already fixed".to_string(),
            disposition: FindingDisposition::Remediated,
        });
        let error = match validate_contract(&candidate) {
            Ok(()) => return Err(eyre!("failed rail with remediated finding unexpectedly passed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("remediated finding"));
        Ok(())
    }

    #[test]
    fn rejects_missing_contract_path() -> Result<()> {
        let error = match load_contract(std::path::Path::new("definitely/absent/contract.json")) {
            Ok(_) => return Err(eyre!("missing contract path unexpectedly loaded")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("definitely/absent"));
        Ok(())
    }

    #[test]
    fn rejects_directory_contract_path() -> Result<()> {
        let dir = std::env::temp_dir()
            .join(format!("candidate-security-contract-dir-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let error = match load_contract(&dir) {
            Ok(_) => return Err(eyre!("directory contract path unexpectedly loaded")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("is not a file"));
        std::fs::remove_dir(&dir)?;
        Ok(())
    }

    #[test]
    fn rejects_oversized_contract_document() -> Result<()> {
        let path = std::env::temp_dir()
            .join(format!("candidate-security-contract-oversized-{}.json", std::process::id()));
        std::fs::write(&path, vec![b'x'; MAX_CONTRACT_BYTES as usize + 1])?;
        let error = match load_contract(&path) {
            Ok(_) => return Err(eyre!("oversized contract unexpectedly loaded")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("closed-contract limit"));
        std::fs::remove_file(&path)?;
        Ok(())
    }
}
