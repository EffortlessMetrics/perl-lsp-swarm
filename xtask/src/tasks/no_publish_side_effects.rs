//! Typed no-publish side-effect surface inventory (`no_publish_side_effects.v1`)
//! derived from the canonical release topology.
//!
//! Schema and external-surface inventory definition only (#9414): this module
//! owns the closed contract shape, the closed mutation-authority and
//! public-state vocabularies, and fail-closed validation. It parses no
//! workflow, observes no endpoint, executes no rehearsal, and mutates no
//! public channel — those belong to the exact publisher/observer claims this
//! schema is a prerequisite for.
//!
//! The inventory binds to the canonical release-topology artifact
//! (`release-topology.json`, produced by `scripts/generate_release_topology.py`):
//! the artifact bytes carry a typed `sha256:<hex>` digest, and the closed
//! surface authority — every inventoried `(id, class, topology_state, subject
//! denominator)` tuple — is derived from those bytes, so the receipt certifies
//! a named topology state instead of an unbound document shape.

use clap::Parser;
use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Schema identity for the inventory governed here. Serialized receipts must
/// carry exactly this string; anything else is a different contract.
pub const INVENTORY_SCHEMA: &str = "no_publish_side_effects.v1";

/// Canonical `topology_digest` format: `sha256:` followed by 64 lowercase hex
/// digits over the release-topology artifact bytes. Matches the digest format
/// enforced by `scripts/validate_public_release_claims.py`.
pub const TOPOLOGY_DIGEST_PREFIX: &str = "sha256:";

#[derive(Debug, Parser)]
#[command(
    name = "no-publish-side-effects",
    about = "Validate a no_publish_side_effects.v1 surface inventory document against \
             the canonical release topology"
)]
struct Cli {
    /// Path to the inventory JSON document.
    #[arg(long)]
    inventory: PathBuf,
    /// Path to the canonical release-topology.json artifact the inventory
    /// must bind to.
    #[arg(long)]
    topology: PathBuf,
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
    /// mandatory, typed (`sha256:<64 lowercase hex>`), and checked against the
    /// canonical artifact bytes — a non-empty but stale digest fails.
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

/// The closed-world surface authority derived from one canonical
/// release-topology artifact: for every surface the topology governs, its
/// required class, required or deferred state, and exact subject denominator.
/// The inventory is validated against this authority, so an invented surface,
/// a missing surface, a class swap, or a denominator rewrite fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologyAuthority {
    /// Typed digest over the artifact bytes this authority was derived from.
    pub digest: String,
    /// Surface id -> exact topology specification, in canonical (sorted) order.
    pub surfaces: BTreeMap<String, TopologySurfaceSpec>,
}

/// The exact topology specification of one surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopologySurfaceSpec {
    pub surface_class: SurfaceClass,
    pub topology_state: TopologyState,
    /// Deferral evidence required when `topology_state` is `Deferred`.
    pub deferral_evidence: Option<String>,
    /// Exact subject denominator: every subject the surface binds, in
    /// canonical (sorted) order.
    pub subjects: Vec<String>,
}

impl TopologySurfaceSpec {
    /// The canonical rendered subject denominator an inventory row must name.
    pub fn subject_denominator(&self) -> String {
        self.subjects.join("; ")
    }
}

/// Channel-name (topology manifest vocabulary) -> inventory surface id.
fn channel_surface_id(channel: &str) -> Option<&'static str> {
    match channel {
        "github_release" => Some("github_release_publication"),
        "crates_io" => Some("crates_io_crate_publication"),
        "vscode_marketplace" => Some("vsix_marketplace_publication"),
        "open_vsx" => Some("open_vsx_publication"),
        "docker" => Some("container_registry_publication"),
        "homebrew" => Some("homebrew_publication"),
        _ => None,
    }
}

