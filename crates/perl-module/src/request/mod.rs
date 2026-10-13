//! Validated module requests and typed resolution outcomes.
//!
//! Resolution entrypoints historically accepted an arbitrary `&str` and returned
//! a three-state enum. That erased material distinctions before resolution even
//! began: a module-style bareword, a quoted literal filename, a partially static
//! expression, a fully dynamic expression, and an outright invalid string all
//! looked alike, and a valid-but-missing module was indistinguishable from a
//! request that was never valid.
//!
//! This module introduces the protocol-neutral domain boundary:
//!
//! ```text
//! source operand
//!   → ModuleRequest (bareword | literal file | partially static | dynamic)
//!   → ModuleResolutionOutcome (exact answer | classified boundary)
//! ```
//!
//! Nothing here touches the filesystem, workspace trust, LSP types, or provider
//! policy. Validation proves the *shape* of a request; it never proves that a
//! module exists.
//!
//! # Not to be confused with `perl_parser_core::hir::ModuleRequest`
//!
//! The upstream `perl-parser-core` crate defines its own `ModuleRequest` and
//! `ModuleRequestKind`. Those are a *different layer* and are not duplicated
//! here: a HIR `ModuleRequest` is a post-parse compiled fact carrying a source
//! range, scope, package context, resolution status, provenance, and confidence,
//! and its `ModuleRequestKind` names the directive that produced it
//! (`Use` / `Require` / `Parent` / `Base`).
//!
//! The [`ModuleRequest`] in this module classifies the *syntactic shape of a raw
//! operand* before any resolution has happened, and deliberately carries no
//! provenance, confidence, or resolution state. The two meet later: a HIR fact
//! supplies the operand, this vocabulary validates and classifies it. Import
//! whichever is meant by its crate path rather than unqualified.
//!
//! # Compatibility adapters
//!
//! | Adapter | Caller inventory | Removal owner |
//! | --- | --- | --- |
//! | [`outcome_from_uri_resolution`] | every current `resolve_module_uri*` consumer | #8521 (M02) |
//! | [`uri_resolution_from_outcome`] | unmigrated consumers during M01 → M02 | #8521 (M02) |
//!
//! Both adapters are documented with the classification they refuse to erase.

mod file_path;
mod name;
mod outcome;

use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

pub use file_path::{ModuleFilePath, ModuleFilePathError};
pub use name::{LegacySeparatorProfile, ModuleName, ModuleNameError, PackageSeparatorForm};
pub use outcome::{
    AbsenceEvidence, ModuleResolutionOutcome, ResolvedEvidence, outcome_from_uri_resolution,
    uri_resolution_from_outcome,
};

use crate::token_core::ModuleTokenSpan;

/// Why a request is not an exact lookup subject.
///
/// This is a bounded vocabulary, not free text: a boundary must be classifiable
/// by a consumer that never saw the source.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RequestBoundary {
    /// The operand interpolates a variable.
    VariableInterpolation,
    /// The operand is produced by an expression.
    ComputedExpression,
    /// The operand is a runtime string with no static fragment.
    RuntimeString,
    /// The operand comes from a construct this crate does not model.
    UnmodeledConstruct,
}

impl RequestBoundary {
    /// Stable identifier for evidence rows and diagnostics.
    #[must_use]
    pub const fn boundary_id(self) -> &'static str {
        match self {
            Self::VariableInterpolation => "request_boundary.variable_interpolation",
            Self::ComputedExpression => "request_boundary.computed_expression",
            Self::RuntimeString => "request_boundary.runtime_string",
            Self::UnmodeledConstruct => "request_boundary.unmodeled_construct",
        }
    }
}

impl fmt::Display for RequestBoundary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VariableInterpolation => f.write_str("operand interpolates a variable"),
            Self::ComputedExpression => f.write_str("operand is a computed expression"),
            Self::RuntimeString => f.write_str("operand is a runtime string"),
            Self::UnmodeledConstruct => f.write_str("operand construct is not modeled"),
        }
    }
}

/// A request whose operand is only partially static.
///
/// The source form, the static fragments that *were* recovered, and the exact
/// span are retained. A partially static request is never promoted to an exact
/// one merely because it contains a plausible-looking fragment.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct PartialModuleRequest {
    source_form: String,
    static_fragments: Vec<String>,
    span: Option<ModuleTokenSpan>,
    boundary: RequestBoundary,
}

