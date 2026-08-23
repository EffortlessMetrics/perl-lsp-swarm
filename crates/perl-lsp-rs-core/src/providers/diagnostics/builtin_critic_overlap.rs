//! Collection of checked built-in critic overlap observations (#11918).
//!
//! The production cut routes these observations into the single critic
//! normalization call together with the native candidates. The values come
//! from the same lint emitter branches that produce the ordinary core
//! diagnostics — construction authority never leaves the emitters.

use std::sync::Arc;

use perl_parser_core::Node;
use perl_semantic_analyzer::analysis::symbol::SymbolExtractor;
use perl_semantic_analyzer::symbol::SymbolTable;

use super::lints::common_mistakes::check_common_mistakes_with_observations;
use super::lints::security::check_security_with_observations;
use crate::tooling::perl_critic::BuiltInCriticObservation;

/// Collect every checked built-in critic overlap observation for one parsed
/// document.
///
/// The ordinary diagnostics produced during this collection pass are
/// discarded: they are byte-identical to those the main provider pass emits,
/// because both passes run the same pure emitter branches over the same AST
/// and symbol table. The observations themselves carry only producer-declared
/// identity, shape, severity, message, and exact source span.
pub fn builtin_critic_overlap_observations(
    ast: &Arc<Node>,
    source: &str,
) -> Vec<BuiltInCriticObservation> {
    let symbol_table: SymbolTable = SymbolExtractor::new_with_source(source).extract(ast);
    let mut observations = Vec::new();
    // Discard-only sink: this pass contributes observations, not diagnostics.
    let mut discarded_diagnostics = Vec::new();

    check_common_mistakes_with_observations(
        ast,
        &symbol_table,
        &mut discarded_diagnostics,
        &mut observations,
    );
    check_security_with_observations(ast, &mut discarded_diagnostics, &mut observations);

    observations
}

#[cfg(test)]
mod tests {
    use super::builtin_critic_overlap_observations;
    use crate::tooling::perl_critic::{CriticFindingOrigin, CriticFindingShape};
    use perl_parser::Parser;
    use perl_tdd_support::must;

    fn observations_for(source: &str) -> Vec<(String, CriticFindingShape)> {
        let ast = must(Parser::new(source).parse());
        let ast = std::sync::Arc::new(ast);
        let mut observed: Vec<_> = builtin_critic_overlap_observations(&ast, source)
            .into_iter()
            .map(|observation| {
                (observation.identity().code().to_string(), observation.identity().shape())
            })
            .collect();
        // Walker order across statement kinds is not part of this contract.
        observed.sort();
        observed
    }

    #[test]
    fn system_document_yields_one_checked_system_observation() {
        let observed = observations_for("system('ls -la');\n");
        assert_eq!(observed, vec![("PL603".to_string(), CriticFindingShape::SystemCall)]);
    }

    #[test]
    fn mixed_cohort_document_yields_each_reviewed_shape() {
        let observed =
            observations_for("exec('ls');\nmy $out = readpipe('ls');\nif (undef == 5) { }\n");
        assert_eq!(
            observed,
            vec![
                ("PL404".to_string(), CriticFindingShape::LiteralUndefComparison),
                ("PL604".to_string(), CriticFindingShape::ExecCall),
                ("PL606".to_string(), CriticFindingShape::Readpipe),
            ]
        );
    }

    #[test]
    fn clean_document_yields_no_observations() {
        assert!(observations_for("my $x = 1;\nprint $x;\n").is_empty());
    }

    #[test]
    fn observations_always_carry_the_builtin_origin() {
        let ast = must(Parser::new("system('ls');\n").parse());
        let ast = std::sync::Arc::new(ast);
        for observation in builtin_critic_overlap_observations(&ast, "system('ls');\n") {
            assert_eq!(observation.identity().origin(), CriticFindingOrigin::BuiltInDiagnostic);
        }
    }
}
