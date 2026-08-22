//! Native critic rule registry and profile orchestration.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::super::identity::{CriticFindingShape, NativeCriticIdentityDisposition};
use super::super::{CriticConfig, Severity, Violation};
use super::native_contract::{CriticContext, CriticFinding, CriticRule, PragmaEntries};
use super::native_suppressions::CriticSuppressionMap;
use super::{
    AssignmentInConditionRule, BacktickExecRule, BarewordFilehandleRule,
    CaptureVarWithoutRegexMatchRule, DeprecatedDefinedRule, DuplicateLexicalDeclarationRule,
    DuplicateParameterRule, ParameterShadowsGlobalRule, PipeOpenRule, PrintfFormatArityRule,
    ProhibitLeadingZerosRule, QxReadpipeRule, RequirePodSectionsRule, RequireUseStrictRule,
    RequireUseWarningsRule, ShadowedLexicalVariableRule, StaleDollarAtRule, StringEvalRule,
    SystemExecRule, TwoArgOpenRule, UncheckedOpenCloseRule, UndeclaredVariableRule,
    UndefComparisonRule, UninitializedVariableRule, UnquotedBarewordRule, UnreachableCodeRule,
    UnusedLexicalVariableRule, UnusedParameterRule,
};

const GENERAL: CriticFindingShape = CriticFindingShape::General;

// Producer-owned logical dispositions. Combined native rules appear once per
// logical finding shape; this is intentionally not reducible to the rule-ID
// catalog because that would lose the distinction completeness must prove.
static NATIVE_IDENTITY_DISPOSITIONS: &[NativeCriticIdentityDisposition] = &[
    NativeCriticIdentityDisposition::new("native.testing.require_use_strict", GENERAL),
    NativeCriticIdentityDisposition::new("native.testing.require_use_warnings", GENERAL),
    NativeCriticIdentityDisposition::new("native.common.assignment_in_condition", GENERAL),
    NativeCriticIdentityDisposition::new("native.common.printf_format_arity", GENERAL),
    NativeCriticIdentityDisposition::new("native.common.deprecated_defined", GENERAL),
    NativeCriticIdentityDisposition::new(
        "native.common.undef_comparison",
        CriticFindingShape::LiteralUndefComparison,
    ),
    NativeCriticIdentityDisposition::new("native.common.stale_dollar_at", GENERAL),
    NativeCriticIdentityDisposition::new("native.common.unreachable_code", GENERAL),
    NativeCriticIdentityDisposition::new("native.io.bareword_filehandle", GENERAL),
    NativeCriticIdentityDisposition::new("native.io.two_arg_open", GENERAL),
    NativeCriticIdentityDisposition::new("native.io.pipe_open", GENERAL),
    NativeCriticIdentityDisposition::new("native.io.unchecked_open_close", GENERAL),
    NativeCriticIdentityDisposition::new("native.security.qx_readpipe", CriticFindingShape::Qx),
    NativeCriticIdentityDisposition::new(
        "native.security.qx_readpipe",
        CriticFindingShape::Readpipe,
    ),
    NativeCriticIdentityDisposition::new(
        "native.security.backtick_exec",
        CriticFindingShape::Backtick,
    ),
    NativeCriticIdentityDisposition::new("native.security.string_eval", GENERAL),
    NativeCriticIdentityDisposition::new(
        "native.security.system_exec",
        CriticFindingShape::SystemCall,
    ),
    NativeCriticIdentityDisposition::new(
        "native.security.system_exec",
        CriticFindingShape::ExecCall,
    ),
    NativeCriticIdentityDisposition::new("native.variables.unused_lexical", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.unused_parameter", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.duplicate_parameter", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.parameter_shadows_global", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.duplicate_lexical", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.shadowed_lexical", GENERAL),
    NativeCriticIdentityDisposition::new("native.regex.capture_without_match", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.undeclared", GENERAL),
    NativeCriticIdentityDisposition::new("native.variables.uninitialized", GENERAL),
    NativeCriticIdentityDisposition::new("native.syntax.unquoted_bareword", GENERAL),
    NativeCriticIdentityDisposition::new("native.documentation.require_pod_sections", GENERAL),
    NativeCriticIdentityDisposition::new("native.syntax.prohibit_leading_zeros", GENERAL),
];

const MAX_REJECTED_PROFILE_CHARS: usize = 80;

/// Native critic rule bundle used by configuration, diagnostics, and readiness tooling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NativeCriticProfile {
    /// Lower-noise default for normal editor diagnostics.
    #[default]
    Recommended,
    /// Every registered native rule, useful for strict audits and rule coverage.
    Strict,
}

impl NativeCriticProfile {
    /// Canonical tokens accepted at external configuration boundaries.
    pub const VALID_OPTIONS: &'static str = "recommended, strict";

    /// Parse a native critic profile token.
    ///
    /// Leading/trailing whitespace and ASCII case are normalized at the
    /// boundary. Callers can carry this enum while removing profile reparsing.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        raw.parse().ok()
    }

    /// Parse a legacy string carrier without changing its historical semantics.
    ///
    /// The older runtime carriers used an exact-token parse and fell back to
    /// [`Self::Strict`] for anything else. Keep that behavior explicit while
    /// configuration boundaries use [`Self::parse`] and its normalization.
    #[must_use]
    pub fn parse_legacy(raw: &str) -> Option<Self> {
        match raw {
            "recommended" => Some(Self::Recommended),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }

    /// Stable canonical profile label for configuration, receipts, and display.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "recommended",
            Self::Strict => "strict",
        }
    }
}

