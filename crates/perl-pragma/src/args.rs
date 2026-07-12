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
    apply_builtin_imports_if_changed(state, args);
}

/// Like [`apply_builtin_imports`] but returns `true` when at least one new name was imported.
///
/// Used by the directive walker to suppress redundant pragma-map entries when a `use builtin`
/// statement lists names that are already in scope.
pub(crate) fn apply_builtin_imports_if_changed(state: &mut PragmaState, args: &[String]) -> bool {
    let mut changed = false;
    for arg in args {
        for name in builtin_import_names(arg) {
            if !state.builtin_imports.iter().any(|import| import == &name) {
                state.builtin_imports.push(name);
                changed = true;
            }
        }
    }
    changed
}

/// Insert `category` into `state.disabled_warning_categories` if not already present and
/// within the hard cap of [`MAX_DISABLED_WARNING_CATEGORIES`].
///
/// Returns `true` when the category was newly inserted; `false` when it was already present
/// or the cap was reached.  Callers can use the return value to suppress redundant pragma-map
/// entries. Categories beyond the cap are silently dropped — Perl's own warning hierarchy has
/// ~30 leaf categories, so the cap is a safety guard against adversarial AST input that would
/// otherwise cause O(n²) clone cost.
pub(crate) fn disable_warning_category_if_new(state: &mut PragmaState, category: &str) -> bool {
    if category.is_empty() {
        return false;
    }

    if state.disabled_warning_categories.iter().any(|c| c == category) {
        return false;
    }

    if state.disabled_warning_categories.len() >= MAX_DISABLED_WARNING_CATEGORIES {
        return false;
    }

    state.disabled_warning_categories.push(category.to_string());
    true
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
