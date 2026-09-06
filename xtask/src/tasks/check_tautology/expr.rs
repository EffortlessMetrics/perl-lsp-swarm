//! Conservative expression predicates for tautology detection.
//!
//! When purity, Option/Result identity, or PartialEq reflexivity cannot be
//! proven from syntax (constructors or explicit ascriptions), the checker
//! skips rather than emitting a finding.

use std::collections::BTreeMap;
use syn::{Expr, Lit, Pat, Path, Type, UnOp};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum QueryKind {
    Option,
    Result,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TypeEnv {
    bindings: BTreeMap<String, QueryKind>,
}

impl TypeEnv {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn bind(&mut self, ident: String, kind: QueryKind) {
        self.bindings.insert(ident, kind);
    }

    pub(crate) fn kind_of(&self, ident: &str) -> Option<QueryKind> {
        self.bindings.get(ident).copied()
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
/// evaluation effects. Function and arbitrary method calls are excluded.
/// Standard Option/Result queries are included only when the receiver is a
/// proven Option/Result.
pub(crate) fn is_side_effect_free(expr: &Expr, env: &TypeEnv) -> bool {
    match peel(expr) {
        Expr::Path(_) | Expr::Lit(_) | Expr::Const(_) => true,
        Expr::Reference(reference) => is_side_effect_free(&reference.expr, env),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_) | UnOp::Deref(_) | UnOp::Neg(_)) => {
            is_side_effect_free(&unary.expr, env)
        }
        Expr::Field(field) => is_side_effect_free(&field.base, env),
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
pub(crate) fn proven_query_kind(expr: &Expr, env: &TypeEnv) -> Option<QueryKind> {
    let expr = peel(expr);
    if let Some(kind) = constructor_query_kind(expr) {
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
            if let Some(kind) = option_or_result_kind(ty) {
                env.bind(ident.ident.to_string(), kind);
            }
        }
        Pat::Reference(reference) => bind_pat_type(env, &reference.pat, ty),
        _ => {}
    }
}

pub(crate) fn option_or_result_kind(ty: &Type) -> Option<QueryKind> {
    let ty = peel_type(ty);
    let Type::Path(path) = ty else {
        return None;
    };
    path_query_kind(&path.path)
}

fn peel_type(ty: &Type) -> &Type {
    match ty {
        Type::Paren(paren) => peel_type(&paren.elem),
        Type::Group(group) => peel_type(&group.elem),
        Type::Reference(reference) => peel_type(&reference.elem),
        other => other,
    }
}

fn constructor_query_kind(expr: &Expr) -> Option<QueryKind> {
    match peel(expr) {
        Expr::Call(call) => {
            if !call.args.iter().all(|arg| is_side_effect_free(arg, &TypeEnv::new())) {
                return None;
            }
            let Expr::Path(path) = peel(&*call.func) else {
                return None;
            };
            ctor_path_kind(&path.path)
        }
        Expr::Path(path) => none_path_kind(&path.path),
        _ => None,
    }
}

fn path_query_kind(path: &Path) -> Option<QueryKind> {
    let last = path.segments.last()?;
    match last.ident.to_string().as_str() {
        "Option" => Some(QueryKind::Option),
        "Result" => Some(QueryKind::Result),
        _ => None,
    }
}

fn ctor_path_kind(path: &Path) -> Option<QueryKind> {
    let last = path.segments.last()?.ident.to_string();
    match last.as_str() {
        "Some" if constructor_owner_is(path, "Option") => Some(QueryKind::Option),
        "Ok" | "Err" if constructor_owner_is(path, "Result") => Some(QueryKind::Result),
        _ => None,
    }
}

fn none_path_kind(path: &Path) -> Option<QueryKind> {
    let last = path.segments.last()?.ident.to_string();
    (last == "None" && constructor_owner_is(path, "Option")).then_some(QueryKind::Option)
}

fn constructor_owner_is(path: &Path, owner: &str) -> bool {
    match path.segments.len() {
        1 => true,
        n if n >= 2 => {
            path.segments.iter().nth(n.saturating_sub(2)).is_some_and(|seg| seg.ident == owner)
        }
        _ => false,
    }
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
        QueryKind, TypeEnv, is_known_reflexive_eq_operand, is_side_effect_free,
        option_or_result_kind, proven_query_kind,
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
