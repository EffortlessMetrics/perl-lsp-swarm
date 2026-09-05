//! Diagnostic codes, severity levels, tags, and categories.
//!
//! This module contains the canonical definitions of all diagnostic codes used
//! throughout the Perl LSP ecosystem. These codes are stable and can be
//! referenced in documentation and error messages.
//!
//! # Code Ranges
//!
//! | Range       | Category                  |
//! |-------------|---------------------------|
//! | PL001-PL099 | Parser diagnostics        |
//! | PL100-PL199 | Strict/warnings           |
//! | PL200-PL299 | Package/module            |
//! | PL300-PL399 | Subroutine                |
//! | PL400-PL499 | Best practices            |
//! | PL500-PL599 | Deprecated syntax         |
//! | PL600-PL699 | Security                  |
//! | PL700-PL799 | Import                    |
//! | PL800-PL899 | Heredoc anti-patterns     |
//! | PL900-PL999 | Version compatibility     |

use std::{fmt, str::FromStr};

mod category;
mod metadata;
mod severity;
mod tag;

pub use category::DiagnosticCategory;
pub use severity::DiagnosticSeverity;
pub use tag::DiagnosticTag;

macro_rules! define_diagnostic_codes {
    (
        $(
            $(#[$attribute:meta])*
            $variant:ident => $code:literal
        ),+ $(,)?
    ) => {
        /// Stable diagnostic codes for Perl LSP.
        ///
        /// Each code has a fixed string representation and associated metadata.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        #[non_exhaustive]
        pub enum DiagnosticCode {
            $(
                $(#[$attribute])*
                $variant,
            )+
        }

        impl DiagnosticCode {
            /// Every registered built-in diagnostic code in stable public-code order.
            ///
            /// Consumers must use this inventory instead of maintaining an
            /// independent all-codes list.
            pub const ALL: &'static [Self] = &[
                $(Self::$variant,)+
            ];

            /// Get the string representation of this code.
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $code,)+
                }
            }

            /// Try to parse a code string into a `DiagnosticCode`.
            ///
            /// Only exact registered identities parse; case, surrounding
            /// whitespace, and Rust variant spellings are rejected.
            pub fn parse_code(code: &str) -> Option<Self> {
                match code {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

define_diagnostic_codes! {
    // Parser diagnostics (PL001-PL099)
    /// General parse error
    #[default]
    ParseError => "PL001",
    /// Syntax error
    SyntaxError => "PL002",
    /// Unexpected end-of-file
    UnexpectedEof => "PL003",

    // Strict/warnings (PL100-PL199)
    /// Missing 'use strict' pragma
    MissingStrict => "PL100",
    /// Missing 'use warnings' pragma
    MissingWarnings => "PL101",
    /// Unused variable
    UnusedVariable => "PL102",
    /// Undefined variable
    UndefinedVariable => "PL103",
    /// Variable shadowing an outer declaration
    VariableShadowing => "PL104",
    /// Variable redeclared in the same scope
    VariableRedeclaration => "PL105",
    /// Duplicate parameter in a subroutine signature
    DuplicateParameter => "PL106",
    /// Subroutine parameter shadows a global variable
    ParameterShadowsGlobal => "PL107",
    /// Subroutine parameter is declared but never used
    UnusedParameter => "PL108",
    /// Bareword used where a quoted string is expected (under strict)
    UnquotedBareword => "PL109",
    /// Variable used before being initialized
    UninitializedVariable => "PL110",
    /// Pragma name appears to be misspelled
    MisspelledPragma => "PL111",
    /// Capture variable ($1, $2, etc.) used without a preceding regex match in scope
    CaptureVarWithoutRegexMatch => "PL112",

    // Package/module (PL200-PL299)
    /// Missing package declaration
    MissingPackageDeclaration => "PL200",
    /// Duplicate package declaration
    DuplicatePackage => "PL201",

    // Subroutine (PL300-PL399)
    /// Duplicate subroutine definition
    DuplicateSubroutine => "PL300",
    /// Missing explicit return statement
    MissingReturn => "PL301",
    /// Invalid character(s) in a subroutine prototype
    ///
    /// Perl only allows `$`, `@`, `%`, `&`, `*`, `\`, `;`, `+`, `_`, and
    /// spaces in old-style prototypes. Any other character triggers Perl's
    /// "Illegal character in prototype" warning.
    InvalidPrototype => "PL302",
    /// Same-file Moo/Moose roles provide conflicting methods
    RoleConflict => "PL303",
    /// Exported subroutine lacks POD documentation
    MissingPodCoverage => "PL304",
    /// Package-qualified call to a sub not defined in the target (in-file) package (#3014)
    UnresolvedQualifiedCall => "PL305",

    // Best practices (PL400-PL499)
    /// Bareword filehandle usage
    BarewordFilehandle => "PL400",
    /// Two-argument open() call
    TwoArgOpen => "PL401",
    /// Implicit return value
    ImplicitReturn => "PL402",
    /// Assignment used where a comparison was likely intended
    AssignmentInCondition => "PL403",
    /// Numeric comparison against a potentially undefined value
    NumericComparisonWithUndef => "PL404",
    /// printf/sprintf format specifier count does not match argument count
    PrintfFormatMismatch => "PL405",
    /// Statement that cannot be reached due to preceding unconditional exit
    UnreachableCode => "PL406",
    /// `$@` / `$EVAL_ERROR` reads that are not paired with a nearby `eval`/`try`
    EvalErrorFlow => "PL407",
    /// Duplicate key in a hash literal or hash reference constructor
    DuplicateHashKey => "PL408",
    /// `goto LABEL` references a label that does not exist in this file
    GotoUndefinedLabel => "PL409",
    /// `next`/`last`/`redo LABEL` references a label that does not exist in this file
    LoopControlUndefinedLabel => "PL410",

    // Pragma pitfalls / deprecated syntax (PL500-PL599)
    /// Use of deprecated defined(@array) / defined(%hash)
    DeprecatedDefined => "PL500",
    /// Use of deprecated $[ array base variable
    DeprecatedArrayBase => "PL501",
    /// `use strict` appears only inside a phase block and does not affect file scope
    PhaseScopedStrictPragma => "PL502",
    /// `use warnings` appears only inside a phase block and does not affect file scope
    PhaseScopedWarningsPragma => "PL503",

    // Security (PL600-PL699)
    /// String eval is a security risk
    SecurityStringEval => "PL600",
    /// Backtick/qx command execution detected
    SecurityBacktickExec => "PL601",
    /// Global assignment to `$SIG{__DIE__}` / `$SIG{__WARN__}`
    SecuritySignalHandler => "PL602",
    /// `system()` call executes shell commands
    SecuritySystemCall => "PL603",
    /// `exec()` call replaces the current process with a shell command
    SecurityExecCall => "PL604",
    /// Pipe-open `open(FH, "|-", ...)` / `open(FH, "-|", ...)` executes shell commands
    SecurityPipeOpen => "PL605",
    /// `readpipe()` function call executes shell commands (equivalent to qx//)
    SecurityReadpipe => "PL606",
    /// Interpolated or concatenated variables form the SQL text passed to a
    /// DBI statement-taking method (`prepare`/`prepare_cached`/`do`) (#5035)
    SecuritySqlInjection => "PL607",
    /// Substitution replacement is evaluated as Perl code by the `e`/`ee`
    /// modifier (`s/pat/repl/e`) (#9818)
    SecuritySubstitutionEval => "PL608",
    /// Regular expression pattern embeds immediate `(?{ ... })` or deferred
    /// `(??{ ... })` executable code in `m//`, `qr//`, a bare regex literal,
    /// or a substitution pattern (#9818)
    SecurityEmbeddedRegexCode => "PL609",

    // Import (PL700-PL799)
    /// Module appears to be unused
    UnusedImport => "PL700",
    /// Module not found in workspace or configured include paths
    ModuleNotFound => "PL701",
    /// Module is a known source filter (rewrites source before parsing)
    SourceFilterModule => "PL702",

    // Heredoc anti-patterns (PL800-PL899)
    /// Heredoc used inside a format block
    HeredocInFormat => "PL800",
    /// Heredoc used inside a BEGIN block
    HeredocInBegin => "PL801",
    /// Heredoc delimiter is dynamic (variable interpolation)
    HeredocDynamicDelimiter => "PL802",
    /// Heredoc used inside a source filter
    HeredocInSourceFilter => "PL803",
    /// Heredoc used inside a regex code block
    HeredocInRegexCode => "PL804",
    /// Heredoc used inside string eval
    HeredocInEval => "PL805",
    /// Heredoc used with a tied filehandle
    HeredocTiedHandle => "PL806",

    // Version compatibility (PL900-PL999)
    /// Use of a Perl feature not available in the declared version
    VersionIncompatFeature => "PL900",
}

/// Error returned when text is not a registered built-in diagnostic code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseDiagnosticCodeError;

impl fmt::Display for ParseDiagnosticCodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unregistered diagnostic code")
    }
}

impl std::error::Error for ParseDiagnosticCodeError {}

impl FromStr for DiagnosticCode {
    type Err = ParseDiagnosticCodeError;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::parse_code(code).ok_or(ParseDiagnosticCodeError)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for DiagnosticCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for DiagnosticCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let code = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::parse_code(&code)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown diagnostic code `{code}`")))
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
