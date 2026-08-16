//! Exact-pinned current-upstream Tree-sitter Perl comparison subject.
//!
//! This adapter introduces the current published upstream grammar as a subject
//! distinct from the repository's historical vendored C snapshot and the
//! native parser's Tree-sitter-style facade. It does not migrate any existing
//! comparison consumer.

use std::error::Error;
use std::fmt;

use tree_sitter::{InputEdit, Language, Parser, Query, Tree};

/// Exact Cargo package name of the current-upstream subject.
pub const PACKAGE_NAME: &str = "ts-parser-perl";
/// Exact pinned Cargo package version.
pub const PACKAGE_VERSION: &str = "1.2.1";
/// Exact Cargo requirement used by this crate.
pub const PACKAGE_REQUIREMENT: &str = "=1.2.1";
/// crates.io checksum for `ts-parser-perl` 1.2.1.
pub const PACKAGE_CHECKSUM: &str =
    "d125f7bfdd1fd82a7e87d2e85793f486ad1b5f465144e9e22132dbe5bd80e694";
/// Canonical upstream repository.
pub const UPSTREAM_REPOSITORY: &str = "https://github.com/tree-sitter-perl/tree-sitter-perl";
/// Exact upstream release tag.
pub const UPSTREAM_TAG: &str = "v1.2.1";
/// Exact commit referenced by the release tag.
pub const UPSTREAM_COMMIT: &str = "c3e17b31179bf8f658c9f37c7a3ea6a202212d5a";
/// Exact Tree-sitter runtime resolved by the checked lockfile.
pub const TREE_SITTER_RUNTIME_VERSION: &str = "0.26.11";
/// Exact `tree-sitter-language` bridge resolved by the checked lockfile.
pub const TREE_SITTER_LANGUAGE_VERSION: &str = "0.1.7";
/// Minimum Rust version declared by the upstream package.
pub const UPSTREAM_RUST_VERSION: &str = "1.77";
/// Reviewed immutable subject metadata checked into the repository.
pub const SUBJECT_IDENTITY_TOML: &str = include_str!("../upstream/ts-parser-perl-1.2.1.toml");

/// Exact identity of the current-upstream comparison subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CurrentUpstreamSubjectIdentity {
    /// Stable subject role, distinct from historical and native subjects.
    pub subject_role: &'static str,
    /// Cargo package name.
    pub package_name: &'static str,
    /// Exact Cargo package version.
    pub package_version: &'static str,
    /// Exact Cargo requirement.
    pub package_requirement: &'static str,
    /// crates.io package checksum.
    pub package_checksum: &'static str,
    /// Canonical upstream repository.
    pub upstream_repository: &'static str,
    /// Exact release tag.
    pub upstream_tag: &'static str,
    /// Exact tagged commit.
    pub upstream_commit: &'static str,
    /// Exact checked Tree-sitter runtime.
    pub tree_sitter_runtime_version: &'static str,
    /// Exact checked Tree-sitter language bridge.
    pub tree_sitter_language_version: &'static str,
    /// Upstream package Rust-version declaration.
    pub upstream_rust_version: &'static str,
}

impl CurrentUpstreamSubjectIdentity {
    /// Return the reviewed exact identity for the active current-upstream subject.
    pub const fn current() -> Self {
        Self {
            subject_role: "current_upstream_tree_sitter",
            package_name: PACKAGE_NAME,
            package_version: PACKAGE_VERSION,
            package_requirement: PACKAGE_REQUIREMENT,
            package_checksum: PACKAGE_CHECKSUM,
            upstream_repository: UPSTREAM_REPOSITORY,
            upstream_tag: UPSTREAM_TAG,
            upstream_commit: UPSTREAM_COMMIT,
            tree_sitter_runtime_version: TREE_SITTER_RUNTIME_VERSION,
            tree_sitter_language_version: TREE_SITTER_LANGUAGE_VERSION,
            upstream_rust_version: UPSTREAM_RUST_VERSION,
        }
    }