/// Compute the typed digest over the release-topology artifact bytes.
fn topology_digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{TOPOLOGY_DIGEST_PREFIX}{}", hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Derive the closed surface authority from one canonical release-topology
/// artifact. The artifact bytes are the authority: the digest is computed over
/// exactly those bytes, and every surface tuple is derived from the manifest
/// fields (`primary_channels`, `secondary_channels`, `published_crates`,
/// `binary_targets`) that `scripts/generate_release_topology.py` owns.
pub fn load_topology_authority(path: &Path) -> Result<TopologyAuthority> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading release topology {}", path.display()))?;
    let digest = topology_digest(&bytes);
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing release topology {}", path.display()))?;

    let mut surfaces = BTreeMap::new();
    let primary_channels =
        manifest.get("primary_channels").and_then(|value| value.as_array()).ok_or_else(|| {
            color_eyre::eyre::eyre!("release topology is missing primary_channels array")
        })?;
    for channel in primary_channels {
        let channel = channel.as_str().ok_or_else(|| {
            color_eyre::eyre::eyre!("release topology primary_channels carries a non-string entry")
        })?;
        let surface_id = channel_surface_id(channel).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "release topology names primary channel {channel:?} with no closed \
                 surface mapping"
            )
        })?;
        surfaces.insert(
            surface_id.to_string(),
            TopologySurfaceSpec {
                surface_class: SurfaceClass::Channel,
                topology_state: TopologyState::Required,
                deferral_evidence: None,
                subjects: vec![channel_subject_denominator(surface_id, &manifest)?],
            },
        );
    }

    let secondary_channels =
        manifest.get("secondary_channels").and_then(|value| value.as_object()).ok_or_else(
            || color_eyre::eyre::eyre!("release topology is missing secondary_channels object"),
        )?;
    for (channel, state) in secondary_channels {
        let state = state.as_str().ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "release topology secondary_channels.{channel} carries a non-string state"
            )
        })?;
        let surface_id = channel_surface_id(channel).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "release topology names secondary channel {channel:?} with no closed \
                 surface mapping"
            )
        })?;
        let topology_state = match state {
            "required" => TopologyState::Required,
            "deferred" => TopologyState::Deferred,
            other => {
                bail!("release topology secondary_channels.{channel} has unknown state {other:?}")
            }
        };
        let deferral_evidence = match topology_state {
            TopologyState::Deferred => Some(format!(
                "release topology secondary_channels records {channel} as deferred; \
                 the deferral is explicit, not an omission"
            )),
            TopologyState::Required => None,
        };
        surfaces.insert(
            surface_id.to_string(),
            TopologySurfaceSpec {
                surface_class: SurfaceClass::Channel,
                topology_state,
                deferral_evidence,
                subjects: vec![channel_subject_denominator(surface_id, &manifest)?],
            },
        );
    }

    // Public subject classes the release candidate ships: one checksums
    // manifest per archive and one SPDX SBOM document, both candidate-local
    // artifacts the topology artifact enumerates.
    let binary_targets =
        manifest.get("binary_targets").and_then(|value| value.as_array()).ok_or_else(|| {
            color_eyre::eyre::eyre!("release topology is missing binary_targets array")
        })?;
    let mut checksum_subjects = Vec::new();
    for target in binary_targets {
        let archive =
            target.get("archive_name").and_then(|value| value.as_str()).ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "release topology binary_targets entry carries no archive_name"
                )
            })?;
        checksum_subjects.push(format!("SHA256SUMS.txt for {archive}"));
    }
    checksum_subjects.sort();
    surfaces.insert(
        "checksums_manifest_subject".to_string(),
        TopologySurfaceSpec {
            surface_class: SurfaceClass::PublicSubjectClass,
            topology_state: TopologyState::Required,
            deferral_evidence: None,
            subjects: checksum_subjects,
        },
    );
    surfaces.insert(
        "sbom_artifact_subject".to_string(),
        TopologySurfaceSpec {
            surface_class: SurfaceClass::PublicSubjectClass,
            topology_state: TopologyState::Required,
            deferral_evidence: None,
            subjects: vec!["sbom-spdx.json (SPDX JSON 2.3)".to_string()],
        },
    );

    Ok(TopologyAuthority { digest, surfaces })
}