impl fmt::Display for NativeCriticProfile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NativeCriticProfile {
    type Err = NativeCriticProfileParseError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "recommended" => Ok(Self::Recommended),
            "strict" => Ok(Self::Strict),
            _ => Err(NativeCriticProfileParseError { value: raw.to_string() }),
        }
    }
}

impl Serialize for NativeCriticProfile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NativeCriticProfile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// Error returned when an external profile token is not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCriticProfileParseError {
    value: String,
}

impl NativeCriticProfileParseError {
    /// Unrecognized token exactly as supplied by the caller.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for NativeCriticProfileParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unrecognized native critic profile '{}'; expected {}",
            render_rejected_profile(&self.value),
            NativeCriticProfile::VALID_OPTIONS
        )
    }
}

impl std::error::Error for NativeCriticProfileParseError {}

fn render_rejected_profile(value: &str) -> String {
    let mut chars = value.chars();
    let mut rendered = chars
        .by_ref()
        .take(MAX_REJECTED_PROFILE_CHARS)
        .flat_map(char::escape_default)
        .collect::<String>();
    if chars.next().is_some() {
        rendered.push('…');
    }
    rendered
}

/// Registry for Rust-native critic rules.
///
/// The registry is intentionally small orchestration: it owns rule instances,
/// runs them against a shared context, and returns their findings in registry
/// order. Runtime diagnostic wiring can build on this without each caller
/// needing to know how native rules are stored or executed.
#[derive(Default)]
pub struct NativeCriticRegistry {
    rules: Vec<Box<dyn CriticRule>>,
}

impl NativeCriticRegistry {
    /// Create an empty native critic registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry from prebuilt rules.
    #[must_use]
    pub fn with_rules(rules: Vec<Box<dyn CriticRule>>) -> Self {
        Self { rules }
    }

    /// Create the default recommended native critic registry.
    ///
    /// This is the lower-noise bundle intended for normal editor diagnostics.
    /// Keep ordering stable so diagnostics and receipts are deterministic.
    #[must_use]
    pub fn recommended() -> Self {
        Self::for_profile(NativeCriticProfile::Recommended)
    }

    /// Create a native critic registry for a named profile.
    ///
    /// The explicit profile entry point lets receipts and readiness checks
    /// measure either the lower-noise default or the full strict rule set.
    #[must_use]
    pub fn for_profile(profile: NativeCriticProfile) -> Self {
        match profile {
            NativeCriticProfile::Recommended => Self::recommended_profile(),
            NativeCriticProfile::Strict => Self::strict_profile(),
        }
    }

