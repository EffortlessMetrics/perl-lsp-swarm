//! Shared dancer2 contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::dancer2`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::SourceGeneration;
use crate::framework::AdapterId;

/// Framework name handled by this adapter.
pub const DANCER2_FRAMEWORK_NAME: &str = "Dancer2";

/// Reviewed supported version range for the Dancer2 application DSL.
///
/// Covers the Dancer2 1.x series. The workspace fixture
/// `test_corpus/real_projects/dancer2_skeleton/lib/Dancer2.pm` carries
/// `1.1.1` (`36bdd7aa4e9b97585ad545180a2135fe26278ad1`). Terminal 1.x
/// release 1.1.2 (`126be702b5628a8ab91e8ec6883f7c975c8898f1`, 2024-11-25)
/// changes only `.github/workflows/ci.yml`, `Changes`, `cpanfile`, and
/// `dist.ini`; the 1.1 semantic profile is reused through 1.1.2. Dancer2
/// 2.x is a separate bounded profile
/// ([`crate::framework_adapters::dancer2_two_x`]), not a widening of this
/// constraint.
pub const DANCER2_VERSION_CONSTRAINT: &str = ">=1.0.0,<2.0.0";

/// Provisional adapter identity.
///
/// The generic registry (#6821) owns final identity assignment; this stable
/// value is reserved for Dancer2 so shadow receipts remain comparable across
/// the registry extraction.
pub const DANCER2_ADAPTER_ID: AdapterId = AdapterId(0x0044_4E43);

/// Versioned identity of the reviewed default-DSL keyword contract.
///
/// The table is the reviewed Dancer2 1.x union: the v1.0.0 registry at
/// `PerlDancer/Dancer2@ab2a50a478f81b005d0a6f4f1ff2a1f6f9ce1aa5`
/// is the base, while `uri_for_route` was introduced by
/// `PerlDancer/Dancer2@1211dbf5e19ffbe46026ddf983c444ce1f5c0b6d`
/// and shipped in v1.1.0. Facts are filtered against the observed
/// framework version before publication. v3 also corrects `redirect`
/// and `cookie` scope, preserves the four upstream-deprecated exports,
/// and removes route/hook names that the reviewed DSL does not register
/// as keywords (#13089).
pub const DANCER2_DSL_CONTRACT_VERSION: &str = "dancer2-dsl.1-1.v3";

/// Reviewed versioned-descriptor schema revision for this adapter. Tracks
/// [`FRAMEWORK_ADAPTER_SCHEMA_VERSION`](crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION):
/// the descriptor travels on the adapter SDK wire, whose version 2 carries
/// the #8921 route-family fact kinds.
pub const DANCER2_DESCRIPTOR_REVISION: u32 = crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Reviewed default Dancer2 DSL keyword scope.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DslKeywordScope {
    /// Available in any package scope that activated the DSL.
    Global,
    /// Available only inside route handlers/hooks (request context).
    RouteHandlerOnly,
}

/// One reviewed default-DSL keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dancer2DslKeyword {
    /// DSL keyword name.
    pub name: &'static str,
    /// Availability scope in the reviewed contract.
    pub scope: DslKeywordScope,
    /// Whether the reviewed contract still exports the keyword but marks it
    /// deprecated (preserved as versioned metadata, never silently dropped).
    pub deprecated: bool,
}

const GLOBAL: DslKeywordScope = DslKeywordScope::Global;
const ROUTE: DslKeywordScope = DslKeywordScope::RouteHandlerOnly;
const fn kw(name: &'static str, scope: DslKeywordScope) -> Dancer2DslKeyword {
    Dancer2DslKeyword { name, scope, deprecated: false }
}
const fn deprecated_kw(name: &'static str, scope: DslKeywordScope) -> Dancer2DslKeyword {
    Dancer2DslKeyword { name, scope, deprecated: true }
}

