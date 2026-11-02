//! Typed package/module resolution states.

use super::identity::{PackageId, PackageSourceTier};
use super::{
    Fingerprint, PACKAGE_TOPOLOGY_SCHEMA_TAG, PackageTopologyContractError, require_identity,
};
use crate::BoundaryKind;

use serde::{Deserialize, Serialize};

/// The request that a resolution state answers.
///
/// A request is not a [`PackageId`]: missing, dynamic, and unsupported
/// targets must not mint a fictional package node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageResolutionRequest {
    project_identity: String,
    resolution_domain: String,
    qualified_name: String,
}

impl PackageResolutionRequest {
    /// Construct a resolution request.
    ///
    /// # Errors
    /// Returns [`PackageTopologyContractError::EmptyIdentityField`] when any
    /// slot is empty. Rootless name identity is rejected here just as it is
    /// for [`PackageId`].
    pub fn new(
        project_identity: impl Into<String>,
        resolution_domain: impl Into<String>,
        qualified_name: impl Into<String>,
    ) -> Result<Self, PackageTopologyContractError> {
        let request = Self {
            project_identity: require_identity(
                "PackageResolutionRequest.project_identity",
                project_identity,
            )?,
            resolution_domain: require_identity(
                "PackageResolutionRequest.resolution_domain",
                resolution_domain,
            )?,
            qualified_name: require_identity(
                "PackageResolutionRequest.qualified_name",
                qualified_name,
            )?,
        };
        request.validate()?;
        Ok(request)
    }

    /// Validate request slots after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        require_identity(
            "PackageResolutionRequest.project_identity",
            self.project_identity.as_str(),
        )?;
        require_identity(
            "PackageResolutionRequest.resolution_domain",
            self.resolution_domain.as_str(),
        )?;
        require_identity("PackageResolutionRequest.qualified_name", self.qualified_name.as_str())?;
        Ok(())
    }

    /// Project or workspace identity of the request.
    #[must_use]
    pub fn project_identity(&self) -> &str {
        &self.project_identity
    }

    /// Resolution domain in which the name was requested.
    #[must_use]
    pub fn resolution_domain(&self) -> &str {
        &self.resolution_domain
    }

    /// Qualified package name that was requested.
    #[must_use]
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    /// Deterministic fingerprint of this request.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.fold(Fingerprint::new(PACKAGE_TOPOLOGY_SCHEMA_TAG)).finish()
    }

    pub(super) fn fold(&self, fp: Fingerprint) -> Fingerprint {
        fp.field("request-project", &self.project_identity)
            .field("request-domain", &self.resolution_domain)
            .field("request-name", &self.qualified_name)
    }
}

/// How a package/module request resolved.
///
/// These states remain distinct. There is no `External` arm: missing,
/// ambiguous, dynamic, unsupported, and resolved-dependency must not collapse
/// into [`crate::PackageKind::External`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageResolution {
    /// Resolved to a workspace package identity.
    Workspace(PackageId),
    /// Resolved to a dependency package identity at an explicit source tier.
    ResolvedDependency {
        /// Identity of the resolved dependency package.
        package_id: PackageId,
        /// Source tier of the resolved dependency.
        source_tier: PackageSourceTier,
    },
    /// Several candidate package identities remain.
    Ambiguous {
        /// The request that produced multiple candidates.
        request: PackageResolutionRequest,
        /// Distinct candidate package identities, sorted by fingerprint.
        candidates: Vec<PackageId>,
    },
    /// No package identity exists for the request.
    Missing {
        /// The request that did not resolve.
        request: PackageResolutionRequest,
    },
    /// Resolution crosses a dynamic boundary and cannot mint a package node.
    Dynamic {
        /// The request that remains dynamic.
        request: PackageResolutionRequest,
        /// Boundary that prevents exact resolution.
        boundary: BoundaryKind,
    },
    /// Resolution is unsupported for a recorded reason.
    Unsupported {
        /// The request that cannot be resolved.
        request: PackageResolutionRequest,
        /// Typed reason resolution is unsupported.
        reason: String,
    },
    /// Resolution was observed against a different generation than expected.
    Stale {
        /// The request whose answer is stale.
        request: PackageResolutionRequest,
        /// Generation the consumer expected.
        expected_generation: String,
        /// Generation that was actually observed.
        observed_generation: String,
    },
}

