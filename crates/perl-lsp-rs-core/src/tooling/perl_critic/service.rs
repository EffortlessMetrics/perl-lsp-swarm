//! One protocol-neutral native critic service (#9062).
//!
//! [`NativeCriticService::analyze`] is the sole production entrypoint for
//! native critic-rule evaluation from push, document-pull, workspace-pull,
//! and code-action consumers. The command adapter cutover remains #6969; this
//! module only exposes the seam that adapter will consume.
//!
//! The service composes the settled authorities instead of rebuilding them:
//! accepted configuration arrives as one immutable
//! [`crate::config::EffectiveCriticState`] (#8253), rule evaluation runs
//! through [`NativeCriticRegistry`], and every producer output enters the
//! canonical normalized finding set (#7475) before policy applies exactly
//! once post-merge. Consumers can no longer assemble their own
//! registry/context/candidate/policy pipeline, so two transports cannot
//! snapshot mutable configuration at different times or flatten metadata
//! differently: equal subjects produce equal ordered runs.
//!
//! Architecture boundary: this API contains no engine selector, no legacy or
//! external backend, no `.perlcriticrc`/executable/profile-path/process
//! state, and no LSP diagnostic or wire types. External Perl::Critic
//! observations cannot enter as product findings because no input field can
//! carry them (#7210/#7211 compatibility evidence stays outside).

use perl_parser_core::Node;

use super::{
    BuiltInCriticObservation, CriticConfig, CriticContext, CriticFinding, CriticSourceIdentity,
    CriticSuppressionMap, NativeCriticPolicy, NativeCriticRegistry, NormalizedCriticFinding,
    account_unresolved_native_identities, built_in_observation_candidates,
    native_finding_candidates, normalize_with_native_policy,
};
use crate::config::EffectiveCriticState;

/// Gate consulted at deterministic barrier points of one run.
///
/// `true` means proceed/current. Gates are plain functions supplied by the
/// transport, so liveness decisions stay generation-based: callers compare
/// captured identities against live authorities instead of sleeping.
#[derive(Clone, Copy)]
pub struct RunGate<'a> {
    check: Option<&'a dyn Fn() -> bool>,
}

impl<'a> RunGate<'a> {
    /// A gate that never blocks and never marks work stale.
    #[must_use]
    pub const fn open() -> Self {
        Self { check: None }
    }

    /// A gate backed by one caller-owned predicate.
    #[must_use]
    pub const fn new(check: &'a dyn Fn() -> bool) -> Self {
        Self { check: Some(check) }
    }

    /// Whether the gated property currently holds.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.check.is_none_or(|check| check())
    }
}

/// One immutable accepted analysis subject (#9062).
///
/// Every field is an owned or borrowed value captured before `analyze`
/// begins; the service copies no mutable server configuration during the
/// run and holds no lock while rules evaluate. The accepted state binds the
/// complete policy (profile, threshold, filters, owning root) from the #8253
/// authority, so contradictory roots naturally evaluate their own configs.
pub struct NativeCriticSubject<'a> {
    /// Accounting label for rejected-producer logs (typically the URI).
    pub label: &'a str,
    /// Exact logical source identity and generation of the analyzed text.
    pub source_identity: CriticSourceIdentity,
    /// Accepted parser/document analysis snapshot.
    pub ast: &'a Node,
    /// Accepted source text the findings' ranges refer to.
    pub source: &'a str,
    /// Complete accepted critic state from the #8253 authority.
    pub accepted_state: EffectiveCriticState,
    /// Producer-declared overlap observations (#11918) emitted by core lint
    /// emitters over this exact source generation. Empty when the caller has
    /// none (for example an action-only fresh analysis).
    pub overlap_observations: Vec<BuiltInCriticObservation>,
    /// Gate consulted before evaluation; a closed gate yields a cancelled run
    /// that performed no rule work.
    pub cancellation: RunGate<'a>,
    /// Gate consulted after evaluation and before the run may publish; a
    /// closed gate yields a stale run no consumer may treat as current.
    pub currentness: RunGate<'a>,
}

