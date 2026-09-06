//! Native critic rule-work receipt named at registration fidelity (#13977).
//!
//! [`NativeCriticWorkReceipt::rules_registered`] stores the accepted-profile
//! include/exclude admission count. That is planned-work evidence, not a
//! producer-observed count of rules whose [`crate::tooling::perl_critic::CriticRule::check`] body
//! ran. A rule that executes cleanly and emits no finding still belonged to
//! the registered set; do not synthesize an "executed" count from findings.
//!
//! [`NativeCriticWorkReceipt::producer_evaluation_entered`] is the current
//! post-work discriminator: it records whether producer evaluation was
//! entered (the `check_unfiltered` barrier), so cancellation tests can prove
//! performed work without treating registration alone as completed
//! evaluation. Per-rule attempted/completed/skipped/reused counters remain
//! #9082 and must come from registry instrumentation, not from this receipt.

use super::{CriticConfig, NativeCriticRegistry};

/// Bounded rule-work counters for one native critic run.
///
/// Registration and producer-entry are independent. A planned-but-not-entered
/// receipt can have `rules_registered > 0` while
/// `producer_evaluation_entered == 0`; that combination must not be treated
/// as proof that evaluation reached the post-work barrier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeCriticWorkReceipt {
    /// Rules admitted by the accepted profile after include/exclude.
    ///
    /// Zero when the run skipped evaluation (disabled or cancelled before
    /// work). This is registration evidence, not an executed-rule cardinality.
    pub rules_registered: usize,
    /// How many times producer evaluation was entered for this run.
    ///
    /// Zero when skipped before work. A positive value is the current
    /// discriminator that evaluation reached the producer barrier. This is
    /// not a per-rule attempted/completed/skipped/reused count (#9082).
    pub producer_evaluation_entered: usize,
}

impl NativeCriticWorkReceipt {
    /// Receipt for a run that performed no native rule work.
    ///
    /// Disabled and pre-work-cancelled runs must use this (or `Default`) so
    /// they cannot report registered work.
    #[must_use]
    pub const fn skipped() -> Self {
        Self { rules_registered: 0, producer_evaluation_entered: 0 }
    }

    /// Planned registration for an accepted enabled profile, before producer
    /// evaluation is entered.
    ///
    /// The count is [`NativeCriticRegistry::enabled_rule_count`], not
    /// [`NativeCriticRegistry::len`] and not a findings length.
    #[must_use]
    pub fn planned(registry: &NativeCriticRegistry, config: &CriticConfig) -> Self {
        Self {
            rules_registered: registry.enabled_rule_count(config),
            producer_evaluation_entered: 0,
        }
    }

