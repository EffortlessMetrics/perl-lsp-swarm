//! Shared dancer2 two x contracts and reference data.
//!
//! Concrete interpretation is owned by `perl_semantic_analyzer::framework_adapters::dancer2_two_x`.
//! This module preserves the fact vocabulary, stable identities, and reviewed tables.

use crate::SourceGeneration;
use crate::framework::AdapterId;
use crate::framework_adapters::dancer2::{AppNameSelection, DslKeywordScope, DslSelection};

/// Framework name handled by this adapter.
pub const DANCER2_TWO_X_FRAMEWORK_NAME: &str = "Dancer2";

/// Reviewed supported version range for the bounded Dancer2 2.x profile.
///
/// The bounded cells are pinned at 2.0.0/2.0.1/2.1.0 (byte-identical
/// evidence; no 2.0-vs-2.1 delta reaches the fact surface). The upper bound
/// forces re-review before any reviewed 2.2 identity is absorbed.
pub const DANCER2_TWO_X_VERSION_CONSTRAINT: &str = ">=2.0.0,<2.2.0";

/// Provisional adapter identity, distinct from the #8914 1.x adapter.
///
/// The generic registry (#6821) owns final identity assignment; this stable
/// value is reserved so shadow receipts remain comparable across the
/// registry extraction.
pub const DANCER2_TWO_X_ADAPTER_ID: AdapterId = AdapterId(0x0044_3243);

/// Versioned identity of the reviewed 2.x default-DSL keyword contract.
///
/// `2-0` is the pinned DSL generation (byte-identical 2.0.0 → 2.1.0); `v1`
/// is this contract's first reviewed revision (#13616 L2).
pub const DANCER2_TWO_X_DSL_CONTRACT_VERSION: &str = "dancer2-dsl.2-0.v1";

/// Reviewed versioned-descriptor schema revision for this adapter. Tracks
/// [`FRAMEWORK_ADAPTER_SCHEMA_VERSION`](crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION).
pub const DANCER2_TWO_X_DESCRIPTOR_REVISION: u32 =
    crate::framework::FRAMEWORK_ADAPTER_SCHEMA_VERSION;

/// Pinned upstream commit reviewed for Dancer2 2.0.0.
pub const DANCER2_TWO_X_PINNED_COMMIT_2_0_0: &str = "09f316678b8dd237d4c4ea0242e70e32591f5d64";

/// Pinned upstream commit reviewed for Dancer2 2.0.1.
pub const DANCER2_TWO_X_PINNED_COMMIT_2_0_1: &str = "674837ce095db3bffb5acfccd50fdac57771d50b";

/// Pinned upstream commit reviewed for Dancer2 2.1.0.
pub const DANCER2_TWO_X_PINNED_COMMIT_2_1_0: &str = "09e9376288ddc3571f226883eb99753fecce818d";

/// Total registered keywords in the pinned 2.x core-DSL registry.
pub const DANCER2_TWO_X_KEYWORD_TOTAL: usize = 82;

/// Keywords registered `is_global => 1` (callable anywhere the DSL is
/// imported) in the pinned 2.x registry.
pub const DANCER2_TWO_X_KEYWORD_GLOBAL: usize = 43;

/// Keywords registered `is_global => 0` (route-handler-only) in the pinned
/// 2.x registry.
pub const DANCER2_TWO_X_KEYWORD_ROUTE_ONLY: usize = 39;

/// The exact compile-time die the pinned `sub import` raises for an odd
/// final argument count.
pub const DANCER2_TWO_X_IMPORT_DIE_MESSAGE: &str =
    "parameters must be key/value pairs or '!keyword'";

/// Import tags the pinned `sub import` silently ignores (`:script`,
/// `:syntax`, `:tests`).
pub const DANCER2_TWO_X_NOOP_IMPORT_TAGS: [&str; 3] = [":script", ":syntax", ":tests"];

/// The import tag that skips the `strict`/`warnings`/`utf8` import into the
/// caller.
pub const DANCER2_TWO_X_NOPRAGMAS_TAG: &str = ":nopragmas";

