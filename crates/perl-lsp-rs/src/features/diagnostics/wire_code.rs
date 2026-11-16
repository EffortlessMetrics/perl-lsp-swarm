//! Wire diagnostic-code metadata resolution across the owning registries.
//!
//! A diagnostic on the wire carries one of three identity kinds, and each kind
//! has exactly one owning registry:
//!
//! 1. built-in `PL*` codes, owned by `perl_diagnostics::codes::DiagnosticCode`;
//! 2. native-critic `native.*` codes, owned by the shared critic identity
//!    registry ([`CriticIdentityRegistry`]), which reviews each code's
//!    equivalence with built-in codes;
//! 3. `dead-code-*` codes, owned by the dead-code provider's identity table.
//!
//! The enrichment seams (push conversion in `runtime::diagnostics`, pull
//! projection in `features::diagnostics::pull`) must consult all three, so
//! every identity emitted on the wire resolves to the same `data.category` and
//! `codeDescription.href` on both transports (#17241). A code no registry owns
//! stays `Other` with no documentation link rather than being guessed.

use perl_diagnostics::codes::DiagnosticCode;
use perl_lsp_rs_core::tooling::perl_critic::CriticIdentityRegistry;

/// Category reported for a code that no registry owns.
pub(crate) const UNREGISTERED_CATEGORY: &str = "Other";

/// Resolve the wire category of a diagnostic code string.
///
/// Built-in codes report their catalog category, native-critic codes report
/// the reviewed critic-identity category (the same value the critic seam
/// reports for the row on pull), and dead-code codes report the dead-code
/// provider's category.
pub(crate) fn wire_code_category(code: &str) -> String {
    if let Some(builtin) = DiagnosticCode::parse_code(code) {
        return format!("{:?}", builtin.category());
    }
    if let Some(category) = CriticIdentityRegistry::category_for_native_code(code) {
        return format!("{category:?}");
    }
    if perl_lsp_rs_core::providers::diagnostics::identity_for_code(code).is_some() {
        return perl_lsp_rs_core::providers::diagnostics::DEAD_CODE_CATEGORY.to_string();
    }
    UNREGISTERED_CATEGORY.to_string()
}

/// Resolve the documentation URL of a diagnostic code string.
///
/// Built-in codes link their own page. Native-critic codes link the page of
/// their reviewed built-in alias when every canonical entry that owns the code
/// agrees on exactly one alias; shape-split codes whose shapes alias different
/// built-ins (for example `native.security.qx_readpipe`, whose `qx` and
/// `readpipe` shapes alias `PL601`/`PL606`) deliberately resolve no page here
/// rather than guessing a shape the wire code does not carry.
pub(crate) fn wire_code_documentation_url(code: &str) -> Option<&'static str> {
    if let Some(builtin) = DiagnosticCode::parse_code(code) {
        return builtin.documentation_url();
    }
    CriticIdentityRegistry::unambiguous_builtin_alias_code(code)
        .and_then(DiagnosticCode::parse_code)
        .and_then(|builtin| builtin.documentation_url())
}

#[cfg(test)]
mod tests {
    use super::{UNREGISTERED_CATEGORY, wire_code_category, wire_code_documentation_url};

    #[test]
    fn builtin_codes_keep_catalog_category_and_url() {
        assert_eq!(wire_code_category("PL406"), "BestPractices");
        assert_eq!(wire_code_category("PL600"), "Security");
        assert_eq!(
            wire_code_documentation_url("PL406"),
            Some("https://docs.perl-lsp.org/errors/PL406"),
        );
    }

    #[test]
    fn native_codes_resolve_critic_identity_category() {
        assert_eq!(wire_code_category("native.common.unreachable_code"), "Maintainability");
        assert_eq!(wire_code_category("native.testing.require_use_strict"), "Syntax");
        assert_eq!(wire_code_category("native.io.unchecked_open_close"), "Security");
    }

    #[test]
    fn native_equivalent_alias_resolves_builtin_documentation_url() {
        // printf_format_arity has exactly one reviewed built-in alias (PL405).
        assert_eq!(
            wire_code_documentation_url("native.common.printf_format_arity"),
            Some("https://docs.perl-lsp.org/errors/PL405"),
        );
    }

    #[test]
    fn shape_split_native_code_resolves_no_documentation_url() {
        // qx/readpipe shapes alias different built-ins (PL601/PL606); the wire
        // code alone must not pick one.
        assert_eq!(wire_code_documentation_url("native.security.qx_readpipe"), None);
        assert_eq!(
            wire_code_category("native.security.qx_readpipe"),
            "Security",
            "the owning entries agree on the category",
        );
    }

    #[test]
    fn distinct_native_code_without_builtin_alias_resolves_no_documentation_url() {
        assert_eq!(
            wire_code_documentation_url("native.io.unchecked_open_close"),
            None,
            "no built-in diagnostic has the same checked-call contract",
        );
    }

    #[test]
    fn dead_code_codes_resolve_provider_owned_category_and_no_url() {
        assert_eq!(wire_code_category("dead-code-subroutine"), "Maintainability");
        assert_eq!(wire_code_category("dead-code-package"), "Maintainability");
        assert_eq!(wire_code_documentation_url("dead-code-subroutine"), None);
    }

    #[test]
    fn unregistered_codes_stay_other_without_url() {
        assert_eq!(wire_code_category("definitely.not.a.code"), UNREGISTERED_CATEGORY);
        assert_eq!(wire_code_documentation_url("definitely.not.a.code"), None);
        assert_eq!(wire_code_category(""), UNREGISTERED_CATEGORY);
    }
}
