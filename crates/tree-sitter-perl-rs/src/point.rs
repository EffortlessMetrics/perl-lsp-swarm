/// A tree-sitter-compatible source position.
///
/// `row` and `column` are both zero-based and `column` is measured in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Point {
    /// Zero-based row number.
    pub row: usize,
    /// Zero-based byte column within `row`.
    pub column: usize,
}
