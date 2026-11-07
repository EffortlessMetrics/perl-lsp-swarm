//! Canonical prototype-shape projection (#16810).
//!
//! This module is the single semantic parser for old-style Perl prototype
//! strings. AST [`NodeKind::Prototype`](perl_ast::NodeKind::Prototype) keeps
//! the raw spelling as a compatibility projection; providers and compiler
//! consumers must not re-scan that string for slots, optionality, or
//! default-to-topic meaning.
//!
//! Authority: `perlsub` (Perl 5.40/5.42). `;` is the optional-slot boundary,
//! not a signature separator. `_` is a scalar-like slot that uses `$_` when
//! its argument is omitted, and is legal only as the last character or
//! immediately before `;`, `@`, or `%`.

mod digest;
mod project;

#[cfg(test)]
mod tests;

pub use digest::PrototypeSemanticDigest;
pub(crate) use project::is_prototype_char;
pub use project::{project_prototype_shape, raw_from_attribute};

/// Ordered, typed projection of one prototype string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeShape {
    raw: String,
    slots: Vec<PrototypeSlot>,
    optional_boundary: Option<PrototypeBoundary>,
    completeness: PrototypeCompleteness,
    syntax_class: PrototypeSyntaxClass,
    semantic_digest: PrototypeSemanticDigest,
}

/// One typed prototype slot with source spelling retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrototypeSlot {
    kind: PrototypeSlotKind,
    optional: bool,
    default: PrototypeDefault,
    raw: String,
    start: usize,
    end: usize,
}

/// Source range of the `;` optionality boundary inside the raw prototype text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrototypeBoundary {
    start: usize,
    end: usize,
}

/// Typed prototype slot kinds admitted by `perlsub`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrototypeSlotKind {
    /// `$` — scalar context.
    Scalar,
    /// `@` — slurpy list.
    ArraySlurpy,
    /// `%` — slurpy hash.
    HashSlurpy,
    /// `&` — code / block-taking when first and unbackslashed.
    Code,
    /// `*` — glob / typeglob.
    Glob,
    /// `+` — scalar, or array/hash reference for a literal aggregate.
    ScalarOrReference,
    /// `_` — scalar that defaults to `$_` when omitted.
    TopicDefaultScalar,
    /// Backslashed referent such as `\@`.
    ReferenceTo(PrototypeReferent),
    /// Backslash group such as `\[$@%&*]`.
    GroupedReference(Vec<PrototypeReferent>),
}

/// Referent admitted after `\` or inside `\[...]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrototypeReferent {
    /// `$`
    Scalar,
    /// `@`
    Array,
    /// `%`
    Hash,
    /// `&`
    Code,
    /// `*`
    Glob,
}

/// Default disposition for a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrototypeDefault {
    /// No implicit argument.
    None,
    /// Omitted argument uses the topic variable `$_`.
    TopicVariable,
}

/// Whether the projection is an exact semantic shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrototypeCompleteness {
    /// Every character was admitted and composition rules held.
    Exact,
    /// The raw text was preserved but must not be treated as an exact shape.
    Recovered {
        /// Why the shape is non-exact.
        reason: PrototypeRecovery,
    },
}

/// Why a prototype string cannot be an exact shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrototypeRecovery {
    /// Character outside the `perlsub` prototype alphabet.
    InvalidCharacter,
    /// More than one `;` boundary.
    DuplicateOptionalBoundary,
    /// `_` not last, and not immediately before `;`, `@`, or `%`.
    InvalidTopicDefaultPosition,
    /// `\[` without a closing `]`.
    UnclosedGroup,
    /// `\[\]` with no referents.
    EmptyGroup,
    /// `\` not followed by `$ @ % & *` or `[`.
    DanglingBackslash,
    /// `]` or other closer outside a group.
    UnexpectedCloser,
    /// A slot or `;` follows an unbackslashed `@` or `%`. Those slurps remaining
    /// arguments, so later material is not an exact prototype (`perlsub`).
    SlotAfterSlurpy,
    /// Form that is not projected as exact (reserved for oracle-disputed cases).
    UnsupportedForm,
}

/// Syntax-affecting classification derived from the projected slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrototypeSyntaxClass {
    /// No slots after whitespace is ignored — nullary, like `time`.
    Nullary,
    /// Leading unbackslashed `&` may take a bare block.
    BlockTaking,
    /// Ordinary prototype; not claimed as nullary or block-taking.
    Ordinary,
}

