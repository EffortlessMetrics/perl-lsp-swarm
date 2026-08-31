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
use perl_source_identity::ContentDigest;

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
/// Construction is sealed behind [`NativeCriticSubject::accepted`]: the
/// service owns the accepted-snapshot shape, so a caller cannot assemble a
/// subject that skips the content binding or the gate wiring. Every field is
/// an owned or borrowed value captured before `analyze` begins; the service
/// copies no mutable server configuration during the run and holds no lock
/// while rules evaluate. The accepted state binds the complete policy
/// (profile, threshold, filters, owning root) from the #8253 authority, so
/// contradictory roots naturally evaluate their own configs.
pub struct NativeCriticSubject<'a> {
    /// Accounting label for rejected-producer logs (typically the URI).
    label: &'a str,
    /// Exact logical source identity and generation of the analyzed text.
    source_identity: CriticSourceIdentity,
    /// Accepted parser/document analysis snapshot.
    ast: &'a Node,
    /// Accepted source text the findings' ranges refer to.
    source: &'a str,
    /// Content digest of the exact accepted source bytes. Binds the text this
    /// run actually consumed into the published result: two runs claiming the
    /// same logical identity over different text carry different digests. The
    /// logical-identity/generation semantics stay owned by the
    /// source_identity.v1 contract (perl-source-identity), which is the
    /// authority for upgrading to full AST↔content revision binding.
    source_digest: ContentDigest,
    /// Complete accepted critic state from the #8253 authority.
    accepted_state: EffectiveCriticState,
    /// Producer-declared overlap observations (#11918) emitted by core lint
    /// emitters over this exact source generation. Empty when the caller has
    /// none (for example an action-only fresh analysis).
    overlap_observations: Vec<BuiltInCriticObservation>,
    /// Gate consulted before evaluation and re-checked at the post-evaluation
    /// settlement barrier; a closed gate yields a cancelled run. Before the
    /// work, no rule runs; after it, the performed work stays in the receipt
    /// as unpublishable values.
    cancellation: RunGate<'a>,
    /// Gate consulted after evaluation and before the run may publish; a
    /// closed gate yields a stale run no consumer may treat as current.
    currentness: RunGate<'a>,
}

impl<'a> NativeCriticSubject<'a> {
    /// Check and accept one immutable snapshot as an analysis subject (#9062).
    ///
    /// This is the only construction path. It binds the exact source bytes
    /// into the subject through a SHA-256 content digest at acceptance time,
    /// so every run published from this subject is traceable to the text it
    /// actually evaluated — not merely to a caller-declared logical identity.
    #[must_use]
    pub fn accepted(
        label: &'a str,
        source_identity: CriticSourceIdentity,
        ast: &'a Node,
        source: &'a str,
        accepted_state: EffectiveCriticState,
        overlap_observations: Vec<BuiltInCriticObservation>,
        cancellation: RunGate<'a>,
        currentness: RunGate<'a>,
    ) -> Self {
        Self {
            label,
            source_identity,
            ast,
            source,
            source_digest: ContentDigest::of_bytes(source.as_bytes()),
            accepted_state,
            overlap_observations,
            cancellation,
            currentness,
        }
    }
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
    /// (consulted at both the pre-evaluation and settlement barriers) and
    /// not-ready states short-circuit earlier and never reach here.
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
///
/// Construction is private to this module: a publishable
/// [`NativeCriticRun`] can only originate from
/// [`NativeCriticService::analyze`], which makes the service the sole
/// production authority instead of a convention. Downstream crates consume
/// runs through the read-only accessors.
#[derive(Debug, Clone)]
pub struct NativeCriticRun {
    /// Explicit completeness/currentness disposition.
    completeness: NativeCriticRunCompleteness,
    /// Deterministic identity of the accepted state that produced this run.
    state_fingerprint: String,
    /// Owning folder/root identity bound into the accepted state.
    owning_root: Option<String>,
    /// Exact logical source identity and generation analyzed.
    source_identity: CriticSourceIdentity,
    /// Content digest of the exact source bytes the run consumed.
    source_digest: ContentDigest,
    /// Ordered normalized findings after canonical merge and policy.
    findings: Vec<NormalizedCriticFinding>,
    /// Raw producer findings of this run, in registry order. Remediation
    /// carriers only; consumers must not re-derive logical rows from them.
    producer_findings: Vec<CriticFinding>,
    /// Bounded work counters for this run.
    work: NativeCriticWorkReceipt,
}

impl NativeCriticRun {
    /// Explicit completeness/currentness disposition of this run.
    #[must_use]
    pub fn completeness(&self) -> &NativeCriticRunCompleteness {
        &self.completeness
    }

