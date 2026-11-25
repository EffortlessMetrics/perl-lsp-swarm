use serde::{Deserialize, Serialize};

/// Explicit external Perl::Tidy style, profile and legacy process inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPerlTidyConfig {
    /// Maximum line length.
    pub maximum_line_length: Option<u32>,
    /// Indent size (spaces).
    ///
    /// `None` means "not configured": the caller decides the indent width.
    /// The LSP formatting path falls back to the editor's `tabSize`, and
    /// [`ExternalPerlTidyConfig::to_args`] emits no `--indent-columns`, leaving
    /// `perltidy`'s own default in force.
    pub indent_columns: Option<u32>,
    /// Use tabs instead of spaces.
    ///
    /// `None` means "not configured", with the same fallback rules as
    /// [`ExternalPerlTidyConfig::indent_columns`]: the LSP formatting path falls back
    /// to the editor's `insertSpaces`, and [`ExternalPerlTidyConfig::to_args`] emits
    /// neither `--tabs` nor `--notabs`.
    pub tabs: Option<bool>,
    /// Opening brace on same line.
    pub opening_brace_on_new_line: Option<bool>,
    /// Cuddled else.
    pub cuddled_else: Option<bool>,
    /// Space after keyword.
    pub space_after_keyword: Option<bool>,
    /// Add trailing commas.
    pub add_trailing_commas: Option<bool>,
    /// Vertical alignment.
    pub vertical_alignment: Option<bool>,
    /// Block comment indentation.
    pub block_comment_indentation: Option<u32>,
    /// Custom perltidyrc file path.
    pub profile: Option<String>,
    /// Additional command line arguments.
    pub extra_args: Vec<String>,
    /// Timeout in seconds for the perltidy subprocess. Default: 10.
    pub timeout_secs: u64,
}

impl Default for ExternalPerlTidyConfig {
    fn default() -> Self {
        Self {
            maximum_line_length: Some(80),
            // Indentation is deliberately unset by default so that an
            // unconfigured project keeps deferring to the editor's `tabSize` /
            // `insertSpaces`, while an explicitly configured value wins. The
            // presets below opt in explicitly.
            indent_columns: None,
            tabs: None,
            opening_brace_on_new_line: Some(false),
            cuddled_else: Some(true),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(false),
            vertical_alignment: Some(true),
            block_comment_indentation: Some(0),
            profile: None,
            extra_args: Vec::new(),
            timeout_secs: 10,
        }
    }
}

impl ExternalPerlTidyConfig {
    /// Create a config for PBP (Perl Best Practices) style.
    #[must_use]
    pub fn pbp() -> Self {
        Self {
            maximum_line_length: Some(78),
            indent_columns: Some(4),
            tabs: Some(false),
            opening_brace_on_new_line: Some(false),
            cuddled_else: Some(false),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(true),
            vertical_alignment: Some(true),
            block_comment_indentation: Some(0),
            profile: None,
            extra_args: vec!["--perl-best-practices".to_string()],
            timeout_secs: 10,
        }
    }

    /// Create a config for GNU style.
    #[must_use]
    pub fn gnu() -> Self {
        Self {
            maximum_line_length: Some(79),
            indent_columns: Some(2),
            tabs: Some(false),
            opening_brace_on_new_line: Some(true),
            cuddled_else: Some(false),
            space_after_keyword: Some(true),
            add_trailing_commas: Some(false),
            vertical_alignment: Some(false),
            block_comment_indentation: Some(2),
            profile: None,
            extra_args: vec!["--gnu-style".to_string()],
            timeout_secs: 10,
        }
    }

    /// Convert the configuration to `perltidy` command-line arguments.
    #[must_use]
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(profile) = &self.profile {
            args.push(format!("--profile={profile}"));
            if let Some(indent) = self.indent_columns {
                args.push(format!("--indent-columns={indent}"));
            }
            if let Some(tabs) = self.tabs {
                args.push(if tabs { "--tabs".to_string() } else { "--notabs".to_string() });
            }
            args.extend(self.extra_args.clone());
            return args;
        }

        if let Some(len) = self.maximum_line_length {
            args.push(format!("--maximum-line-length={len}"));
        }

        if let Some(indent) = self.indent_columns {
            args.push(format!("--indent-columns={indent}"));
        }

        if let Some(tabs) = self.tabs {
            if tabs {
                args.push("--tabs".to_string());
            } else {
                args.push("--notabs".to_string());
            }
        }

        if let Some(brace) = self.opening_brace_on_new_line {
            if brace {
                args.push("--opening-brace-on-new-line".to_string());
            } else {
                args.push("--opening-brace-always-on-right".to_string());
            }
        }

        if let Some(cuddle) = self.cuddled_else {
            if cuddle {
                args.push("--cuddled-else".to_string());
            } else {
                args.push("--nocuddled-else".to_string());
            }
        }

        if let Some(space) = self.space_after_keyword {
            if space {
                args.push("--space-after-keyword".to_string());
            } else {
                args.push("--nospace-after-keyword".to_string());
            }
        }

        if let Some(comma) = self.add_trailing_commas {
            if comma {
                args.push("--add-trailing-commas".to_string());
            } else {
                args.push("--no-add-trailing-commas".to_string());
            }
        }

        if let Some(align) = self.vertical_alignment {
            if align {
                args.push("--vertical-alignment".to_string());
            } else {
                args.push("--no-vertical-alignment".to_string());
            }
        }

        if let Some(indent) = self.block_comment_indentation {
            args.push(format!("--block-comment-indentation={indent}"));
        }

        args.extend(self.extra_args.clone());
        args
    }
}
