//! Moo/Moose/Role::Tiny role conflict diagnostics (same-file and cross-file).
//!
//! Checks for roles consumed by a class that provide overlapping method names.
//! Same-file role definitions are resolved from the local AST; roles defined in
//! other files are resolved via [`SemanticQueries::role_provided_methods`].
//! Transitive role composition (roles that themselves consume other roles) is
//! traversed with cycle protection through a BFS queue, using both same-file
//! ClassModel data and cross-file [`SemanticQueries::transitive_composed_roles`].

use std::collections::{HashMap, HashSet, VecDeque};

use super::super::internal_types::Diagnostic;
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{
    class_model::{ClassModel, ClassModelBuilder},
    symbol::{SymbolKind, SymbolTable},
};
use perl_workspace::semantic::queries::SemanticQueries;

/// Check for Moo/Moose/Role::Tiny role method conflicts.
///
/// When `semantic_queries` is provided, roles not defined in the same file are
/// resolved across the workspace. Roles whose definitions cannot be resolved are
/// skipped (fail-closed: no guessed conflict).
///
/// Transitive role composition is also traversed: a role that itself consumes
/// other roles contributes all transitively-provided methods to the conflict
/// check.
pub fn check_role_conflicts(
    node: &Node,
    symbol_table: &SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
    semantic_queries: &dyn SemanticQueries,
) {
    let mut role_models: HashMap<String, ClassModel> = HashMap::new();
    let mut class_models: Vec<ClassModel> = Vec::new();

    for model in ClassModelBuilder::new().build(node) {
        match package_kind(symbol_table, &model.name) {
            Some(SymbolKind::Role) => {
                role_models.insert(model.name.clone(), model);
            }
            Some(SymbolKind::Class) => {
                class_models.push(model);
            }
            _ => {}
        }
    }

    for class_model in class_models {
        if class_model.roles.is_empty() {
            continue;
        }

        let class_methods = provided_method_names(&class_model);
        let mut method_providers: HashMap<String, Vec<String>> = HashMap::new();

        // BFS queue: start with the class's directly consumed roles and expand
        // transitively. visited_roles prevents re-processing and handles cycles.
        let mut visited_roles: HashSet<String> = HashSet::new();
        let mut role_queue: VecDeque<String> = class_model.roles.iter().cloned().collect();

        while let Some(role_name) = role_queue.pop_front() {
            if !visited_roles.insert(role_name.clone()) {
                continue;
            }

            if let Some(role_model) = role_models.get(&role_name) {
                // Same-file role: collect its methods directly.
                for method_name in provided_method_names(role_model) {
                    method_providers.entry(method_name).or_default().push(role_name.clone());
                }
                // Enqueue roles transitively consumed by this role (same-file).
                for nested in &role_model.roles {
                    if !visited_roles.contains(nested) {
                        role_queue.push_back(nested.clone());
                    }
                }
            } else {
                // Cross-file role: resolve methods and nested compositions via workspace.
                let xfile_methods = semantic_queries.role_provided_methods(&role_name);
                for method_name in xfile_methods {
                    method_providers.entry(method_name).or_default().push(role_name.clone());
                }
                // Enqueue transitively composed roles from the workspace graph.
                for nested in semantic_queries.transitive_composed_roles(&role_name) {
                    if !visited_roles.contains(&nested) {
                        role_queue.push_back(nested);
                    }
                }
            }
        }

        for (method_name, mut providers) in method_providers {
            if providers.len() < 2 || class_methods.contains(&method_name) {
                continue;
            }

            // Deterministic provider order for consistent diagnostic messages.
            providers.sort();
            providers.dedup();
            if providers.len() < 2 {
                continue;
            }

            let Some(location) = role_anchor_location(symbol_table, &providers) else {
                continue;
            };

            diagnostics.push(Diagnostic {
                range: location,
                severity: DiagnosticSeverity::Warning,
                code: Some(DiagnosticCode::RoleConflict.as_str().to_string()),
                message: build_message(&class_model.name, &method_name, &providers),
                related_information: Vec::new(),
                tags: Vec::new(),
                suggestion: Some(format!(
                    "Define `{method_name}` in `{}` or remove one of the conflicting roles.",
                    class_model.name
                )),
            });
        }
    }
}

