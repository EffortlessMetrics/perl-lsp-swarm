use serde::{Deserialize, Serialize};

/// Native formatter operating mode.
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
    /// Add a final newline only when none exists.
    Insert,
    /// Remove trailing final newlines when formatting succeeds.
    Trim,
    /// Remove the terminal newline run, then insert one final newline.
    TrimThenInsert,
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
pub struct FormatConfig {
    /// Formatter engine mode.
    pub mode: FormatterMode,
    /// Preferred line width.
    pub line_width: u32,
    /// Indentation width when spaces are used.
    pub indent_width: u32,
    /// Whether indentation should use tabs instead of spaces.
    pub use_tabs: bool,
    /// Final newline handling.
    pub final_newline: FinalNewline,
    /// Trim spaces and tabs at the end of admitted physical code lines.
    #[serde(default)]
    pub trim_trailing_whitespace: bool,
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
        Self {
            mode: FormatterMode::Native,
            line_width: 100,
            indent_width: 4,
            use_tabs: false,
            final_newline: FinalNewline::Preserve,
            trim_trailing_whitespace: false,
            trailing_comma: TrailingComma::Preserve,
            brace_placement: BracePlacement::SameLine,
            else_placement: ElsePlacement::Cuddled,
            keyword_spacing: KeywordSpacing::Space,
        }
    }
}

impl FormatConfig {
    /// Build an explicit external legacy configuration.
    #[must_use]
    pub fn external_legacy() -> Self {
        Self { mode: FormatterMode::ExternalLegacy, ..Self::default() }
    }
}
