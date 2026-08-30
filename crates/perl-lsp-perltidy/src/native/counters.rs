//! Versioned deterministic work counters for the native formatting pipeline
//! (#10302).
//!
//! [`NativePipelineCounters`] records how much work one pass of the real
//! typed pipeline performed — parse-gate invocations, gate-observed AST
//! nodes, rendered lines, fitted delimited groups, derived edits,
//! replacement bytes, peak nesting depth, and advisory wall-clock elapsed
//! under a named clock. The instrument is strictly additive: it is populated
//! only through [`PipelineCollectorScope`], which the counters-aware typed
//! entries install for the duration of one synchronous pipeline pass.
//! Default callers never install a scope, so production behavior is
//! byte-identical (NPC-001).
//!
//! Envelope movement (any growth-bound change) requires a major bump of
//! [`COUNTER_SCHEMA_V1`] plus exact before/after counter receipts; timing is
//! evidence, never a required gate (#3979/#5282).

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Schema tag of the counter instrument. Pinned by the NPC-001 canary; any
/// envelope movement requires a major bump plus before/after receipts.
pub const COUNTER_SCHEMA_V1: &str = "native-pipeline-counters-v1";

/// Named monotonic clock backing `NativePipelineCounters::elapsed`.
/// Recorded so unlike-environment timings are honestly labeled, never
/// compared (#3979/#5282).
pub const COUNTER_CLOCK_TAG: &str = "std-instant-monotonic-v1";

/// Schema-v1 derived-output envelope: maximum replacement bytes derived per
/// source byte before the growth guard trips. Deliberately generous against
/// the current ~1x document-replacement shape so only real expansion trips
/// it (#7140/#7501 product-bound alignment stays upstream).
pub const MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1: u64 = 4;

/// Schema-v1 scaling detector ratio bound: a counter series measured at
/// N / 2N / 4N is superlinear when `c(4N)` exceeds
/// `SCALING_RATIO_BOUND_V1 * c(2N)` by more than
/// [`SCALING_ABSOLUTE_SLACK_V1`]. The detector applies to observed counters,
/// not to uninstrumented fit/comparison operations or their algorithmic
/// complexity. Loosening either constant stops the synthetic quadratic
/// control from being flagged, which makes detector weakening itself
/// observable (NPC-004).
pub const SCALING_RATIO_BOUND_V1: u64 = 2;

/// Absolute slack of the schema-v1 scaling detector; absorbs the constant
/// term of linear series without masking quadratic growth above tiny sizes.
pub const SCALING_ABSOLUTE_SLACK_V1: u64 = 8;

/// Whether `replacement_bytes` exceeds the schema-v1 derived-output envelope
/// for `source_bytes` (NPC-006).
#[must_use]
pub fn exceeds_replacement_envelope_v1(source_bytes: u64, replacement_bytes: u64) -> bool {
    replacement_bytes > source_bytes.saturating_mul(MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1)
}

/// Deterministic, allocation-light work counters for one native formatting
/// pipeline pass. All fields saturate; `peak_depth` is a maximum-merge, every
/// other field is an additive merge, so reused collectors accumulate honestly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePipelineCounters {
    /// Typed pipeline entries (`format_*_typed`) recorded by this collector.
    pub pipeline_invocations: u64,
    /// Parse-gate invocations (`validate_clean_parse` / `validate_parse_only`).
    /// An applied document gates twice by design: source, then rendered output.
    pub parse_gate_invocations: u64,
    /// Source-input parse-gate invocations.
    pub source_parse_gate_invocations: u64,
    /// Formatted-output parse-gate invocations.
    pub formatted_output_parse_gate_invocations: u64,
    /// AST nodes observed by the parse gate (counted only while a collector
    /// is installed; never estimated post hoc).
    pub gate_nodes_observed: u64,
    /// Physical source lines processed by the render stage.
    pub lines_processed: u64,
    /// Delimited layout groups that rendered flat (fit on one line).
    pub delimited_groups_fitted: u64,
    /// Edits derived by the pipeline and returned to classification.
    pub edits_derived: u64,
    /// Replacement bytes carried by the derived edits.
    pub replacement_bytes: u64,
    /// Peak nesting depth observed (parse-gate depth and render group depth).
    pub peak_depth: u64,
    /// Total wall-clock elapsed under [`COUNTER_CLOCK_TAG`]. Advisory
    /// evidence only; never a required gate (#3979/#5282).
    pub elapsed: Duration,
}