/// Reviewed default Dancer2 1.x DSL keyword union.
///
/// Entries intentionally follow the v1.1.1 upstream `dsl_keywords`
/// registration order so review can compare the two tables without
/// reconstructing a second grouping. Per-activation facts filter additions
/// that were unavailable at the observed framework version. Hook names such
/// as `before` and `after` are operands to `hook`, not imported DSL keywords.
pub const DANCER2_DSL_KEYWORDS: &[Dancer2DslKeyword] = &[
    kw("any", GLOBAL),
    kw("app", GLOBAL),
    kw("captures", ROUTE),
    kw("config", GLOBAL),
    kw("content", ROUTE),
    kw("content_type", ROUTE),
    deprecated_kw("context", ROUTE),
    kw("cookie", ROUTE),
    kw("cookies", ROUTE),
    kw("dance", GLOBAL),
    kw("dancer_app", GLOBAL),
    kw("dancer_version", GLOBAL),
    kw("dancer_major_version", GLOBAL),
    kw("debug", GLOBAL),
    kw("decode_json", GLOBAL),
    kw("del", GLOBAL),
    kw("delayed", ROUTE),
    kw("dirname", GLOBAL),
    kw("done", ROUTE),
    kw("dsl", GLOBAL),
    kw("encode_json", GLOBAL),
    kw("engine", GLOBAL),
    kw("error", GLOBAL),
    kw("false", GLOBAL),
    kw("flush", ROUTE),
    kw("forward", ROUTE),
    kw("from_dumper", GLOBAL),
    kw("from_json", GLOBAL),
    kw("from_yaml", GLOBAL),
    kw("get", GLOBAL),
    kw("halt", ROUTE),
    deprecated_kw("header", ROUTE),
    deprecated_kw("headers", ROUTE),
    kw("hook", GLOBAL),
    kw("info", GLOBAL),
    kw("log", GLOBAL),
    kw("mime", GLOBAL),
    kw("options", GLOBAL),
    kw("param", ROUTE),
    kw("params", ROUTE),
    kw("query_parameters", ROUTE),
    kw("body_parameters", ROUTE),
    kw("route_parameters", ROUTE),
    kw("pass", ROUTE),
    kw("patch", GLOBAL),
    kw("path", GLOBAL),
    kw("post", GLOBAL),
    kw("prefix", GLOBAL),
    kw("prepare_app", GLOBAL),
    kw("psgi_app", GLOBAL),
    deprecated_kw("push_header", ROUTE),
    kw("push_response_header", ROUTE),
    kw("put", GLOBAL),
    kw("redirect", ROUTE),
    kw("request", ROUTE),
    kw("request_data", ROUTE),
    kw("request_header", ROUTE),
    kw("response", ROUTE),
    kw("response_header", ROUTE),
    kw("response_headers", ROUTE),
    kw("runner", GLOBAL),
    kw("send_as", ROUTE),
    kw("send_error", ROUTE),
    kw("send_file", ROUTE),
    kw("session", ROUTE),
    kw("set", GLOBAL),
    kw("setting", GLOBAL),
    kw("splat", ROUTE),
    kw("start", GLOBAL),
    kw("status", ROUTE),
    kw("template", GLOBAL),
    kw("to_app", GLOBAL),
    kw("to_dumper", GLOBAL),
    kw("to_json", GLOBAL),
    kw("to_yaml", GLOBAL),
    kw("true", GLOBAL),
    kw("upload", ROUTE),
    kw("uri_for", ROUTE),
    kw("uri_for_route", ROUTE),
    kw("var", ROUTE),
    kw("vars", ROUTE),
    kw("warning", GLOBAL),
];

/// Application-name selection from the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppNameSelection {
    /// Application identity derives from the caller package.
    Default,
    /// Unambiguous literal `appname => 'Name'`.
    Literal(String),
    /// Computed or unsupported app identity — an explicit dynamic boundary.
    Dynamic {
        /// Why the application identity is dynamic.
        reason: String,
    },
}

