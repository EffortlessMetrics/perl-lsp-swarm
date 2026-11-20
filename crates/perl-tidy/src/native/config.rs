use serde::{Deserialize, Serialize};

/// Formatter selection made by an application before invoking an engine.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FormatterMode {
    /// Run the Rust-native formatter.
    ///
    /// The retired `compat` / `perltidy-compat` configuration tokens were
    /// removed by #7129 and are rejected since #15624 closed the deprecation
    /// window: `compat` was a bare alias that selected this mode and produced
    /// byte-identical output, so it named no behavior a user could observe or
    /// rely on. A future compatibility profile must arrive as its own
    /// behavior-backed contract with a profile identity, a reviewed mapping
    /// table, and behavior fixtures — it does not inherit authority from the
    /// removed alias.
    #[default]
    Native,
    /// Explicitly use an external legacy formatter adapter.
    ExternalLegacy,
    /// Disable formatting.
    Off,
}

/// Final newline handling policy.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinalNewline {
    /// Preserve the input's final newline state.
    #[default]
    Preserve,
    /// Ensure exactly one final newline when formatting succeeds.
    Insert,
    /// Remove trailing final newlines when formatting succeeds.
    Trim,
}

/// Trailing comma handling for wrapped delimited expressions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrailingComma {
    /// Preserve the native formatter's current behavior and do not add commas.
    #[default]
    Preserve,
    /// Add a trailing comma when a call, list, or hash is rendered across lines.
    AddWhenWrapped,
}

/// Opening brace placement for supported native block layouts.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BracePlacement {
    /// Keep the opening brace on the block header line.
    #[default]
    SameLine,
    /// Place the opening brace on its own line at the block indentation.
    NextLine,
}

/// Placement for supported native `else` and `elsif` block tails.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ElsePlacement {
    /// Keep `else` and `elsif` cuddled to the previous closing brace.
    #[default]
    Cuddled,
    /// Place `else` and `elsif` on a fresh line at the block indentation.
    SeparateLine,
}

/// Spacing between supported control keywords and their condition parentheses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeywordSpacing {
    /// Insert a space between the keyword and condition parentheses.
    #[default]
    Space,
    /// Omit the space between the keyword and condition parentheses.
    Compact,
}

/// Configuration shared by native formatter implementations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFormatConfig {
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

impl Default for NativeFormatConfig {
    fn default() -> Self {
        Self {
            line_width: 100,
            indent_width: 4,
            use_tabs: false,
            final_newline: FinalNewline::Preserve,
            trailing_comma: TrailingComma::Preserve,
            brace_placement: BracePlacement::SameLine,
            else_placement: ElsePlacement::Cuddled,
            keyword_spacing: KeywordSpacing::Space,
        }
    }
}

/// Legacy name for the pure native formatting policy.
pub type FormatConfig = NativeFormatConfig;

/// Explicit application-level formatter selection; never a native style field.
pub type FormatterSelection = FormatterMode;

/// Maximum supported indentation width at the native engine boundary.
pub const MAX_NATIVE_INDENT_WIDTH: u32 = 16;
/// Maximum supported preferred line width at the native engine boundary.
pub const MAX_NATIVE_LINE_WIDTH: u32 = 1_000;

/// A native policy that cannot be executed within the renderer's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeConfigError {
    /// Indentation must be between one and sixteen columns.
    IndentWidth,
    /// Preferred width must be between one and one thousand columns.
    LineWidth,
}

impl NativeFormatConfig {
    /// Validate native numeric policy before parsing or allocating indentation.
    ///
    /// These are the existing document renderer's supported limits. The engine
    /// refuses unsupported policy rather than silently clamping part of a render.
    pub fn validate(&self) -> Result<(), NativeConfigError> {
        if !(1..=MAX_NATIVE_INDENT_WIDTH).contains(&self.indent_width) {
            return Err(NativeConfigError::IndentWidth);
        }
        if !(1..=MAX_NATIVE_LINE_WIDTH).contains(&self.line_width) {
            return Err(NativeConfigError::LineWidth);
        }
        Ok(())
    }
}

impl std::fmt::Display for NativeConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IndentWidth => f.write_str("native indentation must be between 1 and 16 columns"),
            Self::LineWidth => f.write_str("native line width must be between 1 and 1000 columns"),
        }
    }
}
impl std::error::Error for NativeConfigError {}
