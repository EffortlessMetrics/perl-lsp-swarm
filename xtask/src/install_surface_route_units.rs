//! Bridge the install-surface registry's product-unit vocabulary to the canonical
//! release-route product units used by the distribution train.
//!
//! The registry (`policy/install-surface-registry.toml`, schema
//! `install_surface_registry.v1`) records what *shape* of product a surface names.
//! The release/install route graph asks a different question: which *route product
//! unit* does that surface own. The two vocabularies overlap without being identical,
//! so a consumer that reads only `installed_product_unit` cannot tell whether
//! `server_dap_pair` means a current public archive pair, a historical pair, a
//! package-manager-owned pair, or only a recipe that could produce one.
//!
//! This module owns that translation once. It is deliberately evidence-gated: a
//! mapping may only name a current route unit when the row carries the registry
//! evidence that rule requires. Missing, unresolved, weak, or contradictory evidence
//! stays [`RouteProductUnit::NotProven`], which is not a route-green state.
//!
//! What this module does **not** do: change a registry row, classify a disposition,
//! generate a route catalog, select a preferred route, or assert that a product is
//! published, installed, current, or supported. Product and executable identity stay
//! with `policy/product-identity.toml`; topology roles stay with the release-topology
//! authorities. Rules reference those authorities rather than restating them.

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Schema token for the mapping report emitted by [`explain_report`].
pub const ROUTE_MAPPING_SCHEMA: &str = "install_surface_route_units.v1";

/// Registry schema this bridge is able to read.
pub const SUPPORTED_REGISTRY_SCHEMA: &str = "install_surface_registry.v1";

/// The committed registry, relative to the repository root.
pub const DEFAULT_REGISTRY_RELATIVE_PATH: &str = "policy/install-surface-registry.toml";

/// Topology authority that owns whether a surface stands in a checked relationship to
/// the release graph.
const TOPOLOGY_AUTHORITY: &str = "#6067";

/// Product, binary, crate, extension, and artifact identity authority.
///
/// Taken from the live owner table in
/// `docs/project/status/release_trust_invariants.md` — the generated projection of
/// `policy/release-trust-invariants.v1.json` — where #6744 is `current`. #10831's
/// prose points at #6855 instead, but #6855 is a closed *child* of #6744 (its own
/// body reads "Parents: #6601, #6744") that owns canonical identity guidance rather
/// than the identity contract, so the registry is the better authority for which
/// issue owns product identity.
const PRODUCT_AUTHORITY: &str = "#6744";

/// The identity-guidance child of [`PRODUCT_AUTHORITY`], accepted alongside it.
///
/// Two registry rows cite it today. It owns the canonical product/package/executable
/// projection, so a row citing it is citing product-identity evidence — unlike the
/// topology, workflow and currentness authorities this gate must reject.
const PRODUCT_IDENTITY_GUIDANCE_AUTHORITY: &str = "#6855";

/// The artifact shape a registry row names.
///
/// These are the registry's own `installed_product_unit` values. The server-only
/// variant is spelled `server_only` here because the registry's bare `server` reads
/// ambiguously next to `server_dap_pair` — it is the whole delivered unit, not the
/// server half of a pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductShape {
    ServerOnly,
    ServerDapPair,
    ManagedServerDapPair,
    EditorPackage,
    PackageSource,
    DocumentationOnly,
    NotApplicable,
    Unknown,
}

impl ProductShape {
    /// Translate a registry `installed_product_unit` value.
    ///
    /// Returns `None` for a value this bridge has never reviewed, so an unreviewed
    /// registry value becomes an explicit non-green mapping rather than a default.
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "server" => Self::ServerOnly,
            "server_dap_pair" => Self::ServerDapPair,
            "managed_server_dap_pair" => Self::ManagedServerDapPair,
            "editor_package" => Self::EditorPackage,
            "package_source" => Self::PackageSource,
            "documentation_only" => Self::DocumentationOnly,
            "not_applicable" => Self::NotApplicable,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ServerOnly => "server_only",
            Self::ServerDapPair => "server_dap_pair",
            Self::ManagedServerDapPair => "managed_server_dap_pair",
            Self::EditorPackage => "editor_package",
            Self::PackageSource => "package_source",
            Self::DocumentationOnly => "documentation_only",
            Self::NotApplicable => "not_applicable",
            Self::Unknown => "unknown",
        }
    }

    /// Shapes that deliver both members, and therefore cannot be described by any
    /// single-member route unit without discarding the DAP adapter.
    const fn is_pair(self) -> bool {
        matches!(self, Self::ServerDapPair | Self::ManagedServerDapPair)
    }
}

/// The unit a route delivers, as the distribution train and public route contract
/// describe it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteProductUnit {
    /// A current first-party route that must deliver the server and DAP pair together.
    ArchivePairRequired,
    /// A pair delivered through a managed editor channel.
    ManagedEditorPair,
    /// A current advanced route that deliberately delivers the server alone.
    ///
    /// The standalone candidate-selection contract spells the same concept
    /// `advanced_source_server_only` under its own schema; #10831's required model
    /// names it `advanced_server_only`, which is the spelling used here. A consumer
    /// bridging the two contracts should treat them as the same unit.
    AdvancedServerOnly,
    /// A server-only unit retained as history; never a current route.
    ///
    /// Like the candidate-selection contract's unit of the same name, this describes
    /// exactly one server member. A pair shape therefore cannot be mapped onto it.
    HistoricalServerOnly,
    /// A unit whose currentness is owned by an external package manager.
    PackageManagerOwned,
    /// Reachable only from a working tree; carries no public route claim.
    LocalDevelopmentNonAuthoritative,
    /// The surface names no product unit at all.
    NotApplicable,
    /// Evidence is missing, unresolved, weak, or contradictory.
    NotProven,
}

impl RouteProductUnit {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArchivePairRequired => "archive_pair_required",
            Self::ManagedEditorPair => "managed_editor_pair",
            Self::AdvancedServerOnly => "advanced_server_only",
            Self::HistoricalServerOnly => "historical_server_only",
            Self::PackageManagerOwned => "package_manager_owned",
            Self::LocalDevelopmentNonAuthoritative => "local_development_non_authoritative",
            Self::NotApplicable => "not_applicable",
            Self::NotProven => "not_proven",
        }
    }

    /// Whether this unit *names* a current route.
    ///
    /// Historical and not-proven units are explicitly not green, which is what keeps
    /// an unresolved row from reading as a route.
    ///
    /// This is only half the question: the same unit can be reached at a ceiling that
    /// forbids a current claim, as a package recipe does. Ask
    /// [`RouteMapping::claims_current_route`] for a mapping's actual standing.
    #[must_use]
    pub const fn is_current_route_claim(self) -> bool {
        matches!(
            self,
            Self::ArchivePairRequired
                | Self::ManagedEditorPair
                | Self::AdvancedServerOnly
                | Self::PackageManagerOwned
        )
    }
}

