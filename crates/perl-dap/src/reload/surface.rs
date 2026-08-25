//! Protocol REQUIREMENTS for the reload request family (no wire format).
//!
//! These are the requirements the negotiated custom DAP family must
//! satisfy. The wire format, registration, and Rust/TypeScript contract
//! synchronization belong to #10138; this module defines only what the
//! family must mean and refuse. Nothing here is advertised, dispatched,
//! or reachable from a DAP client today.

/// The required shape of the custom family name: a non-empty namespace, a
/// `/` separator, and a non-empty local name (for example
/// `perl-lsp/loadedModuleReload`). The name must not collide with any
/// standard DAP request name the adapter dispatches.
pub const FAMILY_NAMESPACE_SEPARATOR: char = '/';

/// What kind of payload a proposed reload request carries.
///
/// The typed subject is the only admissible shape: a request that carries
/// a raw path, a debugger command, or a Perl expression is refused — the
/// client cannot authorize a reload through raw input reconstruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadRequestPayload {
    /// A fully typed module-subject identity (the only admissible shape).
    TypedModuleSubject,
    /// A raw filesystem path string.
    RawPath(String),
    /// A raw debugger command string.
    DebuggerCommand(String),
    /// A raw Perl expression string.
    PerlExpression(String),
}

impl ReloadRequestPayload {
    /// Whether this payload is the admissible typed shape.
    pub fn is_typed(&self) -> bool {
        matches!(self, ReloadRequestPayload::TypedModuleSubject)
    }

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub fn kind_code(&self) -> &'static str {
        match self {
            ReloadRequestPayload::TypedModuleSubject => "typed_module_subject",
            ReloadRequestPayload::RawPath(_) => "raw_path",
            ReloadRequestPayload::DebuggerCommand(_) => "debugger_command",
            ReloadRequestPayload::PerlExpression(_) => "perl_expression",
        }
    }
}

/// How the feature is projected into capabilities.
///
/// The contract stage requires [`ReloadCapabilityProjection::Unadvertised`]:
/// no capability is advertised until the R04 exact-proof leaf lands. An
/// invented standard DAP capability is never valid at any stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadCapabilityProjection {
    /// Not advertised (required until R04 proof).
    Unadvertised,
    /// Advertised under a namespaced, non-standard name (only after R04).
    NamespacedCustom(String),
    /// Claimed as a standard DAP capability spelling (never valid).
    InventedStandard(String),
}

/// A proposed reload request surface, as a reviewable model (not wire).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadRequestSurfaceDescriptor {
    /// Proposed family name (must be namespaced and non-colliding).
    pub family: String,
    /// Proposed family version (must be at least 1).
    pub family_version: u32,
    /// Correlation identity carried by every request/response pair
    /// (must be present).
    pub correlation_identity: Option<u64>,
    /// The payload kind (must be the typed subject).
    pub payload: ReloadRequestPayload,
    /// The capability projection (must be unadvertised at this stage).
    pub capability: ReloadCapabilityProjection,
}

impl Default for ReloadRequestSurfaceDescriptor {
    fn default() -> Self {
        ReloadRequestSurfaceDescriptor {
            family: "perl-lsp/loadedModuleReload".to_string(),
            family_version: 1,
            correlation_identity: Some(1),
            payload: ReloadRequestPayload::TypedModuleSubject,
            capability: ReloadCapabilityProjection::Unadvertised,
        }
    }
}

/// Why a proposed surface violates the protocol requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceViolation {
    /// The family name is not a non-empty `namespace/name` pair.
    UnnamespacedFamily,
    /// The family carries no version.
    UnversionedFamily,
    /// The family collides with a standard DAP request name.
    StandardFamilyCollision,
    /// Requests carry no correlation identity.
    MissingCorrelationIdentity,
    /// The request accepts a raw path, debugger command, or Perl
    /// expression instead of the typed subject.
    RawClientInputAccepted,
    /// A standard DAP capability spelling was invented for the feature.
    StandardCapabilityCollision,
    /// The feature is advertised before the R04 exact proof exists.
    PrematureCapabilityAdvertisement,
}

impl SurfaceViolation {
    /// All violations in frozen check order.
    pub const ALL: [SurfaceViolation; 7] = [
        SurfaceViolation::StandardFamilyCollision,
        SurfaceViolation::UnnamespacedFamily,
        SurfaceViolation::UnversionedFamily,
        SurfaceViolation::MissingCorrelationIdentity,
        SurfaceViolation::RawClientInputAccepted,
        SurfaceViolation::StandardCapabilityCollision,
        SurfaceViolation::PrematureCapabilityAdvertisement,
    ];

    /// Stable closed-vocabulary code used by the `.spec` fixtures.
    pub const fn code(self) -> &'static str {
        match self {
            SurfaceViolation::UnnamespacedFamily => "unnamespaced_family",
            SurfaceViolation::UnversionedFamily => "unversioned_family",
            SurfaceViolation::StandardFamilyCollision => "standard_family_collision",
            SurfaceViolation::MissingCorrelationIdentity => "missing_correlation_identity",
            SurfaceViolation::RawClientInputAccepted => "raw_client_input_accepted",
            SurfaceViolation::StandardCapabilityCollision => "standard_capability_collision",
            SurfaceViolation::PrematureCapabilityAdvertisement => {
                "premature_capability_advertisement"
            }
        }
    }
}

