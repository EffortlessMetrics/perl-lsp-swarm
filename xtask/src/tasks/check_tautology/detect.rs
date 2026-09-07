//! Conservative structural tautology detection for assertion expressions.
//!
//! False negatives are acceptable. False positives are not. Patterns that
//! require type inference or purity analysis of arbitrary calls are skipped,
//! except for the explicitly governed Option/Result method pairs whose
//! receivers match after paren-normalization.

use syn::spanned::Spanned;
use syn::{BinOp, Expr, UnOp};

/// Stable rule identifiers emitted in findings and dispositions.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum RuleId {
    OptionSomeOrNone,
    ResultOkOrErr,
    PredicateOrNegation,
    /// Retained for disposition compatibility. `expr || expr` is not emitted:
    /// it is equivalent to `expr`, not to `true`.
    IdenticalOrAlternatives,
    AssertEqIdentical,
}

impl RuleId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OptionSomeOrNone => "option-is-some-or-none",
            Self::ResultOkOrErr => "result-is-ok-or-err",
            Self::PredicateOrNegation => "predicate-or-negation",
            Self::IdenticalOrAlternatives => "identical-or-alternatives",
            Self::AssertEqIdentical => "assert-eq-identical",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "option-is-some-or-none" => Some(Self::OptionSomeOrNone),
            "result-is-ok-or-err" => Some(Self::ResultOkOrErr),
            "predicate-or-negation" => Some(Self::PredicateOrNegation),
            "identical-or-alternatives" => Some(Self::IdenticalOrAlternatives),
            "assert-eq-identical" => Some(Self::AssertEqIdentical),
            _ => None,
        }
    }

    pub fn shape(self) -> &'static str {
        match self {
            Self::OptionSomeOrNone => "is_some() || is_none()",
            Self::ResultOkOrErr => "is_ok() || is_err()",
            Self::PredicateOrNegation => "predicate || !predicate",
            Self::IdenticalOrAlternatives => "expr || expr",
            Self::AssertEqIdentical => "assert_eq!(expr, expr)",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Detection {
    pub rule: RuleId,
    pub line: u32,
}

pub fn classify_assert_condition(expr: &Expr) -> Option<Detection> {
    classify_or(peel(expr)).map(|rule| Detection { rule, line: line_of(expr) })
}

pub fn classify_assert_eq(left: &Expr, right: &Expr) -> Option<Detection> {
    if identical_side_effect_free(left, right) {
        Some(Detection { rule: RuleId::AssertEqIdentical, line: line_of(peel(left)) })
    } else {
        None
    }
}

fn classify_or(expr: &Expr) -> Option<RuleId> {
    let Expr::Binary(binary) = expr else {
        return None;
    };
    if !matches!(binary.op, BinOp::Or(_)) {
        return None;
    }

    let left = peel(&binary.left);
    let right = peel(&binary.right);

    if let Some(rule) = option_or_result_pair(left, right) {
        return Some(rule);
    }

    if !is_side_effect_free(left) || !is_side_effect_free(right) {
        return None;
    }

    if is_negation_pair(left, right) {
        return Some(RuleId::PredicateOrNegation);
    }

    // `expr || expr` is equivalent to `expr`, not to `true`. Flagging it would
    // reject discriminating assertions (#14061 forbids false positives).
    None
}

fn identical_side_effect_free(left: &Expr, right: &Expr) -> bool {
    let left = peel(left);
    let right = peel(right);
    expr_eq(left, right) && is_side_effect_free(left)
}

fn option_or_result_pair(left: &Expr, right: &Expr) -> Option<RuleId> {
    let (left_recv, left_method) = method_name(left)?;
    let (right_recv, right_method) = method_name(right)?;
    if !expr_eq(left_recv, right_recv) {
        return None;
    }
    // Identical method-call receivers (`iter.next()`) can yield different
    // values. Identical function-call receivers remain governed because
    // #14061 falsifier 5 is the `sanitize_completion_path_input(...)`
    // option-pair. Impure-call residuals are accepted; a purity plugin is
    // a non-goal.
    if !receiver_stable_enough(left_recv) {
        return None;
    }
    match (left_method.as_str(), right_method.as_str()) {
        ("is_some", "is_none") | ("is_none", "is_some") => Some(RuleId::OptionSomeOrNone),
        ("is_ok", "is_err") | ("is_err", "is_ok") => Some(RuleId::ResultOkOrErr),
        _ => None,
    }
}

fn receiver_stable_enough(recv: &Expr) -> bool {
    is_side_effect_free(recv) || matches!(peel(recv), Expr::Call(_))
}

fn is_negation_pair(left: &Expr, right: &Expr) -> bool {
    negated_inner(right).is_some_and(|inner| expr_eq(left, inner))
        || negated_inner(left).is_some_and(|inner| expr_eq(right, inner))
}

fn negated_inner(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_)) => Some(peel(&unary.expr)),
        _ => None,
    }
}

fn method_name(expr: &Expr) -> Option<(&Expr, String)> {
    let Expr::MethodCall(call) = expr else {
        return None;
    };
    if !call.args.is_empty() || call.turbofish.is_some() {
        return None;
    }
    Some((peel(&call.receiver), call.method.to_string()))
}

