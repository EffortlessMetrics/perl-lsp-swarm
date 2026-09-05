//! Exact validated token-stream subject for token-fed parsing (#9623).
//!
//! The current token-fed parser boundary accepts a bare token vector plus an
//! independently supplied source string
//! ([`Parser::from_tokens`](crate::Parser::from_tokens)). That pair proves
//! nothing: the tokens need not come from those bytes, the payloads and spans
//! need not agree with the source, the configuration that produced them is
//! unrecorded, and an edited-source stream can carry predecessor tokens under
//! a final-generation label.
//!
//! This module defines the one subject a token-fed parse may be run against:
//! [`ValidatedTokenStream`], which binds an ordered token sequence to the
//! exact source, canonical source identity, lexer/parser configuration
//! identity, terminal EOF disposition, classification authority, and
//! production provenance that together make the stream valid.
//!
//! # Two validation planes
//!
//! Validation is deliberately split, because the two questions have different
//! answers and different callers:
//!
//! 1. **Construction** — is this stream internally coherent? The tokens must
//!    be ordered, in bounds, on UTF-8 boundaries, payload-identical to the
//!    source they span, terminated exactly once, and carried under an admitted
//!    provenance. A subject that fails here never exists.
//! 2. [`ValidatedTokenStream::verify_against`] — is this coherent subject the
//!    one *this consumer* expects? A perfectly coherent stream over source `B`
//!    is still wrong for an operation on source `A`, and identical bytes under
//!    a different logical source are a different subject.
//!
//! Both planes return [`TokenSubjectError`], whose
//! [`reason`](TokenSubjectError::reason) is a stable machine token.
//!
//! # Scope
//!
//! This is the A05a half of controller #8132. It deliberately does **not**
//! change or remove [`Parser::from_tokens`](crate::Parser::from_tokens) and
//! does not migrate any production consumer; that cutover is A05b (#9625).
//! Nothing here implements suffix reuse, checkpoint storage, a lexer replay
//! algorithm, or any parser grammar/recovery change.
//!
//! # Payload contract
//!
//! A token's payload is byte-identical to the source it spans. This is not an
//! assumption made here: [`Token`] already enforces `text.len() == end - start`
//! at construction, and the corpus sweep in
//! `tests/token_payload_source_invariant.rs` re-establishes it on every run
//! over the repository's Perl corpus. If a lexer change ever broke the
//! contract, that test fails rather than this paragraph quietly becoming
//! false. The single documented exception is the payload-free geometry-only
//! `UnknownRest` recovery shape ([`Token::is_geometry_only`]), which is empty
//! text over a non-empty span and is exempted from the payload check rather
//! than special-cased away.
//!
//! Tokens are ordered but **not** contiguous: trivia, POD, and heredoc bodies
//! occupy the gaps between token spans, so ordering is validated as
//! non-overlap (`start >= previous end`), never as adjacency.
//!
//! One forward note for A05b (#9625). `HeredocBody` and its siblings, emitted
//! only under `PerlLexer::with_body_tokens`, share `UnknownRest`'s
//! empty-text-over-non-empty-span shape but are *not* covered by
//! [`Token::is_geometry_only`]. They cannot reach this validator today, because
//! `token_from_lexer_parts` already collapses such a token to a zero-width span
//! before it becomes a parser [`Token`]. A consumer that converts
//! body-token lexer output through some other path would need those kinds
//! considered for the payload exemption.
//!
//! # What structural validation can and cannot prove
//!
//! It proves that every token's payload is the source it spans, that the
//! sequence is ordered and terminated, and that the declared identity matches
//! the bytes. It **cannot** prove that a token sequence *is* the canonical lex
//! of that source: a fabricated stream carrying a wrong token kind, or one that
//! omits semantic tokens (an omission is indistinguishable from the gap trivia
//! legitimately leaves), satisfies every structural rule.
//!
//! So provenance is not left to a caller's word where it need not be.
//! [`ValidatedTokenStream::from_fresh_lex`] takes no token vector and lexes the
//! source itself, which makes `fresh_full_lex` a fact this module establishes.
//! [`ValidatedTokenStream::from_checkpoint_replay`] must accept the producer's
//! tokens — reusing them is the point of a replay — so there its provenance and
//! classification authority are producer attestations this module cannot check.
//! Rather than let an unchecked attestation carry a production claim, a replay
//! subject is validated in full but reports `is_production_valid() == false`.
//! #9623's own rule is that a class may be named by the schema yet not be
//! emittable as a production subject until its owner proves it; the proof here
//! is the checkpoint-bearing value from #8128 / #7294, which A05b (#9625) is
//! the first consumer positioned to supply.
//!
//! Every variant of [`TokenSubjectError`] is covered by a test except
//! [`TokenSubjectError::InstrumentFailure`], which is documented at its
//! definition: the lexer is error-tolerant and no probed input makes it fail,
//! but the arm must exist because `lex_for_subject` returns a `Result` and
//! every other label would misreport a lexer failure as a caller error.
//!
//! One *arm* is deliberately unreachable, as a safety floor rather than a
//! failure class: the payload check slices with `str::get` and maps `None` onto
//! [`TokenSubjectError::InvalidTokenRange`], so a reversed span would be
//! refused instead of panicking. `perl-token`'s checked constructors mean no
//! caller can reach it; that is precisely why it must not be an index
//! expression.

use std::fmt;

use perl_lexer::LexerConfig;
use perl_source_identity::{ContentDigest, ContentRevision, LogicalSourceId, SourceGeneration};

use crate::engine::parser::ParserConfigIdentity;
use crate::tokens::token_stream::{Token, TokenKind};

