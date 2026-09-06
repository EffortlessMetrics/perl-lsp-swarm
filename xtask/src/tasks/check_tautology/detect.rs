//! Conservative structural tautology detection for assertion expressions.
//!
//! False negatives are acceptable. False positives are not. Option/Result
//! method pairs fire only when the receiver is a constructor or an explicitly
//! ascribed Option/Result. Identical `assert_eq!` operands are governed only
//! when PartialEq reflexivity is known from syntax.

use super::expr::{
    QueryKind, TypeEnv, expr_eq, is_known_reflexive_eq_operand, is_side_effect_free, peel,
    proven_query_kind,
};
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
    classify_assert_condition_in(expr, &TypeEnv::new())
}

pub fn classify_assert_condition_in(expr: &Expr, env: &TypeEnv) -> Option<Detection> {
    classify_or(peel(expr), env).map(|rule| Detection { rule, line: line_of(expr) })
}

pub fn classify_assert_eq(left: &Expr, right: &Expr) -> Option<Detection> {
    if identical_known_reflexive_eq(left, right) {
        Some(Detection { rule: RuleId::AssertEqIdentical, line: line_of(peel(left)) })
    } else {
        None
    }
}

fn classify_or(expr: &Expr, env: &TypeEnv) -> Option<RuleId> {
    let Expr::Binary(binary) = expr else {
        return None;
    };
    if !matches!(binary.op, BinOp::Or(_)) {
        return None;
    }

    let left = peel(&binary.left);
    let right = peel(&binary.right);

    if let Some(rule) = option_or_result_pair(left, right, env) {
        return Some(rule);
    }

    if !is_side_effect_free(left, env) || !is_side_effect_free(right, env) {
        return None;
    }

    if is_negation_pair(left, right) {
        return Some(RuleId::PredicateOrNegation);
    }

    // `expr || expr` is equivalent to `expr`, not to `true`. Flagging it would
    // reject discriminating assertions (#14061 forbids false positives).
    None
}

fn identical_known_reflexive_eq(left: &Expr, right: &Expr) -> bool {
    let left = peel(left);
    let right = peel(right);
    expr_eq(left, right) && is_known_reflexive_eq_operand(left)
}

