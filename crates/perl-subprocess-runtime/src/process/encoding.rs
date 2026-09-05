//! Deterministic canonical encoding and fingerprints for the process domain.
//!
//! The encoding exists so that two plans that mean the same thing produce the
//! same bytes regardless of construction order, and so that a change in
//! meaning is detectable without shipping the underlying values.
//!
//! # Claim boundary
//!
//! [`Fingerprint`] is a **non-cryptographic** 128-bit FNV-1a digest. It proves
//! *canonical identity and change detection only*. It is not collision
//! resistant against an adversary, is not a signature, and must never be used
//! to authenticate a plan, authorize execution, or protect a secret. The crate
//! is deliberately dependency-free (see the crate README), so no cryptographic
//! hash is linked in; a stronger digest belongs to whichever authority
//! actually needs integrity, not to this domain model.

use std::fmt;

/// Canonical-encoding schema tag written ahead of every encoded value.
///
/// Tags are stable: reusing a tag for a different meaning is a schema change
/// and requires moving [`super::PROCESS_DOMAIN_SCHEMA_VERSION`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tag {
    /// A structural section marker.
    Section = 0x01,
    /// An unsigned integer.
    Unsigned = 0x02,
    /// A UTF-8 string.
    Text = 0x03,
    /// A closed-enum discriminant.
    Variant = 0x04,
    /// A boolean.
    Flag = 0x05,
    /// An absent optional value.
    Absent = 0x06,
    /// A nested fingerprint standing in for a private value.
    NestedFingerprint = 0x07,
}

/// Append-only writer producing the canonical byte form of a domain value.
///
/// Every write is tagged and length-prefixed so that no two distinct value
/// sequences can produce the same byte string by concatenation.
#[derive(Debug, Default)]
pub(crate) struct CanonicalEncoder {
    buf: Vec<u8>,
}

impl CanonicalEncoder {
    /// Create an empty encoder.
    pub(crate) fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Open a named structural section.
    pub(crate) fn section(&mut self, name: &str) -> &mut Self {
        self.buf.push(Tag::Section as u8);
        self.push_text(name);
        self
    }

    /// Write an unsigned integer field.
    pub(crate) fn unsigned(&mut self, value: u64) -> &mut Self {
        self.buf.push(Tag::Unsigned as u8);
        self.buf.extend_from_slice(&value.to_be_bytes());
        self
    }

    /// Write a UTF-8 text field.
    pub(crate) fn text(&mut self, value: &str) -> &mut Self {
        self.buf.push(Tag::Text as u8);
        self.push_text(value);
        self
    }

    /// Write a closed-enum discriminant.
    pub(crate) fn variant(&mut self, discriminant: u16) -> &mut Self {
        self.buf.push(Tag::Variant as u8);
        self.buf.extend_from_slice(&discriminant.to_be_bytes());
        self
    }

    /// Write a boolean field.
    pub(crate) fn flag(&mut self, value: bool) -> &mut Self {
        self.buf.push(Tag::Flag as u8);
        self.buf.push(u8::from(value));
        self
    }

    /// Write an explicit "absent" marker for an optional field.
    ///
    /// Absence is encoded rather than skipped so that an omitted field cannot
    /// collide with a present one.
    pub(crate) fn absent(&mut self) -> &mut Self {
        self.buf.push(Tag::Absent as u8);
        self
    }

    /// Write a fingerprint standing in for a value that must not be encoded.
    pub(crate) fn nested_fingerprint(&mut self, fingerprint: Fingerprint) -> &mut Self {
        self.buf.push(Tag::NestedFingerprint as u8);
        self.buf.extend_from_slice(&fingerprint.0.to_be_bytes());
        self
    }

    /// Finish encoding and return the canonical bytes.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.buf
    }

    /// Finish encoding and return the fingerprint of the canonical bytes.
    pub(crate) fn finish_fingerprint(self) -> Fingerprint {
        Fingerprint::of(&self.buf)
    }

    fn push_text(&mut self, value: &str) {
        let bytes = value.as_bytes();
        // Length-prefix so adjacent fields cannot be re-partitioned.
        self.buf.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        self.buf.extend_from_slice(bytes);
    }
}

/// FNV-1a 128-bit offset basis.
const FNV_OFFSET_BASIS: u128 = 144_066_263_297_769_815_596_495_629_667_062_367_629;
/// FNV-1a 128-bit prime (2^88 + 0x13b).
const FNV_PRIME: u128 = 309_485_009_821_345_068_724_781_371;

/// A deterministic, non-cryptographic 128-bit content fingerprint.
///
/// See the module documentation for the claim boundary: this identifies
/// content, it does not authenticate it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fingerprint(u128);

impl Fingerprint {
    /// Fingerprint an arbitrary byte slice.
    pub fn of(bytes: &[u8]) -> Self {
        let mut hash = FNV_OFFSET_BASIS;
        for byte in bytes {
            hash ^= u128::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        Self(hash)
    }

    /// Render the fingerprint as lowercase hexadecimal.
    pub fn to_hex(self) -> String {
        format!("{:032x}", self.0)
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// The public, secret-safe identity of a [`super::ProcessPlan`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanFingerprint(Fingerprint);

impl PlanFingerprint {
    /// Wrap a raw fingerprint as a plan fingerprint.
    pub(crate) fn new(fingerprint: Fingerprint) -> Self {
        Self(fingerprint)
    }

    /// The underlying fingerprint.
    pub fn fingerprint(self) -> Fingerprint {
        self.0
    }
}

impl fmt::Display for PlanFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The identity of captured stream content, independent of retention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentFingerprint(Fingerprint);

impl ContentFingerprint {
    /// Fingerprint observed content bytes.
    pub fn of(bytes: &[u8]) -> Self {
        Self(Fingerprint::of(bytes))
    }

    /// The underlying fingerprint.
    pub fn fingerprint(self) -> Fingerprint {
        self.0
    }
}

impl fmt::Display for ContentFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// The public stand-in for a private filesystem path.
///
/// A path fingerprint may appear in public identities; the path itself may
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathFingerprint(Fingerprint);

impl PathFingerprint {
    /// Fingerprint a path's platform-native representation.
    ///
    /// A platform tag is included so that a Unix byte path and a Windows wide
    /// path cannot collide across the encodings.
    pub(crate) fn of_native(path: &std::ffi::OsStr) -> Self {
        let mut bytes: Vec<u8> = Vec::new();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            bytes.push(b'u');
            bytes.extend_from_slice(path.as_bytes());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            bytes.push(b'w');
            for unit in path.encode_wide() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            // No native byte view is available; a lossy string is the only
            // representation, and its limitation is recorded here.
            bytes.push(b'o');
            bytes.extend_from_slice(path.to_string_lossy().as_bytes());
        }
        Self(Fingerprint::of(&bytes))
    }

    /// The underlying fingerprint.
    pub fn fingerprint(self) -> Fingerprint {
        self.0
    }
}

impl fmt::Display for PathFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