impl NativePipelineCounters {
    /// Schema tag carried by this instrument.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        COUNTER_SCHEMA_V1
    }

    /// Named clock tag backing `elapsed`.
    #[must_use]
    pub const fn clock_tag(&self) -> &'static str {
        COUNTER_CLOCK_TAG
    }

    pub(crate) fn observe_pipeline_invocation(&mut self) {
        self.pipeline_invocations = self.pipeline_invocations.saturating_add(1);
    }

    pub(crate) fn observe_parse_gate(&mut self, kind: ParseGateKind, nodes: u64, parse_depth: u64) {
        self.parse_gate_invocations = self.parse_gate_invocations.saturating_add(1);
        match kind {
            ParseGateKind::Source => {
                self.source_parse_gate_invocations =
                    self.source_parse_gate_invocations.saturating_add(1);
            }
            ParseGateKind::FormattedOutput => {
                self.formatted_output_parse_gate_invocations =
                    self.formatted_output_parse_gate_invocations.saturating_add(1);
            }
        }
        self.gate_nodes_observed = self.gate_nodes_observed.saturating_add(nodes);
        self.peak_depth = self.peak_depth.max(parse_depth);
    }

    pub(crate) fn observe_lines(&mut self, lines: u64) {
        self.lines_processed = self.lines_processed.saturating_add(lines);
    }

    pub(crate) fn observe_render_depth(&mut self, depth: u64) {
        self.peak_depth = self.peak_depth.max(depth);
    }

    pub(crate) fn observe_group_fit(&mut self, depth: u64) {
        self.delimited_groups_fitted = self.delimited_groups_fitted.saturating_add(1);
        self.peak_depth = self.peak_depth.max(depth);
    }

    pub(crate) fn observe_edits_derived(&mut self, edits: u64, replacement_bytes: u64) {
        self.edits_derived = self.edits_derived.saturating_add(edits);
        self.replacement_bytes = self.replacement_bytes.saturating_add(replacement_bytes);
    }

    pub(crate) fn observe_elapsed(&mut self, elapsed: Duration) {
        self.elapsed = self.elapsed.saturating_add(elapsed);
    }
}

/// Parse-gate identity used to keep source and formatted-output validation
/// distinguishable in receipts and mutation controls.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ParseGateKind {
    /// The original source supplied by the caller.
    Source,
    /// The formatted output before edits are authorized.
    FormattedOutput,
}

thread_local! {
    /// Collector installed for the current thread's synchronous pipeline pass.
    /// The native pipeline is fully synchronous, so the scope is exact; the
    /// `None` default keeps every production caller byte-identical.
    static ACTIVE_COLLECTOR: RefCell<Option<NativePipelineCounters>> =
        const { RefCell::new(None) };
}

/// Record one observation into the active collector, if any. When no scope is
/// installed this is a no-op — the zero-effect-when-unset contract.
pub(crate) fn record_with(update: impl FnOnce(&mut NativePipelineCounters)) {
    ACTIVE_COLLECTOR.with(|cell| {
        if let Some(counters) = cell.borrow_mut().as_mut() {
            update(counters);
        }
    });
}

/// Count the nodes of a parse-gate AST with an explicit stack (no recursion on
/// hostile inputs). Only invoked while a collector is installed.
pub(crate) fn count_parse_nodes(root: &perl_parser_core::Node) -> u64 {
    let mut counted = 0_u64;
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        counted = counted.saturating_add(1);
        stack.extend(node.children());
    }
    counted
}

/// RAII installation of the pipeline collector for the current thread.
///
/// Installed by the counters-aware typed entries around exactly one pipeline
/// pass; nested installs restore the previous state on drop.
pub struct PipelineCollectorScope {
    previous: Option<NativePipelineCounters>,
    active: bool,
    // The collector lives in thread-local storage. Keep the RAII guard
    // thread-affine as well, so it cannot be moved to another thread and
    // accidentally restore or detach that thread's unrelated collector.
    _thread_affine: PhantomData<Rc<()>>,
}

impl PipelineCollectorScope {
    /// Install a fresh collector scope for the current thread.
    pub fn install() -> Self {
        let previous = ACTIVE_COLLECTOR
            .with(|cell| cell.borrow_mut().replace(NativePipelineCounters::default()));
        Self { previous, active: true, _thread_affine: PhantomData }
    }

    /// Fold everything recorded inside the scope into `counters` and restore
    /// the previous scope state. If a wider scope already exists, the nested
    /// observations are merged into both the supplied snapshot and that wider
    /// scope so either consumer sees the complete nested pass.
    pub fn merge_into(mut self, counters: &mut NativePipelineCounters) {
        let recorded = ACTIVE_COLLECTOR.with(|cell| cell.borrow_mut().take());
        let mut previous = self.detach();
        let recorded = recorded.unwrap_or_default();
        if let Some(previous) = previous.as_mut() {
            merge_counters(previous, &recorded);
        }
        ACTIVE_COLLECTOR.with(|cell| *cell.borrow_mut() = previous);

        merge_counters(counters, &recorded);
    }

    fn detach(&mut self) -> Option<NativePipelineCounters> {
        self.active = false;
        self.previous.take()
    }
}