impl PartialModuleRequest {
    /// The operand exactly as written in source.
    #[must_use]
    pub fn source_form(&self) -> &str {
        &self.source_form
    }

    /// The static fragments recovered from the operand, in source order.
    #[must_use]
    pub fn static_fragments(&self) -> &[String] {
        &self.static_fragments
    }

    /// The operand's byte span, when the producer knew one.
    #[must_use]
    pub const fn span(&self) -> Option<ModuleTokenSpan> {
        self.span
    }

    /// Why the operand is not exact.
    #[must_use]
    pub const fn boundary(&self) -> RequestBoundary {
        self.boundary
    }
}

/// A request whose operand is fully dynamic.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub struct DynamicModuleRequest {
    source_form: String,
    span: Option<ModuleTokenSpan>,
    boundary: RequestBoundary,
}

impl DynamicModuleRequest {
    /// The operand exactly as written in source.
    #[must_use]
    pub fn source_form(&self) -> &str {
        &self.source_form
    }

    /// The operand's byte span, when the producer knew one.
    #[must_use]
    pub const fn span(&self) -> Option<ModuleTokenSpan> {
        self.span
    }

    /// Why the operand is not exact.
    #[must_use]
    pub const fn boundary(&self) -> RequestBoundary {
        self.boundary
    }
}

/// The classification of a [`ModuleRequest`], without its payload.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleRequestKind {
    /// A module-style bareword operand (`use Foo::Bar;`).
    BarewordModule,
    /// A quoted literal filename operand (`require "Foo/Bar.pm";`).
    LiteralRelativeFile,
    /// A partially static operand.
    PartiallyStatic,
    /// A fully dynamic operand.
    Dynamic,
}

/// Why a source operand is not a validated request.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleRequestError {
    /// A bareword operand failed module-name validation.
    InvalidModuleName(ModuleNameError),
    /// A quoted operand failed literal relative-file validation.
    InvalidFilePath(ModuleFilePathError),
}

impl fmt::Display for ModuleRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModuleName(error) => write!(f, "{error}"),
            Self::InvalidFilePath(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ModuleRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidModuleName(error) => Some(error),
            Self::InvalidFilePath(error) => Some(error),
        }
    }
}

impl ModuleRequestError {
    /// Stable identifier for evidence rows and diagnostics.
    #[must_use]
    pub const fn boundary_id(&self) -> &'static str {
        match self {
            Self::InvalidModuleName(error) => error.boundary_id(),
            Self::InvalidFilePath(error) => error.boundary_id(),
        }
    }
}

impl From<ModuleNameError> for ModuleRequestError {
    fn from(error: ModuleNameError) -> Self {
        Self::InvalidModuleName(error)
    }
}

impl From<ModuleFilePathError> for ModuleRequestError {
    fn from(error: ModuleFilePathError) -> Self {
        Self::InvalidFilePath(error)
    }
}

/// A validated, protocol-neutral module request.
///
/// The variant records how the operand was written in Perl, and that
/// classification cannot be recovered from a string once lost. In particular a
/// quoted `require "Foo::Bar"` is a *filename* and never becomes a bareword
/// module request.
#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum ModuleRequest {
    /// A module-style bareword operand.
    BarewordModule(ModuleName),
    /// A quoted literal filename operand.
    LiteralRelativeFile(ModuleFilePath),
    /// A partially static operand.
    PartiallyStatic(PartialModuleRequest),
    /// A fully dynamic operand.
    Dynamic(DynamicModuleRequest),
}

impl ModuleRequest {
    /// Classify a bareword `use`/`require`/`no` operand under the default profile.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleRequestError::InvalidModuleName`] when the operand is not
    /// a valid module name.
    pub fn bareword(text: &str) -> Result<Self, ModuleRequestError> {
        Self::bareword_with_profile(text, LegacySeparatorProfile::Accept)
    }

    /// Classify a bareword operand under an explicit legacy-separator profile.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleRequestError::InvalidModuleName`] when the operand is not
    /// a valid module name under `profile`.
    pub fn bareword_with_profile(
        text: &str,
        profile: LegacySeparatorProfile,
    ) -> Result<Self, ModuleRequestError> {
        Ok(Self::BarewordModule(ModuleName::parse_with_profile(text, profile)?))
    }

