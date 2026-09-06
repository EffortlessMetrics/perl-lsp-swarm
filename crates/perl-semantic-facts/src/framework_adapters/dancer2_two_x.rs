//! Registry-backed Dancer2 2.x activation, import, and core-DSL registry
//! facts (#13616, leaves L1 + L2).
//!
//! This adapter is the bounded Dancer2 2.x core-DSL profile. It mirrors the
//! shape of the #8914 1.x adapter (`crate::framework_adapters::dancer2`) but
//! is derived from the pinned 2.x upstream sources, not inherited from the
//! 1.x contract:
//!
//! - **pinned evidence:** `lib/Dancer2/Core/DSL.pm` and `lib/Dancer2.pm` at
//!   Dancer2 `2.0.0` (`09f316678b8dd237d4c4ea0242e70e32591f5d64`), `2.0.1`
//!   (`674837ce095db3bffb5acfccd50fdac57771d50b`), and `2.1.0`
//!   (`09e9376288ddc3571f226883eb99753fecce818d`). The `dsl_keywords` map is
//!   byte-identical across the three pins; the `sub import` body is
//!   byte-identical to the 1.1.1 pin, so the import semantics modeled here
//!   are the reviewed 2.x import semantics.
//! - **L2 registry:** exactly 82 keywords — 43 `is_global => 1` (global) and
//!   39 route-handler-only — with prototypes only on `delayed` (`&@`) and
//!   `prepare_app` (`&`). No registry row is deprecated; deprecation lives in
//!   keyword *bodies* as runtime croaks (`header`, `headers`, `push_header`,
//!   `context`), carried here as the upstream replacement keyword.
//! - **repo correction (#13616):** the 1.x table
//!   ([`crate::framework_adapters::dancer2::DANCER2_DSL_KEYWORDS`])
//!   misclassifies `cookie` and `redirect` as global and carries
//!   `route`, `before`, `after`, and `body`, none of which are upstream DSL
//!   keywords. This 2.x registry is transcribed from the pinned upstream map
//!   instead: `cookie` and `redirect` are route-handler-only, and
//!   `route`/`before`/`after`/`body` are absent. The 1.x contract itself is
//!   owned by #13089/#13178 and is deliberately not mutated here.
//! - **L1 import semantics** model the pinned `sub import` exactly:
//!   `:script`/`:syntax`/`:tests` are silent no-ops; `:nopragmas` skips the
//!   `strict`/`warnings`/`utf8` import into the caller; `!keyword` arguments
//!   become `!keyword => 1` pairs; an odd final argument count dies at
//!   compile time with `parameters must be key/value pairs or '!keyword'`;
//!   no version check occurs inside `import` — version identity is the
//!   resolved module's `$Dancer2::VERSION`, exactly what the detection input
//!   models.
//!
//! ## Claim boundary
//!
//! Exact facts here mean *activation and the default core-DSL contract* are
//! current for the pinned 2.x slice (`>=2.0.0,<2.2.0`). Route declarations
//! (L3), the hook registry (L4), and multi-app identity (L5) are separate
//! leaves; Dancer2 2.x config, template/serializer engine, and plugin
//! keyword surfaces are blocked cells that this profile never proves. A
//! plugin-registered keyword therefore mints nothing here. Like its
//! neighbours this is a shadow adapter: comparison/receipt material only,
//! never publication authority.

use crate::framework::{
    AdapterDescriptor, AdapterDetectionInput, AdapterDetectionResult, AdapterDisposition,
    AdapterId, DetectionAbsenceReason, DetectionOutcome, ModuleSelectorEvaluation,
    ModuleSelectorOutcome, UnavailableReason,
};
use crate::framework_adapters::dancer2::{AppNameSelection, DslKeywordScope, DslSelection};
use crate::{Confidence, SourceGeneration};

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

/// Build the Dancer2 2.x adapter descriptor.
///
/// Shadow disposition: this adapter's facts are comparison-only and cannot
/// become publication authority (the SDK's authority validator refuses
/// non-production output by design).
#[must_use]
pub fn dancer2_two_x_descriptor() -> AdapterDescriptor {
    AdapterDescriptor::new(
        DANCER2_TWO_X_ADAPTER_ID,
        "dancer2-2x",
        DANCER2_TWO_X_FRAMEWORK_NAME,
        Some(DANCER2_TWO_X_VERSION_CONSTRAINT.to_string()),
        DANCER2_TWO_X_DESCRIPTOR_REVISION,
        AdapterDisposition::Shadow,
    )
}

