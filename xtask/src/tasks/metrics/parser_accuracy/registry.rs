//! Canonical parser-accuracy metric registry.
//!
//! `.ci/schemas/parser-accuracy.schema.json` stays structural: it types `metric` as any
//! non-empty string, so it cannot tell a renamed metric from a dropped one, nor a ratio
//! reported as `1.4` from a legitimate value. The registry authored in
//! `.ci/policies/parser-accuracy-metrics.toml` carries that meaning instead — one row per
//! canonical metric ID declaring its evidence plane, value kind, valid range, which
//! direction is better, whether it carries a zero budget, how many samples it needs before
//! it may be measured at all or claim high confidence, and which cadences may emit it.
//!
//! The registry is consumptive rather than decorative:
//!
//! * [`MetricRegistry::apply`] derives every emitted row's `direction` and `confidence`
//!   from the registry plus the row's actual `sample_count`, replacing what used to be a
//!   hardcoded `Direction::Neutral` / `Confidence::High` on every measured row.
//! * [`MetricRegistry::validate_conformance`] then fails the artifact build closed when a
//!   row and its registry entry disagree.
//!
//! # Scope of the `direction` field
//!
//! This registry owns the `direction` published in the scorecard artifact. It is **not**
//! the authority the ratchet uses: `super::ratchet` independently infers higher/lower-is-
//! better from a metric-name suffix convention (`_count`, `_nodes`, `_unreadable`) plus a
//! per-baseline `lower_is_better` override list, and reads nothing from here. The two agree
//! for every metric currently in `.ci/metrics/baselines/parser_accuracy.json`, but they
//! disagree for a number of metrics not yet baselined — notably the `*_ms_p95` durations,
//! `peak_rss_mb`, `allocated_bytes`, and the `symbols_emitted_in_*` rows, which this
//! registry calls `down` while the suffix heuristic would call them `up`.
//!
//! So adding one of those to a ratchet baseline requires setting `lower_is_better` there as
//! well; this registry will not do it for you. Reconciling the two onto one authority is
//! tracked on parent #8189 and deliberately out of scope here, so that this change moves no
//! existing gate.
//!
//! Controlling issue: #14553 (parent #8189).

use std::collections::{BTreeMap, BTreeSet};

use color_eyre::eyre::{Result, bail};
use serde::Deserialize;

use super::{Cadence, Confidence, Direction, MetricRow};

/// Repository-relative path of the authored registry, for diagnostics and tests.
pub(super) const REGISTRY_PATH: &str = ".ci/policies/parser-accuracy-metrics.toml";

/// Evidence planes a metric may be assigned to.
///
/// A closed vocabulary so a typo or an invented plane fails the registry's own validation
/// rather than becoming a silently accepted ownership label. Emitted rows carry no family
/// field, so this is the only place the label can be checked.
const KNOWN_FAMILIES: &[&str] = &[
    "ast",
    "cache_reuse",
    "confidence",
    "cost",
    "denominator",
    "determinism",
    "gold_drift",
    "incremental",
    "line",
    "provider",
    "recovery",
    "runtime",
    "safety",
    "scale",
    "span",
    "symbol",
    "unsupported",
];

/// The authored registry, embedded so the compiled validator and the reviewed artifact
/// cannot drift apart.
const REGISTRY_SOURCE: &str =
    include_str!("../../../../../.ci/policies/parser-accuracy-metrics.toml");

/// Value shape of a metric, which fixes its natural range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MetricKind {
    /// A fraction constrained to `0.0..=1.0`.
    Ratio,
    /// A non-negative tally of occurrences.
    Count,
    /// A duration in milliseconds.
    DurationMs,
    /// A size in bytes.
    Bytes,
    /// A non-negative descriptive quantity that is none of the above.
    Scalar,
}

