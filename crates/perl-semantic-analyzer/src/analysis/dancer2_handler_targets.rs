//! Static coderef target resolution and shared handler binding (#8924).
//!
//! The #8918 route extractor classified every `\&handler` operand as a typed
//! boundary because the canonical callable fact layer did not prove
//! named-subroutine targets. This module supplies that proof for the
//! statically resolvable case: an in-file, package-scoped named `sub`
//! declaration.
//!
//! Resolution follows Perl compile-time semantics:
//!
//! - an unqualified `\&name` resolves against the **current package** of the
//!   declaration, and forward declarations count (a `sub name { ... }` later
//!   in the file, or a `sub name;` stub — resolution is position-independent
//!   per package);
//! - a qualified `\&Package::name` resolves against the named in-file
//!   package (a leading `::` is the explicit `main` package);
//! - only package-scoped declarations resolve: `my sub`/`state sub` are
//!   lexical and never resolve through a package-qualified coderef;
//! - a name with no in-file package-scoped declaration stays a typed
//!   boundary (the target may live in another file, be imported, or be
//!   undefined) — never a fictional target;
//! - `\&name(args)` is a reference to a call result, not a coderef to the
//!   sub: it stays a computed boundary.
//!
//! The index walk mirrors the #8914 activation walk's package discipline
//! (bare `package X;` switches the current package for following statements;
//! a lexical block restores the enclosing package state afterwards) and does
//! not descend into subroutine bodies: a named sub declared inside another
//! sub's body is not indexed (statically execution-conditional context), so
//! references to it stay boundaries.

use crate::ast::{Node, NodeKind};
use perl_semantic_facts::handler::{FrameworkHandler, FrameworkHandlerBoundary, SubroutineTarget};
use perl_semantic_facts::{AnchorId, FileId, SourceAnchor};
use std::collections::{HashMap, HashSet};

/// Index of in-file package-scoped named subroutine declarations.
///
/// Keyed by `(package, name)`. A later declaration with a body replaces the
/// earlier entry (Perl package symbol semantics: a redefining `sub name`
/// replaces the CODE slot), but a later bodyless `sub name;` stub does **not**
/// replace an existing concrete definition — Perl keeps the defined body.
/// Slots whose typeglob was reassigned anywhere in the file
/// (`*name = ...`) are tracked separately and never resolve: the invoked
/// target may be a different subroutine at runtime.
#[derive(Debug, Default)]
pub struct SubroutineTargetIndex {
    targets: HashMap<(String, String), SubroutineTarget>,
    mutated_slots: HashSet<(String, String)>,
}

impl SubroutineTargetIndex {
    /// Build the index for one file's AST.
    #[must_use]
    pub fn build(ast: &Node, file_id: FileId) -> Self {
        let mut index = Self::default();
        let mut current_package: Option<String> = Some("main".to_string());
        index.walk(ast, file_id, &mut current_package);
        index
    }

    /// Resolve one coderef target name in `current_package`.
    ///
    /// `None` means the target is not statically resolvable in-file (no
    /// declaration, or the slot's typeglob was reassigned).
    #[must_use]
    pub fn resolve(
        &self,
        written: &str,
        current_package: Option<&str>,
    ) -> Option<SubroutineTarget> {
        let (package, name) = split_qualified_name(written, current_package)?;
        let name = name?;
        let key = (package, name);
        if self.mutated_slots.contains(&key) {
            return None;
        }
        let mut target = self.targets.get(&key)?.clone();
        target.package = key.0;
        Some(target)
    }

    /// Whether the resolved slot of `written` was typeglob-reassigned in this
    /// file (`*name = ...`): the CODE slot may alias another subroutine at
    /// runtime, so an in-file declaration no longer proves the invoked target.
    #[must_use]
    pub fn slot_is_typeglob_reassigned(
        &self,
        written: &str,
        current_package: Option<&str>,
    ) -> bool {
        let Some((package, Some(name))) = split_qualified_name(written, current_package) else {
            return false;
        };
        self.mutated_slots.contains(&(package, name))
    }

