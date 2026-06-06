use std::sync::{Arc, OnceLock};

// Pre-allocated empty Arc to avoid repeated allocations.
static EMPTY_ARC: OnceLock<Arc<str>> = OnceLock::new();

#[inline(always)]
pub(crate) fn empty_arc() -> Arc<str> {
    EMPTY_ARC.get_or_init(|| Arc::from("")).clone()
}

pub(crate) fn truncate_preview(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((idx, _)) => format!("{}...", &text[..idx]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{empty_arc, truncate_preview};
    use std::sync::Arc;

    #[test]
    fn empty_arc_reuses_shared_allocation() -> Result<(), Box<dyn std::error::Error>> {
        let first = empty_arc();
        let second = empty_arc();

        assert!(first.is_empty());
        assert!(Arc::ptr_eq(&first, &second));
        Ok(())
    }

    #[test]
    fn truncate_preview_preserves_short_input() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(truncate_preview("short", 10), "short");
        assert_eq!(truncate_preview("", 10), "");
        Ok(())
    }

    #[test]
    fn truncate_preview_respects_unicode_boundaries() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(truncate_preview("åβçdé", 3), "åβç...");
        assert_eq!(truncate_preview("abcdef", 0), "...");
        Ok(())
    }
}
