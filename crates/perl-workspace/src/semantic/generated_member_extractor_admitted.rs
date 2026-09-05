//! Canonical generated-member publication gate.
//!
//! The historical extractor still contains permissive DBIx::Class recognition
//! based on raw source spelling. DBIx::Class is registered as a shadow adapter
//! with no provider surfaces, so those compatibility facts are not publication
//! authority. This wrapper preserves the legacy implementation as a comparison
//! oracle while removing only facts anchored inside its active DBIx::Class DSL
//! calls from the canonical path.
//!
//! The activation vocabulary (`is_dbix_class_module`,
//! `use_args_include_dbix_class`), the emitting method set
//! (`is_dbix_class_member_method`), and the package-target rule
//! (`package_target_matches`) are imported from the legacy module rather than
//! restated here, so the quarantine denominator is the producer's own.
//!
//! Remove this quarantine only through #13979 after #9736/#9739/#9741 publish
//! the equivalent admitted facts and the matching provider surfaces prove
//! same-request parity.

pub(crate) use super::legacy_generated_member_extractor::GeneratedMemberFact;
use super::legacy_generated_member_extractor::{
    self, is_dbix_class_member_method, is_dbix_class_module, package_target_matches,
    use_args_include_dbix_class,
};
use crate::{Node, NodeKind};
use perl_semantic_facts::FileId;

#[derive(Debug, Clone, Default)]
struct WalkCtx {
    current_package: Option<String>,
    dbix_class_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuarantinedRange {
    start: usize,
    end: usize,
}

impl QuarantinedRange {
    fn contains(self, start: u32, end: u32) -> bool {
        self.start <= start as usize && end as usize <= self.end
    }
}

/// Extract generated-member facts admitted to the canonical workspace shard.
pub(crate) fn extract_generated_member_facts(
    ast: &Node,
    file_id: FileId,
) -> Vec<GeneratedMemberFact> {
    let mut facts = legacy_generated_member_extractor::extract_generated_member_facts(ast, file_id);
    let mut quarantined = Vec::new();
    collect_quarantined_dbix_ranges(ast, &mut WalkCtx::default(), &mut quarantined);

    facts.retain(|fact| {
        !quarantined
            .iter()
            .any(|range| range.contains(fact.anchor.span_start_byte, fact.anchor.span_end_byte))
    });
    facts
}

/// Mirror the legacy walk's package and DBIx::Class activation boundaries so
/// every statement from which the legacy extractor can emit a DBIx::Class fact
/// is recorded as a quarantined source range.
fn collect_quarantined_dbix_ranges(
    node: &Node,
    ctx: &mut WalkCtx,
    out: &mut Vec<QuarantinedRange>,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            for statement in statements {
                collect_quarantined_dbix_ranges(statement, ctx, out);
            }
        }
        NodeKind::Package { name, block, .. } => {
            if let Some(block) = block {
                let saved = ctx.clone();
                ctx.current_package = Some(name.clone());
                ctx.dbix_class_active = false;
                collect_quarantined_dbix_ranges(block, ctx, out);
                *ctx = saved;
            } else {
                ctx.current_package = Some(name.clone());
                ctx.dbix_class_active = false;
            }
        }
        NodeKind::Use { module, .. } if is_dbix_class_module(module) => {
            ctx.dbix_class_active = true;
        }
        NodeKind::Use { module, args, .. }
            if (module == "base" || module == "parent") && use_args_include_dbix_class(args) =>
        {
            ctx.dbix_class_active = true;
        }
        NodeKind::No { module, .. } if is_dbix_class_module(module) => {
            ctx.dbix_class_active = false;
        }
        // Terminal arm, like the legacy walk: nested package-shaped children
        // inside an expression must not mutate this walk's package context.
        NodeKind::ExpressionStatement { expression } => {
            if ctx.dbix_class_active && is_dbix_class_emission_site(expression, ctx) {
                out.push(QuarantinedRange {
                    start: expression.location.start,
                    end: expression.location.end,
                });
            }
        }
        NodeKind::Subroutine { .. } | NodeKind::Method { .. } => {}
        _ => {
            for child in node.children() {
                collect_quarantined_dbix_ranges(child, ctx, out);
            }
        }
    }
}