fn package_kind(symbol_table: &SymbolTable, package_name: &str) -> Option<SymbolKind> {
    symbol_table.symbols.get(package_name)?.iter().find_map(|symbol| match symbol.kind {
        SymbolKind::Class | SymbolKind::Role => Some(symbol.kind),
        _ => None,
    })
}

fn provided_method_names(model: &ClassModel) -> HashSet<String> {
    model.methods.iter().chain(model.adjusts.iter()).map(|method| method.name.clone()).collect()
}

fn role_anchor_location(
    symbol_table: &SymbolTable,
    role_names: &[String],
) -> Option<(usize, usize)> {
    for role_name in role_names {
        if let Some(reference) = symbol_table.references.get(role_name).and_then(|references| {
            references.iter().find(|reference| reference.kind == SymbolKind::Role)
        }) {
            return Some((reference.location.start, reference.location.end));
        }
    }

    None
}

fn build_message(class_name: &str, method_name: &str, role_names: &[String]) -> String {
    let role_list = format_role_list(role_names);
    let provider_verb = if role_names.len() == 2 { "both provide" } else { "all provide" };
    format!("Roles {role_list} {provider_verb} method `{method_name}` consumed by `{class_name}`")
}

fn format_role_list(role_names: &[String]) -> String {
    match role_names {
        [] => String::from(""),
        [single] => format!("`{single}`"),
        [first, second] => format!("`{first}` and `{second}`"),
        many => {
            let mut parts: Vec<String> =
                many[..many.len() - 1].iter().map(|name| format!("`{name}`")).collect();
            parts.push(format!("and `{}`", many[many.len() - 1]));
            parts.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_parser::Parser;
    use perl_semantic_analyzer::analysis::symbol::SymbolExtractor;
    use perl_semantic_facts::{
        DefinitionCandidate, EntityFact, OccurrenceFact, RenamePlan, SafeDeletePlan, VisibleSymbol,
    };
    use perl_semantic_facts::{EntityId, FileId, ScopeId};
    use perl_tdd_support::{must, must_some};
    use perl_workspace::semantic::queries::{DynamicCallableEvidence, QueryContext};

    /// Minimal SemanticQueries implementation for tests that only need same-file behavior.
    struct NullQueries;

    impl perl_workspace::semantic::queries::SemanticQueries for NullQueries {
        fn symbol_at(&self, _: FileId, _: u32) -> Option<(EntityFact, OccurrenceFact)> {
            None
        }
        fn definitions(&self, _: &str, _: &QueryContext) -> Vec<DefinitionCandidate> {
            Vec::new()
        }
        fn references(&self, _: EntityId) -> Vec<OccurrenceFact> {
            Vec::new()
        }
        fn visible_symbols_at(&self, _: FileId, _: u32, _: Option<ScopeId>) -> Vec<VisibleSymbol> {
            Vec::new()
        }
        fn method_candidates(&self, _: &str, _: &str) -> Vec<DefinitionCandidate> {
            Vec::new()
        }
        fn rename_plan(&self, id: EntityId, new_name: &str) -> RenamePlan {
            RenamePlan::new(id, String::new(), new_name.to_string(), vec![], vec![], vec![])
        }
        fn safe_delete_plan(&self, id: EntityId) -> SafeDeletePlan {
            SafeDeletePlan::new(id, String::new(), vec![], vec![])
        }
        fn dynamic_boundary_at(
            &self,
            _: FileId,
            _: u32,
            _: Option<&str>,
        ) -> Option<OccurrenceFact> {
            None
        }
        fn dynamic_callable_may_be_visible_at(
            &self,
            _: FileId,
            _: u32,
            _: &str,
        ) -> Option<DynamicCallableEvidence> {
            None
        }
    }

    /// SemanticQueries implementation for cross-file tests with programmable role data.
    struct MockRoleQueries {
        /// role_name -> list of bare method names it provides.
        role_methods: std::collections::HashMap<String, Vec<String>>,
        /// role_name -> list of roles it transitively composes.
        role_compositions: std::collections::HashMap<String, Vec<String>>,
    }

    impl MockRoleQueries {
        fn new() -> Self {
            Self {
                role_methods: std::collections::HashMap::new(),
                role_compositions: std::collections::HashMap::new(),
            }
        }

        fn with_role_methods(mut self, role: &str, methods: &[&str]) -> Self {
            self.role_methods
                .insert(role.to_string(), methods.iter().map(|s| s.to_string()).collect());
            self
        }

        fn with_role_compositions(mut self, role: &str, nested_roles: &[&str]) -> Self {
            self.role_compositions
                .insert(role.to_string(), nested_roles.iter().map(|s| s.to_string()).collect());
            self
        }
    }

    impl perl_workspace::semantic::queries::SemanticQueries for MockRoleQueries {
        fn symbol_at(&self, _: FileId, _: u32) -> Option<(EntityFact, OccurrenceFact)> {
            None
        }
        fn definitions(&self, _: &str, _: &QueryContext) -> Vec<DefinitionCandidate> {
            Vec::new()
        }
        fn references(&self, _: EntityId) -> Vec<OccurrenceFact> {
            Vec::new()
        }
        fn visible_symbols_at(&self, _: FileId, _: u32, _: Option<ScopeId>) -> Vec<VisibleSymbol> {
            Vec::new()
        }
        fn method_candidates(&self, _: &str, _: &str) -> Vec<DefinitionCandidate> {
            Vec::new()
        }
        fn rename_plan(&self, id: EntityId, new_name: &str) -> RenamePlan {
            RenamePlan::new(id, String::new(), new_name.to_string(), vec![], vec![], vec![])
        }
        fn safe_delete_plan(&self, id: EntityId) -> SafeDeletePlan {
            SafeDeletePlan::new(id, String::new(), vec![], vec![])
        }
        fn dynamic_boundary_at(
            &self,
            _: FileId,
            _: u32,
            _: Option<&str>,
        ) -> Option<OccurrenceFact> {
            None
        }
        fn dynamic_callable_may_be_visible_at(
            &self,
            _: FileId,
            _: u32,
            _: &str,
        ) -> Option<DynamicCallableEvidence> {
            None
        }
        fn role_provided_methods(&self, role_name: &str) -> Vec<String> {
            self.role_methods.get(role_name).cloned().unwrap_or_default()
        }
        fn transitive_composed_roles(&self, role_name: &str) -> Vec<String> {
            self.role_compositions.get(role_name).cloned().unwrap_or_default()
        }
    }

    fn role_conflict_diags(source: &str) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let symbol_table = SymbolExtractor::new_with_source(source).extract(&ast);
        let mut diagnostics = Vec::new();
        check_role_conflicts(&ast, &symbol_table, &mut diagnostics, &NullQueries);
        diagnostics
    }

    fn role_conflict_diags_with_queries(
        source: &str,
        queries: &dyn perl_workspace::semantic::queries::SemanticQueries,
    ) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let symbol_table = SymbolExtractor::new_with_source(source).extract(&ast);
        let mut diagnostics = Vec::new();
        check_role_conflicts(&ast, &symbol_table, &mut diagnostics, queries);
        diagnostics
    }

    fn has_code(diags: &[Diagnostic], code: &str) -> bool {
        diags.iter().any(|d| d.code.as_deref() == Some(code))
    }

    #[test]
    fn two_roles_with_same_method_fires_pl303() {
        let source = r#"
package MyRole::Greet;
use Moo::Role;
sub greet { return "hello" }

package MyRole::Welcome;
use Moo::Role;
sub greet { return "welcome" }

package MyClass;
use Moo;
with 'MyRole::Greet', 'MyRole::Welcome';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            has_code(&diags, "PL303"),
            "two roles with same method should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn class_overriding_conflicting_method_suppresses_pl303() {
        let source = r#"
package MyRole::Greet;
use Moo::Role;
sub greet { return "hello" }

package MyRole::Welcome;
use Moo::Role;
sub greet { return "welcome" }

package MyClass;
use Moo;
with 'MyRole::Greet', 'MyRole::Welcome';
sub greet { return "my custom greeting" }
"#;
        let diags = role_conflict_diags(source);
        assert!(
            !has_code(&diags, "PL303"),
            "class providing its own `greet` should suppress PL303: {diags:?}"
        );
    }

    #[test]
    fn roles_with_non_overlapping_methods_no_pl303() {
        let source = r#"
package MyRole::Greet;
use Moo::Role;
sub greet { return "hello" }

package MyRole::Farewell;
use Moo::Role;
sub farewell { return "goodbye" }

package MyClass;
use Moo;
with 'MyRole::Greet', 'MyRole::Farewell';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            !has_code(&diags, "PL303"),
            "non-overlapping role methods should not fire PL303: {diags:?}"
        );
    }

    #[test]
    fn single_role_consumed_no_pl303() {
        let source = r#"
package MyRole::Greet;
use Moo::Role;
sub greet { return "hello" }

package MyClass;
use Moo;
with 'MyRole::Greet';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            !has_code(&diags, "PL303"),
            "single role consumption should not fire PL303: {diags:?}"
        );
    }

    #[test]
    fn three_roles_all_with_same_method_fires_pl303() {
        let source = r#"
package MyRole::A;
use Moo::Role;
sub process { return "A" }

package MyRole::B;
use Moo::Role;
sub process { return "B" }

package MyRole::C;
use Moo::Role;
sub process { return "C" }

package MyClass;
use Moo;
with 'MyRole::A', 'MyRole::B', 'MyRole::C';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            has_code(&diags, "PL303"),
            "three roles with the same method should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn diagnostic_message_names_conflicting_method() {
        let source = r#"
package MyRole::A;
use Moo::Role;
sub run { 1 }

package MyRole::B;
use Moo::Role;
sub run { 1 }

package MyClass;
use Moo;
with 'MyRole::A', 'MyRole::B';
"#;
        let diags = role_conflict_diags(source);
        let pl303 = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL303")));
        let msg = &pl303.message;
        assert!(msg.contains("run"), "message should name the conflicting method `run`: {msg}");
    }

    #[test]
    fn class_without_any_roles_no_pl303() {
        let source = r#"
package MyClass;
use Moo;
sub greet { "hello" }
"#;
        let diags = role_conflict_diags(source);
        assert!(!has_code(&diags, "PL303"), "class with no roles should not fire PL303: {diags:?}");
    }

    #[test]
    fn plain_package_without_oo_framework_no_pl303() {
        let source = r#"
package MyPackage;
sub greet { "hello" }
"#;
        let diags = role_conflict_diags(source);
        assert!(
            !has_code(&diags, "PL303"),
            "plain package without Moo/Moose should not fire PL303: {diags:?}"
        );
    }

    #[test]
    fn pl303_diagnostic_includes_suggestion() {
        let source = r#"
package MyRole::A;
use Moo::Role;
sub handle { 1 }

package MyRole::B;
use Moo::Role;
sub handle { 1 }

package MyClass;
use Moo;
with 'MyRole::A', 'MyRole::B';
"#;
        let diags = role_conflict_diags(source);
        let pl303 = must_some(diags.iter().find(|d| d.code.as_deref() == Some("PL303")));
        assert!(pl303.suggestion.is_some(), "PL303 should include a resolution suggestion");
    }

    #[test]
    fn moose_role_conflict_also_fires_pl303() {
        let source = r#"
package MyRole::A;
use Moose::Role;
sub serialize { 1 }

package MyRole::B;
use Moose::Role;
sub serialize { 1 }

package MyClass;
use Moose;
with 'MyRole::A', 'MyRole::B';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            has_code(&diags, "PL303"),
            "Moose::Role conflict should also fire PL303: {diags:?}"
        );
    }

    // ─�� Cross-file role detection tests ──

    #[test]
    fn cross_file_role_conflict_fires_pl303() {
        // MyClass consumes two roles defined in other files; the workspace
        // queries supply the method names for those external roles.
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
"#;
        let queries = MockRoleQueries::new()
            .with_role_methods("Ext::RoleA", &["process"])
            .with_role_methods("Ext::RoleB", &["process"]);

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(has_code(&diags, "PL303"), "cross-file role conflict should fire PL303: {diags:?}");
    }

    #[test]
    fn cross_file_role_no_conflict_no_pl303() {
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
"#;
        let queries = MockRoleQueries::new()
            .with_role_methods("Ext::RoleA", &["method_a"])
            .with_role_methods("Ext::RoleB", &["method_b"]);

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(
            !has_code(&diags, "PL303"),
            "non-overlapping cross-file roles should not fire PL303: {diags:?}"
        );
    }

    #[test]
    fn cross_file_role_unresolved_is_skipped_conservatively() {
        // Ext::RoleB is unknown — should not guess a conflict.
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
"#;
        let queries = MockRoleQueries::new().with_role_methods("Ext::RoleA", &["process"]);
        // Ext::RoleB returns empty → unknown → skipped conservatively.

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(
            !has_code(&diags, "PL303"),
            "unresolved cross-file role should not generate a guessed conflict: {diags:?}"
        );
    }

    #[test]
    fn class_method_suppresses_cross_file_conflict() {
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
sub process { "mine" }
"#;
        let queries = MockRoleQueries::new()
            .with_role_methods("Ext::RoleA", &["process"])
            .with_role_methods("Ext::RoleB", &["process"]);

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(
            !has_code(&diags, "PL303"),
            "class-defined method should suppress cross-file conflict: {diags:?}"
        );
    }

    #[test]
    fn mixed_same_file_and_cross_file_roles_detect_conflict() {
        // RoleLocal is defined in the same file; Ext::RoleExternal is cross-file.
        let source = r#"
package RoleLocal;
use Moo::Role;
sub run { 1 }

package MyClass;
use Moo;
with 'RoleLocal', 'Ext::RoleExternal';
"#;
        let queries = MockRoleQueries::new().with_role_methods("Ext::RoleExternal", &["run"]);

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(
            has_code(&diags, "PL303"),
            "mixed same-file and cross-file role conflict should fire PL303: {diags:?}"
        );
    }

    // ── Transitive role composition tests ──

    #[test]
    fn transitive_same_file_role_conflict_fires_pl303() {
        // RoleA composes RoleBase (same file); RoleB also provides base_method.
        let source = r#"
package RoleBase;
use Moo::Role;
sub base_method { "base" }

package RoleA;
use Moo::Role;
with 'RoleBase';

package RoleB;
use Moo::Role;
sub base_method { "B" }

package MyClass;
use Moo;
with 'RoleA', 'RoleB';
"#;
        let diags = role_conflict_diags(source);
        assert!(
            has_code(&diags, "PL303"),
            "transitive same-file conflict through role composition should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn transitive_cross_file_role_conflict_fires_pl303() {
        // MyClass consumes Ext::RoleA which itself transitively composes Ext::RoleBase.
        // Ext::RoleB directly provides the same method as Ext::RoleBase.
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
"#;
        let queries = MockRoleQueries::new()
            // RoleA provides no methods directly but composes RoleBase.
            .with_role_methods("Ext::RoleA", &[])
            .with_role_compositions("Ext::RoleA", &["Ext::RoleBase"])
            .with_role_methods("Ext::RoleBase", &["shared"])
            .with_role_methods("Ext::RoleB", &["shared"]);

        let diags = role_conflict_diags_with_queries(source, &queries);
        assert!(
            has_code(&diags, "PL303"),
            "transitive cross-file role conflict should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn cyclic_role_composition_does_not_hang() {
        // RoleA and RoleB form a cycle in their compositions.
        let source = r#"
package MyClass;
use Moo;
with 'Ext::RoleA', 'Ext::RoleB';
"#;
        let queries = MockRoleQueries::new()
            .with_role_methods("Ext::RoleA", &["run"])
            .with_role_compositions("Ext::RoleA", &["Ext::RoleB"])
            .with_role_methods("Ext::RoleB", &["run"])
            .with_role_compositions("Ext::RoleB", &["Ext::RoleA"]);

        // This must not hang or panic — cycle protection ensures termination.
        let diags = role_conflict_diags_with_queries(source, &queries);
        // run is provided by both RoleA and RoleB → conflict.
        assert!(
            has_code(&diags, "PL303"),
            "cyclic composition should still detect method conflict and terminate: {diags:?}"
        );
    }
}
