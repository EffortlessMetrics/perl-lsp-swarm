//! Conservative expression predicates for tautology detection.
//!
//! These helpers are type-blind. When purity or PartialEq reflexivity cannot be
//! proven from syntax alone, the checker skips rather than emitting a finding.

use syn::{Expr, Lit, UnOp};

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
pub(crate) fn is_side_effect_free(expr: &Expr) -> bool {
    match peel(expr) {
        Expr::Path(_) | Expr::Lit(_) | Expr::Const(_) => true,
        Expr::Reference(reference) => is_side_effect_free(&reference.expr),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_) | UnOp::Deref(_) | UnOp::Neg(_)) => {
            is_side_effect_free(&unary.expr)
        }
        Expr::Field(field) => is_side_effect_free(&field.base),
        Expr::Tuple(tuple) => tuple.elems.iter().all(is_side_effect_free),
        Expr::Array(array) => array.elems.iter().all(is_side_effect_free),
        Expr::Repeat(repeat) => {
            is_side_effect_free(&repeat.expr) && is_side_effect_free(&repeat.len)
        }
        Expr::Struct(strct)
            if strct.qself.is_none()
                && strct.dot2_token.is_none()
                && strct.rest.is_none()
                && strct.fields.iter().all(|field| is_side_effect_free(&field.expr)) =>
        {
            true
        }
        Expr::Cast(cast) => is_side_effect_free(&cast.expr),
        Expr::MethodCall(call)
            if call.args.is_empty()
                && call.turbofish.is_none()
                && (call.method == "is_some"
                    || call.method == "is_none"
                    || call.method == "is_ok"
                    || call.method == "is_err") =>
        {
            is_side_effect_free(&call.receiver)
        }
        _ => false,
    }
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

    use super::{is_known_reflexive_eq_operand, is_side_effect_free};
    use syn::{Expr, parse_str};

    fn expr(src: &str) -> Expr {
        parse_str(src).unwrap_or_else(|error| panic!("parse `{src}`: {error}"))
    }

    #[test]
    fn function_calls_are_not_side_effect_free() {
        assert!(!is_side_effect_free(&expr("counter()")));
        assert!(!is_side_effect_free(&expr(r#"sanitize_completion_path_input("x")"#)));
        assert!(!is_side_effect_free(&expr("iter.next()")));
    }

    #[test]
    fn paths_fields_and_option_queries_are_side_effect_free() {
        assert!(is_side_effect_free(&expr("value")));
        assert!(is_side_effect_free(&expr("item.flag")));
        assert!(is_side_effect_free(&expr("value.is_some()")));
        assert!(is_side_effect_free(&expr("result.is_err()")));
    }

    #[test]
    fn nan_and_untyped_paths_are_not_known_reflexive() {
        assert!(!is_known_reflexive_eq_operand(&expr("f32::NAN")));
        assert!(!is_known_reflexive_eq_operand(&expr("f64::NAN")));
        assert!(!is_known_reflexive_eq_operand(&expr("value")));
        assert!(!is_known_reflexive_eq_operand(&expr("item.flag")));
        assert!(!is_known_reflexive_eq_operand(&expr("RecoverySite::ArgList")));
        assert!(!is_known_reflexive_eq_operand(&expr("TransportMode::Socket { port: 100 }")));
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
    }
}