impl PackageResolution {
    /// Workspace resolution of a fully identified package.
    pub fn workspace(package_id: PackageId) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::Workspace(package_id);
        resolution.validate()?;
        Ok(resolution)
    }

    /// Resolved-dependency resolution. `source_tier` cannot be [`PackageSourceTier::Workspace`].
    pub fn resolved_dependency(
        package_id: PackageId,
        source_tier: PackageSourceTier,
    ) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::ResolvedDependency { package_id, source_tier };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Ambiguous resolution among at least two distinct candidates.
    pub fn ambiguous(
        request: PackageResolutionRequest,
        mut candidates: Vec<PackageId>,
    ) -> Result<Self, PackageTopologyContractError> {
        candidates.sort_by_key(PackageId::fingerprint);
        let resolution = Self::Ambiguous { request, candidates };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Missing target: the request did not resolve to any package identity.
    pub fn missing(
        request: PackageResolutionRequest,
    ) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::Missing { request };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Dynamic target: a boundary prevents minting a package node.
    pub fn dynamic(
        request: PackageResolutionRequest,
        boundary: BoundaryKind,
    ) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::Dynamic { request, boundary };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Unsupported target with a recorded reason.
    pub fn unsupported(
        request: PackageResolutionRequest,
        reason: impl Into<String>,
    ) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::Unsupported { request, reason: reason.into() };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Stale resolution: expected and observed generations disagree.
    pub fn stale(
        request: PackageResolutionRequest,
        expected_generation: impl Into<String>,
        observed_generation: impl Into<String>,
    ) -> Result<Self, PackageTopologyContractError> {
        let resolution = Self::Stale {
            request,
            expected_generation: expected_generation.into(),
            observed_generation: observed_generation.into(),
        };
        resolution.validate()?;
        Ok(resolution)
    }

    /// Validate a resolution state after transport.
    pub fn validate(&self) -> Result<(), PackageTopologyContractError> {
        match self {
            Self::Workspace(package_id) => package_id.validate(),
            Self::ResolvedDependency { package_id, source_tier } => {
                package_id.validate()?;
                if matches!(source_tier, PackageSourceTier::Workspace) {
                    return Err(PackageTopologyContractError::MalformedResolution(
                        "resolved-dependency cannot use the workspace source tier",
                    ));
                }
                Ok(())
            }
            Self::Ambiguous { request, candidates } => {
                request.validate()?;
                if candidates.len() < 2 {
                    return Err(PackageTopologyContractError::MalformedResolution(
                        "ambiguous resolution requires at least two candidates",
                    ));
                }
                for candidate in candidates {
                    candidate.validate()?;
                }
                let mut fingerprints: Vec<String> =
                    candidates.iter().map(PackageId::fingerprint).collect();
                let original = fingerprints.clone();
                fingerprints.sort();
                if fingerprints.windows(2).any(|pair| pair[0] == pair[1]) {
                    return Err(PackageTopologyContractError::MalformedResolution(
                        "ambiguous candidates must be distinct",
                    ));
                }
                if fingerprints != original {
                    return Err(PackageTopologyContractError::MalformedResolution(
                        "ambiguous candidates must be sorted by fingerprint",
                    ));
                }
                Ok(())
            }
            Self::Missing { request } => request.validate(),
            Self::Dynamic { request, .. } => request.validate(),
            Self::Unsupported { request, reason } => {
                request.validate()?;
                require_identity("PackageResolution.unsupported.reason", reason.as_str())?;
                Ok(())
            }
            Self::Stale { request, expected_generation, observed_generation } => {
                request.validate()?;
                require_identity(
                    "PackageResolution.stale.expected_generation",
                    expected_generation.as_str(),
                )?;
                require_identity(
                    "PackageResolution.stale.observed_generation",
                    observed_generation.as_str(),
                )?;
                if expected_generation == observed_generation {
                    return Err(PackageTopologyContractError::MalformedResolution(
                        "stale resolution requires distinct expected and observed generations",
                    ));
                }
                Ok(())
            }
        }
    }

    /// Stable discriminant tag used inside fingerprints.
    #[must_use]
    pub const fn tag(&self) -> &'static str {
        match self {
            Self::Workspace(_) => "workspace",
            Self::ResolvedDependency { .. } => "resolved-dependency",
            Self::Ambiguous { .. } => "ambiguous",
            Self::Missing { .. } => "missing",
            Self::Dynamic { .. } => "dynamic",
            Self::Unsupported { .. } => "unsupported",
            Self::Stale { .. } => "stale",
        }
    }

    /// The resolved [`PackageId`] when this state names exactly one package.
    #[must_use]
    pub fn resolved_package(&self) -> Option<&PackageId> {
        match self {
            Self::Workspace(package_id) | Self::ResolvedDependency { package_id, .. } => {
                Some(package_id)
            }
            Self::Ambiguous { .. }
            | Self::Missing { .. }
            | Self::Dynamic { .. }
            | Self::Unsupported { .. }
            | Self::Stale { .. } => None,
        }
    }

    /// Deterministic fingerprint of this resolution state.
    #[must_use]
    pub fn fingerprint(&self) -> String {
        self.fold(Fingerprint::new(PACKAGE_TOPOLOGY_SCHEMA_TAG)).finish()
    }

    pub(super) fn fold(&self, fp: Fingerprint) -> Fingerprint {
        let fp = fp.discriminant("resolution", self.tag());
        match self {
            Self::Workspace(package_id) => package_id.fold(fp),
            Self::ResolvedDependency { package_id, source_tier } => {
                source_tier.fold(package_id.fold(fp))
            }
            Self::Ambiguous { request, candidates } => {
                let fp = request.fold(fp).field("candidate-count", &candidates.len().to_string());
                candidates.iter().enumerate().fold(fp, |fp, (index, candidate)| {
                    candidate.fold(fp.field("candidate-index", &index.to_string()))
                })
            }
            Self::Missing { request } => request.fold(fp),
            Self::Dynamic { request, boundary } => {
                request.fold(fp).discriminant("boundary", boundary_kind_tag(*boundary))
            }
            Self::Unsupported { request, reason } => request.fold(fp).field("reason", reason),
            Self::Stale { request, expected_generation, observed_generation } => request
                .fold(fp)
                .field("expected-generation", expected_generation)
                .field("observed-generation", observed_generation),
        }
    }
}

const fn boundary_kind_tag(kind: BoundaryKind) -> &'static str {
    match kind {
        BoundaryKind::DynamicValue => "dynamic-value",
        BoundaryKind::DynamicRequire => "dynamic-require",
        BoundaryKind::DynamicIncludePath => "dynamic-include-path",
        BoundaryKind::CompileTimeExecution => "compile-time-execution",
        BoundaryKind::SymbolicReference => "symbolic-reference",
        BoundaryKind::Compatibility => "compatibility",
        BoundaryKind::ExternalEnvironment => "external-environment",
        BoundaryKind::Unsupported => "unsupported",
    }
}