    /// Deterministic identity of the accepted state that produced this run.
    #[must_use]
    pub fn state_fingerprint(&self) -> &str {
        &self.state_fingerprint
    }

    /// Owning folder/root identity bound into the accepted state.
    #[must_use]
    pub fn owning_root(&self) -> Option<&str> {
        self.owning_root.as_deref()
    }

    /// Exact logical source identity and generation analyzed.
    #[must_use]
    pub const fn source_identity(&self) -> &CriticSourceIdentity {
        &self.source_identity
    }

    /// Content digest of the exact source bytes this run consumed.
    #[must_use]
    pub const fn source_digest(&self) -> &ContentDigest {
        &self.source_digest
    }

    /// Ordered normalized findings after canonical merge and policy.
    #[must_use]
    pub fn findings(&self) -> &[NormalizedCriticFinding] {
        &self.findings
    }

    /// Raw producer findings of this run, in registry order. Remediation
    /// carriers only; consumers must not re-derive logical rows from them.
    #[must_use]
    pub fn producer_findings(&self) -> &[CriticFinding] {
        &self.producer_findings
    }

    /// Bounded work counters for this run.
    #[must_use]
    pub const fn work(&self) -> NativeCriticWorkReceipt {
        self.work
    }

    /// Whether this run may populate current result storage or caches.
    ///
    /// Superseded (stale), cancelled, not-ready, and instrument-failed runs
    /// are values, but never current ones (#9062 publication boundary).
    #[must_use]
    pub fn is_publishable(&self) -> bool {
        self.completeness.is_publishable()
    }

