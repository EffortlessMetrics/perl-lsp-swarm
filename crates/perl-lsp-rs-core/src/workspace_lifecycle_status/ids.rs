//! Bounded logical tokens, subject identity, and dimension provenance.
//!
//! Tokens are the only string carrier in this contract. They reject path
//! separators, environment assignment, whitespace, control characters, and
//! other hostile canaries so private absolute paths, source, secrets, and
//! logs cannot enter public-safe identity.

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use super::{
    MAX_HISTORY_ENTRIES, MAX_LOGICAL_TOKEN_BYTES, MAX_OPERATION_LINKS, MAX_REASONS_PER_ROW,
    MAX_STATUS_ROWS, ValidationError,
};

/// Opaque, bounded, hostile-content-refusing logical token.
///
/// Equality and hashing are defined over the exact admitted spelling. There
/// is no normalization, no `Default`, and no path/environment/source carrier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct LogicalToken(String);

impl LogicalToken {
    /// Parse a logical token, refusing empty, oversized, or hostile input.
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, ValidationError> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(ValidationError::EmptyLogicalToken);
        }
        if raw.len() > MAX_LOGICAL_TOKEN_BYTES {
            return Err(ValidationError::LogicalTokenTooLong {
                length: raw.len(),
                limit: MAX_LOGICAL_TOKEN_BYTES,
            });
        }
        if looks_hostile(raw) {
            return Err(ValidationError::HostilePublicContent);
        }
        if !raw.bytes().all(is_logical_token_byte) {
            return Err(ValidationError::InvalidLogicalTokenCharset);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Wire spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LogicalToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LogicalToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(raw).map_err(serde::de::Error::custom)
    }
}

fn is_logical_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':')
}

fn looks_hostile(raw: &str) -> bool {
    if raw.contains('/') || raw.contains('\\') || raw.contains('=') || raw.contains('$') {
        return true;
    }
    if raw.contains("-----BEGIN")
        || raw.contains("password")
        || raw.contains("SECRET")
        || raw.contains("AKIA")
        || raw.contains("ghp_")
    {
        return true;
    }
    if raw.contains("package ") || raw.contains("sub ") {
        return true;
    }
    if raw.len() >= 2 {
        let bytes = raw.as_bytes();
        if bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            return true;
        }
    }
    raw.contains("..")
}

/// One exact application/session plus workspace/root/runtime subject.
///
/// Configuration, trust, and environment identities live here as *identities*,
/// not as sibling status vocabularies. Live #7932/#6736/#7900 types are not
/// imported; later assemblers map into these tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceLifecycleSubject {
    session_id: LogicalToken,
    workspace_id: LogicalToken,
    root_id: LogicalToken,
    runtime_generation: LogicalToken,
    configuration_generation: LogicalToken,
    trust_generation: LogicalToken,
    environment_identity: LogicalToken,
}

impl WorkspaceLifecycleSubject {
    /// Bind one exact session/root/runtime subject.
    pub fn new(
        session_id: LogicalToken,
        workspace_id: LogicalToken,
        root_id: LogicalToken,
        runtime_generation: LogicalToken,
        configuration_generation: LogicalToken,
        trust_generation: LogicalToken,
        environment_identity: LogicalToken,
    ) -> Self {
        Self {
            session_id,
            workspace_id,
            root_id,
            runtime_generation,
            configuration_generation,
            trust_generation,
            environment_identity,
        }
    }

    /// Application/server session identity.
    #[must_use]
    pub const fn session_id(&self) -> &LogicalToken {
        &self.session_id
    }

    /// Logical workspace identity.
    #[must_use]
    pub const fn workspace_id(&self) -> &LogicalToken {
        &self.workspace_id
    }

    /// Logical root identity.
    #[must_use]
    pub const fn root_id(&self) -> &LogicalToken {
        &self.root_id
    }

    /// Root/runtime generation identity.
    #[must_use]
    pub const fn runtime_generation(&self) -> &LogicalToken {
        &self.runtime_generation
    }

    /// Accepted configuration generation identity.
    #[must_use]
    pub const fn configuration_generation(&self) -> &LogicalToken {
        &self.configuration_generation
    }