/// Schema version of the validated token-stream subject.
///
/// A subject built under a different schema is not comparable to one built
/// under this schema and is rejected as [`TokenSubjectError::WrongConfiguration`].
pub const TOKEN_SUBJECT_SCHEMA_VERSION: u32 = 1;

// ── Provenance ────────────────────────────────────────────────────────────────

/// How the token sequence carried by a subject was produced.
///
/// Provenance is never supplied by a caller: each construction seam on
/// [`ValidatedTokenStream`] sets its own variant, so a caller cannot label an
/// arbitrary token vector as a production-admitted stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenStreamProvenance {
    /// One complete fresh lexer pass over the exact source.
    FreshFullLex,
    /// A complete replay to EOF, naming the generation it replayed from.
    ///
    /// The predecessor generation must differ from the subject's own
    /// generation; a replay that claims the same generation it started from is
    /// predecessor tokens relabelled as final-generation tokens.
    CheckpointReplayToEof {
        /// Generation the replayed tokens were originally produced from.
        predecessor_generation: SourceGeneration,
    },
    /// Reserved for exact future-suffix synchronisation.
    ///
    /// Declared so the schema can name the class, but never emittable as a
    /// complete production subject: #6986 remains the exact suffix-sync
    /// admission authority and has not admitted it.
    ExactSuffixSync,
    /// A focused test fixture. Structurally validated, never production-valid.
    TestFixtureUnchecked,
    /// An unsupported or incomplete production class.
    UnsupportedOrIncomplete,
}

impl TokenStreamProvenance {
    /// Whether this provenance may back a production token-fed parse.
    ///
    /// **Only [`TokenStreamProvenance::FreshFullLex`] qualifies today.** It is
    /// the one class this module establishes itself, by performing the lex.
    ///
    /// [`TokenStreamProvenance::CheckpointReplayToEof`] is fully validated —
    /// structure, identity, generations, terminal state — but its provenance is
    /// a producer's word, because a replay must supply its own tokens. #9623's
    /// own rule is that a class may be named by the schema yet "cannot be
    /// emitted as a complete production subject until its owner proves it", and
    /// the checkpoint-bearing value that would prove it comes from #8128 /
    /// #7294 with A05b (#9625) as its first consumer. Admitting replay as
    /// production-valid before then would leave exactly the forgery this type
    /// exists to prevent: caller-supplied tokens carrying a production claim.
    ///
    /// So a replay subject is constructible and validated, and reports
    /// `is_production_valid() == false` until #9625 lands the witness.
    #[must_use]
    pub fn is_production_admissible(&self) -> bool {
        matches!(self, Self::FreshFullLex)
    }

    /// Whether this class asserts a stream complete to EOF.
    ///
    /// Distinct from [`TokenStreamProvenance::is_production_admissible`]: a
    /// replay claims completeness and must satisfy the generation and terminal
    /// rules in full, even though it is not admitted for production. Dropping
    /// its validation along with its admissibility would weaken the type twice
    /// over.
    #[must_use]
    fn claims_complete_stream(&self) -> bool {
        matches!(self, Self::FreshFullLex | Self::CheckpointReplayToEof { .. })
    }

    /// Stable machine label for this provenance class.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::FreshFullLex => "fresh_full_lex",
            Self::CheckpointReplayToEof { .. } => "checkpoint_replay_to_eof",
            Self::ExactSuffixSync => "exact_suffix_sync",
            Self::TestFixtureUnchecked => "test_fixture_unchecked",
            Self::UnsupportedOrIncomplete => "unsupported_or_incomplete",
        }
    }
}

impl fmt::Display for TokenStreamProvenance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ── Classification authority ──────────────────────────────────────────────────

/// How the token *kinds* in a subject were arrived at.
///
/// Perl cannot be lexed without context: whether `/` opens a regex or divides,
/// where a heredoc body ends, and what a bareword means all depend on
/// parser-directed contextual operations (#8128), which a live pass resolves by
/// restoring a real captured boundary checkpoint. A stream whose kinds were
/// resolved that way and one whose kinds were carried over from a predecessor
/// are not equally trustworthy, so a subject records which it is.
///
/// # This describes production, not capability
///
/// It says how the classifications were *obtained*. It is **not** a promise
/// that the subject can serve a contextual operation now: a subject carries
/// tokens and source, so anything a consumer reconstructs from it has a
/// buffered backing, and buffered backings return
/// `ContextualOpResult::FallbackRequired`.
///
/// # `LiveUndirectedLex` is undirected, and the name says so
///
/// A contextual operation is *parser-directed*: the parser calls
/// `TokenStream::apply_contextual` when it reaches an ambiguous position.
/// [`ValidatedTokenStream::from_fresh_lex`] drains the stream without a parser,
/// so no such operation is ever requested. The pass is live — it could have
/// served one — but nothing directed it.
///
/// The practical consequence: a construct whose classification depends on
/// parser direction, such as a format body or a statement-start `/` that the
/// undirected lexer reads as division, may carry the undirected classification
/// rather than the one a real parse would consume. A consumer that needs
/// parser-directed kinds must obtain them through the parser; this type does
/// not claim equivalence with a directed pass, and the variant is named for
/// what actually happened rather than for what would be more reassuring.
///
/// On [`ValidatedTokenStream::from_checkpoint_replay`] the value is a producer
/// attestation this module cannot check; see that constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationAuthority {
    /// Kinds came from a live lexer pass over this subject's own source, with
    /// no parser-directed contextual operation applied. See the type docs.
    LiveUndirectedLex,
    /// Kinds were carried over from a predecessor and not re-resolved.
    ///
    /// Valid for a consumer that never depends on a context-sensitive
    /// classification, and never sufficient for a production subject.
    CarriedFromPredecessor,
}

impl ClassificationAuthority {
    /// Stable machine label for this authority class.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::LiveUndirectedLex => "live_undirected_lex",
            Self::CarriedFromPredecessor => "carried_from_predecessor",
        }
    }
}

