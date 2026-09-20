use crate::types::Variable;
use crate::value::PerlValue;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VariableCacheKind {
    Root,
    Child,
    /// Cached result from evaluate/setExpression/setVariable for structured expansion.
    EvaluateResult,
}

/// One retained cache row: the policy-neutral (decimal) protocol row plus the
/// typed value captured at acquisition time, when one exists (#9588).
///
/// The display `value` in `row` is always the default rendering; a request's
/// DAP `ValueFormat` is projected from `typed` at response time only, so
/// formatting can never leak into the cache or change row identity. Rows
/// without typed facts (frame arguments, fallback placeholders, opaque
/// evaluate results) project to their cached display unchanged under any
/// format.
#[derive(Debug, Clone)]
pub(super) struct CachedVariable {
    pub(super) row: Variable,
    pub(super) typed: Option<PerlValue>,
}

impl CachedVariable {
    /// Wraps an untyped row (no typed numeric authority retained).
    pub(super) fn untyped(row: Variable) -> Self {
        Self { row, typed: None }
    }

    /// Wraps a row with its typed value.
    pub(super) fn typed(row: Variable, typed: PerlValue) -> Self {
        Self { row, typed: Some(typed) }
    }
}

#[derive(Debug, Clone)]
struct VariableCacheEntry {
    kind: VariableCacheKind,
    full: Vec<CachedVariable>,
    page_slices: HashMap<(usize, usize), Vec<CachedVariable>>,
}

#[derive(Debug, Default)]
pub(super) struct VariableCache {
    entries: HashMap<i32, VariableCacheEntry>,
}

impl VariableCache {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }

    pub(super) fn upsert(
        &mut self,
        reference: i32,
        kind: VariableCacheKind,
        variables: Vec<CachedVariable>,
    ) {
        let _ = self.entries.insert(
            reference,
            VariableCacheEntry { kind, full: variables, page_slices: HashMap::new() },
        );
    }

    pub(super) fn get_page(
        &mut self,
        reference: i32,
        start: usize,
        count: usize,
    ) -> Option<Vec<CachedVariable>> {
        let entry = self.entries.get_mut(&reference)?;
        let key = (start, count);
        if let Some(cached) = entry.page_slices.get(&key) {
            return Some(cached.clone());
        }

        let page = slice_variables(&entry.full, start, count);
        let _ = entry.page_slices.insert(key, page.clone());
        Some(page)
    }

    /// Returns the total number of variables stored for the given reference, or `None` if the
    /// reference is not in the cache. This is the pre-pagination count, suitable for populating
    /// the DAP `totalVariables` field.
    pub(super) fn root_count(&self, reference: i32) -> Option<usize> {
        self.entries.get(&reference).map(|e| e.full.len())
    }

    pub(super) fn all_variables(&self) -> impl Iterator<Item = &Variable> {
        self.entries
            .values()
            .filter(|entry| entry.kind == VariableCacheKind::Root)
            .chain(self.entries.values().filter(|entry| entry.kind == VariableCacheKind::Child))
            .chain(
                self.entries
                    .values()
                    .filter(|entry| entry.kind == VariableCacheKind::EvaluateResult),
            )
            .flat_map(|entry| entry.full.iter().map(|cached| &cached.row))
    }
}

pub(super) fn slice_variables(
    variables: &[CachedVariable],
    start: usize,
    count: usize,
) -> Vec<CachedVariable> {
    variables.iter().skip(start).take(count).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_variable(name: &str) -> CachedVariable {
        CachedVariable::untyped(Variable {
            name: name.to_string(),
            value: "test".to_string(),
            type_: None,
            variables_reference: 0,
            named_variables: None,
            indexed_variables: None,
            evaluate_name: None,
        })
    }

    /// all_variables() includes EvaluateResult-kind entries.
    ///
    /// Covered changed lines: ~63-66  EvaluateResult chain in all_variables.
    #[test]
    fn all_variables_includes_evaluate_result_entries() {
        let mut cache = VariableCache::default();
        cache.upsert(1, VariableCacheKind::Root, vec![make_variable("root_var")]);
        cache.upsert(2, VariableCacheKind::EvaluateResult, vec![make_variable("eval_result")]);

        let names: Vec<&str> = cache.all_variables().map(|v| v.name.as_str()).collect();
        assert!(
            names.contains(&"root_var"),
            "all_variables must include Root entries; got {names:?}"
        );
        assert!(
            names.contains(&"eval_result"),
            "all_variables must include EvaluateResult entries; got {names:?}"
        );
    }

    /// all_variables() with only EvaluateResult entries returns those entries.
    ///
    /// Exercises the EvaluateResult chain path when Root/Child are absent.
    #[test]
    fn all_variables_evaluate_result_only() {
        let mut cache = VariableCache::default();
        cache.upsert(
            10,
            VariableCacheKind::EvaluateResult,
            vec![make_variable("x"), make_variable("y")],
        );

        let names: Vec<&str> = cache.all_variables().map(|v| v.name.as_str()).collect();
        assert_eq!(names.len(), 2, "expected 2 EvaluateResult variables; got {names:?}");
        assert!(names.contains(&"x"), "must contain 'x'");
        assert!(names.contains(&"y"), "must contain 'y'");
    }
}