impl std::fmt::Debug for NativeCriticSubject<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeCriticSubject")
            .field("label", &self.label)
            .field("source_identity", &self.source_identity)
            .field("accepted_state_fingerprint", &self.accepted_state.fingerprint())
            .field("overlap_observation_count", &self.overlap_observations.len())
            .finish_non_exhaustive()
    }
}

/// Explicit completeness disposition of one [`NativeCriticRun`] (#9062).
///
/// A complete clean run and a partial/unavailable run are different values;
/// only [`Self::Complete`], [`Self::Partial`], and [`Self::Disabled`] may
/// populate current result storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeCriticRunCompleteness {
    /// Accepted state was disabled: the deliberate configured contribution,
    /// produced without any native rule evaluation.
    Disabled,
    /// All registered producer contributions evaluated and merged cleanly.
    Complete,
    /// The run finished, but some producer outputs were rejected and are
    /// accounted rather than silently missing.
    Partial {
        /// Number of producer findings rejected for an undeclared emission
        /// shape (#7475 accounting).
        unresolved_producer_identities: usize,
    },
    /// A required analysis input (facts tier) was not ready; reserved for the
    /// facts-dependent consumers (#9082 family).
    NotReady,
    /// The subject moved underneath the run; the result is superseded and
    /// must not publish or cache as current.
    Stale,
    /// The caller cancelled the run before or during evaluation.
    Cancelled,
    /// A producer instrument failed; the run carries no trustworthy rows.
    InstrumentFailure,
}

impl NativeCriticRunCompleteness {
    /// Settle the disposition from the observable outcomes of one run.
    ///
    /// Precedence: a closed currentness gate makes any finished work stale,
    /// regardless of how it went; otherwise accounted producer rejections
    /// downgrade completeness; otherwise the run is complete. Cancellation
    /// and not-ready states short-circuit earlier and never reach here.
    #[must_use]
    pub fn settle(current: bool, unresolved_producer_identities: usize) -> Self {
        if !current {
            return Self::Stale;
        }
        if unresolved_producer_identities > 0 {
            return Self::Partial { unresolved_producer_identities };
        }
        Self::Complete
    }

    /// Whether this disposition may populate current result storage.
    #[must_use]
    pub const fn is_publishable(&self) -> bool {
        matches!(self, Self::Complete | Self::Partial { .. } | Self::Disabled)
    }
}

/// Bounded work counters distinguishing what one run built, collected, and
/// kept (#9062). No cross-run reuse exists yet, so no counter claims reuse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeCriticWorkReceipt {
    /// Registered rules the run actually executed (zero when skipped).
    pub rules_evaluated: usize,
    /// Raw producer findings collected before normalization.
    pub native_findings_collected: usize,
    /// Producer-declared overlap observations admitted as candidates (#11918).
    pub observation_candidates_collected: usize,
    /// Producer findings rejected for an undeclared emission shape (#7475).
    pub unresolved_producer_identities: usize,
    /// Logical rows surviving post-merge policy application.
    pub findings_after_policy: usize,
}

/// One protocol-neutral native critic run (#9062).
///
/// The ordered normalized findings are the logical product surface; the raw
/// producer findings ride along solely as remediation carriers owned by the
/// native contract (quick-fix edits/titles), never as an alternative finding
/// set. Transport projection happens entirely downstream.
#[derive(Debug, Clone)]
pub struct NativeCriticRun {
    /// Explicit completeness/currentness disposition.
    pub completeness: NativeCriticRunCompleteness,
    /// Deterministic identity of the accepted state that produced this run.
    pub state_fingerprint: String,
    /// Owning folder/root identity bound into the accepted state.
    pub owning_root: Option<String>,
    /// Exact logical source identity and generation analyzed.
    pub source_identity: CriticSourceIdentity,
    /// Ordered normalized findings after canonical merge and policy.
    pub findings: Vec<NormalizedCriticFinding>,
    /// Raw producer findings of this run, in registry order. Remediation
    /// carriers only; consumers must not re-derive logical rows from them.
    pub producer_findings: Vec<CriticFinding>,
    /// Bounded work counters for this run.
    pub work: NativeCriticWorkReceipt,
}

