//! Conservative expression predicates for tautology detection.
//!
//! When purity, Option/Result identity, or PartialEq reflexivity cannot be
//! proven from syntax (prelude or std/core constructors and ascriptions), the
//! checker skips rather than emitting a finding. A terminal identifier such as
//! `custom::Option` is not a proven std enum. Bare prelude names are refused
//! when the current module declares or imports a shadowing Option/Result or
//! Some/None/Ok/Err. Qualified `std::`/`core::` enum paths are refused when
//! that namespace is a local module or import alias in the resolving scope.

use std::collections::{BTreeMap, BTreeSet};
use syn::{Expr, Lit, Pat, Path, Type, UnOp};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum QueryKind {
    Option,
    Result,
}

/// Module-level names that shadow prelude Option/Result, Some/None/Ok/Err, or
/// the `std`/`core` namespaces those qualified paths depend on.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct PreludeShadow {
    types: BTreeSet<QueryKind>,
    ctors: BTreeSet<String>,
    std_untrusted: bool,
    core_untrusted: bool,
}

impl PreludeShadow {
    pub(crate) fn untrust_type(&mut self, kind: QueryKind) {
        self.types.insert(kind);
    }

    pub(crate) fn untrust_ctor(&mut self, name: impl Into<String>) {
        self.ctors.insert(name.into());
    }

    pub(crate) fn untrust_namespace(&mut self, name: &str) {
        match name {
            "std" => self.std_untrusted = true,
            "core" => self.core_untrusted = true,
            _ => {}
        }
    }

    pub(crate) fn untrust_all_prelude(&mut self) {
        self.untrust_type(QueryKind::Option);
        self.untrust_type(QueryKind::Result);
        for name in ["Some", "None", "Ok", "Err"] {
            self.untrust_ctor(name);
        }
    }

    fn type_untrusted(&self, kind: QueryKind) -> bool {
        self.types.contains(&kind)
    }

    fn ctor_untrusted(&self, name: &str) -> bool {
        self.ctors.contains(name)
    }

    pub(crate) fn namespace_untrusted(&self, name: &str) -> bool {
        match name {
            "std" => self.std_untrusted,
            "core" => self.core_untrusted,
            _ => false,
        }
    }
}

/// Lexical Option/Result ascriptions. Inner scopes shadow outer names, including
/// with an explicit unknown binding so an untyped `let` hides a prior Option.
/// `shadows` is the current module; `crate_root_shadows` governs `::std`/`::core`.
#[derive(Debug, Clone)]
pub(crate) struct TypeEnv {
    scopes: Vec<BTreeMap<String, Option<QueryKind>>>,
    shadows: PreludeShadow,
    crate_root_shadows: PreludeShadow,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            shadows: PreludeShadow::default(),
            crate_root_shadows: PreludeShadow::default(),
        }
    }
}

impl TypeEnv {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_shadows(shadows: PreludeShadow) -> Self {
        Self {
            scopes: vec![BTreeMap::new()],
            shadows,
            crate_root_shadows: PreludeShadow::default(),
        }
    }

    pub(crate) fn with_module_and_crate_root(
        module: PreludeShadow,
        crate_root: PreludeShadow,
    ) -> Self {
        Self { scopes: vec![BTreeMap::new()], shadows: module, crate_root_shadows: crate_root }
    }

    pub(crate) fn type_untrusted(&self, kind: QueryKind) -> bool {
        self.shadows.type_untrusted(kind)
    }

    pub(crate) fn ctor_untrusted(&self, name: &str) -> bool {
        self.shadows.ctor_untrusted(name)
    }

    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub(crate) fn bind(&mut self, ident: String, kind: QueryKind) {
        self.shadow(ident, Some(kind));
    }

    /// Record a binding in the current scope. `None` means the name is bound
    /// but not a proven Option/Result, so it hides an outer ascription.
    pub(crate) fn shadow(&mut self, ident: String, kind: Option<QueryKind>) {
        if self.scopes.is_empty() {
            self.scopes.push(BTreeMap::new());
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(ident, kind);
        }
    }

