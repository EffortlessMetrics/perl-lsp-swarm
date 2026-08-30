//! Error classification inventory (#4982).
//!
//! Provides a read-only inventory of which error types implement `ErrorClass`
//! and their canonical dispositions, so CI/lint tooling can detect unreviewed
//! classification changes.

use crate::protocol::error_disposition::{Disposition, disposition_for};
use perl_parser_core::ErrorCategory;

/// A single entry in the error classification inventory.
#[derive(Debug, Clone)]
pub struct ErrorInventoryEntry {
    /// Type name (e.g. "FramingError", "BackendError").
    pub type_name: &'static str,
    /// Crate where the type is defined.
    pub crate_name: &'static str,
    /// Whether the type implements ErrorClass.
    pub has_error_class: bool,
    /// The category that would be assigned (sampled from the first variant
    /// for display purposes; actual classification may vary by variant).
    pub sample_category: Option<ErrorCategory>,
    /// The disposition for the sample category.
    pub sample_disposition: Option<Disposition>,
}

/// The full inventory of known error types across the workspace.
///
/// This is maintained manually and serves as a checklist for CI enforcement.
/// When a new error type is added, it should be registered here so the
/// inventory distinguishes classified types from intentionally neutral or
/// origin-ambiguous types.
pub fn error_type_inventory() -> Vec<ErrorInventoryEntry> {
    vec![
        // ── perl-parser-core ──
        ErrorInventoryEntry {
            type_name: "ParseError",
            crate_name: "perl-parser-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        // ── perl-lsp-rs-core (LSP boundary) ──
        ErrorInventoryEntry {
            type_name: "FramingError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "LaunchParseError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "CatalogError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "FormattingError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "CancellationError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
        ErrorInventoryEntry {
            type_name: "BackendError (inline-completion)",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "JsonRpcError",
            crate_name: "perl-lsp-rs-core",
            // Wire-only. A legacy finalized-code classifier remains outside the
            // model until #7612 adds originating classification and provenance.
            has_error_class: false,
            sample_category: None,
            sample_disposition: None,
        },
        // ── perl-dap (DAP boundary) ──
        ErrorInventoryEntry {
            type_name: "BackendError (DAP)",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "BreakpointError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "ValidationError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "SecurityError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "PeerFrameError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        // Full parse enums stay unclassified without a caller-supplied origin.
        // #8746 classifies those variants through OriginatedParseError; #8739
        // classifies only the fixed-origin projections beside them.
        ErrorInventoryEntry {
            type_name: "StackParseError",
            crate_name: "perl-dap",
            has_error_class: false, // Origin-ambiguous without OriginatedParseError
            sample_category: None,
            sample_disposition: None,
        },
        ErrorInventoryEntry {
            type_name: "VariableParseError",
            crate_name: "perl-dap",
            has_error_class: false, // Origin-ambiguous without OriginatedParseError
            sample_category: None,
            sample_disposition: None,
        },
        ErrorInventoryEntry {
            type_name: "FixedOriginStackParseError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
        ErrorInventoryEntry {
            type_name: "FixedOriginVariableParseError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::ResourceLimit),
            sample_disposition: Some(disposition_for(ErrorCategory::ResourceLimit)),
        },
        ErrorInventoryEntry {
            type_name: "OriginatedParseError<StackParseError>",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "OriginatedParseError<VariableParseError>",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "VariableReferenceError",
            crate_name: "perl-dap",
            has_error_class: true,
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
    ]
}

/// Returns the count of error types that have ErrorClass implemented.
#[must_use]
pub fn classified_count() -> usize {
    error_type_inventory().iter().filter(|e| e.has_error_class).count()
}

/// Returns the count of error types that do not implement ErrorClass.
#[must_use]
pub fn unclassified_count() -> usize {
    error_type_inventory().iter().filter(|e| !e.has_error_class).count()
}

/// Returns the names of unclassified error types.
#[must_use]
pub fn unclassified_types() -> Vec<&'static str> {
    error_type_inventory().iter().filter(|e| !e.has_error_class).map(|e| e.type_name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_has_entries() {
        let inv = error_type_inventory();
        assert!(!inv.is_empty(), "inventory must not be empty");
        assert!(inv.len() >= 15, "expected at least 15 error types, got {}", inv.len());
    }

    #[test]
    fn unclassified_types_are_wire_or_origin_ambiguous() {
        let unclassified = unclassified_types();
        assert_eq!(
            unclassified,
            vec!["JsonRpcError", "StackParseError", "VariableParseError"],
            "JsonRpcError is wire-only; legacy finalized-code classification stays outside it pending #7612; stack/variable parse enums stay unclassified without origin"
        );
    }

    #[test]
    fn origin_ambiguous_parse_enums_stay_unclassified_beside_classified_projections() {
        let inv = error_type_inventory();
        let by_name = |name: &str| {
            inv.iter()
                .find(|entry| entry.type_name == name)
                .unwrap_or_else(|| panic!("{name} must remain inventoried"))
        };

        let stack = by_name("StackParseError");
        assert!(!stack.has_error_class, "full StackParseError stays unclassified without origin");
        assert!(stack.sample_category.is_none());
        assert!(stack.sample_disposition.is_none());

        let variable = by_name("VariableParseError");
        assert!(
            !variable.has_error_class,
            "full VariableParseError stays unclassified without origin"
        );
        assert!(variable.sample_category.is_none());
        assert!(variable.sample_disposition.is_none());

        let stack_projection = by_name("FixedOriginStackParseError");
        assert!(stack_projection.has_error_class);
        assert_eq!(stack_projection.sample_category, Some(ErrorCategory::Bug));

        let variable_projection = by_name("FixedOriginVariableParseError");
        assert!(variable_projection.has_error_class);
        assert_eq!(variable_projection.sample_category, Some(ErrorCategory::ResourceLimit));

        let originated_stack = by_name("OriginatedParseError<StackParseError>");
        assert!(originated_stack.has_error_class);
        assert_eq!(originated_stack.sample_category, Some(ErrorCategory::Protocol));

        let originated_variable = by_name("OriginatedParseError<VariableParseError>");
        assert!(originated_variable.has_error_class);
        assert_eq!(originated_variable.sample_category, Some(ErrorCategory::Protocol));
    }

    #[test]
    fn classified_count_matches() {
        let total = error_type_inventory().len();
        let classified = classified_count();
        let unclassified = unclassified_count();
        assert_eq!(classified + unclassified, total);
    }

    #[test]
    fn all_classified_entries_have_dispositions() {
        for entry in error_type_inventory() {
            if entry.has_error_class {
                assert!(
                    entry.sample_category.is_some(),
                    "{} has ErrorClass but no sample category",
                    entry.type_name
                );
                assert!(
                    entry.sample_disposition.is_some(),
                    "{} has ErrorClass but no sample disposition",
                    entry.type_name
                );
            }
        }
    }
}
