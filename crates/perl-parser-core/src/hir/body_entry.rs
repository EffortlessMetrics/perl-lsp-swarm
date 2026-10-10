//! Compatibility entry for a complete source program's canonical body (#5813).
//!
//! The non-Program subtree path remains explicitly legacy. This module does not
//! implement another expression traversal or change the production lowerer.

use crate::{Node, NodeKind};

use super::{HirBody, body, lower};

/// Lower the root body of a parsed program through the canonical compiler path.
///
/// For a [`NodeKind::Program`], this is the root-body projection of
/// [`super::lower_ast`], including its scope-derived binding identities, arenas,
/// source maps and explicit limitations. Binding IDs remain local to that
/// lowering; callers needing the associated scope graph should retain the whole
/// [`super::HirFile`] returned by `lower_ast` instead.
///
/// Non-Program AST fragments retain the legacy subtree behavior for compatibility.
/// That path has no surrounding file's binding authority and must not be used as
/// evidence that complete source programs have canonical semantics.
///
/// The Program path constructs a complete `HirFile` before taking its root body;
/// it is not a performance optimization. Neither path executes Perl.
pub fn lower_body(ast: &Node) -> HirBody {
    if matches!(&ast.kind, NodeKind::Program { .. }) {
        let mut file = lower::lower_ast(ast);
        // `lower_bodies_into_file` unconditionally inserts the program root at
        // index 0, including for an empty program, before any callable bodies.
        // Do not hide a broken internal postcondition behind an empty/legacy
        // fallback. Other bodies are discarded, so their order need not survive.
        file.bodies.swap_remove(0)
    } else {
        body::lower_body(ast)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;
    use crate::Parser;
    use crate::hir::{AccessMode, BodyOwnerKind, HirExpr, HirStmt, VariableKind};

    type TestResult = Result<(), Box<dyn Error>>;

    fn parsed(source: &str) -> Node {
        Parser::new(source).parse_with_recovery().ast
    }

    fn first_expression(body: &HirBody) -> Result<&HirExpr, Box<dyn Error>> {
        let block = body.block(body.root_block).ok_or("missing root block")?;
        let id = block.stmts.first().ok_or("missing root statement")?;
        let Some(HirStmt::Expr(expr)) = body.stmt(*id) else {
            return Err("root statement is not an expression".into());
        };
        body.expr(*expr).ok_or_else(|| "missing expression".into())
    }

    #[test]
    fn program_wrapper_retains_a_real_unbound_variable() -> TestResult {
        let ast = parsed("$a;");
        let body = lower_body(&ast);
        let expression = first_expression(&body)?;
        assert!(matches!(
            expression,
            HirExpr::Variable(var)
                if var.name == "a"
                    && var.kind == VariableKind::Package
                    && var.access == AccessMode::Read
                    && var.binding.is_none()
        ));
        // A concrete predecessor, not a fabricated broken model, distinguishes
        // the repaired public route from the old independent traversal.
        let predecessor = body::lower_body(&ast);
        assert!(matches!(first_expression(&predecessor)?, HirExpr::Opaque { .. }));
        assert_ne!(body, predecessor);
        Ok(())
    }

    #[test]
    fn program_declaration_and_read_share_canonical_binding() -> TestResult {
        let ast = parsed("my $x = $a; $x;");
        let body = lower_body(&ast);
        let file = lower::lower_ast(&ast);
        assert_eq!(Some(&body), file.root_body());
        let block = body.block(body.root_block).ok_or("missing root block")?;
        let declaration = block.stmts.first().ok_or("missing declaration")?;
        let Some(HirStmt::Let { binding: Some(binding), .. }) = body.stmt(*declaration) else {
            return Err("declaration has no canonical binding".into());
        };
        let occurrences: Vec<_> = body
            .exprs
            .iter()
            .filter_map(|expr| match expr {
                HirExpr::Variable(var) if var.name == "x" => Some(var),
                _ => None,
            })
            .collect();
        assert_eq!(occurrences.len(), 2);
        assert!(
            occurrences
                .iter()
                .all(|var| var.binding == Some(*binding) && var.kind == VariableKind::Lexical)
        );
        assert_eq!(occurrences[0].access, AccessMode::Write);
        assert_eq!(occurrences[1].access, AccessMode::Read);
        assert_eq!(body.source_map, file.root_body().ok_or("missing canonical root")?.source_map);
        Ok(())
    }

    #[test]
    fn program_loop_keeps_its_nonempty_body() -> TestResult {
        let ast = parsed("while ($ready) { $x = 1; }");
        let body = lower_body(&ast);
        let HirExpr::Loop { body: loop_body, .. } = first_expression(&body)? else {
            return Err("loop collapsed to a non-loop expression".into());
        };
        assert_eq!(body.block(*loop_body).ok_or("missing loop block")?.stmts.len(), 1);
        assert_eq!(Some(&body), lower::lower_ast(&ast).root_body());
        Ok(())
    }

    #[test]
    fn empty_program_has_a_real_empty_root() -> TestResult {
        let ast = parsed("");
        assert!(matches!(&ast.kind, NodeKind::Program { .. }));
        let body = lower_body(&ast);
        assert_eq!(body.owner, BodyOwnerKind::ProgramRoot);
        assert_eq!(body.blocks.len(), 1);
        assert!(body.block(body.root_block).ok_or("missing empty root")?.stmts.is_empty());
        assert!(body.exprs.is_empty());
        assert_eq!(body.source_map.block_range(body.root_block), Some(ast.location));
        assert_eq!(Some(&body), lower::lower_ast(&ast).root_body());
        Ok(())
    }

    #[test]
    fn nested_callable_cannot_replace_the_program_root() -> TestResult {
        let ast = parsed("sub nested { my $inside = 1; $inside; } my $outside = 2;");
        let file = lower::lower_ast(&ast);
        assert_eq!(file.bodies.len(), 2);
        let body = lower_body(&ast);
        assert_eq!(body.owner, BodyOwnerKind::ProgramRoot);
        assert_eq!(Some(&body), file.root_body());
        assert_ne!(Some(&body), file.bodies.get(1));
        assert_eq!(body.source_map.block_range(body.root_block), Some(ast.location));
        Ok(())
    }

    #[test]
    fn recovery_keeps_the_canonical_body_not_an_empty_fallback() {
        let ast = parsed("my $x = ;");
        let file = lower::lower_ast(&ast);
        let body = lower_body(&ast);
        assert_eq!(Some(&body), file.root_body());
        assert_eq!(body.owner, BodyOwnerKind::ProgramRoot);
        // Equality is a projection claim, not permission to execute recovered Perl.
    }

    #[test]
    fn non_program_fragment_keeps_legacy_subtree_behavior() -> TestResult {
        let ast = parsed("my $x = $outside;");
        let NodeKind::Program { statements } = &ast.kind else {
            return Err("parser did not produce Program".into());
        };
        let fragment = statements.first().ok_or("missing declaration fragment")?;
        assert!(!matches!(&fragment.kind, NodeKind::Program { .. }));
        assert_eq!(lower_body(fragment), body::lower_body(fragment));
        let body = lower_body(fragment);
        assert!(body.exprs.iter().any(|expr| matches!(
            expr,
            HirExpr::Variable(var)
                if var.name == "outside"
                    && var.kind == VariableKind::Lexical
                    && var.binding.is_none()
        )));
        Ok(())
    }
}