impl PrototypeShape {
    /// Project `raw` prototype text (no surrounding parentheses).
    #[must_use]
    pub fn project(raw: impl Into<String>) -> Self {
        project::project_prototype_shape(&raw.into())
    }

    /// Exact source spelling, including formatting whitespace.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Ordered slots. Whitespace is not a slot.
    #[must_use]
    pub fn slots(&self) -> &[PrototypeSlot] {
        &self.slots
    }

    /// Byte range of the `;` boundary in [`Self::raw`], when present.
    #[must_use]
    pub fn optional_boundary(&self) -> Option<PrototypeBoundary> {
        self.optional_boundary
    }

    /// Index of the first optional slot, if any slot follows `;`.
    #[must_use]
    pub fn first_optional_index(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.optional)
    }

    /// Completeness of this projection.
    #[must_use]
    pub fn completeness(&self) -> &PrototypeCompleteness {
        &self.completeness
    }

    /// Whether this shape may be treated as exact.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self.completeness, PrototypeCompleteness::Exact)
    }

    /// Syntax-affecting classification.
    #[must_use]
    pub fn syntax_class(&self) -> PrototypeSyntaxClass {
        self.syntax_class
    }

    /// Deterministic digest of semantic slots, independent of formatting whitespace.
    #[must_use]
    pub fn semantic_digest(&self) -> &PrototypeSemanticDigest {
        &self.semantic_digest
    }
}

impl PrototypeSlot {
    /// Slot kind.
    #[must_use]
    pub fn kind(&self) -> &PrototypeSlotKind {
        &self.kind
    }

    /// True when this slot follows the `;` optionality boundary.
    #[must_use]
    pub fn is_optional(&self) -> bool {
        self.optional
    }

    /// Default disposition, including topic-default for `_`.
    #[must_use]
    pub fn default(&self) -> PrototypeDefault {
        self.default
    }

    /// Slot spelling from the raw prototype, without surrounding whitespace.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Inclusive-start exclusive-end byte range in the parent raw text.
    #[must_use]
    pub fn byte_range(&self) -> (usize, usize) {
        (self.start, self.end)
    }
}

impl PrototypeBoundary {
    /// Start byte offset of `;` in the raw prototype text.
    #[must_use]
    pub fn start(self) -> usize {
        self.start
    }

    /// End byte offset of `;` in the raw prototype text.
    #[must_use]
    pub fn end(self) -> usize {
        self.end
    }
}

impl PrototypeCompleteness {
    /// Stable tag for digests and tests.
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Recovered { reason } => reason.tag(),
        }
    }
}

impl PrototypeRecovery {
    /// Stable tag for digests and tests.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::InvalidCharacter => "invalid-character",
            Self::DuplicateOptionalBoundary => "duplicate-optional-boundary",
            Self::InvalidTopicDefaultPosition => "invalid-topic-default-position",
            Self::UnclosedGroup => "unclosed-group",
            Self::EmptyGroup => "empty-group",
            Self::DanglingBackslash => "dangling-backslash",
            Self::UnexpectedCloser => "unexpected-closer",
            Self::SlotAfterSlurpy => "slot-after-slurpy",
            Self::UnsupportedForm => "unsupported-form",
        }
    }
}

impl PrototypeSyntaxClass {
    /// Stable tag for digests and tests.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Nullary => "nullary",
            Self::BlockTaking => "block-taking",
            Self::Ordinary => "ordinary",
        }
    }
}

impl PrototypeSlotKind {
    /// Stable tag for digests and tests.
    #[must_use]
    pub fn tag(&self) -> String {
        match self {
            Self::Scalar => "scalar".to_string(),
            Self::ArraySlurpy => "array-slurpy".to_string(),
            Self::HashSlurpy => "hash-slurpy".to_string(),
            Self::Code => "code".to_string(),
            Self::Glob => "glob".to_string(),
            Self::ScalarOrReference => "scalar-or-reference".to_string(),
            Self::TopicDefaultScalar => "topic-default-scalar".to_string(),
            Self::ReferenceTo(referent) => format!("ref:{}", referent.tag()),
            Self::GroupedReference(referents) => {
                let joined = referents.iter().map(|referent| referent.tag()).collect::<Vec<_>>();
                format!("group:{}", joined.join("+"))
            }
        }
    }
}

impl PrototypeReferent {
    /// Stable tag for digests and tests.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Array => "array",
            Self::Hash => "hash",
            Self::Code => "code",
            Self::Glob => "glob",
        }
    }
}

impl PrototypeDefault {
    /// Stable tag for digests and tests.
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::TopicVariable => "topic-variable",
        }
    }
}