/// One reviewed Dancer2 2.x default-DSL keyword, transcribed from the pinned
/// upstream `dsl_keywords` map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dancer2TwoXDslKeyword {
    /// DSL keyword name.
    pub name: &'static str,
    /// Availability scope in the pinned registry (`is_global`).
    pub scope: DslKeywordScope,
    /// Upstream prototype, applied by `_apply_prototype` only when defined.
    /// Exactly `delayed` (`&@`) and `prepare_app` (`&`) carry one.
    pub prototype: Option<&'static str>,
    /// Upstream replacement keyword named by the keyword body's
    /// `DEPRECATED:` runtime croak, when the body carries one. The registry
    /// itself never flags deprecation and the keyword is still exported.
    pub deprecation_replacement: Option<&'static str>,
}

impl Dancer2TwoXDslKeyword {
    /// Whether the keyword body croaks a `DEPRECATED:` message at runtime.
    #[must_use]
    pub fn is_deprecated(&self) -> bool {
        self.deprecation_replacement.is_some()
    }
}

const fn kw(name: &'static str, scope: DslKeywordScope) -> Dancer2TwoXDslKeyword {
    Dancer2TwoXDslKeyword { name, scope, prototype: None, deprecation_replacement: None }
}

const fn kw_proto(
    name: &'static str,
    scope: DslKeywordScope,
    prototype: &'static str,
) -> Dancer2TwoXDslKeyword {
    Dancer2TwoXDslKeyword { name, scope, prototype: Some(prototype), deprecation_replacement: None }
}

const fn kw_deprecated(
    name: &'static str,
    scope: DslKeywordScope,
    replacement: &'static str,
) -> Dancer2TwoXDslKeyword {
    Dancer2TwoXDslKeyword {
        name,
        scope,
        prototype: None,
        deprecation_replacement: Some(replacement),
    }
}

