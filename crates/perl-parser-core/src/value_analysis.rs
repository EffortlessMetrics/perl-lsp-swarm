//! Bounded abstract values for source-backed semantic analysis.
//!
//! This module deliberately evaluates only a small pure expression subset. It
//! is a fact-producing primitive, not a Perl interpreter: variables, calls,
//! overloaded values, and all other runtime-dependent constructs widen to an
//! explicit boundary. Callers can therefore use the result for names and
//! configuration without accidentally executing project code.

use crate::ast::{Node, NodeKind};

/// Default maximum number of alternatives retained in one finite value.
pub const DEFAULT_MAX_ALTERNATIVES: usize = 8;
/// Default maximum expression nesting evaluated by the pure evaluator.
pub const DEFAULT_MAX_DEPTH: usize = 64;
/// Default maximum length of a statically recovered string.
pub const DEFAULT_MAX_STRING_LENGTH: usize = 4096;

/// A scalar value that was proven from source syntax.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScalarValue {
    /// An integer representable without loss by the analyzer.
    Integer(i128),
    /// A non-interpolated string literal or pure concatenation.
    String(String),
    /// The Perl `undef` literal.
    Undef,
}

/// Result of bounded abstract evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbstractValue {
    /// One proven scalar value.
    Scalar(ScalarValue),
    /// A bounded set of proven scalar alternatives.
    Finite(Vec<ScalarValue>),
    /// The expression is not statically known, but is not itself a declared
    /// dynamic boundary (for example, an unresolved variable).
    Unknown,
    /// The expression crosses a runtime boundary such as a call or magic
    /// value. This distinction prevents consumers from treating uncertainty as
    /// a missing parser case.
    Dynamic,
    /// The evaluator stopped because a declared budget would be exceeded.
    OverBudget,
}

/// Limits governing one evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvaluationBudget {
    /// Maximum alternatives in a finite result.
    pub max_alternatives: usize,
    /// Maximum recursive expression depth.
    pub max_depth: usize,
    /// Maximum recovered string length.
    pub max_string_length: usize,
}

impl Default for EvaluationBudget {
    fn default() -> Self {
        Self {
            max_alternatives: DEFAULT_MAX_ALTERNATIVES,
            max_depth: DEFAULT_MAX_DEPTH,
            max_string_length: DEFAULT_MAX_STRING_LENGTH,
        }
    }
}

/// Evaluate one AST expression using the default safety budget.
#[must_use]
pub fn evaluate(node: &Node) -> AbstractValue {
    evaluate_with_budget(node, EvaluationBudget::default())
}

/// Evaluate one AST expression without executing calls or project code.
#[must_use]
pub fn evaluate_with_budget(node: &Node, budget: EvaluationBudget) -> AbstractValue {
    Evaluator { budget }.eval(node, 0)
}

struct Evaluator {
    budget: EvaluationBudget,
}

impl Evaluator {
    fn eval(&self, node: &Node, depth: usize) -> AbstractValue {
        if depth > self.budget.max_depth {
            return AbstractValue::OverBudget;
        }

        match &node.kind {
            NodeKind::ExpressionStatement { expression } => self.eval(expression, depth + 1),
            NodeKind::Number { value } => parse_integer(value)
                .map_or(AbstractValue::Unknown, |value| {
                    AbstractValue::Scalar(ScalarValue::Integer(value))
                }),
            NodeKind::String { value, interpolated } if !interpolated => {
                let Some(value) = decode_literal(value) else {
                    return AbstractValue::Unknown;
                };
                if value.len() > self.budget.max_string_length {
                    AbstractValue::OverBudget
                } else {
                    AbstractValue::Scalar(ScalarValue::String(value))
                }
            }
            // Backtick capture (`qx{...}`) is stored as an interpolated
            // string, but its value is runtime command output, not the
            // source spelling: classify it as a declared dynamic boundary.
            NodeKind::String { value, interpolated: true } if value.starts_with('`') => {
                AbstractValue::Dynamic
            }
            NodeKind::Undef => AbstractValue::Scalar(ScalarValue::Undef),
            NodeKind::ArrayLiteral { elements } => self.eval_array(elements, depth),
            NodeKind::Binary { op, left, right } => self.eval_binary(op, left, right, depth),
            NodeKind::Unary { op, operand } => self.eval_unary(op, operand, depth),
            NodeKind::Ternary { condition, then_expr, else_expr } => {
                self.eval_ternary(condition, then_expr, else_expr, depth)
            }
            NodeKind::FunctionCall { .. }
            | NodeKind::MethodCall { .. }
            | NodeKind::AmperCall { .. }
            | NodeKind::IndirectCall { .. }
            | NodeKind::Readline { .. }
            | NodeKind::Glob { .. } => AbstractValue::Dynamic,
            _ => AbstractValue::Unknown,
        }
    }

