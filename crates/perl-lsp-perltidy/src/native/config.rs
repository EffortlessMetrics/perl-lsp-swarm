use serde::{Deserialize, Serialize};

pub use perl_tidy::{
    BracePlacement, ElsePlacement, FinalNewline, FormatterMode, FormatterSelection, KeywordSpacing,
    MAX_NATIVE_INDENT_WIDTH, MAX_NATIVE_LINE_WIDTH, NativeConfigError, NativeFormatConfig,
    TrailingComma,
};

/// Former native configuration, retained as a compatibility input.
///
/// `Native` translates to pure native style. `Off` returns a disabled result.
/// `ExternalLegacy` is explicitly refused by [`super::NativeFormatter`]; callers
/// must use an external adapter to execute that selection.
#[deprecated(
    since = "0.17.0",
    note = "migrate by 0.18.0 (#9292): use perl_tidy::NativeFormatConfig and select engines in the application"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatConfig {
    /// Legacy application selection, never passed to the native renderer.
    pub mode: FormatterMode,
    /// Preferred line width.
    pub line_width: u32,
    /// Indentation width when spaces are used.
    pub indent_width: u32,
    /// Whether indentation should use tabs instead of spaces.
    pub use_tabs: bool,
    /// Final newline handling.
    pub final_newline: FinalNewline,
    /// Trailing comma handling for wrapped delimited expressions.
    pub trailing_comma: TrailingComma,
    /// Opening brace placement for supported block layouts.
    pub brace_placement: BracePlacement,
    /// Else/elsif placement for supported block tails.
    pub else_placement: ElsePlacement,
    /// Keyword spacing for supported control-flow condition headers.
    pub keyword_spacing: KeywordSpacing,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self::from(NativeFormatConfig::default())
    }
}

impl From<NativeFormatConfig> for FormatConfig {
    fn from(config: NativeFormatConfig) -> Self {
        Self {
            mode: FormatterMode::Native,
            line_width: config.line_width,
            indent_width: config.indent_width,
            use_tabs: config.use_tabs,
            final_newline: config.final_newline,
            trailing_comma: config.trailing_comma,
            brace_placement: config.brace_placement,
            else_placement: config.else_placement,
            keyword_spacing: config.keyword_spacing,
        }
    }
}

impl FormatConfig {
    /// Build the former external selection. Native calls explicitly refuse it.
    #[must_use]
    pub fn external_legacy() -> Self {
        Self { mode: FormatterMode::ExternalLegacy, ..Self::default() }
    }

    /// Style-only projection after selection admission, or for the legacy IR.
    pub(super) fn native_policy(&self) -> NativeFormatConfig {
        NativeFormatConfig {
            line_width: self.line_width,
            indent_width: self.indent_width,
            use_tabs: self.use_tabs,
            final_newline: self.final_newline,
            trailing_comma: self.trailing_comma,
            brace_placement: self.brace_placement,
            else_placement: self.else_placement,
            keyword_spacing: self.keyword_spacing,
        }
    }
}
