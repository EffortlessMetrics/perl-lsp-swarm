use crate::{builtin_import_names, normalized_pragma_token, parse_perl_version, pragma_arg_items};

fn is_tracked_pragma_module(module: &str) -> bool {
    matches!(module, "strict" | "warnings" | "utf8" | "encoding" | "locale" | "feature" | "builtin")
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
        "builtin" => tail.iter().any(|arg| !builtin_import_names(arg).is_empty()),
        _ => false,
    }
}

pub(crate) fn conditional_pragma_target(args: &[String]) -> Option<(&str, &[String])> {
    args.iter().enumerate().find_map(|(idx, arg)| {
        let module = normalized_pragma_token(arg);
        let tail = &args[idx + 1..];
        if (is_tracked_pragma_module(module) || parse_perl_version(module).is_some())
            && conditional_target_tail_is_valid(module, tail)
        {
            Some((module, tail))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[test]
    fn conditional_target_skips_expression_and_returns_tracked_module_tail() {
        let args = string_args(&["$Config{use_utf8}", "'utf8'"]);

        assert_eq!(conditional_pragma_target(&args), Some(("utf8", &args[2..])));
    }

    #[test]
    fn conditional_target_accepts_version_only_without_tail() {
        let args = string_args(&["$] >= 5.036", "v5.36"]);

        assert_eq!(conditional_pragma_target(&args), Some(("v5.36", &args[2..])));
    }

    #[test]
    fn conditional_target_rejects_invalid_target_tails() {
        let invalid_strict = string_args(&["$cond", "strict", "bogus"]);
        let invalid_feature = string_args(&["$cond", "feature"]);
        let invalid_encoding = string_args(&["$cond", "encoding", "''"]);
        let invalid_version = string_args(&["$cond", "v5.36", "feature"]);

        assert_eq!(conditional_pragma_target(&invalid_strict), None);
        assert_eq!(conditional_pragma_target(&invalid_feature), None);
        assert_eq!(conditional_pragma_target(&invalid_encoding), None);
        assert_eq!(conditional_pragma_target(&invalid_version), None);
    }

    #[test]
    fn conditional_target_accepts_builtin_only_when_names_are_present() {
        let empty_builtin = string_args(&["$cond", "builtin", "''"]);
        let named_builtin = string_args(&["$cond", "builtin", "qw(true false)"]);

        assert_eq!(conditional_pragma_target(&empty_builtin), None);
        assert_eq!(
            conditional_pragma_target(&named_builtin),
            Some(("builtin", &named_builtin[2..]))
        );
    }
}
