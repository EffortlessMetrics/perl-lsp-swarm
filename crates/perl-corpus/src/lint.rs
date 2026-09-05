use crate::meta::{IdSource, Section};
use anyhow::{Result, bail};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Known valid tags (for warnings)
pub const KNOWN_TAGS: &[&str] = &[
    // Core language features
    "regex",
    "regex-code",
    "sets",
    "branch-reset",
    "substitution",
    "transliteration",
    "qw",
    "quote",
    "quote-like",
    "qq",
    "qr",
    "qx",
    "q",
    "heredoc",
    "heredoc-indented",
    "heredoc-backtick",
    // Variables and data types
    "scalar",
    "array",
    "hash",
    "reference",
    "glob",
    "typeglob",
    "my",
    "our",
    "local",
    "state",
    "package",
    "variable",
    "declaration",
    // Control flow
    "if",
    "unless",
    "while",
    "until",
    "for",
    "foreach",
    "loop",
    "given",
    "when",
    "flow",
    "labels",
    "continue",
    "next",
    "last",
    "redo",
    "goto",
    "flipflop",
    "range",
    "ternary",
    // Subroutines and methods
    "sub",
    "subroutine",
    "function",
    "method",
    "attribute",
    "signature",
    "prototype",
    "anonymous",
    "closure",
    "return",
    "wantarray",
    // Built-ins and functions
    "builtin",
    "math",
    "string",
    "list",
    "file",
    "filetest",
    "io",
    "pack",
    "unpack",
    "split",
    "join",
    "tr",
    "sort",
    "map",
    "grep",
    "print",
    "say",
    "printf",
    "sprintf",
    "format",
    // Operators
    "operator",
    "arithmetic",
    "comparison",
    "logical",
    "bitwise",
    "assignment",
    "lvalue",
    "dereference",
    "arrow",
    "smartmatch",
    "binding",
    "range-operator",
    // Pragmas and modules
    "use",
    "require",
    "no",
    "import",
    "pragma",
    "strict",
    "warnings",
    "feature",
    "experimental",
    "version",
    "vstring",
    "constant",
    "bytes",
    "utf8",
    "encoding",
    "charnames",
    "unicode",
    "mro",
    // Object-oriented
    "class",
    "field",
    "bless",
    "isa",
    "can",
    "does",
    "inheritance",
    "overload",
    "constructor",
    "destructor",
    "autoload",
    // Special variables
    "special-var",
    "magic",
    "punctuation-var",
    "english",
    // Modern Perl
    "try",
    "catch",
    "finally",
    "defer",
    "async",
    "await",
    "signatures",
    "postfix",
    "defined-or",
    // I/O and system
    "open",
    "close",
    "pipe",
    "socket",
    "perlio",
    "layers",
    "system",
    "exec",
    "fork",
    "wait",
    "signal",
    "alarm",
    "tie",
    "tied",
    "untie",
    // Miscellaneous
    "pod",
    "comment",
    "annotation",
    "shebang",
    "legacy",
    "do",
    "eval",
    "block",
    "expression",
    "statement",
    "context",
    "list-context",
    "scalar-context",
    "void-context",
    "interpolation",
    "escape",
    "delimiter",
    "end-section",
    "error",
    "diagnostic",
    "source-filter",
    "inline",
    "xs",
    "ffi",
    // Testing and debugging
    "test",
    "debug",
    "assertion",
    "invariant",
    // Specific edge cases
    "edge-case",
    "ambiguous",
    "lexer-sensitive",
    "parser-sensitive",
    "error-recovery",
    "incomplete",
    "invalid",
];

/// Known valid flags
pub const KNOWN_FLAGS: &[&str] = &[
    "lexer-sensitive",
    "parser-sensitive",
    "ambiguous",
    "error-node-expected",
    "expected-error",
    "experimental",
    "version-gated",
    "slow",
    "incomplete",
    "todo",
    "wip",
];