// ── Terminal state ────────────────────────────────────────────────────────────

/// Terminal lexer/EOF disposition of a token sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalState {
    /// The stream reached a complete EOF at this byte offset.
    ///
    /// A complete EOF is only complete at the end of the source; any lower
    /// offset is an early terminal claim.
    CompleteEof {
        /// Byte offset of the terminal EOF. Must equal the source length.
        at: usize,
    },
    /// Lexing stopped before the end of source.
    Incomplete {
        /// Byte offset at which the stream stopped.
        stopped_at: usize,
    },
}

impl TerminalState {
    /// Whether this is a complete terminal EOF.
    #[must_use]
    pub fn is_complete(self) -> bool {
        matches!(self, Self::CompleteEof { .. })
    }

    /// Byte offset the stream terminated at, complete or not.
    #[must_use]
    pub fn offset(self) -> usize {
        match self {
            Self::CompleteEof { at } => at,
            Self::Incomplete { stopped_at } => stopped_at,
        }
    }
}

// ── Lexer configuration identity ──────────────────────────────────────────────

/// Comparable identity of the lexer configuration a token stream was produced
/// under.
///
/// [`LexerConfig`] is neither `PartialEq` nor `Hash` (it may carry a local
/// symbol table), so it cannot itself be an identity. This is a projection of
/// exactly the configuration inputs that can change token kind, payload, or
/// segmentation, not a second lexer configuration model: build it from the real
/// [`LexerConfig`] with [`LexerConfigIdentity::of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LexerConfigIdentity {
    parse_interpolation: bool,
    max_lookahead: usize,
    symbol_table_bound: bool,
}

impl LexerConfigIdentity {
    /// Project the identity of a real lexer configuration.
    #[must_use]
    pub fn of(config: &LexerConfig) -> Self {
        Self {
            parse_interpolation: config.parse_interpolation,
            max_lookahead: config.max_lookahead,
            symbol_table_bound: config.symbol_table.is_some(),
        }
    }

    /// Identity of the documented default lexer configuration.
    #[must_use]
    pub fn production_default() -> Self {
        Self::of(&LexerConfig::default())
    }

    /// Whether interpolating bodies were segmented into string parts.
    #[must_use]
    pub fn parse_interpolation(self) -> bool {
        self.parse_interpolation
    }

    /// Shared cursor lookahead bound in effect.
    #[must_use]
    pub fn max_lookahead(self) -> usize {
        self.max_lookahead
    }

    /// Whether a file-local symbol table was bound for bareword/regex
    /// disambiguation.
    ///
    /// Only *whether*, not *which*: `LocalSymbolTable` exposes no content
    /// identity, so two tables with different known-sub sets — which classify
    /// the same `/` differently — project here identically. Rather than let
    /// that silently accept a subject lexed under another table,
    /// [`ValidatedTokenStream::verify_against`] refuses to compare a
    /// configuration with a bound table at all. Giving the table a
    /// deterministic identity belongs to `perl-lexer` (#14819).
    #[must_use]
    pub fn symbol_table_bound(self) -> bool {
        self.symbol_table_bound
    }
}

// ── Subject identity ──────────────────────────────────────────────────────────

/// Exact identity of a token-fed parser subject.
///
/// Every field is a canonical identity reused from its owning authority:
/// [`ContentRevision`] / [`SourceGeneration`] from `perl-source-identity`, and
/// [`ParserConfigIdentity`] from the parse-operation authority. No path string,
/// client document version, local checksum, or process-local value is admitted
/// as identity here.
///
/// Identical bytes under different logical sources are different subjects, and
/// an edit-then-undo returning to the same digest is a new generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSubjectIdentity {
    schema_version: u32,
    content_revision: ContentRevision,
    generation: SourceGeneration,
    lexer_config: LexerConfigIdentity,
    parser_config: ParserConfigIdentity,
}

impl TokenSubjectIdentity {
    /// Bind one exact subject identity at the current schema version.
    #[must_use]
    pub fn new(
        content_revision: ContentRevision,
        generation: SourceGeneration,
        lexer_config: LexerConfigIdentity,
        parser_config: ParserConfigIdentity,
    ) -> Self {
        Self {
            schema_version: TOKEN_SUBJECT_SCHEMA_VERSION,
            content_revision,
            generation,
            lexer_config,
            parser_config,
        }
    }

    /// Bind an identity at an explicit schema version.
    ///
    /// Reserved for decoding a subject recorded under another schema. A version
    /// other than [`TOKEN_SUBJECT_SCHEMA_VERSION`] cannot construct a
    /// [`ValidatedTokenStream`].
    #[must_use]
    pub fn with_schema_version(mut self, schema_version: u32) -> Self {
        self.schema_version = schema_version;
        self
    }

    /// Schema version this identity was recorded under.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Logical source this subject belongs to.
    #[must_use]
    pub fn logical_source(&self) -> &LogicalSourceId {
        &self.content_revision.logical_source_id
    }

    /// Digest of the exact source bytes the tokens were produced from.
    #[must_use]
    pub fn content_digest(&self) -> &ContentDigest {
        &self.content_revision.content_digest
    }

    /// Content revision (logical source plus exact content digest).
    #[must_use]
    pub fn content_revision(&self) -> &ContentRevision {
        &self.content_revision
    }

    /// Freshness label of the source this subject was produced from.
    #[must_use]
    pub fn generation(&self) -> &SourceGeneration {
        &self.generation
    }

    /// Lexer configuration identity in effect.
    #[must_use]
    pub fn lexer_config(&self) -> LexerConfigIdentity {
        self.lexer_config
    }