    /// Deterministic semantic identity suitable for run and receipt binding.
    pub fn semantic_identity(&self) -> String {
        format!(
            "{}:{}@{}:{}:{}:{}:{}",
            self.subject_role,
            self.package_name,
            self.package_version,
            self.package_checksum,
            self.upstream_commit,
            self.tree_sitter_runtime_version,
            self.tree_sitter_language_version,
        )
    }

    /// Render one deterministic bounded JSON identity receipt.
    pub fn render_json_receipt(&self) -> String {
        format!(
            concat!(
                "{{\n",
                "  \"schema_version\": \"parser-comparison-subject.v1\",\n",
                "  \"subject_role\": \"{}\",\n",
                "  \"package_name\": \"{}\",\n",
                "  \"package_version\": \"{}\",\n",
                "  \"package_requirement\": \"{}\",\n",
                "  \"package_checksum\": \"{}\",\n",
                "  \"upstream_repository\": \"{}\",\n",
                "  \"upstream_tag\": \"{}\",\n",
                "  \"upstream_commit\": \"{}\",\n",
                "  \"tree_sitter_runtime_version\": \"{}\",\n",
                "  \"tree_sitter_language_version\": \"{}\",\n",
                "  \"upstream_rust_version\": \"{}\"\n",
                "}}"
            ),
            self.subject_role,
            self.package_name,
            self.package_version,
            self.package_requirement,
            self.package_checksum,
            self.upstream_repository,
            self.upstream_tag,
            self.upstream_commit,
            self.tree_sitter_runtime_version,
            self.tree_sitter_language_version,
            self.upstream_rust_version,
        )
    }
}

/// Execution disposition reported by the thin current-upstream adapter.
///
/// This is subject execution evidence, not a correctness verdict. The generic
/// comparison model introduced separately maps these values into the shared
/// execution vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurrentUpstreamExecutionDisposition {
    /// Tree returned without an `ERROR` node.
    AcceptedClean,
    /// Tree returned with recovery or `ERROR` nodes.
    AcceptedRecovered,
}

/// One current-upstream parse result bound to its exact subject identity.
#[derive(Debug)]
#[non_exhaustive]
pub struct CurrentUpstreamParse {
    tree: Tree,
    disposition: CurrentUpstreamExecutionDisposition,
    subject: CurrentUpstreamSubjectIdentity,
}

impl CurrentUpstreamParse {
    /// Parsed Tree-sitter tree.
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Subject execution disposition.
    pub const fn disposition(&self) -> CurrentUpstreamExecutionDisposition {
        self.disposition
    }

    /// Exact subject identity carried by this result.
    pub const fn subject(&self) -> CurrentUpstreamSubjectIdentity {
        self.subject
    }

    /// Root S-expression retained as bounded comparison/debug input.
    pub fn root_sexp(&self) -> String {
        self.tree.root_node().to_sexp()
    }
}

/// Thin reusable adapter for the exact-pinned current-upstream grammar.
pub struct CurrentUpstreamAdapter {
    parser: Parser,
    language: Language,
    subject: CurrentUpstreamSubjectIdentity,
}

impl fmt::Debug for CurrentUpstreamAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CurrentUpstreamAdapter")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

