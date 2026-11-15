//! Compatibility facade for `perl-tidy` and `perl-tidy-perltidy`.
//!
//! New callers should import the package that owns their selected formatter.
//! Native compatibility DTOs delegate to `perl-tidy`; external configuration
//! and text execution belong to `perl-tidy-perltidy`. File/range/cache helpers
//! remain here only for existing callers.

#![deny(unsafe_code)]
#![allow(deprecated)] // This package intentionally implements the published compatibility API.
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]
#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

use perl_subprocess_runtime::SubprocessRuntime;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

pub mod native;

#[allow(deprecated)]
pub use native::{
    BracePlacement, COUNTER_CLOCK_TAG, COUNTER_SCHEMA_V1, ElsePlacement, FinalNewline,
    FormatConfig, FormatDiagnostic, FormatDiagnosticSeverity, FormatDoc, FormatResult,
    FormatterMode, FormatterSelection, KeywordSpacing, MAX_NATIVE_INDENT_WIDTH,
    MAX_NATIVE_LINE_WIDTH, MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1, NativeConfigError,
    NativeFormatConfig, NativeFormatter, NativePipelineCounters, PerlFormatter,
    PipelineCollectorScope, SCALING_ABSOLUTE_SLACK_V1, SCALING_RATIO_BOUND_V1, TextEdit,
    TextPosition, TextRange, TrailingComma, exceeds_replacement_envelope_v1,
};

/// Configuration owned by the explicit external adapter package.
pub use perl_tidy_perltidy::ExternalPerlTidyConfig;

/// Former configuration name retained as a thin source-compatible alias.
pub type PerlTidyConfig = ExternalPerlTidyConfig;

/// Perltidy formatter.
pub struct PerlTidyFormatter {
    config: PerlTidyConfig,
    cache: HashMap<String, String>,
    runtime: Arc<dyn SubprocessRuntime>,
}

impl PerlTidyFormatter {
    /// Creates a new formatter with the given configuration and runtime.
    #[must_use]
    pub fn new(config: PerlTidyConfig, runtime: Arc<dyn SubprocessRuntime>) -> Self {
        Self { config, cache: HashMap::new(), runtime }
    }

    /// Creates a new formatter with the OS subprocess runtime (non-WASM only).
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn with_os_runtime(config: PerlTidyConfig) -> Self {
        use perl_subprocess_runtime::OsSubprocessRuntime;
        // OsSubprocessRuntime::with_timeout normalizes zero to 1s; clamp here
        // as well so the formatter's floor is explicit at its own seam.
        let timeout = config.timeout_secs.max(1);
        Self::new(config, Arc::new(OsSubprocessRuntime::with_timeout(timeout)))
    }

    /// Format Perl code.
    pub fn format(&mut self, code: &str) -> Result<String, String> {
        if let Some(cached) = self.cache.get(code) {
            return Ok(cached.clone());
        }

        let mut args = self.config.to_args();
        args.push("-st".to_string());
        let formatted = perl_tidy_perltidy::LegacyPerlTidyExecutor::new(self.runtime.as_ref())
            .format("perltidy", &args, code)
            .map_err(|error| error.to_string())?;
        self.cache.insert(code.to_string(), formatted.clone());
        Ok(formatted)
    }

    /// Format a file in place.
    pub fn format_file(&self, file_path: &Path) -> Result<(), String> {
        let mut args = self.config.to_args();
        args.push("--".to_string());
        args.push(file_path.to_string_lossy().into_owned());
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let output =
            self.runtime.run_command("perltidy", &args_refs, None).map_err(|e| e.message)?;

        if !output.success() {
            return Err(format!("Perltidy failed: {}", output.stderr_lossy()));
        }

        Ok(())
    }

    /// Clear any memoized formatting results.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Return the number of memoized formatting results.
    #[must_use]
    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    /// Format a range of code.
    pub fn format_range(
        &mut self,
        code: &str,
        start_line: u32,
        end_line: u32,
    ) -> Result<String, String> {
        if start_line > end_line {
            return Err(
                "Invalid line range: start line must be less than or equal to end line".to_string()
            );
        }

        let lines: Vec<&str> = code.lines().collect();

        if start_line as usize >= lines.len() || end_line as usize >= lines.len() {
            return Err("Line range out of bounds".to_string());
        }

        let range_code = lines[start_line as usize..=end_line as usize].join("\n");
        let formatted_range = self.format(&range_code)?;

        let mut result = Vec::new();
        if start_line > 0 {
            result.extend_from_slice(&lines[0..start_line as usize]);
        }
        result.extend(formatted_range.lines());
        if (end_line as usize) < lines.len() - 1 {
            result.extend_from_slice(&lines[(end_line as usize + 1)..]);
        }

        Ok(result.join("\n"))
    }

