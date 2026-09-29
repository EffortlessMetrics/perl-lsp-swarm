//! Generation, envelope, and scope identities. No secrets or private paths.

use std::fmt::Write as _;

use crate::hashing::sha256_hex;

/// Producer/schema generation of this accepted-view contract.
///
/// Bump only when identity-visible layout, family membership, or fingerprint
/// material changes.
pub(crate) const ACCEPTED_RUNTIME_LIMITS_SCHEMA_GENERATION: u32 = 2;

/// Length-tagged byte material so concatenated fingerprint parts cannot collide
/// on a shared prefix.
fn tag(prefix: &str, bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{byte:02x}");
    }
    format!("{prefix}{}:{hex}", bytes.len())
}

pub(crate) fn push_tagged(material: &mut String, key: &str, bytes: &[u8]) {
    let _ = write!(material, "{key}={};", tag(key, bytes));
}

/// Accepted configuration generation (#10857). Unbound until that store lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigurationGenerationIdentity {
    Bound { fingerprint: String },
    NotProven { reason: &'static str },
}

impl ConfigurationGenerationIdentity {
    pub(crate) fn bound(material: &str) -> Result<Self, IdentityError> {
        if material.is_empty() {
            return Err(IdentityError::EmptyMaterial);
        }
        Ok(Self::Bound { fingerprint: sha256_hex(material.as_bytes()) })
    }

    pub(crate) const fn not_proven(reason: &'static str) -> Self {
        Self::NotProven { reason }
    }

    pub(crate) fn fingerprint(&self) -> Option<&str> {
        match self {
            Self::Bound { fingerprint } => Some(fingerprint.as_str()),
            Self::NotProven { .. } => None,
        }
    }

    pub(crate) fn push_material(&self, material: &mut String) {
        match self {
            Self::Bound { fingerprint } => {
                push_tagged(material, "cfg", fingerprint.as_bytes());
            }
            Self::NotProven { reason } => {
                push_tagged(material, "cfg-np", reason.as_bytes());
            }
        }
    }
}

/// Product hard-envelope identity (#7479). Unbound until that owner lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HardEnvelopeIdentity {
    Bound { schema_generation: u32, fingerprint: String },
    NotProven { reason: &'static str },
}

impl HardEnvelopeIdentity {
    pub(crate) fn bound(schema_generation: u32, material: &str) -> Result<Self, IdentityError> {
        if material.is_empty() {
            return Err(IdentityError::EmptyMaterial);
        }
        Ok(Self::Bound { schema_generation, fingerprint: sha256_hex(material.as_bytes()) })
    }

    pub(crate) const fn not_proven(reason: &'static str) -> Self {
        Self::NotProven { reason }
    }

    pub(crate) fn push_material(&self, material: &mut String) {
        match self {
            Self::Bound { schema_generation, fingerprint } => {
                let _ = write!(material, "env-gen={schema_generation};");
                push_tagged(material, "env", fingerprint.as_bytes());
            }
            Self::NotProven { reason } => {
                push_tagged(material, "env-np", reason.as_bytes());
            }
        }
    }
}

/// Global versus root-scoped accepted-view subject. Root material is digested
/// so private paths never enter view identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScopeIdentity {
    Global,
    Root { fingerprint: String },
}

impl ScopeIdentity {
    pub(crate) fn root(root_identity: &str) -> Result<Self, IdentityError> {
        if root_identity.is_empty() {
            return Err(IdentityError::EmptyMaterial);
        }
        Ok(Self::Root { fingerprint: sha256_hex(root_identity.as_bytes()) })
    }

    pub(crate) const fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }

    pub(crate) fn push_material(&self, material: &mut String) {
        match self {
            Self::Global => material.push_str("scope=global;"),
            Self::Root { fingerprint } => {
                push_tagged(material, "scope-root", fingerprint.as_bytes());
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentityError {
    EmptyMaterial,
}
