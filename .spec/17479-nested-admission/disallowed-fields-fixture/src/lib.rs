#![deny(clippy::disallowed_fields)]

pub fn range_start(range: std::ops::Range<usize>) -> usize {
    range.start
}