/// One reviewed registry row.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct MetricPolicy {
    /// Canonical, stable metric identifier.
    pub(super) name: String,
    /// Evidence plane that owns the metric.
    pub(super) family: String,
    /// Value shape.
    pub(super) kind: MetricKind,
    /// Which way is better.
    pub(super) direction: Direction,
    /// Inclusive lower bound for a measured value.
    pub(super) min: f64,
    /// Inclusive upper bound for a measured value, when the metric is bounded.
    #[serde(default)]
    pub(super) max: Option<f64>,
    /// A measured value other than zero is a hard violation.
    #[serde(default)]
    pub(super) zero_budget: bool,
    /// Below this sample count the row must be reported as `insufficient_data`.
    pub(super) min_sample_count: u64,
    /// Below this sample count the row must not claim `high` confidence.
    pub(super) high_confidence_sample_count: u64,
    /// Cadences this metric may be emitted at.
    pub(super) cadences: Vec<Cadence>,
}

impl MetricPolicy {
    /// Confidence this policy permits for an observation backed by `sample_count` samples.
    fn permitted_confidence(&self, sample_count: u64) -> Confidence {
        if sample_count >= self.high_confidence_sample_count {
            Confidence::High
        } else {
            Confidence::Low
        }
    }

    /// Whether `value` lies inside the declared range.
    fn accepts_value(&self, value: f64) -> bool {
        if !value.is_finite() {
            return false;
        }
        if value < self.min {
            return false;
        }
        match self.max {
            Some(max) => value <= max,
            None => true,
        }
    }

    /// Whether `value` has the shape the declared kind implies.
    ///
    /// A tally of occurrences or a size in bytes is a whole number; a fractional one means
    /// the emitter computed an average or a rate and reported it under a counting metric.
    /// Ratios are constrained by their declared range instead, and durations and scalars are
    /// legitimately fractional.
    fn accepts_shape(&self, value: f64) -> bool {
        match self.kind {
            MetricKind::Count | MetricKind::Bytes => value.fract() == 0.0,
            MetricKind::Ratio | MetricKind::DurationMs | MetricKind::Scalar => true,
        }
    }

