//! Typed no-publish side-effect surface inventory (`no_publish_side_effects.v1`)
//! derived from the canonical release topology.
//!
//! Schema and external-surface inventory definition only (#9414): this module
//! owns the closed contract shape, the closed mutation-authority and
//! public-state vocabularies, and fail-closed validation. It parses no
//! workflow, observes no endpoint, executes no rehearsal, and mutates no
//! public channel — those belong to the exact publisher/observer claims this
//! schema is a prerequisite for.

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Schema identity for the inventory governed here. Serialized receipts must
/// carry exactly this string; anything else is a different contract.
pub const INVENTORY_SCHEMA: &str = "no_publish_side_effects.v1";

#[derive(Debug, Parser)]
#[command(
    name = "no-publish-side-effects",
    about = "Validate a no_publish_side_effects.v1 surface inventory document"
)]
struct Cli {
    /// Path to the inventory JSON document.
    #[arg(long)]
    inventory: PathBuf,
}

/// The closed no-publish side-effect inventory. Every field is required and
/// unknown fields are rejected, so an under-specified or over-specified
/// document fails instead of silently narrowing the inventoried surface.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NoPublishSideEffectsInventory {
    pub schema_version: String,
    /// Digest of the canonical release topology this inventory derives from.
    /// The inventory is a pure projection of that topology, so the binding is
    /// mandatory.
    pub topology_digest: String,
    /// One row per topology-required or explicitly deferred channel or public
    /// subject class, in deterministic order.
    pub surfaces: Vec<PublishSurface>,
}

/// Whether an inventoried surface is a release channel or a public subject
/// class. The topology requires both kinds to carry rows.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceClass {
    Channel,
    PublicSubjectClass,
}

/// Whether the release topology requires this surface now or defers it.
/// Deferral is explicit: a deferred surface still carries a full row, so
/// omission can never masquerade as deferral.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TopologyState {
    Required,
    Deferred,
}

/// Positive applicability state. A deferred channel may only be represented as
/// `unchanged` when this state is `applicable` and the deferral evidence names
/// the topology rule that defers it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    Applicable,
    NotApplicable,
}

/// Closed mutation-authority vocabulary. The value records the authority class
/// the release-candidate path holds over the surface; `public_mutation_authorized`
/// contradicts the no-publish receipt and fails validation.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MutationAuthority {
    ReadOnly,
    ArtifactLocalWrite,
    CandidateEvidenceWrite,
    PublicMutationDisabled,
    PublicMutationUnreachable,
    PublicMutationAuthorized,
    NotProven,
}

/// Closed public-state vocabulary for the expected before/after comparison of
/// one surface across the release-candidate path.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PublicState {
    Unchanged,
    Changed,
    NotApplicable,
    NotProven,
}

/// One external surface row. Every field is required (optionality is explicit)
/// so a surface cannot quietly drop its observer, authority rule, or owner.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublishSurface {
    /// Stable surface ID shared by the topology and this inventory.
    pub surface_id: String,
    pub surface_class: SurfaceClass,
    /// Exact subject identity the surface binds.
    pub subject_identity: String,
    /// Observer requirement: what must compare before/after state for this
    /// surface. Every row names one; a required surface without an observer
    /// fails closed.
    pub observer: String,
    /// Authority rule naming how mutation authority over this surface is
    /// decided on the release-candidate path.
    pub authority_rule: String,
    pub mutation_authority: MutationAuthority,
    pub topology_state: TopologyState,
    pub applicability: Applicability,
    /// Topology evidence for the applicability/deferral decision. Required for
    /// every deferred row; omission is not deferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applicability_evidence: Option<String>,
    /// Expected before/after comparison of the surface's public state.
    pub expected_public_state: PublicState,
    pub owner: String,
    pub claim_boundary: String,
}

/// The canonical release topology's required no-publish surface IDs, in
/// canonical order. A newly required topology channel fails the inventory
/// until a row names its observer and authority rule.
pub fn required_surface_ids() -> [&'static str; 6] {
    [
        "crates_io_crate_publication",
        "vsix_marketplace_publication",
        "github_release_publication",
        "container_registry_publication",
        "checksums_manifest_subject",
        "sbom_artifact_subject",
    ]
}