    /// Create a native critic registry for `profile`, widened by `config.include`.
    ///
    /// `profile` supplies the base rule set. Any rule ID listed in
    /// `config.include` that the profile does not already carry is resolved
    /// against the full rule catalog and appended, so `include` can name a
    /// strict-only rule (for example `native.variables.unused_lexical`)
    /// without switching the whole profile to `strict`.
    ///
    /// `include` remains a whitelist: [`NativeCriticRegistry::check`] still
    /// runs only the listed rules when the list is non-empty. Widening the
    /// registry changes exactly one case — an `include` entry outside the
    /// profile used to resolve to nothing at all, and now resolves to the rule
    /// it names. IDs that match no catalog rule are ignored here; config load
    /// warns about them.
    ///
    /// Call this on the paths that honor user configuration. Use
    /// [`NativeCriticRegistry::for_profile`] when you want the profile roster
    /// itself (receipts, readiness tooling, the known-ID catalog).
    #[must_use]
    pub fn for_profile_with_config(profile: NativeCriticProfile, config: &CriticConfig) -> Self {
        let mut registry = Self::for_profile(profile);
        if config.include.is_empty() {
            return registry;
        }

        let already_present = registry.rule_ids();
        for rule in Self::catalog() {
            let id = rule.id();
            if already_present.contains(&id) {
                continue;
            }
            if config.include.iter().any(|policy| policy == id) {
                registry.add_rule(rule);
            }
        }

        registry
    }

    /// Every native rule this build ships, in stable catalog order.
    ///
    /// The strict profile is the full catalog by construction, so it is the
    /// single roster both entry points read.
    fn catalog() -> Vec<Box<dyn CriticRule>> {
        Self::strict_profile().rules
    }

    fn recommended_profile() -> Self {
        Self::with_rules(vec![
            Box::new(RequireUseStrictRule),
            Box::new(RequireUseWarningsRule),
            Box::new(AssignmentInConditionRule),
            Box::new(PrintfFormatArityRule),
            Box::new(DeprecatedDefinedRule),
            Box::new(UndefComparisonRule),
            Box::new(StaleDollarAtRule),
            Box::new(UnreachableCodeRule),
            Box::new(BarewordFilehandleRule),
            Box::new(TwoArgOpenRule),
            Box::new(PipeOpenRule),
            Box::new(UncheckedOpenCloseRule),
            Box::new(QxReadpipeRule),
            Box::new(BacktickExecRule),
            Box::new(StringEvalRule),
            Box::new(SystemExecRule),
        ])
    }

    fn strict_profile() -> Self {
        Self::with_rules(vec![
            Box::new(RequireUseStrictRule),
            Box::new(RequireUseWarningsRule),
            Box::new(AssignmentInConditionRule),
            Box::new(PrintfFormatArityRule),
            Box::new(DeprecatedDefinedRule),
            Box::new(UndefComparisonRule),
            Box::new(StaleDollarAtRule),
            Box::new(UnreachableCodeRule),
            Box::new(BarewordFilehandleRule),
            Box::new(TwoArgOpenRule),
            Box::new(PipeOpenRule),
            Box::new(UncheckedOpenCloseRule),
            Box::new(QxReadpipeRule),
            Box::new(BacktickExecRule),
            Box::new(StringEvalRule),
            Box::new(SystemExecRule),
            Box::new(UnusedLexicalVariableRule),
            Box::new(UnusedParameterRule),
            Box::new(DuplicateParameterRule),
            Box::new(ParameterShadowsGlobalRule),
            Box::new(DuplicateLexicalDeclarationRule),
            Box::new(ShadowedLexicalVariableRule),
            Box::new(CaptureVarWithoutRegexMatchRule),
            Box::new(UndeclaredVariableRule),
            Box::new(UninitializedVariableRule),
            Box::new(UnquotedBarewordRule),
            Box::new(RequirePodSectionsRule),
            Box::new(ProhibitLeadingZerosRule),
        ])
    }