    /// Parser configuration identity in effect.
    #[must_use]
    pub fn parser_config(&self) -> ParserConfigIdentity {
        self.parser_config
    }

    /// Deterministic digest over every identity field.
    ///
    /// Equal identities always produce an equal fingerprint, and repeated
    /// construction of the same identity reproduces it byte for byte. This
    /// reuses the canonical domain-separated [`ContentDigest`] rather than a
    /// local checksum, and is a comparison aid — it is never accepted in place
    /// of the identity fields themselves.
    #[must_use]
    pub fn fingerprint(&self) -> ContentDigest {
        let parser_config = self.parser_config;
        let budget = parser_config.budget();
        let encoded = format!(
            "parser_token_subject.v{schema}\n\
             logical_source={logical_source}\n\
             content_digest={digest}\n\
             generation={generation}\n\
             lexer.parse_interpolation={parse_interpolation}\n\
             lexer.max_lookahead={max_lookahead}\n\
             lexer.symbol_table_bound={symbol_table_bound}\n\
             parser.max_recursion_depth={max_recursion_depth}\n\
             parser.max_block_nesting_depth={max_block_nesting_depth}\n\
             parser.budget.max_errors={max_errors}\n\
             parser.budget.max_depth={max_depth}\n\
             parser.budget.max_tokens_skipped={max_tokens_skipped}\n\
             parser.budget.max_recoveries={max_recoveries}\n",
            schema = self.schema_version,
            logical_source = self.content_revision.logical_source_id.as_wire(),
            digest = self.content_revision.content_digest.as_wire(),
            generation = self.generation,
            parse_interpolation = self.lexer_config.parse_interpolation,
            max_lookahead = self.lexer_config.max_lookahead,
            symbol_table_bound = self.lexer_config.symbol_table_bound,
            max_recursion_depth = parser_config.max_recursion_depth(),
            max_block_nesting_depth = parser_config.max_block_nesting_depth(),
            max_errors = budget.max_errors,
            max_depth = budget.max_depth,
            max_tokens_skipped = budget.max_tokens_skipped,
            max_recoveries = budget.max_recoveries,
        );
        ContentDigest::of_bytes(encoded.as_bytes())
    }
}

// ── Typed failures ────────────────────────────────────────────────────────────

/// Why a token-fed parser subject is not valid.
///
/// A subject failure is parser input or instrument failure, or a requirement to
/// fall back to a full source pass. It is never ordinary Perl syntax recovery,
/// and it must never be converted into a parse diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenSubjectError {
    /// The tokens do not belong to the source they were paired with, or the
    /// subject belongs to a different logical source than the consumer expects.
    #[error("wrong source: {detail}")]
    WrongSource {
        /// What specifically disagreed.
        detail: String,
    },

    /// The subject's freshness label is absent, stale, or contradicts its
    /// replay provenance.
    #[error("wrong generation: {detail}")]
    WrongGeneration {
        /// What specifically disagreed.
        detail: String,
    },

    /// The subject's schema or lexer/parser configuration identity disagrees
    /// with the operation it is being used for.
    #[error("wrong configuration: {detail}")]
    WrongConfiguration {
        /// What specifically disagreed.
        detail: String,
    },

    /// A token span is out of source bounds, out of order, overlapping, or not
    /// on a UTF-8 character boundary.
    #[error("invalid token range at index {index}: {detail}")]
    InvalidTokenRange {
        /// Position of the offending token in the sequence.
        index: usize,
        /// What specifically was invalid.
        detail: String,
    },

    /// A token's payload is not the source it spans.
    #[error("payload/source mismatch at index {index}: token text is not the source it spans")]
    PayloadSourceMismatch {
        /// Position of the offending token in the sequence.
        index: usize,
        /// Byte offset the token starts at.
        start: usize,
        /// Byte offset the token ends at.
        end: usize,
    },

    /// The terminal EOF disposition is missing, duplicated, early, or does not
    /// agree with the token sequence.
    #[error("invalid terminal state: {detail}")]
    InvalidTerminalState {
        /// What specifically was invalid.
        detail: String,
    },

    /// A production subject does not cover its source to a complete terminal
    /// EOF.
    #[error("incomplete stream: {detail}")]
    IncompleteStream {
        /// What specifically was incomplete.
        detail: String,
    },

    /// A replay subject's token kinds were never resolved by a live pass.
    #[error("missing classification authority: {detail}")]
    MissingClassificationAuthority {
        /// What specifically was missing.
        detail: String,
    },

    /// The instrument that was supposed to produce the subject failed.
    ///
    /// [`ValidatedTokenStream::from_fresh_lex`] runs the lexer itself, so a
    /// lexer error is the producing instrument failing rather than a malformed
    /// input the caller supplied — and this is the only truthful label for it.
    ///
    /// It is the one variant with no test: the lexer is error-tolerant by
    /// design and no probed input (deep nesting, unterminated strings and
    /// heredocs, a million tokens, embedded NUL) makes it return `Err`. The
    /// arm exists because `lex_for_subject` returns a `Result` that must be
    /// handled, and mapping a lexer failure onto any other variant would
    /// misreport it as a caller error.
    #[error("instrument failure: {detail}")]
    InstrumentFailure {
        /// What specifically failed.
        detail: String,
    },

    /// The declared provenance class is not admitted for production parsing.
    #[error("unsupported provenance: {provenance} is not admitted for production parsing")]
    UnsupportedProvenance {
        /// The provenance label that was refused.
        provenance: &'static str,
    },
}

