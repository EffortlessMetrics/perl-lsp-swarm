use super::FormatConfig;

/// Legacy document-tree DTO; rendering is delegated to `perl-tidy`.
///
/// Variant constructors remain available to existing callers. Rendering
/// converts this DTO to the canonical IR; all layout decisions belong there.
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

    /// Render through the canonical native IR with the legacy style projection.
    ///
    /// As before, this syntax-independent IR consumes style only; it does not
    /// select or execute a formatter engine.
    #[must_use]
    pub fn render(&self, config: &FormatConfig) -> String {
        self.to_native().render(&config.native_policy())
    }

    fn to_native(&self) -> perl_tidy::FormatDoc {
        use perl_tidy::FormatDoc as NativeDoc;
        match self {
            Self::Text(text) => NativeDoc::Text(text.clone()),
            Self::Space => NativeDoc::Space,
            Self::Line => NativeDoc::Line,
            Self::SoftLine => NativeDoc::SoftLine,
            Self::HardLine => NativeDoc::HardLine,
            Self::Group(parts) => NativeDoc::Group(parts.iter().map(Self::to_native).collect()),
            Self::Indent(parts) => NativeDoc::Indent(parts.iter().map(Self::to_native).collect()),
            Self::IfBreak { broken, flat } => NativeDoc::IfBreak {
                broken: Box::new(broken.to_native()),
                flat: Box::new(flat.to_native()),
            },
            Self::LiteralPreserve(text) => NativeDoc::LiteralPreserve(text.clone()),
        }
    }
}
