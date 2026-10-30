//! Provider-local spelling assistance for missing executable route edges (#6913).
//!
//! Suggestion text must not decide edgehood, suppress a hard error, or depend on
//! today's hyphenated skill-name shape. Ambiguous or distant candidates stay silent.

use std::collections::BTreeSet;

/// Closest known skill name within a small edit budget, if one name is uniquely closest.
pub(super) fn unique_near_miss<'a>(
    target: &str,
    known_names: &'a BTreeSet<String>,
) -> Option<&'a str> {
    let budget = suggestion_budget(target);
    let mut best: Option<(usize, &'a str)> = None;
    let mut tied = false;
    for name in known_names {
        let distance = edit_distance(target, name);
        if distance == 0 || distance > budget {
            continue;
        }
        match best {
            Some((best_distance, _)) if distance > best_distance => {}
            Some((best_distance, _)) if distance == best_distance => tied = true,
            _ => {
                best = Some((distance, name.as_str()));
                tied = false;
            }
        }
    }
    match best {
        Some((_, name)) if !tied => Some(name),
        _ => None,
    }
}

fn suggestion_budget(target: &str) -> usize {
    if target.len() <= 6 { 1 } else { 2 }
}

/// Levenshtein distance over bytes. Skill names are ASCII kebab-case.
fn edit_distance(left: &str, right: &str) -> usize {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len().saturating_add(1)];

    for (row, left_byte) in left.iter().copied().enumerate() {
        let Some(leading) = current.first_mut() else {
            return left.len().saturating_add(right.len());
        };
        *leading = row.saturating_add(1);
        for (col, right_byte) in right.iter().copied().enumerate() {
            let substitution = previous
                .get(col)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(usize::from(left_byte != right_byte));
            let deletion = previous
                .get(col.saturating_add(1))
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let insertion = current.get(col).copied().unwrap_or(usize::MAX).saturating_add(1);
            if let Some(cell) = current.get_mut(col.saturating_add(1)) {
                *cell = substitution.min(deletion).min(insertion);
            }
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous.last().copied().unwrap_or(left.len().saturating_add(right.len()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{edit_distance, unique_near_miss};

    fn names(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn unique_close_typo_selects_the_intended_skill() {
        let known = names(&["deliver-pr", "finish-pr", "review-tests", "build-candidate"]);
        assert_eq!(unique_near_miss("delver-pr", &known), Some("deliver-pr"));
        assert_eq!(unique_near_miss("deliver-pd", &known), Some("deliver-pr"));
        assert_eq!(unique_near_miss("deliver-rp", &known), Some("deliver-pr"));
    }

    #[test]
    fn closer_in_budget_candidate_beats_a_farther_in_budget_candidate() {
        let known = names(&["deliver-pr", "deliver-proof"]);
        assert_eq!(unique_near_miss("deliver-pd", &known), Some("deliver-pr"));
    }

    #[test]
    fn hyphenation_is_not_required_for_a_unique_near_miss() {
        let known = names(&["deliver", "finish"]);
        assert_eq!(unique_near_miss("delivr", &known), Some("deliver"));
    }

    #[test]
    fn ambiguous_equal_distance_candidates_produce_no_suggestion() {
        let known = names(&["review-plan", "review-plat"]);
        assert_eq!(unique_near_miss("review-plax", &known), None);
    }

    #[test]
    fn distant_unknown_produces_no_suggestion() {
        let known = names(&["deliver-pr", "finish-pr", "review-tests", "build-candidate"]);
        assert_eq!(unique_near_miss("archive-corpus-nightly", &known), None);
    }

    #[test]
    fn short_unrelated_token_is_not_stretched_into_a_skill_name() {
        let known = names(&["deliver-pr", "finish-pr", "review-tests", "build-candidate"]);
        assert_eq!(unique_near_miss("clear", &known), None);
        assert_eq!(unique_near_miss("path", &known), None);
        assert_eq!(unique_near_miss("main", &known), None);
    }

    #[test]
    fn empty_inventory_and_exact_match_produce_no_suggestion() {
        assert_eq!(unique_near_miss("delver-pr", &BTreeSet::new()), None);
        let known = names(&["deliver-pr"]);
        assert_eq!(unique_near_miss("deliver-pr", &known), None);
    }

    #[test]
    fn edit_distance_is_symmetric_and_zero_on_equality() {
        assert_eq!(edit_distance("finish-pr", "finish-pr"), 0);
        assert_eq!(edit_distance("finish-pr", "finish-p"), 1);
        assert_eq!(edit_distance("finish-p", "finish-pr"), 1);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
        assert_eq!(edit_distance("", ""), 0);
    }
}