impl TokenSubjectError {
    /// Stable machine reason token for this failure.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::WrongSource { .. } => "wrong_source",
            Self::WrongGeneration { .. } => "wrong_generation",
            Self::WrongConfiguration { .. } => "wrong_configuration",
            Self::InvalidTokenRange { .. } => "invalid_token_range",
            Self::PayloadSourceMismatch { .. } => "payload_source_mismatch",
            Self::InvalidTerminalState { .. } => "invalid_terminal_state",
            Self::IncompleteStream { .. } => "incomplete_stream",
            Self::MissingClassificationAuthority { .. } => "missing_classification_authority",
            Self::InstrumentFailure { .. } => "instrument_failure",
            Self::UnsupportedProvenance { .. } => "unsupported_provenance",
        }
    }

    /// Whether this failure requires a full-source fallback pass rather than
    /// rejecting the operation outright.
    ///
    /// A fallback is a visible, typed re-lex of the whole source. It is never
    /// token, AST, or parser work avoided.
    #[must_use]
    pub fn requires_full_source_fallback(&self) -> bool {
        matches!(
            self,
            Self::IncompleteStream { .. }
                | Self::MissingClassificationAuthority { .. }
                | Self::UnsupportedProvenance { .. }
        )
    }
}

// ── The subject ───────────────────────────────────────────────────────────────

/// One exact validated token-fed parser subject.
///
/// A value of this type is the proof that a token sequence, the source it spans,
/// and the identity it claims all agree. It can only be produced by a canonical
/// construction seam, each of which runs the full validation contract:
///
/// - [`ValidatedTokenStream::from_fresh_lex`] — one complete fresh lexer pass;
/// - [`ValidatedTokenStream::from_checkpoint_replay`] — a complete replay to EOF
///   backed by live boundary checkpoints;
/// - [`ValidatedTokenStream::from_test_fixture`] — a focused fixture, never
///   production-valid;
/// - [`ValidatedTokenStream::from_exact_suffix_sync`] — the reserved class,
///   which always refuses.
///
/// There is no constructor that accepts a caller-chosen provenance, so a bare
/// token vector cannot be labelled as an admitted production stream.
///
/// # Examples
///
/// ```rust
/// use perl_parser_core::tokens::token_subject::{
///     LexerConfigIdentity, TokenSubjectIdentity, ValidatedTokenStream,
/// };
/// use perl_parser_core::ParserConfigIdentity;
/// use perl_source_identity::{
///     ContentDigest, ContentRevision, LogicalSourceId, ProjectId, SourceGeneration,
///     WorkspaceRootId,
/// };
///
/// let source = "my $x = 1;";
///
/// let project = ProjectId::from_canonical_name("https://example.invalid/demo");
/// let root = WorkspaceRootId::from_project_and_root_key(&project, "root");
/// let logical = LogicalSourceId::from_root_and_path(&root, "lib/Demo.pm");
/// let revision = ContentRevision::new(logical, ContentDigest::of_bytes(source.as_bytes()));
///
/// let identity = TokenSubjectIdentity::new(
///     revision,
///     SourceGeneration::known("1"),
///     LexerConfigIdentity::production_default(),
///     ParserConfigIdentity::production_default(),
/// );
///
/// let subject = ValidatedTokenStream::from_fresh_lex(identity, source)?;
///
/// assert!(subject.is_production_valid());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct ValidatedTokenStream<'a> {
    identity: TokenSubjectIdentity,
    source: &'a str,
    tokens: Vec<Token>,
    terminal: TerminalState,
    provenance: TokenStreamProvenance,
    classification_authority: ClassificationAuthority,
}

impl<'a> ValidatedTokenStream<'a> {
    /// Build a subject by performing one complete fresh lexer pass over
    /// `source`.
    ///
    /// This seam takes **no** token vector. Structural validation can prove
    /// that each token's payload matches the source it spans, but it cannot
    /// prove that a caller-supplied sequence *is* the canonical lex: a
    /// fabricated stream with a wrong token kind, or one that silently omits
    /// semantic tokens (an omission is indistinguishable from the gap trivia
    /// legitimately leaves), satisfies every structural rule. So the module
    /// lexes here itself, and `fresh_full_lex` provenance becomes a fact this
    /// type establishes rather than a label a caller applies.
    ///
    /// The lex uses the documented default lexer configuration, so `identity`
    /// must carry [`LexerConfigIdentity::production_default`]; anything else is
    /// [`TokenSubjectError::WrongConfiguration`]. The terminal state is derived,
    /// not accepted, for the same reason.
    ///
    /// A caller holding tokens from somewhere else wants
    /// [`ValidatedTokenStream::from_checkpoint_replay`] (whose provenance is an
    /// explicit producer attestation) or
    /// [`ValidatedTokenStream::from_test_fixture`] (never production-valid).
    ///
    /// # Errors
    ///
    /// [`TokenSubjectError::InstrumentFailure`] if the lexer itself fails, then
    /// the first [`TokenSubjectError`] the validation contract detects.
    pub fn from_fresh_lex(
        identity: TokenSubjectIdentity,
        source: &'a str,
    ) -> Result<Self, TokenSubjectError> {
        if identity.lexer_config != LexerConfigIdentity::production_default() {
            return Err(TokenSubjectError::WrongConfiguration {
                detail: "a fresh lex is performed under the default lexer configuration; this \
                         identity claims another one"
                    .to_owned(),
            });
        }

        let tokens = Self::lex_for_subject(source).map_err(|error| {
            TokenSubjectError::InstrumentFailure { detail: format!("fresh lex failed: {error}") }
        })?;

        Self::assemble(
            identity,
            source,
            tokens,
            TerminalState::CompleteEof { at: source.len() },
            TokenStreamProvenance::FreshFullLex,
            ClassificationAuthority::LiveUndirectedLex,
        )
    }

