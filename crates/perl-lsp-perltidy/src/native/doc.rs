use super::config::FormatConfig;

const MAX_LINE_WIDTH: usize = 1_000;
const MAX_INDENT_WIDTH: usize = 16;
const MAX_RENDER_INDENT_COLUMNS: usize = 4_096;

/// Native formatter document tree.
///
/// This is the small, lossless-friendly formatting IR from the replacement
/// contract. It is deliberately independent of Perl syntax for now; later
/// parser-facing formatter passes should lower CST/AST fragments into this
/// tree and then render it deterministically.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatDoc {
    /// Literal text that may be laid out with surrounding IR.
    Text(String),
    /// One ordinary space.
    Space,
    /// A newline at the current indentation level.
    Line,
    /// A line break that becomes a space when its containing group fits.
    SoftLine,
    /// A newline that cannot be flattened.
    HardLine,
    /// A layout group that may render flat or broken.
    Group(Vec<FormatDoc>),
    /// A nested document rendered one indentation level deeper when broken.
    Indent(Vec<FormatDoc>),
    /// Render one branch when broken and another branch when flat.
    IfBreak {
        /// Document to render when the containing group breaks.
        broken: Box<FormatDoc>,
        /// Document to render when the containing group fits flat.
        flat: Box<FormatDoc>,
    },
    /// Literal source text that must be preserved byte-for-byte.
    LiteralPreserve(String),
}

impl FormatDoc {
    /// Create literal text.
    #[must_use]
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Create a layout group.
    #[must_use]
    pub fn group(parts: impl Into<Vec<FormatDoc>>) -> Self {
        Self::Group(parts.into())
    }

    /// Create an indented document.
    #[must_use]
    pub fn indent(parts: impl Into<Vec<FormatDoc>>) -> Self {
        Self::Indent(parts.into())
    }

    /// Create an if-break choice.
    #[must_use]
    pub fn if_break(broken: FormatDoc, flat: FormatDoc) -> Self {
        Self::IfBreak { broken: Box::new(broken), flat: Box::new(flat) }
    }

    /// Create a literal-preserve region.
    #[must_use]
    pub fn literal_preserve(value: impl Into<String>) -> Self {
        Self::LiteralPreserve(value.into())
    }

    /// Render this document using the native formatter configuration.
    #[must_use]
    pub fn render(&self, config: &FormatConfig) -> String {
        let mut renderer = DocRenderer::new(config);
        renderer.render_doc(self, 0, false, false);
        renderer.output
    }

    fn flat_width(&self) -> Option<usize> {
        match self {
            Self::Text(text) | Self::LiteralPreserve(text) => {
                (!text.contains('\n')).then_some(text.chars().count())
            }
            Self::Space | Self::SoftLine => Some(1),
            Self::Line | Self::HardLine => None,
            Self::Group(parts) | Self::Indent(parts) => parts.iter().try_fold(
                0_usize,
                |sum, doc| doc.flat_width().and_then(|width| sum.checked_add(width)),
            ),
            Self::IfBreak { flat, .. } => flat.flat_width(),
        }
    }
}

struct DocRenderer {
    output: String,
    column: usize,
    line_width: usize,
    indent_width: usize,
    use_tabs: bool,
}

impl DocRenderer {
    fn new(config: &FormatConfig) -> Self {
        let line_width = usize::try_from(config.line_width)
            .unwrap_or(MAX_LINE_WIDTH)
            .clamp(1, MAX_LINE_WIDTH);
        let indent_width = usize::try_from(config.indent_width)
            .unwrap_or(MAX_INDENT_WIDTH)
            .clamp(1, MAX_INDENT_WIDTH);
        Self {
            output: String::new(),
            column: 0,
            line_width,
            indent_width,
            use_tabs: config.use_tabs,
        }
    }

    fn render_doc(&mut self, doc: &FormatDoc, indent_level: usize, flat: bool, broken: bool) {
        match doc {
            FormatDoc::Text(text) | FormatDoc::LiteralPreserve(text) => self.push_text(text),
            FormatDoc::Space => self.push_text(" "),
            FormatDoc::Line | FormatDoc::HardLine => self.push_line(indent_level),
            FormatDoc::SoftLine if flat => self.push_text(" "),
            FormatDoc::SoftLine => self.push_line(indent_level),
            FormatDoc::Group(parts) => {
                let fits = doc
                    .flat_width()
                    .and_then(|width| self.column.checked_add(width))
                    .is_some_and(|end_column| end_column <= self.line_width);
                for part in parts {
                    self.render_doc(part, indent_level, fits, !fits);
                }
            }
            FormatDoc::Indent(parts) => {
                let nested_indent = indent_level.saturating_add(1);
                for part in parts {
                    self.render_doc(part, nested_indent, flat, broken);
                }
            }
            FormatDoc::IfBreak { broken: broken_doc, flat: flat_doc } => {
                let selected = if broken { broken_doc } else { flat_doc };
                self.render_doc(selected, indent_level, flat, broken);
            }
        }
    }

    fn push_text(&mut self, text: &str) {
        self.output.push_str(text);
        if let Some((_, tail)) = text.rsplit_once('\n') {
            self.column = tail.chars().count();
        } else {
            self.column = self.column.saturating_add(text.chars().count());
        }
    }

    fn push_line(&mut self, indent_level: usize) {
        self.output.push('\n');
        let indent_columns = if self.use_tabs {
            indent_level.min(MAX_RENDER_INDENT_COLUMNS)
        } else {
            indent_level
                .checked_mul(self.indent_width)
                .unwrap_or(MAX_RENDER_INDENT_COLUMNS)
                .min(MAX_RENDER_INDENT_COLUMNS)
        };
        let indent = if self.use_tabs {
            "\t".repeat(indent_columns)
        } else {
            " ".repeat(indent_columns)
        };
        self.output.push_str(&indent);
        self.column = indent_columns;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_bounds_extreme_indent_width_before_allocation() {
        let config = FormatConfig { indent_width: u32::MAX, ..FormatConfig::default() };
        let doc = FormatDoc::indent(vec![FormatDoc::HardLine, FormatDoc::text("x")]);

        let rendered = doc.render(&config);

        assert_eq!(rendered, format!("\n{}x", " ".repeat(MAX_INDENT_WIDTH)));
    }

    #[test]
    fn renderer_bounds_zero_indent_width_to_a_nonzero_step() {
        let config = FormatConfig { indent_width: 0, ..FormatConfig::default() };
        let doc = FormatDoc::indent(vec![FormatDoc::HardLine, FormatDoc::text("x")]);

        assert_eq!(doc.render(&config), "\n x");
    }

    #[test]
    fn renderer_bounds_extreme_line_width_for_group_decisions() {
        let config = FormatConfig { line_width: u32::MAX, ..FormatConfig::default() };
        let doc = FormatDoc::group(vec![
            FormatDoc::text("x".repeat(MAX_LINE_WIDTH)),
            FormatDoc::SoftLine,
            FormatDoc::text("y"),
        ]);

        let rendered = doc.render(&config);

        assert!(rendered.contains('\n'));
        assert!(!rendered.contains(" y"));
    }
}