    /// Whether the caller's producer-declared overlap observations (#11918)
    /// actually entered this run's normalization, so the ordinary carrier rows
    /// are represented in [`Self::findings`].
    ///
    /// This is NOT the same question as [`Self::is_publishable`]. A `Disabled`
    /// run is publishable -- it is the deliberate configured contribution --
    /// but it evaluates no rule and consumes no observation, so it supersedes
    /// nothing. A transport that surrenders carrier rows on publishability
    /// alone deletes ordinary core diagnostics whenever critic is switched off.
    #[must_use]
    pub fn superseded_overlap_carriers(&self) -> bool {
        matches!(
            self.completeness,
            NativeCriticRunCompleteness::Complete | NativeCriticRunCompleteness::Partial { .. }
        )
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

        // Gates govern every disposition, including Disabled: a run whose
        // subject moved or was cancelled mid-flight is superseded regardless
        // of how little work it would have performed, so a stale empty
        // contribution can never clear newer current storage (#9062
        // currentness law).
        if !subject.cancellation.holds() {
            return NativeCriticRun {
                completeness: NativeCriticRunCompleteness::Cancelled,
                state_fingerprint,
                owning_root,
                source_identity: subject.source_identity,
                source_digest: subject.source_digest,
                findings: Vec::new(),
                producer_findings: Vec::new(),
                work: NativeCriticWorkReceipt::default(),
            };
        }

        let EffectiveCriticState::Native(accepted) = subject.accepted_state else {
            // Disabled is a deliberate configured contribution: no policy
            // object exists, so no rule evaluation can run (#8253/#9062).
            // Currentness still governs publication: configuration that
            // changed while this run was in flight makes the empty disabled
            // contribution stale exactly like any evaluated run, so it can
            // never clear newer current storage.
            let completeness = if subject.currentness.holds() {
                NativeCriticRunCompleteness::Disabled
            } else {
                NativeCriticRunCompleteness::Stale
            };
            return NativeCriticRun {
                completeness,
                state_fingerprint,
                owning_root,
                source_identity: subject.source_identity,
                source_digest: subject.source_digest,
                findings: Vec::new(),
                producer_findings: Vec::new(),
                work: NativeCriticWorkReceipt::default(),
            };
        };

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
            rules_evaluated: registry.enabled_rule_count(&critic_config),
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

        // Mid-flight cancellation re-check (#9062): a caller whose
        // cancellation predicate closes while rules run receives a cancelled
        // run, never a publishable Complete/Partial one. Explicit precedence
        // at this barrier: caller abandonment outranks staleness, so a
        // subject that was both cancelled and moved reports Cancelled. The
        // performed work stays in the receipt and the evaluated rows stay as
        // values, exactly like stale runs.
        if !subject.cancellation.holds() {
            return NativeCriticRun {
                completeness: NativeCriticRunCompleteness::Cancelled,
                state_fingerprint,
                owning_root,
                source_identity: subject.source_identity,
                source_digest: subject.source_digest,
                findings,
                producer_findings: raw_findings,
                work,
            };
        }

        let completeness = NativeCriticRunCompleteness::settle(
            subject.currentness.holds(),
            work.unresolved_producer_identities,
        );

        NativeCriticRun {
            completeness,
            state_fingerprint,
            owning_root,
            source_identity: subject.source_identity,
            source_digest: subject.source_digest,
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
        NativeCriticSubject::accepted(
            label,
            identity,
            ast,
            source,
            state,
            Vec::new(),
            RunGate::open(),
            RunGate::open(),
        )
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

        assert_eq!(first.completeness(), &NativeCriticRunCompleteness::Complete);
        assert_eq!(first.findings(), second.findings(), "label is not a finding input");
        assert_eq!(first.state_fingerprint(), second.state_fingerprint());
        assert_eq!(
            first.work().rules_evaluated,
            second.work().rules_evaluated,
            "both transports evaluate the same rule set for one subject"
        );
        assert!(
            !first.findings().is_empty(),
            "the strict probe source must produce at least one row"
        );
    }

    #[test]
    fn work_receipt_counts_only_rules_enabled_by_policy() {
        let source = STRICT_SOURCE;
        let ast = parse(source);
        let mut state = native_state(None);
        let full = NativeCriticService::analyze(subject(
            "file:///receipt-full.pm",
            &ast,
            source,
            state.clone(),
            critic_source_identity_for_uri("file:///receipt-full.pm", 1),
        ));
        if full.work().rules_evaluated == 0 {
            return;
        }
        if let EffectiveCriticState::Native(config) = &mut state {
            config.exclude = vec!["native.testing.require_use_strict".to_string()];
        }
        let filtered = NativeCriticService::analyze(subject(
            "file:///receipt-filtered.pm",
            &ast,
            source,
            state,
            critic_source_identity_for_uri("file:///receipt-filtered.pm", 1),
        ));
        assert_eq!(
            filtered.work().rules_evaluated + 1,
            full.work().rules_evaluated,
            "receipt must count executed rules, not the full registry"
        );
    }

    #[test]
    fn published_runs_bind_the_exact_source_content_digest() {
        // The logical identity is identical for both subjects; only the text
        // differs. A publishable run must remain traceable to the bytes it
        // actually evaluated, so the digests must diverge even though no
        // caller-declared field changed.
        let identity = critic_source_identity_for_uri("file:///digest.pm", 5);
        let ast_a = parse(STRICT_SOURCE);
        let other_source = "my $other_unused = 2;\n";
        let ast_b = parse(other_source);

        let run_a = NativeCriticService::analyze(subject(
            "file:///digest.pm",
            &ast_a,
            STRICT_SOURCE,
            native_state(None),
            identity.clone(),
        ));
        let run_b = NativeCriticService::analyze(subject(
            "file:///digest.pm",
            &ast_b,
            other_source,
            native_state(None),
            identity,
        ));
        let run_a_again = NativeCriticService::analyze(subject(
            "file:///digest.pm",
            &ast_a,
            STRICT_SOURCE,
            native_state(None),
            critic_source_identity_for_uri("file:///digest.pm", 5),
        ));

        assert_eq!(run_a.source_digest(), run_a_again.source_digest());
        assert_ne!(
            run_a.source_digest(),
            run_b.source_digest(),
            "same claimed identity over different content must not share a digest"
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

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Disabled);
        assert!(run.is_publishable(), "disabled is the deliberate configured contribution");
        assert_eq!(run.work().rules_evaluated, 0, "no rule may run while disabled");
        assert!(run.findings().is_empty());
        assert!(run.producer_findings().is_empty());
        assert_eq!(run.owning_root(), None);
    }

    #[test]
    fn stale_disabled_run_can_never_publish_over_newer_configuration() {
        // Disabled → Native race falsifier (#9062): configuration moved while
        // this run was in flight, so the empty disabled contribution is
        // superseded exactly like any evaluated run.
        let source = STRICT_SOURCE;
        let ast = parse(source);
        let closed = RunGate::new(&|| false);

        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///disabled-to-native.pm",
            critic_source_identity_for_uri("file:///disabled-to-native.pm", 1),
            &ast,
            source,
            EffectiveCriticState::Disabled,
            Vec::new(),
            RunGate::open(),
            closed,
        ));

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Stale);
        assert!(
            !run.is_publishable(),
            "a stale disabled contribution can never clear newer current storage"
        );
    }

    #[test]
    fn cancelled_disabled_run_performs_no_rule_work_and_stays_unpublishable() {
        let source = STRICT_SOURCE;
        let ast = parse(source);
        let closed = RunGate::new(&|| false);

        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///cancelled-disabled.pm",
            critic_source_identity_for_uri("file:///cancelled-disabled.pm", 1),
            &ast,
            source,
            EffectiveCriticState::Disabled,
            Vec::new(),
            closed,
            RunGate::open(),
        ));

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Cancelled);
        assert!(!run.is_publishable());
        assert_eq!(run.work().rules_evaluated, 0);
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
        assert_ne!(run_a.state_fingerprint(), run_b.state_fingerprint());
        assert_eq!(run_a.owning_root(), Some("root-a"));
        assert_eq!(run_b.owning_root(), None);
        assert!(
            run_b.findings().len() < run_a.findings().len(),
            "root B's own threshold-5 policy must not be satisfied by root A's findings"
        );
    }

    #[test]
    fn movement_after_analysis_makes_the_run_stale_and_unpublishable() {
        let source = STRICT_SOURCE;
        let ast = parse(source);

        let closed = RunGate::new(&|| false);
        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///moved.pm",
            critic_source_identity_for_uri("file:///moved.pm", 4),
            &ast,
            source,
            native_state(Some("root-a")),
            Vec::new(),
            RunGate::open(),
            closed,
        ));

        assert!(
            !run.findings().is_empty(),
            "stale runs still carry their evaluated rows as values"
        );
        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Stale);
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
        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///cancelled.pm",
            critic_source_identity_for_uri("file:///cancelled.pm", 2),
            &ast,
            source,
            native_state(None),
            Vec::new(),
            closed,
            RunGate::open(),
        ));

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Cancelled);
        assert!(!run.is_publishable());
        assert_eq!(run.work().rules_evaluated, 0);
        assert!(run.producer_findings().is_empty());
    }

    #[test]
    fn cancellation_during_evaluation_yields_cancelled_after_real_work() {
        // Mid-flight falsifier (review #12067): the gate answers true at the
        // pre-evaluation consult and false at the post-evaluation settlement
        // barrier, so rules genuinely ran yet the run must settle Cancelled
        // instead of publishing Complete/Partial.
        use std::cell::Cell;

        let source = STRICT_SOURCE;
        let ast = parse(source);

        let consults = Cell::new(0);
        let closes_mid_flight = || {
            consults.set(consults.get() + 1);
            consults.get() < 2
        };

        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///cancelled-mid-flight.pm",
            critic_source_identity_for_uri("file:///cancelled-mid-flight.pm", 2),
            &ast,
            source,
            native_state(Some("root-a")),
            Vec::new(),
            RunGate::new(&closes_mid_flight),
            RunGate::open(),
        ));

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Cancelled);
        assert!(
            !run.is_publishable(),
            "a run cancelled during evaluation can never populate current storage"
        );
        assert!(
            run.work().rules_evaluated > 0,
            "the pre-work consult passed, so rule evaluation really ran"
        );
        assert!(
            !run.findings().is_empty(),
            "cancelled-after-work runs keep their evaluated rows as values"
        );
    }

    #[test]
    fn cancellation_outranks_staleness_at_the_settlement_barrier() {
        // Explicit precedence pin (review #12067): when both gates close
        // during the run, the disposition names the caller's abandonment,
        // not mere staleness.
        use std::cell::Cell;

        let source = STRICT_SOURCE;
        let ast = parse(source);

        let consults = Cell::new(0);
        let closes_mid_flight = || {
            consults.set(consults.get() + 1);
            consults.get() < 2
        };
        let moved = RunGate::new(&|| false);

        let run = NativeCriticService::analyze(NativeCriticSubject::accepted(
            "file:///cancelled-and-moved.pm",
            critic_source_identity_for_uri("file:///cancelled-and-moved.pm", 2),
            &ast,
            source,
            native_state(Some("root-a")),
            Vec::new(),
            RunGate::new(&closes_mid_flight),
            moved,
        ));

        assert_eq!(run.completeness(), &NativeCriticRunCompleteness::Cancelled);
        assert!(!run.is_publishable());
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
