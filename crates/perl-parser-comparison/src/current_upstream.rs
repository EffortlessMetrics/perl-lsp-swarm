//! Exact-pinned current-upstream Tree-sitter Perl comparison subject.
//!
//! This module exposes one exact subject manifest and a thin factual adapter.
//! It does not assign generic execution or correctness outcomes. The consumer
//! routing train maps these subject-local facts into the shared comparison
//! model after both authorities are current.

use std::error::Error;
use std::fmt;
use std::ops::Range;

use tree_sitter::{InputEdit, Language, Parser, Query, Tree};

const MAX_ROOT_SEXP_BYTES: usize = 4_096;
const MAX_ADAPTER_DIAGNOSTIC_BYTES: usize = 1_024;

/// Checked-in machine-readable projection of [`CURRENT_UPSTREAM_SUBJECT`].
pub const SUBJECT_MANIFEST_TOML: &str = include_str!("../upstream/ts-parser-perl-1.2.1.toml");

/// One canonical exact manifest for the maintained-current upstream subject.
pub const CURRENT_UPSTREAM_SUBJECT: CurrentUpstreamSubjectManifest =
    CurrentUpstreamSubjectManifest {
        schema_version: "parser-comparison-subject.v1",
        subject_role: "current_upstream_tree_sitter",
        package_name: "ts-parser-perl",
        package_version: "1.2.1",
        package_requirement: "=1.2.1",
        package_checksum: "d125f7bfdd1fd82a7e87d2e85793f486ad1b5f465144e9e22132dbe5bd80e694",
        upstream_repository: "https://github.com/tree-sitter-perl/tree-sitter-perl",
        upstream_tag: "v1.2.1",
        upstream_commit: "c3e17b31179bf8f658c9f37c7a3ea6a202212d5a",
        tree_sitter_runtime_version: "0.26.12",
        tree_sitter_language_version: "0.1.7",
        upstream_rust_version: "1.77",
        semantic_digest: "sha256:750bf42fd1190088c649e5c0ab50995b8895a8002ac15d6bbe560721a97134b2",
        reviewed_on: "2026-08-17",
        refresh_owner: "#7255",
        claim_boundary: "exact current-upstream comparison subject; no consumer migration or superiority claim",
    };

/// Exact immutable identity of the maintained-current upstream subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentUpstreamSubjectManifest {
    schema_version: &'static str,
    subject_role: &'static str,
    package_name: &'static str,
    package_version: &'static str,
    package_requirement: &'static str,
    package_checksum: &'static str,
    upstream_repository: &'static str,
    upstream_tag: &'static str,
    upstream_commit: &'static str,
    tree_sitter_runtime_version: &'static str,
    tree_sitter_language_version: &'static str,
    upstream_rust_version: &'static str,
    semantic_digest: &'static str,
    reviewed_on: &'static str,
    refresh_owner: &'static str,
    claim_boundary: &'static str,
}