/// Validate `inventory` fully, failing closed on every structural hazard the
/// issue names. Empty result means the inventory is well-formed; it makes no
/// claim about any real release candidate.
pub fn validate_inventory(inventory: &NoPublishSideEffectsInventory) -> Result<()> {
    if inventory.schema_version != INVENTORY_SCHEMA {
        bail!(
            "unknown schema_version {:?}; expected {:?}",
            inventory.schema_version,
            INVENTORY_SCHEMA
        );
    }
    if inventory.topology_digest.trim().is_empty() {
        bail!("required topology_digest is omitted");
    }

    let mut seen = BTreeSet::new();
    for surface in &inventory.surfaces {
        if surface.surface_id.trim().is_empty() {
            bail!("inventoried surface names no stable surface id");
        }
        if !seen.insert(surface.surface_id.as_str()) {
            bail!("duplicate surface id {:?}", surface.surface_id);
        }
        if surface.subject_identity.trim().is_empty() {
            bail!("surface {:?} names no subject identity", surface.surface_id);
        }
        if surface.observer.trim().is_empty() {
            bail!("surface {:?} names no observer requirement", surface.surface_id);
        }
        if surface.authority_rule.trim().is_empty() {
            bail!("surface {:?} names no authority rule", surface.surface_id);
        }
        if surface.owner.trim().is_empty() {
            bail!("surface {:?} has no owner", surface.surface_id);
        }
        if surface.claim_boundary.trim().is_empty() {
            bail!("surface {:?} has no claim boundary", surface.surface_id);
        }
        if surface.mutation_authority == MutationAuthority::PublicMutationAuthorized {
            bail!(
                "surface {:?} claims public_mutation_authorized, which contradicts \
                 the no_publish_side_effects receipt",
                surface.surface_id
            );
        }
        if surface.topology_state == TopologyState::Deferred {
            let evidence = surface.applicability_evidence.as_deref().map(str::trim).unwrap_or("");
            if evidence.is_empty() {
                bail!(
                    "surface {:?} is deferred without naming the topology rule that \
                     defers it; omission is not deferred",
                    surface.surface_id
                );
            }
            if surface.expected_public_state == PublicState::Unchanged
                && surface.applicability != Applicability::Applicable
            {
                bail!(
                    "deferred surface {:?} is represented as unchanged without a \
                     positive applicability state",
                    surface.surface_id
                );
            }
        }
        if surface.applicability == Applicability::NotApplicable
            && surface.expected_public_state != PublicState::NotApplicable
        {
            bail!(
                "surface {:?} is not applicable but expects {:?}; a surface without \
                 applicability cannot carry a concrete public-state expectation",
                surface.surface_id,
                public_state_label(surface.expected_public_state)
            );
        }
    }

    for required in required_surface_ids() {
        if !seen.contains(required) {
            bail!("required topology surface {:?} is absent from the inventory", required);
        }
    }
    for surface in &inventory.surfaces {
        if required_surface_ids().contains(&surface.surface_id.as_str())
            && surface.topology_state == TopologyState::Deferred
        {
            bail!(
                "required topology surface {:?} cannot be inventoried as deferred; \
                 deferral cannot silently remove a required surface",
                surface.surface_id
            );
        }
    }

    Ok(())
}

fn public_state_label(state: PublicState) -> &'static str {
    match state {
        PublicState::Unchanged => "unchanged",
        PublicState::Changed => "changed",
        PublicState::NotApplicable => "not_applicable",
        PublicState::NotProven => "not_proven",
    }
}

/// Serialize the inventory to its canonical schema projection and prove the
/// projection round-trips byte-identically. Any divergence between the
/// generated projection and the schema model fails closed.
pub fn canonical_projection(inventory: &NoPublishSideEffectsInventory) -> Result<String> {
    let projection = serde_json::to_string(inventory)
        .wrap_err("serializing no_publish_side_effects.v1 projection")?;
    let reparsed: NoPublishSideEffectsInventory = serde_json::from_str(&projection)
        .wrap_err("generated projection diverges from schema: reparse failed")?;
    let reprojected = serde_json::to_string(&reparsed)
        .wrap_err("re-serializing no_publish_side_effects.v1 projection")?;
    if reprojected != projection {
        bail!("generated projection diverges from schema");
    }
    Ok(projection)
}

