//! Dancer2 hook-declaration extraction (#8924).
//!
//! Extracts the statically supported Dancer2 hook declaration grammar from an
//! AST into [`Dancer2HookDeclaration`] carriers for the registry-activated
//! minting in `perl_semantic_facts::framework_adapters::dancer2_hooks`. This
//! is pure source observation: extraction knows the reviewed hook grammar, it
//! does not decide activation — a hook fact exists only after the registry
//! adapter minted it over an exact activation (#8914 seam).
//!
//! Supported form (reviewed Dancer2 1.x `Dancer2::Core::DSL::hook` profile —
//! `hook($name, $code)` with a required CODE ref):
//!
//! ```perl
//! hook 'before' => sub { ... };
//! hook 'core.app.before_request' => \&on_request;
//! ```
//!
//! Name operands are static literals; a computed name (`hook $name => ...`)
//! is a dynamic boundary the minter degrades. Handler operands bind through
//! the shared handler contract (#8924 promotion): inline subs and statically
//! resolvable `\&name` coderefs are exact, everything else is a typed
//! boundary. Malformed arities mint nothing.
//!
//! Package scoping mirrors the #8914 activation walk: an unqualified file
//! defaults to `main`, bare `package X;` switches the current package for
//! following statements, and a lexical block restores the enclosing package
//! state afterwards. Hook calls inside subroutine bodies are
//! execution-conditional and mint nothing. Hook calls inside control flow
//! (`if`/`unless`/loops/`try`/statement modifiers, and the short-circuited
//! right operand of `&&`/`||`-style operators) register only when the
//! enclosing condition executes at load time: they are likewise
//! execution-conditional and mint nothing, while `package` statements inside
//! those blocks stay compile-time effective and keep being tracked.

use crate::analysis::dancer2_handler_targets::{SubroutineTargetIndex, handler_from_node};
use crate::analysis::dancer2_routes::interpolated_value_is_dynamic;
use crate::ast::{Node, NodeKind};
use perl_semantic_facts::framework_adapters::dancer2_hooks::{
    Dancer2HookDeclaration, normalize_dancer2_hook_name,
};
use perl_semantic_facts::hook::{HookDeclaration, HookName, HookNameSelection};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};

/// Extract every supported Dancer2 hook declaration from `ast`, in source
/// order, with per-declaration package/file identity and a source-order
/// declaration index.
#[must_use]
pub fn extract_dancer2_hook_declarations(
    ast: &Node,
    file_id: FileId,
) -> Vec<Dancer2HookDeclaration> {
    let targets = SubroutineTargetIndex::build(ast, file_id);
    let mut declarations = Vec::new();
    let mut current_package: Option<String> = Some("main".to_string());
    let mut next_index: u32 = 0;
    walk_node(
        ast,
        file_id,
        &mut current_package,
        &mut declarations,
        &mut next_index,
        &targets,
        false,
    );
    declarations
}

/// Whether a node makes its subtree's execution conditional: control flow
/// (condition bodies run only when the condition holds) and short-circuit
/// operators (the right operand runs only when the left does not shortcut).
/// A bare `do { ... }` block is *not* conditional: it executes whenever its
/// statement does (a `do { ... } while $c` modifier is caught by the
/// statement-modifier arm instead).
fn bounds_execution(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::If { .. }
            | NodeKind::While { .. }
            | NodeKind::For { .. }
            | NodeKind::Foreach { .. }
            | NodeKind::Defer { .. }
            | NodeKind::Try { .. }
            | NodeKind::Ternary { .. }
            | NodeKind::StatementModifier { .. }
    )
}

fn is_short_circuit(op: &str) -> bool {
    matches!(op, "&&" | "||" | "//" | "and" | "or")
}

