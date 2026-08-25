//! Stable identity primitives for maintained compiler operating profiles.
//!
//! These newtypes are the only way profile, version, row, and digest identity
//! enter the model; each constructor validates its token so malformed identity
//! cannot be represented. Identity values participate in the deterministic
//! semantic fingerprint through [`super::fingerprint::CanonicalEncode`].

use std::fmt::Display;
use std::fmt::Formatter;

use super::CompilerProfileError;
use super::fingerprint::CanonWriter;
use super::fingerprint::CanonicalEncode;

/// Stable identifier of a maintained compiler profile (`compiler_local_lexical`).
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompilerProfileId(String);

impl CompilerProfileId {
    /// Validated constructor; rejects tokens outside `[a-z0-9._-]` or empty.
    pub fn new(value: &str) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(value) {
            return Err(CompilerProfileError::Identity {
                message: format!(
                    "profile id {value:?} must match [a-z0-9._-] and start alphanumeric"
                ),
            });
        }
        Ok(Self(value.to_string()))
    }

    /// The underlying token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CompilerProfileId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl CanonicalEncode for CompilerProfileId {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("pid", &self.0);
    }
}

/// Version selector of a profile (`v1`). Always begins with `v`.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompilerProfileVersion(String);

impl CompilerProfileVersion {
    /// Validated constructor; requires a stable token beginning with `v`.
    pub fn new(value: &str) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(value) || !value.starts_with('v') {
            return Err(CompilerProfileError::Identity {
                message: format!(
                    "profile version {value:?} must be a stable token starting with 'v'"
                ),
            });
        }
        Ok(Self(value.to_string()))
    }

    /// The underlying token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CompilerProfileVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl CanonicalEncode for CompilerProfileVersion {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("pver", &self.0);
    }
}

/// Stable identifier of one denominator-style row inside a profile. Row IDs
/// own the obligation, never wording or position.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct CompilerProfileRowId(String);

impl CompilerProfileRowId {
    /// Validated constructor; rejects tokens outside `[a-z0-9._-]`.
    pub fn new(value: &str) -> Result<Self, CompilerProfileError> {
        if !super::is_stable_token(value) {
            return Err(CompilerProfileError::Identity {
                message: format!("row id {value:?} must match [a-z0-9._-] and start alphanumeric"),
            });
        }
        Ok(Self(value.to_string()))
    }

    /// The underlying token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CompilerProfileRowId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl CanonicalEncode for CompilerProfileRowId {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("rid", &self.0);
    }
}

/// Content digest of a profile's semantic fingerprint, rendered as 16 lowercase
/// hex characters (FNV-1a 64). Imports bind against this exact value.
#[derive(Clone, Debug, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct ProfileContentDigest(String);

impl ProfileContentDigest {
    /// Validated constructor; requires exactly 16 lowercase hex characters.
    pub fn new(value: &str) -> Result<Self, CompilerProfileError> {
        let well_formed = value.len() == 16
            && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
        if !well_formed {
            return Err(CompilerProfileError::Identity {
                message: format!(
                    "profile content digest {value:?} must be 16 lowercase hex characters"
                ),
            });
        }
        Ok(Self(value.to_string()))
    }

    /// Digest derived from a raw 64-bit semantic fingerprint.
    pub fn from_fingerprint(fingerprint: u64) -> Self {
        Self(format!("{fingerprint:016x}"))
    }

    /// Consumes the digest and returns its hex token.
    pub fn into_inner(self) -> String {
        self.0
    }

    /// The underlying hex token.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ProfileContentDigest {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl CanonicalEncode for ProfileContentDigest {
    fn encode(&self, writer: &mut CanonWriter) {
        writer.str_field("dig", &self.0);
    }
}
