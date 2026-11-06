use crate::{
    builtin_import_names, looks_like_version_literal, normalized_pragma_token, parse_perl_version,
    pragma_arg_items,
};

/// Whether a conditional `use if` / `no if` condition is statically decidable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaticConditionTruth {
    /// Literal `1` condition: apply the target when it is unambiguous.
    ProvenTrue,
    /// Literal `0` condition: never apply the target.
    ProvenFalse,
}

/// Result of resolving a conditional pragma argument list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConditionalApplyDecision<'a> {
    /// Apply the resolved target exactly once.
    Apply { target_module: &'a str, target_args: &'a [String] },
    /// Proven-false condition: leave exact state unchanged.
    NoEffect,
    /// Unknown or malformed conditional: leave exact state unchanged.
    Unsupported,
}

fn is_tracked_pragma_module(module: &str) -> bool {
    matches!(
        module,
        "strict"
            | "warnings"
            | "utf8"
            | "encoding"
            | "locale"
            | "feature"
            | "builtin"
            | "experimental"
    )
}

fn valid_strict_args(args: &[String]) -> bool {
    args.iter()
        .flat_map(|arg| pragma_arg_items(arg))
        .all(|item| matches!(item.as_str(), "vars" | "subs" | "refs"))
}

fn conditional_target_tail_is_valid(module: &str, tail: &[String]) -> bool {
    if parse_perl_version(module).is_some() {
        return tail.is_empty();
    }

    match module {
        "strict" => tail.is_empty() || valid_strict_args(tail),
        "warnings" => true,
        "utf8" => tail.is_empty(),
        "encoding" => tail.len() == 1 && !normalized_pragma_token(&tail[0]).is_empty(),
        "locale" => {
            tail.is_empty() || (tail.len() == 1 && !normalized_pragma_token(&tail[0]).is_empty())
        }
        "feature" => !tail.is_empty(),
        "experimental" => !tail.is_empty(),
        "builtin" => tail.iter().any(|arg| !builtin_import_names(arg).is_empty()),
        _ => false,
    }
}

fn static_condition_truth(first_arg: &str) -> Option<StaticConditionTruth> {
    match first_arg {
        "1" => Some(StaticConditionTruth::ProvenTrue),
        "0" => Some(StaticConditionTruth::ProvenFalse),
        _ => None,
    }
}

/// Resolve the target module from the first argument after a proven literal condition.
///
/// Does not scan deeper into the list for alternate recognized tokens.
fn conditional_target_after_condition(target_args: &[String]) -> Option<(&str, &[String])> {
    let first = target_args.first()?;
    let module = normalized_pragma_token(first);
    let tail = target_args.get(1..).unwrap_or_default();
    if module.is_empty() {
        return None;
    }

    if (looks_like_version_literal(module) || parse_perl_version(module).is_some())
        && (tail.is_empty() || tail.first().is_some_and(|token| token == "."))
    {
        return Some((module, tail));
    }

    if is_tracked_pragma_module(module) && conditional_target_tail_is_valid(module, tail) {
        return Some((module, tail));
    }

    None
}

/// Decide whether a conditional `use if` / `no if` directive may mutate pragma state.
pub(crate) fn conditional_apply_decision(args: &[String]) -> ConditionalApplyDecision<'_> {
    let Some(first) = args.first() else {
        return ConditionalApplyDecision::Unsupported;
    };

    let Some(truth) = static_condition_truth(first) else {
        return ConditionalApplyDecision::Unsupported;
    };

    match truth {
        StaticConditionTruth::ProvenFalse => ConditionalApplyDecision::NoEffect,
        StaticConditionTruth::ProvenTrue => {
            let target_args = args.get(1..).unwrap_or_default();
            match conditional_target_after_condition(target_args) {
                Some((target_module, target_tail)) => {
                    ConditionalApplyDecision::Apply { target_module, target_args: target_tail }
                }
                None => ConditionalApplyDecision::Unsupported,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn literal_true_applies_recognized_target_once() {
        let argv = args(&["1", "'strict'"]);
        let decision = conditional_apply_decision(&argv);
        assert!(matches!(
            decision,
            ConditionalApplyDecision::Apply { target_module: "strict", target_args: [] }
        ));
    }

    #[test]
    fn literal_false_never_applies_target() {
        let argv = args(&["0", "'strict'"]);
        let decision = conditional_apply_decision(&argv);
        assert_eq!(decision, ConditionalApplyDecision::NoEffect);
    }

    #[test]
    fn dynamic_condition_is_unsupported_without_scanning_for_targets() {
        let argv = args(&["$dynamic", "'strict'"]);
        let decision = conditional_apply_decision(&argv);
        assert_eq!(decision, ConditionalApplyDecision::Unsupported);
    }

    #[test]
    fn condition_expression_tokens_do_not_yield_targets() {
        let argv = args(&["$^O", "eq", "'MSWin32'", "'strict'"]);
        let decision = conditional_apply_decision(&argv);
        assert_eq!(decision, ConditionalApplyDecision::Unsupported);
    }

    #[test]
    fn unless_module_name_is_not_handled_here() {
        // Callers must only invoke this helper for module == "if".
        let argv = args(&["1", "'strict'"]);
        let decision = conditional_apply_decision(&argv);
        assert!(matches!(decision, ConditionalApplyDecision::Apply { .. }));
    }
}
