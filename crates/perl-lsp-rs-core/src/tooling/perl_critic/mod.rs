//! Perl::Critic integration for code quality analysis.
//!
//! Provides integration with Perl::Critic for static code analysis
//! and policy enforcement in Perl code.

mod analyzer;
mod built_in;
mod identity;
mod native;
mod normalized;
mod quick_fix;
mod remediation;
mod result_identity;
mod semantic;
mod service;
mod types;

pub use analyzer::{CriticAnalyzer, hash_content};
pub use built_in::{BuiltInAnalyzer, Policy};
pub use identity::{
    CRITIC_IDENTITY_SCHEMA_VERSION, CriticAlias, CriticFindingOrigin, CriticFindingShape,
    CriticIdentityCategory, CriticIdentityDisposition, CriticIdentityEntry, CriticIdentityRegistry,
    CriticIdentityRegistryError, CriticObservedIdentity, CriticObservedIdentityError,
    NativeCriticIdentityDisposition,
};
pub use native::{
    AssignmentInConditionRule, CriticCategory, CriticContext, CriticFinding, CriticFix,
    CriticRelatedInformation, CriticRule, CriticSuppression, CriticSuppressionMap,
    CriticSuppressionScope, CriticTextEdit, DeprecatedDefinedRule, DuplicateLexicalDeclarationRule,
    DuplicateParameterRule, FixSafety, NativeCriticProfile, NativeCriticRegistry,
    ParameterShadowsGlobalRule, PrintfFormatArityRule, RequirePodSectionsRule,
    RequireUseStrictRule, RequireUseWarningsRule, ShadowedLexicalVariableRule, StaleDollarAtRule,
    UndefComparisonRule, UnreachableCodeRule, UnusedLexicalVariableRule, UnusedParameterRule,
};
pub use normalized::{
    CriticFindingCandidate, CriticFindingContributor, CriticPolicyRetention, CriticSourceIdentity,
    NormalizedCriticFinding, OwnedCriticObservedIdentity, normalize_critic_findings,
};
pub use quick_fix::{QuickFix, TextEdit};
pub use remediation::{CriticRemediationClass, CriticRemediationEligibility};
pub use result_identity::{
    AcceptedCriticPolicyIdentity, CriticPolicyIdentity, CriticPolicyIdentityError,
    DIAGNOSTIC_RESULT_IDENTITY_SCHEMA_VERSION, DiagnosticFactIdentity, DiagnosticResultIdentity,
    DiagnosticResultIdentityInput, DiagnosticResultSchemaVersions, DiagnosticSourceIdentity,
};
pub use semantic::{
    BuiltInCriticObservation, NativeCriticPolicy, UnresolvedNativeFindingIdentity,
    account_unresolved_native_identities, built_in_observation_candidates,
    critic_source_identity_for_uri, native_finding_candidates,
    native_finding_candidates_with_accounting, normalize_with_native_policy,
};
pub use service::{
    NativeCriticRun, NativeCriticRunCompleteness, NativeCriticService, NativeCriticSubject,
    NativeCriticWorkReceipt, RunGate,
};
pub use types::{CriticConfig, Severity, Violation};

/// String-surface form classifiers shared by the native critic rules and the
/// core lint emitters so both producers observe identical syntax shapes.
pub(crate) use native::{is_backtick_string, is_qx_string};

/// Error returned when an external native-critic profile token is not recognized.
///
/// The concrete error is owned by the profile implementation; this public alias
/// keeps the error name stable without requiring every caller to know that
/// implementation module's path.
pub type NativeCriticProfileParseError = <NativeCriticProfile as std::str::FromStr>::Err;

#[cfg(not(feature = "lsp-compat"))]
pub use types::ViolationSummary;

pub(crate) use quick_fix::built_in_quick_fix;
#[cfg(feature = "lsp-compat")]
pub(crate) use quick_fix::perlcritic_quick_fix;
pub(crate) use types::insertion_range;