fn merge_counters(counters: &mut NativePipelineCounters, recorded: &NativePipelineCounters) {
    counters.pipeline_invocations =
        counters.pipeline_invocations.saturating_add(recorded.pipeline_invocations);
    counters.parse_gate_invocations =
        counters.parse_gate_invocations.saturating_add(recorded.parse_gate_invocations);
    counters.source_parse_gate_invocations = counters
        .source_parse_gate_invocations
        .saturating_add(recorded.source_parse_gate_invocations);
    counters.formatted_output_parse_gate_invocations = counters
        .formatted_output_parse_gate_invocations
        .saturating_add(recorded.formatted_output_parse_gate_invocations);
    counters.gate_nodes_observed =
        counters.gate_nodes_observed.saturating_add(recorded.gate_nodes_observed);
    counters.lines_processed = counters.lines_processed.saturating_add(recorded.lines_processed);
    counters.delimited_groups_fitted =
        counters.delimited_groups_fitted.saturating_add(recorded.delimited_groups_fitted);
    counters.edits_derived = counters.edits_derived.saturating_add(recorded.edits_derived);
    counters.replacement_bytes =
        counters.replacement_bytes.saturating_add(recorded.replacement_bytes);
    counters.peak_depth = counters.peak_depth.max(recorded.peak_depth);
    counters.elapsed = counters.elapsed.saturating_add(recorded.elapsed);
}

impl Drop for PipelineCollectorScope {
    fn drop(&mut self) {
        if self.active {
            let previous = self.detach();
            ACTIVE_COLLECTOR.with(|cell| *cell.borrow_mut() = previous);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NativePipelineCounters, PipelineCollectorScope, record_with};
    use std::time::Duration;

    #[test]
    fn scope_merge_preserves_every_counter_field() {
        let scope = PipelineCollectorScope::install();
        record_with(|recorded| {
            recorded.pipeline_invocations = 1;
            recorded.parse_gate_invocations = 2;
            recorded.source_parse_gate_invocations = 1;
            recorded.formatted_output_parse_gate_invocations = 1;
            recorded.gate_nodes_observed = 3;
            recorded.lines_processed = 4;
            recorded.delimited_groups_fitted = 5;
            recorded.edits_derived = 6;
            recorded.replacement_bytes = 7;
            recorded.peak_depth = 8;
            recorded.elapsed = Duration::from_nanos(9);
        });

        let mut counters = NativePipelineCounters::default();
        scope.merge_into(&mut counters);

        assert_eq!(counters.pipeline_invocations, 1);
        assert_eq!(counters.parse_gate_invocations, 2);
        assert_eq!(counters.source_parse_gate_invocations, 1);
        assert_eq!(counters.formatted_output_parse_gate_invocations, 1);
        assert_eq!(counters.gate_nodes_observed, 3);
        assert_eq!(counters.lines_processed, 4);
        assert_eq!(counters.delimited_groups_fitted, 5);
        assert_eq!(counters.edits_derived, 6);
        assert_eq!(counters.replacement_bytes, 7);
        assert_eq!(counters.peak_depth, 8);
        assert_eq!(counters.elapsed, Duration::from_nanos(9));
    }

    #[test]
    fn scope_merge_saturates_additive_counter_fields() {
        let scope = PipelineCollectorScope::install();
        record_with(|recorded| {
            recorded.pipeline_invocations = 1;
            recorded.parse_gate_invocations = 1;
            recorded.source_parse_gate_invocations = 1;
            recorded.formatted_output_parse_gate_invocations = 1;
            recorded.gate_nodes_observed = 1;
            recorded.lines_processed = 1;
            recorded.delimited_groups_fitted = 1;
            recorded.edits_derived = 1;
            recorded.replacement_bytes = 1;
            recorded.elapsed = Duration::from_nanos(1);
        });

        let mut counters = NativePipelineCounters {
            pipeline_invocations: u64::MAX,
            parse_gate_invocations: u64::MAX,
            source_parse_gate_invocations: u64::MAX,
            formatted_output_parse_gate_invocations: u64::MAX,
            gate_nodes_observed: u64::MAX,
            lines_processed: u64::MAX,
            delimited_groups_fitted: u64::MAX,
            edits_derived: u64::MAX,
            replacement_bytes: u64::MAX,
            peak_depth: 0,
            elapsed: Duration::MAX,
        };
        scope.merge_into(&mut counters);

        assert_eq!(counters.pipeline_invocations, u64::MAX);
        assert_eq!(counters.parse_gate_invocations, u64::MAX);
        assert_eq!(counters.source_parse_gate_invocations, u64::MAX);
        assert_eq!(counters.formatted_output_parse_gate_invocations, u64::MAX);
        assert_eq!(counters.gate_nodes_observed, u64::MAX);
        assert_eq!(counters.lines_processed, u64::MAX);
        assert_eq!(counters.delimited_groups_fitted, u64::MAX);
        assert_eq!(counters.edits_derived, u64::MAX);
        assert_eq!(counters.replacement_bytes, u64::MAX);
        assert_eq!(counters.elapsed, Duration::MAX);
    }
}