/// The strongest claim a mapping is allowed to support.
///
/// The ceiling is independent of the unit: naming `archive_pair_required` does not by
/// itself prove the archive exists, is published, or is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimCeiling {
    /// Nothing may be claimed; evidence is absent or unresolved.
    NotProven,
    /// The surface makes no product claim by construction.
    NoProductClaim,
    /// A recipe that could produce a unit; not an installed product.
    SourceRecipeOnly,
    /// Retained history only.
    HistoricalOnly,
    /// Reachable from a working tree only.
    LocalDevelopmentOnly,
    /// The unit's currentness belongs to an external channel owner.
    ExternallyOwnedChannel,
    /// The row may name a current first-party route unit.
    CurrentPublicRoute,
}

impl ClaimCeiling {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotProven => "not_proven",
            Self::NoProductClaim => "no_product_claim",
            Self::SourceRecipeOnly => "source_recipe_only",
            Self::HistoricalOnly => "historical_only",
            Self::LocalDevelopmentOnly => "local_development_only",
            Self::ExternallyOwnedChannel => "externally_owned_channel",
            Self::CurrentPublicRoute => "current_public_route",
        }
    }
}

/// Registry `disposition` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryDisposition {
    CanonicalGenerated,
    ActiveOwnedChannelSource,
    CandidateOnly,
    DeferredScaffold,
    HistoricalFixture,
    Retired,
    NeedsDisposition,
}

impl RegistryDisposition {
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "canonical_generated" => Self::CanonicalGenerated,
            "active_owned_channel_source" => Self::ActiveOwnedChannelSource,
            "candidate_only" => Self::CandidateOnly,
            "deferred_scaffold" => Self::DeferredScaffold,
            "historical_fixture" => Self::HistoricalFixture,
            "retired" => Self::Retired,
            "needs_disposition" => Self::NeedsDisposition,
            _ => return None,
        })
    }
}

/// Registry `publication_stage` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicationStage {
    Source,
    Candidate,
    PublicArtifact,
    ExternalChannel,
    Historical,
    Deferred,
}

impl PublicationStage {
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "source" => Self::Source,
            "candidate" => Self::Candidate,
            "public_artifact" => Self::PublicArtifact,
            "external_channel" => Self::ExternalChannel,
            "historical" => Self::Historical,
            "deferred" => Self::Deferred,
            _ => return None,
        })
    }
}

/// Registry `active_user_reachability` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reachability {
    Active,
    Candidate,
    External,
    Historical,
    Deferred,
}

impl Reachability {
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "active" => Self::Active,
            "candidate" => Self::Candidate,
            "external" => Self::External,
            "historical" => Self::Historical,
            "deferred" => Self::Deferred,
            _ => return None,
        })
    }
}

/// Registry `topology_relationship` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyRelationship {
    Authoritative,
    Checked,
    Derived,
    Related,
    NotApplicable,
    NotProven,
}

impl TopologyRelationship {
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "authoritative" => Self::Authoritative,
            "checked" => Self::Checked,
            "derived" => Self::Derived,
            "related" => Self::Related,
            "not_applicable" => Self::NotApplicable,
            "not_proven" => Self::NotProven,
            _ => return None,
        })
    }
}

/// How a target channel distributes a unit.
///
/// The class, not the channel string, is what a route rule keys on, so adding a new
/// channel of a known kind does not require a new rule row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelClass {
    /// First-party archives and the installers that select them.
    FirstPartyArchive,
    /// An editor marketplace that manages the download itself.
    ManagedEditor,
    /// A third-party package manager that owns its own currentness.
    PackageManager,
    /// A reusable action that external callers invoke to install a published
    /// artifact.
    ///
    /// Deliberately not [`Self::RepositoryInternal`]: the action's implementation
    /// lives in this repository, but consumers call it from their own workflows and
    /// it resolves and installs public releases. It *consumes* a route rather than
    /// owning one, and the route vocabulary has no unit for a consumer, so no rule
    /// maps it yet.
    ReusableSetupAction,
    /// Repository-internal surfaces with no public distribution.
    RepositoryInternal,
}

impl ChannelClass {
    #[must_use]
    pub fn from_registry_value(value: &str) -> Option<Self> {
        Some(match value {
            "github_release" | "standalone_installer" => Self::FirstPartyArchive,
            "vscode_marketplace" => Self::ManagedEditor,
            "homebrew" | "linux_packages" | "package_managers" | "cargo_binstall"
            | "external_channels" => Self::PackageManager,
            "github_action" => Self::ReusableSetupAction,
            "repository_gate" | "documentation" => Self::RepositoryInternal,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstPartyArchive => "first_party_archive",
            Self::ManagedEditor => "managed_editor",
            Self::PackageManager => "package_manager",
            Self::ReusableSetupAction => "reusable_setup_action",
            Self::RepositoryInternal => "repository_internal",
        }
    }
}

/// Stable machine reasons. Human copy is a later projection of these codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// Every gate the selected rule requires was satisfied.
    RuleEvidenceSatisfied,
    /// The row's `installed_product_unit` is literally `unknown`.
    UnknownProductShape,
    /// A registry value this bridge has never reviewed.
    UnrecognizedProductShape,
    UnrecognizedTargetChannel,
    UnrecognizedPublicationStage,
    UnrecognizedReachability,
    UnrecognizedTopologyRelationship,
    UnrecognizedDisposition,
    /// The registry has not classified the row, so no route claim may be derived.
    RegistryDispositionUnresolved,
    /// The row's disposition is outside the selected rule's allowed set.
    DispositionOutsideRule,
    PublicationStageOutsideRule,
    ReachabilityOutsideRule,
    TopologyOutsideRule,
    /// The row states its topology relationship is itself unproven.
    TopologyNotProven,
    /// Historical evidence pins the row out of every current route.
    HistoricalEvidence,
    /// Historical evidence on a shape with no historical route unit.
    HistoricalNonServerShape,
    /// Historical evidence on a pair shape; `historical_server_only` names exactly
    /// one server member, so it cannot carry a pair.
    HistoricalPairHasNoRouteUnit,
    /// A reusable setup action consumes a route; the vocabulary has no unit for a
    /// route consumer.
    SetupActionRouteOwnershipUnreviewed,
    /// The surface names no product unit.
    NoProductUnitClaimed,
    /// A package recipe is not an installed product.
    PackageRecipeNotInstalledProduct,
    /// A package-source row on a channel that no package manager owns.
    PackageSourceWithoutPackageChannel,
    /// An editor package without the product relation evidence a managed pair needs.
    EditorPackageWithoutProductRelation,
    /// A bare pair shape cannot become a managed pair by channel alone.
    ManagedPairRequiresManagedShape,
    /// Shape and channel would support more than one current unit.
    AmbiguousRouteOwnership,
    /// No reviewed rule covers this shape and channel class.
    NoRuleForShapeAndChannel,
}

