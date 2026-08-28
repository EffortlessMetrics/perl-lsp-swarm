use crate::{
    PragmaState, add_disabled_warning_category, apply_builtin_imports_if_changed,
    apply_feature_state, conditional_pragma_target, normalized_pragma_token, parse_perl_version,
    pragma_arg_items,
};
use super::TrackedPragmaState;
use std::ops::Range;

pub(super) fn apply_use_directive(
    range: Range<usize>,
    module: &str,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    if apply_conditional_use(module, args, range.clone(), tracked, ranges) {
        return;
    }

    match module {
        "strict" => {
            set_strict_categories(&mut tracked.state, args, true);
            push_state(range, tracked, ranges);
        }
        "warnings" => {
            apply_use_warnings(range, args, tracked, ranges);
        }
        "utf8" => {
            tracked.state.utf8 = true;
            push_state(range, tracked, ranges);
        }
        "encoding" => {
            tracked.state.encoding = first_normalized_arg(args);
            push_state(range, tracked, ranges);
        }
        "locale" => {
            tracked.state.locale = true;
            tracked.state.locale_scope = first_normalized_arg(args);
            push_state(range, tracked, ranges);
        }
        "feature" => {
            if apply_feature_state(&mut tracked.state, args, true) {
                push_state(range, tracked, ranges);
            }
        }
        "experimental" => {
            // `use experimental 'class'` enables the corresponding feature, just
            // like `use feature 'class'`.  The experimental pragma maps feature
            // names directly (class, signatures, defer, try, postderef, isa, etc.).
            // (#5091)
            let feature_args: Vec<String> = args
                .iter()
                .flat_map(|arg| crate::pragma_arg_items(arg))
                .map(|item| format!("'{item}'"))
                .collect();
            if apply_feature_state(&mut tracked.state, &feature_args, true) {
                push_state(range, tracked, ranges);
            }
        }
        "builtin" => {
            if apply_builtin_imports_if_changed(&mut tracked.state, args) {
                push_state(range, tracked, ranges);
            }
        }
        _ => {
            if let Some(version) = parse_perl_version(module) {
                tracked.enable_version_semantics(version);
                push_state(range, tracked, ranges);
            }
        }
    }
}

pub(super) fn apply_no_directive(
    range: Range<usize>,
    module: &str,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    if apply_conditional_no(module, args, range.clone(), tracked, ranges) {
        return;
    }

    match module {
        "strict" => {
            set_strict_categories(&mut tracked.state, args, false);
            push_state(range, tracked, ranges);
        }
        "warnings" => {
            apply_no_warnings(range, args, tracked, ranges);
        }
        "utf8" => {
            tracked.state.utf8 = false;
            push_state(range, tracked, ranges);
        }
        "encoding" => {
            tracked.state.encoding = None;
            push_state(range, tracked, ranges);
        }
        "locale" => {
            tracked.state.locale = false;
            tracked.state.locale_scope = None;
            push_state(range, tracked, ranges);
        }
        "feature" if apply_feature_state(&mut tracked.state, args, false) => {
            push_state(range, tracked, ranges);
        }
        "experimental" if apply_feature_state(&mut tracked.state, args, false) => {
            push_state(range, tracked, ranges);
        }
        "builtin" => {}
        _ => {}
    }
}

fn apply_conditional_use(
    module: &str,
    args: &[String],
    range: Range<usize>,
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) -> bool {
    if !matches!(module, "if" | "unless") {
        return false;
    }

    if let Some((target, target_args)) = conditional_pragma_target(args) {
        apply_conditional_use_target(range, target, target_args, tracked, ranges);
    }
    true
}

fn apply_conditional_no(
    module: &str,
    args: &[String],
    range: Range<usize>,
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) -> bool {
    if !matches!(module, "if" | "unless") {
        return false;
    }

    if let Some((target, target_args)) = conditional_pragma_target(args) {
        apply_conditional_no_target(range, target, target_args, tracked, ranges);
    }
    true
}

