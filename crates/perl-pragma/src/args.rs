use crate::PragmaState;

const MAX_DISABLED_WARNING_CATEGORIES: usize = 256;

pub(crate) fn builtin_import_names(arg: &str) -> Vec<String> {
    let trimmed = normalized_pragma_token(arg);

    if let Some(inner) = qw_list_inner(trimmed) {
        return pragma_words(inner).into_iter().map(|name| name.to_string()).collect();
    }

    if trimmed.is_empty() { Vec::new() } else { vec![trimmed.to_string()] }
}

pub(crate) fn apply_builtin_imports(state: &mut PragmaState, args: &[String]) {
    for arg in args {
        for name in builtin_import_names(arg) {
            if !state.builtin_imports.iter().any(|import| import == &name) {
                state.builtin_imports.push(name);
            }
        }
    }
}

/// Insert `category` into `state.disabled_warning_categories` if not already present and
/// within the hard cap of [`MAX_DISABLED_WARNING_CATEGORIES`].
///
/// Categories beyond the cap are silently dropped. In valid Perl code this is never reached
/// (Perl's own warning hierarchy has ~30 leaf categories); the cap is a safety guard against
/// pathological or adversarial AST input that would otherwise cause O(n²) clone cost.
pub(crate) fn add_disabled_warning_category(state: &mut PragmaState, category: &str) {
    if category.is_empty() {
        return;
    }

    if state.disabled_warning_categories.iter().any(|c| c == category) {
        return;
    }

    if state.disabled_warning_categories.len() >= MAX_DISABLED_WARNING_CATEGORIES {
        return;
    }

    state.disabled_warning_categories.push(category.to_string());
}

pub(crate) fn remove_builtin_imports(state: &mut PragmaState, args: &[String]) {
    if args.is_empty() {
        state.builtin_imports.clear();
        return;
    }

    let names_to_remove: Vec<String> =
        args.iter().flat_map(|arg| builtin_import_names(arg)).collect();
    state.builtin_imports.retain(|import| !names_to_remove.iter().any(|name| name == import));
}

pub(crate) fn pragma_arg_items(arg: &str) -> Vec<String> {
    let trimmed = normalized_pragma_token(arg);

    if let Some(inner) = qw_list_inner(trimmed) {
        return pragma_words(inner).into_iter().map(|item| item.to_string()).collect();
    }

    if trimmed.contains(char::is_whitespace) {
        return pragma_words(trimmed).into_iter().map(|item| item.to_string()).collect();
    }

    vec![trimmed.to_string()]
}

fn qw_list_inner(arg: &str) -> Option<&str> {
    let rest = arg.strip_prefix("qw")?.trim_start();
    let opener = rest.chars().next()?;
    let closer = qw_closer(opener)?;
    let after_opener = &rest[opener.len_utf8()..];

    after_opener.strip_suffix(closer)
}

fn qw_closer(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        delimiter if !delimiter.is_alphanumeric() && !delimiter.is_whitespace() => Some(delimiter),
        _ => None,
    }
}

fn pragma_words(value: &str) -> Vec<&str> {
    value
        .split_whitespace()
        .map(|item| item.trim_matches('\'').trim_matches('"'))
        .filter(|item| !item.is_empty())
        .collect()
}

pub(crate) fn normalized_pragma_token(arg: &str) -> &str {
    arg.trim().trim_matches('\'').trim_matches('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pragma_arg_items_splits_qw_forms_with_multiple_delimiters() {
        assert_eq!(pragma_arg_items("qw(vars subs)"), vec!["vars", "subs"]);
        assert_eq!(pragma_arg_items("qw< deprecated io >"), vec!["deprecated", "io"]);
        assert_eq!(pragma_arg_items("qw!alpha beta!"), vec!["alpha", "beta"]);
    }

    #[test]
    fn pragma_arg_items_trims_quotes_and_whitespace() {
        assert_eq!(pragma_arg_items("  'uninitialized'  "), vec!["uninitialized"]);
        assert_eq!(pragma_arg_items("\"vars subs\""), vec!["vars", "subs"]);
    }

    #[test]
    fn invalid_qw_delimiter_falls_back_to_single_token() {
        assert_eq!(pragma_arg_items("qwAvars subsA"), vec!["qwAvars", "subsA"]);
    }

    #[test]
    fn builtin_imports_deduplicate_qw_and_single_name_imports() {
        let mut state = PragmaState::default();
        apply_builtin_imports(
            &mut state,
            &["qw(true false)".to_string(), "'true'".to_string(), "\"blessed\"".to_string()],
        );

        assert_eq!(state.builtin_imports, vec!["true", "false", "blessed"]);
    }

    #[test]
    fn remove_builtin_imports_removes_selected_names_or_all_names() {
        let mut state = PragmaState {
            builtin_imports: vec!["true".to_string(), "false".to_string(), "blessed".to_string()],
            ..PragmaState::default()
        };

        remove_builtin_imports(&mut state, &["qw(false true)".to_string()]);
        assert_eq!(state.builtin_imports, vec!["blessed"]);

        remove_builtin_imports(&mut state, &[]);
        assert!(state.builtin_imports.is_empty());
    }

    #[test]
    fn add_disabled_warning_category_ignores_empty_duplicate_and_over_cap_values() {
        let mut state = PragmaState::default();

        add_disabled_warning_category(&mut state, "");
        add_disabled_warning_category(&mut state, "uninitialized");
        add_disabled_warning_category(&mut state, "uninitialized");
        for index in 0..300 {
            add_disabled_warning_category(&mut state, &format!("category_{index}"));
        }

        assert_eq!(
            state.disabled_warning_categories.first().map(String::as_str),
            Some("uninitialized")
        );
        assert_eq!(state.disabled_warning_categories.len(), MAX_DISABLED_WARNING_CATEGORIES);
        assert!(
            !state.disabled_warning_categories.iter().any(|category| category == "category_299")
        );
    }
}