impl ReasonCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuleEvidenceSatisfied => "rule_evidence_satisfied",
            Self::UnknownProductShape => "unknown_product_shape",
            Self::UnrecognizedProductShape => "unrecognized_product_shape",
            Self::UnrecognizedTargetChannel => "unrecognized_target_channel",
            Self::UnrecognizedPublicationStage => "unrecognized_publication_stage",
            Self::UnrecognizedReachability => "unrecognized_reachability",
            Self::UnrecognizedTopologyRelationship => "unrecognized_topology_relationship",
            Self::UnrecognizedDisposition => "unrecognized_disposition",
            Self::RegistryDispositionUnresolved => "registry_disposition_unresolved",
            Self::DispositionOutsideRule => "disposition_outside_rule",
            Self::PublicationStageOutsideRule => "publication_stage_outside_rule",
            Self::ReachabilityOutsideRule => "reachability_outside_rule",
            Self::TopologyOutsideRule => "topology_outside_rule",
            Self::TopologyNotProven => "topology_not_proven",
            Self::HistoricalEvidence => "historical_evidence",
            Self::HistoricalNonServerShape => "historical_non_server_shape",
            Self::HistoricalPairHasNoRouteUnit => "historical_pair_has_no_route_unit",
            Self::SetupActionRouteOwnershipUnreviewed => "setup_action_route_ownership_unreviewed",
            Self::NoProductUnitClaimed => "no_product_unit_claimed",
            Self::PackageRecipeNotInstalledProduct => "package_recipe_not_installed_product",
            Self::PackageSourceWithoutPackageChannel => "package_source_without_package_channel",
            Self::EditorPackageWithoutProductRelation => "editor_package_without_product_relation",
            Self::ManagedPairRequiresManagedShape => "managed_pair_requires_managed_shape",
            Self::AmbiguousRouteOwnership => "ambiguous_route_ownership",
            Self::NoRuleForShapeAndChannel => "no_rule_for_shape_and_channel",
        }
    }
}

/// One registry row reduced to the fields the bridge reads.
///
/// Values stay as the registry spells them so an unreviewed value is visible to the
/// mapping rather than lost at load time.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SurfaceRouteSubject {
    pub surface_id: String,
    pub installed_product_unit: String,
    pub target_channel: String,
    pub publication_stage: String,
    pub active_user_reachability: String,
    pub topology_relationship: String,
    pub disposition: String,
    #[serde(default)]
    pub authority_refs: Vec<String>,
}

/// One surface's translated route position.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouteMapping {
    pub surface_id: String,
    pub product_shape: Option<ProductShape>,
    pub channel_class: Option<ChannelClass>,
    pub route_product_unit: RouteProductUnit,
    pub claim_ceiling: ClaimCeiling,
    /// Topology and product authorities that own the facts this rule leans on.
    pub owning_authorities: Vec<String>,
    pub reasons: Vec<ReasonCode>,
    /// What the mapping deliberately does not establish.
    pub limitation: String,
}

impl RouteMapping {
    /// Whether a current-route consumer may treat this mapping as settled.
    ///
    /// Both the unit and the ceiling must agree. A package recipe reaches
    /// [`RouteProductUnit::PackageManagerOwned`] — a unit that does name a current
    /// route — at [`ClaimCeiling::SourceRecipeOnly`], whose whole point is that the
    /// recipe is not an installed or published product. Counting the unit alone would
    /// contradict the ceiling in the same mapping.
    #[must_use]
    pub fn claims_current_route(&self) -> bool {
        self.route_product_unit.is_current_route_claim()
            && matches!(
                self.claim_ceiling,
                ClaimCeiling::CurrentPublicRoute | ClaimCeiling::ExternallyOwnedChannel
            )
    }
}

/// A reviewed rule: one (shape, channel class) pair and the evidence it demands.
#[derive(Debug, Clone, Copy)]
struct RouteRule {
    product_shape: ProductShape,
    channel_class: ChannelClass,
    route_product_unit: RouteProductUnit,
    claim_ceiling: ClaimCeiling,
    allowed_dispositions: &'static [RegistryDisposition],
    allowed_publication_stages: &'static [PublicationStage],
    allowed_reachability: &'static [Reachability],
    allowed_topology: &'static [TopologyRelationship],
    /// Managed-editor pairs additionally need a named product relation, so an editor
    /// package cannot become a pair on channel evidence alone.
    ///
    /// Satisfied only by an `authority_refs` entry in
    /// [`PRODUCT_RELATION_AUTHORITIES`]. A nonempty list is not evidence: rows
    /// routinely cite topology, release, workflow and currentness authorities, none
    /// of which says the package carries this product.
    requires_product_relation_authority: bool,
    limitation: &'static str,
}

/// Authorities that establish which product a surface carries.
///
/// A row must cite one of these for a managed-pair rule to treat the package as
/// carrying the product. Topology, release, workflow and currentness authorities are
/// deliberately absent: they say who owns the row, not what it carries.
const PRODUCT_RELATION_AUTHORITIES: &[&str] =
    &[PRODUCT_AUTHORITY, PRODUCT_IDENTITY_GUIDANCE_AUTHORITY];

/// Dispositions under which a row is a settled, currently-owned surface.
const SETTLED_ACTIVE: &[RegistryDisposition] =
    &[RegistryDisposition::CanonicalGenerated, RegistryDisposition::ActiveOwnedChannelSource];

/// Dispositions that may still describe a working-tree-only surface.
const SETTLED_OR_PROVISIONAL: &[RegistryDisposition] = &[
    RegistryDisposition::CanonicalGenerated,
    RegistryDisposition::ActiveOwnedChannelSource,
    RegistryDisposition::CandidateOnly,
    RegistryDisposition::DeferredScaffold,
];

/// A first-party route may be owned by the published artifact or by the reviewed
/// source that produces it; neither a candidate nor a deferred surface owns one.
const PUBLISHED_OR_SOURCE: &[PublicationStage] =
    &[PublicationStage::Source, PublicationStage::PublicArtifact];

const EXTERNAL_STAGES: &[PublicationStage] = &[
    PublicationStage::Source,
    PublicationStage::PublicArtifact,
    PublicationStage::ExternalChannel,
];

const INTERNAL_STAGES: &[PublicationStage] =
    &[PublicationStage::Source, PublicationStage::Candidate, PublicationStage::Deferred];

const ACTIVE_ONLY: &[Reachability] = &[Reachability::Active];
const ACTIVE_OR_EXTERNAL: &[Reachability] = &[Reachability::Active, Reachability::External];
const INTERNAL_REACHABILITY: &[Reachability] =
    &[Reachability::Active, Reachability::Candidate, Reachability::Deferred];

/// Topology evidence strong enough to place a surface in the release graph.
const PLACED_TOPOLOGY: &[TopologyRelationship] = &[
    TopologyRelationship::Authoritative,
    TopologyRelationship::Checked,
    TopologyRelationship::Derived,
];

/// A managed pair needs a directly checked relation; `derived` is not enough to bind
/// an editor package to the product it is claimed to carry.
const DIRECT_TOPOLOGY: &[TopologyRelationship] =
    &[TopologyRelationship::Authoritative, TopologyRelationship::Checked];

