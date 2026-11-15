//! Checked length-prefix encoding for `source_identity.v1`.
//!
//! Every hashed field is prefixed with a big-endian `u32` length. Fields longer
//! than [`SOURCE_IDENTITY_V1_MAX_FIELD_BYTES`] cannot be represented without
//! truncating that prefix; constructors reject them fail-closed instead.

/// Maximum byte length of any single length-prefixed field in `source_identity.v1`.
pub const SOURCE_IDENTITY_V1_MAX_FIELD_BYTES: u32 = u32::MAX;

/// A caller supplied more bytes than the v1 length prefix can represent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceIdentityFieldLengthError {
    actual_bytes: usize,
}

impl SourceIdentityFieldLengthError {
    /// The inclusive maximum field size accepted by v1 encoding.
    pub const MAX_BYTES: u32 = SOURCE_IDENTITY_V1_MAX_FIELD_BYTES;

    /// How many bytes the caller attempted to encode.
    #[must_use]
    pub fn actual_bytes(self) -> usize {
        self.actual_bytes
    }
}

impl std::fmt::Display for SourceIdentityFieldLengthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "source_identity.v1 field length {} exceeds the v1 maximum of {} bytes",
            self.actual_bytes,
            Self::MAX_BYTES
        )
    }
}

impl std::error::Error for SourceIdentityFieldLengthError {}

/// Map a slice length to the v1 prefix integer without silent `usize`→`u32` truncation.
pub(crate) fn field_length_u32(len: usize) -> Result<u32, SourceIdentityFieldLengthError> {
    u32::try_from(len).map_err(|_too_long| SourceIdentityFieldLengthError { actual_bytes: len })
}

/// Fail closed when an infallible constructor receives an oversize field.
#[expect(
    clippy::panic,
    reason = "policy:15064: v1 infallible constructors panic rather than truncate length prefixes"
)]
pub(crate) fn infallible_precondition_violation(err: SourceIdentityFieldLengthError) -> ! {
    panic!("{err}");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn field_length_accepts_u32_max() {
        assert!(matches!(field_length_u32(u32::MAX as usize), Ok(u32::MAX)));
    }

    #[test]
    fn field_length_rejects_u32_max_plus_one_on_wide_usize() {
        if usize::BITS <= u32::BITS {
            return;
        }
        let oversize = u32::MAX as usize + 1;
        assert_eq!(
            field_length_u32(oversize),
            Err(SourceIdentityFieldLengthError { actual_bytes: oversize })
        );
    }

    /// Restoring a wrapping `as u32` cast must make this fail: the wrapped length
    /// would not equal the true byte length.
    #[test]
    fn field_length_rejects_wrapping_cast_at_boundary() {
        if usize::BITS <= u32::BITS {
            return;
        }
        let oversize = u32::MAX as usize + 5;
        let wrapped = oversize as u32;
        let oversize_u64 = u64::from(u32::MAX) + 5;
        assert_ne!(
            u64::from(wrapped),
            oversize_u64,
            "mutation control: wrapping cast must diverge from the true length"
        );
        assert!(field_length_u32(oversize).is_err());
    }
}
