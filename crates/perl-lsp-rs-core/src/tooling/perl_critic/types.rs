use perl_parser_core::position::{Position, Range};
use serde::{Deserialize, Serialize};

pub use super::severity::Severity;

/// A Perl::Critic violation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    /// The policy name that was violated (e.g., "TestingAndDebugging::RequireUseStrict")
    pub policy: String,
    /// A brief description of the violation
    pub description: String,
    /// A detailed explanation of why this policy exists
    pub explanation: String,
    /// The severity level of this violation
    pub severity: Severity,
    /// The source location where the violation occurred
    pub range: Range,
    /// The file path where the violation was found
    pub file: String,
}

/// Configuration for Perl::Critic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriticConfig {
    /// Minimum severity level to report (1-5)
    pub severity: u8,
    /// Path to perlcriticrc file
    pub profile: Option<String>,
    /// Policies to explicitly include in analysis
    pub include: Vec<String>,
    /// Policies to explicitly exclude from analysis
    pub exclude: Vec<String>,
    /// Theme to use
    pub theme: Option<String>,
    /// Enable verbose output
    pub verbose: bool,
    /// Color output
    pub color: bool,
    /// Timeout in seconds for the perlcritic subprocess. Default: 30.
    pub timeout_secs: u64,
    /// Maximum number of file results to keep in the violation cache. Default: 512.
    ///
    /// When the cache is full, the least-recently-used entry is evicted to make
    /// room for the new result. Set to 0 to disable caching entirely.
    pub max_cache_entries: usize,
}

impl Default for CriticConfig {
    fn default() -> Self {
        Self {
            severity: 3,
            profile: None,
            include: Vec::new(),
            exclude: Vec::new(),
            theme: None,
            verbose: false,
            color: false,
            timeout_secs: 30,
            max_cache_entries: 512,
        }
    }
}

#[cfg(not(feature = "lsp-compat"))]
/// Violation summary for non-LSP contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationSummary {
    /// Policy name
    pub policy: String,
    /// Description
    pub description: String,
    /// Severity level (1-5)
    pub severity: u8,
    /// Line number
    pub line: usize,
}

pub(crate) fn insertion_range() -> Range {
    Range {
        start: Position { byte: 0, line: 0, column: 0 },
        end: Position { byte: 0, line: 0, column: 0 },
    }
}