    /// Classify a *quoted* `require` operand.
    ///
    /// Perl looks a quoted operand up as a filename in `@INC` without translating
    /// `::` to `/`, so this never yields [`Self::BarewordModule`].
    ///
    /// `text` is the **decoded** operand — the value Perl looks up. Quote
    /// characters in it are filename bytes and are preserved, because Perl
    /// permits them in a filename and the string alone cannot say whether it is
    /// a decoded value or a still-quoted token. A caller holding the raw token
    /// states so by calling [`Self::quoted_require_token`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleRequestError::InvalidFilePath`] when the operand is not a
    /// valid literal relative file request.
    pub fn quoted_require(text: &str) -> Result<Self, ModuleRequestError> {
        Ok(Self::LiteralRelativeFile(ModuleFilePath::parse(text)?))
    }

    /// Classify a *raw* quoted `require` token, delimiters included.
    ///
    /// Strips exactly one matching outer pair of `'` or `"` and classifies the
    /// operand inside. This is the constructor for a caller bridging a
    /// `perl_parser_core::hir` require target, whose value is stored verbatim.
    ///
    /// # Errors
    ///
    /// Returns [`ModuleRequestError::InvalidFilePath`] when the token carries no
    /// matching delimiter pair, or when the decoded operand is not a valid
    /// literal relative file request.
    pub fn quoted_require_token(token: &str) -> Result<Self, ModuleRequestError> {
        Ok(Self::LiteralRelativeFile(ModuleFilePath::from_quoted_token(token)?))
    }

    /// Record a partially static operand and why it is not exact.
    ///
    /// "Partially static" means at least one non-empty static fragment was
    /// actually recovered. A `static_fragments` that is empty — or that holds
    /// only empty strings, which is evidence-shaped but carries no known text —
    /// is not a partial request but a fully dynamic one, so this normalizes to
    /// [`Self::Dynamic`] rather than producing a variant whose name overstates
    /// what is behind it. No evidence is lost: both variants retain the source
    /// form and span.
    ///
    /// [`RequestBoundary::RuntimeString`] is defined as "no static fragment",
    /// so it can never label a partial request: a caller that pairs it with
    /// recovered text is contradicting itself, and the boundary wins. The
    /// request normalizes to [`Self::Dynamic`] and the fragments are dropped,
    /// because a runtime string has, by definition, none to keep.
    #[must_use]
    pub fn partially_static(
        source_form: impl Into<String>,
        static_fragments: Vec<String>,
        span: Option<ModuleTokenSpan>,
        boundary: RequestBoundary,
    ) -> Self {
        if boundary == RequestBoundary::RuntimeString
            || static_fragments.iter().all(String::is_empty)
        {
            return Self::dynamic(source_form, span, boundary);
        }

        Self::PartiallyStatic(PartialModuleRequest {
            source_form: source_form.into(),
            static_fragments,
            span,
            boundary,
        })
    }

    /// Record a fully dynamic operand and why it is not exact.
    #[must_use]
    pub fn dynamic(
        source_form: impl Into<String>,
        span: Option<ModuleTokenSpan>,
        boundary: RequestBoundary,
    ) -> Self {
        Self::Dynamic(DynamicModuleRequest { source_form: source_form.into(), span, boundary })
    }

    /// The request's classification.
    #[must_use]
    pub const fn kind(&self) -> ModuleRequestKind {
        match self {
            Self::BarewordModule(_) => ModuleRequestKind::BarewordModule,
            Self::LiteralRelativeFile(_) => ModuleRequestKind::LiteralRelativeFile,
            Self::PartiallyStatic(_) => ModuleRequestKind::PartiallyStatic,
            Self::Dynamic(_) => ModuleRequestKind::Dynamic,
        }
    }

    /// The validated module name, for bareword requests only.
    ///
    /// A literal filename request has no module identity, so this returns `None`
    /// rather than reinterpreting the filename as a module name.
    #[must_use]
    pub const fn module_name(&self) -> Option<&ModuleName> {
        match self {
            Self::BarewordModule(name) => Some(name),
            _ => None,
        }
    }