impl CurrentUpstreamAdapter {
    /// Construct a parser and verify Tree-sitter runtime compatibility.
    pub fn new() -> Result<Self, CurrentUpstreamAdapterError> {
        let language: Language = ts_parser_perl::LANGUAGE.into();
        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|error| CurrentUpstreamAdapterError::LanguageSetup(error.to_string()))?;
        Ok(Self {
            parser,
            language,
            subject: CurrentUpstreamSubjectIdentity::current(),
        })
    }

    /// Exact subject identity used by every parse and query result.
    pub const fn subject(&self) -> CurrentUpstreamSubjectIdentity {
        self.subject
    }

    /// Parse UTF-8 source, optionally using an edited old tree.
    pub fn parse_str(
        &mut self,
        source: &str,
        old_tree: Option<&Tree>,
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        self.parse_bytes(source.as_bytes(), old_tree)
    }

    /// Parse raw source bytes, optionally using an edited old tree.
    pub fn parse_bytes(
        &mut self,
        source: &[u8],
        old_tree: Option<&Tree>,
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        let tree = self
            .parser
            .parse(source, old_tree)
            .ok_or(CurrentUpstreamAdapterError::ParseReturnedNone)?;
        let disposition = if tree.root_node().has_error() {
            CurrentUpstreamExecutionDisposition::AcceptedRecovered
        } else {
            CurrentUpstreamExecutionDisposition::AcceptedClean
        };
        Ok(CurrentUpstreamParse {
            tree,
            disposition,
            subject: self.subject,
        })
    }

    /// Apply an edit to an old tree and parse the new source incrementally.
    pub fn parse_edited(
        &mut self,
        old_tree: &Tree,
        edit: &InputEdit,
        new_source: &[u8],
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        let mut edited_tree = old_tree.clone();
        edited_tree.edit(edit);
        self.parse_bytes(new_source, Some(&edited_tree))
    }

    /// Compile the exact upstream highlight query against the active language.
    pub fn highlight_query(&self) -> Result<Query, CurrentUpstreamAdapterError> {
        Query::new(&self.language, ts_parser_perl::HIGHLIGHTS_QUERY).map_err(|error| {
            CurrentUpstreamAdapterError::Query {
                kind: "highlights",
                message: error.to_string(),
            }
        })
    }

    /// Compile the exact upstream injection query against the active language.
    pub fn injection_query(&self) -> Result<Query, CurrentUpstreamAdapterError> {
        Query::new(&self.language, ts_parser_perl::INJECTIONS_QUERY).map_err(|error| {
            CurrentUpstreamAdapterError::Query {
                kind: "injections",
                message: error.to_string(),
            }
        })
    }

    /// Upstream node-types payload supplied by the pinned package.
    pub const fn node_types(&self) -> &'static str {
        ts_parser_perl::NODE_TYPES
    }
}

/// Typed setup, parse, and query failures from the current-upstream adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurrentUpstreamAdapterError {
    /// Tree-sitter rejected the pinned language/runtime combination.
    LanguageSetup(String),
    /// Tree-sitter returned no tree, for example after cancellation.
    ParseReturnedNone,
    /// A pinned upstream query failed to compile.
    Query {
        /// Query family.
        kind: &'static str,
        /// Bounded Tree-sitter query diagnostic.
        message: String,
    },
}

impl fmt::Display for CurrentUpstreamAdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LanguageSetup(message) => {
                write!(f, "current-upstream language setup failed: {message}")
            }
            Self::ParseReturnedNone => {
                write!(f, "current-upstream parser returned no tree")
            }
            Self::Query { kind, message } => {
                write!(f, "current-upstream {kind} query failed: {message}")
            }
        }
    }
}

impl Error for CurrentUpstreamAdapterError {}

/// Validate that a Cargo requirement is the reviewed exact package pin.
pub fn validate_exact_package_requirement(
    requirement: &str,
) -> Result<(), CurrentUpstreamPinError> {
    if requirement == PACKAGE_REQUIREMENT {
        Ok(())
    } else {
        Err(CurrentUpstreamPinError {
            expected: PACKAGE_REQUIREMENT,
            actual: requirement.to_owned(),
        })
    }
}

/// Error returned when the current-upstream dependency is not exactly pinned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentUpstreamPinError {
    expected: &'static str,
    actual: String,
}

impl CurrentUpstreamPinError {
    /// Expected exact Cargo requirement.
    pub const fn expected(&self) -> &'static str {
        self.expected
    }

    /// Actual requirement that failed validation.
    pub fn actual(&self) -> &str {
        &self.actual
    }
}

impl fmt::Display for CurrentUpstreamPinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ts-parser-perl requirement must be exactly '{}', got '{}'",
            self.expected, self.actual
        )
    }
}

impl Error for CurrentUpstreamPinError {}