    /// Build a subject from a complete replay to EOF.
    ///
    /// `predecessor_generation` names the generation the replayed tokens were
    /// originally produced from and must differ from the subject's own
    /// generation. `classification_authority` must be
    /// [`ClassificationAuthority::LiveUndirectedLex`]: a replay whose token
    /// kinds were carried over without being re-resolved is refused outright.
    ///
    /// Unlike [`ValidatedTokenStream::from_fresh_lex`], this seam must accept
    /// the producer's tokens — a replay's whole purpose is to reuse them — so
    /// its provenance and classification authority are **producer
    /// attestations, not facts this module can check**. Structural validation
    /// still applies in full. Making the attestation unforgeable needs the
    /// checkpoint-bearing value from #8128 / #7294, which A05b (#9625) is the
    /// first consumer positioned to supply.
    ///
    /// # Errors
    ///
    /// Returns the first [`TokenSubjectError`] that the validation contract
    /// detects.
    pub fn from_checkpoint_replay(
        identity: TokenSubjectIdentity,
        source: &'a str,
        tokens: Vec<Token>,
        terminal: TerminalState,
        predecessor_generation: SourceGeneration,
        classification_authority: ClassificationAuthority,
    ) -> Result<Self, TokenSubjectError> {
        Self::assemble(
            identity,
            source,
            tokens,
            terminal,
            TokenStreamProvenance::CheckpointReplayToEof { predecessor_generation },
            classification_authority,
        )
    }

    /// Build a focused, explicitly non-production test fixture subject.
    ///
    /// The full structural contract still runs — a fixture may not be
    /// incoherent — but the result is never production-valid, so it cannot be
    /// mistaken for an admitted stream.
    ///
    /// # Errors
    ///
    /// Returns the first [`TokenSubjectError`] that the validation contract
    /// detects.
    pub fn from_test_fixture(
        identity: TokenSubjectIdentity,
        source: &'a str,
        tokens: Vec<Token>,
        terminal: TerminalState,
    ) -> Result<Self, TokenSubjectError> {
        Self::assemble(
            identity,
            source,
            tokens,
            terminal,
            TokenStreamProvenance::TestFixtureUnchecked,
            ClassificationAuthority::CarriedFromPredecessor,
        )
    }

    /// Reserved exact-suffix-sync seam, which always refuses.
    ///
    /// The class is named by the schema so it can be described and tested, but
    /// #6986 is the exact suffix-sync admission authority and has not admitted
    /// it. This seam exists so the refusal is executable rather than a comment,
    /// and so it lands through the same validation plane as every other seam
    /// rather than as a special case beside it.
    ///
    /// # Errors
    ///
    /// Always returns [`TokenSubjectError::UnsupportedProvenance`].
    pub fn from_exact_suffix_sync(
        identity: TokenSubjectIdentity,
        source: &'a str,
        tokens: Vec<Token>,
        terminal: TerminalState,
    ) -> Result<Self, TokenSubjectError> {
        Self::assemble(
            identity,
            source,
            tokens,
            terminal,
            TokenStreamProvenance::ExactSuffixSync,
            ClassificationAuthority::CarriedFromPredecessor,
        )
    }

    /// Seam for a producer that knows its stream is unsupported or incomplete.
    ///
    /// A producer that cannot classify its own output has a typed way to say so
    /// and receive the typed refusal, instead of submitting a stream that would
    /// be judged by a weaker rule. The refusal is the point: this seam never
    /// yields a subject.
    ///
    /// # Errors
    ///
    /// Always returns [`TokenSubjectError::UnsupportedProvenance`].
    pub fn from_unsupported(
        identity: TokenSubjectIdentity,
        source: &'a str,
        tokens: Vec<Token>,
        terminal: TerminalState,
    ) -> Result<Self, TokenSubjectError> {
        Self::assemble(
            identity,
            source,
            tokens,
            terminal,
            TokenStreamProvenance::UnsupportedOrIncomplete,
            ClassificationAuthority::CarriedFromPredecessor,
        )
    }

    /// Lex `source` into the token sequence a subject is built from.
    ///
    /// This is a convenience over the canonical [`TokenStream`] so callers do
    /// not hand-roll a collection loop; it performs no validation of its own.
    ///
    /// [`TokenStream`]: crate::tokens::token_stream::TokenStream
    ///
    /// # Errors
    ///
    /// Propagates the lexer's own parse error.
    pub fn lex_for_subject(source: &str) -> Result<Vec<Token>, crate::error::ParseError> {
        let mut stream = crate::tokens::token_stream::TokenStream::new(source);
        let mut tokens = Vec::new();
        loop {
            let token = stream.next()?;
            let is_eof = token.kind() == TokenKind::Eof;
            tokens.push(token);
            if is_eof {
                return Ok(tokens);
            }
        }
    }

    fn assemble(
        identity: TokenSubjectIdentity,
        source: &'a str,
        tokens: Vec<Token>,
        terminal: TerminalState,
        provenance: TokenStreamProvenance,
        classification_authority: ClassificationAuthority,
    ) -> Result<Self, TokenSubjectError> {
        validate_provenance(&provenance)?;
        validate_schema(&identity)?;
        validate_source_binding(&identity, source)?;
        validate_generation(&identity, &provenance)?;
        validate_classification_authority(&provenance, classification_authority)?;
        validate_tokens(source, &tokens)?;
        validate_terminal(source, &tokens, terminal, &provenance)?;

        Ok(Self { identity, source, tokens, terminal, provenance, classification_authority })
    }

    /// Identity this subject is bound to.
    #[must_use]
    pub fn identity(&self) -> &TokenSubjectIdentity {
        &self.identity
    }

