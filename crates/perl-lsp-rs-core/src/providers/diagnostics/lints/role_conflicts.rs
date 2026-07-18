//! Moo/Moose/Role::Tiny role conflict diagnostics.
//!
//! This lint checks for roles consumed by a class that provide overlapping
//! method names. Roles defined in the same file are resolved from the local
//! [`ClassModel`]; roles defined in other files — and roles reached through
//! transitive composition — are resolved via the `resolve_role_methods`
//! callback, which is backed by the workspace semantic index in production.
//!
//! An unresolved role (external, dynamically composed, or simply not indexed)
//! contributes no methods and therefore cannot create a conflict: the lint
//! stays conservative and never guesses.

use std::collections::{HashMap, HashSet};

use super::super::internal_types::Diagnostic;
use perl_diagnostics::codes::DiagnosticCode;
use perl_diagnostics::codes::DiagnosticSeverity;
use perl_parser_core::ast::Node;
use perl_semantic_analyzer::{
    class_model::{ClassModel, ClassModelBuilder},
    symbol::{SymbolKind, SymbolTable},
};

/// Check for Moo/Moose/Role::Tiny role method conflicts.
///
/// `resolve_role_methods` maps a role package name to the method names it
/// provides transitively (including through composed roles). In production it
/// is backed by `SemanticQueries::transitive_role_methods`; it returns an empty
/// vec for roles that cannot be resolved, which keeps detection conservative.
/// Pass a closure returning `Vec::new()` to restrict the lint to same-file
/// analysis (e.g. when no workspace index is available).
pub fn check_role_conflicts(
    node: &Node,
    symbol_table: &SymbolTable,
    resolve_role_methods: &dyn Fn(&str) -> Vec<String>,
    diagnostics: &mut Vec<Diagnostic>,
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
        let mut seen_roles = HashSet::new();

        for role_name in &class_model.roles {
            if !seen_roles.insert(role_name.clone()) {
                continue;
            }

            // Same-file roles resolve from the local ClassModel (works with no
            // workspace index). Cross-file and transitively-composed roles
            // resolve via the workspace-backed callback. Union both sources; an
            // unresolved role simply contributes no methods.
            let mut method_names: HashSet<String> =
                role_models.get(role_name).map(provided_method_names).unwrap_or_default();
            method_names.extend(resolve_role_methods(role_name));

            for method_name in method_names {
                method_providers.entry(method_name).or_default().push(role_name.clone());
            }
        }

        for (method_name, providers) in method_providers {
            if providers.len() < 2 || class_methods.contains(&method_name) {
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
    use perl_tdd_support::{must, must_some};

    /// Same-file analysis only: the workspace resolver returns no methods, so
    /// only roles defined in `source` participate.
    fn role_conflict_diags(source: &str) -> Vec<Diagnostic> {
        role_conflict_diags_with_resolver(source, &|_| Vec::new())
    }

    /// Analysis with a caller-supplied role→methods resolver, simulating the
    /// workspace-backed `SemanticQueries::transitive_role_methods` for roles
    /// that live outside `source`.
    fn role_conflict_diags_with_resolver(
        source: &str,
        resolve_role_methods: &dyn Fn(&str) -> Vec<String>,
    ) -> Vec<Diagnostic> {
        let ast = must(Parser::new(source).parse());
        let symbol_table = SymbolExtractor::new_with_source(source).extract(&ast);
        let mut diagnostics = Vec::new();
        check_role_conflicts(&ast, &symbol_table, resolve_role_methods, &mut diagnostics);
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

    // ── Cross-file / transitive resolution (workspace-backed resolver) ──

    /// Build a resolver from a fixed role→methods map, mimicking the workspace
    /// index returning transitive method sets for roles defined elsewhere.
    fn map_resolver(entries: &[(&str, &[&str])]) -> impl Fn(&str) -> Vec<String> {
        let map: HashMap<String, Vec<String>> = entries
            .iter()
            .map(|(role, methods)| {
                (role.to_string(), methods.iter().map(|m| m.to_string()).collect())
            })
            .collect();
        move |role: &str| map.get(role).cloned().unwrap_or_default()
    }

    #[test]
    fn cross_file_roles_with_same_method_fire_pl303() {
        // Neither role is defined in this file; both are resolved via the
        // workspace resolver, and both provide `greet`.
        let source = r#"
package MyClass;
use Moo;
with 'RemoteRole::Greet', 'RemoteRole::Welcome';
"#;
        let resolver =
            map_resolver(&[("RemoteRole::Greet", &["greet"]), ("RemoteRole::Welcome", &["greet"])]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            has_code(&diags, "PL303"),
            "cross-file roles both providing `greet` should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn cross_file_roles_non_overlapping_no_pl303() {
        let source = r#"
package MyClass;
use Moo;
with 'RemoteRole::Greet', 'RemoteRole::Farewell';
"#;
        let resolver = map_resolver(&[
            ("RemoteRole::Greet", &["greet"]),
            ("RemoteRole::Farewell", &["farewell"]),
        ]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            !has_code(&diags, "PL303"),
            "cross-file roles with disjoint methods should not fire PL303: {diags:?}"
        );
    }

    #[test]
    fn cross_file_conflict_suppressed_when_class_defines_method() {
        let source = r#"
package MyClass;
use Moo;
with 'RemoteRole::Greet', 'RemoteRole::Welcome';
sub greet { "mine" }
"#;
        let resolver =
            map_resolver(&[("RemoteRole::Greet", &["greet"]), ("RemoteRole::Welcome", &["greet"])]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            !has_code(&diags, "PL303"),
            "class defining `greet` should suppress the cross-file conflict: {diags:?}"
        );
    }

    #[test]
    fn mixed_same_file_and_cross_file_roles_conflict() {
        // MyRole::Local is defined in-file; RemoteRole::Greet is cross-file.
        // Both provide `greet`.
        let source = r#"
package MyRole::Local;
use Moo::Role;
sub greet { "local" }

package MyClass;
use Moo;
with 'MyRole::Local', 'RemoteRole::Greet';
"#;
        let resolver = map_resolver(&[("RemoteRole::Greet", &["greet"])]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            has_code(&diags, "PL303"),
            "same-file role and cross-file role both providing `greet` should conflict: {diags:?}"
        );
    }

    #[test]
    fn transitive_role_methods_participate_in_conflict() {
        // The resolver already returns the *transitive* method set for each
        // role (that traversal happens in the workspace layer). RemoteRole::A
        // transitively provides `run` via a composed role; RemoteRole::B
        // provides `run` directly.
        let source = r#"
package MyClass;
use Moo;
with 'RemoteRole::A', 'RemoteRole::B';
"#;
        let resolver = map_resolver(&[
            ("RemoteRole::A", &["run"]), // contributed through transitive composition
            ("RemoteRole::B", &["run"]),
        ]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            has_code(&diags, "PL303"),
            "transitively-provided overlapping method should fire PL303: {diags:?}"
        );
    }

    #[test]
    fn unresolved_cross_file_roles_stay_conservative() {
        // Two roles consumed but neither can be resolved (resolver returns
        // empty, e.g. external/dynamic roles). No conflict may be guessed.
        let source = r#"
package MyClass;
use Moo;
with 'External::Unknown::A', 'External::Unknown::B';
"#;
        let diags = role_conflict_diags_with_resolver(source, &|_| Vec::new());
        assert!(
            !has_code(&diags, "PL303"),
            "unresolved external roles must not produce a guessed conflict: {diags:?}"
        );
    }

    #[test]
    fn single_cross_file_provider_no_conflict() {
        // Only one of the two consumed roles provides `run`; not a conflict.
        let source = r#"
package MyClass;
use Moo;
with 'RemoteRole::A', 'RemoteRole::B';
"#;
        let resolver = map_resolver(&[("RemoteRole::A", &["run"])]);
        let diags = role_conflict_diags_with_resolver(source, &resolver);
        assert!(
            !has_code(&diags, "PL303"),
            "a method provided by only one role is not a conflict: {diags:?}"
        );
    }
}