    /// The validated literal file request, for quoted requests only.
    #[must_use]
    pub const fn literal_file(&self) -> Option<&ModuleFilePath> {
        match self {
            Self::LiteralRelativeFile(path) => Some(path),
            _ => None,
        }
    }

    /// Whether the request identifies an exact lookup subject.
    #[must_use]
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::BarewordModule(_) | Self::LiteralRelativeFile(_))
    }

    /// Why the request is not exact, or `None` when it is.
    #[must_use]
    pub const fn boundary(&self) -> Option<RequestBoundary> {
        match self {
            Self::BarewordModule(_) | Self::LiteralRelativeFile(_) => None,
            Self::PartiallyStatic(request) => Some(request.boundary()),
            Self::Dynamic(request) => Some(request.boundary()),
        }
    }
}

impl fmt::Display for ModuleRequest {
    /// Renders a kind-qualified form.
    ///
    /// The kind prefix is deliberate: an unqualified render would let a literal
    /// filename be read as a logical module identity.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BarewordModule(name) => write!(f, "module:{name}"),
            Self::LiteralRelativeFile(path) => write!(f, "file:{path}"),
            Self::PartiallyStatic(_) => f.write_str("partial:<redacted>"),
            Self::Dynamic(_) => f.write_str("dynamic:<redacted>"),
        }
    }
}

impl fmt::Debug for PartialModuleRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("PartialModuleRequest");
        debug
            .field("kind", &ModuleRequestKind::PartiallyStatic)
            .field("boundary", &self.boundary)
            .field("span", &self.span)
            .field("static_fragment_count", &self.static_fragments.len())
            .finish()
    }
}

impl fmt::Debug for DynamicModuleRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("DynamicModuleRequest");
        debug
            .field("kind", &ModuleRequestKind::Dynamic)
            .field("boundary", &self.boundary)
            .field("span", &self.span)
            .finish()
    }
}

impl fmt::Debug for ModuleRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ModuleRequest");
        debug.field("kind", &self.kind());
        match self {
            Self::BarewordModule(_) | Self::LiteralRelativeFile(_) => {}
            Self::PartiallyStatic(request) => {
                debug
                    .field("boundary", &request.boundary)
                    .field("span", &request.span)
                    .field("static_fragment_count", &request.static_fragments.len());
            }
            Self::Dynamic(request) => {
                debug.field("boundary", &request.boundary).field("span", &request.span);
            }
        }
        debug.finish()
    }
}

impl Serialize for ModuleRequest {
    /// Serialize only the request's structural classification.
    ///
    /// Raw source expressions and validated file/module payloads remain
    /// available only through their explicitly named evidence accessors. This
    /// one-way representation is intentionally not deserializable: callers
    /// must use the validating constructors to mint a request.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ModuleRequest", 5)?;
        state.serialize_field("kind", request_kind_name(self.kind()))?;
        state.serialize_field("boundary", &self.boundary().map(RequestBoundary::boundary_id))?;
        match self {
            Self::PartiallyStatic(request) => {
                state.serialize_field("span_start", &request.span.map(|span| span.start))?;
                state.serialize_field("span_end", &request.span.map(|span| span.end))?;
                state.serialize_field("static_fragment_count", &request.static_fragments.len())?;
            }
            Self::Dynamic(request) => {
                state.serialize_field("span_start", &request.span.map(|span| span.start))?;
                state.serialize_field("span_end", &request.span.map(|span| span.end))?;
                state.serialize_field("static_fragment_count", &0usize)?;
            }
            Self::BarewordModule(_) | Self::LiteralRelativeFile(_) => {
                state.serialize_field("span_start", &Option::<usize>::None)?;
                state.serialize_field("span_end", &Option::<usize>::None)?;
                state.serialize_field("static_fragment_count", &0usize)?;
            }
        }
        state.end()
    }
}