    fn eval_array(&self, elements: &[Node], depth: usize) -> AbstractValue {
        // Alternatives are a set, matching `join`'s semantics: duplicates
        // collapse while evaluating and `max_alternatives` bounds the number
        // of distinct retained values, never the source element count.
        let mut values: Vec<ScalarValue> = Vec::new();
        for element in elements {
            match self.eval(element, depth + 1) {
                AbstractValue::Scalar(value) => {
                    if values.contains(&value) {
                        continue;
                    }
                    if values.len() >= self.budget.max_alternatives {
                        return AbstractValue::OverBudget;
                    }
                    values.push(value);
                }
                AbstractValue::Finite(_) => return AbstractValue::Unknown,
                AbstractValue::OverBudget => return AbstractValue::OverBudget,
                AbstractValue::Dynamic => return AbstractValue::Dynamic,
                AbstractValue::Unknown => return AbstractValue::Unknown,
            }
        }
        AbstractValue::Finite(values)
    }

    fn eval_binary(&self, op: &str, left: &Node, right: &Node, depth: usize) -> AbstractValue {
        let lhs = self.eval(left, depth + 1);
        let rhs = self.eval(right, depth + 1);
        if matches!(lhs, AbstractValue::OverBudget) || matches!(rhs, AbstractValue::OverBudget) {
            return AbstractValue::OverBudget;
        }
        if matches!(lhs, AbstractValue::Dynamic) || matches!(rhs, AbstractValue::Dynamic) {
            return AbstractValue::Dynamic;
        }
        let (AbstractValue::Scalar(lhs), AbstractValue::Scalar(rhs)) = (lhs, rhs) else {
            return AbstractValue::Unknown;
        };
        match (op, lhs, rhs) {
            ("+", ScalarValue::Integer(a), ScalarValue::Integer(b)) => {
                a.checked_add(b).map_or(AbstractValue::Unknown, scalar_integer)
            }
            ("-", ScalarValue::Integer(a), ScalarValue::Integer(b)) => {
                a.checked_sub(b).map_or(AbstractValue::Unknown, scalar_integer)
            }
            ("*", ScalarValue::Integer(a), ScalarValue::Integer(b)) => {
                a.checked_mul(b).map_or(AbstractValue::Unknown, scalar_integer)
            }
            ("/", ScalarValue::Integer(a), ScalarValue::Integer(b)) if b != 0 => {
                // `checked_rem` (not `%`): `i128::MIN % -1` would panic before
                // the widening decision, and the checked form returns `None`
                // so the pair widens to `Unknown` instead.
                if a.checked_rem(b) == Some(0) {
                    a.checked_div(b).map_or(AbstractValue::Unknown, scalar_integer)
                } else {
                    AbstractValue::Unknown
                }
            }
            (".", a, b) => self.concat(a, b),
            ("==", a, b) => numeric_equal(&a, &b).map_or(AbstractValue::Unknown, scalar_bool),
            ("!=", a, b) => {
                numeric_equal(&a, &b).map_or(AbstractValue::Unknown, |equal| scalar_bool(!equal))
            }
            ("eq", a, b) => string_equal(&a, &b).map_or(AbstractValue::Unknown, scalar_bool),
            ("ne", a, b) => {
                string_equal(&a, &b).map_or(AbstractValue::Unknown, |equal| scalar_bool(!equal))
            }
            _ => AbstractValue::Unknown,
        }
    }

    fn concat(&self, lhs: ScalarValue, rhs: ScalarValue) -> AbstractValue {
        let value = format_scalar(&lhs) + &format_scalar(&rhs);
        if value.len() > self.budget.max_string_length {
            AbstractValue::OverBudget
        } else {
            AbstractValue::Scalar(ScalarValue::String(value))
        }
    }