const INTERNAL_TOPOLOGY: &[TopologyRelationship] = &[
    TopologyRelationship::Authoritative,
    TopologyRelationship::Checked,
    TopologyRelationship::Derived,
    TopologyRelationship::Related,
    TopologyRelationship::NotApplicable,
];

/// The reviewed mapping table.
///
/// Absence is meaningful: a (shape, channel) pair with no row cannot be mapped by
/// analogy to a neighbouring row.
const ROUTE_RULES: &[RouteRule] = &[
    // A server-only unit stays server-only. It never becomes a pair because it
    // travels a first-party archive route.
    RouteRule {
        product_shape: ProductShape::ServerOnly,
        channel_class: ChannelClass::FirstPartyArchive,
        route_product_unit: RouteProductUnit::AdvancedServerOnly,
        claim_ceiling: ClaimCeiling::CurrentPublicRoute,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: PUBLISHED_OR_SOURCE,
        allowed_reachability: ACTIVE_ONLY,
        allowed_topology: PLACED_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "Names the advanced server-only route; does not prove the DAP \
                     adapter is absent by policy rather than by omission.",
    },
    RouteRule {
        product_shape: ProductShape::ServerOnly,
        channel_class: ChannelClass::PackageManager,
        route_product_unit: RouteProductUnit::PackageManagerOwned,
        claim_ceiling: ClaimCeiling::ExternallyOwnedChannel,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: EXTERNAL_STAGES,
        allowed_reachability: ACTIVE_OR_EXTERNAL,
        allowed_topology: PLACED_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "The package manager owns currentness; this does not prove the \
                     published package version matches the repository.",
    },
    RouteRule {
        product_shape: ProductShape::ServerOnly,
        channel_class: ChannelClass::RepositoryInternal,
        route_product_unit: RouteProductUnit::LocalDevelopmentNonAuthoritative,
        claim_ceiling: ClaimCeiling::LocalDevelopmentOnly,
        allowed_dispositions: SETTLED_OR_PROVISIONAL,
        allowed_publication_stages: INTERNAL_STAGES,
        allowed_reachability: INTERNAL_REACHABILITY,
        allowed_topology: INTERNAL_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "Working-tree reachability only; carries no public route claim.",
    },
    // A pair shape on a first-party archive route is the archive pair case.
    RouteRule {
        product_shape: ProductShape::ServerDapPair,
        channel_class: ChannelClass::FirstPartyArchive,
        route_product_unit: RouteProductUnit::ArchivePairRequired,
        claim_ceiling: ClaimCeiling::CurrentPublicRoute,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: PUBLISHED_OR_SOURCE,
        allowed_reachability: ACTIVE_ONLY,
        allowed_topology: PLACED_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "Requires both members on the route; does not prove either member \
                     was built, signed, or published for any given release.",
    },
    RouteRule {
        product_shape: ProductShape::ServerDapPair,
        channel_class: ChannelClass::PackageManager,
        route_product_unit: RouteProductUnit::PackageManagerOwned,
        claim_ceiling: ClaimCeiling::ExternallyOwnedChannel,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: EXTERNAL_STAGES,
        allowed_reachability: ACTIVE_OR_EXTERNAL,
        allowed_topology: PLACED_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "The package manager owns currentness and may ship either member \
                     on its own cadence.",
    },
    RouteRule {
        product_shape: ProductShape::ServerDapPair,
        channel_class: ChannelClass::RepositoryInternal,
        route_product_unit: RouteProductUnit::LocalDevelopmentNonAuthoritative,
        claim_ceiling: ClaimCeiling::LocalDevelopmentOnly,
        allowed_dispositions: SETTLED_OR_PROVISIONAL,
        allowed_publication_stages: INTERNAL_STAGES,
        allowed_reachability: INTERNAL_REACHABILITY,
        allowed_topology: INTERNAL_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "Working-tree reachability only; carries no public route claim.",
    },
    // Only the managed shape may name a managed editor pair.
    RouteRule {
        product_shape: ProductShape::ManagedServerDapPair,
        channel_class: ChannelClass::ManagedEditor,
        route_product_unit: RouteProductUnit::ManagedEditorPair,
        claim_ceiling: ClaimCeiling::CurrentPublicRoute,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: PUBLISHED_OR_SOURCE,
        allowed_reachability: ACTIVE_ONLY,
        allowed_topology: DIRECT_TOPOLOGY,
        requires_product_relation_authority: true,
        limitation: "Names the managed pair route; does not prove the managed \
                     downloader resolves a current or matching pair.",
    },
    // An editor package may carry a managed pair, but only with a named product
    // relation. Marketplace presence alone is not that evidence.
    RouteRule {
        product_shape: ProductShape::EditorPackage,
        channel_class: ChannelClass::ManagedEditor,
        route_product_unit: RouteProductUnit::ManagedEditorPair,
        claim_ceiling: ClaimCeiling::CurrentPublicRoute,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: PUBLISHED_OR_SOURCE,
        allowed_reachability: ACTIVE_ONLY,
        allowed_topology: DIRECT_TOPOLOGY,
        requires_product_relation_authority: true,
        limitation: "The package carries the pair through its managed downloader; this \
                     does not prove which server or adapter version it resolves.",
    },
    RouteRule {
        product_shape: ProductShape::EditorPackage,
        channel_class: ChannelClass::RepositoryInternal,
        route_product_unit: RouteProductUnit::LocalDevelopmentNonAuthoritative,
        claim_ceiling: ClaimCeiling::LocalDevelopmentOnly,
        allowed_dispositions: SETTLED_OR_PROVISIONAL,
        allowed_publication_stages: INTERNAL_STAGES,
        allowed_reachability: INTERNAL_REACHABILITY,
        allowed_topology: INTERNAL_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "Working-tree reachability only; carries no public route claim.",
    },
    // A recipe is owned by the package manager it feeds. It is never itself installed.
    RouteRule {
        product_shape: ProductShape::PackageSource,
        channel_class: ChannelClass::PackageManager,
        route_product_unit: RouteProductUnit::PackageManagerOwned,
        claim_ceiling: ClaimCeiling::SourceRecipeOnly,
        allowed_dispositions: SETTLED_ACTIVE,
        allowed_publication_stages: EXTERNAL_STAGES,
        allowed_reachability: ACTIVE_OR_EXTERNAL,
        allowed_topology: PLACED_TOPOLOGY,
        requires_product_relation_authority: false,
        limitation: "A recipe that a package manager may build; not an installed \
                     product and not proof that any package was published from it.",
    },
];

fn find_rule(
    product_shape: ProductShape,
    channel_class: ChannelClass,
) -> Option<&'static RouteRule> {
    ROUTE_RULES
        .iter()
        .find(|rule| rule.product_shape == product_shape && rule.channel_class == channel_class)
}

