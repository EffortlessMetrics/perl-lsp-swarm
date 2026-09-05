//! Error classification inventory (#4982).
//!
//! Provides a read-only inventory of which error types implement `ErrorClass`
//! and their canonical dispositions, so CI/lint tooling can detect gaps
//! when new error types are added without classification.

use crate::protocol::error_disposition::{Disposition, disposition_for};
use perl_parser_core::ErrorCategory;

/// How a type's canonical [`ErrorCategory`] is reached.
///
/// A type can carry a canonical category without implementing `ErrorClass`:
/// runtime-neutral errors are projected onto the Perl taxonomy by an
/// application-owned adapter instead (#13997). Distinguishing the two keeps
/// [`unclassified_types`] a list of real gaps rather than a list of
/// deliberate neutrality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationRoute {
    /// The type implements `perl_parser_core::ErrorClass` directly.
    TraitImpl,
    /// The type is deliberately taxonomy-neutral; the named application-owned
    /// adapter projects it onto an [`ErrorCategory`].
    AppAdapter(&'static str),
    /// The type has no canonical category yet. This is a real gap.
    Unclassified,
}

/// A single entry in the error classification inventory.
#[derive(Debug, Clone)]
pub struct ErrorInventoryEntry {
    /// Type name (e.g. "FramingError", "BackendError").
    pub type_name: &'static str,
    /// Crate where the type is defined.
    pub crate_name: &'static str,
    /// Whether the type implements ErrorClass.
    ///
    /// This is narrower than "has a canonical category": see
    /// [`ErrorInventoryEntry::classification_route`].
    pub has_error_class: bool,
    /// How this type's canonical category is reached.
    pub classification_route: ClassificationRoute,
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
/// inventory test can detect missing ErrorClass implementations.
pub fn error_type_inventory() -> Vec<ErrorInventoryEntry> {
    vec![
        // ── perl-parser-core ──
        ErrorInventoryEntry {
            type_name: "ParseError",
            crate_name: "perl-parser-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        // ── perl-lsp-rs-core (LSP boundary) ──
        ErrorInventoryEntry {
            type_name: "FramingError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "LaunchParseError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "CatalogError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "FormattingError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        // Taxonomy-neutral runtime error (#13997): the type deliberately does
        // not implement ErrorClass so the cancellation mechanism can move below
        // the Perl product (#7611). Its category comes from the app-owned
        // adapter, which #7612 retires.
        ErrorInventoryEntry {
            type_name: "CancellationError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: false,
            classification_route: ClassificationRoute::AppAdapter(
                crate::protocol::cancellation_error_class::CANCELLATION_ERROR_ADAPTER,
            ),
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
        ErrorInventoryEntry {
            type_name: "BackendError (inline-completion)",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        // `JsonRpcError` implements `ErrorClass` in `protocol/jsonrpc.rs`, mapping
        // JSON-RPC codes to categories. The row previously read `false` with a
        // "needs type refinement (#4978)" note; #4978 has since closed and the
        // impl is present, so the row was stale. Its category varies by code —
        // `Protocol` for parse/invalid-request, `UserError` for
        // method-not-found/invalid-params, `Bug` for internal and unknown codes,
        // `Transient` for cancellation/content-modified, `Infra` for
        // server-not-initialized — so the sample below is representative only,
        // matching the first arm. #4978 notes that `RequestFailed` ideally
        // carries its originating category rather than inferring one from the
        // code; that refinement is not this row's to make.
        ErrorInventoryEntry {
            type_name: "JsonRpcError",
            crate_name: "perl-lsp-rs-core",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        // ── perl-dap (DAP boundary) ──
        ErrorInventoryEntry {
            type_name: "BackendError (DAP)",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Infra),
            sample_disposition: Some(disposition_for(ErrorCategory::Infra)),
        },
        ErrorInventoryEntry {
            type_name: "BreakpointError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "ValidationError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "SecurityError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::UserError),
            sample_disposition: Some(disposition_for(ErrorCategory::UserError)),
        },
        ErrorInventoryEntry {
            type_name: "PeerFrameError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
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
            classification_route: ClassificationRoute::Unclassified,
            sample_category: None,
            sample_disposition: None,
        },
        ErrorInventoryEntry {
            type_name: "VariableParseError",
            crate_name: "perl-dap",
            has_error_class: false, // Origin-ambiguous without OriginatedParseError
            classification_route: ClassificationRoute::Unclassified,
            sample_category: None,
            sample_disposition: None,
        },
        ErrorInventoryEntry {
            type_name: "FixedOriginStackParseError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
        ErrorInventoryEntry {
            type_name: "FixedOriginVariableParseError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::ResourceLimit),
            sample_disposition: Some(disposition_for(ErrorCategory::ResourceLimit)),
        },
        ErrorInventoryEntry {
            type_name: "OriginatedParseError<StackParseError>",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "OriginatedParseError<VariableParseError>",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Protocol),
            sample_disposition: Some(disposition_for(ErrorCategory::Protocol)),
        },
        ErrorInventoryEntry {
            type_name: "VariableReferenceError",
            crate_name: "perl-dap",
            has_error_class: true,
            classification_route: ClassificationRoute::TraitImpl,
            sample_category: Some(ErrorCategory::Bug),
            sample_disposition: Some(disposition_for(ErrorCategory::Bug)),
        },
    ]
}

/// Returns the count of error types that have a canonical category, whether
/// through an `ErrorClass` impl or an application-owned adapter.
#[must_use]
pub fn classified_count() -> usize {
    error_type_inventory()
        .iter()
        .filter(|e| e.classification_route != ClassificationRoute::Unclassified)
        .count()
}

/// Returns the count of error types that still need a canonical category.
#[must_use]
pub fn unclassified_count() -> usize {
    error_type_inventory()
        .iter()
        .filter(|e| e.classification_route == ClassificationRoute::Unclassified)
        .count()
}

/// Returns the names of error types that still need a canonical category.
///
/// A deliberately taxonomy-neutral type classified through an adapter is not a
/// gap and must not appear here.
#[must_use]
pub fn unclassified_types() -> Vec<&'static str> {
    error_type_inventory()
        .iter()
        .filter(|e| e.classification_route == ClassificationRoute::Unclassified)
        .map(|e| e.type_name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_some_with;

    #[test]
    fn inventory_has_entries() {
        let inv = error_type_inventory();
        assert!(!inv.is_empty(), "inventory must not be empty");
        assert!(inv.len() >= 15, "expected at least 15 error types, got {}", inv.len());
    }

    #[test]
    fn unclassified_types_are_only_the_origin_ambiguous_parse_errors() {
        let unclassified = unclassified_types();
        assert_eq!(
            unclassified,
            vec!["StackParseError", "VariableParseError"],
            "stack/variable parse enums stay unclassified without origin; JsonRpcError is classified in protocol::jsonrpc"
        );
    }

    #[test]
    fn jsonrpc_error_is_inventoried_as_classified_because_it_implements_the_trait() {
        // Regression guard for a row that had drifted: the inventory recorded
        // `has_error_class: false` for `JsonRpcError` long after
        // `impl ErrorClass for JsonRpcError` landed in `protocol/jsonrpc.rs`,
        // which made both the gap list and the classified/unclassified counts
        // wrong. Bind the row to the trait rather than to a hand-maintained bool.
        const _: () = assert!(
            ErrorClassProbe::<crate::protocol::JsonRpcError>::IMPLEMENTS_ERROR_CLASS,
            "JsonRpcError implements ErrorClass; the inventory row must say so"
        );

        let inv = error_type_inventory();
        let rows: Vec<_> = inv.iter().filter(|e| e.type_name == "JsonRpcError").collect();
        assert_eq!(rows.len(), 1, "JsonRpcError must remain inventoried exactly once");

        for row in rows {
            assert!(row.has_error_class, "JsonRpcError implements ErrorClass");
            assert_eq!(row.classification_route, ClassificationRoute::TraitImpl);
            assert!(
                row.sample_category.is_some(),
                "a classified row must record a representative category"
            );
        }
        assert!(!unclassified_types().contains(&"JsonRpcError"));
    }

    /// Compile-time "does `T` implement `ErrorClass`?" probe.
    ///
    /// Inherent associated items win over trait ones, but only where the
    /// inherent impl applies — so the bound below selects `true` exactly when
    /// `T: ErrorClass`, and the blanket trait impl answers otherwise.
    struct ErrorClassProbe<T>(core::marker::PhantomData<T>);

    #[expect(
        dead_code,
        reason = "Fallback arm of an inherent-vs-blanket specialization emulation: its \
                  associated const is observed through trait resolution when the more \
                  specific `impl<T: ErrorClass> ErrorClassProbe<T>` does not apply, never \
                  through a direct call the dead-code pass can see."
    )]
    trait ErrorClassProbeFallback {
        const IMPLEMENTS_ERROR_CLASS: bool = false;
    }

    impl<T> ErrorClassProbeFallback for ErrorClassProbe<T> {}

    impl<T: perl_parser_core::ErrorClass> ErrorClassProbe<T> {
        const IMPLEMENTS_ERROR_CLASS: bool = true;
    }

    #[test]
    fn origin_ambiguous_parse_enums_stay_unclassified_beside_classified_projections() {
        let inv = error_type_inventory();
        let by_name = |name: &str| {
            must_some_with(
                inv.iter().find(|entry| entry.type_name == name),
                format!("{name} must remain inventoried"),
            )
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
    fn cancellation_error_is_classified_through_the_app_adapter_not_a_trait_impl() {
        let inv = error_type_inventory();
        let entries: Vec<_> = inv.iter().filter(|e| e.type_name == "CancellationError").collect();
        assert_eq!(entries.len(), 1, "CancellationError must remain inventoried exactly once");

        for entry in entries {
            assert!(
                !entry.has_error_class,
                "CancellationError must stay taxonomy-neutral (#13997): no ErrorClass impl"
            );
            assert_eq!(
                entry.classification_route,
                ClassificationRoute::AppAdapter(
                    crate::protocol::cancellation_error_class::CANCELLATION_ERROR_ADAPTER
                ),
                "the app-owned adapter is the single category map for CancellationError"
            );
            assert_eq!(
                entry.sample_category,
                Some(ErrorCategory::Bug),
                "neutrality must not lose the recorded category"
            );
        }
        assert!(
            !unclassified_types().contains(&"CancellationError"),
            "deliberate neutrality is not a classification gap"
        );
    }

    #[test]
    fn all_classified_entries_have_dispositions() {
        for entry in error_type_inventory() {
            if entry.classification_route != ClassificationRoute::Unclassified {
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