    /// The exact source the tokens span.
    ///
    /// Every source-backed parser operation — heredoc bodies included — must
    /// read through this accessor, so it cannot observe another source.
    #[must_use]
    pub fn source(&self) -> &'a str {
        self.source
    }

    /// The validated ordered token sequence.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Terminal EOF disposition of the sequence.
    #[must_use]
    pub fn terminal(&self) -> TerminalState {
        self.terminal
    }

    /// How this token sequence was produced.
    #[must_use]
    pub fn provenance(&self) -> &TokenStreamProvenance {
        &self.provenance
    }

    /// How this subject's token kinds were arrived at.
    #[must_use]
    pub fn classification_authority(&self) -> ClassificationAuthority {
        self.classification_authority
    }

    /// Whether this subject may back a production token-fed parse.
    #[must_use]
    pub fn is_production_valid(&self) -> bool {
        self.provenance.is_production_admissible()
    }

    /// Deterministic fingerprint of this subject's identity.
    #[must_use]
    pub fn subject_fingerprint(&self) -> ContentDigest {
        self.identity.fingerprint()
    }

    /// Check that this subject is the one the consumer expects.
    ///
    /// Construction proves internal coherence only. A perfectly coherent
    /// subject over another source, another logical source, another generation,
    /// or another configuration is still the wrong subject for this operation,
    /// and this is where that is caught.
    ///
    /// # Errors
    ///
    /// - [`TokenSubjectError::WrongConfiguration`] — schema or lexer/parser
    ///   configuration identity differs;
    /// - [`TokenSubjectError::WrongSource`] — content digest differs;
    /// - [`TokenSubjectError::WrongGeneration`] — logical source or generation
    ///   differs.
    pub fn verify_against(&self, expected: &TokenSubjectIdentity) -> Result<(), TokenSubjectError> {
        if self.identity.schema_version != expected.schema_version {
            return Err(TokenSubjectError::WrongConfiguration {
                detail: format!(
                    "subject schema v{} does not match expected v{}",
                    self.identity.schema_version, expected.schema_version
                ),
            });
        }
        if self.identity.content_revision.content_digest != expected.content_revision.content_digest
        {
            return Err(TokenSubjectError::WrongSource {
                detail: format!(
                    "subject content digest {} does not match expected {}",
                    self.identity.content_revision.content_digest.as_wire(),
                    expected.content_revision.content_digest.as_wire()
                ),
            });
        }
        if self.identity.content_revision.logical_source_id
            != expected.content_revision.logical_source_id
        {
            return Err(TokenSubjectError::WrongSource {
                detail: "identical content under a different logical source is a different subject"
                    .to_owned(),
            });
        }
        if self.identity.generation != expected.generation {
            return Err(TokenSubjectError::WrongGeneration {
                detail: format!(
                    "subject {} does not match expected {}",
                    self.identity.generation, expected.generation
                ),
            });
        }
        // A bound symbol table changes bareword/regex classification, but
        // `LocalSymbolTable` exposes no content identity, so two different
        // tables project to the same `LexerConfigIdentity`. Comparing them
        // would silently accept a subject lexed under a different table, so
        // refuse the comparison instead of getting it wrong. Giving the table a
        // deterministic identity belongs to `perl-lexer`; see #14819.
        if self.identity.lexer_config.symbol_table_bound || expected.lexer_config.symbol_table_bound
        {
            return Err(TokenSubjectError::WrongConfiguration {
                detail: "a bound lexer symbol table has no content identity, so two different \
                         tables are indistinguishable here; this configuration cannot be verified \
                         (see #14819)"
                    .to_owned(),
            });
        }
        if self.identity.lexer_config != expected.lexer_config {
            return Err(TokenSubjectError::WrongConfiguration {
                detail: "subject lexer configuration identity does not match expected".to_owned(),
            });
        }
        if self.identity.parser_config != expected.parser_config {
            return Err(TokenSubjectError::WrongConfiguration {
                detail: "subject parser configuration identity does not match expected".to_owned(),
            });
        }
        Ok(())
    }
}

// ── Validation planes ─────────────────────────────────────────────────────────

fn validate_provenance(provenance: &TokenStreamProvenance) -> Result<(), TokenSubjectError> {
    match provenance {
        TokenStreamProvenance::ExactSuffixSync | TokenStreamProvenance::UnsupportedOrIncomplete => {
            Err(TokenSubjectError::UnsupportedProvenance { provenance: provenance.label() })
        }
        _ => Ok(()),
    }
}

fn validate_schema(identity: &TokenSubjectIdentity) -> Result<(), TokenSubjectError> {
    if identity.schema_version == TOKEN_SUBJECT_SCHEMA_VERSION {
        return Ok(());
    }
    Err(TokenSubjectError::WrongConfiguration {
        detail: format!(
            "unknown subject schema v{} (this build supports v{TOKEN_SUBJECT_SCHEMA_VERSION})",
            identity.schema_version
        ),
    })
}

fn validate_source_binding(
    identity: &TokenSubjectIdentity,
    source: &str,
) -> Result<(), TokenSubjectError> {
    let actual = ContentDigest::of_bytes(source.as_bytes());
    if identity.content_revision.content_digest == actual {
        return Ok(());
    }
    Err(TokenSubjectError::WrongSource {
        detail: format!(
            "identity claims digest {} but the supplied source hashes to {}",
            identity.content_revision.content_digest.as_wire(),
            actual.as_wire()
        ),
    })
}

fn validate_generation(
    identity: &TokenSubjectIdentity,
    provenance: &TokenStreamProvenance,
) -> Result<(), TokenSubjectError> {
    if !provenance.claims_complete_stream() {
        return Ok(());
    }
    if !identity.generation.is_known() {
        return Err(TokenSubjectError::WrongGeneration {
            detail: format!(
                "a {} subject requires a known generation, found {}",
                provenance.label(),
                identity.generation
            ),
        });
    }
    if let TokenStreamProvenance::CheckpointReplayToEof { predecessor_generation } = provenance {
        if !predecessor_generation.is_known() {
            return Err(TokenSubjectError::WrongGeneration {
                detail: "replay provenance requires a known predecessor generation".to_owned(),
            });
        }
        if *predecessor_generation == identity.generation {
            return Err(TokenSubjectError::WrongGeneration {
                detail: format!(
                    "replay predecessor {predecessor_generation} is the subject's own generation: \
                     predecessor tokens cannot be relabelled as final-generation tokens"
                ),
            });
        }
    }
    Ok(())
}