/// Translate one registry row into its route position.
///
/// The mapping is total: every subject returns a [`RouteMapping`], and every mapping
/// that cannot be justified returns [`RouteProductUnit::NotProven`] with the reasons
/// that blocked it.
#[must_use]
pub fn map_surface(subject: &SurfaceRouteSubject) -> RouteMapping {
    let product_shape = ProductShape::from_registry_value(&subject.installed_product_unit);
    let channel_class = ChannelClass::from_registry_value(&subject.target_channel);
    let stage = PublicationStage::from_registry_value(&subject.publication_stage);
    let reachability = Reachability::from_registry_value(&subject.active_user_reachability);
    let topology = TopologyRelationship::from_registry_value(&subject.topology_relationship);
    let disposition = RegistryDisposition::from_registry_value(&subject.disposition);

    // An unreviewed registry value is reported as itself, never absorbed into a
    // neighbouring meaning.
    let mut unrecognized = Vec::new();
    if product_shape.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedProductShape);
    }
    if channel_class.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedTargetChannel);
    }
    if stage.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedPublicationStage);
    }
    if reachability.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedReachability);
    }
    if topology.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedTopologyRelationship);
    }
    if disposition.is_none() {
        unrecognized.push(ReasonCode::UnrecognizedDisposition);
    }
    if !unrecognized.is_empty() {
        return not_proven(
            subject,
            product_shape,
            channel_class,
            unrecognized,
            "The registry uses a value this bridge has not reviewed; the row is \
             unmapped until the vocabulary is reconciled.",
        );
    }

    let (
        Some(product_shape),
        Some(channel_class),
        Some(stage),
        Some(reachability),
        Some(topology),
        Some(disposition),
    ) = (product_shape, channel_class, stage, reachability, topology, disposition)
    else {
        // Unreachable: every `None` was collected above. Staying total rather than
        // panicking keeps the bridge usable from a gate.
        return not_proven(
            subject,
            product_shape,
            channel_class,
            vec![ReasonCode::NoRuleForShapeAndChannel],
            "Registry vocabulary could not be resolved.",
        );
    };

    if product_shape == ProductShape::Unknown {
        return not_proven(
            subject,
            Some(product_shape),
            Some(channel_class),
            vec![ReasonCode::UnknownProductShape],
            "The registry states the product unit is unknown; no route unit follows \
             from an unknown shape.",
        );
    }

    if topology == TopologyRelationship::NotProven {
        return not_proven(
            subject,
            Some(product_shape),
            Some(channel_class),
            vec![ReasonCode::TopologyNotProven],
            "The row states its own topology relationship is unproven.",
        );
    }

    // Historical evidence is decisive and is evaluated before any current rule, so a
    // retired surface cannot be promoted onto a current route by shape or channel.
    let historical = stage == PublicationStage::Historical
        || reachability == Reachability::Historical
        || matches!(
            disposition,
            RegistryDisposition::HistoricalFixture | RegistryDisposition::Retired
        );
    if historical {
        return if product_shape == ProductShape::ServerOnly {
            RouteMapping {
                surface_id: subject.surface_id.clone(),
                product_shape: Some(product_shape),
                channel_class: Some(channel_class),
                route_product_unit: RouteProductUnit::HistoricalServerOnly,
                claim_ceiling: ClaimCeiling::HistoricalOnly,
                owning_authorities: authorities(),
                reasons: vec![ReasonCode::HistoricalEvidence],
                limitation: "Retained as history. Not a current route and not evidence \
                             that the historical unit was ever published."
                    .to_string(),
            }
        } else if product_shape.is_pair() {
            // `historical_server_only` names exactly one server member, so mapping a
            // pair onto it would silently discard the DAP adapter. There is no
            // historical pair unit, and inventing one is outside this claim.
            not_proven(
                subject,
                Some(product_shape),
                Some(channel_class),
                vec![ReasonCode::HistoricalEvidence, ReasonCode::HistoricalPairHasNoRouteUnit],
                "Historical evidence on a pair shape. The historical unit describes a \
                 single server member, so the pair cannot be mapped onto it without \
                 losing the DAP adapter.",
            )
        } else {
            not_proven(
                subject,
                Some(product_shape),
                Some(channel_class),
                vec![ReasonCode::HistoricalEvidence, ReasonCode::HistoricalNonServerShape],
                "Historical evidence with no historical route unit for this shape.",
            )
        };
    }

    if matches!(product_shape, ProductShape::DocumentationOnly | ProductShape::NotApplicable) {
        return RouteMapping {
            surface_id: subject.surface_id.clone(),
            product_shape: Some(product_shape),
            channel_class: Some(channel_class),
            route_product_unit: RouteProductUnit::NotApplicable,
            claim_ceiling: ClaimCeiling::NoProductClaim,
            owning_authorities: authorities(),
            reasons: vec![ReasonCode::NoProductUnitClaimed],
            limitation: "The surface names no product unit; it is outside the route \
                         product graph."
                .to_string(),
        };
    }

    let Some(rule) = find_rule(product_shape, channel_class) else {
        return not_proven(
            subject,
            Some(product_shape),
            Some(channel_class),
            vec![missing_rule_reason(product_shape, channel_class)],
            missing_rule_limitation(product_shape, channel_class),
        );
    };

    let mut blocked = Vec::new();
    if !rule.allowed_dispositions.contains(&disposition) {
        blocked.push(if disposition == RegistryDisposition::NeedsDisposition {
            ReasonCode::RegistryDispositionUnresolved
        } else {
            ReasonCode::DispositionOutsideRule
        });
    }
    if !rule.allowed_publication_stages.contains(&stage) {
        blocked.push(ReasonCode::PublicationStageOutsideRule);
    }
    if !rule.allowed_reachability.contains(&reachability) {
        blocked.push(ReasonCode::ReachabilityOutsideRule);
    }
    if !rule.allowed_topology.contains(&topology) {
        blocked.push(ReasonCode::TopologyOutsideRule);
    }
    if rule.requires_product_relation_authority
        && !subject
            .authority_refs
            .iter()
            .any(|reference| PRODUCT_RELATION_AUTHORITIES.contains(&reference.trim()))
    {
        blocked.push(ReasonCode::EditorPackageWithoutProductRelation);
    }

    if !blocked.is_empty() {
        return not_proven(
            subject,
            Some(product_shape),
            Some(channel_class),
            blocked,
            "The row does not carry the evidence this rule requires for a current \
             route claim.",
        );
    }

    RouteMapping {
        surface_id: subject.surface_id.clone(),
        product_shape: Some(product_shape),
        channel_class: Some(channel_class),
        route_product_unit: rule.route_product_unit,
        claim_ceiling: rule.claim_ceiling,
        owning_authorities: authorities(),
        reasons: vec![ReasonCode::RuleEvidenceSatisfied],
        limitation: rule.limitation.to_string(),
    }
}

/// Name the specific way a shape and channel disagree, rather than reporting a bare
/// table miss, so a reviewer can tell a deliberate gap from an unreviewed pair.
fn missing_rule_reason(product_shape: ProductShape, channel_class: ChannelClass) -> ReasonCode {
    match (product_shape, channel_class) {
        (_, ChannelClass::ReusableSetupAction) => ReasonCode::SetupActionRouteOwnershipUnreviewed,
        (ProductShape::ServerDapPair, ChannelClass::ManagedEditor) => {
            ReasonCode::ManagedPairRequiresManagedShape
        }
        (ProductShape::PackageSource, _) => ReasonCode::PackageSourceWithoutPackageChannel,
        (ProductShape::ServerOnly | ProductShape::ManagedServerDapPair, _) => {
            ReasonCode::AmbiguousRouteOwnership
        }
        _ => ReasonCode::NoRuleForShapeAndChannel,
    }
}

