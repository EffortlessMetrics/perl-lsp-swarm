use super::capture::{CaptureGroup, extract_named_captures as direct_projection};

pub(crate) fn extract_named_captures(pattern: &str) -> Vec<CaptureGroup> {
    direct_projection(pattern)
}
