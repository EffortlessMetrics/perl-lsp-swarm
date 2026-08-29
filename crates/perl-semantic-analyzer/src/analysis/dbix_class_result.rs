//! Static DBIx::Class result-class/result-source source extraction (#9736).
//!
//! This module extracts only the source evidence admitted by the checked
//! DBIx::Class shadow profile: exact `use base`/`use parent` activation of
//! `DBIx::Class::Core` and static current-package `table(...)` declarations.
//! It does not extract columns, relationships, keys, ResultSet types, or
//! provider behavior, and it never executes Perl or consults a database.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dbix_class::{
    DBIX_CLASS_CORE_MODULE, DbixClassInheritanceEvidence, DbixClassInheritanceForm,
    DbixResultSiteAnchor, DbixTableEvidence,
};
use perl_semantic_facts::{AnchorId, FileId, SourceGeneration};
use std::collections::BTreeMap;

/// One package-local DBIx::Class result profile candidate extracted from
/// source. The checked adapter decides whether this evidence is current and
/// exact enough to mint identities.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbixClassResultSite {
    /// Load-bearing package/source identity.
    pub anchor: DbixResultSiteAnchor,
    /// Static, dynamic, unsupported, or absent inheritance evidence.
    pub inheritance: DbixClassInheritanceEvidence,
    /// Static, dynamic, recovered, or absent table/source evidence.
    pub table: DbixTableEvidence,
}

#[derive(Debug, Clone)]
struct PartialResultSite {
    file_id: FileId,
    package: String,
    activation_anchor_id: Option<AnchorId>,
    activation_range: Option<(u32, u32)>,
    inheritance: DbixClassInheritanceEvidence,
    table: DbixTableEvidence,
    saw_inheritance: bool,
    saw_table: bool,
}

impl PartialResultSite {
    fn new(file_id: FileId, package: String) -> Self {
        Self {
            file_id,
            package,
            activation_anchor_id: None,
            activation_range: None,
            inheritance: DbixClassInheritanceEvidence::Missing,
            table: DbixTableEvidence::Missing,
            saw_inheritance: false,
            saw_table: false,
        }
    }

    fn record_inheritance(
        &mut self,
        evidence: DbixClassInheritanceEvidence,
        anchor_id: AnchorId,
        range: (u32, u32),
    ) {
        if self.saw_inheritance {
            self.inheritance = DbixClassInheritanceEvidence::Unsupported {
                reason: "multiple DBIx::Class-relevant inheritance declarations are outside the \
                         reviewed single-activation profile"
                    .to_string(),
            };
            return;
        }
        self.saw_inheritance = true;
        self.activation_anchor_id = Some(anchor_id);
        self.activation_range = Some(range);
        self.inheritance = evidence;
    }

    fn record_table(&mut self, evidence: DbixTableEvidence) {
        if self.saw_table {
            self.table = DbixTableEvidence::Recovered {
                reason: "multiple current-package table declarations cannot establish one static \
                         result-source identity"
                    .to_string(),
            };
            return;
        }
        self.saw_table = true;
        self.table = evidence;
    }

    fn finish(self, generation: SourceGeneration) -> DbixClassResultSite {
        DbixClassResultSite {
            anchor: DbixResultSiteAnchor::new(
                self.file_id,
                Some(self.package),
                self.activation_anchor_id,
                self.activation_range,
                generation,
                None,
                None,
            ),
            inheritance: self.inheritance,
            table: self.table,
        }
    }
}

/// Extract DBIx::Class result-profile candidates in deterministic package order.
///
/// A same-named `table` method remains visible as a candidate with
/// [`DbixClassInheritanceEvidence::Missing`]; it cannot activate exact
/// framework semantics. Unrelated static `base`/`parent` declarations are
/// ignored, while dynamic or malformed parents remain explicit boundaries
/// because they could resolve to the reviewed module only at runtime.
///
/// Shadow status (#13140): no production consumer wires this extractor yet,
/// so it stays crate-visible and is exercised by its in-module tests until
/// the extraction and the checked adapter meet in a comparison consumer.
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn extract_dbix_class_result_sites(
    ast: &Node,
    file_id: FileId,
    generation: SourceGeneration,
) -> Vec<DbixClassResultSite> {
    let mut partials = BTreeMap::<String, PartialResultSite>::new();
    let mut current_package = "main".to_string();
    walk_result_sites(ast, file_id, &mut current_package, &mut partials);
    partials.into_values().map(|partial| partial.finish(generation.clone())).collect()
}