/// The exact subject denominator of one channel surface, derived from the
/// manifest so the inventory cannot quietly narrow or widen the bound
/// subjects.
fn channel_subject_denominator(surface_id: &str, manifest: &serde_json::Value) -> Result<String> {
    match surface_id {
        // The crates.io surface binds every published crate the topology
        // enumerates — never a single hand-written crate name.
        "crates_io_crate_publication" => {
            let crates = manifest
                .get("published_crates")
                .and_then(|value| value.as_array())
                .ok_or_else(|| {
                    color_eyre::eyre::eyre!("release topology is missing published_crates array")
                })?;
            let mut names = Vec::new();
            for entry in crates {
                let name = entry.get("name").and_then(|value| value.as_str()).ok_or_else(|| {
                    color_eyre::eyre::eyre!(
                        "release topology published_crates entry carries no name"
                    )
                })?;
                names.push(format!("crates.io:{name}"));
            }
            names.sort();
            if names.is_empty() {
                bail!("release topology published_crates is empty");
            }
            Ok(names.join("; "))
        }
        // The docker channel publishes two images (the release image and the
        // perl-runtime image) to two registries (ghcr.io and Docker Hub):
        // four independently mutable registry/image subjects, enumerated
        // exactly.
        "container_registry_publication" => {
            let mut subjects = vec![
                "container image ghcr.io release runtime image".to_string(),
                "container image ghcr.io perl runtime image".to_string(),
                "container image docker.io release runtime image".to_string(),
                "container image docker.io perl runtime image".to_string(),
            ];
            subjects.sort();
            Ok(subjects.join("; "))
        }
        "github_release_publication" => {
            Ok("GitHub release objects and tags for the release candidate".to_string())
        }
        "vsix_marketplace_publication" => {
            Ok("release VSIX on the Visual Studio Code Marketplace".to_string())
        }
        "open_vsx_publication" => Ok("release VSIX on Open VSX".to_string()),
        "homebrew_publication" => Ok("Homebrew formula for the release candidate".to_string()),
        other => bail!("channel surface {other:?} has no subject denominator derivation"),
    }
}

/// Per-surface observation policy: what must be compared, under which
/// authority rule, with which mutation-authority class. Keyed by surface id;
/// an authority surface without a policy row fails the derivation.
fn observation_policy(surface_id: &str) -> Option<(&'static str, &'static str, MutationAuthority)> {
    match surface_id {
        "crates_io_crate_publication" => Some((
            "crates.io registry index and crate pages before/after diff for every bound crate",
            "no publish credential is reachable from the release-candidate path",
            MutationAuthority::PublicMutationDisabled,
        )),
        "vsix_marketplace_publication" => Some((
            "Marketplace extension listing before/after diff",
            "marketplace publisher token is absent from the release-candidate path",
            MutationAuthority::PublicMutationDisabled,
        )),
        "github_release_publication" => Some((
            "GitHub releases and refs API before/after diff",
            "release mutation scope is disabled on the release-candidate path",
            MutationAuthority::PublicMutationDisabled,
        )),
        "container_registry_publication" => Some((
            "registry manifest digest before/after diff for every bound registry/image subject",
            "registry write credentials are unreachable from the release-candidate path",
            MutationAuthority::PublicMutationUnreachable,
        )),
        "open_vsx_publication" => Some((
            "Open VSX extension listing before/after diff",
            "open-vsx publisher token is absent from the release-candidate path",
            MutationAuthority::PublicMutationUnreachable,
        )),
        "homebrew_publication" => Some((
            "Homebrew formula and bottle before/after diff",
            "no homebrew tap write credential is reachable from the release-candidate path",
            MutationAuthority::PublicMutationUnreachable,
        )),
        "checksums_manifest_subject" => Some((
            "candidate-local checksums manifest digest comparison",
            "checksums are written only as candidate-local artifacts",
            MutationAuthority::ArtifactLocalWrite,
        )),
        "sbom_artifact_subject" => Some((
            "candidate-local SBOM digest comparison",
            "SBOM generation writes only candidate-local artifacts",
            MutationAuthority::ArtifactLocalWrite,
        )),
        _ => None,
    }
}

