use std::sync::Arc;

/// Specification for a pending heredoc.
#[derive(Clone)]
pub(crate) struct HeredocSpec {
    pub(crate) label: Arc<str>,
    pub(crate) body_start: usize,  // byte offset where the body begins
    pub(crate) allow_indent: bool, // true if we saw <<~ (Perl 5.26 indented heredocs)
    /// Whether the body interpolates (#8779): bareword and `<<"EOF"`
    /// delimiters interpolate; `<<'EOF'`, `<<\EOF`, and backtick delimiters
    /// do not (the backtick form is an intentional command boundary).
    pub(crate) interpolates: bool,
}
