//! DocumentState constructors for text synchronization.
//!
//! Keeps raw state assembly separate from LSP notification handlers so the
//! handlers can focus on protocol flow and parse/index decisions.

use super::*;
use crate::state::DegradationTier;

pub(super) fn minimal_state(text: &str, version: i32) -> DocumentState {
    let rope = ropey::Rope::from_str(text);
    minimal_state_from_rope(rope, text.to_string(), version, Arc::new(AtomicU32::new(0)))
}

pub(super) fn empty_state(version: i32) -> DocumentState {
    minimal_state_from_rope(ropey::Rope::new(), String::new(), version, Arc::new(AtomicU32::new(0)))
}

pub(super) fn minimal_state_from_rope(
    rope: ropey::Rope,
    text: String,
    version: i32,
    generation: Arc<AtomicU32>,
) -> DocumentState {
    let line_starts = LineStartsCache::new_rope(&rope);
    DocumentState {
        rope,
        text,
        version,
        ast: None,
        parse_errors: vec![],
        parent_map: ParentMap::default(),
        line_starts,
        generation,
        degradation_tier: DegradationTier::Minimal,
        #[cfg(feature = "incremental")]
        incremental_doc: None,
        #[cfg(feature = "incremental")]
        incremental_state: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_matches_minimal_state_for_empty_content() {
        let version = 17;

        let empty = empty_state(version);
        let minimal = minimal_state("", version);

        assert_eq!(empty.text, minimal.text);
        assert_eq!(empty.version, minimal.version);
        assert_eq!(empty.parse_errors, minimal.parse_errors);
        assert_eq!(empty.degradation_tier, minimal.degradation_tier);
        assert_eq!(empty.line_starts.position_to_offset("", 0, 0), 0);
        assert_eq!(minimal.line_starts.position_to_offset("", 0, 0), 0);
        assert_eq!(empty.generation.load(Ordering::Relaxed), 0);
    }
}