fn request_kind_name(kind: ModuleRequestKind) -> &'static str {
    match kind {
        ModuleRequestKind::BarewordModule => "bareword_module",
        ModuleRequestKind::LiteralRelativeFile => "literal_relative_file",
        ModuleRequestKind::PartiallyStatic => "partially_static",
        ModuleRequestKind::Dynamic => "dynamic",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DynamicModuleRequest, ModuleRequest, ModuleRequestError, ModuleRequestKind,
        PartialModuleRequest, RequestBoundary,
    };
    use crate::token_core::ModuleTokenSpan;

    #[test]
    fn bareword_and_quoted_operands_stay_distinct() -> Result<(), ModuleRequestError> {
        let bareword = ModuleRequest::bareword("Foo::Bar")?;
        let quoted = ModuleRequest::quoted_require("Foo::Bar")?;

        assert_eq!(bareword.kind(), ModuleRequestKind::BarewordModule);
        assert_eq!(quoted.kind(), ModuleRequestKind::LiteralRelativeFile);
        assert_ne!(
            bareword, quoted,
            "identical text under different Perl syntax is not one request"
        );

        assert_eq!(bareword.module_name().map(ToString::to_string), Some("Foo::Bar".to_string()));
        assert_eq!(quoted.module_name(), None, "a filename has no module identity");
        assert_eq!(quoted.literal_file().map(ToString::to_string), Some("Foo::Bar".to_string()));
        assert_eq!(bareword.literal_file(), None);
        Ok(())
    }

    #[test]
    fn display_is_kind_qualified() -> Result<(), ModuleRequestError> {
        assert_eq!(ModuleRequest::bareword("Foo::Bar")?.to_string(), "module:Foo::Bar");
        assert_eq!(ModuleRequest::quoted_require("Foo/Bar.pm")?.to_string(), "file:Foo/Bar.pm");
        assert_eq!(
            ModuleRequest::dynamic("$class", None, RequestBoundary::VariableInterpolation)
                .to_string(),
            "dynamic:<redacted>"
        );
        assert_eq!(
            ModuleRequest::partially_static(
                "\"Foo::$leaf\"",
                vec!["Foo::".to_string()],
                None,
                RequestBoundary::VariableInterpolation,
            )
            .to_string(),
            "partial:<redacted>"
        );
        Ok(())
    }

    #[test]
    fn display_debug_and_serde_redact_source_evidence() -> Result<(), Box<dyn std::error::Error>> {
        let source = "/private/customer/$class";
        let request = ModuleRequest::dynamic(
            source,
            Some(ModuleTokenSpan { start: 8, end: 32 }),
            RequestBoundary::RuntimeString,
        );

        let display = request.to_string();
        let debug = format!("{request:?}");
        let serialized = serde_json::to_string(&request)?;

        assert!(!display.contains(source));
        assert!(!debug.contains(source));
        assert!(!serialized.contains(source));
        assert!(serialized.contains("dynamic"));
        assert!(serialized.contains("request_boundary.runtime_string"));
        assert!(serialized.contains("static_fragment_count"));
        Ok(())
    }

    #[test]
    fn inexact_requests_keep_their_boundary_and_evidence() {
        let span = ModuleTokenSpan { start: 4, end: 16 };
        let partial = ModuleRequest::partially_static(
            "\"Foo::$leaf\"",
            vec!["Foo::".to_string()],
            Some(span),
            RequestBoundary::VariableInterpolation,
        );

        assert!(!partial.is_exact(), "a static fragment does not make a request exact");
        assert_eq!(partial.module_name(), None);
        assert_eq!(partial.boundary(), Some(RequestBoundary::VariableInterpolation));

        let inner = match &partial {
            ModuleRequest::PartiallyStatic(inner) => Some(inner),
            _ => None,
        };
        assert_eq!(
            inner.map(PartialModuleRequest::static_fragments),
            Some(&["Foo::".to_string()][..]),
            "partially static request must keep its variant and fragments"
        );
        assert_eq!(inner.and_then(PartialModuleRequest::span), Some(span));
        assert_eq!(inner.map(PartialModuleRequest::source_form), Some("\"Foo::$leaf\""));
    }

    #[test]
    fn a_partial_request_without_evidence_is_simply_dynamic() {
        let span = ModuleTokenSpan { start: 4, end: 10 };
        let request = ModuleRequest::partially_static(
            "$class",
            Vec::new(),
            Some(span),
            RequestBoundary::VariableInterpolation,
        );

        assert_eq!(
            request.kind(),
            ModuleRequestKind::Dynamic,
            "no recovered fragment means the operand is dynamic, not partly known"
        );
        assert_eq!(request.boundary(), Some(RequestBoundary::VariableInterpolation));

        let source_form = match &request {
            ModuleRequest::Dynamic(inner) => Some(DynamicModuleRequest::source_form(inner)),
            _ => None,
        };
        assert_eq!(source_form, Some("$class"), "the source form survives the normalization");
        let retained_span = match &request {
            ModuleRequest::Dynamic(inner) => DynamicModuleRequest::span(inner),
            _ => None,
        };
        assert_eq!(retained_span, Some(span), "the span survives the normalization");
    }

    #[test]
    fn empty_string_fragments_are_not_evidence() {
        // A vector of empty strings is evidence-shaped but carries no known text,
        // so it must not sustain the `PartiallyStatic` claim either.
        for fragments in [vec![String::new()], vec![String::new(), String::new()]] {
            let request = ModuleRequest::partially_static(
                "$class",
                fragments.clone(),
                None,
                RequestBoundary::VariableInterpolation,
            );
            assert_eq!(
                request.kind(),
                ModuleRequestKind::Dynamic,
                "{fragments:?} recovers no text, so the operand is dynamic"
            );
        }

        // One genuinely recovered fragment still yields a partial request, even
        // alongside empty ones.
        let request = ModuleRequest::partially_static(
            "\"Foo::$leaf\"",
            vec![String::new(), "Foo::".to_string()],
            None,
            RequestBoundary::VariableInterpolation,
        );
        assert_eq!(request.kind(), ModuleRequestKind::PartiallyStatic);
    }

    #[test]
    fn runtime_string_boundary_never_labels_a_partial_request() {
        // `RuntimeString` means "no static fragment"; recovered text cannot
        // coexist with it, so the boundary wins and the request is dynamic.
        for fragments in [vec!["Foo::".to_string()], vec!["Foo".to_string(), "Bar".to_string()]] {
            let span = ModuleTokenSpan { start: 3, end: 9 };
            let request = ModuleRequest::partially_static(
                "$runtime",
                fragments.clone(),
                Some(span),
                RequestBoundary::RuntimeString,
            );
            assert_eq!(
                request.kind(),
                ModuleRequestKind::Dynamic,
                "{fragments:?} with a runtime-string boundary is a contradiction"
            );
            assert_eq!(request.boundary(), Some(RequestBoundary::RuntimeString));
            let retained_span = match &request {
                ModuleRequest::Dynamic(inner) => DynamicModuleRequest::span(inner),
                _ => None,
            };
            assert_eq!(retained_span, Some(span), "the span survives the normalization");
        }

        // Every other boundary still admits genuinely recovered fragments.
        for boundary in [
            RequestBoundary::VariableInterpolation,
            RequestBoundary::ComputedExpression,
            RequestBoundary::UnmodeledConstruct,
        ] {
            let request = ModuleRequest::partially_static(
                "\"Foo::$leaf\"",
                vec!["Foo::".to_string()],
                None,
                boundary,
            );
            assert_eq!(request.kind(), ModuleRequestKind::PartiallyStatic, "{boundary:?}");
        }
    }

    #[test]
    fn exact_requests_have_no_boundary() -> Result<(), ModuleRequestError> {
        assert!(ModuleRequest::bareword("Foo")?.is_exact());
        assert_eq!(ModuleRequest::bareword("Foo")?.boundary(), None);
        assert!(ModuleRequest::quoted_require("Foo.pl")?.is_exact());
        assert_eq!(ModuleRequest::quoted_require("Foo.pl")?.boundary(), None);
        Ok(())
    }

    #[test]
    fn invalid_operands_are_errors_not_dynamic_requests() {
        for text in ["", "../../etc/passwd", "Foo Bar", "$Foo"] {
            assert!(
                ModuleRequest::bareword(text).is_err(),
                "`{text}` must not become a validated bareword request"
            );
        }
        for text in ["", "/etc/passwd", "../escape"] {
            assert!(
                ModuleRequest::quoted_require(text).is_err(),
                "`{text}` must not become a validated file request"
            );
        }
    }

    #[test]
    fn request_errors_expose_their_name_or_path_classification() {
        let name_error = ModuleRequest::bareword("Foo/Bar").err().map(|e| e.boundary_id());
        assert_eq!(name_error, Some("module_name.path_separator"));

        let path_error = ModuleRequest::quoted_require("/abs").err().map(|e| e.boundary_id());
        assert_eq!(path_error, Some("module_file_path.absolute"));
    }
}