fn walk_result_sites(
    node: &Node,
    file_id: FileId,
    current_package: &mut String,
    partials: &mut BTreeMap<String, PartialResultSite>,
) {
    match &node.kind {
        NodeKind::Program { statements } => {
            for statement in statements {
                walk_result_sites(statement, file_id, current_package, partials);
            }
            return;
        }
        NodeKind::Block { statements } => {
            let mut block_package = current_package.clone();
            for statement in statements {
                walk_result_sites(statement, file_id, &mut block_package, partials);
            }
            return;
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut package_scope = name.clone();
            walk_result_sites(block, file_id, &mut package_scope, partials);
            return;
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = name.clone();
        }
        NodeKind::Use { module, args, .. } if matches!(module.as_str(), "base" | "parent") => {
            if let Some(evidence) = classify_inheritance(module, args) {
                let partial = partials
                    .entry(current_package.clone())
                    .or_insert_with(|| PartialResultSite::new(file_id, current_package.clone()));
                partial.record_inheritance(
                    evidence,
                    AnchorId(node.location.start as u64),
                    source_range(node),
                );
            }
        }
        NodeKind::ExpressionStatement { expression } => {
            if let Some(table) = table_evidence(expression, node, current_package) {
                let partial = partials
                    .entry(current_package.clone())
                    .or_insert_with(|| PartialResultSite::new(file_id, current_package.clone()));
                partial.record_table(table);
            }
        }
        _ => {}
    }

    for child in node.children() {
        walk_result_sites(child, file_id, current_package, partials);
    }
}

fn classify_inheritance(module: &str, args: &[String]) -> Option<DbixClassInheritanceEvidence> {
    let form = if module == "base" {
        DbixClassInheritanceForm::Base
    } else {
        DbixClassInheritanceForm::Parent
    };
    let tokens = static_parent_tokens(args);

    if tokens.is_empty() {
        return Some(DbixClassInheritanceEvidence::Recovered {
            reason: format!("`use {module}` lacks a recoverable parent spelling"),
        });
    }
    if let Some(token) = tokens.iter().find(|token| malformed_quote(token)) {
        return Some(DbixClassInheritanceEvidence::Recovered {
            reason: format!("unterminated or recovered parent spelling `{token}`"),
        });
    }
    if let Some(token) = tokens.iter().find(|token| dynamic_parent_token(token)) {
        return Some(DbixClassInheritanceEvidence::Dynamic {
            reason: format!("parent expression `{token}` is computed at runtime"),
        });
    }

    let parents: Vec<String> = tokens.iter().map(|token| unquote(token)).collect();
    let mentions_dbix = parents.iter().any(|parent| parent.starts_with("DBIx::Class"));
    if !mentions_dbix {
        return None;
    }
    if parents.len() == 1 && parents[0] == DBIX_CLASS_CORE_MODULE {
        return Some(DbixClassInheritanceEvidence::Exact {
            form,
            module: DBIX_CLASS_CORE_MODULE.to_string(),
        });
    }

    Some(DbixClassInheritanceEvidence::Unsupported {
        reason: format!(
            "static inheritance [{}] is outside the reviewed exact \
             `{DBIX_CLASS_CORE_MODULE}` profile",
            parents.join(", ")
        ),
    })
}

/// Collect candidate parent spellings from `use base`/`use parent` arguments.
/// The parser folds `qw(WORD ...)` lists into one `qw(...)` token; their
/// contents are static words, not runtime-computed parent expressions.
fn static_parent_tokens(args: &[String]) -> Vec<String> {
    let mut tokens = Vec::new();
    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() || matches!(arg, "," | "=>" | "(" | ")" | "-norequire") {
            continue;
        }
        if let Some(inner) = arg.strip_prefix("qw(").and_then(|rest| rest.strip_suffix(')')) {
            tokens.extend(inner.split_whitespace().map(str::to_string));
            continue;
        }
        tokens.push(arg.to_string());
    }
    tokens
}