impl NativeCriticRun {
    /// Whether this run may populate current result storage or caches.
    ///
    /// Superseded (stale), cancelled, not-ready, and instrument-failed runs
    /// are values, but never current ones (#9062 publication boundary).
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.completeness.is_publishable()
    }
}

/// The one production entrypoint for native critic-rule evaluation (#9062).
///
/// Consumes the semantic authorities rather than rebuilding them:
///
/// ```text
/// accepted source + #8253 accepted state + accepted facts
///   → required analysis inputs
///   → native rule evaluation
///   → #7475 canonical normalization
///   → profile/severity/include/exclude/scoped suppression
///   → deterministic ordering
///   → NativeCriticRun
/// ```
///
/// Filtering and suppression are NOT duplicated here: they delegate to the
/// settled #7475 seam (`normalize_with_native_policy`). No external process,
/// path, or engine selection is reachable from this call.
pub struct NativeCriticService;

impl NativeCriticService {
    /// Analyze one immutable accepted subject.
    #[must_use]
    pub fn analyze(subject: NativeCriticSubject<'_>) -> NativeCriticRun {
        let state_fingerprint = subject.accepted_state.fingerprint();
        let owning_root = subject.accepted_state.owning_root().map(ToOwned::to_owned);

        let EffectiveCriticState::Native(accepted) = subject.accepted_state else {
            // Disabled is a deliberate configured contribution: no policy
            // object exists, so no rule evaluation can run (#8253/#9062).
            return NativeCriticRun {
                completeness: NativeCriticRunCompleteness::Disabled,
                state_fingerprint,
                owning_root,
                source_identity: subject.source_identity,
                findings: Vec::new(),
                producer_findings: Vec::new(),
                work: NativeCriticWorkReceipt::default(),
            };
        };

        if !subject.cancellation.holds() {
            return NativeCriticRun {
                completeness: NativeCriticRunCompleteness::Cancelled,
                state_fingerprint,
                owning_root,
                source_identity: subject.source_identity,
                findings: Vec::new(),
                producer_findings: Vec::new(),
                work: NativeCriticWorkReceipt::default(),
            };
        }

        // The accepted policy is the only parameter source. The external
        // rc/profile/theme concepts stay out of the native subject entirely:
        // no shipped rule reads them, and no executable state is representable.
        let critic_config = CriticConfig {
            severity: accepted.severity_threshold,
            profile: None,
            theme: None,
            include: accepted.include.clone(),
            exclude: accepted.exclude.clone(),
            ..CriticConfig::default()
        };
        let context = CriticContext::new(subject.source, subject.ast, &critic_config);
        let registry =
            NativeCriticRegistry::for_profile_with_config(accepted.profile, &critic_config);
        let raw_findings = registry.check_unfiltered(&context);

        let mut work = NativeCriticWorkReceipt {
            rules_evaluated: registry.rule_ids().len(),
            native_findings_collected: raw_findings.len(),
            observation_candidates_collected: subject.overlap_observations.len(),
            ..NativeCriticWorkReceipt::default()
        };

        // Producer outputs enter the canonical normalized set (#7475): checked
        // identities at collection, alias merge, then policy applied exactly
        // once post-merge. Rejections are logged through the settled
        // accounting seam, never silently dropped, and counted into both the
        // receipt and the completeness disposition.
        let (candidates, unresolved) =
            native_finding_candidates(raw_findings.iter().cloned(), subject.source_identity);
        work.unresolved_producer_identities =
            account_unresolved_native_identities(subject.label, &unresolved);
        let candidates = candidates.into_iter().chain(built_in_observation_candidates(
            subject.overlap_observations,
            subject.source,
            subject.source_identity,
        ));

        let suppressions = CriticSuppressionMap::from_source(subject.source);
        let policy = NativeCriticPolicy::new(
            accepted.severity_threshold,
            &accepted.include,
            &accepted.exclude,
            &suppressions,
        );
        let findings = normalize_with_native_policy(candidates, &policy);
        work.findings_after_policy = findings.len();

        let completeness = NativeCriticRunCompleteness::settle(
            subject.currentness.holds(),
            work.unresolved_producer_identities,
        );

        NativeCriticRun {
            completeness,
            state_fingerprint,
            owning_root,
            source_identity: subject.source_identity,
            findings,
            producer_findings: raw_findings,
            work,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeCriticRunCompleteness, NativeCriticService, NativeCriticSubject, RunGate};
    use crate::config::{EffectiveCriticState, EffectiveNativeCriticConfig};
    use crate::tooling::perl_critic::{CriticSourceIdentity, critic_source_identity_for_uri};

    const STRICT_SOURCE: &str = "my $unused = 1;\n";

    fn native_state(root: Option<&str>) -> EffectiveCriticState {
        EffectiveCriticState::Native(EffectiveNativeCriticConfig {
            profile: crate::tooling::perl_critic::NativeCriticProfile::Strict,
            severity_threshold: 1,
            include: Vec::new(),
            exclude: Vec::new(),
            owning_root: root.map(ToOwned::to_owned),
        })
    }

    fn parse(source: &str) -> perl_parser_core::Node {
        use perl_parser::Parser;
        perl_tdd_support::must(Parser::new(source).parse())
    }

    fn subject<'a>(
        label: &'a str,
        ast: &'a perl_parser_core::Node,
        source: &'a str,
        state: EffectiveCriticState,
        identity: CriticSourceIdentity,
    ) -> NativeCriticSubject<'a> {
        NativeCriticSubject {
            label,
            source_identity: identity,
            ast,
            source,
            accepted_state: state,
            overlap_observations: Vec::new(),
            cancellation: RunGate::open(),
            currentness: RunGate::open(),
        }
    }

    #[test]
    fn same_accepted_subject_produces_identical_ordered_runs() {
        let source = STRICT_SOURCE;
        let ast = parse(source);
        let state = native_state(Some("root-a"));

        let first = NativeCriticService::analyze(subject(
            "file:///a.pm",
            &ast,
            source,
            state.clone(),
            critic_source_identity_for_uri("file:///a.pm", 7),
        ));
        let second = NativeCriticService::analyze(subject(
            "other-label",
            &ast,
            source,
            state,
            critic_source_identity_for_uri("file:///a.pm", 7),
        ));

        assert_eq!(first.completeness, NativeCriticRunCompleteness::Complete);
        assert_eq!(first.findings, second.findings, "label is not a finding input");
        assert_eq!(first.state_fingerprint, second.state_fingerprint);
        assert_eq!(
            first.work.rules_evaluated, second.work.rules_evaluated,
            "both transports evaluate the same rule set for one subject"
        );
        assert!(
            !first.findings.is_empty(),
            "the strict probe source must produce at least one row"
        );
    }

    #[test]
    fn disabled_state_performs_no_native_rule_evaluation() {
        let source = STRICT_SOURCE;
        let ast = parse(source);

        let run = NativeCriticService::analyze(subject(
            "file:///disabled.pm",
            &ast,
            source,
            EffectiveCriticState::Disabled,
            critic_source_identity_for_uri("file:///disabled.pm", 3),
        ));

        assert_eq!(run.completeness, NativeCriticRunCompleteness::Disabled);
        assert!(run.is_publishable(), "disabled is the deliberate configured contribution");
        assert_eq!(run.work.rules_evaluated, 0, "no rule may run while disabled");
        assert!(run.findings.is_empty());
        assert!(run.producer_findings.is_empty());
        assert_eq!(run.owning_root, None);
    }

    #[test]
    fn contradictory_roots_use_their_own_accepted_configs() {
        let source = STRICT_SOURCE;
        let ast = parse(source);
        let strict = native_state(Some("root-a"));
        let mut lenient = native_state(None);
        if let EffectiveCriticState::Native(config) = &mut lenient {
            config.severity_threshold = 5;
        }
        let lenient = lenient;

        let run_a = NativeCriticService::analyze(subject(
            "file:///a.pm",
            &ast,
            source,
            strict.clone(),
            critic_source_identity_for_uri("file:///a.pm", 1),
        ));
        let run_b = NativeCriticService::analyze(subject(
            "file:///b.pm",
            &ast,
            source,
            lenient.clone(),
            critic_source_identity_for_uri("file:///b.pm", 1),
        ));

        assert_ne!(strict.fingerprint(), lenient.fingerprint());
        assert_ne!(run_a.state_fingerprint, run_b.state_fingerprint);
        assert_eq!(run_a.owning_root.as_deref(), Some("root-a"));
        assert_eq!(run_b.owning_root, None);
        assert!(
            run_b.findings.len() < run_a.findings.len(),
            "root B's own threshold-5 policy must not be satisfied by root A's findings"
        );
    }

    #[test]
    fn movement_after_analysis_makes_the_run_stale_and_unpublishable() {
        let source = STRICT_SOURCE;
        let ast = parse(source);

        let closed = RunGate::new(&|| false);
        let run = NativeCriticService::analyze(NativeCriticSubject {
            label: "file:///moved.pm",
            source_identity: critic_source_identity_for_uri("file:///moved.pm", 4),
            ast: &ast,
            source,
            accepted_state: native_state(Some("root-a")),
            overlap_observations: Vec::new(),
            cancellation: RunGate::open(),
            currentness: closed,
        });

        assert!(!run.findings.is_empty(), "stale runs still carry their evaluated rows as values");
        assert_eq!(run.completeness, NativeCriticRunCompleteness::Stale);
        assert!(
            !run.is_publishable(),
            "a superseded run can never populate current result storage"
        );
    }

    #[test]
    fn cancellation_before_evaluation_performs_no_work() {
        let source = STRICT_SOURCE;
        let ast = parse(source);

        let closed = RunGate::new(&|| false);
        let run = NativeCriticService::analyze(NativeCriticSubject {
            label: "file:///cancelled.pm",
            source_identity: critic_source_identity_for_uri("file:///cancelled.pm", 2),
            ast: &ast,
            source,
            accepted_state: native_state(None),
            overlap_observations: Vec::new(),
            cancellation: closed,
            currentness: RunGate::open(),
        });

        assert_eq!(run.completeness, NativeCriticRunCompleteness::Cancelled);
        assert!(!run.is_publishable());
        assert_eq!(run.work.rules_evaluated, 0);
        assert!(run.producer_findings.is_empty());
    }

    #[test]
    fn partial_completeness_never_collapses_into_complete_clean() {
        assert_eq!(
            NativeCriticRunCompleteness::settle(true, 0),
            NativeCriticRunCompleteness::Complete
        );
        assert_eq!(
            NativeCriticRunCompleteness::settle(true, 2),
            NativeCriticRunCompleteness::Partial { unresolved_producer_identities: 2 }
        );
        assert_eq!(
            NativeCriticRunCompleteness::settle(false, 0),
            NativeCriticRunCompleteness::Stale,
            "staleness outranks completeness: late work cannot publish as clean"
        );
        assert_eq!(
            NativeCriticRunCompleteness::settle(false, 3),
            NativeCriticRunCompleteness::Stale
        );

        let partial = NativeCriticRunCompleteness::Partial { unresolved_producer_identities: 1 };
        assert!(partial.is_publishable(), "partial rows are real values");
        assert!(!NativeCriticRunCompleteness::NotReady.is_publishable());
        assert!(!NativeCriticRunCompleteness::InstrumentFailure.is_publishable());
    }

    #[test]
    fn service_result_carries_no_engine_or_external_process_surface() {
        // The accepted state family cannot represent an engine, executable, or
        // profile path; the fingerprint of a contaminated raw configuration
        // equals a clean one because derivation ignores those fields (#8253).
        use crate::config::{CriticEngine, ServerConfig};

        let clean = ServerConfig::default();
        let contaminated = ServerConfig {
            critic_engine: CriticEngine::Legacy,
            perlcritic_profile: Some("/discovered/.perlcriticrc".to_string()),
            ..ServerConfig::default()
        };

        assert_eq!(
            clean.effective_critic_state(Some("root")).fingerprint(),
            contaminated.effective_critic_state(Some("root")).fingerprint()
        );
    }
}