    /// Human-readable rendering of the declared range, for failure messages.
    fn range_label(&self) -> String {
        match self.max {
            Some(max) => format!("{}..={}", self.min, max),
            None => format!("{}..", self.min),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RegistryFile {
    schema_version: u32,
    #[serde(default)]
    metric: Vec<MetricPolicy>,
}

/// The parsed, self-validated registry.
#[derive(Debug, Clone)]
pub(super) struct MetricRegistry {
    policies: BTreeMap<String, MetricPolicy>,
}

impl MetricRegistry {
    /// Load the registry embedded at compile time from [`REGISTRY_PATH`].
    pub(super) fn load() -> Result<Self> {
        Self::parse(REGISTRY_SOURCE)
    }

    /// Parse and self-validate a registry document.
    ///
    /// An incoherent registry is rejected here rather than silently governing nothing:
    /// a duplicate metric ID, an inverted range, a ratio declared outside `0.0..=1.0`, a
    /// zero `min_sample_count`, a high-confidence threshold below the measurement
    /// threshold, or an empty cadence set all fail closed.
    pub(super) fn parse(source: &str) -> Result<Self> {
        let file: RegistryFile = toml::from_str(source)
            .map_err(|err| color_eyre::eyre::eyre!("{REGISTRY_PATH} is not valid TOML: {err}"))?;

        if file.schema_version != 1 {
            bail!("{REGISTRY_PATH} schema_version must be 1, found {}", file.schema_version);
        }
        if file.metric.is_empty() {
            bail!("{REGISTRY_PATH} declares no metrics");
        }

        let mut policies = BTreeMap::new();
        for policy in file.metric {
            if policy.name.trim().is_empty() {
                bail!("{REGISTRY_PATH} declares a metric with an empty name");
            }
            if !KNOWN_FAMILIES.contains(&policy.family.as_str()) {
                bail!(
                    "{REGISTRY_PATH} metric '{}' declares unknown family '{}'; expected one of {}",
                    policy.name,
                    policy.family,
                    KNOWN_FAMILIES.join(", ")
                );
            }
            if policy.cadences.is_empty() {
                bail!("{REGISTRY_PATH} metric '{}' declares no eligible cadences", policy.name);
            }
            if policy.min_sample_count == 0 {
                bail!(
                    "{REGISTRY_PATH} metric '{}' declares min_sample_count 0; a measured row \
                     always has at least one sample",
                    policy.name
                );
            }
            if policy.high_confidence_sample_count < policy.min_sample_count {
                bail!(
                    "{REGISTRY_PATH} metric '{}' declares high_confidence_sample_count {} below \
                     min_sample_count {}",
                    policy.name,
                    policy.high_confidence_sample_count,
                    policy.min_sample_count
                );
            }
            if let Some(max) = policy.max
                && max < policy.min
            {
                bail!(
                    "{REGISTRY_PATH} metric '{}' declares an inverted range {}..={}",
                    policy.name,
                    policy.min,
                    max
                );
            }
            if policy.kind == MetricKind::Ratio && (policy.min < 0.0 || policy.max != Some(1.0)) {
                bail!(
                    "{REGISTRY_PATH} ratio metric '{}' must declare a range inside 0.0..=1.0, \
                     found {}",
                    policy.name,
                    policy.range_label()
                );
            }
            if policies.insert(policy.name.clone(), policy.clone()).is_some() {
                bail!("{REGISTRY_PATH} declares metric '{}' more than once", policy.name);
            }
        }

        Ok(Self { policies })
    }

    /// Look up one metric's policy.
    pub(super) fn policy(&self, metric: &str) -> Option<&MetricPolicy> {
        self.policies.get(metric)
    }

    /// Number of registered metrics.
    pub(super) fn len(&self) -> usize {
        self.policies.len()
    }

    /// Derive `direction` and `confidence` for every measured row from the registry and
    /// the row's own `sample_count`.
    ///
    /// This is what makes the registry load-bearing rather than documentation: before it,
    /// every measured row was published as `direction: neutral, confidence: high`
    /// regardless of how few samples backed it.
    pub(super) fn apply(&self, metrics: &mut [MetricRow]) -> Result<()> {
        for row in metrics.iter_mut() {
            match row {
                MetricRow::Measured { metric, sample_count, direction, confidence, .. } => {
                    let Some(policy) = self.policies.get(metric.as_str()) else {
                        bail!(
                            "parser accuracy emitted unregistered metric '{metric}'; add it to \
                             {REGISTRY_PATH}"
                        );
                    };
                    *direction = policy.direction;
                    *confidence = policy.permitted_confidence(*sample_count);
                }
                MetricRow::InsufficientData { metric, confidence, .. } => {
                    if !self.policies.contains_key(metric.as_str()) {
                        bail!(
                            "parser accuracy emitted unregistered metric '{metric}'; add it to \
                             {REGISTRY_PATH}"
                        );
                    }
                    *confidence = Confidence::Low;
                }
            }
        }
        Ok(())
    }

    /// Fail closed when an emitted row and its registry entry disagree.
    pub(super) fn validate_conformance(&self, metrics: &[MetricRow]) -> Result<()> {
        let mut seen: BTreeSet<&str> = BTreeSet::new();

        for row in metrics {
            let metric = row.name();
            if !seen.insert(metric) {
                bail!("parser accuracy emitted metric '{metric}' more than once");
            }

            let Some(policy) = self.policies.get(metric) else {
                bail!(
                    "parser accuracy emitted unregistered metric '{metric}'; add it to \
                     {REGISTRY_PATH}"
                );
            };

            let MetricRow::Measured { value, sample_count, direction, confidence, cadence, .. } =
                row
            else {
                continue;
            };

            if !policy.accepts_value(*value) {
                bail!(
                    "parser accuracy metric '{metric}' reported {value}, outside its declared \
                     range {}",
                    policy.range_label()
                );
            }
            if !policy.accepts_shape(*value) {
                bail!(
                    "parser accuracy metric '{metric}' is declared {:?} but reported the \
                     fractional value {value}",
                    policy.kind
                );
            }
            if policy.zero_budget && *value != 0.0 {
                bail!(
                    "parser accuracy metric '{metric}' carries a zero budget but reported {value}"
                );
            }
            if *sample_count < policy.min_sample_count {
                bail!(
                    "parser accuracy metric '{metric}' is measured from {sample_count} sample(s) \
                     but requires {}; it must be reported as insufficient_data",
                    policy.min_sample_count
                );
            }
            let permitted = policy.permitted_confidence(*sample_count);
            if *confidence != permitted {
                // Checked in both directions. An over-claim publishes a stronger result than
                // the samples support; an under-claim is a quieter disagreement that still
                // means the row and the registry were derived from different rules.
                bail!(
                    "parser accuracy metric '{metric}' reports {confidence:?} confidence from \
                     {sample_count} sample(s), but the registry permits {permitted:?} \
                     (high requires {})",
                    policy.high_confidence_sample_count
                );
            }
            if !policy.cadences.contains(cadence) {
                bail!(
                    "parser accuracy metric '{metric}' was emitted at an ineligible cadence \
                     {cadence:?}"
                );
            }
            if *direction != policy.direction {
                bail!(
                    "parser accuracy metric '{metric}' reported direction {direction:?} but the \
                     registry declares {:?}",
                    policy.direction
                );
            }
        }

        Ok(())
    }

    /// Fail closed when a registered metric was not emitted at all.
    ///
    /// This is the completeness half of the denominator, and it only holds for a run over
    /// the canonical fixture manifest: a deliberately reduced manifest legitimately scores
    /// fewer planes, so callers pass a reduced artifact to
    /// [`MetricRegistry::validate_conformance`] alone.
    pub(super) fn validate_completeness(&self, metrics: &[MetricRow]) -> Result<()> {
        let emitted: BTreeSet<&str> = metrics.iter().map(MetricRow::name).collect();
        for name in self.policies.keys() {
            if !emitted.contains(name.as_str()) {
                bail!(
                    "parser accuracy did not emit registered metric '{name}'; remove it from \
                     {REGISTRY_PATH} or restore its emission"
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(metric: &str, value: f64, sample_count: u64) -> MetricRow {
        MetricRow::Measured {
            metric: metric.to_string(),
            value,
            previous: None,
            delta: None,
            floor: None,
            threshold: None,
            sample_count,
            direction: Direction::Up,
            confidence: Confidence::High,
            cadence: Cadence::Pr,
            macro_value: None,
            micro_value: None,
        }
    }

    /// A single-metric registry used to build focused falsifiers.
    fn one_metric_registry(body: &str) -> Result<MetricRegistry> {
        MetricRegistry::parse(&format!("schema_version = 1\n\n[[metric]]\n{body}"))
    }

    fn ratio_registry() -> MetricRegistry {
        one_metric_registry(
            r#"name = "sample_rate"
family = "line"
kind = "ratio"
direction = "up"
min = 0.0
max = 1.0
zero_budget = false
min_sample_count = 2
high_confidence_sample_count = 5
cadences = ["pr"]
"#,
        )
        .expect("fixture registry is coherent")
    }

    #[test]
    fn authored_registry_is_coherent_and_non_empty() {
        let registry = MetricRegistry::load().expect("authored registry must parse");
        assert!(registry.len() > 100, "registry governs {} metrics", registry.len());
    }

    #[test]
    fn apply_derives_direction_and_confidence_from_sample_count() {
        let registry = ratio_registry();
        let mut rows = vec![measured("sample_rate", 0.5, 2)];
        registry.apply(&mut rows).expect("registered metric applies");

        let MetricRow::Measured { direction, confidence, .. } = &rows[0] else {
            panic!("expected a measured row");
        };
        assert_eq!(*direction, Direction::Up);
        assert_eq!(
            *confidence,
            Confidence::Low,
            "2 samples is below the declared high-confidence threshold of 5"
        );
    }

    #[test]
    fn apply_permits_high_confidence_at_the_declared_threshold() {
        let registry = ratio_registry();
        let mut rows = vec![measured("sample_rate", 0.5, 5)];
        registry.apply(&mut rows).expect("registered metric applies");

        let MetricRow::Measured { confidence, .. } = &rows[0] else {
            panic!("expected a measured row");
        };
        assert_eq!(*confidence, Confidence::High);
    }

    #[test]
    fn unregistered_metric_is_rejected() {
        let registry = ratio_registry();
        let rows = vec![measured("not_registered", 0.5, 9)];
        let err = registry.validate_conformance(&rows).expect_err("unknown metric must fail");
        assert!(err.to_string().contains("unregistered metric 'not_registered'"), "{err}");
    }

    #[test]
    fn unemitted_registered_metric_is_rejected() {
        let registry = ratio_registry();
        let err = registry.validate_completeness(&[]).expect_err("missing metric must fail");
        assert!(err.to_string().contains("did not emit registered metric 'sample_rate'"), "{err}");
    }

    #[test]
    fn conformance_alone_tolerates_a_reduced_manifest() {
        // A deliberately reduced fixture manifest scores fewer planes. That is a smaller
        // artifact, not a violation, so soundness must pass where completeness would not.
        let registry = ratio_registry();
        registry.validate_conformance(&[]).expect("a reduced artifact is still sound");
        assert!(registry.validate_completeness(&[]).is_err());
    }

    #[test]
    fn duplicate_metric_row_is_rejected() {
        let registry = ratio_registry();
        let rows = vec![measured("sample_rate", 0.5, 9), measured("sample_rate", 0.6, 9)];
        let err = registry.validate_conformance(&rows).expect_err("duplicate row must fail");
        assert!(err.to_string().contains("more than once"), "{err}");
    }

    #[test]
    fn out_of_range_value_is_rejected() {
        let registry = ratio_registry();
        let rows = vec![measured("sample_rate", 1.4, 9)];
        let err = registry.validate_conformance(&rows).expect_err("out-of-range must fail");
        assert!(err.to_string().contains("outside its declared range 0..=1"), "{err}");
    }

    #[test]
    fn zero_budget_violation_is_rejected() {
        let registry = one_metric_registry(
            r#"name = "false_exact_count"
family = "safety"
kind = "count"
direction = "down"
min = 0.0
zero_budget = true
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect("fixture registry is coherent");

        let mut row = measured("false_exact_count", 1.0, 9);
        if let MetricRow::Measured { direction, .. } = &mut row {
            *direction = Direction::Down;
        }
        let err = registry.validate_conformance(&[row]).expect_err("zero budget must fail");
        assert!(err.to_string().contains("carries a zero budget but reported 1"), "{err}");
    }

    #[test]
    fn under_sampled_measured_row_is_rejected() {
        let registry = ratio_registry();
        let rows = vec![measured("sample_rate", 0.5, 1)];
        let err = registry.validate_conformance(&rows).expect_err("under-sampled must fail");
        assert!(err.to_string().contains("must be reported as insufficient_data"), "{err}");
    }

    #[test]
    fn over_claimed_confidence_is_rejected() {
        let registry = ratio_registry();
        // 3 samples clears min_sample_count (2) but not high_confidence_sample_count (5),
        // so the hardcoded `Confidence::High` on this row is now a violation.
        let rows = vec![measured("sample_rate", 0.5, 3)];
        let err = registry.validate_conformance(&rows).expect_err("over-claim must fail");
        assert!(err.to_string().contains("reports High confidence from 3 sample(s)"), "{err}");
    }

    #[test]
    fn under_claimed_confidence_is_rejected() {
        // The opposite direction. An under-claim is not "safe": it still means the row was
        // derived from a different rule than the registry's, which is the drift this check
        // exists to catch.
        let registry = ratio_registry();
        let mut row = measured("sample_rate", 0.5, 9);
        if let MetricRow::Measured { confidence, .. } = &mut row {
            *confidence = Confidence::Low;
        }
        let err = registry.validate_conformance(&[row]).expect_err("under-claim must fail");
        assert!(err.to_string().contains("reports Low confidence from 9 sample(s)"), "{err}");
    }

    #[test]
    fn ineligible_cadence_is_rejected() {
        let registry = ratio_registry();
        let mut row = measured("sample_rate", 0.5, 9);
        if let MetricRow::Measured { cadence, .. } = &mut row {
            *cadence = Cadence::Release;
        }
        let err = registry.validate_conformance(&[row]).expect_err("bad cadence must fail");
        assert!(err.to_string().contains("ineligible cadence"), "{err}");
    }

    #[test]
    fn direction_disagreement_is_rejected() {
        let registry = ratio_registry();
        let mut row = measured("sample_rate", 0.5, 9);
        if let MetricRow::Measured { direction, .. } = &mut row {
            *direction = Direction::Down;
        }
        let err = registry.validate_conformance(&[row]).expect_err("bad direction must fail");
        assert!(err.to_string().contains("registry declares Up"), "{err}");
    }

    #[test]
    fn duplicate_registry_entry_is_rejected() {
        let err = MetricRegistry::parse(
            r#"schema_version = 1

[[metric]]
name = "dup"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]

[[metric]]
name = "dup"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect_err("duplicate registry entry must fail");
        assert!(err.to_string().contains("more than once"), "{err}");
    }

    #[test]
    fn inverted_registry_range_is_rejected() {
        let err = one_metric_registry(
            r#"name = "inverted"
family = "line"
kind = "count"
direction = "down"
min = 5.0
max = 1.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect_err("inverted range must fail");
        assert!(err.to_string().contains("inverted range"), "{err}");
    }

    #[test]
    fn ratio_declared_outside_unit_interval_is_rejected() {
        let err = one_metric_registry(
            r#"name = "bad_ratio"
family = "line"
kind = "ratio"
direction = "up"
min = 0.0
max = 100.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect_err("ratio outside 0..=1 must fail");
        assert!(err.to_string().contains("inside 0.0..=1.0"), "{err}");
    }

    #[test]
    fn high_confidence_threshold_below_measurement_threshold_is_rejected() {
        let err = one_metric_registry(
            r#"name = "incoherent"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 8
high_confidence_sample_count = 2
cadences = ["pr"]
"#,
        )
        .expect_err("incoherent thresholds must fail");
        assert!(err.to_string().contains("below min_sample_count"), "{err}");
    }

    #[test]
    fn fractional_count_value_is_rejected() {
        // A count is a tally. A fractional one means the emitter reported an average or a
        // rate under a counting metric, which the declared range alone cannot catch.
        let registry = one_metric_registry(
            r#"name = "widget_count"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect("fixture registry is coherent");

        let mut row = measured("widget_count", 2.5, 9);
        if let MetricRow::Measured { direction, .. } = &mut row {
            *direction = Direction::Down;
        }
        let err = registry.validate_conformance(&[row]).expect_err("fractional count must fail");
        assert!(err.to_string().contains("reported the fractional value 2.5"), "{err}");
    }

    #[test]
    fn whole_count_value_is_accepted() {
        let registry = one_metric_registry(
            r#"name = "widget_count"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect("fixture registry is coherent");

        let mut row = measured("widget_count", 3.0, 9);
        if let MetricRow::Measured { direction, .. } = &mut row {
            *direction = Direction::Down;
        }
        registry.validate_conformance(&[row]).expect("a whole count is well shaped");
    }

    #[test]
    fn fractional_duration_is_accepted() {
        // Durations are legitimately fractional; the shape rule must not over-reach.
        let registry = one_metric_registry(
            r#"name = "widget_ms_p95"
family = "cost"
kind = "duration_ms"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect("fixture registry is coherent");

        let mut row = measured("widget_ms_p95", 0.0155, 9);
        if let MetricRow::Measured { direction, .. } = &mut row {
            *direction = Direction::Down;
        }
        registry.validate_conformance(&[row]).expect("a fractional duration is well shaped");
    }

    #[test]
    fn unknown_family_is_rejected() {
        let err = one_metric_registry(
            r#"name = "orphan"
family = "not_a_plane"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = ["pr"]
"#,
        )
        .expect_err("unknown family must fail");
        assert!(err.to_string().contains("declares unknown family 'not_a_plane'"), "{err}");
    }

    #[test]
    fn empty_cadence_set_is_rejected() {
        let err = one_metric_registry(
            r#"name = "no_cadence"
family = "line"
kind = "count"
direction = "down"
min = 0.0
min_sample_count = 1
high_confidence_sample_count = 1
cadences = []
"#,
        )
        .expect_err("empty cadence set must fail");
        assert!(err.to_string().contains("no eligible cadences"), "{err}");
    }
}
