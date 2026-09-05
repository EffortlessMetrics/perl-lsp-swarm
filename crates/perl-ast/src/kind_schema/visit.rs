//! Production field-aware child traversal derived from the structural registry.
//!
//! Immutable and mutable walkers share one exhaustiveness table so field identity,
//! emission order, and short-circuiting cannot drift. `for_each_child_mut` is a
//! compatibility wrapper over the mutable field-aware walker.
//!
//! Physical child storage is not 1:1 with registry rows (`HashLiteral` pairs,
//! `If` elsif clauses). The table therefore emits in observable source/model
//! order using registry [`crate::FieldId`] values rather than regrouping by
//! field. A naive "emit every child of field A, then field B" walk would change
//! `If` and `HashLiteral` order.

use super::{ChildFieldSpec, KindStructuralRow, NODE_KIND_STRUCTURAL_REGISTRY};
use crate::{FieldId, Node, NodeKind};
use std::ops::ControlFlow;

/// Look up the structural row for a stable [`crate::NodeKind::kind_name`].
#[must_use]
pub fn structural_row(kind_name: &str) -> Option<&'static KindStructuralRow<'static>> {
    NODE_KIND_STRUCTURAL_REGISTRY.iter().find(|row| row.kind_name == kind_name)
}

/// Child-field specs registered for `kind_name`, or an empty slice when unknown.
#[must_use]
pub fn registered_child_fields(kind_name: &str) -> &'static [ChildFieldSpec] {
    structural_row(kind_name).map(|row| row.children).unwrap_or(&[])
}

/// Unique [`FieldId`] values in first-seen registry order.
///
/// Public [`FieldId::ALL`] keeps the compatibility order of the named constants.
/// Set membership must match this inventory; order may differ.
#[must_use]
pub fn registry_field_id_set() -> Vec<FieldId> {
    let mut seen = Vec::new();
    for row in NODE_KIND_STRUCTURAL_REGISTRY {
        for child in row.children {
            if !seen.iter().any(|field: &FieldId| field.name() == child.field.name()) {
                seen.push(child.field);
            }
        }
    }
    seen
}