    pub(crate) fn kind_of(&self, ident: &str) -> Option<QueryKind> {
        for scope in self.scopes.iter().rev() {
            if let Some(kind) = scope.get(ident) {
                return *kind;
            }
        }
        None
    }
}

pub(crate) fn peel(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => peel(&paren.expr),
        Expr::Group(group) => peel(&group.expr),
        other => other,
    }
}

pub(crate) fn expr_eq(left: &Expr, right: &Expr) -> bool {
    peel(left) == peel(right)
}

/// Returns true only for expressions the checker treats as free of observable
/// evaluation effects. Function calls, arbitrary methods, unary dereference,
/// and field access are excluded: they can run user `Deref`/`Index` code.
/// Standard Option/Result queries are included only when the receiver is a
/// proven Option/Result.
pub(crate) fn is_side_effect_free(expr: &Expr, env: &TypeEnv) -> bool {
    match peel(expr) {
        Expr::Path(_) | Expr::Lit(_) | Expr::Const(_) => true,
        Expr::Reference(reference) => is_side_effect_free(&reference.expr, env),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_) | UnOp::Neg(_)) => {
            is_side_effect_free(&unary.expr, env)
        }
        Expr::Tuple(tuple) => tuple.elems.iter().all(|elem| is_side_effect_free(elem, env)),
        Expr::Array(array) => array.elems.iter().all(|elem| is_side_effect_free(elem, env)),
        Expr::Repeat(repeat) => {
            is_side_effect_free(&repeat.expr, env) && is_side_effect_free(&repeat.len, env)
        }
        Expr::Struct(strct)
            if strct.qself.is_none()
                && strct.dot2_token.is_none()
                && strct.rest.is_none()
                && strct.fields.iter().all(|field| is_side_effect_free(&field.expr, env)) =>
        {
            true
        }
        Expr::Cast(cast) => is_side_effect_free(&cast.expr, env),
        Expr::MethodCall(call)
            if call.args.is_empty()
                && call.turbofish.is_none()
                && proven_query_kind(&call.receiver, env).is_some_and(|kind| {
                    matches!(
                        (kind, call.method.to_string().as_str()),
                        (QueryKind::Option, "is_some" | "is_none")
                            | (QueryKind::Result, "is_ok" | "is_err")
                    )
                }) =>
        {
            true
        }
        _ => false,
    }
}

/// Option/Result identity proven from a constructor or an explicit ascription.
/// Ascriptions and constructor owners must be the prelude `Option`/`Result` or
/// a `std`/`core` path; `custom::Option` is not admitted.
pub(crate) fn proven_query_kind(expr: &Expr, env: &TypeEnv) -> Option<QueryKind> {
    let expr = peel(expr);
    if let Some(kind) = constructor_query_kind(expr, env) {
        return Some(kind);
    }
    let Expr::Path(path) = expr else {
        return None;
    };
    simple_ident(&path.path).and_then(|ident| env.kind_of(&ident))
}

pub(crate) fn bind_pat_type(env: &mut TypeEnv, pat: &Pat, ty: &Type) {
    match pat {
        Pat::Type(typed) => bind_pat_type(env, &typed.pat, &typed.ty),
        Pat::Ident(ident) => {
            env.shadow(ident.ident.to_string(), option_or_result_kind_in(ty, env));
            if let Some((_, subpat)) = &ident.subpat {
                bind_unknown_pat(env, subpat);
            }
        }
        Pat::Reference(reference) => bind_pat_type(env, &reference.pat, ty),
        other => bind_unknown_pat(env, other),
    }
}

/// Bind a pattern, recording Option/Result only when the pattern is ascribed.
/// Untyped names shadow any outer ascription as unknown.
pub(crate) fn bind_binding_pat(env: &mut TypeEnv, pat: &Pat) {
    match pat {
        Pat::Type(typed) => bind_pat_type(env, &typed.pat, &typed.ty),
        other => bind_unknown_pat(env, other),
    }
}