#[allow(clippy::too_many_arguments)]
fn walk_node(
    node: &Node,
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2HookDeclaration>,
    next_index: &mut u32,
    targets: &SubroutineTargetIndex,
    conditional: bool,
) {
    match &node.kind {
        NodeKind::Program { statements } | NodeKind::Block { statements } => {
            // A lexical block scopes statement-form `package X;` declarations:
            // walk it with a block-local copy so the enclosing package state
            // is restored afterwards (mirrors the #8914 activation walk).
            let mut block_package = current_package.clone();
            walk_statements(
                statements,
                file_id,
                &mut block_package,
                declarations,
                next_index,
                targets,
                conditional,
            );
        }
        NodeKind::Package { name, block: Some(block), .. } => {
            let mut package_scope = Some(name.clone());
            if let NodeKind::Block { statements } = &block.kind {
                walk_statements(
                    statements,
                    file_id,
                    &mut package_scope,
                    declarations,
                    next_index,
                    targets,
                    conditional,
                );
            }
        }
        NodeKind::Package { name, block: None, .. } => {
            *current_package = Some(name.clone());
        }
        // Hook calls inside a subroutine body register only when that sub
        // executes — statically execution-conditional, never a load-time
        // declaration. Do not descend.
        NodeKind::Subroutine { .. } => {}
        NodeKind::Binary { op, left, right } if is_short_circuit(op) => {
            // The left operand executes unconditionally; the right operand is
            // short-circuited by the left's truthiness.
            walk_node(
                left,
                file_id,
                current_package,
                declarations,
                next_index,
                targets,
                conditional,
            );
            walk_node(right, file_id, current_package, declarations, next_index, targets, true);
        }
        _ => {
            let subtree_is_conditional = conditional || bounds_execution(&node.kind);
            for child in node.children() {
                walk_node(
                    child,
                    file_id,
                    current_package,
                    declarations,
                    next_index,
                    targets,
                    subtree_is_conditional,
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_statements(
    statements: &[Node],
    file_id: FileId,
    current_package: &mut Option<String>,
    declarations: &mut Vec<Dancer2HookDeclaration>,
    next_index: &mut u32,
    targets: &SubroutineTargetIndex,
    conditional: bool,
) {
    for statement in statements {
        if let NodeKind::ExpressionStatement { expression } = &statement.kind
            && !conditional
            && let Some(declaration) =
                hook_from_expression(expression, file_id, current_package, *next_index, targets)
        {
            declarations.push(declaration);
            *next_index += 1;
        } else {
            walk_node(
                statement,
                file_id,
                current_package,
                declarations,
                next_index,
                targets,
                conditional,
            );
        }
    }
}

/// Bind one `hook NAME, CODE` call into a declaration carrier.
///
/// The reviewed `hook` keyword takes exactly a name operand and a code
/// operand; any other arity is malformed and mints nothing.
fn hook_from_expression(
    expression: &Node,
    file_id: FileId,
    current_package: &Option<String>,
    declaration_index: u32,
    targets: &SubroutineTargetIndex,
) -> Option<Dancer2HookDeclaration> {
    let NodeKind::FunctionCall { name, args } = &expression.kind else {
        return None;
    };
    if name != "hook" {
        return None;
    }
    let [name_node, code_node] = args.as_slice() else {
        return None;
    };
    let keyword_start = expression.location.start;
    Some(Dancer2HookDeclaration {
        package: current_package.clone(),
        file_id,
        declaration_start_byte: span_u32(keyword_start),
        declaration_end_byte: span_u32(expression.location.end),
        hook: HookDeclaration {
            declaration_index,
            keyword: name.clone(),
            keyword_anchor: anchor(keyword_start, keyword_start + name.len(), file_id),
            name: name_from_node(name_node, file_id),
            handler: handler_from_node(code_node, file_id, current_package.as_deref(), targets),
        },
    })
}

fn name_from_node(node: &Node, file_id: FileId) -> HookNameSelection {
    let name_anchor = anchor(node.location.start, node.location.end, file_id);
    // `hook before => sub { ... }` is the canonical Dancer2 spelling. Perl's
    // fat comma auto-quotes the bareword immediately before it, so this
    // operand is a *literal* hook name, not a computed one — the parser
    // surfaces it as a bare `Identifier`. A genuinely computed operand stays
    // dynamic: a variable is `Variable` and a call is `FunctionCall`, and
    // neither reaches this arm. (`hook before, sub {...}` — no fat comma, so
    // no auto-quoting — does not parse as a `hook` call at all, so it cannot
    // arrive here either.)
    if let NodeKind::Identifier { name } = &node.kind {
        return HookNameSelection::Literal(HookName {
            normalization: normalize_dancer2_hook_name(name),
            literal: name.clone(),
            anchor: name_anchor,
        });
    }
    let NodeKind::String { value, interpolated } = &node.kind else {
        return HookNameSelection::Dynamic {
            reason: "computed hook name operand".to_string(),
            anchor: name_anchor,
        };
    };
    if *interpolated && interpolated_value_is_dynamic(value) {
        return HookNameSelection::Dynamic {
            reason: "interpolated hook name operand".to_string(),
            anchor: name_anchor,
        };
    }
    match unquote(value) {
        Some(literal) => HookNameSelection::Literal(HookName {
            normalization: normalize_dancer2_hook_name(&literal),
            literal,
            anchor: name_anchor,
        }),
        None => HookNameSelection::Dynamic {
            reason: "empty hook name operand".to_string(),
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
    use perl_semantic_facts::handler::{FrameworkHandler, FrameworkHandlerBoundary};
    use perl_semantic_facts::hook::HookNameNormalization;
    use perl_tdd_support::{must, must_some};

    fn declarations(code: &str) -> Vec<Dancer2HookDeclaration> {
        let mut parser = Parser::new(code);
        let ast = must(parser.parse());
        extract_dancer2_hook_declarations(&ast, FileId(1))
    }

    fn literal_name(declaration: &Dancer2HookDeclaration) -> &HookName {
        must_some(match &declaration.hook.name {
            HookNameSelection::Literal(name) => Some(name),
            HookNameSelection::Dynamic { .. } => None,
            _ => None,
        })
    }

    #[test]
    fn canonical_application_hook_binds_exact_anchor_tokens() {
        let code = "hook 'core.app.before_request' => sub { 1 };";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        let hook = &found[0].hook;
        assert_eq!(hook.keyword, "hook");
        let name = literal_name(&found[0]);
        assert_eq!(name.literal, "core.app.before_request");
        assert_eq!(name.canonical(), Some("core.app.before_request"));
        assert_eq!(name.normalization, HookNameNormalization::Canonical);
        assert!(matches!(hook.handler, FrameworkHandler::InlineSub { .. }), "inline handler");
        assert!(!hook.has_boundary());
        assert_eq!(
            &code[name.anchor.start_byte as usize..name.anchor.end_byte as usize],
            "'core.app.before_request'"
        );
        let keyword = hook.keyword_anchor;
        assert_eq!(&code[keyword.start_byte as usize..keyword.end_byte as usize], "hook");
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(hook.declaration_index, 0);
        assert_eq!(
            &code[found[0].declaration_start_byte as usize..found[0].declaration_end_byte as usize],
            "hook 'core.app.before_request' => sub { 1 }"
        );
    }

    #[test]
    fn reviewed_alias_normalizes_to_the_canonical_name() {
        let found = declarations("hook 'before' => sub { 1 };");
        let name = literal_name(&found[0]);
        assert_eq!(name.literal, "before");
        assert_eq!(name.canonical(), Some("core.app.before_request"));
        assert_eq!(
            name.normalization,
            HookNameNormalization::Alias { canonical: "core.app.before_request".to_string() }
        );
        assert!(!name.is_boundary());
    }

    #[test]
    fn two_stage_coerce_alias_normalizes() {
        let found = declarations("hook 'before_template' => sub { 1 };");
        let name = literal_name(&found[0]);
        assert_eq!(name.literal, "before_template");
        assert_eq!(name.canonical(), Some("engine.template.before_render"));
    }

    #[test]
    fn static_coderef_handler_resolves_forward_declaration() {
        let code = "hook 'after' => \\&teardown;\nsub teardown { 1 }";
        let found = declarations(code);
        assert_eq!(found.len(), 1);
        let (name, target, anchor) = must_some(match &found[0].hook.handler {
            FrameworkHandler::StaticCoderef { name, target, anchor } => {
                Some((name.as_str(), target, *anchor))
            }
            _ => None,
        });
        assert_eq!(name, "teardown");
        assert_eq!(target.name, "teardown");
        assert_eq!(target.package, "main");
        assert_eq!(&code[anchor.start_byte as usize..anchor.end_byte as usize], "\\&teardown");
        assert!(!found[0].hook.has_boundary());
    }

    #[test]
    fn computed_and_dynamic_operands_are_boundaries() {
        let found = declarations("hook $name => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(&found[0].hook.name, HookNameSelection::Dynamic { .. }));

        let found = declarations("hook \"before_$phase\" => sub { 1 };");
        assert_eq!(found.len(), 1);
        assert!(matches!(&found[0].hook.name, HookNameSelection::Dynamic { .. }));

        let found = declarations("hook 'before' => $code;");
        assert_eq!(found.len(), 1);
        assert!(matches!(
            &found[0].hook.handler,
            FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::Computed, .. }
        ));

        let found = declarations("hook 'before' => \\&missing;");
        assert_eq!(found.len(), 1);
        assert!(matches!(
            &found[0].hook.handler,
            FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::StaticCoderef, .. }
        ));
    }

    #[test]
    fn unreviewed_names_keep_literal_behind_a_boundary() {
        let found = declarations("hook 'plugin.database.before_dbi_connect' => sub { 1 };");
        assert_eq!(found.len(), 1);
        let name = literal_name(&found[0]);
        assert_eq!(name.literal, "plugin.database.before_dbi_connect");
        assert_eq!(name.canonical(), None);
        assert!(name.is_boundary());
        assert!(found[0].hook.has_boundary(), "ownership stays a typed boundary");
    }

    #[test]
    fn malformed_arities_mint_nothing() {
        for code in [
            "hook;",
            "hook 'before';",
            "hook sub { 1 };",
            "hook 'before', sub { 1 }, sub { 2 };",
            "hook 'before', 'extra', sub { 1 };",
        ] {
            assert!(declarations(code).is_empty(), "`{code}` must not mint a hook");
        }
    }

    #[test]
    fn hooks_inside_sub_bodies_mint_nothing() {
        let code = "package App;
use Dancer2;
sub deferred { hook 'before' => sub { 1 }; }
hook 'after' => sub { 2 };
";
        let found = declarations(code);
        assert_eq!(found.len(), 1, "only the load-time hook mints");
        assert_eq!(literal_name(&found[0]).literal, "after");
    }

    #[test]
    fn hooks_inside_control_flow_mint_nothing() {
        // A hook under `if`/`unless`/loops registers only when the enclosing
        // condition executes at load time: statically execution-conditional,
        // so no hook fact may claim unconditional registration.
        for code in [
            "if ($ENV{ENABLE}) { hook 'before' => sub { 1 }; }",
            "unless ($disabled) { hook 'before' => sub { 1 }; }",
            "while (my $next = $it->()) { hook 'before' => sub { 1 }; }",
            "for my $phase (@phases) { hook 'before' => sub { 1 }; }",
            "foreach my $phase (@phases) { hook 'before' => sub { 1 }; }",
            "try { hook 'before' => sub { 1 }; } catch ($e) { }",
            "$enabled && hook 'before' => sub { 1 };",
            "$enabled || hook 'before' => sub { 1 };",
            "hook 'before' => sub { 1 } if $enabled;",
            "hook 'before' => sub { 1 } unless $disabled;",
            "hook 'before' => sub { 1 } for @phases;",
        ] {
            assert!(
                declarations(code).is_empty(),
                "an execution-conditional `{code}` must mint no hook fact"
            );
        }

        // Straight-line load-time hooks (and a nested block) still mint.
        let found = declarations(
            "hook 'before' => sub { 1 };\n{ hook 'after' => sub { 2 }; }\nhook 'init_error' => sub { 3 };",
        );
        assert_eq!(found.len(), 3, "unconditional load-time hooks keep minting");

        // A hook embedded in a larger expression (rather than standing as
        // the whole statement) mints nothing: only statement-form hook
        // declarations are reviewed grammar.
        assert!(declarations("hook 'before' => sub { 1 } and warn 'registered';").is_empty());
    }

    #[test]
    fn package_state_after_conditional_blocks_follows_block_scoping() {
        // `package` inside a block (conditional or not) is lexically scoped:
        // it applies to statements inside that block and reverts afterwards.
        // The conditional hook inside mints nothing; the following load-time
        // hook observes the restored enclosing package.
        let code = "package App;
if (1) { package Inner; hook 'before' => sub { 1 }; }
hook 'after' => sub { 2 };";
        let found = declarations(code);
        assert_eq!(found.len(), 1, "the conditional hook mints nothing");
        assert_eq!(found[0].package.as_deref(), Some("App"), "block package state reverts");
        assert_eq!(literal_name(&found[0]).literal, "after");
    }

    #[test]
    fn hooks_are_package_scoped_and_main_defaulted() {
        let found = declarations(
            "hook 'before' => sub { 1 };\npackage App;\nhook 'before' => sub { 2 };\n{ package Inner; hook 'after' => sub { 3 }; }\nhook 'init_error' => sub { 4 };\n",
        );
        assert_eq!(found.len(), 4);
        assert_eq!(found[0].package.as_deref(), Some("main"));
        assert_eq!(found[1].package.as_deref(), Some("App"));
        assert_eq!(found[2].package.as_deref(), Some("Inner"));
        assert_eq!(found[3].package.as_deref(), Some("App"), "block package state restored");
        for (index, declaration) in found.iter().enumerate() {
            assert_eq!(declaration.hook.declaration_index, index as u32);
        }
    }

    #[test]
    fn unrelated_hook_shaped_calls_mint_nothing() {
        assert!(declarations("hooked 'before' => sub { 1 };").is_empty());
        assert!(declarations("my $x = hook;\nprint $x;").is_empty());
    }

    #[test]
    fn same_alias_twice_stays_distinct_by_source_order() {
        let found = declarations("hook 'before' => sub { 1 };\nhook 'before' => sub { 2 };");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].hook.declaration_index, 0);
        assert_eq!(found[1].hook.declaration_index, 1);
        assert_ne!(found[0].declaration_start_byte, found[1].declaration_start_byte);
    }
}
