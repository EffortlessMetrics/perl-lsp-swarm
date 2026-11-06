//! Dancer2 template-target extraction (#8937).
//!
//! Extracts the statically supported Dancer2 `template` call grammar from an
//! AST into [`Dancer2TemplateDeclaration`] carriers for the registry-activated
//! minting in `perl_semantic_facts::framework_adapters::dancer2_templates`.
//! This is pure source observation: extraction knows the reviewed template
//! call grammar, it does not decide activation — a template fact exists only
//! after the registry adapter minted it over an exact activation (#8914 seam).
//!
//! Supported form (reviewed Dancer2 1.x `Dancer2::Core::DSL::template`
//! profile — `template($name, $tokens, $opts)` with a required name):
//!
//! ```perl
//! template 'index';
//! template 'index', { foo => 1 };
//! template 'index', { foo => 1 }, { layout => 'main' };
//! get '/' => sub { template 'index' };
//! ```
//!
//! Name operands are static literals; a computed name (`template $name`)
//! is a dynamic boundary the minter degrades. Extra operands (tokens/options)
//! are admitted and not interpreted. Malformed arities (no name operand)
//! mint nothing.
//!
//! Unlike hook/route *declarations*, template *references* commonly live
//! inside route-handler bodies, so this walk **does** descend into
//! subroutine bodies. Package scoping still mirrors the #8914 activation
//! walk: an unqualified file defaults to `main`, bare `package X;` switches
//! the current package for following statements, and a lexical block
//! restores the enclosing package state afterwards.

use crate::analysis::dancer2_routes::interpolated_value_is_dynamic;
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2_templates::{
    DANCER2_TEMPLATE_KEYWORD, Dancer2TemplateDeclaration,
};
use perl_semantic_facts::template::{
    TemplateFilesystemTarget, TemplateName, TemplateNameSelection, TemplateReference,
};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};

/// Extract every supported Dancer2 template-target call from `ast`, in source
/// order, with per-call package/file identity and a source-order index.
///
/// The auto-quoted/computed distinction for a bareword operand
/// (`template index => {}` versus `template(index)`) is decided by the
/// parser, which quotes the operand left of a fat comma as Perl does. This
/// function therefore reads only `ast`: a literal operand arrives as a string
/// node, and anything else stays a computed boundary.
#[must_use]
pub fn extract_dancer2_template_calls(
    ast: &Node,
    file_id: FileId,
) -> Vec<Dancer2TemplateDeclaration> {
    let mut declarations = Vec::new();
    let mut current_package: Option<String> = Some("main".to_string());
    let mut next_index: u32 = 0;
    walk_node(ast, file_id, &mut current_package, &mut declarations, &mut next_index);
    declarations
}

fn walk_node(
    node: &Node,
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2TemplateDeclaration>,
    next_index: &mut u32,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            let mut block_package = current_package.clone();
            walk_statements(statements, file_id, &mut block_package, declarations, next_index);
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut package_scope = Some(name.clone());
            if let NodeKind::Block { statements } = &block.kind {
                walk_statements(statements, file_id, &mut package_scope, declarations, next_index);
            }
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = Some(name.clone());
        }
        NodeKind::FunctionCall { name, .. } if name == DANCER2_TEMPLATE_KEYWORD => {
            if let Some(declaration) =
                template_from_call(node, file_id, current_package, *next_index)
            {
                declarations.push(declaration);
                *next_index += 1;
            }
            for child in node.children() {
                walk_node(child, file_id, current_package, declarations, next_index);
            }
        }
        _ => {
            for child in node.children() {
                walk_node(child, file_id, current_package, declarations, next_index);
            }
        }
    }
}

fn walk_statements(
    statements: &[Node],
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2TemplateDeclaration>,
    next_index: &mut u32,
) {
    for statement in statements {
        walk_node(statement, file_id, current_package, declarations, next_index);
    }
}

/// Bind one `template NAME, ...` call into a declaration carrier.
///
/// The reviewed `template` keyword requires a name operand; extra token and
/// option operands are admitted and ignored. Zero-arity calls mint nothing.
fn template_from_call(
    expression: &Node,
    file_id: FileId,
    current_package: &Option<String>,
    declaration_index: u32,
) -> Option<Dancer2TemplateDeclaration> {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return None;
    };
    if name != DANCER2_TEMPLATE_KEYWORD {
        return None;
    }
    let name_node = args.first()?;
    let keyword_start = expression.location.start;
    Some(Dancer2TemplateDeclaration {
        package: current_package.clone(),
        file_id,
        declaration_start_byte: span_u32(keyword_start),
        declaration_end_byte: span_u32(expression.location.end),
        template: TemplateReference {
            declaration_index,
            keyword: name.clone(),
            keyword_anchor: anchor(keyword_start, keyword_start + name.len(), file_id),
            name: name_from_node(name_node, file_id),
            filesystem: TemplateFilesystemTarget::views_root_unresolved(),
        },
    })
}