fn table_evidence(
    expression: &Node,
    statement: &Node,
    current_package: &str,
) -> Option<DbixTableEvidence> {
    let NodeKind::MethodCall { object, method, args } = &expression.kind else {
        return None;
    };
    if method != "table" || !target_is_current_package(object, current_package) {
        return None;
    }

    let Some(argument) = args.first() else {
        return Some(DbixTableEvidence::Recovered {
            reason: "current-package `table` call has no source-name argument".to_string(),
        });
    };
    match &argument.kind {
        NodeKind::String { value, .. } => {
            // The parser stores raw token text, so a quoted spelling still
            // carries its delimiters here.
            let spelling = unquote(value);
            if spelling.contains('$') || spelling.contains('@') {
                Some(DbixTableEvidence::Dynamic {
                    reason: format!("table/source spelling `{spelling}` interpolates at runtime"),
                })
            } else if spelling.trim().is_empty() {
                Some(DbixTableEvidence::Recovered {
                    reason: "table/source spelling is empty".to_string(),
                })
            } else {
                Some(DbixTableEvidence::Static {
                    name: spelling.trim().to_string(),
                    anchor_id: AnchorId(statement.location.start as u64),
                    source_range: source_range(argument),
                })
            }
        }
        NodeKind::Identifier { name } if !name.trim().is_empty() => {
            Some(DbixTableEvidence::Static {
                name: name.trim().to_string(),
                anchor_id: AnchorId(statement.location.start as u64),
                source_range: source_range(argument),
            })
        }
        _ => Some(DbixTableEvidence::Dynamic {
            reason: "table/source argument is not a static string or bareword".to_string(),
        }),
    }
}

fn target_is_current_package(object: &Node, current_package: &str) -> bool {
    match &object.kind {
        NodeKind::Identifier { name } => name == "__PACKAGE__" || name == current_package,
        NodeKind::String { value, .. } => value == current_package,
        _ => false,
    }
}

fn malformed_quote(token: &str) -> bool {
    let Some(first) = token.chars().next() else {
        return true;
    };
    (first == '\'' || first == '"') && (token.len() < 2 || !token.ends_with(first))
}

fn dynamic_parent_token(token: &str) -> bool {
    token.starts_with('$')
        || token.starts_with('@')
        || token.starts_with('%')
        || token.starts_with('\\')
        || token.contains('(')
        || (token.starts_with('"') && (token.contains('$') || token.contains('@')))
}