    fn walk(&mut self, node: &Node, file_id: FileId, current_package: &mut Option<String>) {
        match &node.kind {
            NodeKind::Program { statements } | NodeKind::Block { statements } => {
                let mut block_package = current_package.clone();
                self.walk_statements(statements, file_id, &mut block_package);
            }
            NodeKind::Package { name, block: Some(block), .. } => {
                let mut package_scope = Some(name.clone());
                if let NodeKind::Block { statements } = &block.kind {
                    self.walk_statements(statements, file_id, &mut package_scope);
                }
            }
            NodeKind::Package { name, block: None, .. } => {
                *current_package = Some(name.clone());
            }
            NodeKind::Subroutine { name, declarator, .. } => {
                // Package-scoped declarations only: `my`/`state` subs are
                // lexical and never resolve through `\&package::name`. A
                // `package None` context cannot own package subs.
                if let (Some(sub_name), Some(package)) = (name, current_package.as_deref()) {
                    let is_package_scoped = match declarator.as_deref() {
                        None | Some("our") => true,
                        Some("my") | Some("state") => false,
                        Some(_) => false,
                    };
                    if is_package_scoped {
                        let key = (package.to_string(), sub_name.clone());
                        let candidate = target_of(node, file_id);
                        // A later bodyless stub does not replace an existing
                        // concrete definition: Perl keeps the CODE slot's
                        // defined body. Anything else (a later body, or the
                        // first sighting of the name) replaces the entry.
                        let keep_existing = self.targets.get(&key).is_some_and(|existing| {
                            existing.body_anchor.is_some() && candidate.body_anchor.is_none()
                        });
                        if !keep_existing {
                            self.targets.insert(key, candidate);
                        }
                    }
                }
            }
            NodeKind::Assignment { lhs, .. } => {
                // A typeglob assignment to a sub slot (`*name = ...`,
                // `*Package::name = ...`) can alias the CODE slot to any
                // other subroutine at runtime: existence of a same-name `sub`
                // no longer proves which target a `\&name` invokes. Record
                // the slot and keep it unresolvable.
                if let NodeKind::Typeglob { name: glob_name } = &lhs.kind {
                    let (package, name) =
                        split_qualified_name(glob_name, current_package.as_deref())
                            .unwrap_or_else(|| ("main".to_string(), None));
                    if let Some(name) = name {
                        self.mutated_slots.insert((package, name));
                    }
                }
                for child in node.children() {
                    self.walk(child, file_id, current_package);
                }
            }
            _ => {
                for child in node.children() {
                    self.walk(child, file_id, current_package);
                }
            }
        }
    }

    fn walk_statements(
        &mut self,
        statements: &[Node],
        file_id: FileId,
        current_package: &mut Option<String>,
    ) {
        for statement in statements {
            self.walk(statement, file_id, current_package);
        }
    }
}

/// Split a written coderef name into `(package, sub_name)`.
///
/// Unqualified names resolve against `current_package`; `Package::name` (or a
/// leading `::name` for explicit `main`) resolves against the named package.
/// `None` for the sub name means the written name cannot denote a static
/// target (empty segment).
fn split_qualified_name(
    written: &str,
    current_package: Option<&str>,
) -> Option<(String, Option<String>)> {
    let written = written.trim();
    if let Some(rest) = written.strip_prefix("::") {
        return Some(("main".to_string(), non_empty(rest)));
    }
    match written.rfind("::") {
        Some(position) => {
            let package = &written[..position];
            let name = &written[position + 2..];
            Some((non_empty(package)?, non_empty(name)))
        }
        None => Some((current_package.unwrap_or("main").to_string(), non_empty(written))),
    }
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() { None } else { Some(value.to_string()) }
}