macro_rules! visit_kind_children {
    ($kind:expr, $emit:ident) => {{
        match $kind {
            NodeKind::Tie { variable, package, args } => {
                $emit!(FieldId::VARIABLE, variable);
                $emit!(FieldId::PACKAGE, package);
                for arg in args {
                    $emit!(FieldId::ARGS, arg);
                }
            }
            NodeKind::Untie { variable } => $emit!(FieldId::VARIABLE, variable),

            // Root program node
            NodeKind::Program { statements } => {
                for stmt in statements {
                    $emit!(FieldId::STATEMENTS, stmt);
                }
            }

            // Statement wrappers
            NodeKind::ExpressionStatement { expression } => $emit!(FieldId::EXPRESSION, expression),

            // Variable declarations
            NodeKind::VariableDeclaration { variable, initializer, .. } => {
                $emit!(FieldId::VARIABLE, variable);
                if let Some(init) = initializer {
                    $emit!(FieldId::INITIALIZER, init);
                }
            }
            NodeKind::VariableListDeclaration { variables, initializer, .. } => {
                for var in variables {
                    $emit!(FieldId::VARIABLE, var);
                }
                if let Some(init) = initializer {
                    $emit!(FieldId::INITIALIZER, init);
                }
            }
            NodeKind::NestedVariableList { items } => {
                for item in items {
                    $emit!(FieldId::ITEMS, item);
                }
            }
            NodeKind::VariableWithAttributes { variable, .. } => {
                $emit!(FieldId::VARIABLE, variable)
            }

            // Binary operations
            NodeKind::Binary { left, right, .. } => {
                $emit!(FieldId::LEFT, left);
                $emit!(FieldId::RIGHT, right);
            }
            NodeKind::ArraySlice { target, indices } => {
                $emit!(FieldId::TARGET, target);
                $emit!(FieldId::ELEMENTS, indices);
            }
            NodeKind::HashSlice { target, keys } | NodeKind::KeyValueSlice { target, keys } => {
                $emit!(FieldId::TARGET, target);
                $emit!(FieldId::KEY, keys);
            }
            NodeKind::ChainedComparison { operands, .. } => {
                for operand in operands {
                    $emit!(FieldId::ELEMENTS, operand);
                }
            }
            NodeKind::Ternary { condition, then_expr, else_expr } => {
                $emit!(FieldId::CONDITION, condition);
                $emit!(FieldId::THEN_EXPR, then_expr);
                $emit!(FieldId::ELSE_EXPR, else_expr);
            }
            NodeKind::Unary { operand, .. } => $emit!(FieldId::OPERAND, operand),
            NodeKind::Assignment { lhs, rhs, .. } => {
                $emit!(FieldId::LHS, lhs);
                $emit!(FieldId::RHS, rhs);
            }

            // Control flow
            NodeKind::Block { statements } => {
                for stmt in statements {
                    $emit!(FieldId::STATEMENTS, stmt);
                }
            }
            NodeKind::If { condition, then_branch, elsif_branches, else_branch, .. } => {
                $emit!(FieldId::CONDITION, condition);
                $emit!(FieldId::THEN_BRANCH, then_branch);
                for (elsif_cond, elsif_body) in elsif_branches {
                    $emit!(FieldId::CONDITION, elsif_cond);
                    $emit!(FieldId::BODY, elsif_body);
                }
                if let Some(else_body) = else_branch {
                    $emit!(FieldId::ELSE_BRANCH, else_body);
                }
            }
            NodeKind::While { condition, body, continue_block, .. } => {
                $emit!(FieldId::CONDITION, condition);
                $emit!(FieldId::BODY, body);
                if let Some(cont) = continue_block {
                    $emit!(FieldId::CONTINUE_BLOCK, cont);
                }
            }
            NodeKind::For { init, condition, update, body, continue_block, .. } => {
                if let Some(i) = init {
                    $emit!(FieldId::INIT, i);
                }
                if let Some(c) = condition {
                    $emit!(FieldId::CONDITION, c);
                }
                if let Some(u) = update {
                    $emit!(FieldId::UPDATE, u);
                }
                $emit!(FieldId::BODY, body);
                if let Some(cont) = continue_block {
                    $emit!(FieldId::CONTINUE_BLOCK, cont);
                }
            }
            NodeKind::Foreach { variable, list, body, continue_block } => {
                $emit!(FieldId::VARIABLE, variable);
                $emit!(FieldId::LIST, list);
                $emit!(FieldId::BODY, body);
                if let Some(cb) = continue_block {
                    $emit!(FieldId::CONTINUE_BLOCK, cb);
                }
            }
            NodeKind::Given { expr, body } => {
                $emit!(FieldId::EXPR, expr);
                $emit!(FieldId::BODY, body);
            }
            NodeKind::When { condition, body } => {
                $emit!(FieldId::CONDITION, condition);
                $emit!(FieldId::BODY, body);
            }
            NodeKind::Default { body } => $emit!(FieldId::BODY, body),
            NodeKind::StatementModifier { statement, condition, .. } => {
                $emit!(FieldId::STATEMENT, statement);
                $emit!(FieldId::CONDITION, condition);
            }
            NodeKind::LabeledStatement { statement, .. } => $emit!(FieldId::STATEMENT, statement),

            // Eval and Do blocks
            NodeKind::Eval { block } => $emit!(FieldId::BLOCK, block),
            NodeKind::Do { block } => $emit!(FieldId::BLOCK, block),
            NodeKind::Defer { block } => $emit!(FieldId::BLOCK, block),
            NodeKind::Try { body, catch_blocks, finally_block } => {
                $emit!(FieldId::BODY, body);
                for (_, catch_body) in catch_blocks {
                    $emit!(FieldId::CATCH, catch_body);
                }
                if let Some(finally) = finally_block {
                    $emit!(FieldId::FINALLY, finally);
                }
            }

            // Function calls
            NodeKind::FunctionCall { args, .. } | NodeKind::AmperCall { args, .. } => {
                for arg in args {
                    $emit!(FieldId::ARGS, arg);
                }
            }
            NodeKind::MethodCall { object, args, .. } => {
                $emit!(FieldId::OBJECT, object);
                for arg in args {
                    $emit!(FieldId::ARGS, arg);
                }
            }
            NodeKind::IndirectCall { object, args, .. } => {
                $emit!(FieldId::OBJECT, object);
                for arg in args {
                    $emit!(FieldId::ARGS, arg);
                }
            }

            // Functions
            NodeKind::Subroutine { prototype, signature, body, .. } => {
                if let Some(proto) = prototype {
                    $emit!(FieldId::PROTOTYPE, proto);
                }
                if let Some(sig) = signature {
                    $emit!(FieldId::SIGNATURE, sig);
                }
                $emit!(FieldId::BODY, body);
            }
            NodeKind::Method { signature, body, .. } => {
                if let Some(sig) = signature {
                    $emit!(FieldId::SIGNATURE, sig);
                }
                $emit!(FieldId::BODY, body);
            }
            NodeKind::Return { value } => {
                if let Some(v) = value {
                    $emit!(FieldId::VALUE, v);
                }
            }
            NodeKind::Goto { target, .. } => $emit!(FieldId::TARGET, target),
            NodeKind::Signature { parameters } => {
                for param in parameters {
                    $emit!(FieldId::PARAMETERS, param);
                }
            }
            NodeKind::MandatoryParameter { variable } => $emit!(FieldId::VARIABLE, variable),
            NodeKind::OptionalParameter { variable, default_value } => {
                $emit!(FieldId::VARIABLE, variable);
                $emit!(FieldId::DEFAULT_VALUE, default_value);
            }
            NodeKind::SlurpyParameter { variable } => $emit!(FieldId::VARIABLE, variable),
            NodeKind::NamedParameter { variable, default_value, .. } => {
                $emit!(FieldId::VARIABLE, variable);
                if let Some(default) = default_value {
                    $emit!(FieldId::DEFAULT_VALUE, default);
                }
            }

            // Pattern matching
            NodeKind::Match { expr, .. } => $emit!(FieldId::EXPR, expr),
            NodeKind::Substitution { expr, .. } => $emit!(FieldId::EXPR, expr),
            NodeKind::Transliteration { expr, .. } => $emit!(FieldId::EXPR, expr),

            // Containers
            NodeKind::ArrayLiteral { elements } => {
                for elem in elements {
                    $emit!(FieldId::ELEMENTS, elem);
                }
            }
            NodeKind::HashLiteral { pairs } => {
                for (key, value) in pairs {
                    $emit!(FieldId::KEY, key);
                    $emit!(FieldId::VALUE, value);
                }
            }

            // Package system
            NodeKind::Package { block, .. } => {
                if let Some(b) = block {
                    $emit!(FieldId::BLOCK, b);
                }
            }
            NodeKind::PhaseBlock { block, .. } => $emit!(FieldId::BLOCK, block),
            NodeKind::Class { body, .. } => $emit!(FieldId::BODY, body),

            // Error node might have a partial valid tree
            NodeKind::Error { partial, .. } => {
                if let Some(node) = partial {
                    $emit!(FieldId::PARTIAL, node);
                }
            }

            // Leaf nodes (no children to traverse)
            NodeKind::Variable { .. }
            | NodeKind::Identifier { .. }
            | NodeKind::Number { .. }
            | NodeKind::String { .. }
            | NodeKind::VString { .. }
            | NodeKind::Heredoc { .. }
            | NodeKind::Regex { .. }
            | NodeKind::Readline { .. }
            | NodeKind::Glob { .. }
            | NodeKind::Typeglob { .. }
            | NodeKind::Diamond
            | NodeKind::Ellipsis
            | NodeKind::Undef
            | NodeKind::Use { .. }
            | NodeKind::No { .. }
            | NodeKind::Prototype { .. }
            | NodeKind::DataSection { .. }
            | NodeKind::Format { .. }
            | NodeKind::LoopControl { .. }
            | NodeKind::MissingExpression
            | NodeKind::MissingStatement
            | NodeKind::MissingIdentifier
            | NodeKind::MissingBlock
            | NodeKind::UnknownRest => {}
        }
    }};
}