fn apply_conditional_use_target(
    range: Range<usize>,
    module: &str,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    match module {
        "strict" => set_strict_categories(&mut tracked.state, args, true),
        "warnings" => {
            enable_warnings_categories(args, &mut tracked.state);
        }
        "utf8" => tracked.state.utf8 = true,
        "encoding" => tracked.state.encoding = first_normalized_arg(args),
        "locale" => {
            tracked.state.locale = true;
            tracked.state.locale_scope = first_normalized_arg(args);
        }
        "feature" => {
            if !apply_feature_state(&mut tracked.state, args, true) {
                return;
            }
        }
        "builtin" => {
            if !apply_builtin_imports_if_changed(&mut tracked.state, args) {
                return;
            }
        }
        _ => {
            if let Some(version) = parse_perl_version(module) {
                tracked.enable_version_semantics(version);
            } else {
                return;
            }
        }
    }
    push_state(range, tracked, ranges);
}

fn apply_conditional_no_target(
    range: Range<usize>,
    module: &str,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    match module {
        "strict" => set_strict_categories(&mut tracked.state, args, false),
        "warnings" => disable_warnings_categories(args, &mut tracked.state),
        "utf8" => tracked.state.utf8 = false,
        "encoding" => tracked.state.encoding = None,
        "locale" => {
            tracked.state.locale = false;
            tracked.state.locale_scope = None;
        }
        "feature" => {
            if !apply_feature_state(&mut tracked.state, args, false) {
                return;
            }
        }
        "builtin" => return,
        _ => return,
    }
    push_state(range, tracked, ranges);
}

fn set_strict_categories(state: &mut PragmaState, args: &[String], enabled: bool) {
    if args.is_empty() {
        state.strict_vars = enabled;
        state.strict_subs = enabled;
        state.strict_refs = enabled;
        return;
    }

    for arg in args {
        for item in pragma_arg_items(arg) {
            match item.as_str() {
                "vars" => state.strict_vars = enabled,
                "subs" => state.strict_subs = enabled,
                "refs" => state.strict_refs = enabled,
                _ => {}
            }
        }
    }
}

fn apply_use_warnings(
    range: Range<usize>,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    enable_warnings_categories(args, &mut tracked.state);
    push_state(range, tracked, ranges);
}

fn enable_warnings_categories(args: &[String], state: &mut PragmaState) {
    state.warnings = true;

    if args.is_empty() {
        state.disabled_warning_categories.clear();
        return;
    }

    for arg in args {
        for category in pragma_arg_items(arg) {
            if category == "all" {
                // `use warnings 'all'` re-enables every category, exactly like a
                // bare `use warnings`. Clearing the disabled set keeps the list
                // accurate so a later per-category query does not read a stale
                // `disabled_warning_categories` entry as still disabled after the
                // blanket re-enable.
                state.disabled_warning_categories.clear();
            } else {
                state.disabled_warning_categories.retain(|disabled| disabled != &category);
            }
        }
    }
}

fn apply_no_warnings(
    range: Range<usize>,
    args: &[String],
    tracked: &mut TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    let warnings_before = tracked.state.warnings;
    let had_disabled_before = !tracked.state.disabled_warning_categories.is_empty();
    let before = tracked.state.disabled_warning_categories.len();
    disable_warnings_categories(args, &mut tracked.state);

    let changed = if args.is_empty() {
        warnings_before || had_disabled_before
    } else {
        tracked.state.disabled_warning_categories.len() != before
    };
    if changed {
        push_state(range, tracked, ranges);
    }
}

fn disable_warnings_categories(args: &[String], state: &mut PragmaState) {
    if args.is_empty() {
        state.warnings = false;
        state.disabled_warning_categories.clear();
        return;
    }

    for arg in args {
        for category in pragma_arg_items(arg) {
            add_disabled_warning_category(state, &category);
        }
    }
}

fn first_normalized_arg(args: &[String]) -> Option<String> {
    args.first().map(|arg| normalized_pragma_token(arg).to_string())
}

fn push_state(
    range: Range<usize>,
    tracked: &TrackedPragmaState,
    ranges: &mut Vec<(Range<usize>, TrackedPragmaState)>,
) {
    ranges.push((range, tracked.clone()));
}