/// Build the canonical [`SubroutineTarget`] for one declaration node.
fn target_of(node: &Node, file_id: FileId) -> SubroutineTarget {
    let NodeKind::Subroutine { name, name_span, body, .. } = &node.kind else {
        unreachable!("target_of is only called on Subroutine nodes");
    };
    let name = name.clone().unwrap_or_default();
    let name_anchor =
        name_span.as_ref().map(|span| anchor(span.start(), span.end(), file_id)).unwrap_or_else(
            || anchor(node.location.start(), node.location.start() + name.len(), file_id),
        );
    let declaration_anchor = anchor(node.location.start(), node.location.end(), file_id);
    // Every `Block` with a real source span retains its body anchor — an
    // empty `{}` body is a body. Only the degenerate zero-width body of a
    // `sub name;` forward stub records `None`; the spans distinguish the two
    // because the parser shapes them identically at the node-kind level.
    let body_anchor = match &body.kind {
        NodeKind::Block { .. } if body.location.start() < body.location.end() => {
            Some(anchor(body.location.start(), body.location.end(), file_id))
        }
        _ => None,
    };
    SubroutineTarget { name, package: String::new(), name_anchor, declaration_anchor, body_anchor }
}

/// Bind one handler operand node to the canonical handler relation.
///
/// Shared by the Dancer2 route and hook extractors: inline subs and
/// statically resolvable coderefs are exact; string, unresolvable coderef,
/// and computed operands are typed boundaries with explicit reasons.
#[must_use]
pub fn handler_from_node(
    node: &Node,
    file_id: FileId,
    current_package: Option<&str>,
    targets: &SubroutineTargetIndex,
) -> FrameworkHandler {
    let operand_anchor = anchor(node.location.start(), node.location.end(), file_id);
    match &node.kind {
        NodeKind::Subroutine { name, .. } if name.is_none() => {
            FrameworkHandler::InlineSub { anchor: operand_anchor }
        }
        NodeKind::String { value, .. } => FrameworkHandler::Bounded {
            boundary: FrameworkHandlerBoundary::String,
            anchor: Some(operand_anchor),
            reason: format!(
                "string handler `{value}` is not an exact coderef target (the reviewed DSL \
                 requires a CODE ref)"
            ),
        },
        NodeKind::Unary { op, operand } if op == "\\" => match &operand.kind {
            // `\&name` (no call arguments) is a coderef to the named sub.
            NodeKind::AmperCall { name, args } if args.is_empty() => {
                match targets.resolve(name, current_package) {
                    Some(target) => FrameworkHandler::StaticCoderef {
                        name: name.clone(),
                        anchor: operand_anchor,
                        target,
                    },
                    None => FrameworkHandler::Bounded {
                        boundary: FrameworkHandlerBoundary::StaticCoderef,
                        anchor: Some(operand_anchor),
                        reason: if targets.slot_is_typeglob_reassigned(name, current_package) {
                            format!(
                                "static coderef `{name}` targets a slot whose typeglob was \
                                 reassigned in this file; the invoked subroutine is not \
                                 statically provable"
                            )
                        } else {
                            format!(
                                "static coderef `{name}` has no in-file package-scoped \
                                 declaration; the target is not statically provable"
                            )
                        },
                    },
                }
            }
            _ => FrameworkHandler::Bounded {
                boundary: FrameworkHandlerBoundary::Computed,
                anchor: Some(operand_anchor),
                reason: "reference expression is not a named-subroutine coderef".to_string(),
            },
        },
        _ => FrameworkHandler::Bounded {
            boundary: FrameworkHandlerBoundary::Computed,
            anchor: Some(operand_anchor),
            reason: "computed handler expression is not an exact handler target".to_string(),
        },
    }
}