/// The pinned 2.x default-DSL keyword registry, in upstream `dsl_keywords`
/// map order.
///
/// Derived from `lib/Dancer2/Core/DSL.pm` at the pinned commits; the
/// checked-in oracle
/// `crates/perl-semantic-facts/tests/data/dancer2_two_x_dsl_registry_oracle.tsv`
/// is generated from the same upstream source and asserted equal by test.
pub const DANCER2_TWO_X_DSL_KEYWORDS: &[Dancer2TwoXDslKeyword] = &[
    kw("any", DslKeywordScope::Global),
    kw("app", DslKeywordScope::Global),
    kw("captures", DslKeywordScope::RouteHandlerOnly),
    kw("config", DslKeywordScope::Global),
    kw("content", DslKeywordScope::RouteHandlerOnly),
    kw("content_type", DslKeywordScope::RouteHandlerOnly),
    kw_deprecated("context", DslKeywordScope::RouteHandlerOnly, "app"),
    kw("cookie", DslKeywordScope::RouteHandlerOnly),
    kw("cookies", DslKeywordScope::RouteHandlerOnly),
    kw("dance", DslKeywordScope::Global),
    kw("dancer_app", DslKeywordScope::Global),
    kw("dancer_version", DslKeywordScope::Global),
    kw("dancer_major_version", DslKeywordScope::Global),
    kw("debug", DslKeywordScope::Global),
    kw("decode_json", DslKeywordScope::Global),
    kw("del", DslKeywordScope::Global),
    kw_proto("delayed", DslKeywordScope::RouteHandlerOnly, "&@"),
    kw("dirname", DslKeywordScope::Global),
    kw("done", DslKeywordScope::RouteHandlerOnly),
    kw("dsl", DslKeywordScope::Global),
    kw("encode_json", DslKeywordScope::Global),
    kw("engine", DslKeywordScope::Global),
    kw("error", DslKeywordScope::Global),
    kw("false", DslKeywordScope::Global),
    kw("flush", DslKeywordScope::RouteHandlerOnly),
    kw("forward", DslKeywordScope::RouteHandlerOnly),
    kw("from_dumper", DslKeywordScope::Global),
    kw("from_json", DslKeywordScope::Global),
    kw("from_yaml", DslKeywordScope::Global),
    kw("get", DslKeywordScope::Global),
    kw("halt", DslKeywordScope::RouteHandlerOnly),
    kw_deprecated("header", DslKeywordScope::RouteHandlerOnly, "response_header"),
    kw_deprecated("headers", DslKeywordScope::RouteHandlerOnly, "response_headers"),
    kw("hook", DslKeywordScope::Global),
    kw("info", DslKeywordScope::Global),
    kw("log", DslKeywordScope::Global),
    kw("mime", DslKeywordScope::Global),
    kw("options", DslKeywordScope::Global),
    kw("param", DslKeywordScope::RouteHandlerOnly),
    kw("params", DslKeywordScope::RouteHandlerOnly),
    kw("query_parameters", DslKeywordScope::RouteHandlerOnly),
    kw("body_parameters", DslKeywordScope::RouteHandlerOnly),
    kw("route_parameters", DslKeywordScope::RouteHandlerOnly),
    kw("pass", DslKeywordScope::RouteHandlerOnly),
    kw("patch", DslKeywordScope::Global),
    kw("path", DslKeywordScope::Global),
    kw("post", DslKeywordScope::Global),
    kw("prefix", DslKeywordScope::Global),
    kw_proto("prepare_app", DslKeywordScope::Global, "&"),
    kw("psgi_app", DslKeywordScope::Global),
    kw_deprecated("push_header", DslKeywordScope::RouteHandlerOnly, "push_response_header"),
    kw("push_response_header", DslKeywordScope::RouteHandlerOnly),
    kw("put", DslKeywordScope::Global),
    kw("redirect", DslKeywordScope::RouteHandlerOnly),
    kw("request", DslKeywordScope::RouteHandlerOnly),
    kw("request_data", DslKeywordScope::RouteHandlerOnly),
    kw("request_header", DslKeywordScope::RouteHandlerOnly),
    kw("response", DslKeywordScope::RouteHandlerOnly),
    kw("response_header", DslKeywordScope::RouteHandlerOnly),
    kw("response_headers", DslKeywordScope::RouteHandlerOnly),
    kw("runner", DslKeywordScope::Global),
    kw("send_as", DslKeywordScope::RouteHandlerOnly),
    kw("send_error", DslKeywordScope::RouteHandlerOnly),
    kw("send_file", DslKeywordScope::RouteHandlerOnly),
    kw("session", DslKeywordScope::RouteHandlerOnly),
    kw("set", DslKeywordScope::Global),
    kw("setting", DslKeywordScope::Global),
    kw("splat", DslKeywordScope::RouteHandlerOnly),
    kw("start", DslKeywordScope::Global),
    kw("status", DslKeywordScope::RouteHandlerOnly),
    kw("template", DslKeywordScope::Global),
    kw("to_app", DslKeywordScope::Global),
    kw("to_dumper", DslKeywordScope::Global),
    kw("to_json", DslKeywordScope::Global),
    kw("to_yaml", DslKeywordScope::Global),
    kw("true", DslKeywordScope::Global),
    kw("upload", DslKeywordScope::RouteHandlerOnly),
    kw("uri_for", DslKeywordScope::RouteHandlerOnly),
    kw("uri_for_route", DslKeywordScope::RouteHandlerOnly),
    kw("var", DslKeywordScope::RouteHandlerOnly),
    kw("vars", DslKeywordScope::RouteHandlerOnly),
    kw("warning", DslKeywordScope::Global),
];

/// Look up one registry keyword by name.
#[must_use]
pub fn dancer2_two_x_keyword(name: &str) -> Option<&'static Dancer2TwoXDslKeyword> {
    DANCER2_TWO_X_DSL_KEYWORDS.iter().find(|keyword| keyword.name == name)
}

/// Import-argument state of one keyword for one 2.x activation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dancer2TwoXKeywordState {
    /// The keyword is installed into the caller by the import.
    Imported,
    /// The keyword is excluded by an `!keyword` import argument, so the
    /// export map skips it and mints no binding.
    Excluded,
    /// A same-package named package subroutine already owns the name, so the
    /// upstream un-overwrite rule (`next if defined $existing`) skips the
    /// install and no DSL binding is minted for it.
    Shadowed,
}

/// One typed Dancer2 2.x DSL keyword fact for one activation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2TwoXKeywordFact {
    /// DSL keyword name.
    pub keyword: String,
    /// Reviewed availability scope (global vs route-handler-only).
    pub scope: DslKeywordScope,
    /// Upstream prototype when one is registered.
    pub prototype: Option<&'static str>,
    /// Upstream replacement named by the `DEPRECATED:` runtime croak, when
    /// the keyword body carries one.
    pub deprecation_replacement: Option<&'static str>,
    /// Import state for this activation.
    pub state: Dancer2TwoXKeywordState,
}

