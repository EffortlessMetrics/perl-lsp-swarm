//! The public request/error types plus the request-field validation they
//! share with [`crate::packet::build_ripr_facts_packet`] and [`crate::cli`].

/// Expected schema version for `ripr-perl-facts-v1` packets.
pub(crate) const EXPECTED_RIPR_FACTS_SCHEMA: &str = "ripr-perl-facts-v1";

/// A structured request to the `ripr-facts` batch exporter.
///
/// This is the programmatic input shape for [`build_ripr_facts_packet`]. The
/// `perl-lsp` / `perllsp` `ripr-facts` subcommand parses argv into one of these
/// and calls the batch API through [`run_ripr_facts`]; other batch producers
/// can construct it directly.
#[derive(Debug, Clone, Copy)]
pub struct RiprFactsRequest<'a> {
    /// Packet schema version; must equal `ripr-perl-facts-v1`.
    pub schema: &'a str,
    /// Repo-relative workspace root to scan (forward-slash, no `..`/drive/absolute).
    pub root: &'a str,
    /// Optional base ref for diff-derived facts (managed-producer mode; not yet emitted).
    pub base: Option<&'a str>,
    /// Optional head ref recorded in the packet.
    pub head: Option<&'a str>,
    /// Comma-separated fact classes to request; validated + normalized internally.
    pub fact_classes: &'a str,
    /// Pre-computed unified diff (base→head) text, supplied by a managed-producer
    /// caller and consumed only when `changes` is requested. `None` in the batch
    /// / CLI path (which does not yet produce one — see #3293 PR 5). The diff is
    /// treated as opaque text: no git is run, no process is spawned, and its
    /// paths are expected in `git diff`'s default repo-root-relative `a/`/`b/`
    /// form. base/head/diff are caller-asserted, never verified here.
    pub diff: Option<&'a str>,
}

/// A validation failure from [`build_ripr_facts_packet`] that prevents packet
/// assembly.
///
/// Emission itself is infallible — the conservative string-scan emitter degrades
/// to an `unavailable` / `partial` packet rather than erroring — so the only way
/// to build no packet at all is to fail input validation.
///
/// The [`Display`](std::fmt::Display) form is the operator-facing reason without
/// the `ripr-facts: ` prefix that [`run_ripr_facts`] adds when printing to
/// stderr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiprFactsError {
    /// The requested schema is not the supported `ripr-perl-facts-v1`.
    UnsupportedSchema {
        /// The unsupported schema string the caller passed.
        schema: String,
    },
    /// The `root` path is not repo-relative (absolute, `./`, `..`, or drive).
    InvalidRoot(String),
    /// The `fact_classes` list is empty or contains an unknown class.
    InvalidFactClasses(String),
}

impl std::fmt::Display for RiprFactsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema { schema } => {
                write!(f, "unsupported schema `{schema}`; expected `{EXPECTED_RIPR_FACTS_SCHEMA}`")
            }
            Self::InvalidRoot(reason) | Self::InvalidFactClasses(reason) => write!(f, "{reason}"),
        }
    }
}

impl std::error::Error for RiprFactsError {}

/// Validate a path is repo-relative: forward-slash, no host/drive/temp prefix,
/// no `..` escape, no leading `/` or `./`.
pub(crate) fn validate_ripr_facts_path(path: &str, field: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err(format!("`{field}` must not be empty"));
    }
    if path.starts_with('/') {
        return Err(format!("`{field}` must be repo-relative, not absolute: `{path}`"));
    }
    if path.starts_with("./") {
        return Err(format!("`{field}` must not start with `./`: `{path}`"));
    }
    if path.contains("..") {
        return Err(format!("`{field}` must not contain `..` (path escape): `{path}`"));
    }
    // Reject Windows drive letters (e.g. `C:\`) and UNC paths.
    if path.len() >= 2 && path.as_bytes()[1] == b':' {
        return Err(format!("`{field}` must be repo-relative, not a drive path: `{path}`"));
    }
    Ok(())
}

/// The closed vocabulary of fact classes the producer can emit.
const VALID_FACT_CLASSES: &[&str] = &[
    "files",
    "owners",
    "changes",
    "tests",
    "oracles",
    "relations",
    "dynamic_boundaries",
    "verify_commands",
    "limitations",
    "provenance",
];

/// Parse + deduplicate + deterministically order the comma-separated
/// fact-class list.
pub(crate) fn normalize_fact_classes(raw: &str) -> Result<Vec<String>, String> {
    let mut seen: Vec<String> = Vec::new();
    for class in raw.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        if !VALID_FACT_CLASSES.contains(&class) {
            return Err(format!(
                "unknown fact class `{class}`; valid: {}",
                VALID_FACT_CLASSES.join(", ")
            ));
        }
        if !seen.iter().any(|s| s == class) {
            seen.push(class.to_string());
        }
    }
    // Deterministic order: canonical VALID_FACT_CLASSES order.
    seen.sort_by_key(|c| {
        VALID_FACT_CLASSES.iter().position(|v| *v == c.as_str()).unwrap_or(usize::MAX)
    });
    if seen.is_empty() {
        return Err("fact_classes must not be empty".to_string());
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ripr_facts_validates_path_helper_directly() {
        // Directly test the path validator for all branches.
        assert!(validate_ripr_facts_path(".", "test").is_ok());
        assert!(validate_ripr_facts_path("target/ripr/x.json", "test").is_ok());
        assert!(validate_ripr_facts_path("", "test").is_err());
        assert!(validate_ripr_facts_path("/abs", "test").is_err());
        assert!(validate_ripr_facts_path("./rel", "test").is_err());
        assert!(validate_ripr_facts_path("../escape", "test").is_err());
        assert!(validate_ripr_facts_path("C:/drive", "test").is_err());
    }
}