    fn eval_unary(&self, op: &str, operand: &Node, depth: usize) -> AbstractValue {
        let value = self.eval(operand, depth + 1);
        let AbstractValue::Scalar(value) = value else {
            return value;
        };
        match (op, value) {
            ("+", ScalarValue::Integer(value)) | ("", ScalarValue::Integer(value)) => {
                scalar_integer(value)
            }
            ("-", ScalarValue::Integer(value)) => {
                value.checked_neg().map_or(AbstractValue::Unknown, scalar_integer)
            }
            ("!", value) => scalar_bool(!is_truthy(&value)),
            _ => AbstractValue::Unknown,
        }
    }

    fn eval_ternary(
        &self,
        condition: &Node,
        then_expr: &Node,
        else_expr: &Node,
        depth: usize,
    ) -> AbstractValue {
        match self.eval(condition, depth + 1) {
            AbstractValue::Scalar(value) if is_truthy(&value) => self.eval(then_expr, depth + 1),
            AbstractValue::Scalar(_) => self.eval(else_expr, depth + 1),
            AbstractValue::OverBudget => AbstractValue::OverBudget,
            AbstractValue::Dynamic => AbstractValue::Dynamic,
            AbstractValue::Unknown => {
                self.join(self.eval(then_expr, depth + 1), self.eval(else_expr, depth + 1))
            }
            AbstractValue::Finite(_) => AbstractValue::Unknown,
        }
    }

    fn join(&self, left: AbstractValue, right: AbstractValue) -> AbstractValue {
        let mut values = Vec::new();
        for value in [left, right] {
            match value {
                AbstractValue::Scalar(value) => {
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
                AbstractValue::Finite(more) => {
                    for value in more {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                }
                AbstractValue::OverBudget => return AbstractValue::OverBudget,
                AbstractValue::Dynamic => return AbstractValue::Dynamic,
                AbstractValue::Unknown => return AbstractValue::Unknown,
            }
        }
        if values.len() > self.budget.max_alternatives {
            AbstractValue::OverBudget
        } else {
            AbstractValue::Finite(values)
        }
    }
}

fn scalar_integer(value: i128) -> AbstractValue {
    AbstractValue::Scalar(ScalarValue::Integer(value))
}
fn scalar_bool(value: bool) -> AbstractValue {
    scalar_integer(i128::from(value))
}

fn is_truthy(value: &ScalarValue) -> bool {
    match value {
        ScalarValue::Integer(value) => *value != 0,
        ScalarValue::String(value) => !value.is_empty() && value != "0",
        ScalarValue::Undef => false,
    }
}

fn format_scalar(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Integer(value) => value.to_string(),
        ScalarValue::String(value) => value.clone(),
        ScalarValue::Undef => String::new(),
    }
}

/// Decode one non-interpolated string literal exactly, or return `None`
/// when the spelling cannot be proven from source alone.
///
/// Quote-like operators (`q{...}`, `qw{...}`, ...) keep their full lexeme in
/// the AST, and their delimiter/escape decoding is out of scope here, so
/// they classify as `Unknown`. Double-quoted literals carry a richer escape
/// set than this evaluator decodes, so only escape-free spellings publish.
fn decode_literal(raw: &str) -> Option<String> {
    if raw.len() > 1 && raw.starts_with('q') && !raw.starts_with("qq") {
        return None;
    }
    if let Some(content) = raw.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')) {
        return decode_single_quoted(content);
    }
    if let Some(content) = raw.strip_prefix('"').and_then(|value| value.strip_suffix('"')) {
        return if content.contains('\\') { None } else { Some(content.to_owned()) };
    }
    // Bare content with an unknown quoting context: escape-free only.
    if raw.contains('\\') { None } else { Some(raw.to_owned()) }
}

/// Perl single-quote escape semantics, exactly: `\\` decodes to a backslash
/// and `\'` to a quote; every other backslash sequence is literal (the
/// backslash is retained), and a trailing lone backslash is not a valid
/// literal spelling.
fn decode_single_quoted(content: &str) -> Option<String> {
    let mut decoded = String::with_capacity(content.len());
    let mut chars = content.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') => decoded.push('\\'),
                Some('\'') => decoded.push('\''),
                Some(other) => {
                    decoded.push('\\');
                    decoded.push(other);
                }
                None => return None,
            }
        } else {
            decoded.push(ch);
        }
    }
    Some(decoded)
}