/// Run the registry-backed Dancer2 2.x detection over one checked input.
///
/// Only the descriptor-owned `Dancer2` selector participates; a resolved
/// `Dancer2::Core` module never activates this adapter, and a resolved 1.x
/// identity fails the reviewed 2.x constraint explicitly. A foreign
/// descriptor (not this adapter's id) fails closed instead of detecting, a
/// pre-admission cancellation wins over any evidence, and matching selector
/// evaluations with contradictory terminal outcomes surface as `Conflicting`
/// rather than silently picking one.
#[must_use]
pub fn detect_dancer2_two_x(input: &AdapterDetectionInput) -> AdapterDetectionResult {
    let descriptor = &input.descriptor;
    if descriptor.adapter_id != DANCER2_TWO_X_ADAPTER_ID {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unsupported {
                reason: format!(
                    "foreign descriptor `{}` (adapter id {}) cannot drive the Dancer2 2.x detector",
                    descriptor.name, descriptor.adapter_id.0
                ),
            },
        );
    }
    if input.cancellation.is_cancelled {
        return AdapterDetectionResult::for_input(input, DetectionOutcome::Cancelled);
    }
    let matching: Vec<&ModuleSelectorEvaluation> = input
        .module_observation
        .evaluations
        .iter()
        .filter(|evaluation: &&ModuleSelectorEvaluation| {
            descriptor
                .required_module_selectors
                .iter()
                .any(|selector| selector == &evaluation.selector)
        })
        .collect();
    if matching.is_empty() {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
        );
    }
    let any_matched = matching
        .iter()
        .any(|evaluation| matches!(evaluation.outcome, ModuleSelectorOutcome::Matched { .. }));
    let any_absent = matching
        .iter()
        .any(|evaluation| matches!(evaluation.outcome, ModuleSelectorOutcome::Absent));
    if any_matched && any_absent {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![format!(
                    "{} matching selector evaluations disagree: both a matched and an absent \
                     terminal outcome were observed for {}",
                    matching.len(),
                    descriptor.required_module_selectors.join(", ")
                )],
            },
        );
    }
    let Some(evaluation) = matching
        .iter()
        .find(|evaluation| matches!(evaluation.outcome, ModuleSelectorOutcome::Matched { .. }))
        .or_else(|| matching.first())
    else {
        return AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
        );
    };
    match &evaluation.outcome {
        ModuleSelectorOutcome::Absent => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Absent { reason: DetectionAbsenceReason::RequiredModulesMissing },
        ),
        ModuleSelectorOutcome::Unresolved { .. } | ModuleSelectorOutcome::Unavailable { .. } => {
            AdapterDetectionResult::for_input(
                input,
                DetectionOutcome::Unavailable { reason: UnavailableReason::NoModulesAvailable },
            )
        }
        ModuleSelectorOutcome::Ambiguous { .. } => AdapterDetectionResult::for_input(
            input,
            DetectionOutcome::Conflicting {
                conflict_descriptions: vec![format!(
                    "selector `{}` matched more than one module identity",
                    evaluation.selector
                )],
            },
        ),
        ModuleSelectorOutcome::Matched { activation, evidence_class } => {
            let identity_confidence = evidence_class.confidence_ceiling();
            if identity_confidence != Confidence::High {
                // A module named Dancer2 without resolved supported identity
                // is not exact activation (the #8914 posture carried over).
                return AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: format!(
                            "Dancer2 selector matched with {identity_confidence:?} identity \
                             evidence; exact 2.x activation requires resolved module identity"
                        ),
                    },
                );
            }
            match &activation.observed_version {
                None => AdapterDetectionResult::for_input(
                    input,
                    DetectionOutcome::Unsupported {
                        reason: "Dancer2 activation lacks observed version evidence; the \
                                 reviewed 2.x version constraint cannot be checked"
                            .to_string(),
                    },
                ),
                Some(version) => {
                    match crate::framework::version_constraint_matches(
                        DANCER2_TWO_X_VERSION_CONSTRAINT,
                        &version.version,
                    ) {
                        Some(true) => {
                            let mut result = AdapterDetectionResult::for_input(
                                input,
                                DetectionOutcome::Detected {
                                    confidence: Confidence::High,
                                    framework_version: Some(version.version.clone()),
                                },
                            );
                            result = result.with_contributing_modules(vec![activation.clone()]);
                            result.with_version_evidence(version.clone())
                        }
                        Some(false) => {
                            let result = AdapterDetectionResult::for_input(
                                input,
                                DetectionOutcome::Absent {
                                    reason: DetectionAbsenceReason::VersionConstraintNotSatisfied,
                                },
                            );
                            result.with_version_evidence(version.clone())
                        }
                        // The observed version cannot be compared against the
                        // reviewed constraint; it stays explicitly unsupported.
                        None => AdapterDetectionResult::for_input(
                            input,
                            DetectionOutcome::Unsupported {
                                reason: format!(
                                    "observed Dancer2 version `{}` is not comparable with the \
                                     reviewed 2.x constraint `{DANCER2_TWO_X_VERSION_CONSTRAINT}`",
                                    version.version
                                ),
                            },
                        ),
                    }
                }
            }
        }
    }
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

/// Parse `use Dancer2` import arguments (parser token strings) into evidence
/// under the pinned `sub import` semantics.
///
/// Modeled in upstream order: each argument is first classified (no-op tags,
/// `:nopragmas`, `!keyword` pairing); the survivors form the final argument
/// list whose odd length dies; the even list is then walked as key/value
/// pairs (`appname`, `dsl`, exclusions). Computed values become explicit
/// dynamic selections, never defaults.
#[must_use]
pub fn parse_dancer2_two_x_import_args(args: &[String]) -> Dancer2TwoXImportEvidence {
    parse_dancer2_two_x_import_args_inner(args, true)
}

/// `version_slot` marks whether the first positional token may still be the
/// `use MODULE VERSION` requirement. A parenthesized import list
/// (`use Dancer2 (v2.01);`) delivers its contents as plain arguments — the
/// parens are consumed by the parser — so a leading v-string there is an
/// import argument, never a version slot.
fn parse_dancer2_two_x_import_args_inner(
    args: &[String],
    version_slot: bool,
) -> Dancer2TwoXImportEvidence {
    let mut evidence = Dancer2TwoXImportEvidence::default();
    // Pass 1 mirrors the pinned import loop: filter no-op tags and
    // `:nopragmas`, expand `!keyword` into `(!keyword, 1)` pairs, and collect
    // the final argument list.
    let mut final_args: Vec<ImportToken> = Vec::new();
    let mut positional_index = 0usize;
    for arg in args {
        for word in normalize_import_tokens(arg) {
            positional_index += 1;
            if DANCER2_TWO_X_NOOP_IMPORT_TAGS.contains(&word.text.as_str()) {
                evidence.no_op_tags.push(word.text);
                continue;
            }
            if word.text == DANCER2_TWO_X_NOPRAGMAS_TAG {
                evidence.nopragmas = true;
                continue;
            }
            // Perl consumes the `use MODULE VERSION` slot before `import`
            // runs; the parser reports a v-string spelling there as an
            // ordinary argument (#14277). Only an UNQUOTED first positional
            // token can occupy that slot — a quoted `'v2.01'` is a literal
            // string import argument, not a version (#14408 review).
            if word.quoting == TokenQuoting::Unquoted
                && version_slot
                && positional_index == 1
                && word.text.len() > 1
                && word.text.starts_with('v')
                && word.text[1..].chars().all(|c| c.is_ascii_digit() || c == '.' || c == '_')
            {
                evidence.version_slot_spellings.push(word.text);
                continue;
            }
            if word.text.starts_with('!') {
                // Upstream pairs every `!`-prefixed argument with a literal 1,
                // even a bare `!`.
                final_args.push(word);
                final_args.push(ImportToken::unquoted("1"));
                continue;
            }
            final_args.push(word);
        }
    }

    // The pinned arity check runs before anything else in the import body's
    // effectful half: an odd count dies at compile time.
    if final_args.len() % 2 == 1 {
        evidence.odd_argument_count = true;
        return evidence;
    }

    // Pass 2 walks the final list as key/value pairs, like `%final_args`.
    let mut index = 0;
    while index < final_args.len() {
        let key = &final_args[index];
        let value = final_args.get(index + 1);
        match (key.text.as_str(), value) {
            ("appname", Some(value)) => {
                evidence.appname = Some(option_value(&value.text, value.quoting).map_name());
            }
            ("dsl", Some(value)) => {
                evidence.dsl = Some(option_value(&value.text, value.quoting).map_dsl());
            }
            (key, _) if key.starts_with('!') => {
                push_exclusion(&mut evidence, key[1..].to_string());
            }
            (key, _) => {
                evidence.unmodeled_options.push(key.to_string());
            }
        }
        index += 2;
    }
    evidence
}