impl Node {
    /// Visit direct children with short-circuiting and preserve their structural fields.
    ///
    /// `None` identifies an intentionally unnamed child. Repeated children in
    /// list-like fields use the same [`FieldId`] for each element.
    #[inline]
    pub fn try_for_each_child_with_field<'a, F, B>(&'a self, f: F) -> ControlFlow<B>
    where
        F: FnMut(Option<FieldId>, &'a Node) -> ControlFlow<B>,
    {
        self.try_for_each_child_with_field_observed(|_, _| {}, f)
    }

    /// Visit direct children with short-circuiting while observing each source pull.
    ///
    /// The observer runs inside child enumeration, immediately before the child
    /// is passed to `f`. This makes early-break behavior measurable without
    /// materializing an intermediate child collection.
    #[inline]
    pub fn try_for_each_child_with_field_observed<'a, P, F, B>(
        &'a self,
        mut observe_pull: P,
        mut f: F,
    ) -> ControlFlow<B>
    where
        P: FnMut(Option<FieldId>, &'a Node),
        F: FnMut(Option<FieldId>, &'a Node) -> ControlFlow<B>,
    {
        macro_rules! emit {
            ($field:expr, $child:expr) => {{
                observe_pull(Some($field), $child);
                if let ControlFlow::Break(b) = f(Some($field), $child) {
                    return ControlFlow::Break(b);
                }
            }};
        }
        visit_kind_children!(&self.kind, emit);
        ControlFlow::Continue(())
    }

    /// Mutably visit direct children with the same field sequence as the immutable walker.
    #[inline]
    pub fn try_for_each_child_mut_with_field<F, B>(&mut self, f: F) -> ControlFlow<B>
    where
        F: FnMut(Option<FieldId>, &mut Node) -> ControlFlow<B>,
    {
        self.try_for_each_child_mut_with_field_observed(|_, _| {}, f)
    }

    /// Mutably visit direct children while observing each source pull.
    ///
    /// Field identity, order, and short-circuiting match
    /// [`Self::try_for_each_child_with_field_observed`].
    #[inline]
    pub fn try_for_each_child_mut_with_field_observed<P, F, B>(
        &mut self,
        mut observe_pull: P,
        mut f: F,
    ) -> ControlFlow<B>
    where
        P: FnMut(Option<FieldId>, &mut Node),
        F: FnMut(Option<FieldId>, &mut Node) -> ControlFlow<B>,
    {
        macro_rules! emit {
            ($field:expr, $child:expr) => {{
                observe_pull(Some($field), $child);
                if let ControlFlow::Break(b) = f(Some($field), $child) {
                    return ControlFlow::Break(b);
                }
            }};
        }
        visit_kind_children!(&mut self.kind, emit);
        ControlFlow::Continue(())
    }

    /// Call a function on every direct child, preserving its structural field.
    #[inline]
    pub fn for_each_child_with_field<'a, F: FnMut(Option<FieldId>, &'a Node)>(&'a self, mut f: F) {
        let _ = self.try_for_each_child_with_field(|field, child| {
            f(field, child);
            ControlFlow::<()>::Continue(())
        });
    }

    /// Mutably visit every direct child, preserving its structural field.
    #[inline]
    pub fn for_each_child_mut_with_field<F: FnMut(Option<FieldId>, &mut Node)>(
        &mut self,
        mut f: F,
    ) {
        let _ = self.try_for_each_child_mut_with_field(|field, child| {
            f(field, child);
            ControlFlow::<()>::Continue(())
        });
    }

    /// Call a function on every direct child without field metadata.
    #[inline]
    pub fn for_each_child<'a, F: FnMut(&'a Node)>(&'a self, mut f: F) {
        self.for_each_child_with_field(|_, child| f(child));
    }

    /// Call a function on every direct child node of this node.
    ///
    /// This is a compatibility wrapper over [`Self::try_for_each_child_mut_with_field`].
    /// Field identity is dropped; order and short-circuit-free visitation match
    /// the field-aware mutable walker.
    #[inline]
    pub fn for_each_child_mut<F: FnMut(&mut Node)>(&mut self, mut f: F) {
        let _ = self.try_for_each_child_mut_with_field(|_, child| {
            f(child);
            ControlFlow::<()>::Continue(())
        });
    }
}
