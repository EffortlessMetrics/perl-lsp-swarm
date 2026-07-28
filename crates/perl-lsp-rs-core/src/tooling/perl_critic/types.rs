use perl_parser_core::position::{Position, Range};
use serde::{Deserialize, Serialize};

#[cfg(feature = "lsp-compat")]
use lsp_types;

/// Severity levels for Perl::Critic violations.
///
/// # The variant names are threshold names, not severity names
///
/// Perl::Critic scores each violation from **1 (least severe) to 5 (most
/// severe)** -- see [`Perl::Critic::Violation`], which states the severity is
/// "an integer ranging from 1 to 5, where 5 is the 'most' severe".
///
/// The names below (`gentle`, `stern`, `harsh`, `cruel`, `brutal`) come from
/// `perlcritic`'s command-line *threshold* shortcuts, and they run in the
/// opposite direction: they describe how harsh the critic is being, i.e. how
/// far down the severity scale it will report. `--gentle` (5) reports only
/// the most severe violations; `--brutal` (1) reports everything.
///
/// So `Gentle` carries numeric severity **5 and is the most severe bucket**,
/// and `Brutal` carries numeric severity **1 and is the least severe bucket**.
/// This reads backwards at a glance and has been misread before -- do not
/// "correct" the mapping in [`Severity::to_diagnostic_severity`] without
/// re-reading the upstream documentation first.
///
/// [`Perl::Critic::Violation`]: https://metacpan.org/pod/Perl::Critic::Violation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Most severe issues -- perlcritic severity 5 (`--gentle` threshold).
    Gentle = 5,
    /// Severe issues -- perlcritic severity 4 (`--stern` threshold).
    Stern = 4,
    /// Important issues -- perlcritic severity 3 (`--harsh` threshold).
    Harsh = 3,
    /// Less severe issues -- perlcritic severity 2 (`--cruel` threshold).
    Cruel = 2,
    /// Least severe issues -- perlcritic severity 1 (`--brutal` threshold).
    Brutal = 1,
}

impl Severity {
    /// Converts a numeric severity (1-5) to a `Severity` variant.
    ///
    /// Values outside 1-5 default to `Harsh`.
    pub fn from_number(n: u8) -> Self {
        match n {
            1 => Self::Brutal,
            2 => Self::Cruel,
            3 => Self::Harsh,
            4 => Self::Stern,
            5 => Self::Gentle,
            _ => Self::Harsh,
        }
    }

    /// Converts this severity to a `DiagnosticSeverity` for LSP reporting.
    ///
    /// Perl::Critic severity 5 ([`Severity::Gentle`], the most severe bucket)
    /// maps to [`lsp_types::DiagnosticSeverity::ERROR`]; severity 1
    /// ([`Severity::Brutal`], the least severe bucket) maps to
    /// [`lsp_types::DiagnosticSeverity::HINT`]. See the type-level docs for
    /// why the variant names run opposite to the numbers.
    ///
    /// This is the single source of truth for the perlcritic-to-LSP severity
    /// mapping. Call it rather than re-deriving the `match` at a call site.
    #[cfg(feature = "lsp-compat")]
    pub fn to_diagnostic_severity(self) -> lsp_types::DiagnosticSeverity {
        match self {
            Self::Gentle => lsp_types::DiagnosticSeverity::ERROR,
            Self::Stern | Self::Harsh => lsp_types::DiagnosticSeverity::WARNING,
            Self::Cruel => lsp_types::DiagnosticSeverity::INFORMATION,
            Self::Brutal => lsp_types::DiagnosticSeverity::HINT,
        }
    }

    /// Converts this severity to a numeric severity level (for non-LSP contexts).
    #[cfg(not(feature = "lsp-compat"))]
    pub fn to_severity_level(self) -> u8 {
        self as u8
    }
}

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

#[cfg(test)]
#[cfg(feature = "lsp-compat")]
mod severity_direction_tests {
    use super::*;

    /// Perl::Critic scores violations 1 (least severe) to 5 (most severe):
    /// <https://metacpan.org/pod/Perl::Critic::Violation> -- "an integer
    /// ranging from 1 to 5, where 5 is the 'most' severe".
    ///
    /// The `Gentle`/`Brutal` variant names are `perlcritic` *threshold* names
    /// and run in the opposite direction, which makes the mapping look
    /// inverted at a glance. This test pins the numeric direction so that a
    /// well-meaning "fix" of the apparent inversion fails loudly here rather
    /// than silently shipping backwards diagnostics to users.
    #[test]
    fn numeric_severity_five_is_most_severe_and_maps_to_error() {
        assert_eq!(
            Severity::from_number(5).to_diagnostic_severity(),
            lsp_types::DiagnosticSeverity::ERROR,
            "perlcritic severity 5 is the MOST severe and must surface as an LSP Error"
        );
        assert_eq!(
            Severity::from_number(1).to_diagnostic_severity(),
            lsp_types::DiagnosticSeverity::HINT,
            "perlcritic severity 1 is the LEAST severe and must surface as an LSP Hint"
        );
    }

    #[test]
    fn severity_mapping_is_monotonic_in_the_numeric_score() {
        // Higher perlcritic number == worse code == more prominent LSP
        // severity. `DiagnosticSeverity` numbers run the other way (ERROR is
        // 1), so descending perlcritic scores must be non-decreasing here.
        let ordered: Vec<lsp_types::DiagnosticSeverity> =
            (1..=5).rev().map(|n| Severity::from_number(n).to_diagnostic_severity()).collect();

        assert_eq!(
            ordered,
            vec![
                lsp_types::DiagnosticSeverity::ERROR,       // 5
                lsp_types::DiagnosticSeverity::WARNING,     // 4
                lsp_types::DiagnosticSeverity::WARNING,     // 3
                lsp_types::DiagnosticSeverity::INFORMATION, // 2
                lsp_types::DiagnosticSeverity::HINT,        // 1
            ],
            "perlcritic 5..=1 must map to a non-increasing severity ramp"
        );
    }

    #[test]
    fn threshold_variant_names_carry_their_documented_numbers() {
        // `--gentle` is severity 5, `--brutal` is severity 1.
        assert_eq!(Severity::Gentle as u8, 5);
        assert_eq!(Severity::Stern as u8, 4);
        assert_eq!(Severity::Harsh as u8, 3);
        assert_eq!(Severity::Cruel as u8, 2);
        assert_eq!(Severity::Brutal as u8, 1);
    }
}