fn push_exclusion(evidence: &mut Dancer2TwoXImportEvidence, keyword: String) {
    if !keyword.is_empty() && !evidence.excluded_keywords.contains(&keyword) {
        evidence.excluded_keywords.push(keyword);
    }
}

enum OptionValue {
    Literal(String),
    Dynamic,
}

impl OptionValue {
    fn map_name(self) -> AppNameSelection {
        match self {
            OptionValue::Literal(value) => AppNameSelection::Literal(value),
            OptionValue::Dynamic => AppNameSelection::Dynamic {
                reason: "appname value is computed at runtime".to_string(),
            },
        }
    }

    fn map_dsl(self) -> DslSelection {
        match self {
            OptionValue::Literal(value) => DslSelection::CustomLiteral(value),
            OptionValue::Dynamic => {
                DslSelection::Dynamic { reason: "dsl value is computed at runtime".to_string() }
            }
        }
    }
}

fn option_value(token: &str, quoting: TokenQuoting) -> OptionValue {
    // `qw` words and single-quoted strings never interpolate: their sigils
    // are literal text (#14408 review).
    if matches!(quoting, TokenQuoting::Qw | TokenQuoting::Single) {
        return OptionValue::Literal(token.to_string());
    }
    let dynamic = if matches!(quoting, TokenQuoting::Double) {
        // Double quotes interpolate sigils anywhere in the text.
        token.contains('$') || token.contains('@') || token.contains('%') || token.contains('\\')
    } else {
        token.starts_with('$')
            || token.starts_with('@')
            || token.starts_with('%')
            || token.starts_with('\\')
            || token.contains('(')
    };
    if dynamic {
        return OptionValue::Dynamic;
    }
    // Bareword and pre-unquoted values are literal in the reviewed forms.
    OptionValue::Literal(token.to_string())
}

/// Closing delimiter for a Perl quote-like opening delimiter: the four
/// nested pairs, plus any other punctuation used symmetrically (`/`, `|`,
/// `!`, `#`, `,`...).
fn closing_delimiter(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '<' => Some('>'),
        c if !c.is_alphanumeric() && !c.is_whitespace() => Some(c),
        _ => None,
    }
}

/// Extract the body of a delimited quote-like construct (`qw//`, `q{}`,
/// `qq()`...). Returns the inner text, the opening delimiter, and whatever
/// follows the closing delimiter. Nested identical opens increase depth; a
/// symmetric delimiter is its own close.
fn split_delimited_quote_like<'a>(token: &'a str, kind: &str) -> Option<(String, char, &'a str)> {
    let rest = token.strip_prefix(kind)?;
    let mut characters = rest.chars();
    let open = characters.next()?;
    let close = closing_delimiter(open)?;
    let symmetric = open == close;
    let after_open = rest.strip_prefix(open)?;
    let mut depth = 0usize;
    let mut characters = after_open.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        // An escaped character (including an escaped delimiter or backslash)
        // is literal inside the quoted construct.
        if character == '\\' {
            characters.next();
            continue;
        }
        if character == open && !symmetric {
            depth += 1;
            continue;
        }
        if character == close {
            if depth == 0 {
                let inner = &after_open[..index];
                let rest = &after_open[index + close.len_utf8()..];
                return Some((inner.to_string(), open, rest));
            }
            depth -= 1;
        }
    }
    None
}

/// How a token reached the import list, which decides interpolation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TokenQuoting {
    /// Bare source token: sigils interpolate, and only an unquoted token can
    /// occupy the `use MODULE VERSION` slot.
    Unquoted,
    /// `'...'` and `q...`: Perl single quotes never interpolate.
    Single,
    /// `"..."` and `qq...`: Perl double quotes interpolate sigils.
    Double,
    /// A `qw//` word: never interpolates and can never be a version slot.
    Qw,
}

#[derive(Clone, Debug)]
struct ImportToken {
    text: String,
    quoting: TokenQuoting,
}

impl ImportToken {
    fn unquoted(text: impl Into<String>) -> Self {
        Self { text: text.into(), quoting: TokenQuoting::Unquoted }
    }
}

/// The words of a `qw//`-style token, or `None` when the token is not a
/// qw construct. Exposed for the analyzer's import-shadow analysis.
pub fn qw_words(token: &str) -> Option<Vec<String>> {
    let (inner, _open, _rest) = split_delimited_quote_like(token, "qw")?;
    Some(inner.split_whitespace().map(ToString::to_string).collect())
}

fn normalize_import_tokens(arg: &str) -> Vec<ImportToken> {
    let token = arg.trim();
    if token.is_empty() || matches!(token, "," | "=>" | "(" | ")" | ";") {
        return Vec::new();
    }
    // `qw(...)` arrives as one parser token; upstream receives its words as
    // individual import arguments. Every legal delimiter pair (and any
    // symmetric punctuation) is honored — qw words never interpolate and can
    // never occupy the VERSION slot.
    if let Some((inner, _open, _rest)) = split_delimited_quote_like(token, "qw") {
        return inner
            .split_whitespace()
            .map(|word| ImportToken { text: word.to_string(), quoting: TokenQuoting::Qw })
            .collect();
    }
    // Quote-like `q{}`/`qq()` forms: `q` never interpolates, `qq` does. A
    // trailing `=> value` in the same parser token re-normalizes.
    for (kind, quoting) in [("q", TokenQuoting::Single), ("qq", TokenQuoting::Double)] {
        if let Some((inner, _open, rest)) = split_delimited_quote_like(token, kind) {
            let mut tokens = vec![ImportToken { text: inner, quoting }];
            tokens.extend(normalize_import_tokens(rest));
            return tokens;
        }
    }
    // Unwrap one level of quoting: `'!get'` reaches `import` as the string
    // `!get`, whose first character is `!` at the pairing check.
    let unquoted = token
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .map(|inner| (inner, TokenQuoting::Single))
        .or_else(|| {
            token
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .map(|inner| (inner, TokenQuoting::Double))
        });
    match unquoted {
        Some((inner, quoting)) => vec![ImportToken { text: inner.to_string(), quoting }],
        None => vec![ImportToken::unquoted(token.to_string())],
    }
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
    /// Whether this activation is exact (registry-authoritative shape).
    #[must_use]
    pub fn is_exact(&self) -> bool {
        matches!(self.state, Dancer2TwoXActivationState::Exact { .. })
    }
}