/// Lint configuration
pub struct LintConfig {
    pub max_sections_per_file: usize,
    pub check_unknown_tags: bool,
    pub check_unknown_flags: bool,
    pub require_perl_version: bool,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            max_sections_per_file: 12,
            check_unknown_tags: true,
            check_unknown_flags: true,
            require_perl_version: false,
        }
    }
}

/// Lint result
pub struct LintResult {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl LintResult {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Lint corpus sections
pub fn lint(sections: &[Section]) -> Result<()> {
    lint_with_config(sections, &LintConfig::default())
}

/// Lint with custom configuration
pub fn lint_with_config(sections: &[Section], config: &LintConfig) -> Result<()> {
    let result = check_sections(sections, config);

    // Log warnings
    for warning in &result.warnings {
        tracing::warn!("{}", warning);
    }

    // Log errors
    for error in &result.errors {
        tracing::error!("{}", error);
    }

    if !result.is_ok() {
        bail!("Linting failed with {} errors", result.errors.len());
    }

    Ok(())
}

/// Check sections and return lint results
pub fn check_sections(sections: &[Section], config: &LintConfig) -> LintResult {
    let mut result = LintResult { errors: Vec::new(), warnings: Vec::new() };

    // Regex for valid ID format - pattern is a compile-time constant, so parsing cannot fail
    static ID_RE: std::sync::LazyLock<Option<Regex>> =
        std::sync::LazyLock::new(|| Regex::new(r"^[a-z0-9._-]+$").ok());

    // Track seen IDs for duplicate detection
    let mut seen_ids = BTreeSet::new();

    // Track sections per file
    let mut per_file: BTreeMap<&str, usize> = BTreeMap::new();

    // Convert known tags/flags to sets for fast lookup
    let known_tags: HashSet<&str> = KNOWN_TAGS.iter().copied().collect();
    let known_flags: HashSet<&str> = KNOWN_FLAGS.iter().copied().collect();

    for section in sections {
        // Check ID format
        if section.id.is_empty() {
            result
                .errors
                .push(format!("Missing effective ID in {}: {}", section.file, section.title));
        } else if section.id_source == IdSource::Generated {
            result
                .errors
                .push(format!("Missing explicit @id in {}: {}", section.file, section.title));
        } else if !ID_RE.as_ref().is_some_and(|re| re.is_match(&section.id)) {
            result.errors.push(format!(
                "Invalid @id format '{}' in {}: {} (must match [a-z0-9._-]+)",
                section.id, section.file, section.title
            ));
        }

        // Check for duplicate IDs
        if !section.id.is_empty() && !seen_ids.insert(&section.id) {
            result.errors.push(format!("Duplicate @id '{}' in {}", section.id, section.file));
        }

        // Count sections per file
        *per_file.entry(&section.file).or_default() += 1;

        // Check unknown tags
        if config.check_unknown_tags {
            for tag in &section.tags {
                if !known_tags.contains(tag.as_str()) {
                    result
                        .warnings
                        .push(format!("Unknown tag '{}' in {}: {}", tag, section.file, section.id));
                }
            }
        }

        // Check unknown flags
        if config.check_unknown_flags {
            for flag in &section.flags {
                if !known_flags.contains(flag.as_str()) {
                    result.warnings.push(format!(
                        "Unknown flag '{}' in {}: {}",
                        flag, section.file, section.id
                    ));
                }
            }
        }

        // Check perl version if required
        if config.require_perl_version && section.perl.is_none() {
            result
                .warnings
                .push(format!("Missing @perl version in {}: {}", section.file, section.id));
        }

        // Check for empty body
        if section.body.trim().is_empty() {
            result.warnings.push(format!("Empty body in {}: {}", section.file, section.id));
        }
    }

    // Check sections per file limit
    for (file, count) in per_file {
        if count > config.max_sections_per_file {
            result.warnings.push(format!(
                "File {} has {} sections (exceeds limit of {})",
                file, count, config.max_sections_per_file
            ));
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::{KNOWN_FLAGS, KNOWN_TAGS};
    use anyhow::{Result, bail, ensure};
    use std::collections::BTreeSet;

    /// Canonical shape for a temporary vocabulary entry.
    ///
    /// `metadata::parser::parse_sections` splits declared `@tags`/`@flags` on
    /// whitespace, so an entry carrying whitespace, uppercase, non-ASCII, or
    /// punctuation can never match a parsed token: it is dead vocabulary that
    /// silently widens the accepted set without ever accepting anything. A
    /// leading or trailing hyphen is rejected for the same reason -- no corpus
    /// file declares one, and `-` alone is not a name.
    fn is_canonical_entry(value: &str) -> bool {
        let bytes = value.as_bytes();
        let alphanumeric = |byte: &u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();

        bytes.iter().all(|byte| alphanumeric(byte) || *byte == b'-')
            && bytes.first().is_some_and(alphanumeric)
            && bytes.last().is_some_and(alphanumeric)
    }

    fn validate_temporary_vocabulary(kind: &str, values: &[&str]) -> Result<()> {
        let mut seen = BTreeSet::new();

        for value in values {
            ensure!(!value.is_empty(), "{kind} vocabulary entry {value:?} must not be empty");
            ensure!(
                is_canonical_entry(value),
                "{kind} vocabulary entry {value:?} must be lowercase ASCII \
                 letters, digits, and interior hyphens"
            );
            ensure!(seen.insert(*value), "{kind} vocabulary contains duplicate entry {value:?}");
        }

        Ok(())
    }

    fn rejected_message(kind: &str, values: &[&str]) -> Result<String> {
        match validate_temporary_vocabulary(kind, values) {
            Ok(()) => bail!("temporary {kind} vocabulary unexpectedly accepted {values:?}"),
            Err(error) => Ok(error.to_string()),
        }
    }

    #[test]
    fn temporary_vocabularies_are_unique_and_canonical() -> Result<()> {
        validate_temporary_vocabulary("tag", KNOWN_TAGS)?;
        validate_temporary_vocabulary("flag", KNOWN_FLAGS)?;

        for retained_tag in ["method", "pragma"] {
            ensure!(
                KNOWN_TAGS.contains(&retained_tag),
                "duplicate cleanup removed canonical tag {retained_tag:?}"
            );
        }

        Ok(())
    }

    #[test]
    fn temporary_vocabulary_validator_rejects_false_green_entries() -> Result<()> {
        let cases: &[(&[&str], &str, &str)] = &[
            (&["regex", "regex"], "duplicate", "regex"),
            (&[""], "must not be empty", "\"\""),
            (&["Regex"], "lowercase ASCII", "Regex"),
            (&[" regex"], "lowercase ASCII", " regex"),
            (&["regex "], "lowercase ASCII", "regex "),
            // Interior whitespace survives a trim-only guard, but no parsed
            // token can ever contain it.
            (&["re gex"], "lowercase ASCII", "re gex"),
            (&["regex!"], "lowercase ASCII", "regex!"),
            (&["r\u{e9}gex"], "lowercase ASCII", "r\u{e9}gex"),
            (&["-regex"], "lowercase ASCII", "-regex"),
            (&["regex-"], "lowercase ASCII", "regex-"),
        ];

        // Both live kinds are driven so that a validator hardcoding one kind
        // label instead of threading `kind` cannot pass.
        for kind in ["tag", "flag"] {
            for &(values, reason, token) in cases {
                let message = rejected_message(kind, values)?;
                ensure!(
                    message.contains(&format!("{kind} vocabulary")),
                    "validator failure lost vocabulary kind {kind:?}: {message}"
                );
                ensure!(
                    message.contains(reason),
                    "validator failure did not explain {reason:?}: {message}"
                );
                ensure!(
                    message.contains(token),
                    "validator failure lost offending token {token:?}: {message}"
                );
            }
        }

        Ok(())
    }
}