fn is_dbix_class_emission_site(expression: &Node, ctx: &WalkCtx) -> bool {
    let NodeKind::MethodCall { object, method, .. } = &expression.kind else {
        return false;
    };
    let current_package = ctx.current_package.as_deref().unwrap_or("main");
    is_dbix_class_member_method(method) && package_target_matches(object, current_package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn parse(source: &str) -> Node {
        let mut parser = Parser::new(source);
        parser.parse_with_recovery().ast
    }

    fn names(facts: &[GeneratedMemberFact]) -> Vec<&str> {
        facts.iter().map(|fact| fact.entity.canonical_name.as_str()).collect()
    }

    #[test]
    fn legacy_dbix_columns_are_not_canonical_publication_authority() {
        let ast = parse(
            r#"
package MyApp::Schema::Result::User;
use DBIx::Class;
__PACKAGE__->add_columns(qw/id name email/);
1;
"#,
        );

        let legacy =
            legacy_generated_member_extractor::extract_generated_member_facts(&ast, FileId(1));
        assert!(names(&legacy).contains(&"MyApp::Schema::Result::User::id"));

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(admitted.is_empty(), "raw DBIx spelling must not publish generated members");
    }

    #[test]
    fn legacy_dbix_relationships_and_base_activation_are_quarantined() {
        let ast = parse(
            r#"
package MyApp::Schema::Result::Author;
use base 'DBIx::Class::Core';
__PACKAGE__->has_many('posts', 'MyApp::Schema::Result::Post', 'author_id');
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(
            !names(&admitted).contains(&"MyApp::Schema::Result::Author::posts"),
            "raw base inheritance must not authorize relationship publication"
        );
    }

    #[test]
    fn non_dbix_generated_members_remain_admitted() {
        let ast = parse(
            r#"
package MyApp::User;
use Moo;
has 'name' => (is => 'ro');
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        assert!(names(&admitted).contains(&"MyApp::User::name"));
    }

    #[test]
    fn supported_accessor_frameworks_remain_admitted() {
        for (framework, source, expected_name) in [
            (
                "Moo",
                "package MyApp::Moo; use Moo; has 'name' => (is => 'ro'); 1;",
                "MyApp::Moo::name",
            ),
            (
                "Moose",
                "package MyApp::Moose; use Moose; has 'name' => (is => 'ro'); 1;",
                "MyApp::Moose::name",
            ),
            (
                "Mouse",
                "package MyApp::Mouse; use Mouse; has 'name' => (is => 'ro'); 1;",
                "MyApp::Mouse::name",
            ),
            (
                "Class::Tiny",
                "package MyApp::ClassTiny; use Class::Tiny; has 'name'; 1;",
                "MyApp::ClassTiny::name",
            ),
        ] {
            let admitted = extract_generated_member_facts(&parse(source), FileId(1));
            assert!(
                names(&admitted).contains(&expected_name),
                "{framework} generated member should remain admitted"
            );
        }
    }

    #[test]
    fn nested_package_expression_does_not_leak_context_into_dbix_quarantine() {
        let ast = parse(
            r#"
package MyApp::Schema::Result::User;
do { package Nested::Scope; };
use DBIx::Class;
MyApp::Schema::Result::User->add_columns(qw/id/);
1;
"#,
        );

        let legacy =
            legacy_generated_member_extractor::extract_generated_member_facts(&ast, FileId(1));
        assert!(names(&legacy).contains(&"MyApp::Schema::Result::User::id"));
        assert!(extract_generated_member_facts(&ast, FileId(1)).is_empty());
    }

    #[test]
    fn mixed_package_preserves_moo_member_and_quarantines_dbix_member() {
        let ast = parse(
            r#"
package MyApp::User;
use Moo;
use DBIx::Class::Core;
has 'name' => (is => 'ro');
__PACKAGE__->add_columns(qw/id/);
1;
"#,
        );

        let admitted = extract_generated_member_facts(&ast, FileId(1));
        let admitted_names = names(&admitted);
        assert!(admitted_names.contains(&"MyApp::User::name"));
        assert!(!admitted_names.contains(&"MyApp::User::id"));
    }

    #[test]
    fn same_named_dsl_without_dbix_activation_remains_non_dbix() {
        let ast = parse(
            r#"
package Plain::Package;
__PACKAGE__->add_columns(qw/id/);
__PACKAGE__->has_many('children', 'Plain::Child', 'parent_id');
1;
"#,
        );

        assert!(extract_generated_member_facts(&ast, FileId(1)).is_empty());
    }

    #[test]
    fn every_legacy_dbix_emission_method_is_quarantined() {
        // Each method the shared classifier admits must both emit through the
        // legacy oracle and be removed by the canonical gate. The loop walks
        // `DBIX_CLASS_MEMBER_METHODS` itself rather than restating the names, so
        // a method added to the denominator is covered here the moment it is
        // added; a restated list would be a third denominator free to drift.
        // A dead entry fails the legacy assertion, and a method the gate stops
        // removing fails the canonical one.
        for method in legacy_generated_member_extractor::DBIX_CLASS_MEMBER_METHODS {
            let call = if *method == "add_columns" {
                "__PACKAGE__->add_columns(qw/member/);".to_string()
            } else {
                format!("__PACKAGE__->{method}('member', 'Other::Class', 'fk');")
            };
            assert!(is_dbix_class_member_method(method));
            let ast =
                parse(&format!("package Coupled::Result;\nuse DBIx::Class::Core;\n{call}\n1;\n"));
            let legacy =
                legacy_generated_member_extractor::extract_generated_member_facts(&ast, FileId(1));
            assert!(
                names(&legacy).contains(&"Coupled::Result::member"),
                "{method}: legacy oracle must still emit the compatibility row"
            );
            assert!(
                extract_generated_member_facts(&ast, FileId(1)).is_empty(),
                "{method}: canonical gate must quarantine the legacy row"
            );
        }
        assert!(!is_dbix_class_member_method("table"));
        assert!(!is_dbix_class_member_method("has"));
    }
}