/// DSL selection from the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslSelection {
    /// The reviewed default Dancer2 DSL contract.
    Default,
    /// Literal `dsl => 'Some::DSL'` with exact source/module evidence.
    CustomLiteral(String),
    /// Computed/configured DSL selection — an explicit dynamic boundary.
    Dynamic {
        /// Why the DSL selection is dynamic.
        reason: String,
    },
}

/// Import evidence extracted from the activating `use Dancer2 ...;` argument
/// list, in parser token form.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dancer2ImportEvidence {
    /// Application-name selection.
    pub appname: Option<AppNameSelection>,
    /// DSL selection.
    pub dsl: Option<DslSelection>,
    /// `!keyword` exclusions in source order.
    pub excluded_keywords: Vec<String>,
    /// Import options this adapter does not model; the activation carries an
    /// explicit boundary for them.
    pub unmodeled_options: Vec<String>,
}

/// Availability state of one keyword in the activating import.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dancer2KeywordState {
    /// Keyword is imported by this activation.
    Imported,
    /// Keyword is excluded by `!keyword`.
    Excluded,
}

/// One typed Dancer2 DSL keyword import fact.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2KeywordImportFact {
    /// DSL keyword name.
    pub keyword: String,
    /// Reviewed availability scope.
    pub scope: DslKeywordScope,
    /// Imported/excluded state for this activation.
    pub state: Dancer2KeywordState,
    /// Deprecated keywords are preserved as versioned metadata.
    pub deprecated: bool,
}

impl Dancer2KeywordImportFact {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(
        keyword: String,
        scope: DslKeywordScope,
        state: Dancer2KeywordState,
        deprecated: bool,
    ) -> Self {
        Self { keyword, scope, state, deprecated }
    }
}

/// Final activation state of one Dancer2 activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2ActivationState {
    /// Exact registry-backed activation.
    Exact {
        /// Application identity derived from caller package or literal appname.
        application_name: String,
        /// Observed supported framework version.
        framework_version: String,
        /// Source generation that produced the activation evidence.
        source_generation: SourceGeneration,
    },
    /// The site is an explicit dynamic boundary, not exact activation.
    DynamicBoundary {
        /// Bounded boundary explanation.
        reason: String,
    },
    /// The site did not activate under the registry contract.
    NotActivated {
        /// Bounded non-activation explanation.
        reason: String,
    },
}

/// Typed registry-backed Dancer2 activation facts for one activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2ActivationFacts {
    /// Activation state.
    pub state: Dancer2ActivationState,
    /// Effective DSL selection.
    pub dsl: DslSelection,
    /// Versioned identity of the keyword contract that produced `keywords`.
    pub dsl_contract_version: &'static str,
    /// Keyword import facts. Empty when a custom/dynamic DSL owns the keyword
    /// vocabulary (the default contract must not be inherited).
    pub keywords: Vec<Dancer2KeywordImportFact>,
    /// `!keyword` exclusions that do not name a reviewed DSL keyword.
    pub unknown_exclusions: Vec<String>,
}

impl Dancer2ActivationFacts {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(
        state: Dancer2ActivationState,
        dsl: DslSelection,
        dsl_contract_version: &'static str,
        keywords: Vec<Dancer2KeywordImportFact>,
        unknown_exclusions: Vec<String>,
    ) -> Self {
        Self { state, dsl, dsl_contract_version, keywords, unknown_exclusions }
    }
}

