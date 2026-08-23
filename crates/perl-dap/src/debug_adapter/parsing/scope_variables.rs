//! SRP helpers for `parse_scope_variables_from_lines`.
//!
//! Each function owns exactly one responsibility:
//! - [`parse_assignments`] — iterate, normalize, filter, dedupe, cap
//! - [`sort_and_paginate`] — reverse chronological order, sort, slice
//! - [`compute_child_reference`] — stable child-ref codec, or 0 when unrepresentable
//! - [`render_paged_variable`] — render one variable and its optional children

use super::super::{
    DebugAdapter, HashSet, PerlVariableRenderer, Variable, VariableParser, VariableRenderer,
};
use crate::value::PerlValue;

const MAX_CACHED_CHILDREN: usize = 1024;

/// Iterate `lines` in reverse, parse variable assignments, apply scope filter,
/// deduplicate by name, and cap at 256 entries.
///
/// Returns a vec built in reverse-iteration order (most-recent-first).
/// [`sort_and_paginate`] reverses it before sorting to restore chronological order.
pub(super) fn parse_assignments(lines: &[String], scope_type: i32) -> Vec<(String, PerlValue)> {
    let parser = VariableParser::new();
    let mut seen = HashSet::new();
    let mut parsed = Vec::new();

    for line in lines.iter().rev() {
        let normalized = DebugAdapter::normalize_debugger_output_line(line);
        let text = normalized.trim();
        if text.is_empty() {
            continue;
        }
        if let Ok((name, value)) = parser.parse_assignment(text) {
            if !DebugAdapter::scope_allows_variable_name(scope_type, &name) {
                continue;
            }
            if seen.insert(name.clone()) {
                parsed.push((name, value));
            }
            if parsed.len() >= 256 {
                break;
            }
        }
    }

    parsed
}

/// Reverse the vec (restoring chronological order), sort by name, then
/// apply `skip(start).take(count)` pagination.
pub(super) fn sort_and_paginate(
    mut parsed: Vec<(String, PerlValue)>,
    start: usize,
    count: usize,
) -> Vec<(String, PerlValue)> {
    parsed.reverse();
    parsed.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
    parsed.into_iter().skip(start).take(count).collect()
}

/// Compute a stable child reference integer for a paged variable entry.
///
/// Child references use the canonical disjoint-band codec. `start + idx` is
/// the zero-based absolute child index within the parent scope/evaluation result.
///
/// Returns `0` (DAP: "not expandable") when the pair cannot be represented
/// exactly. The `Child` band packs `parent << 16 | index`, so it is lossy at
/// two edges: a parent above 2 250 saturates to `i32::MAX`, and an index above
/// 65 535 wraps modulo 2^16. Both make distinct entries share one reference,
/// and callers key the child cache by that reference — so a lossy encoding
/// would serve one variable's children under another variable's handle.
/// Refusing the handle keeps the entry honestly unexpanded instead.
pub(super) fn compute_child_reference(variables_ref: i32, start: usize, idx: usize) -> i32 {
    use crate::debug_adapter::var_ref::VariableReference;

    let absolute_index = start.saturating_add(idx);
    let Ok(index) = u32::try_from(absolute_index) else {
        return 0;
    };
    let requested = VariableReference::Child { parent: variables_ref, index };
    let Some(reference) = requested.encode() else {
        return 0;
    };
    // Only hand out a reference that round-trips to exactly what was asked for.
    if VariableReference::decode(reference) == Some(requested) { reference } else { 0 }
}

/// Render a single variable and, if expandable, its children.
///
/// Returns `(top_level_variable, Some((child_ref, children)))` when the value
/// is expandable and has at least one child; otherwise `None` for the second
/// tuple element.
///
/// A non-positive `child_ref` means [`compute_child_reference`] refused to mint
/// a representable handle. The value is then rendered without a reference: the
/// client sees the aggregate but no expand affordance, which is honest, rather
/// than a handle that would collide with another entry's cached children.
pub(super) fn render_paged_variable(
    name: String,
    value: PerlValue,
    child_ref: i32,
) -> (Variable, Option<(i32, Vec<Variable>)>) {
    let renderer = PerlVariableRenderer::new();
    let expandable = value.is_expandable() && child_ref > 0;

    let rendered = if expandable {
        renderer.render_with_reference(&name, &value, i64::from(child_ref))
    } else {
        renderer.render(&name, &value)
    };
    let top = DebugAdapter::rendered_to_variable(rendered);

    let cache_entry = if expandable {
        let children = renderer
            .render_children(&value, 0, MAX_CACHED_CHILDREN)
            .into_iter()
            .map(DebugAdapter::rendered_to_variable)
            .collect::<Vec<_>>();
        if children.is_empty() { None } else { Some((child_ref, children)) }
    } else {
        None
    };

    (top, cache_entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debug_adapter::var_ref::VariableReference;

    #[test]
    fn child_reference_uses_canonical_codec() {
        let reference = compute_child_reference(11, 250, 0);
        assert_eq!(
            VariableReference::decode(reference),
            Some(VariableReference::Child { parent: 11, index: 250 })
        );
    }

    /// The `Child` band saturates once `parent` exceeds 2 250 (a scope reference
    /// for frame_id ≥ 225). Every entry in such a scope would otherwise encode to
    /// `i32::MAX` and share one child-cache key, so expanding one aggregate would
    /// serve a different aggregate's children.
    #[test]
    fn deep_frame_parent_refuses_rather_than_aliasing() {
        let deep_parent = 2251; // frame_id 225, Locals
        let first = compute_child_reference(deep_parent, 0, 0);
        let second = compute_child_reference(deep_parent, 0, 1);
        assert_eq!(first, 0, "unrepresentable parent must not mint a reference");
        assert_eq!(second, 0, "unrepresentable parent must not mint a reference");

        // The largest exactly representable parent still pages normally.
        let last_exact = compute_child_reference(2250, 0, 0);
        let last_exact_next = compute_child_reference(2250, 0, 1);
        assert_ne!(last_exact, last_exact_next);
        assert_eq!(
            VariableReference::decode(last_exact),
            Some(VariableReference::Child { parent: 2250, index: 0 })
        );
    }

    /// The index half of the `Child` band is 16 bits, so absolute index 70 000
    /// would wrap onto 4 464 and collide with a much earlier page.
    #[test]
    fn index_past_sixteen_bits_refuses_rather_than_wrapping() {
        let wrapped = compute_child_reference(11, 70_000, 0);
        let collides_with = compute_child_reference(11, 4_464, 0);
        assert_eq!(wrapped, 0, "index past 2^16 must not mint a wrapped reference");
        assert_ne!(collides_with, 0, "an exactly representable index still pages");
    }

    /// A refused reference must leave the entry unexpanded and uncached — never
    /// cached under key 0, which DAP reserves for "no children".
    #[test]
    fn refused_reference_renders_unexpanded_and_uncached() {
        let value = PerlValue::Array(vec![PerlValue::Integer(1), PerlValue::Integer(2)]);
        assert!(value.is_expandable(), "fixture must be an expandable aggregate");

        let (top, cache_entry) = render_paged_variable("@deep".to_string(), value, 0);
        assert_eq!(top.variables_reference, 0, "refused entry must not advertise expansion");
        assert!(cache_entry.is_none(), "refused entry must not populate the child cache");
    }
}