/// Derive application identity for an admitted 2.x activation.
///
/// Dancer2's own app-name assignment treats a falsy literal (`''`, `0`,
/// `'0'`) as no name at all and falls back to the caller package, so the
/// facts do the same instead of recording an exact identity the runtime
/// would replace.
#[must_use]
pub fn dancer2_two_x_application_identity(
    package_name: Option<&str>,
    appname: Option<&AppNameSelection>,
) -> AppNameSelection {
    match appname {
        Some(selection @ AppNameSelection::Literal(value)) if !value.is_empty() && value != "0" => {
            selection.clone()
        }
        Some(selection @ AppNameSelection::Dynamic { .. }) => selection.clone(),
        Some(AppNameSelection::Default) | None | Some(AppNameSelection::Literal(_)) => {
            match package_name {
                Some(package) if !package.trim().is_empty() => AppNameSelection::Default,
                _ => AppNameSelection::Dynamic {
                    reason: "activating package name is unavailable".to_string(),
                },
            }
        }
    }
}

/// Build typed 2.x activation facts from one detection result plus import
/// evidence and the same-package keywords already owned by named subs.
///
/// Fail-closed ordering (each step returns without consulting later ones):
///
/// 1. an explicit empty import list never calls `import` — nothing activates;
/// 2. the detection outcome gates everything: only a resolved, supported 2.x
///    identity can carry facts;
/// 3. the pinned odd-argument die dominates any other import effect;
/// 4. dynamic boundaries (computed appname/DSL, unmodeled options) refuse
///    exactness explicitly;
/// 5. exact activation mints the pinned keyword contract last.
#[must_use]
pub fn dancer2_two_x_activation_facts(
    detection: &AdapterDetectionResult,
    package_name: Option<&str>,
    evidence: &Dancer2TwoXImportEvidence,
    shadowed_keywords: &[String],
) -> Dancer2TwoXActivationFacts {
    let dsl = evidence.dsl.clone().unwrap_or(DslSelection::Default);
    let base = |state, keywords| Dancer2TwoXActivationFacts {
        state,
        dsl: dsl.clone(),
        dsl_contract_version: DANCER2_TWO_X_DSL_CONTRACT_VERSION,
        nopragmas: evidence.nopragmas,
        no_op_tags: evidence.no_op_tags.clone(),
        keywords,
        unknown_exclusions: unknown_exclusions(evidence),
    };

    // Step 1: `use Dancer2 ();` never calls import.
    if evidence.import_suppressed {
        return base(
            Dancer2TwoXActivationState::NotActivated {
                reason: "explicit empty import list `()` calls no import; no app and no DSL \
                         is installed"
                    .to_string(),
            },
            Vec::new(),
        );
    }

    // Step 2: detection gates everything.
    let (framework_version, source_generation) = match &detection.outcome {
        DetectionOutcome::Detected { framework_version, .. } => {
            (framework_version.clone().unwrap_or_default(), detection.project_generation.clone())
        }
        outcome => {
            return base(
                Dancer2TwoXActivationState::NotActivated {
                    reason: format!("detection outcome is {outcome:?}"),
                },
                Vec::new(),
            );
        }
    };

    // Step 3: the pinned arity die precedes every effectful import step.
    if evidence.odd_argument_count {
        return base(
            Dancer2TwoXActivationState::ImportDied {
                die_message: DANCER2_TWO_X_IMPORT_DIE_MESSAGE.to_string(),
            },
            Vec::new(),
        );
    }

    // Step 4: dynamic boundaries refuse exactness explicitly.
    let appname = dancer2_two_x_application_identity(package_name, evidence.appname.as_ref());
    if let Some(reason) = dynamic_boundary_reason(&appname, &dsl, evidence) {
        return base(Dancer2TwoXActivationState::DynamicBoundary { reason }, Vec::new());
    }

    // Step 5: exact activation carries the pinned keyword contract.
    let application_name = match &appname {
        AppNameSelection::Literal(name) => name.clone(),
        _ => package_name.map(ToOwned::to_owned).unwrap_or_default(),
    };
    let keywords = default_keyword_facts(evidence, shadowed_keywords);
    base(
        Dancer2TwoXActivationState::Exact {
            application_name,
            framework_version,
            source_generation,
        },
        keywords,
    )
}

fn dynamic_boundary_reason(
    appname: &AppNameSelection,
    dsl: &DslSelection,
    evidence: &Dancer2TwoXImportEvidence,
) -> Option<String> {
    if let AppNameSelection::Dynamic { reason } = appname {
        return Some(format!("application identity is a dynamic boundary: {reason}"));
    }
    match dsl {
        DslSelection::Dynamic { reason } => {
            Some(format!("DSL selection is a dynamic boundary: {reason}"))
        }
        DslSelection::CustomLiteral(name) => Some(format!(
            "custom DSL `{name}` owns its keyword vocabulary; the pinned default contract \
             is not inherited"
        )),
        DslSelection::Default => {
            if evidence.unmodeled_options.is_empty() {
                None
            } else {
                Some(format!(
                    "import carries unmodeled options: {}",
                    evidence.unmodeled_options.join(", ")
                ))
            }
        }
    }
}

fn default_keyword_facts(
    evidence: &Dancer2TwoXImportEvidence,
    shadowed_keywords: &[String],
) -> Vec<Dancer2TwoXKeywordFact> {
    DANCER2_TWO_X_DSL_KEYWORDS
        .iter()
        .map(|keyword| {
            let state =
                if evidence.excluded_keywords.iter().any(|excluded| excluded == keyword.name) {
                    Dancer2TwoXKeywordState::Excluded
                } else if shadowed_keywords.iter().any(|shadowed| shadowed == keyword.name) {
                    Dancer2TwoXKeywordState::Shadowed
                } else {
                    Dancer2TwoXKeywordState::Imported
                };
            Dancer2TwoXKeywordFact {
                keyword: keyword.name.to_string(),
                scope: keyword.scope,
                prototype: keyword.prototype,
                deprecation_replacement: keyword.deprecation_replacement,
                state,
            }
        })
        .collect()
}