impl Dancer2ActivationFacts {
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self.state, Dancer2ActivationState::Exact { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_table_matches_reviewed_v1_1_registration() {
        // Independent expected tuples from upstream lib/Dancer2/Core/DSL.pm,
        // v1.1.1 commit 36bdd7aa4e9b97585ad545180a2135fe26278ad1 (82 exports).
        // v1.0.0 commit ab2a50a478f81b005d0a6f4f1ff2a1f6f9ce1aa5 has the
        // same 81 tuples after omitting uri_for_route. Scope is is_global;
        // deprecation is the four DEPRECATED runtime bodies. See #13089.
        let expected: &[(&str, DslKeywordScope, bool)] = &[
            ("any", GLOBAL, false),
            ("app", GLOBAL, false),
            ("captures", ROUTE, false),
            ("config", GLOBAL, false),
            ("content", ROUTE, false),
            ("content_type", ROUTE, false),
            ("context", ROUTE, true),
            ("cookie", ROUTE, false),
            ("cookies", ROUTE, false),
            ("dance", GLOBAL, false),
            ("dancer_app", GLOBAL, false),
            ("dancer_version", GLOBAL, false),
            ("dancer_major_version", GLOBAL, false),
            ("debug", GLOBAL, false),
            ("decode_json", GLOBAL, false),
            ("del", GLOBAL, false),
            ("delayed", ROUTE, false),
            ("dirname", GLOBAL, false),
            ("done", ROUTE, false),
            ("dsl", GLOBAL, false),
            ("encode_json", GLOBAL, false),
            ("engine", GLOBAL, false),
            ("error", GLOBAL, false),
            ("false", GLOBAL, false),
            ("flush", ROUTE, false),
            ("forward", ROUTE, false),
            ("from_dumper", GLOBAL, false),
            ("from_json", GLOBAL, false),
            ("from_yaml", GLOBAL, false),
            ("get", GLOBAL, false),
            ("halt", ROUTE, false),
            ("header", ROUTE, true),
            ("headers", ROUTE, true),
            ("hook", GLOBAL, false),
            ("info", GLOBAL, false),
            ("log", GLOBAL, false),
            ("mime", GLOBAL, false),
            ("options", GLOBAL, false),
            ("param", ROUTE, false),
            ("params", ROUTE, false),
            ("query_parameters", ROUTE, false),
            ("body_parameters", ROUTE, false),
            ("route_parameters", ROUTE, false),
            ("pass", ROUTE, false),
            ("patch", GLOBAL, false),
            ("path", GLOBAL, false),
            ("post", GLOBAL, false),
            ("prefix", GLOBAL, false),
            ("prepare_app", GLOBAL, false),
            ("psgi_app", GLOBAL, false),
            ("push_header", ROUTE, true),
            ("push_response_header", ROUTE, false),
            ("put", GLOBAL, false),
            ("redirect", ROUTE, false),
            ("request", ROUTE, false),
            ("request_data", ROUTE, false),
            ("request_header", ROUTE, false),
            ("response", ROUTE, false),
            ("response_header", ROUTE, false),
            ("response_headers", ROUTE, false),
            ("runner", GLOBAL, false),
            ("send_as", ROUTE, false),
            ("send_error", ROUTE, false),
            ("send_file", ROUTE, false),
            ("session", ROUTE, false),
            ("set", GLOBAL, false),
            ("setting", GLOBAL, false),
            ("splat", ROUTE, false),
            ("start", GLOBAL, false),
            ("status", ROUTE, false),
            ("template", GLOBAL, false),
            ("to_app", GLOBAL, false),
            ("to_dumper", GLOBAL, false),
            ("to_json", GLOBAL, false),
            ("to_yaml", GLOBAL, false),
            ("true", GLOBAL, false),
            ("upload", ROUTE, false),
            ("uri_for", ROUTE, false),
            ("uri_for_route", ROUTE, false),
            ("var", ROUTE, false),
            ("vars", ROUTE, false),
            ("warning", GLOBAL, false),
        ];
        let observed: Vec<_> = DANCER2_DSL_KEYWORDS
            .iter()
            .map(|keyword| (keyword.name, keyword.scope, keyword.deprecated))
            .collect();
        assert_eq!(observed.as_slice(), expected);
    }
}