    /// Record that producer evaluation was entered for this run.
    ///
    /// Cancellation tests must assert this counter (or a later #9082
    /// execution counter), not [`Self::rules_registered`] alone.
    #[must_use]
    pub const fn record_evaluation_entered(mut self) -> Self {
        self.producer_evaluation_entered = self.producer_evaluation_entered.saturating_add(1);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::NativeCriticWorkReceipt;
    use crate::tooling::perl_critic::{
        CriticCategory, CriticConfig, CriticContext, CriticFinding, CriticFindingShape, CriticRule,
        NativeCriticRegistry, Severity,
    };

    struct SilentRule(&'static str);
    struct FlagRule {
        id: &'static str,
        called: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    struct EmittingRule(&'static str);

    impl FlagRule {
        fn new(id: &'static str) -> (Self, std::sync::Arc<std::sync::atomic::AtomicBool>) {
            let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            (Self { id, called: std::sync::Arc::clone(&called) }, called)
        }
    }

    impl CriticRule for SilentRule {
        fn id(&self) -> &'static str {
            self.0
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Syntax
        }

        fn default_severity(&self) -> Severity {
            Severity::Harsh
        }

        fn check(&self, _ctx: &CriticContext<'_>, _out: &mut Vec<CriticFinding>) {}
    }

    impl CriticRule for FlagRule {
        fn id(&self) -> &'static str {
            self.id
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Syntax
        }

        fn default_severity(&self) -> Severity {
            Severity::Harsh
        }

        fn check(&self, _ctx: &CriticContext<'_>, _out: &mut Vec<CriticFinding>) {
            self.called.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl CriticRule for EmittingRule {
        fn id(&self) -> &'static str {
            self.0
        }

        fn category(&self) -> CriticCategory {
            CriticCategory::Syntax
        }

        fn default_severity(&self) -> Severity {
            Severity::Harsh
        }

        fn check(&self, _ctx: &CriticContext<'_>, out: &mut Vec<CriticFinding>) {
            out.push(CriticFinding {
                rule_id: self.0.to_string(),
                category: CriticCategory::Syntax,
                severity: Severity::Harsh,
                range: perl_parser_core::position::Range::new(
                    perl_parser_core::position::Position::new(0, 1, 1),
                    perl_parser_core::position::Position::new(0, 1, 1),
                ),
                message: format!("{} finding", self.0),
                explanation: String::new(),
                suppression_key: self.0.to_string(),
                observed_shape: CriticFindingShape::General,
                related: Vec::new(),
                fix: None,
            });
        }
    }

    fn two_rule_registry() -> NativeCriticRegistry {
        NativeCriticRegistry::with_rules(vec![
            Box::new(SilentRule("rule.silent")),
            Box::new(EmittingRule("rule.emit")),
        ])
    }

    #[test]
    fn skipped_and_default_report_zero_registered_work() {
        assert_eq!(NativeCriticWorkReceipt::skipped(), NativeCriticWorkReceipt::default());
        let skipped = NativeCriticWorkReceipt::skipped();
        assert_eq!(skipped.rules_registered, 0);
        assert_eq!(skipped.producer_evaluation_entered, 0);
    }

    #[test]
    fn planned_count_matches_enabled_registration_not_findings() {
        let registry = two_rule_registry();
        let config = CriticConfig::default();
        let planned = NativeCriticWorkReceipt::planned(&registry, &config);

        assert_eq!(planned.rules_registered, 2);
        assert_eq!(planned.rules_registered, registry.enabled_rule_count(&config));
        assert_eq!(
            planned.producer_evaluation_entered, 0,
            "planning registration must not claim that evaluation was entered"
        );
    }

    #[test]
    fn exclude_reduces_registered_count_without_running_rules() {
        let (first, first_called) = FlagRule::new("rule.a");
        let (second, second_called) = FlagRule::new("rule.b");
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(first), Box::new(second)]);
        let full = CriticConfig::default();
        let excluded = CriticConfig { exclude: vec!["rule.b".to_string()], ..Default::default() };

        assert_eq!(NativeCriticWorkReceipt::planned(&registry, &full).rules_registered, 2);
        assert_eq!(NativeCriticWorkReceipt::planned(&registry, &excluded).rules_registered, 1);
        assert!(
            !first_called.load(std::sync::atomic::Ordering::SeqCst)
                && !second_called.load(std::sync::atomic::Ordering::SeqCst),
            "planned registration must not invoke CriticRule::check"
        );
    }

    #[test]
    fn include_whitelist_is_the_registered_set() {
        let registry = NativeCriticRegistry::with_rules(vec![
            Box::new(SilentRule("rule.a")),
            Box::new(SilentRule("rule.b")),
        ]);
        let config = CriticConfig { include: vec!["rule.a".to_string()], ..Default::default() };
        let planned = NativeCriticWorkReceipt::planned(&registry, &config);
        assert_eq!(planned.rules_registered, 1);
        assert_eq!(planned.rules_registered, registry.enabled_rule_count(&config));
    }

    #[test]
    fn clean_rule_still_counts_as_registered() {
        let registry = NativeCriticRegistry::with_rules(vec![Box::new(SilentRule("rule.clean"))]);
        let planned = NativeCriticWorkReceipt::planned(&registry, &CriticConfig::default());
        assert_eq!(
            planned.rules_registered, 1,
            "a clean rule must not be treated as unexecuted merely because it emitted no finding"
        );
    }

    #[test]
    fn registration_alone_is_not_post_work_evidence() {
        let registry = two_rule_registry();
        let planned = NativeCriticWorkReceipt::planned(&registry, &CriticConfig::default());
        assert!(
            planned.rules_registered > 0,
            "control: the planned receipt really carries registration"
        );
        assert_eq!(
            planned.producer_evaluation_entered, 0,
            "tests must not use rules_registered > 0 as the sole evidence that evaluation ran"
        );
    }

    #[test]
    fn entered_evaluation_is_the_post_work_discriminator() {
        let registry = two_rule_registry();
        let entered = NativeCriticWorkReceipt::planned(&registry, &CriticConfig::default())
            .record_evaluation_entered();
        assert_eq!(entered.rules_registered, 2);
        assert_eq!(entered.producer_evaluation_entered, 1);
    }

    #[test]
    fn skipped_stays_zero_even_when_a_profile_exists() {
        let registry = two_rule_registry();
        assert!(registry.enabled_rule_count(&CriticConfig::default()) > 0);
        let skipped = NativeCriticWorkReceipt::skipped();
        assert_eq!(skipped.rules_registered, 0);
        assert_eq!(skipped.producer_evaluation_entered, 0);
    }

    #[test]
    fn receipt_production_source_does_not_claim_execution() {
        let source = include_str!("work_receipt.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let lower = production.to_ascii_lowercase();
        let evaluated_name = format!("rules_{}", "evaluated");
        assert!(
            !production.contains(&evaluated_name),
            "no service receipt field may be named as evaluated cardinality"
        );
        assert!(
            !lower.contains("actually executed") && !lower.contains("rules the run actually"),
            "production docs must not claim that registration is execution"
        );
        assert!(
            production.contains("rules_registered"),
            "control: the production source must name the honest field"
        );
    }
}