/// Build the deterministic topology-derived inventory: every required surface
/// present with explicit subject identity, observer requirement, authority
/// class, applicability, before/after expectation, owner, and claim boundary.
/// Values name their rule, not observed results — this constructor exists so
/// the inventory shape is executable and provable before any observer or
/// publisher runs.
///
/// `#[allow(dead_code)]`: no production call site in the `xtask` bin builds
/// the inventory yet (#9414 lands the schema/inventory only). The diagnostic
/// bin and this module's own unit tests consume it.
#[allow(dead_code)]
pub fn derived_inventory(topology_digest: &str) -> NoPublishSideEffectsInventory {
    let owner = "issue-9414".to_string();
    let claim_boundary = "schema/inventory only; no workflow parsing, endpoint observation, or \
         public mutation in this claim"
        .to_string();

    let required = |surface_id: &str,
                    surface_class: SurfaceClass,
                    subject_identity: &str,
                    observer: &str,
                    authority_rule: &str,
                    mutation_authority: MutationAuthority| PublishSurface {
        surface_id: surface_id.to_string(),
        surface_class,
        subject_identity: subject_identity.to_string(),
        observer: observer.to_string(),
        authority_rule: authority_rule.to_string(),
        mutation_authority,
        topology_state: TopologyState::Required,
        applicability: Applicability::Applicable,
        applicability_evidence: Some(
            "topology requires this surface for the release candidate".to_string(),
        ),
        expected_public_state: PublicState::Unchanged,
        owner: owner.clone(),
        claim_boundary: claim_boundary.clone(),
    };

    NoPublishSideEffectsInventory {
        schema_version: INVENTORY_SCHEMA.to_string(),
        topology_digest: topology_digest.to_string(),
        surfaces: vec![
            required(
                "crates_io_crate_publication",
                SurfaceClass::Channel,
                "perllsp crate on crates.io",
                "crates.io registry index and crate page before/after diff",
                "no publish credential is reachable from the release-candidate path",
                MutationAuthority::PublicMutationDisabled,
            ),
            required(
                "vsix_marketplace_publication",
                SurfaceClass::Channel,
                "perllsp VSIX on the Visual Studio Code Marketplace",
                "Marketplace extension listing before/after diff",
                "marketplace publisher token is absent from the release-candidate path",
                MutationAuthority::PublicMutationDisabled,
            ),
            required(
                "github_release_publication",
                SurfaceClass::Channel,
                "GitHub release objects and tags for perllsp",
                "GitHub releases and refs API before/after diff",
                "release mutation scope is disabled on the release-candidate path",
                MutationAuthority::PublicMutationDisabled,
            ),
            required(
                "container_registry_publication",
                SurfaceClass::Channel,
                "perllsp container image in the release registry",
                "registry manifest digest before/after diff",
                "registry write credentials are unreachable from the release-candidate path",
                MutationAuthority::PublicMutationUnreachable,
            ),
            required(
                "checksums_manifest_subject",
                SurfaceClass::PublicSubjectClass,
                "checksums manifest shipped with the release candidate",
                "candidate-local checksums manifest digest comparison",
                "checksums are written only as candidate-local artifacts",
                MutationAuthority::ArtifactLocalWrite,
            ),
            required(
                "sbom_artifact_subject",
                SurfaceClass::PublicSubjectClass,
                "SBOM document shipped with the release candidate",
                "candidate-local SBOM digest comparison",
                "SBOM generation writes only candidate-local artifacts",
                MutationAuthority::ArtifactLocalWrite,
            ),
            PublishSurface {
                surface_id: "open_vsx_publication".to_string(),
                surface_class: SurfaceClass::Channel,
                subject_identity: "perllsp VSIX on Open VSX".to_string(),
                observer: "Open VSX extension listing before/after diff".to_string(),
                authority_rule:
                    "open-vsx publisher token is absent from the release-candidate path".to_string(),
                mutation_authority: MutationAuthority::PublicMutationUnreachable,
                topology_state: TopologyState::Deferred,
                applicability: Applicability::NotApplicable,
                applicability_evidence: Some(
                    "topology defers Open VSX publication to a later train; the \
                     deferral is explicit, not an omission"
                        .to_string(),
                ),
                expected_public_state: PublicState::NotApplicable,
                owner: owner.clone(),
                claim_boundary: claim_boundary.clone(),
            },
        ],
    }
}