fn validate_classification_authority(
    provenance: &TokenStreamProvenance,
    authority: ClassificationAuthority,
) -> Result<(), TokenSubjectError> {
    if matches!(provenance, TokenStreamProvenance::CheckpointReplayToEof { .. })
        && authority == ClassificationAuthority::CarriedFromPredecessor
    {
        return Err(TokenSubjectError::MissingClassificationAuthority {
            detail: "a replay-to-EOF subject must have had its token kinds resolved by a live \
                     pass; kinds carried over from a predecessor were never re-resolved against \
                     this source"
                .to_owned(),
        });
    }
    Ok(())
}

fn validate_tokens(source: &str, tokens: &[Token]) -> Result<(), TokenSubjectError> {
    let mut previous_end = 0usize;
    let mut seen_eof = false;

    for (index, token) in tokens.iter().enumerate() {
        if seen_eof {
            return Err(TokenSubjectError::InvalidTerminalState {
                detail: format!("token at index {index} follows the terminal EOF token"),
            });
        }

        let (start, end) = (token.start(), token.end());

        if end > source.len() {
            return Err(TokenSubjectError::InvalidTokenRange {
                index,
                detail: format!("span {start}..{end} exceeds source length {}", source.len()),
            });
        }
        if start < previous_end {
            return Err(TokenSubjectError::InvalidTokenRange {
                index,
                detail: format!(
                    "span {start}..{end} overlaps or precedes the previous token ending at \
                     {previous_end}"
                ),
            });
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return Err(TokenSubjectError::InvalidTokenRange {
                index,
                detail: format!("span {start}..{end} is not on a UTF-8 character boundary"),
            });
        }

        // The payload-free geometry-only `UnknownRest` recovery shape is empty
        // text over a non-empty span: its payload-free geometry *is* the
        // signal, so it is exempt from the payload check rather than repaired.
        //
        // `get` rather than indexing: a reversed span would make `&source[..]`
        // panic. `Token`'s public constructors reject `start > end` and its
        // unchecked constructor is private to `perl-token`, so no caller can
        // reach that arm from here — which is exactly why it must not be a
        // panic. This is a safety floor, not a modelled failure class.
        match source.get(start..end) {
            Some(slice) if token.is_geometry_only() || *token.text == *slice => {}
            Some(_) => return Err(TokenSubjectError::PayloadSourceMismatch { index, start, end }),
            None => {
                return Err(TokenSubjectError::InvalidTokenRange {
                    index,
                    detail: format!("span {start}..{end} is not a valid slice of the source"),
                });
            }
        }

        if token.kind() == TokenKind::Eof {
            seen_eof = true;
        }
        previous_end = end;
    }

    Ok(())
}

fn validate_terminal(
    source: &str,
    tokens: &[Token],
    terminal: TerminalState,
    provenance: &TokenStreamProvenance,
) -> Result<(), TokenSubjectError> {
    match terminal {
        TerminalState::CompleteEof { at } => {
            if at != source.len() {
                return Err(TokenSubjectError::InvalidTerminalState {
                    detail: format!(
                        "complete EOF claimed at {at} but the source ends at {}",
                        source.len()
                    ),
                });
            }
            // A complete EOF must be *carried*, not merely asserted. Without
            // this, an empty token vector over a non-empty source validates:
            // the loop above sees nothing to reject and the claim goes
            // unchallenged. Requiring the terminal event is also what makes
            // "missing EOF" a rejection rather than a silent truncation.
            match tokens.last() {
                Some(last) if last.kind() == TokenKind::Eof && last.start() == at => {}
                Some(last) if last.kind() == TokenKind::Eof => {
                    return Err(TokenSubjectError::InvalidTerminalState {
                        detail: format!(
                            "EOF token at {} does not agree with the terminal EOF at {at}",
                            last.start()
                        ),
                    });
                }
                _ => {
                    return Err(TokenSubjectError::InvalidTerminalState {
                        detail: format!(
                            "a complete EOF at {at} requires a terminal EOF token; this stream \
                             carries none"
                        ),
                    });
                }
            }
        }
        TerminalState::Incomplete { stopped_at } => {
            if stopped_at > source.len() {
                return Err(TokenSubjectError::InvalidTerminalState {
                    detail: format!(
                        "stopped at {stopped_at} beyond the source length {}",
                        source.len()
                    ),
                });
            }
            if tokens.iter().any(|token| token.kind() == TokenKind::Eof) {
                return Err(TokenSubjectError::InvalidTerminalState {
                    detail: "an incomplete stream carries a terminal EOF token".to_owned(),
                });
            }
            // Without this the stop offset is decorative: a stream could report
            // stopping at byte 5 while carrying tokens that run to byte 40.
            if let Some(last) = tokens.last().filter(|last| last.end() > stopped_at) {
                return Err(TokenSubjectError::InvalidTerminalState {
                    detail: format!(
                        "the stream reports stopping at {stopped_at} but its last token ends at {}",
                        last.end()
                    ),
                });
            }
            if provenance.claims_complete_stream() {
                return Err(TokenSubjectError::IncompleteStream {
                    detail: format!(
                        "a {} subject must reach a complete terminal EOF; this stream stopped at \
                         {stopped_at} of {}",
                        provenance.label(),
                        source.len()
                    ),
                });
            }
        }
    }
    Ok(())
}