impl CurrentUpstreamSubjectManifest {
    /// Subject-manifest schema identity.
    pub const fn schema_version(&self) -> &'static str {
        self.schema_version
    }

    /// Stable subject role, distinct from historical and native subjects.
    pub const fn subject_role(&self) -> &'static str {
        self.subject_role
    }

    /// Exact Cargo package name.
    pub const fn package_name(&self) -> &'static str {
        self.package_name
    }

    /// Exact Cargo package version.
    pub const fn package_version(&self) -> &'static str {
        self.package_version
    }

    /// Exact Cargo package requirement.
    pub const fn package_requirement(&self) -> &'static str {
        self.package_requirement
    }

    /// Exact crates.io package checksum.
    pub const fn package_checksum(&self) -> &'static str {
        self.package_checksum
    }

    /// Canonical upstream repository.
    pub const fn upstream_repository(&self) -> &'static str {
        self.upstream_repository
    }

    /// Exact upstream release tag.
    pub const fn upstream_tag(&self) -> &'static str {
        self.upstream_tag
    }

    /// Exact commit referenced by the release tag.
    pub const fn upstream_commit(&self) -> &'static str {
        self.upstream_commit
    }

    /// Exact Tree-sitter runtime resolved by the checked lockfile.
    pub const fn tree_sitter_runtime_version(&self) -> &'static str {
        self.tree_sitter_runtime_version
    }

    /// Exact `tree-sitter-language` bridge resolved by the checked lockfile.
    pub const fn tree_sitter_language_version(&self) -> &'static str {
        self.tree_sitter_language_version
    }

    /// Minimum Rust version declared by the upstream package.
    pub const fn upstream_rust_version(&self) -> &'static str {
        self.upstream_rust_version
    }

    /// Reviewed semantic digest of [`Self::canonical_semantic_json`].
    pub const fn semantic_digest(&self) -> &'static str {
        self.semantic_digest
    }

    /// Date on which this exact subject manifest was reviewed.
    pub const fn reviewed_on(&self) -> &'static str {
        self.reviewed_on
    }

    /// Issue that owns intentional pin refresh.
    pub const fn refresh_owner(&self) -> &'static str {
        self.refresh_owner
    }

    /// Claim boundary attached to this subject manifest.
    pub const fn claim_boundary(&self) -> &'static str {
        self.claim_boundary
    }

    /// Compact exact identity suitable for references in later evidence payloads.
    pub fn semantic_identity(&self) -> String {
        format!("{}:{}", self.subject_role, self.semantic_digest)
    }

    /// Deterministic semantic JSON whose SHA-256 is [`Self::semantic_digest`].
    pub fn canonical_semantic_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"schema_version\":\"{}\",",
                "\"subject_role\":\"{}\",",
                "\"package_name\":\"{}\",",
                "\"package_version\":\"{}\",",
                "\"package_requirement\":\"{}\",",
                "\"package_checksum\":\"{}\",",
                "\"upstream_repository\":\"{}\",",
                "\"upstream_tag\":\"{}\",",
                "\"upstream_commit\":\"{}\",",
                "\"tree_sitter_runtime_version\":\"{}\",",
                "\"tree_sitter_language_version\":\"{}\",",
                "\"upstream_rust_version\":\"{}\"",
                "}}"
            ),
            self.schema_version,
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

    /// Render the checked-in TOML projection from the canonical typed manifest.
    pub fn render_toml(&self) -> String {
        format!(
            concat!(
                "# Generated projection of CURRENT_UPSTREAM_SUBJECT; do not edit independently.\n",
                "schema_version = \"{}\"\n",
                "subject_role = \"{}\"\n",
                "package_name = \"{}\"\n",
                "package_version = \"{}\"\n",
                "package_requirement = \"{}\"\n",
                "package_checksum = \"{}\"\n",
                "upstream_repository = \"{}\"\n",
                "upstream_tag = \"{}\"\n",
                "upstream_commit = \"{}\"\n",
                "tree_sitter_runtime_version = \"{}\"\n",
                "tree_sitter_language_version = \"{}\"\n",
                "upstream_rust_version = \"{}\"\n",
                "semantic_digest = \"{}\"\n",
                "reviewed_on = \"{}\"\n",
                "refresh_owner = \"{}\"\n",
                "claim_boundary = \"{}\"\n"
            ),
            self.schema_version,
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
            self.semantic_digest,
            self.reviewed_on,
            self.refresh_owner,
            self.claim_boundary,
        )
    }
}

/// Factual lifecycle used to produce one current-upstream tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurrentUpstreamParseMode {
    /// Parsed source without an old tree.
    Fresh,
    /// Parsed source with a caller-supplied old tree.
    ReusedOldTree,
    /// Applied an exact [`InputEdit`] to a cloned old tree before reparsing.
    EditedOldTree,
}

/// Exact pinned query asset compiled by the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurrentUpstreamQueryKind {
    /// Upstream highlight query.
    Highlights,
    /// Upstream injection query.
    Injections,
}

impl fmt::Display for CurrentUpstreamQueryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Highlights => formatter.write_str("highlights"),
            Self::Injections => formatter.write_str("injections"),
        }
    }
}

/// Bounded factual adapter text with explicit omission accounting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedSubjectText {
    text: String,
    original_bytes: usize,
    omitted_bytes: usize,
}