fn is_side_effect_free(expr: &Expr) -> bool {
    match peel(expr) {
        Expr::Path(_) | Expr::Lit(_) | Expr::Const(_) => true,
        Expr::Reference(reference) => is_side_effect_free(&reference.expr),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Not(_) | UnOp::Deref(_) | UnOp::Neg(_)) => {
            is_side_effect_free(&unary.expr)
        }
        Expr::Field(field) => is_side_effect_free(&field.base),
        Expr::Tuple(tuple) => tuple.elems.iter().all(is_side_effect_free),
        Expr::Array(array) => array.elems.iter().all(is_side_effect_free),
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

pub(crate) fn peel(expr: &Expr) -> &Expr {
    match expr {
        Expr::Paren(paren) => peel(&paren.expr),
        Expr::Group(group) => peel(&group.expr),
        other => other,
    }
}

fn expr_eq(left: &Expr, right: &Expr) -> bool {
    peel(left) == peel(right)
}

fn line_of(expr: &Expr) -> u32 {
    u32::try_from(expr.span().start().line).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{RuleId, classify_assert_condition, classify_assert_eq};
    use syn::parse_str;

    fn rule_of(src: &str) -> Option<RuleId> {
        let expr = parse_str(src).unwrap_or_else(|error| panic!("parse `{src}`: {error}"));
        classify_assert_condition(&expr).map(|detection| detection.rule)
    }

    fn eq_rule(left: &str, right: &str) -> Option<RuleId> {
        let left = parse_str(left).unwrap_or_else(|error| panic!("parse left `{left}`: {error}"));
        let right =
            parse_str(right).unwrap_or_else(|error| panic!("parse right `{right}`: {error}"));
        classify_assert_eq(&left, &right).map(|detection| detection.rule)
    }

    #[test]
    fn flags_option_some_or_none() {
        assert_eq!(rule_of("value.is_some() || value.is_none()"), Some(RuleId::OptionSomeOrNone));
        assert_eq!(rule_of("value.is_none() || value.is_some()"), Some(RuleId::OptionSomeOrNone));
        assert_eq!(
            rule_of("(value.is_some()) || (value.is_none())"),
            Some(RuleId::OptionSomeOrNone)
        );
    }

    #[test]
    fn flags_multiline_call_receivers_as_option_pair() {
        assert_eq!(
            rule_of(
                r#"sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_some()
                    || sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_none()"#
            ),
            Some(RuleId::OptionSomeOrNone)
        );
    }

    #[test]
    fn flags_result_ok_or_err() {
        assert_eq!(rule_of("result.is_ok() || result.is_err()"), Some(RuleId::ResultOkOrErr));
        assert_eq!(
            rule_of("parse_result.is_err() || parse_result.is_ok()"),
            Some(RuleId::ResultOkOrErr)
        );
    }

    #[test]
    fn flags_predicate_or_negation_and_reverse() {
        assert_eq!(rule_of("ready || !ready"), Some(RuleId::PredicateOrNegation));
        assert_eq!(rule_of("!ready || ready"), Some(RuleId::PredicateOrNegation));
        assert_eq!(rule_of("flag.is_some() || !flag.is_some()"), Some(RuleId::PredicateOrNegation));
    }

    #[test]
    fn identical_or_alternatives_are_not_tautologies() {
        assert_eq!(rule_of("ready || ready"), None);
        assert_eq!(rule_of("(flag) || flag"), None);
    }

    #[test]
    fn flags_assert_eq_identical_side_effect_free_values() {
        assert_eq!(eq_rule("value", "value"), Some(RuleId::AssertEqIdentical));
        assert_eq!(
            eq_rule("RecoverySite::ArgList", "RecoverySite::ArgList"),
            Some(RuleId::AssertEqIdentical)
        );
        assert_eq!(eq_rule("1", "1"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("(value)", "value"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("item.flag", "item.flag"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("&value", "&value"), Some(RuleId::AssertEqIdentical));
        assert_eq!(
            eq_rule("TransportMode::Socket { port: 100 }", "TransportMode::Socket { port: 100 }"),
            Some(RuleId::AssertEqIdentical)
        );
    }

    #[test]
    fn does_not_flag_different_fields_or_outcomes() {
        assert_eq!(rule_of("item.code.is_some() || item.data.is_none()"), None);
        assert_eq!(rule_of("result.is_ok() || matches!(result, Err(Expected::Deferred))"), None);
    }

    #[test]
    fn does_not_flag_mutating_method_receivers() {
        assert_eq!(rule_of("iter.next().is_some() || iter.next().is_none()"), None);
        assert_eq!(rule_of("iter.next().is_ok() || iter.next().is_err()"), None);
    }

    #[test]
    fn does_not_flag_side_effecting_or_calls() {
        assert_eq!(rule_of("tick() || !tick()"), None);
        assert_eq!(rule_of("tick() || tick()"), None);
        assert_eq!(
            eq_rule(
                "RegexSourceDigest::for_source(source)",
                "RegexSourceDigest::for_source(source)"
            ),
            None
        );
    }

    #[test]
    fn does_not_flag_non_or_assert_conditions() {
        assert_eq!(rule_of("value.is_some()"), None);
        assert_eq!(rule_of("ready && !ready"), None);
        assert_eq!(eq_rule("left", "right"), None);
        assert_eq!(eq_rule("kind", "other"), None);
    }

    #[test]
    fn clone_method_assert_eq_is_a_false_negative_not_a_repair() {
        // `.clone()` is a method call, so the checker conservatively skips it.
        // That is an accepted false negative, not a blessed repair recipe.
        assert_eq!(eq_rule("site", "site.clone()"), None);
        assert_eq!(eq_rule("SyncPoint::Semicolon", "SyncPoint::Semicolon.clone()"), None);
        assert_eq!(
            eq_rule(
                "TransportMode::Socket { port: 100 }",
                "TransportMode::Socket { port: 100 }.clone()"
            ),
            None
        );
    }
}