    /// Add a rule to the registry.
    pub fn add_rule(&mut self, rule: Box<dyn CriticRule>) {
        self.rules.push(rule);
    }

    /// Number of rules in the registry.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the registry has no rules.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Stable IDs for registered rules, in execution order.
    #[must_use]
    pub fn rule_ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|rule| rule.id()).collect()
    }

    /// Producer-owned `(rule_id, shape)` obligations for identity coverage.
    #[must_use]
    pub const fn identity_dispositions() -> &'static [NativeCriticIdentityDisposition] {
        NATIVE_IDENTITY_DISPOSITIONS
    }

    /// Run all registered rules and return collected findings.
    #[must_use]
    pub fn check(&self, ctx: &CriticContext<'_>) -> Vec<CriticFinding> {
        let suppressions = CriticSuppressionMap::from_source(ctx.source);
        self.check_unfiltered(ctx)
            .into_iter()
            .filter(|finding| severity_enabled(finding.severity, ctx.config))
            .filter(|finding| !suppressions.suppresses(finding))
            .collect()
    }

    /// Run all registered rules and return findings before post-normalization
    /// policy.
    ///
    /// Include/exclude stay execution gates here; the severity threshold and
    /// scoped suppression are policy decisions that must apply exactly once,
    /// after canonical alias merging (#7475). The production semantic boundary
    /// consumes this path; [`Self::check`] keeps the legacy filtered behavior
    /// for callers that have not migrated yet.
    #[must_use]
    pub fn check_unfiltered(&self, ctx: &CriticContext<'_>) -> Vec<CriticFinding> {
        let mut findings = Vec::new();

        // Pre-compute scope analysis once and share it across all scope-based
        // rules, instead of each rule independently rebuilding the pragma map
        // and re-walking the AST (#4999 item 3).
        let pragma_map_owned;
        let scope_issues_owned;
        let (scope_issues_ref, pragma_map_ref): (
            Option<&[perl_semantic_analyzer::scope_analyzer::ScopeIssue]>,
            Option<&PragmaEntries>,
        ) = if ctx.scope_issues.is_some() && ctx.pragma_map.is_some() {
            // Caller already pre-computed; reuse.
            (ctx.scope_issues, ctx.pragma_map)
        } else {
            pragma_map_owned = perl_pragma::PragmaTracker::build(ctx.ast);
            scope_issues_owned = perl_semantic_analyzer::scope_analyzer::ScopeAnalyzer::new()
                .analyze(ctx.ast, ctx.source, &pragma_map_owned);
            (Some(&scope_issues_owned[..]), Some(&pragma_map_owned[..]))
        };

        // Build a context that carries the pre-computed scope results.
        let rich_ctx = match (scope_issues_ref, pragma_map_ref) {
            (Some(si), Some(pm)) => {
                CriticContext::with_scope(ctx.source, ctx.ast, ctx.config, si, pm)
            }
            _ => CriticContext::new(ctx.source, ctx.ast, ctx.config),
        };

        for rule in &self.rules {
            if !rule_enabled(rule.as_ref(), rich_ctx.config) {
                continue;
            }
            rule.check(&rich_ctx, &mut findings);
        }

        findings
    }

    /// Run all registered rules and return current legacy violation values.
    ///
    /// This keeps native rule execution single-sourced while callers migrate
    /// from `Violation` consumers to richer native finding/code-action data.
    #[must_use]
    pub fn check_violations(
        &self,
        ctx: &CriticContext<'_>,
        file: impl Into<String>,
    ) -> Vec<Violation> {
        let file = file.into();
        self.check(ctx).into_iter().map(|finding| finding.to_violation(file.clone())).collect()
    }
}