impl BoundedSubjectText {
    fn new(value: String, maximum_bytes: usize) -> Self {
        let original_bytes = value.len();
        let mut retained_bytes = original_bytes.min(maximum_bytes);
        while !value.is_char_boundary(retained_bytes) {
            retained_bytes -= 1;
        }
        Self {
            text: value[..retained_bytes].to_owned(),
            original_bytes,
            omitted_bytes: original_bytes - retained_bytes,
        }
    }

    /// Borrow the retained text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Original UTF-8 byte length before bounding.
    pub const fn original_bytes(&self) -> usize {
        self.original_bytes
    }

    /// Number of UTF-8 bytes omitted from the retained text.
    pub const fn omitted_bytes(&self) -> usize {
        self.omitted_bytes
    }

    /// Whether the adapter text was truncated.
    pub const fn is_truncated(&self) -> bool {
        self.omitted_bytes > 0
    }
}

/// One factual current-upstream parse result bound to its exact subject manifest.
#[derive(Debug)]
#[non_exhaustive]
pub struct CurrentUpstreamParse {
    tree: Tree,
    mode: CurrentUpstreamParseMode,
    source_len: usize,
    subject: &'static CurrentUpstreamSubjectManifest,
}

impl CurrentUpstreamParse {
    /// Parsed Tree-sitter tree.
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Factual lifecycle used to produce this tree.
    pub const fn mode(&self) -> CurrentUpstreamParseMode {
        self.mode
    }

    /// Exact source byte length supplied to Tree-sitter.
    pub const fn source_len(&self) -> usize {
        self.source_len
    }

    /// Exact subject manifest carried by this result.
    pub const fn subject(&self) -> &'static CurrentUpstreamSubjectManifest {
        self.subject
    }

    /// Root node kind reported by the exact subject.
    pub fn root_kind(&self) -> &str {
        self.tree.root_node().kind()
    }

    /// Whether the root contains an `ERROR` node or equivalent recovery marker.
    pub fn root_has_error(&self) -> bool {
        self.tree.root_node().has_error()
    }

    /// Number of named root children reported by the exact subject.
    pub fn root_named_child_count(&self) -> usize {
        self.tree.root_node().named_child_count()
    }

    /// Root byte range reported by the exact subject.
    pub fn root_byte_range(&self) -> Range<usize> {
        self.tree.root_node().byte_range()
    }

    /// Bounded root S-expression retained as factual diagnostic input only.
    pub fn bounded_root_sexp(&self) -> BoundedSubjectText {
        BoundedSubjectText::new(self.tree.root_node().to_sexp(), MAX_ROOT_SEXP_BYTES)
    }
}

/// Thin reusable adapter for the exact-pinned current-upstream grammar.
pub struct CurrentUpstreamAdapter {
    parser: Parser,
    language: Language,
    subject: &'static CurrentUpstreamSubjectManifest,
}

impl fmt::Debug for CurrentUpstreamAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CurrentUpstreamAdapter")
            .field("subject", &self.subject.semantic_identity())
            .finish_non_exhaustive()
    }
}