fn parse_integer(value: &str) -> Option<i128> {
    let value = value.replace('_', "");
    let (negative, value) =
        value.strip_prefix('-').map_or((false, value.as_str()), |value| (true, value));
    let parsed = if let Some(value) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X"))
    {
        i128::from_str_radix(value, 16).ok()
    } else if let Some(value) = value.strip_prefix("0b").or_else(|| value.strip_prefix("0B")) {
        i128::from_str_radix(value, 2).ok()
    } else if let Some(value) = value.strip_prefix("0o").or_else(|| value.strip_prefix("0O")) {
        i128::from_str_radix(value, 8).ok()
    } else if value.len() > 1
        && value.starts_with('0')
        && value.bytes().all(|byte| byte.is_ascii_digit())
    {
        i128::from_str_radix(&value[1..], 8).ok()
    } else if !value.contains('.') && !value.contains('e') && !value.contains('E') {
        value.parse().ok()
    } else {
        None
    };
    parsed.map(|value| if negative { -value } else { value })
}

fn numeric_equal(left: &ScalarValue, right: &ScalarValue) -> Option<bool> {
    Some(numeric_value(left)? == numeric_value(right)?)
}

fn numeric_value(value: &ScalarValue) -> Option<i128> {
    match value {
        ScalarValue::Integer(value) => Some(*value),
        ScalarValue::String(value) => numify_string(value),
        ScalarValue::Undef => Some(0),
    }
}

/// Conservative numeric numification of a proven string scalar.
///
/// Perl numifies strings decimally at runtime; source-literal spellings
/// (`0x`, `0b`, leading-zero octal, `_` separators) do not apply to a
/// runtime string, so reusing `parse_integer` here would fabricate values
/// (`'0x10'` would fold to 16 although Perl numifies it to 0). Only plain
/// decimal digits (and the empty string, which numifies to 0) are proven;
/// every other spelling widens to `None` so the comparison publishes
/// `Unknown` instead of a fabricated branch.
fn numify_string(value: &str) -> Option<i128> {
    if value.is_empty() {
        return Some(0);
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().ok();
    }
    None
}