fn option_or_result_pair(left: &Expr, right: &Expr, env: &TypeEnv) -> Option<RuleId> {
    let (left_recv, left_method) = method_name(left)?;
    let (right_recv, right_method) = method_name(right)?;
    if !expr_eq(left_recv, right_recv) {
        return None;
    }
    let kind = proven_query_kind(left_recv, env)?;
    match (kind, left_method.as_str(), right_method.as_str()) {
        (QueryKind::Option, "is_some", "is_none") | (QueryKind::Option, "is_none", "is_some") => {
            Some(RuleId::OptionSomeOrNone)
        }
        (QueryKind::Result, "is_ok", "is_err") | (QueryKind::Result, "is_err", "is_ok") => {
            Some(RuleId::ResultOkOrErr)
        }
        _ => None,
    }
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

fn line_of(expr: &Expr) -> u32 {
    u32::try_from(expr.span().start().line).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::super::expr::{QueryKind, TypeEnv};
    use super::{
        RuleId, classify_assert_condition, classify_assert_condition_in, classify_assert_eq,
    };
    use syn::parse_str;

    fn rule_of(src: &str) -> Option<RuleId> {
        let expr = parse_str(src).unwrap_or_else(|error| panic!("parse `{src}`: {error}"));
        classify_assert_condition(&expr).map(|detection| detection.rule)
    }

    fn typed_rule(src: &str, bindings: &[(&str, QueryKind)]) -> Option<RuleId> {
        let expr = parse_str(src).unwrap_or_else(|error| panic!("parse `{src}`: {error}"));
        let mut env = TypeEnv::new();
        for (ident, kind) in bindings {
            env.bind((*ident).to_string(), *kind);
        }
        classify_assert_condition_in(&expr, &env).map(|detection| detection.rule)
    }

    fn eq_rule(left: &str, right: &str) -> Option<RuleId> {
        let left = parse_str(left).unwrap_or_else(|error| panic!("parse left `{left}`: {error}"));
        let right =
            parse_str(right).unwrap_or_else(|error| panic!("parse right `{right}`: {error}"));
        classify_assert_eq(&left, &right).map(|detection| detection.rule)
    }

    #[test]
    fn flags_option_some_or_none() {
        assert_eq!(
            typed_rule("value.is_some() || value.is_none()", &[("value", QueryKind::Option)]),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(
            typed_rule("value.is_none() || value.is_some()", &[("value", QueryKind::Option)]),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(
            typed_rule("(value.is_some()) || (value.is_none())", &[("value", QueryKind::Option)]),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(
            rule_of("Some(1).is_some() || Some(1).is_none()"),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(rule_of("None.is_some() || None.is_none()"), Some(RuleId::OptionSomeOrNone));
    }

    #[test]
    fn flags_result_ok_or_err() {
        assert_eq!(
            typed_rule("result.is_ok() || result.is_err()", &[("result", QueryKind::Result)]),
            Some(RuleId::ResultOkOrErr)
        );
        assert_eq!(
            typed_rule(
                "parse_result.is_err() || parse_result.is_ok()",
                &[("parse_result", QueryKind::Result)]
            ),
            Some(RuleId::ResultOkOrErr)
        );
        assert_eq!(rule_of("Ok(()).is_ok() || Ok(()).is_err()"), Some(RuleId::ResultOkOrErr));
    }

    #[test]
    fn flags_predicate_or_negation_and_reverse() {
        assert_eq!(rule_of("ready || !ready"), Some(RuleId::PredicateOrNegation));
        assert_eq!(rule_of("!ready || ready"), Some(RuleId::PredicateOrNegation));
        assert_eq!(
            typed_rule("flag.is_some() || !flag.is_some()", &[("flag", QueryKind::Option)]),
            Some(RuleId::PredicateOrNegation)
        );
        assert_eq!(rule_of("flag.is_some() || !flag.is_some()"), None);
    }

    #[test]
    fn identical_or_alternatives_are_not_tautologies() {
        assert_eq!(rule_of("ready || ready"), None);
        assert_eq!(rule_of("(flag) || flag"), None);
    }

    #[test]
    fn still_flags_known_reflexive_identical_literals_after_reflexivity_narrowing() {
        assert_eq!(eq_rule("1", "1"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("true", "true"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("\"ok\"", "\"ok\""), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("'a'", "'a'"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("-1", "-1"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("(1)", "1"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("1u8", "1u8"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("&1", "&1"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("(1, true)", "(1, true)"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("1 as i32", "1 as i32"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("[1, 2]", "[1, 2]"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("[0; 3]", "[0; 3]"), Some(RuleId::AssertEqIdentical));
        assert_eq!(eq_rule("1.0", "1.0"), Some(RuleId::AssertEqIdentical));
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

    #[test]
    fn does_not_flag_stateful_or_nondeterministic_function_call_receivers() {
        // Two evaluations of the same call are not one Option/Result value.
        assert_eq!(rule_of("counter().is_some() || counter().is_none()"), None);
        assert_eq!(rule_of("random().is_ok() || random().is_err()"), None);
        assert_eq!(
            rule_of(
                r#"sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_some()
                    || sanitize_completion_path_input("..%2f..%2fetc%2fpasswd").is_none()"#
            ),
            None
        );
    }

    #[test]
    fn still_flags_side_effect_free_option_and_result_paths_after_purity_narrowing() {
        assert_eq!(
            typed_rule("value.is_some() || value.is_none()", &[("value", QueryKind::Option)]),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(
            typed_rule("result.is_ok() || result.is_err()", &[("result", QueryKind::Result)]),
            Some(RuleId::ResultOkOrErr)
        );
        assert_eq!(
            typed_rule("(value.is_some()) || (value.is_none())", &[("value", QueryKind::Option)]),
            Some(RuleId::OptionSomeOrNone)
        );
        assert_eq!(rule_of("value.is_some() || value.is_none()"), None);
        assert_eq!(rule_of("item.flag.is_ok() || item.flag.is_err()"), None);
    }

    #[test]
    fn does_not_flag_custom_query_methods() {
        assert_eq!(rule_of("probe.is_some() || probe.is_none()"), None);
        assert_eq!(rule_of("probe.is_ok() || probe.is_err()"), None);
        assert_eq!(rule_of("probe.is_some() || !probe.is_some()"), None);
    }

    #[test]
    fn does_not_flag_non_reflexive_or_type_unknown_assert_eq() {
        assert_eq!(eq_rule("f32::NAN", "f32::NAN"), None);
        assert_eq!(eq_rule("f64::NAN", "f64::NAN"), None);
        assert_eq!(eq_rule("value", "value"), None);
        assert_eq!(eq_rule("item.flag", "item.flag"), None);
        assert_eq!(eq_rule("RecoverySite::ArgList", "RecoverySite::ArgList"), None);
        assert_eq!(eq_rule("&mut 1", "&mut 1"), None);
        assert_eq!(eq_rule("-f32::NAN", "-f32::NAN"), None);
        assert_eq!(eq_rule("[path, path]", "[path, path]"), None);
        assert_eq!(eq_rule("[path; 3]", "[path; 3]"), None);
        assert_eq!(
            eq_rule("TransportMode::Socket { port: 100 }", "TransportMode::Socket { port: 100 }"),
            None
        );
    }
}