/// Validate a proposed surface against the frozen protocol requirements.
///
/// Frozen check precedence: exact standard-name collision (checked against
/// the adapter's single supported-command authority — a bare standard
/// request name can never be the custom family), namespacing shape,
/// versioning, correlation identity, payload shape, capability
/// projection. The first violation is returned; a valid descriptor passes.
pub fn validate_request_surface(
    descriptor: &ReloadRequestSurfaceDescriptor,
) -> Result<(), SurfaceViolation> {
    let family = descriptor.family.trim();
    if crate::debug_adapter::is_supported_dap_command(family) {
        return Err(SurfaceViolation::StandardFamilyCollision);
    }
    let mut separator_index = None;
    for (index, character) in descriptor.family.char_indices() {
        if character == FAMILY_NAMESPACE_SEPARATOR {
            separator_index = Some(index);
            break;
        }
    }
    let namespaced = match separator_index {
        Some(index) => {
            let (namespace, local) = descriptor.family.split_at(index);
            let local = &local[FAMILY_NAMESPACE_SEPARATOR.len_utf8()..];
            !namespace.trim().is_empty() && !local.trim().is_empty()
        }
        None => false,
    };
    if !namespaced {
        return Err(SurfaceViolation::UnnamespacedFamily);
    }
    if descriptor.family_version == 0 {
        return Err(SurfaceViolation::UnversionedFamily);
    }
    if descriptor.correlation_identity.is_none() {
        return Err(SurfaceViolation::MissingCorrelationIdentity);
    }
    if !descriptor.payload.is_typed() {
        return Err(SurfaceViolation::RawClientInputAccepted);
    }
    match &descriptor.capability {
        ReloadCapabilityProjection::Unadvertised => Ok(()),
        ReloadCapabilityProjection::NamespacedCustom(_) => {
            Err(SurfaceViolation::PrematureCapabilityAdvertisement)
        }
        ReloadCapabilityProjection::InventedStandard(_) => {
            Err(SurfaceViolation::StandardCapabilityCollision)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_conforming_descriptor_passes() {
        assert_eq!(validate_request_surface(&ReloadRequestSurfaceDescriptor::default()), Ok(()));
    }

    #[test]
    fn every_violation_is_reachable_with_its_exact_code() {
        let cases: Vec<(ReloadRequestSurfaceDescriptor, SurfaceViolation)> = vec![
            (
                ReloadRequestSurfaceDescriptor {
                    family: "loadedModuleReload".to_string(),
                    ..Default::default()
                },
                SurfaceViolation::UnnamespacedFamily,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    family: "perl-lsp/".to_string(),
                    ..Default::default()
                },
                SurfaceViolation::UnnamespacedFamily,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    family: "/reload".to_string(),
                    ..Default::default()
                },
                SurfaceViolation::UnnamespacedFamily,
            ),
            (
                ReloadRequestSurfaceDescriptor { family_version: 0, ..Default::default() },
                SurfaceViolation::UnversionedFamily,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    family: "modules".to_string(),
                    ..Default::default()
                },
                SurfaceViolation::StandardFamilyCollision,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    family: "restart".to_string(),
                    ..Default::default()
                },
                SurfaceViolation::StandardFamilyCollision,
            ),
            (
                ReloadRequestSurfaceDescriptor { correlation_identity: None, ..Default::default() },
                SurfaceViolation::MissingCorrelationIdentity,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    payload: ReloadRequestPayload::RawPath("/etc/passwd".to_string()),
                    ..Default::default()
                },
                SurfaceViolation::RawClientInputAccepted,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    payload: ReloadRequestPayload::DebuggerCommand("p $x".to_string()),
                    ..Default::default()
                },
                SurfaceViolation::RawClientInputAccepted,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    payload: ReloadRequestPayload::PerlExpression(
                        "delete $INC{'App/Core.pm'}".to_string(),
                    ),
                    ..Default::default()
                },
                SurfaceViolation::RawClientInputAccepted,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    capability: ReloadCapabilityProjection::InventedStandard(
                        "supportsModuleReload".to_string(),
                    ),
                    ..Default::default()
                },
                SurfaceViolation::StandardCapabilityCollision,
            ),
            (
                ReloadRequestSurfaceDescriptor {
                    capability: ReloadCapabilityProjection::NamespacedCustom(
                        "perl-lsp.moduleReload".to_string(),
                    ),
                    ..Default::default()
                },
                SurfaceViolation::PrematureCapabilityAdvertisement,
            ),
        ];
        for (descriptor, expected) in cases {
            assert_eq!(
                validate_request_surface(&descriptor),
                Err(expected),
                "descriptor {:?} must fail with {}",
                descriptor.family,
                expected.code()
            );
        }
    }

    #[test]
    fn a_bare_standard_command_name_can_never_be_the_custom_family() {
        for command in crate::debug_adapter::SUPPORTED_COMMANDS {
            let descriptor = ReloadRequestSurfaceDescriptor {
                family: command.to_string(),
                ..Default::default()
            };
            assert_eq!(
                validate_request_surface(&descriptor),
                Err(SurfaceViolation::StandardFamilyCollision),
                "standard command {command} must not be usable as the family name"
            );
        }
        // Namespacing is the collision-resistant escape: a namespaced
        // family whose local name resembles a standard command does not
        // collide with it.
        let namespaced = ReloadRequestSurfaceDescriptor {
            family: "perl-lsp/loadedModuleReload".to_string(),
            ..Default::default()
        };
        assert_eq!(validate_request_surface(&namespaced), Ok(()));
    }
}