/// Validate `inventory` fully, failing closed on every structural hazard the
/// issue names. Empty result means the inventory is well-formed; it makes no
/// claim about any real release candidate. Binding to the canonical topology
/// bytes is a separate, stronger check: `validate_topology_binding`.
pub fn validate_inventory(inventory: &NoPublishSideEffectsInventory) -> Result<()> {
    if inventory.schema_version != INVENTORY_SCHEMA {
        bail!(
            "unknown schema_version {:?}; expected {:?}",
            inventory.schema_version,
            INVENTORY_SCHEMA
        );
    }
    let digest = inventory.topology_digest.trim();
    if digest.is_empty() {
        bail!("required topology_digest is omitted");
    }
    let well_formed = digest.len() == TOPOLOGY_DIGEST_PREFIX.len() + 64
        && digest.starts_with(TOPOLOGY_DIGEST_PREFIX)
        && digest[TOPOLOGY_DIGEST_PREFIX.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !well_formed {
        bail!(
            "topology_digest {:?} is malformed; expected {}<64 lowercase hex> over the \
             canonical release topology bytes",
            inventory.topology_digest,
            TOPOLOGY_DIGEST_PREFIX
        );
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
        // Fail-closed symmetry (#14305 review): `changed` is as much a
        // publish side effect as `public_mutation_authorized`. A no-publish
        // receipt cannot expect the surface's public state to move.
        if surface.expected_public_state == PublicState::Changed {
            bail!(
                "surface {:?} expects expected_public_state changed, which contradicts \
                 the no_publish_side_effects receipt exactly as public_mutation_authorized \
                 does",
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
        // Closed-world class/state join (#14305 review): a topology-required
        // surface cannot be marked not applicable any more than it can be
        // marked deferred.
        if surface.topology_state == TopologyState::Required
            && surface.applicability == Applicability::NotApplicable
        {
            bail!(
                "required topology surface {:?} is marked not applicable; a required \
                 surface cannot shed applicability any more than it can shed its row",
                surface.surface_id
            );
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

    Ok(())
}

/// Bind the inventory to the canonical release-topology artifact: the typed
/// digest must match the artifact bytes, and the inventoried surface set must
/// be exactly the closed authority — no omitted surface, no invented surface,
/// no class or topology-state swap, no rewritten subject denominator.
pub fn validate_topology_binding(
    inventory: &NoPublishSideEffectsInventory,
    authority: &TopologyAuthority,
) -> Result<()> {
    if inventory.topology_digest.trim() != authority.digest {
        bail!(
            "topology_digest {:?} does not match the canonical release topology bytes \
             ({:?}); the inventory is stale or fabricated",
            inventory.topology_digest,
            authority.digest
        );
    }
    let mut inventoried = BTreeSet::new();
    for surface in &inventory.surfaces {
        inventoried.insert(surface.surface_id.as_str());
    }
    for required in authority.surfaces.keys() {
        if !inventoried.contains(required.as_str()) {
            bail!("required topology surface {:?} is absent from the inventory", required);
        }
    }
    for surface in &inventory.surfaces {
        let spec = authority.surfaces.get(&surface.surface_id).ok_or_else(|| {
            color_eyre::eyre::eyre!(
                "surface {:?} is absent from the canonical release topology; invented \
                 surfaces fail the closed-world authority",
                surface.surface_id
            )
        })?;
        if surface.surface_class != spec.surface_class {
            bail!(
                "surface {:?} is inventoried as {:?} but the topology requires {:?}; \
                 class swaps fail the closed-world authority",
                surface.surface_id,
                surface.surface_class,
                spec.surface_class
            );
        }
        if surface.topology_state != spec.topology_state {
            bail!(
                "surface {:?} is inventoried as {:?} but the topology records {:?}; \
                 topology-state swaps fail the closed-world authority",
                surface.surface_id,
                surface.topology_state,
                spec.topology_state
            );
        }
        let expected_applicability = match spec.topology_state {
            TopologyState::Required => Applicability::Applicable,
            TopologyState::Deferred => Applicability::NotApplicable,
        };
        if surface.applicability != expected_applicability {
            bail!(
                "surface {:?} carries applicability {:?} but the topology state {:?} \
                 requires {:?}",
                surface.surface_id,
                surface.applicability,
                spec.topology_state,
                expected_applicability
            );
        }
        let expected_denominator = spec.subject_denominator();
        if surface.subject_identity != expected_denominator {
            bail!(
                "surface {:?} binds subject identity {:?} but the topology denominator \
                 is {:?}; subject rewrites fail the closed-world authority",
                surface.surface_id,
                surface.subject_identity,
                expected_denominator
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
/// projection round-trips byte-identically. Surface rows are emitted in
/// canonical (surface-id) order, so a reordered input produces the same
/// canonical bytes; any divergence between the generated projection and the
/// schema model fails closed.
pub fn canonical_projection(inventory: &NoPublishSideEffectsInventory) -> Result<String> {
    let mut canonical = inventory.clone();
    canonical.surfaces.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
    let projection = serde_json::to_string(&canonical)
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

/// Build the deterministic topology-derived inventory from the closed surface
/// authority: every authority surface present with explicit subject identity,
/// observer requirement, authority class, applicability, before/after
/// expectation, owner, and claim boundary. Values name their rule, not
/// observed results — this constructor exists so the inventory shape is
/// executable and provable before any observer or publisher runs.
pub fn derive_inventory(authority: &TopologyAuthority) -> Result<NoPublishSideEffectsInventory> {
    let owner = "issue-9414".to_string();
    let claim_boundary = "schema/inventory only; no workflow parsing, endpoint observation, or \
         public mutation in this claim"
        .to_string();

    let mut surfaces = Vec::new();
    for (surface_id, spec) in &authority.surfaces {
        let (observer, authority_rule, mutation_authority) = observation_policy(surface_id)
            .ok_or_else(|| {
                color_eyre::eyre::eyre!(
                    "topology surface {surface_id:?} has no observation policy row"
                )
            })?;
        let expected_public_state = match spec.topology_state {
            TopologyState::Required => PublicState::Unchanged,
            TopologyState::Deferred => PublicState::NotApplicable,
        };
        let applicability = match spec.topology_state {
            TopologyState::Required => Applicability::Applicable,
            TopologyState::Deferred => Applicability::NotApplicable,
        };
        surfaces.push(PublishSurface {
            surface_id: surface_id.clone(),
            surface_class: spec.surface_class,
            subject_identity: spec.subject_denominator(),
            observer: observer.to_string(),
            authority_rule: authority_rule.to_string(),
            mutation_authority,
            topology_state: spec.topology_state,
            applicability,
            applicability_evidence: spec.deferral_evidence.clone().or_else(|| {
                Some("topology requires this surface for the release candidate".to_string())
            }),
            expected_public_state,
            owner: owner.clone(),
            claim_boundary: claim_boundary.clone(),
        });
    }

    Ok(NoPublishSideEffectsInventory {
        schema_version: INVENTORY_SCHEMA.to_string(),
        topology_digest: authority.digest.clone(),
        surfaces,
    })
}

/// Validate one inventory document against one canonical release-topology
/// artifact through the production validation path: closed schema, typed
/// topology binding, closed-world surface authority, and canonical projection.
pub fn run(inventory_path: PathBuf, topology_path: PathBuf) -> Result<()> {
    let authority = load_topology_authority(&topology_path)
        .with_context(|| format!("loading topology authority {}", topology_path.display()))?;
    let text = std::fs::read_to_string(&inventory_path)
        .with_context(|| format!("reading inventory {}", inventory_path.display()))?;
    let inventory: NoPublishSideEffectsInventory = serde_json::from_str(&text)
        .with_context(|| format!("parsing inventory {}", inventory_path.display()))?;
    validate_topology_binding(&inventory, &authority)?;
    validate_inventory(&inventory)?;
    // The generated projection must round-trip the closed schema before the
    // document is reported valid; divergence fails closed.
    canonical_projection(&inventory)?;
    println!(
        "no_publish_side_effects inventory {} binds release topology {} and is closed and valid",
        inventory_path.display(),
        topology_path.display()
    );
    Ok(())
}

/// `no-publish-side-effects` diagnostic bin entry point: parses the document
/// and topology paths, then runs the production validation path.
pub fn run_cli() -> Result<()> {
    let cli = Cli::parse();
    run(cli.inventory, cli.topology)
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::{WrapErr, eyre};

    /// A minimal but structurally faithful release-topology artifact: the
    /// manifest fields the closed authority derives from, matching
    /// `scripts/generate_release_topology.py`'s accepted v0.18 sets.
    fn topology_artifact() -> serde_json::Value {
        serde_json::json!({
            "schema": 1,
            "release": "0.0.0-test",
            "published_crates": [
                {"name": "perllsp", "version": "0.0.0-test", "package_path": "crates/perllsp"},
                {"name": "perl-dap", "version": "0.0.0-test", "package_path": "crates/perl-dap"}
            ],
            "binary_targets": [
                {"archive_name": "perllsp-0.0.0-test-x86_64-unknown-linux-gnu.tar.gz"},
                {"archive_name": "perllsp-0.0.0-test-aarch64-apple-darwin.tar.gz"}
            ],
            "primary_channels": ["github_release", "crates_io", "vscode_marketplace", "open_vsx"],
            "secondary_channels": {"docker": "required", "homebrew": "deferred"}
        })
    }

    fn authority() -> TopologyAuthority {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("release-topology.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&topology_artifact()).expect("serialize topology"),
        )
        .expect("write topology");
        // The authority is pure derived data; keep no reference to the temp
        // directory's lifetime.
        load_topology_authority(&path).expect("topology authority")
    }

    fn inventory() -> NoPublishSideEffectsInventory {
        derive_inventory(&authority()).expect("derived inventory")
    }

    fn rejection_error(candidate: &NoPublishSideEffectsInventory) -> color_eyre::eyre::Error {
        match validate_inventory(candidate) {
            Ok(()) => eyre!("invalid inventory unexpectedly passed"),
            Err(error) => error,
        }
    }

    #[test]
    fn derived_inventory_is_valid_deterministic_and_binds_the_topology() -> Result<()> {
        let fixture = authority();
        let derived = derive_inventory(&fixture)?;
        validate_topology_binding(&derived, &fixture)?;
        validate_inventory(&derived)?;
        let left = canonical_projection(&derived)?;
        let right = canonical_projection(&derive_inventory(&fixture)?)?;
        assert_eq!(left, right, "identical inputs must project identically");
        Ok(())
    }

    #[test]
    fn authority_is_the_canonical_closed_world() -> Result<()> {
        let fixture = authority();
        // Primary channels are required — including Open VSX, which the
        // topology lists as primary; the docker secondary channel is
        // required; homebrew is explicitly deferred.
        for required in [
            "crates_io_crate_publication",
            "vsix_marketplace_publication",
            "github_release_publication",
            "open_vsx_publication",
            "container_registry_publication",
            "checksums_manifest_subject",
            "sbom_artifact_subject",
        ] {
            let spec =
                fixture.surfaces.get(required).ok_or_else(|| eyre!("authority lost {required}"))?;
            assert_eq!(spec.topology_state, TopologyState::Required, "{required} must be required");
        }
        let homebrew = fixture
            .surfaces
            .get("homebrew_publication")
            .ok_or_else(|| eyre!("authority lost the deferred homebrew surface"))?;
        assert_eq!(homebrew.topology_state, TopologyState::Deferred);
        assert!(
            homebrew.deferral_evidence.as_deref().unwrap_or_default().contains("deferred"),
            "homebrew deferral must name its topology rule"
        );
        // The crates.io denominator binds every published crate, not one.
        let crates_io = fixture
            .surfaces
            .get("crates_io_crate_publication")
            .ok_or_else(|| eyre!("authority lost the crates.io surface"))?;
        let denominator = crates_io.subject_denominator();
        assert!(denominator.contains("crates.io:perllsp"), "{denominator}");
        assert!(denominator.contains("crates.io:perl-dap"), "{denominator}");
        // The docker denominator enumerates the four independently mutable
        // registry/image subjects.
        let docker = fixture
            .surfaces
            .get("container_registry_publication")
            .ok_or_else(|| eyre!("authority lost the container surface"))?;
        let docker_denominator = docker.subject_denominator();
        assert_eq!(docker_denominator.split("; ").count(), 4, "{docker_denominator}");
        for subject in [
            "ghcr.io release runtime",
            "ghcr.io perl runtime",
            "docker.io release runtime",
            "docker.io perl runtime",
        ] {
            assert!(docker_denominator.contains(subject), "{docker_denominator}");
        }
        Ok(())
    }

    #[test]
    fn reordered_inventory_projects_identically() -> Result<()> {
        let mut reordered = inventory();
        reordered.surfaces.reverse();
        let forward = canonical_projection(&inventory())?;
        let backward = canonical_projection(&reordered)?;
        assert_eq!(forward, backward, "canonical projection must be order-independent");
        Ok(())
    }

    #[test]
    fn rejects_stale_topology_digest_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        candidate.topology_digest = format!("{}{}", TOPOLOGY_DIGEST_PREFIX, "0".repeat(64));
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("stale digest unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("does not match"), "{error}");
        assert!(error.to_string().contains("stale or fabricated"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_malformed_topology_digest() -> Result<()> {
        for malformed in [
            "<release topology digest>",
            "sha256:ABCDEF",
            "md5:0000000000000000000000000000000000000000000000000000000000000000",
            "sha256:000000000000000000000000000000000000000000000000000000000000000g",
        ] {
            let mut candidate = inventory();
            candidate.topology_digest = malformed.to_string();
            let error = rejection_error(&candidate);
            assert!(error.to_string().contains("malformed"), "{malformed}: {error}");
        }
        Ok(())
    }

    #[test]
    fn rejects_invented_surface_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        candidate.surfaces.push(PublishSurface {
            surface_id: "mastodon_announcement_publication".to_string(),
            surface_class: SurfaceClass::Channel,
            subject_identity: "release announcement on Mastodon".to_string(),
            observer: "Mastodon timeline before/after diff".to_string(),
            authority_rule: "no social publish credential is reachable".to_string(),
            mutation_authority: MutationAuthority::PublicMutationUnreachable,
            topology_state: TopologyState::Required,
            applicability: Applicability::Applicable,
            applicability_evidence: None,
            expected_public_state: PublicState::Unchanged,
            owner: "issue-9414".to_string(),
            claim_boundary: "schema/inventory only".to_string(),
        });
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("invented surface unexpectedly bound"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("absent from the canonical release topology"),
            "{error}"
        );
        Ok(())
    }

    #[test]
    fn rejects_surface_class_swap_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        let row = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "checksums_manifest_subject")
            .ok_or_else(|| eyre!("derived inventory lost the checksums surface"))?;
        row.surface_class = SurfaceClass::Channel;
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("class swap unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("class swaps"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_topology_state_swap_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        let row = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "homebrew_publication")
            .ok_or_else(|| eyre!("derived inventory lost the homebrew surface"))?;
        row.topology_state = TopologyState::Required;
        row.applicability = Applicability::Applicable;
        row.expected_public_state = PublicState::Unchanged;
        row.applicability_evidence = None;
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("topology-state swap unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("topology-state swaps"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_subject_denominator_rewrite_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        let row = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "crates_io_crate_publication")
            .ok_or_else(|| eyre!("derived inventory lost the crates.io surface"))?;
        row.subject_identity = "perllsp crate on crates.io".to_string();
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("subject rewrite unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("subject rewrites"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_missing_surface_through_the_binding_check() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        candidate.surfaces.retain(|surface| surface.surface_id != "open_vsx_publication");
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("omitted surface unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("absent from the inventory"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_required_surface_silently_reclassified_as_deferred() -> Result<()> {
        let fixture = authority();
        let mut candidate = inventory();
        let required = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "crates_io_crate_publication")
            .ok_or_else(|| eyre!("derived inventory lost a required surface"))?;
        // Reclassification is a topology-state swap against the closed-world
        // authority: required -> deferred silently removes a required surface
        // from the applicability denominator.
        required.topology_state = TopologyState::Deferred;
        required.applicability = Applicability::NotApplicable;
        required.expected_public_state = PublicState::NotApplicable;
        required.applicability_evidence = Some("fabricated deferral".to_string());
        let error = match validate_topology_binding(&candidate, &fixture) {
            Ok(()) => eyre!("reclassified surface unexpectedly bound"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("topology-state swaps"), "{error}");
        assert!(error.to_string().contains("Deferred"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_required_surface_marked_not_applicable() -> Result<()> {
        let mut candidate = inventory();
        let required = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "crates_io_crate_publication")
            .ok_or_else(|| eyre!("derived inventory lost a required surface"))?;
        required.applicability = Applicability::NotApplicable;
        required.expected_public_state = PublicState::NotApplicable;
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("cannot shed applicability"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_expected_public_state_changed_symmetrically() -> Result<()> {
        let mut candidate = inventory();
        candidate.surfaces[0].expected_public_state = PublicState::Changed;
        let error = rejection_error(&candidate);
        assert!(
            error.to_string().contains("contradicts the no_publish_side_effects receipt"),
            "{error}"
        );
        assert!(error.to_string().contains("public_mutation_authorized"), "{error}");
        Ok(())
    }

    #[test]
    fn rejects_deferred_channel_as_unchanged_without_positive_applicability() -> Result<()> {
        let mut candidate = inventory();
        let deferred = candidate
            .surfaces
            .iter_mut()
            .find(|surface| surface.surface_id == "homebrew_publication")
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
            .find(|surface| surface.surface_id == "homebrew_publication")
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
    fn rejects_semantic_tamper_through_the_production_validation_path() -> Result<()> {
        let expected = inventory();
        let projection = canonical_projection(&expected)?;
        // A generated projection that drifts from the schema-derived inventory
        // is detected through the production validation path: the tampered
        // document parses, then fails validation — not just a reparse compare.
        let tampered = projection.replace(
            r#""mutation_authority":"public_mutation_disabled""#,
            r#""mutation_authority":"public_mutation_authorized""#,
        );
        assert_ne!(projection, tampered, "tamper must change the projection");
        let reparsed: NoPublishSideEffectsInventory = serde_json::from_str(&tampered)
            .wrap_err("tampered projection must still parse to be comparable")?;
        assert_ne!(reparsed, expected, "divergence from the derived inventory must be detected");
        let error = rejection_error(&reparsed);
        assert!(error.to_string().contains("contradicts"), "{error}");
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
    fn rejects_unknown_schema_version() -> Result<()> {
        let mut candidate = inventory();
        candidate.schema_version = "no_publish_side_effects.v2".to_string();
        let error = rejection_error(&candidate);
        assert!(error.to_string().contains("unknown schema_version"), "{error}");
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