fn missing_rule_limitation(
    product_shape: ProductShape,
    channel_class: ChannelClass,
) -> &'static str {
    match (product_shape, channel_class) {
        (_, ChannelClass::ReusableSetupAction) => {
            "A reusable setup action that external callers invoke to install a \
             published artifact. It consumes a route rather than owning one, and the \
             route vocabulary has no unit for a consumer, so its ownership is \
             reported unresolved rather than guessed."
        }
        (ProductShape::PackageSource, _) => {
            "A package recipe on a channel no package manager owns. A recipe is never \
             itself an installed product."
        }
        (ProductShape::EditorPackage, _) => {
            "An editor package on a channel that does not manage editor extensions; \
             no reviewed rule places it in the route graph."
        }
        _ => {
            "Shape and channel would admit more than one current route unit; the \
             ambiguity is reported rather than resolved by preference."
        }
    }
}

fn authorities() -> Vec<String> {
    vec![TOPOLOGY_AUTHORITY.to_string(), PRODUCT_AUTHORITY.to_string()]
}

fn not_proven(
    subject: &SurfaceRouteSubject,
    product_shape: Option<ProductShape>,
    channel_class: Option<ChannelClass>,
    reasons: Vec<ReasonCode>,
    limitation: &str,
) -> RouteMapping {
    RouteMapping {
        surface_id: subject.surface_id.clone(),
        product_shape,
        channel_class,
        route_product_unit: RouteProductUnit::NotProven,
        claim_ceiling: ClaimCeiling::NotProven,
        owning_authorities: authorities(),
        reasons,
        limitation: limitation.to_string(),
    }
}

/// The deterministic mapping report for a whole registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteMappingReport {
    pub schema_version: String,
    pub registry_schema_version: String,
    pub mapped_surface_count: usize,
    pub current_route_claim_count: usize,
    pub not_proven_count: usize,
    pub mappings: Vec<RouteMapping>,
}

#[derive(Debug, Deserialize)]
struct RegistryDocument {
    schema_version: String,
    #[serde(default)]
    surfaces: Vec<SurfaceRouteSubject>,
}

/// Resolve the committed registry from the repository root rather than the caller's
/// working directory, so a consumer and a test agree on the same subject.
pub fn default_registry_path() -> Result<PathBuf> {
    Ok(crate::utils::project_root()?.join(DEFAULT_REGISTRY_RELATIVE_PATH))
}

/// Read the install-surface registry and reduce each row to a route subject.
///
/// Only the fields this bridge reads are parsed; the inventory task remains the owner
/// of full-row validation.
pub fn load_subjects(registry_path: &Path) -> Result<Vec<SurfaceRouteSubject>> {
    let source = fs::read_to_string(registry_path)
        .wrap_err_with(|| format!("read install surface registry {}", registry_path.display()))?;
    parse_subjects(&source)
        .wrap_err_with(|| format!("parse install surface registry {}", registry_path.display()))
}

fn parse_subjects(source: &str) -> Result<Vec<SurfaceRouteSubject>> {
    let document: RegistryDocument = toml::from_str(source)?;
    if document.schema_version != SUPPORTED_REGISTRY_SCHEMA {
        bail!(
            "unsupported registry schema {:?}; expected {:?}",
            document.schema_version,
            SUPPORTED_REGISTRY_SCHEMA
        );
    }
    if document.surfaces.is_empty() {
        bail!("install surface registry must contain at least one row");
    }
    let mut subjects = document.surfaces;
    subjects.sort();
    Ok(subjects)
}

/// Build the deterministic report for a set of subjects.
#[must_use]
pub fn explain_report(subjects: &[SurfaceRouteSubject]) -> RouteMappingReport {
    let mut mappings: Vec<RouteMapping> = subjects.iter().map(map_surface).collect();
    mappings.sort_by(|left, right| left.surface_id.cmp(&right.surface_id));
    let current_route_claim_count =
        mappings.iter().filter(|mapping| mapping.claims_current_route()).count();
    let not_proven_count = mappings
        .iter()
        .filter(|mapping| mapping.route_product_unit == RouteProductUnit::NotProven)
        .count();
    RouteMappingReport {
        schema_version: ROUTE_MAPPING_SCHEMA.to_string(),
        registry_schema_version: SUPPORTED_REGISTRY_SCHEMA.to_string(),
        mapped_surface_count: mappings.len(),
        current_route_claim_count,
        not_proven_count,
        mappings,
    }
}