fn bind_unknown_pat(env: &mut TypeEnv, pat: &Pat) {
    match pat {
        Pat::Type(typed) => bind_pat_type(env, &typed.pat, &typed.ty),
        Pat::Ident(ident) => {
            env.shadow(ident.ident.to_string(), None);
            if let Some((_, subpat)) = &ident.subpat {
                bind_unknown_pat(env, subpat);
            }
        }
        Pat::Reference(reference) => bind_unknown_pat(env, &reference.pat),
        Pat::Paren(paren) => bind_unknown_pat(env, &paren.pat),
        Pat::Guard(guard) => bind_unknown_pat(env, &guard.pat),
        Pat::Or(or_pat) => {
            for case in &or_pat.cases {
                bind_unknown_pat(env, case);
            }
        }
        Pat::Tuple(tuple) => {
            for elem in &tuple.elems {
                bind_unknown_pat(env, elem);
            }
        }
        Pat::TupleStruct(tuple) => {
            for elem in &tuple.elems {
                bind_unknown_pat(env, elem);
            }
        }
        Pat::Slice(slice) => {
            for elem in &slice.elems {
                bind_unknown_pat(env, elem);
            }
        }
        Pat::Struct(strct) => {
            for field in &strct.fields {
                bind_unknown_pat(env, &field.pat);
            }
        }
        _ => {}
    }
}

pub(crate) fn option_or_result_kind(ty: &Type) -> Option<QueryKind> {
    option_or_result_kind_in(ty, &TypeEnv::new())
}

pub(crate) fn option_or_result_kind_in(ty: &Type, env: &TypeEnv) -> Option<QueryKind> {
    let ty = peel_type(ty);
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let kind = std_enum_kind(&path.path, env)?;
    if path.path.leading_colon.is_none()
        && path.path.segments.len() == 1
        && env.type_untrusted(kind)
    {
        return None;
    }
    Some(kind)
}

fn peel_type(ty: &Type) -> &Type {
    match ty {
        Type::Paren(paren) => peel_type(&paren.elem),
        Type::Group(group) => peel_type(&group.elem),
        Type::Reference(reference) => peel_type(&reference.elem),
        other => other,
    }
}

fn constructor_query_kind(expr: &Expr, env: &TypeEnv) -> Option<QueryKind> {
    match peel(expr) {
        Expr::Call(call) => {
            if !call.args.iter().all(|arg| is_side_effect_free(arg, env)) {
                return None;
            }
            let Expr::Path(path) = peel(&*call.func) else {
                return None;
            };
            if path.qself.is_some() {
                return None;
            }
            ctor_path_kind(&path.path, env)
        }
        Expr::Path(path) if path.qself.is_none() => none_path_kind(&path.path, env),
        _ => None,
    }
}

/// Proven std/core Option or Result path. A terminal identifier is not enough:
/// `custom::Option` and `crate::Result` are not the prelude or std enums.
/// Bare `Option`/`Result` (no leading `::`) are the prelude names unless the
/// current module shadows them; `::Option` is crate-root and is refused.
/// Qualified `std::`/`core::` paths are refused when that namespace is a local
/// module or alias in the resolving scope (`::std` uses crate-root shadows).
fn std_enum_kind(path: &Path, env: &TypeEnv) -> Option<QueryKind> {
    trusted_std_enum_kind(path.leading_colon.is_some(), &path_idents(path), env)
}

fn trusted_std_enum_kind(rooted: bool, segs: &[String], env: &TypeEnv) -> Option<QueryKind> {
    if std_path_namespace_untrusted(rooted, segs, &env.shadows, &env.crate_root_shadows) {
        return None;
    }
    std_enum_kind_from_idents(rooted, segs)
}

fn none_path_kind(path: &Path, env: &TypeEnv) -> Option<QueryKind> {
    let last = path.segments.last()?.ident.to_string();
    (last == "None").then(|| ctor_path_kind(path, env)).flatten()
}