/// Whitelist/blacklist gate applied to the rules the registry actually holds.
///
/// A non-empty `include` narrows execution to the listed IDs. Rules the
/// profile does not carry are added by
/// [`NativeCriticRegistry::for_profile_with_config`] before this runs, so an
/// `include` entry from outside the profile reaches this gate as a real rule
/// rather than resolving to nothing.
fn rule_enabled(rule: &dyn CriticRule, config: &CriticConfig) -> bool {
    let id = rule.id();
    let included = config.include.is_empty() || config.include.iter().any(|policy| policy == id);
    let excluded = config.exclude.iter().any(|policy| policy == id);

    included && !excluded
}

fn severity_enabled(severity: Severity, config: &CriticConfig) -> bool {
    severity as u8 >= config.severity
}

#[cfg(test)]
mod profile_tests {
    use std::str::FromStr;

    use super::{MAX_REJECTED_PROFILE_CHARS, NativeCriticProfile, NativeCriticProfileParseError};

    #[test]
    fn recommended_is_the_internal_default() {
        assert_eq!(NativeCriticProfile::default(), NativeCriticProfile::Recommended);
    }

    #[test]
    fn parsing_normalizes_only_case_and_surrounding_whitespace() {
        assert_eq!(
            NativeCriticProfile::from_str(" Recommended "),
            Ok(NativeCriticProfile::Recommended)
        );
        assert_eq!(NativeCriticProfile::from_str("STRICT"), Ok(NativeCriticProfile::Strict));
        assert!(NativeCriticProfile::from_str("recomended").is_err());
    }

    #[test]
    fn legacy_parsing_keeps_exact_token_compatibility() {
        assert_eq!(
            NativeCriticProfile::parse_legacy("recommended"),
            Some(NativeCriticProfile::Recommended)
        );
        assert_eq!(NativeCriticProfile::parse_legacy("strict"), Some(NativeCriticProfile::Strict));
        assert!(NativeCriticProfile::parse_legacy(" RECOMMENDED ").is_none());
        assert!(NativeCriticProfile::parse_legacy("STRICT").is_none());
    }

    #[test]
    fn invalid_tokens_preserve_the_original_value_in_the_error() {
        let error = NativeCriticProfile::from_str(" recomended ");
        assert_eq!(error, Err(NativeCriticProfileParseError { value: " recomended ".to_string() }));
    }

    #[test]
    fn invalid_token_display_escapes_control_characters_without_changing_evidence() {
        let raw = "strict\n\t\u{0007}'";
        let error =
            NativeCriticProfile::from_str(raw).expect_err("control-bearing token must fail");
        let rendered = error.to_string();

        assert_eq!(error.value(), raw);
        assert!(!rendered.contains('\n'));
        assert!(!rendered.contains('\t'));
        assert!(!rendered.contains('\u{0007}'));
        assert!(rendered.contains("\\n"));
        assert!(rendered.contains("\\t"));
        assert!(rendered.contains("\\u{7}"));
        assert!(rendered.contains("\\'"));
    }

    #[test]
    fn invalid_token_display_is_bounded_while_the_error_retains_the_full_value() {
        let raw = "x".repeat(MAX_REJECTED_PROFILE_CHARS + 32);
        let error = NativeCriticProfile::from_str(&raw).expect_err("oversized token must fail");
        let rendered = error.to_string();

        assert_eq!(error.value(), raw);
        assert!(rendered.contains(&format!("{}…", "x".repeat(MAX_REJECTED_PROFILE_CHARS))));
        assert!(!rendered.contains(&"x".repeat(MAX_REJECTED_PROFILE_CHARS + 1)));
    }

    #[test]
    fn serde_round_trips_canonical_tokens() -> Result<(), serde_json::Error> {
        for profile in [NativeCriticProfile::Recommended, NativeCriticProfile::Strict] {
            let encoded = serde_json::to_string(&profile)?;
            assert_eq!(encoded, format!("\"{}\"", profile.as_str()));
            let decoded: NativeCriticProfile = serde_json::from_str(&encoded)?;
            assert_eq!(decoded, profile);
        }
        Ok(())
    }

    #[test]
    fn serde_rejects_unknown_tokens_without_widening() {
        let decoded = serde_json::from_str::<NativeCriticProfile>("\"unknown\"");
        assert!(decoded.is_err());
    }
}