/// `#[allow(dead_code)]`: the `xtask` bin dispatches no subcommand here; this
/// entry point is consumed by the `no-publish-side-effects` diagnostic bin.
#[allow(dead_code)]
pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    let text = std::fs::read_to_string(&cli.inventory)
        .with_context(|| format!("reading inventory {}", cli.inventory.display()))?;
    let inventory: NoPublishSideEffectsInventory = serde_json::from_str(&text)
        .with_context(|| format!("parsing inventory {}", cli.inventory.display()))?;
    validate_inventory(&inventory)?;
    // The generated projection must round-trip the closed schema before the
    // document is reported valid; divergence fails closed.
    canonical_projection(&inventory)?;
    println!("no_publish_side_effects inventory {} is closed and valid", cli.inventory.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{WrapErr, eyre};

    fn inventory() -> NoPublishSideEffectsInventory {
        derived_inventory("<release topology digest>")
    }

    fn rejection_error(candidate: &NoPublishSideEffectsInventory) -> color_eyre::eyre::Error {
        match validate_inventory(candidate) {
            Ok(()) => eyre!("invalid inventory unexpectedly passed"),
            Err(error) => error,
        }
    }

    #[test]
    fn derived_inventory_is_valid_and_deterministic() -> Result<()> {
        validate_inventory(&inventory())?;
        let left = canonical_projection(&inventory())?;
        let right = canonical_projection(&inventory())?;
        assert_eq!(left, right, "identical inputs must project identically");
        Ok(())
    }

    #[test]
    fn rejects_unknown_schema_version() -> Result<()> {
        let mut candidate = inventory();
        candidate.schema_version = "no_publish_side_effects.v2".to_string();
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("unknown schema_version"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_required_surface_with_no_row() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces.retain(|surface| surface.surface_id != "crates_io_crate_publication");
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("absent from the inventory"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_duplicate_surface_id() -> Result<()> {
        let mut candidate = inventory();
        let duplicate = candidate.surfaces[0].clone();
        candidate.surfaces.push(duplicate);
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("duplicate surface id"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_required_surface_silently_reclassified_as_deferred() -> Result<()> {
        let mut candidate = inventory();
        let required = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "crates_io_crate_publication")
            .ok_or_else(|| eyre!("derived inventory lost a required surface"))?;
        required.topology_state = TopologyState::Deferred;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("cannot be inventoried as deferred"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_deferred_channel_as_unchanged_without_positive_applicability() -> Result<()> {
        let mut candidate = inventory();
        let deferred = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "open_vsx_publication")
            .ok_or_else(|| eyre!("derived inventory lost the deferred surface"))?;
        deferred.expected_public_state = PublicState::Unchanged;
        deferred.applicability = Applicability::NotApplicable;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("without a positive applicability state"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_deferred_surface_without_named_deferral_rule() -> Result<()> {
        let mut candidate = inventory();
        let deferred = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "open_vsx_publication")
            .ok_or_else(|| eyre!("derived inventory lost the deferred surface"))?;
        deferred.applicability_evidence = None;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("omission is not deferred"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_surface_with_no_owner() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].owner = "  ".to_string();
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("has no owner"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_surface_with_no_observer() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].observer = "  ".to_string();
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("names no observer requirement"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_surface_with_no_authority_rule() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].authority_rule = "  ".to_string();
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("names no authority rule"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_public_mutation_authorized_authority() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].mutation_authority = MutationAuthority::PublicMutationAuthorized;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("contradicts"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_not_applicable_surface_with_concrete_public_state() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].applicability = Applicability::NotApplicable;
        candidate.surfaces[0].applicability_evidence = None;
        candidate.surfaces[0].expected_public_state = PublicState::Unchanged;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("cannot carry a concrete public-state"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_generated_projection_divergence() -> Result<()> {
        let expected = inventory();
        let projection = canonical_projection(&expected)?;
        // A generated projection that drifts from the schema-derived inventory
        // (here: a rewritten owner) must be detectable by comparison.
        let tampered = projection.replace(r#""owner":"issue-9414""#, r#""owner":"issue-9999""#);
        assert_ne!(projection, tampered, "tamper must change the projection");
        let reparsed: NoPublishSideEffectsInventory = serde_json::from_str(&tampered)
            .wrap_err("tampered projection must still parse to be comparable")?;
        assert_ne!(reparsed, expected, "divergence from the derived inventory must be detected");
        Ok(())
    }

    #[test]
    fn rejects_unknown_document_fields() -> Result<()> {
        let text = r#"{"schema_version":"no_publish_side_effects.v1","extra":true}"#;
        let parsed: Result<NoPublishSideEffectsInventory, _> = serde_json::from_str(text);
        assert!(parsed.is_err(), "closed inventory must deny unknown fields");
        Ok(())
    }
}