fn ctor_path_kind(path: &Path, env: &TypeEnv) -> Option<QueryKind> {
    let segs = path_idents(path);
    let last = segs.last()?.as_str();
    let rooted = path.leading_colon.is_some();
    match last {
        "Some" | "None" => ctor_kind_for_owner(rooted, &segs, last, QueryKind::Option, env),
        "Ok" | "Err" => ctor_kind_for_owner(rooted, &segs, last, QueryKind::Result, env),
        _ => None,
    }
}

fn ctor_kind_for_owner(
    rooted: bool,
    segs: &[String],
    last: &str,
    expected: QueryKind,
    env: &TypeEnv,
) -> Option<QueryKind> {
    if segs.len() == 1 {
        if rooted || env.ctor_untrusted(last) {
            return None;
        }
        return Some(expected);
    }
    let owner = trusted_std_enum_kind(rooted, &segs[..segs.len() - 1], env)?;
    if owner != expected {
        return None;
    }
    if segs.len() == 2 && !rooted && env.type_untrusted(expected) {
        return None;
    }
    Some(expected)
}

fn path_idents(path: &Path) -> Vec<String> {
    path.segments.iter().map(|segment| segment.ident.to_string()).collect()
}

pub(crate) fn std_enum_kind_from_idents(rooted: bool, segs: &[String]) -> Option<QueryKind> {
    let names: Vec<&str> = segs.iter().map(String::as_str).collect();
    match names.as_slice() {
        ["Option"] if !rooted => Some(QueryKind::Option),
        ["Result"] if !rooted => Some(QueryKind::Result),
        ["std", "option", "Option"] | ["core", "option", "Option"] => Some(QueryKind::Option),
        ["std", "result", "Result"] | ["core", "result", "Result"] => Some(QueryKind::Result),
        _ => None,
    }
}

pub(crate) fn imported_path_is_std_enum_or_ctor(
    rooted: bool,
    segs: &[String],
    module: &PreludeShadow,
    crate_root: &PreludeShadow,
) -> bool {
    if std_path_namespace_untrusted(rooted, segs, module, crate_root) {
        return false;
    }
    if std_enum_kind_from_idents(rooted, segs).is_some() {
        return true;
    }
    let Some(last) = segs.last() else {
        return false;
    };
    match last.as_str() {
        "Some" | "None" => {
            segs.len() == 1 && !rooted
                || std_enum_kind_from_idents(rooted, &segs[..segs.len() - 1])
                    == Some(QueryKind::Option)
        }
        "Ok" | "Err" => {
            segs.len() == 1 && !rooted
                || std_enum_kind_from_idents(rooted, &segs[..segs.len() - 1])
                    == Some(QueryKind::Result)
        }
        _ => false,
    }
}

pub(crate) fn import_prefix_is_std_namespace(
    rooted: bool,
    prefix: &[String],
    module: &PreludeShadow,
    crate_root: &PreludeShadow,
) -> bool {
    if std_path_namespace_untrusted(rooted, prefix, module, crate_root) {
        return false;
    }
    let names: Vec<&str> = prefix.iter().map(String::as_str).collect();
    matches!(
        names.as_slice(),
        ["std"]
            | ["core"]
            | ["std", "option"]
            | ["core", "option"]
            | ["std", "result"]
            | ["core", "result"]
    )
}

/// Relative `std::`/`core::` paths use the current module; `::std`/`::core`
/// use crate-root namespace shadows.
pub(crate) fn std_path_namespace_untrusted(
    rooted: bool,
    segs: &[String],
    module: &PreludeShadow,
    crate_root: &PreludeShadow,
) -> bool {
    let Some(first) = segs.first() else {
        return false;
    };
    let shadows = if rooted { crate_root } else { module };
    shadows.namespace_untrusted(first)
}

fn simple_ident(path: &Path) -> Option<String> {
    if path.segments.len() != 1 || path.leading_colon.is_some() {
        return None;
    }
    path.segments.first().map(|segment| segment.ident.to_string())
}

