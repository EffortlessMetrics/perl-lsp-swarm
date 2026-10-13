use crate::{
    builtin_import_names, looks_like_version_literal, normalized_pragma_token, parse_perl_version,
    pragma_arg_items,
};

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

// Flattened arguments do not preserve the target/import-list boundary. A terminal
// version-like import argument can therefore conservatively invalidate admission;
// resolving that ambiguity requires the canonical conditional application model.
pub(crate) fn conditional_pragma_target(args: &[String]) -> Option<(&str, &[String])> {
    args.iter().enumerate().find_map(|(idx, arg)| {
        let module = normalized_pragma_token(arg);
        let tail = &args[idx + 1..];
        // The parser may split a malformed dotted target into `v5`, `.`,
        // and a suffix. Retain it for authority invalidation even with a tail.
        if (idx > 0
            && (looks_like_version_literal(module) || parse_perl_version(module).is_some())
            && (tail.is_empty() || tail.first().is_some_and(|token| token == ".")))
            || (is_tracked_pragma_module(module) && conditional_target_tail_is_valid(module, tail))
        {
            Some((module, tail))
        } else {
            None
        }
    })
}