    /// Accepted trust generation identity.
    #[must_use]
    pub const fn trust_generation(&self) -> &LogicalToken {
        &self.trust_generation
    }

    /// Project-environment identity.
    #[must_use]
    pub const fn environment_identity(&self) -> &LogicalToken {
        &self.environment_identity
    }
}

/// Source identity and generations retained by one independent dimension.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionProvenance {
    session_id: LogicalToken,
    root_id: LogicalToken,
    runtime_generation: LogicalToken,
    source_identity: LogicalToken,
    domain_generation: LogicalToken,
}

impl DimensionProvenance {
    /// Bind one dimension observation to an exact subject and domain source.
    pub fn new(
        session_id: LogicalToken,
        root_id: LogicalToken,
        runtime_generation: LogicalToken,
        source_identity: LogicalToken,
        domain_generation: LogicalToken,
    ) -> Self {
        Self { session_id, root_id, runtime_generation, source_identity, domain_generation }
    }

    /// Session carried by this observation.
    #[must_use]
    pub const fn session_id(&self) -> &LogicalToken {
        &self.session_id
    }

    /// Root carried by this observation.
    #[must_use]
    pub const fn root_id(&self) -> &LogicalToken {
        &self.root_id
    }

    /// Runtime generation carried by this observation.
    #[must_use]
    pub const fn runtime_generation(&self) -> &LogicalToken {
        &self.runtime_generation
    }

    /// Domain source identity (reload transport, store backend, provider, …).
    #[must_use]
    pub const fn source_identity(&self) -> &LogicalToken {
        &self.source_identity
    }

    /// Domain-owned generation or fingerprint.
    #[must_use]
    pub const fn domain_generation(&self) -> &LogicalToken {
        &self.domain_generation
    }

    pub(super) fn must_join_subject(
        &self,
        subject: &WorkspaceLifecycleSubject,
    ) -> Result<(), ValidationError> {
        if self.session_id != subject.session_id {
            return Err(ValidationError::CrossSessionJoin);
        }
        if self.root_id != subject.root_id {
            return Err(ValidationError::CrossRootJoin);
        }
        if self.runtime_generation != subject.runtime_generation {
            return Err(ValidationError::CrossGenerationJoin);
        }
        Ok(())
    }
}

/// Producer of a validated status envelope. Display wording is not identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerIdentity {
    implementation: LogicalToken,
}

impl ProducerIdentity {
    /// Bind the producing implementation identity.
    pub fn new(implementation: LogicalToken) -> Self {
        Self { implementation }
    }

    /// Producing implementation token.
    #[must_use]
    pub const fn implementation(&self) -> &LogicalToken {
        &self.implementation
    }
}

/// Domain-separated fingerprint over canonical length-prefixed parts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct StatusFingerprint(String);

impl StatusFingerprint {
    pub(super) fn of_parts(domain: &[u8], parts: &[&[u8]]) -> Self {
        let mut canonical = Vec::with_capacity(64);
        canonical.extend_from_slice(domain);
        canonical.push(0);
        for part in parts {
            let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
            canonical.extend_from_slice(&len.to_be_bytes());
            canonical.extend_from_slice(part);
        }
        Self(crate::hashing::sha256_hex(&canonical))
    }

    /// Algorithm-tagged digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for StatusFingerprint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        if !raw.starts_with("sha256:") || raw.len() != "sha256:".len() + 64 {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"sha256:<64 lowercase hex digits>",
            ));
        }
        let hex = &raw["sha256:".len()..];
        if !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"sha256:<64 lowercase hex digits>",
            ));
        }
        Ok(Self(raw))
    }
}

pub(super) fn check_bound(
    what: &'static str,
    len: usize,
    limit: usize,
) -> Result<(), ValidationError> {
    if len > limit {
        Err(ValidationError::BoundExceeded { what, length: len, limit })
    } else {
        Ok(())
    }
}

pub(super) fn envelope_row_limit() -> usize {
    MAX_STATUS_ROWS
}

pub(super) fn reason_limit() -> usize {
    MAX_REASONS_PER_ROW
}

pub(super) fn operation_link_limit() -> usize {
    MAX_OPERATION_LINKS
}

pub(super) fn history_limit() -> u16 {
    MAX_HISTORY_ENTRIES
}