fn unquote(token: &str) -> String {
    let bytes = token.as_bytes();
    if bytes.len() >= 2
        && ((bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
            || (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"'))
    {
        token[1..token.len() - 1].to_string()
    } else {
        token.to_string()
    }
}

fn source_range(node: &Node) -> (u32, u32) {
    (
        node.location.start.min(u32::MAX as usize) as u32,
        node.location.end.min(u32::MAX as usize) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn sites(code: &str) -> Vec<DbixClassResultSite> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dbix_class_result_sites(&ast, FileId(7), SourceGeneration::known("source-gen-1"))
    }

    fn site_for<'a>(found: &'a [DbixClassResultSite], package: &str) -> &'a DbixClassResultSite {
        must_some(found.iter().find(|site| site.anchor.package.as_deref() == Some(package)))
    }

    #[test]
    fn base_activation_and_static_table_are_retained() {
        let code = "package App::Schema::Result::User;\nuse base 'DBIx::Class::Core';\n__PACKAGE__->table('users');\n";
        let found = sites(code);
        let site = site_for(&found, "App::Schema::Result::User");
        assert!(matches!(
            site.inheritance,
            DbixClassInheritanceEvidence::Exact { form: DbixClassInheritanceForm::Base, .. }
        ));
        let DbixTableEvidence::Static { name, source_range, .. } = &site.table else {
            assert!(matches!(site.table, DbixTableEvidence::Static { .. }));
            return;
        };
        assert_eq!(name, "users");
        assert!(source_range.1 > source_range.0);
        assert_eq!(site.anchor.source_generation, SourceGeneration::known("source-gen-1"));
    }

    #[test]
    fn parent_norequire_form_is_admitted() {
        let found = sites(
            "package App::Schema::Result::User;\nuse parent -norequire, 'DBIx::Class::Core';\n__PACKAGE__->table('users');\n",
        );
        let site = site_for(&found, "App::Schema::Result::User");
        assert!(matches!(
            site.inheritance,
            DbixClassInheritanceEvidence::Exact { form: DbixClassInheritanceForm::Parent, .. }
        ));
    }

    #[test]
    fn folded_qw_word_list_is_a_static_exact_parent_form() {
        let found = sites(
            "package App::Schema::Result::User;\nuse base qw(DBIx::Class::Core);\n__PACKAGE__->table('users');\n",
        );
        let site = site_for(&found, "App::Schema::Result::User");
        assert!(matches!(
            site.inheritance,
            DbixClassInheritanceEvidence::Exact { form: DbixClassInheritanceForm::Base, .. }
        ));
    }

    #[test]
    fn quoted_and_padded_table_spellings_normalize_to_one_identity() {
        let double_quoted = sites(
            "package App::Schema::Result::User;\nuse base 'DBIx::Class::Core';\n__PACKAGE__->table(\"users\");\n",
        );
        let padded = sites(
            "package App::Schema::Result::User;\nuse base 'DBIx::Class::Core';\n__PACKAGE__->table(' users '); \n",
        );
        let site = site_for(&double_quoted, "App::Schema::Result::User");
        let DbixTableEvidence::Static { name, .. } = &site.table else {
            panic!("double-quoted table spelling must stay static");
        };
        assert_eq!(name, "users");
        let site = site_for(&padded, "App::Schema::Result::User");
        let DbixTableEvidence::Static { name, .. } = &site.table else {
            panic!("padded table spelling must stay static");
        };
        assert_eq!(name, "users");
    }

    #[test]
    fn same_named_table_without_activation_stays_non_framework_evidence() {
        let found = sites("package Local::Thing;\nsub table { }\n__PACKAGE__->table('users');\n");
        let site = site_for(&found, "Local::Thing");
        assert_eq!(site.inheritance, DbixClassInheritanceEvidence::Missing);
        assert!(matches!(site.table, DbixTableEvidence::Static { .. }));
    }

    #[test]
    fn bare_table_function_is_not_a_result_source_declaration() {
        assert!(sites("package Local::Thing;\ntable('users');\n").is_empty());
    }

    #[test]
    fn dynamic_parent_and_table_are_explicit_boundaries() {
        let parent =
            sites("package Dynamic::Parent;\nuse parent $base;\n__PACKAGE__->table('users');\n");
        assert!(matches!(
            site_for(&parent, "Dynamic::Parent").inheritance,
            DbixClassInheritanceEvidence::Dynamic { .. }
        ));

        let table = sites(
            "package Dynamic::Table;\nuse base 'DBIx::Class::Core';\n__PACKAGE__->table($table_name);\n",
        );
        assert!(matches!(
            site_for(&table, "Dynamic::Table").table,
            DbixTableEvidence::Dynamic { .. }
        ));
    }

    #[test]
    fn unrelated_static_parent_is_not_a_dbix_candidate() {
        assert!(sites("package App;\nuse parent 'App::Base';\n").is_empty());
    }

    #[test]
    fn packages_are_isolated_and_order_is_deterministic() {
        let found = sites(
            "package Zed;\nuse base 'DBIx::Class::Core';\n__PACKAGE__->table('same');\npackage Alpha;\nuse parent 'DBIx::Class::Core';\n__PACKAGE__->table('same');\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].anchor.package.as_deref(), Some("Alpha"));
        assert_eq!(found[1].anchor.package.as_deref(), Some("Zed"));
        assert_ne!(found[0].anchor.activation_anchor_id, found[1].anchor.activation_anchor_id);
    }

    #[test]
    fn lexical_package_scope_is_restored_after_block() {
        let found = sites(
            "package Outer;\n{ package Inner; use base 'DBIx::Class::Core'; __PACKAGE__->table('inner'); }\nuse parent 'DBIx::Class::Core';\n__PACKAGE__->table('outer');\n",
        );
        assert!(matches!(
            site_for(&found, "Inner").table,
            DbixTableEvidence::Static { ref name, .. } if name == "inner"
        ));
        assert!(matches!(
            site_for(&found, "Outer").table,
            DbixTableEvidence::Static { ref name, .. } if name == "outer"
        ));
    }
}