fn unknown_exclusions(evidence: &Dancer2TwoXImportEvidence) -> Vec<String> {
    evidence
        .excluded_keywords
        .iter()
        .filter(|excluded| {
            !DANCER2_TWO_X_DSL_KEYWORDS.iter().any(|keyword| &keyword.name == excluded)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_test_must::must_some_with;

    #[test]
    fn descriptor_is_dancer2_selective_shadow_with_two_x_constraint() {
        let descriptor = dancer2_two_x_descriptor();
        assert_eq!(descriptor.required_module_selectors, vec!["Dancer2"]);
        assert_eq!(descriptor.disposition, AdapterDisposition::Shadow);
        assert_eq!(descriptor.adapter_id, DANCER2_TWO_X_ADAPTER_ID);
        assert_ne!(descriptor.adapter_id, crate::framework_adapters::dancer2::DANCER2_ADAPTER_ID);
        assert_eq!(
            descriptor.framework_version_constraint.as_deref(),
            Some(DANCER2_TWO_X_VERSION_CONSTRAINT)
        );
    }

    #[test]
    fn registry_matches_pinned_counts_and_scope_split() {
        assert_eq!(DANCER2_TWO_X_DSL_KEYWORDS.len(), DANCER2_TWO_X_KEYWORD_TOTAL);
        let global = DANCER2_TWO_X_DSL_KEYWORDS
            .iter()
            .filter(|keyword| keyword.scope == DslKeywordScope::Global)
            .count();
        let route_only = DANCER2_TWO_X_DSL_KEYWORDS
            .iter()
            .filter(|keyword| keyword.scope == DslKeywordScope::RouteHandlerOnly)
            .count();
        assert_eq!(global, DANCER2_TWO_X_KEYWORD_GLOBAL);
        assert_eq!(route_only, DANCER2_TWO_X_KEYWORD_ROUTE_ONLY);
        assert_eq!(global + route_only, DANCER2_TWO_X_KEYWORD_TOTAL);
        // Registry entries are unique.
        let mut names: Vec<_> = DANCER2_TWO_X_DSL_KEYWORDS.iter().map(|k| k.name).collect();
        names.sort_unstable();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len);
    }

    #[test]
    fn prototypes_are_exactly_delayed_and_prepare_app() {
        let prototyped: Vec<_> = DANCER2_TWO_X_DSL_KEYWORDS
            .iter()
            .filter_map(|keyword| keyword.prototype.map(|proto| (keyword.name, proto)))
            .collect();
        assert_eq!(prototyped, vec![("delayed", "&@"), ("prepare_app", "&")]);
    }

    #[test]
    fn deprecations_are_the_upstream_runtime_croaks() {
        let deprecated: Vec<_> = DANCER2_TWO_X_DSL_KEYWORDS
            .iter()
            .filter_map(|keyword| {
                keyword.deprecation_replacement.map(|replacement| (keyword.name, replacement))
            })
            .collect();
        assert_eq!(
            deprecated,
            vec![
                ("context", "app"),
                ("header", "response_header"),
                ("headers", "response_headers"),
                ("push_header", "push_response_header"),
            ]
        );
        // Every named replacement is itself a registered keyword.
        for keyword in DANCER2_TWO_X_DSL_KEYWORDS {
            if let Some(replacement) = keyword.deprecation_replacement {
                assert!(
                    dancer2_two_x_keyword(replacement).is_some(),
                    "`{}` deprecation names unregistered replacement `{replacement}`",
                    keyword.name
                );
            }
        }
    }

    #[test]
    fn repo_correction_cookie_and_redirect_are_route_handler_only() {
        let cookie = must_some_with(dancer2_two_x_keyword("cookie"), "cookie must be registered");
        let redirect =
            must_some_with(dancer2_two_x_keyword("redirect"), "redirect must be registered");
        assert_eq!(cookie.scope, DslKeywordScope::RouteHandlerOnly);
        assert_eq!(redirect.scope, DslKeywordScope::RouteHandlerOnly);
    }

    #[test]
    fn repo_correction_non_upstream_keywords_are_absent() {
        for absent in ["route", "before", "after", "body"] {
            assert!(
                dancer2_two_x_keyword(absent).is_none(),
                "`{absent}` is not an upstream DSL keyword and must stay absent"
            );
        }
    }

    #[test]
    fn noop_tags_are_silent_and_recorded() {
        let args: Vec<String> =
            [":script", ":syntax", ":tests"].iter().map(|tag| format!("'{tag}'")).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert_eq!(evidence.no_op_tags, vec![":script", ":syntax", ":tests"]);
        assert!(!evidence.odd_argument_count);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn nopragmas_sets_the_pragma_boundary() {
        let args: Vec<String> = vec![":nopragmas".to_string()];
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert!(evidence.nopragmas);
        assert!(!evidence.odd_argument_count);
    }

    #[test]
    fn exclusions_are_keyword_scoped_and_deduped() {
        let args: Vec<String> =
            ["'!params'", "qw(!get !params)"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert_eq!(evidence.excluded_keywords, vec!["params", "get"]);
        assert!(!evidence.odd_argument_count);
    }

    #[test]
    fn odd_argument_count_dies_with_the_upstream_message() {
        // A lone key with no value is odd: `use Dancer2 'appname';`
        let args: Vec<String> = vec!["'appname'".to_string()];
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert!(evidence.odd_argument_count);

        // No-op tags are filtered before the arity check.
        let args: Vec<String> =
            ["':script'", "'appname'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert!(evidence.odd_argument_count);

        // A bare `!` pairs with 1 upstream, so `'!' 'post'` is still odd.
        let args: Vec<String> = ["'!'", "'post'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert!(evidence.odd_argument_count);
        assert_eq!(evidence.excluded_keywords, Vec::<String>::new());
    }

    #[test]
    fn even_lists_survive_the_arity_check() {
        let cases: Vec<Vec<String>> = vec![
            vec![],
            vec!["'!get'".to_string()],
            vec!["appname".to_string(), "=>".to_string(), "'Named'".to_string()],
            vec![
                "appname".to_string(),
                "=>".to_string(),
                "'Named'".to_string(),
                "'!get'".to_string(),
            ],
        ];
        for args in cases {
            let evidence = parse_dancer2_two_x_import_args(&args);
            assert!(!evidence.odd_argument_count, "args {args:?} must survive");
        }
    }

    #[test]
    fn bareword_without_a_value_dies_like_upstream() {
        // `use Dancer2 appname => 'A', skip_check;` hands `import` three
        // items (`=>` is list syntax and never reaches it): the pinned arity
        // check dies. The 1.x evidence parser models this spelling as an
        // unmodeled option instead — the pinned import semantics are
        // deliberately stricter.
        let args: Vec<String> =
            ["appname", "=>", "'A'", "skip_check"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert!(evidence.odd_argument_count);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn appname_and_dsl_forms_are_represented() {
        let literal: Vec<String> =
            ["appname", "=>", "'Named'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&literal);
        assert_eq!(evidence.appname, Some(AppNameSelection::Literal("Named".to_string())));

        let computed: Vec<String> =
            ["appname", "=>", "$name"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&computed);
        assert!(
            matches!(evidence.appname, Some(AppNameSelection::Dynamic { .. })),
            "computed appname must not silently become Default"
        );

        let custom: Vec<String> =
            ["dsl", "=>", "'My::DSL'"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&custom);
        assert_eq!(evidence.dsl, Some(DslSelection::CustomLiteral("My::DSL".to_string())));
    }

    #[test]
    fn unknown_options_are_recorded_not_dropped() {
        // `use Dancer2 skip_check => 1;` — an even, well-formed list whose
        // key the import ignores.
        let args: Vec<String> = ["skip_check", "=>", "1"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert_eq!(evidence.unmodeled_options, vec!["skip_check".to_string()]);
        assert!(!evidence.odd_argument_count);
    }

    #[test]
    fn vstring_version_slot_is_not_an_import_argument() {
        // `use Dancer2 v2.01;` — perl consumes the VERSION slot before
        // import runs; the parser reports it in the argument list.
        let args: Vec<String> = vec!["v2.01".to_string()];
        let evidence = parse_dancer2_two_x_import_args(&args);
        assert_eq!(evidence.version_slot_spellings, vec!["v2.01".to_string()]);
        assert!(!evidence.odd_argument_count);
        assert!(evidence.unmodeled_options.is_empty());
    }

    #[test]
    fn suppressed_import_is_detectable_on_evidence() {
        let mut evidence = parse_dancer2_two_x_import_args(&[]);
        assert!(!evidence.import_suppressed);
        evidence.import_suppressed = true;
        assert!(evidence.import_suppressed);
    }

    fn matched_two_x(
        version: Option<&str>,
        generation: &str,
        evidence: crate::framework::DetectionEvidenceClass,
    ) -> ModuleSelectorEvaluation {
        use crate::framework::{ModuleActivationIdentity, ModuleVersionEvidence};
        let activation = ModuleActivationIdentity::new(
            "Dancer2",
            Some(crate::FileId(7)),
            SourceGeneration::known(generation),
        );
        let activation = match version {
            Some(version) => activation.with_observed_version(ModuleVersionEvidence::new(
                version,
                SourceGeneration::known(generation),
            )),
            None => activation,
        };
        ModuleSelectorEvaluation::new(
            "Dancer2",
            ModuleSelectorOutcome::Matched { activation, evidence_class: evidence },
        )
    }

    fn detection_input(
        evaluations: Vec<ModuleSelectorEvaluation>,
        generation: &str,
    ) -> AdapterDetectionInput {
        use crate::framework::{AdapterCancellation, ModuleObservationReceipt};
        let observation = ModuleObservationReceipt::new(
            "module-resolver.v1",
            "root:fixture",
            "project-environment.v1",
            SourceGeneration::known(generation),
            "sha256:fixture-input",
            evaluations,
        );
        AdapterDetectionInput::new(
            dancer2_two_x_descriptor(),
            observation,
            None,
            AdapterCancellation::active(),
        )
    }

    fn absent_selector(selector: &str, _generation: &str) -> ModuleSelectorEvaluation {
        ModuleSelectorEvaluation::new(selector.to_string(), ModuleSelectorOutcome::Absent)
    }

    fn detected() -> AdapterDetectionResult {
        detect_dancer2_two_x(&detection_input(
            vec![matched_two_x(
                Some("2.0.1"),
                "gen-1",
                crate::framework::DetectionEvidenceClass::ResolvedModule,
            )],
            "gen-1",
        ))
    }

    #[test]
    fn detection_accepts_pinned_two_x_and_rejects_one_x() {
        use crate::framework::DetectionEvidenceClass;
        let detected_result = detected();
        assert_eq!(
            detected_result.outcome,
            DetectionOutcome::Detected {
                confidence: Confidence::High,
                framework_version: Some("2.0.1".to_string()),
            }
        );
        // The 1.x identity fails the reviewed 2.x constraint explicitly.
        let one_x = detect_dancer2_two_x(&detection_input(
            vec![matched_two_x(Some("1.1.1"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
            "gen-1",
        ));
        assert_eq!(
            one_x.outcome,
            DetectionOutcome::Absent {
                reason: DetectionAbsenceReason::VersionConstraintNotSatisfied
            }
        );
        // The 2.1.0 pin satisfies the same bounded contract.
        let two_one = detect_dancer2_two_x(&detection_input(
            vec![matched_two_x(Some("2.1.0"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
            "gen-1",
        ));
        assert!(two_one.is_detected());
        // The upper bound forces re-review at 2.2.
        let two_two = detect_dancer2_two_x(&detection_input(
            vec![matched_two_x(Some("2.2.0"), "gen-1", DetectionEvidenceClass::ResolvedModule)],
            "gen-1",
        ));
        assert!(matches!(two_two.outcome, DetectionOutcome::Absent { .. }));
    }

    #[test]
    fn facts_suppressed_import_activates_nothing() {
        let mut evidence = parse_dancer2_two_x_import_args(&[]);
        evidence.import_suppressed = true;
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(!facts.is_exact());
        assert!(
            matches!(facts.state, Dancer2TwoXActivationState::NotActivated { .. }),
            "suppressed import must not activate, got {:?}",
            facts.state
        );
        assert!(facts.keywords.is_empty());
    }

    #[test]
    fn facts_name_only_identity_stays_not_activated() {
        use crate::framework::DetectionEvidenceClass;
        let detection = detect_dancer2_two_x(&detection_input(
            vec![matched_two_x(Some("2.0.1"), "gen-1", DetectionEvidenceClass::NameOnly)],
            "gen-1",
        ));
        let evidence = parse_dancer2_two_x_import_args(&[]);
        let facts = dancer2_two_x_activation_facts(&detection, Some("App"), &evidence, &[]);
        assert!(!facts.is_exact());
        assert!(matches!(facts.state, Dancer2TwoXActivationState::NotActivated { .. }));
    }

    #[test]
    fn facts_odd_args_die_before_any_keyword_is_minted() {
        let evidence = parse_dancer2_two_x_import_args(&["'appname'".to_string()]);
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(
            matches!(&facts.state,
                Dancer2TwoXActivationState::ImportDied { die_message }
                if die_message == DANCER2_TWO_X_IMPORT_DIE_MESSAGE),
            "expected ImportDied with the upstream message, got {:?}",
            facts.state
        );
        assert!(facts.keywords.is_empty());
    }

    #[test]
    fn facts_unmodeled_options_are_a_dynamic_boundary() {
        let args: Vec<String> = ["skip_check", "=>", "1"].iter().map(ToString::to_string).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(matches!(facts.state, Dancer2TwoXActivationState::DynamicBoundary { .. }));
        assert!(facts.keywords.is_empty());
    }

    #[test]
    fn facts_exact_activation_carries_the_pinned_contract() {
        let evidence = parse_dancer2_two_x_import_args(&[]);
        let facts = dancer2_two_x_activation_facts(&detected(), Some("MyApp"), &evidence, &[]);
        assert!(facts.is_exact());
        assert!(
            matches!(&facts.state,
                Dancer2TwoXActivationState::Exact { application_name, framework_version, .. }
                if application_name == "MyApp" && framework_version == "2.0.1"),
            "expected exact activation, got {:?}",
            facts.state
        );
        assert_eq!(facts.dsl_contract_version, DANCER2_TWO_X_DSL_CONTRACT_VERSION);
        assert_eq!(facts.keywords.len(), DANCER2_TWO_X_KEYWORD_TOTAL);
        assert!(
            facts.keywords.iter().all(|fact| fact.state == Dancer2TwoXKeywordState::Imported),
            "a bare import imports every keyword"
        );
    }

    #[test]
    fn facts_exclusion_and_unknown_exclusion_are_separated() {
        let evidence = parse_dancer2_two_x_import_args(
            &["'!params'", "'!nonexistent'"]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<String>>(),
        );
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(facts.is_exact());
        let params = facts.keywords.iter().find(|fact| fact.keyword == "params");
        assert!(matches!(params, Some(fact) if fact.state == Dancer2TwoXKeywordState::Excluded));
        assert_eq!(facts.unknown_exclusions, vec!["nonexistent".to_string()]);
    }

    #[test]
    fn facts_shadowed_keywords_never_mint_a_binding() {
        let evidence = parse_dancer2_two_x_import_args(&[]);
        let facts = dancer2_two_x_activation_facts(
            &detected(),
            Some("App"),
            &evidence,
            &["template".to_string(), "get".to_string()],
        );
        assert!(facts.is_exact(), "the upstream import still succeeds");
        let template = facts.keywords.iter().find(|fact| fact.keyword == "template");
        assert!(matches!(template, Some(fact) if fact.state == Dancer2TwoXKeywordState::Shadowed));
        let get = facts.keywords.iter().find(|fact| fact.keyword == "get");
        assert!(matches!(get, Some(fact) if fact.state == Dancer2TwoXKeywordState::Shadowed));
        let post = facts.keywords.iter().find(|fact| fact.keyword == "post");
        assert!(matches!(post, Some(fact) if fact.state == Dancer2TwoXKeywordState::Imported));
    }

    #[test]
    fn facts_custom_dsl_never_inherits_the_default_contract() {
        let evidence = parse_dancer2_two_x_import_args(
            &["dsl", "=>", "'My::DSL'"].iter().map(ToString::to_string).collect::<Vec<String>>(),
        );
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(matches!(facts.state, Dancer2TwoXActivationState::DynamicBoundary { .. }));
        assert!(facts.keywords.is_empty());
    }

    #[test]
    fn facts_carry_nopragmas_and_no_op_tags() {
        let args: Vec<String> =
            [":script", ":nopragmas"].iter().map(|tag| format!("'{tag}'")).collect();
        let evidence = parse_dancer2_two_x_import_args(&args);
        let facts = dancer2_two_x_activation_facts(&detected(), Some("App"), &evidence, &[]);
        assert!(facts.is_exact());
        assert!(facts.nopragmas);
        assert_eq!(facts.no_op_tags, vec![":script"]);
    }

    #[test]
    fn application_identity_defaults_to_the_caller_package() {
        let identity = dancer2_two_x_application_identity(Some("Pkg"), None);
        assert_eq!(identity, AppNameSelection::Default);
        let identity = dancer2_two_x_application_identity(None, None);
        assert!(matches!(identity, AppNameSelection::Dynamic { .. }));
    }

    #[test]
    fn escaped_delimiter_does_not_truncate_a_q_string_argument() {
        // `q{...}` is ONE string argument: word-splitting never applies. The
        // escaped close stays literal text, so the exclusion is the whole
        // inner string minus the leading `!` — not a truncation at `\\}`.
        let evidence = parse_dancer2_two_x_import_args(&["q{!get \\} !post}".to_string()]);
        assert_eq!(
            evidence.excluded_keywords,
            vec!["get \\} !post".to_string()],
            "the escaped close must stay literal text inside the single string argument"
        );
        // The single-word form remains an exclusion.
        let evidence = parse_dancer2_two_x_import_args(&["q{!get}".to_string()]);
        assert!(evidence.excluded_keywords.contains(&"get".to_string()));
    }
    #[test]
    fn falsy_appnames_fall_back_to_the_caller_package() {
        // Dancer2's own assignment treats '' / 0 / '0' as no name and uses
        // the caller package — the facts must not record an exact identity
        // the runtime would replace (#14408 review).
        for falsy in ["''", "0", "'0'"] {
            let evidence =
                parse_dancer2_two_x_import_args(&["appname".to_string(), falsy.to_string()]);
            let identity =
                dancer2_two_x_application_identity(Some("My::App"), evidence.appname.as_ref());
            assert_eq!(
                identity,
                AppNameSelection::Default,
                "falsy appname {falsy} must fall back to the caller package, got {identity:?}"
            );
        }
    }

    // --- #14408 review round: quoting, delimiters, and version slots -------

    #[test]
    fn qw_honors_every_legal_delimiter() {
        for spelling in [
            "qw(get post put delete)",
            "qw{get post put delete}",
            "qw[get post put delete]",
            "qw<get post put delete>",
            "qw/get post put delete/",
            "qw|get post put delete|",
            "qw!get post put delete!",
            "qw#get post put delete#",
        ] {
            let evidence = parse_dancer2_two_x_import_args(&[spelling.to_string()]);
            // Bare words pair as unmodeled options; the delimiter proof is
            // that the exact words came out in order for every delimiter.
            assert_eq!(
                evidence.unmodeled_options,
                vec!["get".to_string(), "put".to_string()],
                "{spelling}: delimiter must yield the exact words"
            );
            assert_eq!(evidence.odd_argument_count, false, "{spelling}: word count");
        }
        // The slash delimiter yields the exact words in order (strongest
        // check: same words, exotic delimiter).
        let evidence = parse_dancer2_two_x_import_args(&["qw/get post put delete/".to_string()]);
        assert_eq!(evidence.unmodeled_options, vec!["get".to_string(), "put".to_string()]);
        assert!(!evidence.odd_argument_count);
    }

    #[test]
    fn qw_words_never_occupy_the_version_slot() {
        // A qw word shaped like a v-string is an import argument, not a
        // version: the qw quoting marks it as a literal word.
        let evidence = parse_dancer2_two_x_import_args(&["qw(v2.01 get)".to_string()]);
        assert!(
            evidence.version_slot_spellings.is_empty(),
            "a qw word must not be taken for the VERSION slot: {:?}",
            evidence.version_slot_spellings
        );
    }

    #[test]
    fn quoted_v_string_is_a_literal_import_argument() {
        // `'v2.01'` is quoted: it reaches import as a string argument, not a
        // version slot. A lone argument makes the arity odd — the pinned
        // upstream contract dies at compile time there.
        let evidence = parse_dancer2_two_x_import_args(&["'v2.01'".to_string()]);
        assert!(
            evidence.version_slot_spellings.is_empty(),
            "a quoted v-string must not be taken for the VERSION slot: {:?}",
            evidence.version_slot_spellings
        );
        assert!(evidence.odd_argument_count, "one lone argument is odd");
    }

    #[test]
    fn unquoted_v_string_still_occupies_the_version_slot() {
        let evidence = parse_dancer2_two_x_import_args(&["v2.01".to_string(), "get".to_string()]);
        assert_eq!(evidence.version_slot_spellings, vec!["v2.01".to_string()]);
    }

    #[test]
    fn quote_like_exclusions_reach_the_exclusion_list() {
        // `q{!get}` reaches import as the string `!get` — an exclusion, not an
        // unmodeled option.
        let evidence = parse_dancer2_two_x_import_args(&["q{!get}".to_string()]);
        assert!(
            evidence.excluded_keywords.contains(&"get".to_string()),
            "q{{!get}} must exclude get: {:?}",
            evidence
        );
        let evidence = parse_dancer2_two_x_import_args(&["qq(!post)".to_string()]);
        assert!(
            evidence.excluded_keywords.contains(&"post".to_string()),
            "qq(!post) must exclude post: {:?}",
            evidence
        );
    }

    #[test]
    fn double_quoted_option_values_are_dynamic_single_quoted_are_literal() {
        let evidence = parse_dancer2_two_x_import_args(
            &["appname".to_string(), "\"prefix-$app\"".to_string()].to_vec(),
        );
        assert!(
            matches!(evidence.appname, Some(AppNameSelection::Dynamic { .. })),
            "a double-quoted interpolated value must be dynamic: {:?}",
            evidence.appname
        );
        let evidence =
            parse_dancer2_two_x_import_args(&["appname".to_string(), "'$app'".to_string()]);
        assert_eq!(
            evidence.appname,
            Some(AppNameSelection::Literal("$app".to_string())),
            "single quotes never interpolate: the sigil is literal text"
        );
    }

    #[test]
    fn cancelled_admission_returns_cancelled_not_detected() {
        use crate::framework::AdapterCancellation;
        let mut input = detection_input(
            vec![matched_two_x(
                Some("2.0.1"),
                "gen-1",
                crate::framework::DetectionEvidenceClass::ResolvedModule,
            )],
            "gen-1",
        );
        input.cancellation = AdapterCancellation::cancelled();
        let result = detect_dancer2_two_x(&input);
        assert_eq!(result.outcome, DetectionOutcome::Cancelled);
    }

    #[test]
    fn foreign_descriptor_fails_closed_without_detecting() {
        use crate::framework::AdapterId;
        let mut input = detection_input(
            vec![matched_two_x(
                Some("2.0.1"),
                "gen-1",
                crate::framework::DetectionEvidenceClass::ResolvedModule,
            )],
            "gen-1",
        );
        input.descriptor.adapter_id = AdapterId(u64::MAX);
        input.descriptor.name = "some-other-adapter".to_string();
        let result = detect_dancer2_two_x(&input);
        assert!(
            !matches!(result.outcome, DetectionOutcome::Detected { .. }),
            "a foreign descriptor must not produce a Dancer2 detection: {:?}",
            result.outcome
        );
    }

    #[test]
    fn contradictory_terminal_observations_surface_as_conflicting() {
        let input = detection_input(
            vec![
                matched_two_x(
                    Some("2.0.1"),
                    "gen-1",
                    crate::framework::DetectionEvidenceClass::ResolvedModule,
                ),
                absent_selector("Dancer2", "gen-1"),
            ],
            "gen-1",
        );
        let result = detect_dancer2_two_x(&input);
        assert!(
            matches!(result.outcome, DetectionOutcome::Conflicting { .. }),
            "matched + absent must surface as Conflicting, got {:?}",
            result.outcome
        );
    }
}