impl Dancer2TwoXKeywordFact {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(
        keyword: String,
        scope: DslKeywordScope,
        prototype: Option<&'static str>,
        deprecation_replacement: Option<&'static str>,
        state: Dancer2TwoXKeywordState,
    ) -> Self {
        Self { keyword, scope, prototype, deprecation_replacement, state }
    }
}

/// Import evidence extracted from one `use Dancer2 ...;` argument list under
/// the pinned 2.x `sub import` semantics.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dancer2TwoXImportEvidence {
    /// Application-name selection (`appname`; default is the caller).
    pub appname: Option<AppNameSelection>,
    /// DSL selection (`dsl`; default is `Dancer2::Core::DSL`).
    pub dsl: Option<DslSelection>,
    /// `!keyword` exclusions in source order.
    pub excluded_keywords: Vec<String>,
    /// Silent no-op tags observed (`:script`, `:syntax`, `:tests`).
    pub no_op_tags: Vec<String>,
    /// `:nopragmas` was observed: the pinned import skips importing
    /// `strict`/`warnings`/`utf8` into the caller.
    pub nopragmas: bool,
    /// Version-slot spellings the parser folded into the argument list
    /// (v-string form). Perl consumes the `use MODULE VERSION` slot before
    /// `import` runs, so these are import arguments to the parser but not to
    /// `import` (#14277 parser bound).
    pub version_slot_spellings: Vec<String>,
    /// Import arguments this profile does not model; the activation carries
    /// an explicit boundary for them instead of dropping them.
    pub unmodeled_options: Vec<String>,
    /// The pinned import's odd final argument count would die at compile
    /// time with [`DANCER2_TWO_X_IMPORT_DIE_MESSAGE`].
    pub odd_argument_count: bool,
    /// The statement spelled an explicit empty import list (`use Dancer2
    /// ();`), which calls no `import` at all: no app, no DSL, no pragmas.
    pub import_suppressed: bool,
}

/// Final activation state of one Dancer2 2.x activation site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dancer2TwoXActivationState {
    /// Exact registry-backed activation under the pinned 2.x contract.
    Exact {
        /// Application identity from literal `appname` or the caller package.
        application_name: String,
        /// Observed supported framework version.
        framework_version: String,
        /// Source generation that produced the activation evidence.
        source_generation: SourceGeneration,
    },
    /// The pinned import would die at compile time with the carried
    /// upstream message; no app is created and no keyword is exported.
    ImportDied {
        /// The exact upstream die text.
        die_message: String,
    },
    /// The site is an explicit dynamic boundary, not exact activation.
    DynamicBoundary {
        /// Bounded boundary explanation.
        reason: String,
    },
    /// The site did not activate under the reviewed 2.x contract.
    NotActivated {
        /// Bounded non-activation explanation.
        reason: String,
    },
}

/// Typed registry-backed Dancer2 2.x activation facts for one site.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dancer2TwoXActivationFacts {
    /// Activation state.
    pub state: Dancer2TwoXActivationState,
    /// Effective DSL selection.
    pub dsl: DslSelection,
    /// Versioned identity of the keyword contract that produced `keywords`.
    pub dsl_contract_version: &'static str,
    /// The pinned import skipped `strict`/`warnings`/`utf8` (`:nopragmas`).
    pub nopragmas: bool,
    /// Silent no-op tags the import observed.
    pub no_op_tags: Vec<String>,
    /// Keyword facts from the pinned default contract. Empty unless the DSL
    /// selection is the default and the activation is exact: a custom or
    /// dynamic DSL owns its own keyword vocabulary.
    pub keywords: Vec<Dancer2TwoXKeywordFact>,
    /// `!keyword` exclusions that do not name a registry keyword.
    pub unknown_exclusions: Vec<String>,
}

impl Dancer2TwoXActivationFacts {
    /// Assemble a record from the caller's established contract values.
    #[must_use]
    pub fn new(
        state: Dancer2TwoXActivationState,
        dsl: DslSelection,
        dsl_contract_version: &'static str,
        nopragmas: bool,
        no_op_tags: Vec<String>,
        keywords: Vec<Dancer2TwoXKeywordFact>,
        unknown_exclusions: Vec<String>,
    ) -> Self {
        Self {
            state,
            dsl,
            dsl_contract_version,
            nopragmas,
            no_op_tags,
            keywords,
            unknown_exclusions,
        }
    }
}

impl Dancer2TwoXActivationFacts {
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self.state, Dancer2TwoXActivationState::Exact { .. })
    }
}