/// Render the report as stable review text, one line per surface.
#[must_use]
pub fn render_explain(report: &RouteMappingReport) -> String {
    let mut rendered = String::new();
    for mapping in &report.mappings {
        let reasons =
            mapping.reasons.iter().map(|reason| reason.as_str()).collect::<Vec<_>>().join(",");
        // The channel class selects the rule, so a reader cannot diagnose a mapping
        // without seeing which class the row's channel resolved to.
        let _ = writeln!(
            rendered,
            "{}\t{}\t{}\t{}\t{}\t{}",
            mapping.surface_id,
            mapping.product_shape.map_or("unrecognized", ProductShape::as_str),
            mapping.channel_class.map_or("unrecognized", ChannelClass::as_str),
            mapping.route_product_unit.as_str(),
            mapping.claim_ceiling.as_str(),
            reasons
        );
    }
    let _ = writeln!(
        rendered,
        "totals\tmapped={}\tcurrent_route_claims={}\tnot_proven={}",
        report.mapped_surface_count, report.current_route_claim_count, report.not_proven_count
    );
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::eyre::eyre;

    /// A row carrying every piece of evidence a current first-party route needs.
    /// Individual tests weaken exactly one field, so a failure names one cause.
    ///
    /// `authority_refs` deliberately holds only the topology authority. That is a
    /// real registry shape (`workflow.publish-extension` cites `#6067` with no
    /// product authority) and it must not satisfy a product-relation gate, so the
    /// managed-pair tests have to add that evidence explicitly.
    fn settled(product_unit: &str, channel: &str) -> SurfaceRouteSubject {
        SurfaceRouteSubject {
            surface_id: "surface.under.test".to_string(),
            installed_product_unit: product_unit.to_string(),
            target_channel: channel.to_string(),
            publication_stage: "public_artifact".to_string(),
            active_user_reachability: "active".to_string(),
            topology_relationship: "checked".to_string(),
            disposition: "active_owned_channel_source".to_string(),
            authority_refs: vec![TOPOLOGY_AUTHORITY.to_string()],
        }
    }

    /// The same row, additionally citing the product-identity authority.
    fn settled_with_product_relation(product_unit: &str, channel: &str) -> SurfaceRouteSubject {
        let mut subject = settled(product_unit, channel);
        subject.authority_refs.push(PRODUCT_AUTHORITY.to_string());
        subject
    }

    fn unit_of(subject: &SurfaceRouteSubject) -> RouteProductUnit {
        map_surface(subject).route_product_unit
    }

    // Falsifier 1: `Server` automatically mapped to a full archive pair.
    #[test]
    fn server_only_never_becomes_an_archive_pair() {
        let mapping = map_surface(&settled("server", "github_release"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::AdvancedServerOnly);
        assert_ne!(mapping.route_product_unit, RouteProductUnit::ArchivePairRequired);
    }

    // Falsifier 2: `ServerDapPair` mapped to a current public pair without topology
    // or currentness evidence.
    #[test]
    fn pair_needs_currentness_and_topology_evidence() {
        let settled_pair = settled("server_dap_pair", "github_release");
        assert_eq!(unit_of(&settled_pair), RouteProductUnit::ArchivePairRequired);

        let mut candidate_reach = settled_pair.clone();
        candidate_reach.active_user_reachability = "candidate".to_string();
        let mapping = map_surface(&candidate_reach);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::ReachabilityOutsideRule));

        let mut weak_topology = settled_pair.clone();
        weak_topology.topology_relationship = "related".to_string();
        let mapping = map_surface(&weak_topology);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::TopologyOutsideRule));

        let mut unproven_topology = settled_pair;
        unproven_topology.topology_relationship = "not_proven".to_string();
        let mapping = map_surface(&unproven_topology);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::TopologyNotProven));
    }

    // Falsifier 3: `PackageSource` treated as an installed product.
    #[test]
    fn package_source_is_never_an_installed_product() {
        let mapping = map_surface(&settled("package_source", "homebrew"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::PackageManagerOwned);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::SourceRecipeOnly);

        // On a first-party archive channel there is no package manager to own it, and
        // the recipe still does not become the archive.
        let mapping = map_surface(&settled("package_source", "github_release"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::PackageSourceWithoutPackageChannel));
    }

    /// A recipe reaches `package_manager_owned`, a unit that does name a current
    /// route, but at the `source_recipe_only` ceiling. Counting the unit alone would
    /// report the recipe as a current route claim while the same mapping says it is
    /// not an installed product.
    #[test]
    fn a_source_recipe_is_not_counted_as_a_current_route_claim() {
        let recipe = settled("package_source", "homebrew");
        let mapping = map_surface(&recipe);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::PackageManagerOwned);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::SourceRecipeOnly);
        assert!(
            mapping.route_product_unit.is_current_route_claim(),
            "the unit itself names a current route; the ceiling is what withholds it"
        );
        assert!(!mapping.claims_current_route());

        // And the report's counter must agree with the mapping.
        let report = explain_report(&[recipe]);
        assert_eq!(report.current_route_claim_count, 0);

        // An installed unit on the same channel is still counted.
        let installed = settled("server", "homebrew");
        let mapping = map_surface(&installed);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::ExternallyOwnedChannel);
        assert!(mapping.claims_current_route());
        assert_eq!(explain_report(&[installed]).current_route_claim_count, 1);
    }

    // Falsifier 4: `EditorPackage` treated as a managed pair without product relation
    // evidence.
    #[test]
    fn editor_package_needs_product_relation_evidence() {
        let settled_package = settled_with_product_relation("editor_package", "vscode_marketplace");
        assert_eq!(unit_of(&settled_package), RouteProductUnit::ManagedEditorPair);

        // Both product-identity authorities satisfy the gate: #6744 owns the identity
        // contract and #6855 owns its canonical guidance projection.
        for authority in [PRODUCT_AUTHORITY, PRODUCT_IDENTITY_GUIDANCE_AUTHORITY] {
            let mut subject = settled("editor_package", "vscode_marketplace");
            subject.authority_refs.push(authority.to_string());
            assert_eq!(
                unit_of(&subject),
                RouteProductUnit::ManagedEditorPair,
                "{authority} should satisfy the product-relation gate"
            );
        }

        let mut without_authority = settled_package.clone();
        without_authority.authority_refs.clear();
        let mapping = map_surface(&without_authority);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::EditorPackageWithoutProductRelation));

        // Nonemptiness is not the evidence. This is the real shape of
        // `workflow.publish-extension`: authorities that own topology and workflow,
        // none of which says the package carries this product.
        let mut unrelated_authorities = settled_package.clone();
        unrelated_authorities.authority_refs =
            vec!["#9093".to_string(), TOPOLOGY_AUTHORITY.to_string()];
        let mapping = map_surface(&unrelated_authorities);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::EditorPackageWithoutProductRelation));

        // A merely derived relation does not bind the package to the product it is
        // claimed to carry.
        let mut derived = settled_package;
        derived.topology_relationship = "derived".to_string();
        let mapping = map_surface(&derived);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::TopologyOutsideRule));
    }

    // Falsifier 5: a historical server-only row promoted to a current advanced route.
    #[test]
    fn historical_rows_cannot_reach_a_current_route() {
        for (field, value) in [
            ("publication_stage", "historical"),
            ("active_user_reachability", "historical"),
            ("disposition", "retired"),
            ("disposition", "historical_fixture"),
        ] {
            let mut subject = settled("server", "github_release");
            match field {
                "publication_stage" => subject.publication_stage = value.to_string(),
                "active_user_reachability" => {
                    subject.active_user_reachability = value.to_string();
                }
                _ => subject.disposition = value.to_string(),
            }
            let mapping = map_surface(&subject);
            assert_eq!(
                mapping.route_product_unit,
                RouteProductUnit::HistoricalServerOnly,
                "{field}={value} should stay historical"
            );
            assert_eq!(mapping.claim_ceiling, ClaimCeiling::HistoricalOnly);
            assert!(!mapping.claims_current_route());
        }
    }

    /// `historical_server_only` describes exactly one server member, so a historical
    /// pair must not be mapped onto it — that would silently drop the DAP adapter and
    /// collapse the pair role into the server-only role.
    #[test]
    fn historical_pair_is_not_collapsed_into_a_server_only_unit() {
        for shape in ["server_dap_pair", "managed_server_dap_pair"] {
            let mut subject = settled(shape, "github_release");
            subject.publication_stage = "historical".to_string();
            let mapping = map_surface(&subject);
            assert_eq!(
                mapping.route_product_unit,
                RouteProductUnit::NotProven,
                "{shape} must not claim a single-member historical unit"
            );
            assert!(mapping.reasons.contains(&ReasonCode::HistoricalEvidence));
            assert!(mapping.reasons.contains(&ReasonCode::HistoricalPairHasNoRouteUnit));
        }
    }

    /// A reusable setup action installs public releases for external callers, so it
    /// must not be classified as a repository-internal, working-tree-only surface.
    #[test]
    fn reusable_setup_action_is_not_local_development() {
        let mut subject = settled("server", "github_action");
        subject.publication_stage = "source".to_string();
        let mapping = map_surface(&subject);
        assert_eq!(mapping.channel_class, Some(ChannelClass::ReusableSetupAction));
        assert_ne!(mapping.route_product_unit, RouteProductUnit::LocalDevelopmentNonAuthoritative);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::SetupActionRouteOwnershipUnreviewed));
    }

    // Falsifier 6: an unknown mapping serialized as an empty or default success.
    #[test]
    fn unknown_and_unreviewed_values_stay_non_green() -> Result<()> {
        let mapping = map_surface(&settled("unknown", "github_release"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::UnknownProductShape));
        assert!(!mapping.claims_current_route());

        let mapping = map_surface(&settled("server_quad_bundle", "github_release"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::UnrecognizedProductShape));

        let mapping = map_surface(&settled("server", "some_new_store"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::UnrecognizedTargetChannel));

        // Serialization keeps the non-green state visible rather than omitting it.
        let json = serde_json::to_string(&mapping)?;
        assert!(json.contains("\"not_proven\""), "{json}");
        Ok(())
    }

    // Falsifier 7: one surface silently mapped to two current route units.
    #[test]
    fn ambiguous_ownership_is_typed_rather_than_resolved() {
        // A bare pair on a managed editor channel could read as the archive pair or
        // the managed pair. Neither is chosen.
        let mapping = map_surface(&settled("server_dap_pair", "vscode_marketplace"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::ManagedPairRequiresManagedShape));

        let mapping = map_surface(&settled("server", "vscode_marketplace"));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::AmbiguousRouteOwnership));
    }

    // Falsifier 8: the registry and the bridge drifting into two vocabularies.
    // Every value the committed registry uses must be one this bridge has reviewed.
    #[test]
    fn bridge_covers_every_value_the_committed_registry_uses() -> Result<()> {
        let subjects = load_subjects(&default_registry_path()?)?;
        assert!(!subjects.is_empty());
        for subject in &subjects {
            let mapping = map_surface(subject);
            assert!(
                !mapping.reasons.iter().any(|reason| {
                    matches!(
                        reason,
                        ReasonCode::UnrecognizedProductShape
                            | ReasonCode::UnrecognizedTargetChannel
                            | ReasonCode::UnrecognizedPublicationStage
                            | ReasonCode::UnrecognizedReachability
                            | ReasonCode::UnrecognizedTopologyRelationship
                            | ReasonCode::UnrecognizedDisposition
                    )
                }),
                "{} uses registry vocabulary this bridge has not reviewed: {:?}",
                subject.surface_id,
                mapping.reasons
            );
        }
        Ok(())
    }

    /// Every committed row is `needs_disposition` today, so the bridge must report the
    /// whole registry as unresolved rather than inferring routes from shape.
    #[test]
    fn unresolved_registry_rows_produce_no_current_route_claim() -> Result<()> {
        let subjects = load_subjects(&default_registry_path()?)?;
        let report = explain_report(&subjects);
        assert_eq!(report.mapped_surface_count, subjects.len());
        for mapping in &report.mappings {
            assert!(
                !mapping.claims_current_route(),
                "{} claimed current route unit {} from an unclassified registry row",
                mapping.surface_id,
                mapping.route_product_unit.as_str()
            );
        }
        assert!(report.mappings.iter().any(|mapping| {
            mapping.reasons.contains(&ReasonCode::RegistryDispositionUnresolved)
        }));
        Ok(())
    }

    #[test]
    fn unresolved_disposition_blocks_an_otherwise_complete_row() {
        let mut subject = settled("server_dap_pair", "github_release");
        assert_eq!(unit_of(&subject), RouteProductUnit::ArchivePairRequired);
        subject.disposition = "needs_disposition".to_string();
        let mapping = map_surface(&subject);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::NotProven);
        assert!(mapping.reasons.contains(&ReasonCode::RegistryDispositionUnresolved));
    }

    #[test]
    fn repository_internal_surfaces_stay_non_authoritative() {
        let mut subject = settled("server", "repository_gate");
        subject.publication_stage = "candidate".to_string();
        subject.active_user_reachability = "candidate".to_string();
        subject.disposition = "candidate_only".to_string();
        let mapping = map_surface(&subject);
        assert_eq!(mapping.route_product_unit, RouteProductUnit::LocalDevelopmentNonAuthoritative);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::LocalDevelopmentOnly);
        assert!(!mapping.claims_current_route());
    }

    #[test]
    fn documentation_and_not_applicable_claim_no_product_unit() {
        for shape in ["documentation_only", "not_applicable"] {
            let mapping = map_surface(&settled(shape, "documentation"));
            assert_eq!(mapping.route_product_unit, RouteProductUnit::NotApplicable);
            assert_eq!(mapping.claim_ceiling, ClaimCeiling::NoProductClaim);
            assert!(mapping.reasons.contains(&ReasonCode::NoProductUnitClaimed));
        }
    }

    #[test]
    fn managed_shape_reaches_the_managed_pair_route() {
        let mapping = map_surface(&settled_with_product_relation(
            "managed_server_dap_pair",
            "vscode_marketplace",
        ));
        assert_eq!(mapping.route_product_unit, RouteProductUnit::ManagedEditorPair);
        assert_eq!(mapping.claim_ceiling, ClaimCeiling::CurrentPublicRoute);
    }

    #[test]
    fn every_rule_key_is_unique() {
        let mut keys: Vec<(ProductShape, ChannelClass)> =
            ROUTE_RULES.iter().map(|rule| (rule.product_shape, rule.channel_class)).collect();
        let total = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), total, "the mapping table has a duplicate (shape, channel) key");
    }

    #[test]
    fn a_current_route_rule_never_accepts_an_unclassified_disposition() {
        for rule in ROUTE_RULES {
            if rule.claim_ceiling == ClaimCeiling::CurrentPublicRoute {
                assert!(
                    !rule.allowed_dispositions.contains(&RegistryDisposition::NeedsDisposition),
                    "{:?}/{:?} would claim a current route from an unclassified row",
                    rule.product_shape,
                    rule.channel_class
                );
            }
        }
    }

    #[test]
    fn explain_output_is_deterministic_and_order_independent() -> Result<()> {
        let mut subjects = load_subjects(&default_registry_path()?)?;
        let first = render_explain(&explain_report(&subjects));
        subjects.reverse();
        let second = render_explain(&explain_report(&subjects));
        assert_eq!(first, second);
        assert!(first.contains("totals\tmapped="));
        Ok(())
    }

    #[test]
    fn registry_schema_drift_is_rejected() -> Result<()> {
        let source = "schema_version = \"install_surface_registry.v2\"\n\n\
                      [[surfaces]]\nsurface_id = \"a\"\ninstalled_product_unit = \"server\"\n\
                      target_channel = \"github_release\"\npublication_stage = \"source\"\n\
                      active_user_reachability = \"active\"\ntopology_relationship = \"checked\"\n\
                      disposition = \"needs_disposition\"\n";
        let error = match parse_subjects(source) {
            Ok(_) => return Err(eyre!("registry schema v2 unexpectedly parsed")),
            Err(error) => error,
        };
        assert!(error.to_string().contains("unsupported registry schema"), "{error}");
        Ok(())
    }
}
