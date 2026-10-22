//! Dead code detection using workspace-wide symbol analysis

use super::internal_types::{Diagnostic, DiagnosticTag};
use perl_diagnostics::codes::DiagnosticSeverity;

/// Wire identity of one dead-code symbol kind (#17241).
///
/// The dead-code provider owns the `dead-code-*` identity space: these codes
/// are emitted on the wire by [`detect_dead_code`] and resolved by transport
/// enrichment from this one table, so the emitted identity and its published
/// metadata cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadCodeIdentity {
    /// Stable wire code (for example, `"dead-code-subroutine"`).
    pub code: &'static str,
    /// Symbol subject used in the user-visible message.
    pub subject: &'static str,
}

const SUBROUTINE_IDENTITY: DeadCodeIdentity =
    DeadCodeIdentity { code: "dead-code-subroutine", subject: "subroutine" };
const VARIABLE_IDENTITY: DeadCodeIdentity =
    DeadCodeIdentity { code: "dead-code-variable", subject: "variable" };
const CONSTANT_IDENTITY: DeadCodeIdentity =
    DeadCodeIdentity { code: "dead-code-constant", subject: "constant" };
const PACKAGE_IDENTITY: DeadCodeIdentity =
    DeadCodeIdentity { code: "dead-code-package", subject: "package" };

/// Every registered dead-code wire identity, in emission order.
pub static DEAD_CODE_IDENTITIES: &[DeadCodeIdentity] =
    &[SUBROUTINE_IDENTITY, VARIABLE_IDENTITY, CONSTANT_IDENTITY, PACKAGE_IDENTITY];

/// Wire category reported for every dead-code identity.
///
/// Unused workspace symbols are a maintainability concern; the dead-code
/// provider owns this label for its identity space the way the critic
/// identity registry owns `Maintainability` for its native rows (#17241).
pub const DEAD_CODE_CATEGORY: &str = "Maintainability";

/// Resolve a wire code to its registered dead-code identity.
#[must_use]
pub fn identity_for_code(code: &str) -> Option<&'static DeadCodeIdentity> {
    DEAD_CODE_IDENTITIES.iter().find(|identity| identity.code == code)
}

/// Detect dead code using workspace-wide symbol analysis
///
/// Identifies unused symbols (subroutines, variables, constants, packages)
/// that have no references in the workspace. Returns diagnostics for symbols
/// in the specified document.
///
/// # Arguments
///
/// * `workspace_index` - Workspace-wide symbol index
/// * `document_uri` - URI of the document to generate diagnostics for
/// * `source_text` - The source text of the document (for position conversion)
/// * `line_index` - Line index helper for position conversion
///
/// # Returns
///
/// Dead code diagnostics for symbols in the specified document
#[cfg(not(target_arch = "wasm32"))]
pub fn detect_dead_code(
    workspace_index: &perl_workspace::workspace_index::WorkspaceIndex,
    document_uri: &str,
    source_text: &str,
    line_index: &perl_parser_core::position::LineStartsCache,
) -> Vec<Diagnostic> {
    use perl_workspace::workspace_index::SymbolKind;

    let unused_symbols = workspace_index.find_unused_symbols();
    let mut diagnostics = Vec::new();

    for symbol in unused_symbols {
        // Only report diagnostics for symbols in the current document
        if symbol.uri != document_uri {
            continue;
        }

        // Determine diagnostic code and message subject based on symbol kind
        let identity: &'static DeadCodeIdentity = match symbol.kind {
            SymbolKind::Subroutine => &SUBROUTINE_IDENTITY,
            SymbolKind::Variable(_) => &VARIABLE_IDENTITY,
            SymbolKind::Constant => &CONSTANT_IDENTITY,
            SymbolKind::Package => &PACKAGE_IDENTITY,
            _ => continue, // Skip other symbol kinds
        };

        let message = format!("Unused {}: '{}'", identity.subject, symbol.name);

        // Convert line/column to byte offsets using the line index
        let start_byte = line_index.position_to_offset(
            source_text,
            symbol.range.start.line,
            symbol.range.start.column,
        );
        let end_byte = line_index.position_to_offset(
            source_text,
            symbol.range.end.line,
            symbol.range.end.column,
        );

        diagnostics.push(Diagnostic {
            range: (start_byte, end_byte),
            severity: DiagnosticSeverity::Hint,
            code: Some(identity.code.to_string()),
            message,
            related_information: Vec::new(),
            tags: vec![DiagnosticTag::Unnecessary],
            fixable: false,
            critic_observation: None,
            suggestion: Some(format!("Remove unused {} '{}'", identity.subject, symbol.name)),
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::{DEAD_CODE_CATEGORY, DEAD_CODE_IDENTITIES, identity_for_code};

    #[test]
    fn every_registered_identity_resolves_from_its_wire_code() {
        for identity in DEAD_CODE_IDENTITIES {
            assert_eq!(
                identity_for_code(identity.code),
                Some(identity),
                "wire code {} must resolve to its own identity",
                identity.code,
            );
        }
        assert_eq!(identity_for_code("dead-code-unknown"), None);
        assert_eq!(identity_for_code("PL300"), None);
    }

    #[test]
    fn identity_table_stays_complete_over_the_emitted_kinds() {
        let codes: Vec<&str> = DEAD_CODE_IDENTITIES.iter().map(|i| i.code).collect();
        assert_eq!(
            codes,
            [
                "dead-code-subroutine",
                "dead-code-variable",
                "dead-code-constant",
                "dead-code-package",
            ],
            "the table is the single owner of the dead-code wire identities",
        );
        for identity in DEAD_CODE_IDENTITIES {
            assert!(!identity.subject.is_empty(), "{} must carry a message subject", identity.code);
        }
        assert_eq!(DEAD_CODE_CATEGORY, "Maintainability");
    }
}
