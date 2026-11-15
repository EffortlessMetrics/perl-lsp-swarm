//! Perl-owned operational projection of neutral framing failures (#13996).
//!
//! This preserves the former trait implementation's category map. The generic
//! error never imports this module. The DAP peer-error adapter consumes this
//! explicit projection until #7612/#7602 converge the application boundary.

use crate::transport::FramingError;
use perl_parser_core::ErrorCategory;

/// Inventory identity for the application-owned framing category projection.
pub const FRAMING_ERROR_ADAPTER: &str =
    "perl_lsp_rs_core::protocol::framing_error_class::framing_error_category";

/// Project typed framing facts onto the existing Perl operational taxonomy.
///
/// Match variants, never display text or JSON-RPC codes. A new framing variant
/// requires an explicit policy decision rather than a catch-all category.
#[must_use]
pub const fn framing_error_category(error: &FramingError) -> ErrorCategory {
    match error {
        FramingError::InvalidHeader
        | FramingError::InvalidHeaderUtf8
        | FramingError::MissingContentLength
        | FramingError::InvalidContentLength => ErrorCategory::Protocol,
        FramingError::FrameTooLarge { .. } => ErrorCategory::ResourceLimit,
    }
}

#[cfg(test)]
mod tests {
    use super::framing_error_category;
    use crate::transport::{FramingError, MAX_FRAME_SIZE};
    use perl_parser_core::ErrorCategory;

    #[test]
    fn framing_errors_have_stable_operational_categories() {
        for error in [
            FramingError::InvalidHeader,
            FramingError::InvalidHeaderUtf8,
            FramingError::MissingContentLength,
            FramingError::InvalidContentLength,
        ] {
            assert_eq!(framing_error_category(&error), ErrorCategory::Protocol);
        }

        assert_eq!(
            framing_error_category(&FramingError::FrameTooLarge { len: MAX_FRAME_SIZE + 1 }),
            ErrorCategory::ResourceLimit
        );
    }

    #[test]
    fn category_depends_on_the_variant_not_the_numeric_payload() {
        for len in [0, MAX_FRAME_SIZE, MAX_FRAME_SIZE + 1, usize::MAX] {
            assert_eq!(
                framing_error_category(&FramingError::FrameTooLarge { len }),
                ErrorCategory::ResourceLimit
            );
        }
        assert_ne!(
            framing_error_category(&FramingError::InvalidContentLength),
            framing_error_category(&FramingError::FrameTooLarge { len: 0 })
        );
    }
}