    /// Get formatting suggestions without applying them.
    pub fn get_suggestions(&mut self, code: &str) -> Result<Vec<FormatSuggestion>, String> {
        let formatted = self.format(code)?;
        if formatted == code {
            return Ok(Vec::new());
        }

        let orig_lines: Vec<&str> = code.lines().collect();
        let fmt_lines: Vec<&str> = formatted.lines().collect();
        let mut suggestions = Vec::new();
        let max_lines = orig_lines.len().max(fmt_lines.len());

        for i in 0..max_lines {
            match (orig_lines.get(i), fmt_lines.get(i)) {
                (Some(orig), Some(fmt)) if orig != fmt => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: (*orig).to_string(),
                    formatted: (*fmt).to_string(),
                    description: "Line formatting change".to_string(),
                }),
                (Some(orig), None) => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: (*orig).to_string(),
                    formatted: String::new(),
                    description: "Line removed by formatting".to_string(),
                }),
                (None, Some(fmt)) => suggestions.push(FormatSuggestion {
                    line: i as u32,
                    original: String::new(),
                    formatted: (*fmt).to_string(),
                    description: "Line added by formatting".to_string(),
                }),
                _ => {}
            }
        }

        Ok(suggestions)
    }
}

/// A formatting suggestion.
#[derive(Debug, Clone)]
pub struct FormatSuggestion {
    /// Zero-based line number where the change applies.
    pub line: u32,
    /// Original line content before formatting.
    pub original: String,
    /// Suggested formatted line content.
    pub formatted: String,
    /// Human-readable description of the formatting change.
    pub description: String,
}

/// Deprecated compatibility delegate to the canonical Rust-native formatter.
///
/// This type no longer owns a delimiter scanner or an indentation engine.
/// Native parse and preservation refusals return the unchanged source through
/// the former string-only API. Use `perl_tidy::NativeFormatter` for typed
/// outcomes, exact byte plans and explicit native policy.
#[deprecated(
    since = "0.17.0",
    note = "migrate by 0.18.0 (#9292): use perl_tidy::NativeFormatter and NativeFormatConfig"
)]
pub struct BuiltInFormatter {
    config: PerlTidyConfig,
}

#[allow(deprecated)]
impl BuiltInFormatter {
    /// Retain former construction while treating external-only fields as inert.
    #[must_use]
    pub fn new(config: PerlTidyConfig) -> Self {
        Self { config }
    }

    /// Delegate admitted native style to the one native engine.
    ///
    /// This method does not probe Perl::Tidy or select an external fallback.
    #[must_use]
    pub fn format(&self, code: &str) -> String {
        use perl_tidy::{
            BracePlacement, ElsePlacement, KeywordSpacing, NativeFormatConfig, TrailingComma,
        };
        let mut policy = NativeFormatConfig::default();
        if let Some(width) = self.config.maximum_line_length {
            policy.line_width = width;
        }
        if let Some(indent) = self.config.indent_columns {
            policy.indent_width = indent;
        }
        if let Some(tabs) = self.config.tabs {
            policy.use_tabs = tabs;
        }
        if let Some(next_line) = self.config.opening_brace_on_new_line {
            policy.brace_placement =
                if next_line { BracePlacement::NextLine } else { BracePlacement::SameLine };
        }
        if let Some(cuddled) = self.config.cuddled_else {
            policy.else_placement =
                if cuddled { ElsePlacement::Cuddled } else { ElsePlacement::SeparateLine };
        }
        if let Some(space) = self.config.space_after_keyword {
            policy.keyword_spacing =
                if space { KeywordSpacing::Space } else { KeywordSpacing::Compact };
        }
        if let Some(add) = self.config.add_trailing_commas {
            policy.trailing_comma =
                if add { TrailingComma::AddWhenWrapped } else { TrailingComma::Preserve };
        }
        perl_tidy::NativeFormatter::new()
            .format_document_typed(code, &policy, &perl_tidy::FormatContext::default())
            .result
            .formatted
    }
}