impl CurrentUpstreamAdapter {
    /// Construct a parser and verify Tree-sitter runtime compatibility.
    pub fn new() -> Result<Self, CurrentUpstreamAdapterError> {
        let language: Language = ts_parser_perl::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).map_err(|error| {
            CurrentUpstreamAdapterError::LanguageSetup(BoundedSubjectText::new(
                error.to_string(),
                MAX_ADAPTER_DIAGNOSTIC_BYTES,
            ))
        })?;
        Ok(Self { parser, language, subject: &CURRENT_UPSTREAM_SUBJECT })
    }

    /// Exact subject manifest used by every parse and query result.
    pub const fn subject(&self) -> &'static CurrentUpstreamSubjectManifest {
        self.subject
    }

    /// Parse UTF-8 source, optionally reusing an old tree.
    pub fn parse_str(
        &mut self,
        source: &str,
        old_tree: Option<&Tree>,
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        self.parse_bytes(source.as_bytes(), old_tree)
    }

    /// Parse raw source bytes, optionally reusing an old tree.
    pub fn parse_bytes(
        &mut self,
        source: &[u8],
        old_tree: Option<&Tree>,
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        let mode = if old_tree.is_some() {
            CurrentUpstreamParseMode::ReusedOldTree
        } else {
            CurrentUpstreamParseMode::Fresh
        };
        self.parse_with_mode(source, old_tree, mode)
    }

    /// Apply an edit to an old tree and parse the exact new source incrementally.
    pub fn parse_edited(
        &mut self,
        old_tree: &Tree,
        edit: &InputEdit,
        new_source: &[u8],
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        let mut edited_tree = old_tree.clone();
        edited_tree.edit(edit);
        self.parse_with_mode(
            new_source,
            Some(&edited_tree),
            CurrentUpstreamParseMode::EditedOldTree,
        )
    }

    /// Compile the exact upstream highlight query against the active language.
    pub fn highlight_query(&self) -> Result<Query, CurrentUpstreamAdapterError> {
        self.compile_query(CurrentUpstreamQueryKind::Highlights, ts_parser_perl::HIGHLIGHTS_QUERY)
    }

    /// Compile the exact upstream injection query against the active language.
    pub fn injection_query(&self) -> Result<Query, CurrentUpstreamAdapterError> {
        self.compile_query(CurrentUpstreamQueryKind::Injections, ts_parser_perl::INJECTIONS_QUERY)
    }

    /// Upstream node-types payload supplied by the exact pinned package.
    pub const fn node_types(&self) -> &'static str {
        ts_parser_perl::NODE_TYPES
    }

    fn parse_with_mode(
        &mut self,
        source: &[u8],
        old_tree: Option<&Tree>,
        mode: CurrentUpstreamParseMode,
    ) -> Result<CurrentUpstreamParse, CurrentUpstreamAdapterError> {
        let tree = self
            .parser
            .parse(source, old_tree)
            .ok_or(CurrentUpstreamAdapterError::ParseReturnedNone)?;
        Ok(CurrentUpstreamParse { tree, mode, source_len: source.len(), subject: self.subject })
    }

    fn compile_query(
        &self,
        kind: CurrentUpstreamQueryKind,
        source: &str,
    ) -> Result<Query, CurrentUpstreamAdapterError> {
        Query::new(&self.language, source).map_err(|error| CurrentUpstreamAdapterError::Query {
            kind,
            message: BoundedSubjectText::new(error.to_string(), MAX_ADAPTER_DIAGNOSTIC_BYTES),
        })
    }
}

/// Typed factual setup, parse, and query failures from the current-upstream adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CurrentUpstreamAdapterError {
    /// Tree-sitter rejected the exact pinned language/runtime combination.
    LanguageSetup(BoundedSubjectText),
    /// Tree-sitter returned no tree, for example after cancellation.
    ParseReturnedNone,
    /// An exact pinned upstream query failed to compile.
    Query {
        /// Query asset that failed.
        kind: CurrentUpstreamQueryKind,
        /// Bounded Tree-sitter query diagnostic.
        message: BoundedSubjectText,
    },
}

impl fmt::Display for CurrentUpstreamAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LanguageSetup(message) => {
                write!(formatter, "current-upstream language setup failed: {}", message.as_str())
            }
            Self::ParseReturnedNone => {
                formatter.write_str("current-upstream parser returned no tree")
            }
            Self::Query { kind, message } => {
                write!(formatter, "current-upstream {kind} query failed: {}", message.as_str())
            }
        }
    }
}

impl Error for CurrentUpstreamAdapterError {}

/// Validate that a Cargo requirement is the reviewed exact package pin.
pub fn validate_exact_package_requirement(
    requirement: &str,
) -> Result<(), CurrentUpstreamPinError> {
    let expected = CURRENT_UPSTREAM_SUBJECT.package_requirement();
    if requirement == expected {
        Ok(())
    } else {
        Err(CurrentUpstreamPinError { expected, actual: requirement.to_owned() })
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
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ts-parser-perl requirement must be exactly '{}', got '{}'",
            self.expected, self.actual
        )
    }
}

impl Error for CurrentUpstreamPinError {}