fn string_equal(left: &ScalarValue, right: &ScalarValue) -> Option<bool> {
    Some(format_scalar(left) == format_scalar(right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    fn expression(source: &str) -> Node {
        Parser::new(source)
            .parse()
            .expect("fixture should parse")
            .children()
            .into_iter()
            .next()
            .expect("expression statement")
            .clone()
    }

    #[test]
    fn folds_bounded_integer_and_string_expressions() {
        assert_eq!(evaluate(&expression("2 + 3 * 4;")), scalar_integer(14));
        assert_eq!(
            evaluate(&expression("'find_' . 'user';")),
            AbstractValue::Scalar(ScalarValue::String("find_user".into()))
        );
    }

    #[test]
    fn preserves_unknown_and_dynamic_boundaries() {
        assert_eq!(evaluate(&expression("$x + 1;")), AbstractValue::Unknown);
        assert_eq!(evaluate(&expression("make_value();")), AbstractValue::Dynamic);
    }

    #[test]
    fn joins_unknown_condition_without_fabricating_a_single_value() {
        assert_eq!(
            evaluate(&expression("$flag ? 'Foo' : 'Bar';")),
            AbstractValue::Finite(vec![
                ScalarValue::String("Foo".into()),
                ScalarValue::String("Bar".into())
            ])
        );
    }

    #[test]
    fn widening_is_explicit_and_budgeted() {
        let budget = EvaluationBudget { max_alternatives: 1, ..EvaluationBudget::default() };
        assert_eq!(
            evaluate_with_budget(&expression("$flag ? 'Foo' : 'Bar';"), budget),
            AbstractValue::OverBudget
        );
        assert_eq!(
            evaluate(&expression("(1, 2, 3);")),
            AbstractValue::Finite(vec![
                ScalarValue::Integer(1),
                ScalarValue::Integer(2),
                ScalarValue::Integer(3)
            ])
        );
    }
    #[test]
    fn single_quote_escapes_decode_exactly() {
        // `\'` decodes to a quote and `\\` to a backslash; the published
        // value is the Perl value, not the raw token text.
        assert_eq!(
            evaluate(&expression(r"'it\'s';")),
            AbstractValue::Scalar(ScalarValue::String("it's".into()))
        );
        assert_eq!(
            evaluate(&expression(r"'a\\b';")),
            AbstractValue::Scalar(ScalarValue::String(r"a\b".into()))
        );
        // Other backslash sequences are literal in single quotes.
        assert_eq!(
            evaluate(&expression(r"'a\tb';")),
            AbstractValue::Scalar(ScalarValue::String("a\\tb".into()))
        );
        // Double-quoted literals are interpolating contexts (the parser
        // marks them interpolated), so they widen to Unknown regardless of
        // escape content; the exact decoder never sees them.
        assert_eq!(evaluate(&expression(r#""plain";"#)), AbstractValue::Unknown);
        assert_eq!(evaluate(&expression(r#""a\\b";"#)), AbstractValue::Unknown);
    }

    #[test]
    fn remainder_overflow_pair_neither_panics_nor_fabricates() {
        // i128::MIN is reachable via checked multiplication; `MIN % -1`
        // must widen instead of panicking (the pre-fix `%` panicked before
        // the widening decision).
        let source = "(-85070591730234615865843651857942052864 * 2) / -1;";
        assert_eq!(evaluate(&expression(source)), AbstractValue::Unknown);
        // Integral quotients still fold, including negative divisors.
        assert_eq!(evaluate(&expression("6 / -2;")), scalar_integer(-3));
        // Fractional quotients widen rather than truncating.
        assert_eq!(evaluate(&expression("3 / 2;")), AbstractValue::Unknown);
    }

    #[test]
    fn array_alternatives_deduplicate_before_the_budget() {
        // Alternatives are a set: duplicates collapse while evaluating and
        // the budget bounds distinct values, never the source element count.
        assert_eq!(
            evaluate(&expression("(1, 1, 1);")),
            AbstractValue::Finite(vec![ScalarValue::Integer(1)])
        );
        let one_alternative =
            EvaluationBudget { max_alternatives: 1, ..EvaluationBudget::default() };
        assert_eq!(
            evaluate_with_budget(&expression("(1, 1, 1);"), one_alternative),
            AbstractValue::Finite(vec![ScalarValue::Integer(1)])
        );
        assert_eq!(
            evaluate_with_budget(&expression("(1, 1, 2);"), one_alternative),
            AbstractValue::OverBudget
        );
        assert_eq!(
            evaluate(&expression("(1, 2, 3);")),
            AbstractValue::Finite(vec![
                ScalarValue::Integer(1),
                ScalarValue::Integer(2),
                ScalarValue::Integer(3)
            ])
        );
    }

    #[test]
    fn legacy_leading_zero_literals_decode_as_octal() {
        // Source literals follow Perl's legacy octal spelling; a decimal
        // fallback would publish 755 for `0755` although Perl evaluates 493.
        assert_eq!(evaluate(&expression("0755;")), scalar_integer(493));
        assert_eq!(evaluate(&expression("0644;")), scalar_integer(420));
        // Invalid octal digits widen instead of fabricating a decimal value.
        assert_eq!(evaluate(&expression("08;")), AbstractValue::Unknown);
        assert_eq!(evaluate(&expression("0;")), scalar_integer(0));
    }

    #[test]
    fn equality_coerces_per_operator_without_fabricating() {
        // Numeric coercion: `"01" == 1` is true because Perl numifies the
        // string decimally (not octally) at runtime.
        assert_eq!(evaluate(&expression("'01' == 1;")), scalar_integer(1));
        assert_eq!(evaluate(&expression("'01' != 1;")), scalar_integer(0));
        // String coercion: `"1" eq 1` compares both sides as strings.
        assert_eq!(evaluate(&expression("'1' eq 1;")), scalar_integer(1));
        assert_eq!(evaluate(&expression("'1' ne 1;")), scalar_integer(0));
        // Spellings whose numification is not proven (Perl numifies `'0x10'`
        // to 0, unlike the source literal `0x10`) widen instead of folding a
        // fabricated branch.
        assert_eq!(evaluate(&expression("'0x10' == 0;")), AbstractValue::Unknown);
        assert_eq!(evaluate(&expression("'' == 0;")), scalar_integer(1));
    }

    #[test]
    fn runtime_boundaries_stay_distinct_from_missing_cases() {
        // Filehandle reads and glob expansion depend on runtime state.
        assert_eq!(evaluate(&expression("<STDIN>;")), AbstractValue::Dynamic);
        assert_eq!(evaluate(&expression("<*.pm>;")), AbstractValue::Dynamic);
        // Backtick capture is command output: a declared dynamic boundary,
        // not an unknown evaluation.
        assert_eq!(evaluate(&expression("`date`;")), AbstractValue::Dynamic);
    }
}
