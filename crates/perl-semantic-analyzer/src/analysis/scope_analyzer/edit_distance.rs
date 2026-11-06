//! Edit-distance helpers for nearest-match variable suggestions (#1770).

use rustc_hash::FxHashSet;

/// Names shorter than this skip nearest-match suggestions (avoids `$i` → `$x` noise).
const MIN_SUGGESTION_NAME_LEN: usize = 3;

/// Levenshtein distance between two identifiers (ASCII case-insensitive).
pub(super) fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.to_ascii_lowercase();
    let right = right.to_ascii_lowercase();
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();

    let mut previous: Vec<usize> = (0..=right_bytes.len()).collect();
    let mut current = vec![0; right_bytes.len() + 1];

    for (left_index, left_byte) in left_bytes.iter().enumerate() {
        current[0] = left_index + 1;

        for (right_index, right_byte) in right_bytes.iter().enumerate() {
            let substitution_cost = usize::from(left_byte != right_byte);
            let deletion = previous[right_index + 1] + 1;
            let insertion = current[right_index] + 1;
            let substitution = previous[right_index] + substitution_cost;
            current[right_index + 1] = deletion.min(insertion).min(substitution);
        }

        std::mem::swap(&mut previous, &mut current);
    }

    previous[right_bytes.len()]
}

/// Dynamic threshold aligned with perl-dap command typo suggestions.
fn suggestion_threshold(left: &str, right: &str) -> usize {
    let max_len = left.len().max(right.len());
    if max_len < MIN_SUGGESTION_NAME_LEN {
        return 0;
    }
    max_len.saturating_div(4).clamp(1, 4)
}

/// Collect declared lexical names visible from `scope` for the given sigil.
fn visible_variable_names(scope: &super::Scope, sigil: &str) -> Vec<String> {
    let idx = super::sigil_to_index(sigil);
    let mut names = Vec::new();
    let mut seen = FxHashSet::default();
    let mut current = scope;

    loop {
        {
            let vars = current.variables.borrow();
            if let Some(map) = &vars[idx] {
                for name in map.keys() {
                    if seen.insert(name.clone()) {
                        names.push(name.clone());
                    }
                }
            }
        }

        if let Some(ref parent) = current.parent {
            current = parent;
        } else {
            break;
        }
    }

    names
}

/// Rank up to three full variable names (`$count`) similar to the typo within `scope`.
pub(super) fn similar_variable_names(
    scope: &super::Scope,
    sigil: &str,
    typo_name: &str,
) -> Vec<String> {
    if typo_name.len() < MIN_SUGGESTION_NAME_LEN {
        return Vec::new();
    }

    let mut ranked: Vec<(usize, String)> = visible_variable_names(scope, sigil)
        .into_iter()
        .filter(|candidate| candidate != typo_name)
        .filter_map(|candidate| {
            let distance = edit_distance(typo_name, &candidate);
            let threshold = suggestion_threshold(typo_name, &candidate);
            (distance <= threshold).then_some((distance, format!("{sigil}{candidate}")))
        })
        .collect();

    ranked.sort_by(|(left_distance, left_name), (right_distance, right_name)| {
        left_distance
            .cmp(right_distance)
            .then_with(|| left_name.len().cmp(&right_name.len()))
            .then_with(|| left_name.cmp(right_name))
    });

    ranked.into_iter().take(3).map(|(_, full_name)| full_name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use rustc_hash::FxHashMap;

    use super::super::{Scope, Variable};

    fn scope_with_scalar_names(names: &[&str]) -> Rc<Scope> {
        let scope = Rc::new(Scope::new());
        {
            let mut vars = scope.variables.borrow_mut();
            let map = vars[0].get_or_insert_with(FxHashMap::default);
            for name in names {
                map.insert(
                    (*name).to_string(),
                    Rc::new(Variable {
                        declaration_offset: 0,
                        is_used: RefCell::new(false),
                        is_our: false,
                        is_initialized: RefCell::new(true),
                        is_pre_pass_placeholder: false,
                    }),
                );
            }
        }
        scope
    }

    #[test]
    fn single_character_typo_finds_nearest_scalar() {
        let scope = scope_with_scalar_names(&["count"]);
        let similar = similar_variable_names(scope.as_ref(), "$", "cont");
        assert_eq!(similar, vec!["$count".to_string()]);
    }

    #[test]
    fn unrelated_name_is_not_suggested() {
        let scope = scope_with_scalar_names(&["totally_different"]);
        let similar = similar_variable_names(scope.as_ref(), "$", "cont");
        assert!(similar.is_empty());
    }

    #[test]
    fn very_short_undeclared_name_does_not_suggest_unrelated() {
        let scope = scope_with_scalar_names(&["x"]);
        let similar = similar_variable_names(scope.as_ref(), "$", "i");
        assert!(similar.is_empty());
    }
}