/// Returns true only when PartialEq is known to be reflexive without types.
///
/// Paths, fields, struct literals, and associated constants such as `f32::NAN`
/// are excluded: they may be non-reflexive or have a custom `PartialEq`.
pub(crate) fn is_known_reflexive_eq_operand(expr: &Expr) -> bool {
    match peel(expr) {
        Expr::Lit(lit) => lit_is_known_reflexive(&lit.lit),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => {
            is_known_reflexive_eq_operand(&unary.expr)
        }
        Expr::Reference(reference) if reference.mutability.is_none() => {
            is_known_reflexive_eq_operand(&reference.expr)
        }
        Expr::Tuple(tuple) => tuple.elems.iter().all(is_known_reflexive_eq_operand),
        Expr::Array(array) => array.elems.iter().all(is_known_reflexive_eq_operand),
        Expr::Repeat(repeat) => {
            is_known_reflexive_eq_operand(&repeat.expr)
                && is_known_reflexive_eq_operand(&repeat.len)
        }
        Expr::Cast(cast) => is_known_reflexive_eq_operand(&cast.expr),
        _ => false,
    }
}

fn lit_is_known_reflexive(lit: &Lit) -> bool {
    matches!(
        lit,
        Lit::Int(_)
            | Lit::Bool(_)
            | Lit::Str(_)
            | Lit::ByteStr(_)
            | Lit::Byte(_)
            | Lit::Char(_)
            | Lit::CStr(_)
            | Lit::Float(_)
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{
        PreludeShadow, QueryKind, TypeEnv, is_known_reflexive_eq_operand, is_side_effect_free,
        option_or_result_kind, option_or_result_kind_in, proven_query_kind,
    };
    use syn::{Expr, Type, parse_str};

    fn expr(src: &str) -> Expr {
        parse_str(src).unwrap_or_else(|error| panic!("parse `{src}`: {error}"))
    }

    fn ty(src: &str) -> Type {
        parse_str(src).unwrap_or_else(|error| panic!("parse type `{src}`: {error}"))
    }

    #[test]
    fn function_calls_are_not_side_effect_free() {
        let env = TypeEnv::new();
        assert!(!is_side_effect_free(&expr("counter()"), &env));
        assert!(!is_side_effect_free(&expr(r#"sanitize_completion_path_input("x")"#), &env));
        assert!(!is_side_effect_free(&expr("iter.next()"), &env));
    }

    #[test]
    fn untyped_option_queries_are_not_side_effect_free() {
        let env = TypeEnv::new();
        assert!(is_side_effect_free(&expr("value"), &env));
        assert!(!is_side_effect_free(&expr("value.is_some()"), &env));
        assert!(!is_side_effect_free(&expr("result.is_err()"), &env));
    }

    #[test]
    fn typed_option_queries_are_side_effect_free() {
        let mut env = TypeEnv::new();
        env.bind("value".to_string(), QueryKind::Option);
        env.bind("result".to_string(), QueryKind::Result);
        assert!(is_side_effect_free(&expr("value.is_some()"), &env));
        assert!(is_side_effect_free(&expr("result.is_err()"), &env));
        assert!(proven_query_kind(&expr("value"), &env) == Some(QueryKind::Option));
        assert!(proven_query_kind(&expr("Some(1)"), &env) == Some(QueryKind::Option));
        assert!(proven_query_kind(&expr("Ok(())"), &env) == Some(QueryKind::Result));
        assert!(proven_query_kind(&expr("None"), &env) == Some(QueryKind::Option));
        assert!(proven_query_kind(&expr("probe"), &env).is_none());
    }

    #[test]
    fn ascriptions_identify_option_and_result() {
        assert_eq!(option_or_result_kind(&ty("Option<u8>")), Some(QueryKind::Option));
        assert_eq!(option_or_result_kind(&ty("&Result<(), ()>")), Some(QueryKind::Result));
        assert_eq!(option_or_result_kind(&ty("Probe")), None);
        assert_eq!(option_or_result_kind(&ty("std::option::Option<u8>")), Some(QueryKind::Option));
        assert_eq!(option_or_result_kind(&ty("core::option::Option<u8>")), Some(QueryKind::Option));
        assert_eq!(
            option_or_result_kind(&ty("::std::option::Option<u8>")),
            Some(QueryKind::Option)
        );
        assert_eq!(
            option_or_result_kind(&ty("std::result::Result<(), ()>")),
            Some(QueryKind::Result)
        );
        assert_eq!(
            option_or_result_kind(&ty("core::result::Result<(), ()>")),
            Some(QueryKind::Result)
        );
        assert_eq!(option_or_result_kind(&ty("custom::Option<u8>")), None);
        assert_eq!(option_or_result_kind(&ty("custom::Result<(), ()>")), None);
        assert_eq!(option_or_result_kind(&ty("crate::Option<u8>")), None);
        assert_eq!(option_or_result_kind(&ty("super::Option<u8>")), None);
        assert_eq!(option_or_result_kind(&ty("self::Result<(), ()>")), None);
        assert_eq!(option_or_result_kind(&ty("::Option<u8>")), None);
        assert_eq!(option_or_result_kind(&ty("option::Option<u8>")), None);
        assert_eq!(option_or_result_kind(&ty("std::Option<u8>")), None);
    }

    #[test]
    fn local_prelude_shadows_refuse_bare_option_result() {
        let mut shadows = PreludeShadow::default();
        shadows.untrust_type(QueryKind::Option);
        shadows.untrust_ctor("Some");
        let env = TypeEnv::with_shadows(shadows);
        assert_eq!(option_or_result_kind_in(&ty("Option<u8>"), &env), None);
        assert_eq!(
            option_or_result_kind_in(&ty("std::option::Option<u8>"), &env),
            Some(QueryKind::Option)
        );
        assert_eq!(proven_query_kind(&expr("Some(1)"), &env), None);
        assert_eq!(
            proven_query_kind(&expr("std::option::Option::Some(1)"), &env),
            Some(QueryKind::Option)
        );
        assert_eq!(proven_query_kind(&expr("Option::Some(1)"), &env), None);
    }

    #[test]
    fn local_std_and_core_namespaces_refuse_qualified_std_paths() {
        let mut std_shadow = PreludeShadow::default();
        std_shadow.untrust_namespace("std");
        let env = TypeEnv::with_shadows(std_shadow.clone());
        assert_eq!(option_or_result_kind_in(&ty("std::option::Option<u8>"), &env), None);
        assert_eq!(option_or_result_kind_in(&ty("Option<u8>"), &env), Some(QueryKind::Option));
        assert_eq!(
            option_or_result_kind_in(&ty("::std::option::Option<u8>"), &env),
            Some(QueryKind::Option)
        );
        assert_eq!(proven_query_kind(&expr("std::option::Option::Some(1)"), &env), None);

        let env_rooted = TypeEnv::with_module_and_crate_root(PreludeShadow::default(), std_shadow);
        assert_eq!(option_or_result_kind_in(&ty("::std::option::Option<u8>"), &env_rooted), None);
        assert_eq!(
            option_or_result_kind_in(&ty("std::option::Option<u8>"), &env_rooted),
            Some(QueryKind::Option)
        );

        let mut core_shadow = PreludeShadow::default();
        core_shadow.untrust_namespace("core");
        let env_core = TypeEnv::with_shadows(core_shadow);
        assert_eq!(option_or_result_kind_in(&ty("core::option::Option<u8>"), &env_core), None);
        assert_eq!(option_or_result_kind_in(&ty("core::result::Result<(), ()>"), &env_core), None);
    }

    #[test]
    fn constructors_require_prelude_or_std_owners() {
        let env = TypeEnv::new();
        assert_eq!(proven_query_kind(&expr("Some(1)"), &env), Some(QueryKind::Option));
        assert_eq!(proven_query_kind(&expr("None"), &env), Some(QueryKind::Option));
        assert_eq!(proven_query_kind(&expr("Ok(())"), &env), Some(QueryKind::Result));
        assert_eq!(proven_query_kind(&expr("Err(())"), &env), Some(QueryKind::Result));
        assert_eq!(proven_query_kind(&expr("Option::Some(1)"), &env), Some(QueryKind::Option));
        assert_eq!(
            proven_query_kind(&expr("std::option::Option::Some(1)"), &env),
            Some(QueryKind::Option)
        );
        assert_eq!(
            proven_query_kind(&expr("core::option::Option::None"), &env),
            Some(QueryKind::Option)
        );
        assert_eq!(
            proven_query_kind(&expr("std::result::Result::Ok(())"), &env),
            Some(QueryKind::Result)
        );
        assert_eq!(proven_query_kind(&expr("custom::Option::Some(1)"), &env), None);
        assert_eq!(proven_query_kind(&expr("custom::Option::None"), &env), None);
        assert_eq!(proven_query_kind(&expr("custom::Result::Ok(())"), &env), None);
        assert_eq!(proven_query_kind(&expr("custom::Some(1)"), &env), None);
        assert_eq!(proven_query_kind(&expr("::Some(1)"), &env), None);
        assert_eq!(proven_query_kind(&expr("::None"), &env), None);
    }

    #[test]
    fn nan_and_untyped_paths_are_not_known_reflexive() {
        assert!(!is_known_reflexive_eq_operand(&expr("f32::NAN")));
        assert!(!is_known_reflexive_eq_operand(&expr("f64::NAN")));
        assert!(!is_known_reflexive_eq_operand(&expr("value")));
        assert!(!is_known_reflexive_eq_operand(&expr("item.flag")));
        assert!(!is_known_reflexive_eq_operand(&expr("RecoverySite::ArgList")));
        assert!(!is_known_reflexive_eq_operand(&expr("TransportMode::Socket { port: 100 }")));
        assert!(!is_known_reflexive_eq_operand(&expr("-f32::NAN")));
        assert!(!is_known_reflexive_eq_operand(&expr("[path; 3]")));
    }

    #[test]
    fn deref_and_field_access_are_not_side_effect_free() {
        let env = TypeEnv::new();
        assert!(!is_side_effect_free(&expr("*value"), &env));
        assert!(!is_side_effect_free(&expr("item.flag"), &env));
        assert!(!is_side_effect_free(&expr("!item.flag"), &env));
        assert!(is_side_effect_free(&expr("ready"), &env));
        assert!(is_side_effect_free(&expr("!ready"), &env));
    }

    #[test]
    fn shadowing_and_scopes_restore_option_identity() {
        let mut env = TypeEnv::new();
        env.bind("value".to_string(), QueryKind::Option);
        assert_eq!(env.kind_of("value"), Some(QueryKind::Option));
        env.push_scope();
        env.shadow("value".to_string(), None);
        assert_eq!(env.kind_of("value"), None);
        env.pop_scope();
        assert_eq!(env.kind_of("value"), Some(QueryKind::Option));
        env.shadow("value".to_string(), None);
        assert_eq!(env.kind_of("value"), None);
    }

    #[test]
    fn literals_tuples_and_immutable_refs_are_known_reflexive() {
        assert!(is_known_reflexive_eq_operand(&expr("1")));
        assert!(is_known_reflexive_eq_operand(&expr("1u8")));
        assert!(is_known_reflexive_eq_operand(&expr("true")));
        assert!(is_known_reflexive_eq_operand(&expr("\"ok\"")));
        assert!(is_known_reflexive_eq_operand(&expr("'a'")));
        assert!(is_known_reflexive_eq_operand(&expr("-1")));
        assert!(is_known_reflexive_eq_operand(&expr("(1)")));
        assert!(is_known_reflexive_eq_operand(&expr("&1")));
        assert!(is_known_reflexive_eq_operand(&expr("(1, true)")));
        assert!(is_known_reflexive_eq_operand(&expr("1.0")));
        assert!(is_known_reflexive_eq_operand(&expr("1 as i32")));
        assert!(is_known_reflexive_eq_operand(&expr("[1, 2]")));
        assert!(is_known_reflexive_eq_operand(&expr("[0; 3]")));
    }
}