fn name_from_node(node: &Node, file_id: FileId) -> TemplateNameSelection {
    let name_anchor = anchor(node.location.start, node.location.end, file_id);
    let NodeKind::String { value, interpolated } = &node.kind else {
        return TemplateNameSelection::Dynamic {
            reason: "computed template name operand".to_string(),
            anchor: name_anchor,
        };
    };
    if *interpolated && interpolated_value_is_dynamic(value) {
        return TemplateNameSelection::Dynamic {
            reason: "interpolated template name operand".to_string(),
            anchor: name_anchor,
        };
    }
    match unquote(value) {
        Some(logical) if logical.contains('\\') => TemplateNameSelection::Dynamic {
            reason: "escaped template name operand".to_string(),
            anchor: name_anchor,
        },
        Some(logical) => {
            TemplateNameSelection::Literal(TemplateName { logical, anchor: name_anchor })
        }
        None => TemplateNameSelection::Dynamic {
            reason: "empty template name operand".to_string(),
            anchor: name_anchor,
        },
    }
}

fn unquote(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| trimmed.strip_prefix('"').and_then(|value| value.strip_suffix('"')))
        .unwrap_or(trimmed);
    if stripped.is_empty() { None } else { Some(stripped.to_string()) }
}

fn span_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn anchor(start: usize, end: usize, file_id: FileId) -> SourceAnchor {
    SourceAnchor::new(Some(AnchorId(start as u64)), file_id, span_u32(start), span_u32(end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_semantic_facts::template::TemplateNameSelection;
    use perl_tdd_support::{must, must_some};

    fn declarations(code: &str) -> Vec<Dancer2TemplateDeclaration> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_template_calls(&ast, FileId(1))
    }

    fn logical_name(declaration: &Dancer2TemplateDeclaration) -> &str {
        must_some(declaration.template.name.logical())
    }

    #[test]
    fn literal_template_call_binds_exact_anchor_tokens() {
        let code = "template 'index';";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].template.keyword, "template");
        assert_eq!(logical_name(&found[0]), "index");
        assert!(!found[0].template.has_name_boundary());
        let name = must_some(match &found[0].template.name {
            TemplateNameSelection::Literal(name) => Some(name),
            TemplateNameSelection::Dynamic { .. } => None,
            _ => None,
        });
        assert_eq!(
            &code[name.anchor.start_byte as usize..name.anchor.end_byte as usize],
            "'index'"
        );
        let keyword = found[0].template.keyword_anchor;
        assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "template");
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(found[0].template.declaration_index, 0);
        assert_eq!(
            &code[found[0].declaration_start_byte as usize..found[0].declaration_end_byte as usize],
            "template 'index'"
        );
        assert!(matches!(
            found[0].template.filesystem,
            TemplateFilesystemTarget::Unresolved { .. }
        ));
    }

    #[test]
    fn extra_token_and_option_operands_are_admitted() {
        let found = declarations("template 'index', { foo => 1 }, { layout => 'main' };");
        assert_eq!(found.len(), 1);
        assert_eq!(logical_name(&found[0]), "index");
    }

    #[test]
    fn template_calls_inside_route_handlers_are_extracted() {
        let code = "get '/' => sub { template 'index' };";
        let found = declarations(code);
        assert_eq!(found.len(), 1, "handler-body template calls are the primary form");
        assert_eq!(logical_name(&found[0]), "index");
    }

    #[test]
    fn computed_and_dynamic_operands_are_boundaries() {
        let found = declarations("template $name;");
        assert_eq!(found.len(), 1);
        assert!(matches!(&found[0].template.name, TemplateNameSelection::Dynamic { .. }));

        let found = declarations("template \"page_$id\";");
        assert_eq!(found.len(), 1);
        assert!(matches!(&found[0].template.name, TemplateNameSelection::Dynamic { .. }));
    }

    #[test]
    fn malformed_arities_mint_nothing() {
        for code in ["template;", "template();"] {
            assert!(declarations(code).is_empty(), "`{code}` must not mint a template");
        }
    }

    #[test]
    fn fat_comma_barewords_are_literal_names() {
        let found = declarations("template index => {};");
        assert_eq!(found.len(), 1);
        assert_eq!(logical_name(&found[0]), "index");
    }

    #[test]
    fn comma_separated_bareword_operand_is_never_promoted() {
        let found = declarations("template(index);");
        assert_eq!(found.len(), 1);
        assert!(
            matches!(&found[0].template.name, TemplateNameSelection::Dynamic { .. }),
            "a comma-separated bareword calls a sub; it is not an auto-quoted name"
        );
    }

    #[test]
    fn templates_are_package_scoped_and_main_defaulted() {
        let found = declarations(
            "template 'a';\npackage App;\ntemplate 'b';\n{ package Inner; template 'c'; }\ntemplate 'd';\n",
        );
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(found[1].package.as_deref(), Some("App"));
        assert_eq!(found[2].package.as_deref(), Some("Inner"));
        assert_eq!(found[3].package.as_deref(), Some("App"), "block package state restored");
        for (index, declaration) in found.iter().enumerate() {
            assert_eq!(declaration.template.declaration_index, index as u32);
        }
    }

    #[test]
    fn unrelated_template_shaped_calls_mint_nothing() {
        assert!(declarations("templates 'index';").is_empty());
        assert!(declarations("my $x = template;\nprint $x;").is_empty());
    }

    #[test]
    fn same_logical_name_twice_stays_distinct_by_source_order() {
        let found = declarations("template 'index';\ntemplate 'index';");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].template.declaration_index, 0);
        assert_eq!(found[1].template.declaration_index, 1);
        assert_ne!(found[0].declaration_start_byte, found[1].declaration_start_byte);
    }
}