pub(crate) fn anchor(start: usize, end: usize, file_id: FileId) -> SourceAnchor {
    SourceAnchor::new(
        Some(AnchorId(start as u64)),
        file_id,
        start.min(u32::MAX as usize) as u32,
        end.min(u32::MAX as usize) as u32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
    use perl_tdd_support::{must, must_some};

    fn parse(code: &str) -> Node {
        let mut parser = Parser::new(code);
        must(parser.parse())
    }

    /// Last operand of the first `hook`/route-style call in the program.
    fn last_call_operand<'a>(ast: &'a Node, call_name: &str) -> &'a Node {
        let mut found: Option<&'a Node> = None;
        find_call_operand(ast, call_name, &mut found);
        must_some(found)
    }

    fn find_call_operand<'a>(node: &'a Node, call_name: &str, found: &mut Option<&'a Node>) {
        if let NodeKind::FunctionCall { name, args } = &node.kind {
            if name == call_name {
                if let Some(last) = args.last() {
                    *found = Some(last);
                    return;
                }
            }
        }
        for child in node.children() {
            find_call_operand(child, call_name, found);
        }
    }

    fn resolved(handler: &FrameworkHandler) -> (&str, &SubroutineTarget) {
        must_some(match handler {
            FrameworkHandler::StaticCoderef { name, target, .. } => Some((name.as_str(), target)),
            _ => None,
        })
    }

    fn bounded_parts(handler: &FrameworkHandler) -> (FrameworkHandlerBoundary, &str) {
        must_some(match handler {
            FrameworkHandler::Bounded { boundary, reason, .. } => {
                Some((*boundary, reason.as_str()))
            }
            _ => None,
        })
    }

    fn bind(code: &str, call_name: &str) -> FrameworkHandler {
        let ast = parse(code);
        let index = SubroutineTargetIndex::build(&ast, FileId(1));
        handler_from_node(last_call_operand(&ast, call_name), FileId(1), Some("main"), &index)
    }

    #[test]
    fn coderef_resolves_to_exact_forward_declaration_identity() {
        let code = "get '/x' => \\&handler;\nsub handler { 1 }";
        let handler = bind(code, "get");
        let (name, target) = resolved(&handler);
        assert_eq!(name, "handler");
        assert_eq!(target.name, "handler");
        assert_eq!(target.package, "main");
        assert!(target.body_anchor.is_some(), "the resolved declaration has a body");
        assert!(handler.is_exact());
        // The target name anchor points at the declaration tokens.
        assert_eq!(
            &code[target.name_anchor.start_byte as usize..target.name_anchor.end_byte as usize],
            "handler"
        );
    }

    #[test]
    fn coderef_resolves_to_a_stub_declaration_without_body() {
        let code = "sub handler;\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        assert!(target.body_anchor.is_none(), "a forward stub carries no body");
        assert!(handler.is_exact(), "the stub is still an exact declaration target");
    }

    #[test]
    fn coderef_resolves_within_the_current_package() {
        let code = "package App;\nsub handler { 1 }\npackage Other;\nsub handler { 2 }\nget '/x' => \\&handler;";
        let ast = parse(code);
        let index = SubroutineTargetIndex::build(&ast, FileId(1));
        let handler =
            handler_from_node(last_call_operand(&ast, "get"), FileId(1), Some("Other"), &index);
        let (_, target) = resolved(&handler);
        assert_eq!(target.package, "Other");
    }

    #[test]
    fn qualified_coderef_resolves_to_the_named_package() {
        let code = "package App;\nsub handler { 1 }\npackage Other;\nget '/x' => \\&App::handler;";
        let handler = bind(code, "get");
        let (name, target) = resolved(&handler);
        assert_eq!(name, "App::handler");
        assert_eq!(target.package, "App");
    }

    #[test]
    fn unresolvable_coderef_stays_a_typed_boundary() {
        // No in-file declaration at all (imported/undefined/cross-file).
        let handler = bind("get '/x' => \\&missing_handler;", "get");
        let (boundary, reason) = bounded_parts(&handler);
        assert_eq!(boundary, FrameworkHandlerBoundary::StaticCoderef);
        assert!(reason.contains("missing_handler"), "reason names the written target: {reason}");
        assert!(!handler.is_exact());
    }

    #[test]
    fn cross_package_name_without_declaration_stays_a_boundary() {
        let handler = bind("package Other;\nget '/x' => \\&App::handler;", "get");
        assert!(
            matches!(
                handler,
                FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::StaticCoderef, .. }
            ),
            "a qualified name with no in-file declaration stays bounded"
        );
    }

    #[test]
    fn lexical_sub_does_not_resolve_through_a_package_coderef() {
        let handler = bind("my sub handler { 1 }\nget '/x' => \\&handler;", "get");
        assert!(
            matches!(
                handler,
                FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::StaticCoderef, .. }
            ),
            "`my sub` is lexical and never a package-scoped coderef target"
        );
    }

    #[test]
    fn coderef_with_call_arguments_is_a_computed_boundary() {
        let handler = bind("sub handler { 1 }\nget '/x' => \\&handler(1);", "get");
        assert!(
            matches!(
                handler,
                FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::Computed, .. }
            ),
            "`\\&name(args)` references a call result, not the sub"
        );
    }

    #[test]
    fn string_and_computed_operands_stay_boundaries() {
        assert!(matches!(
            bind("get '/x' => 'handler_name';", "get"),
            FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::String, .. }
        ));
        assert!(matches!(
            bind("get '/x' => $code;", "get"),
            FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::Computed, .. }
        ));
    }

    #[test]
    fn inline_sub_stays_exact() {
        let handler = bind("get '/x' => sub { 1 };", "get");
        assert!(matches!(handler, FrameworkHandler::InlineSub { .. }));
        assert!(handler.is_exact());
    }

    #[test]
    fn duplicate_declarations_last_one_wins() {
        let code = "sub handler { 1 }\nsub handler { 2 }\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        let body = must_some(target.body_anchor.as_ref());
        assert!(
            &code[body.start_byte as usize..body.end_byte as usize].contains('2'),
            "the later declaration replaces the earlier CODE slot entry"
        );
    }

    #[test]
    fn later_stub_does_not_replace_a_concrete_definition() {
        // `sub handler;` after a concrete definition declares nothing new:
        // Perl keeps the defined CODE slot, so the resolved target must keep
        // the real body and its anchors.
        let code = "sub handler { return 42; }\nsub handler;\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        let body = must_some(target.body_anchor.as_ref());
        assert!(
            &code[body.start_byte as usize..body.end_byte as usize].contains("42"),
            "the concrete definition survives a later bodyless stub"
        );
        // The reverse order still resolves to the later concrete definition.
        let code = "sub handler;\nsub handler { return 7; }\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        let body = must_some(target.body_anchor.as_ref());
        assert!(&code[body.start_byte as usize..body.end_byte as usize].contains("7"));
    }

    #[test]
    fn empty_sub_body_retains_its_body_anchor() {
        // `sub handler {}` is a real (empty) body, distinct from the
        // `sub handler;` forward stub: the braces have a source span.
        let code = "sub handler {}\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        let body = must_some(target.body_anchor.as_ref());
        assert_eq!(
            &code[body.start_byte as usize..body.end_byte as usize],
            "{}",
            "an empty body keeps its exact source span"
        );

        // The stub keeps no body anchor.
        let code = "sub handler;\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (_, target) = resolved(&handler);
        assert!(target.body_anchor.is_none(), "a forward stub carries no body");
    }

    #[test]
    fn typeglob_reassignment_bounds_the_coderef() {
        // `*handler = \&other` aliases the CODE slot at runtime: Perl invokes
        // `other`, so existence of `sub handler` no longer proves the target.
        let code =
            "sub handler { 1 }\nsub other { 2 }\n*handler = \\&other;\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        let (boundary, reason) = bounded_parts(&handler);
        assert_eq!(boundary, FrameworkHandlerBoundary::StaticCoderef);
        assert!(
            reason.contains("typeglob"),
            "the boundary names the typeglob reassignment: {reason}"
        );
        assert!(!handler.is_exact());

        // A qualified glob assignment bounds the named package's slot.
        let code = "package App;\nsub handler { 1 }\npackage Other;\n*App::handler = \\&elsewhere;\nget '/x' => \\&App::handler;";
        let ast = parse(code);
        let index = SubroutineTargetIndex::build(&ast, FileId(1));
        let handler =
            handler_from_node(last_call_operand(&ast, "get"), FileId(1), Some("Other"), &index);
        assert!(matches!(
            handler,
            FrameworkHandler::Bounded { boundary: FrameworkHandlerBoundary::StaticCoderef, .. }
        ));

        // Control: no typeglob assignment keeps the exact resolution.
        let code = "sub handler { 1 }\nget '/x' => \\&handler;";
        let handler = bind(code, "get");
        assert!(handler.is_exact());
    }
}
