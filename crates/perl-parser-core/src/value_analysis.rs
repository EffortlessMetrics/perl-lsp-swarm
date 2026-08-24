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
            NodeKind::Number { value } => parse_integer(value).map_or(AbstractValue::Unknown, |value| {
                AbstractValue::Scalar(ScalarValue::Integer(value))
            }),
            NodeKind::String { value, interpolated } if !interpolated => {
                let Some(value) = unquote(value) else {
                    return AbstractValue::Unknown;
                };
                if value.len() > self.budget.max_string_length {
                    AbstractValue::OverBudget
                } else {
                    AbstractValue::Scalar(ScalarValue::String(value.to_owned()))
                }
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
        if elements.len() > self.budget.max_alternatives {
            return AbstractValue::OverBudget;
        }
        let mut values = Vec::with_capacity(elements.len());
        for element in elements {
            match self.eval(element, depth + 1) {
                AbstractValue::Scalar(value) => values.push(value),
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
                if a % b == 0 {
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

fn unquote(value: &str) -> Option<&str> {
    if value.len() > 1 && value.starts_with('q') && !value.starts_with("qq") {
        return None;
    }
    Some(value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')))
        .unwrap_or(value))
}

fn parse_integer(value: &str) -> Option<i128> {
    let value = value.replace('_', "");
    let (negative, value) =
        value.strip_prefix('-').map_or((false, value.as_str()), |value| (true, value));
    let parsed = if let Some(value) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
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
        ScalarValue::String(value) => parse_integer(value),
        ScalarValue::Undef => Some(0),
    }
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
        let budget = EvaluationBudget {
            max_alternatives: 1,
            ..EvaluationBudget::default()
        };
        assert_eq!(
            evaluate_with_budget(&expression("$flag ? 'Foo' : 'Bar';"), budget),
            AbstractValue::OverBudget
        );
        assert_eq!(
