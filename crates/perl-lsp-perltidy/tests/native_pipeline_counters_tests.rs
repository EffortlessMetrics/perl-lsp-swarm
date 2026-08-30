//! Counter-shape canaries for the native formatter production pipeline
//! (#10302, acceptance rows NPC-001..NPC-010).
//!
//! These are ordinary deterministic package tests, not timing gates: all
//! assertions bind deterministic integer counters, structural workflow pins,
//! and exact subject identity. Wall-clock stays advisory per the standing
//! #3979/#5282 policy and no check in this file may be satisfied by
//! increasing a timeout, budget constant, size cap, or iteration bound
//! (NPC-010 ratchet).

#[path = "../benches/support/perf_subjects.rs"]
mod perf_subjects;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use perf_subjects::{SubjectSpec, identity_row_with_counters, toolchain_tag};
use perl_lsp_perltidy::native::{
    COUNTER_CLOCK_TAG, COUNTER_SCHEMA_V1, FormatConfig, FormatContext, FormatDisposition,
    FormatReasonCode, MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1, NativeFormatter,
    NativePipelineCounters, PipelineCollectorScope, SCALING_ABSOLUTE_SLACK_V1,
    SCALING_RATIO_BOUND_V1, TextPosition, TextRange, TypedFormatResult,
};

// ---------------------------------------------------------------------------
// NPC-001 — schema pin and zero-effect-when-unset
// ---------------------------------------------------------------------------

#[test]
fn counter_schema_is_pinned_v1() {
    assert_eq!(COUNTER_SCHEMA_V1, "native-pipeline-counters-v1");
    assert_eq!(COUNTER_CLOCK_TAG, "std-instant-monotonic-v1");
    assert_eq!(SCALING_RATIO_BOUND_V1, 2);
    assert_eq!(SCALING_ABSOLUTE_SLACK_V1, 8);
    let counters = NativePipelineCounters::default();
    assert_eq!(counters.schema(), COUNTER_SCHEMA_V1);
    assert_eq!(counters.clock_tag(), COUNTER_CLOCK_TAG);
}

#[test]
fn unset_collector_leaves_outcomes_byte_identical() {
    for family in perf_subjects::FAMILIES {
        let spec = SubjectSpec { family, line_ending: "lf", indent: "tabs", units: 4 };
        let source = spec.source();
        let config = FormatConfig::default();
        let context = FormatContext::new(Some("npc-001".to_string()), Some(7));

        let plain = NativeFormatter::new().format_document_typed(&source, &config, &context);
        let mut counters = NativePipelineCounters::default();
        let counted = NativeFormatter::new().format_document_typed_with_counters(
            &source,
            &config,
            &context,
            &mut counters,
        );

        assert_eq!(
            plain, counted,
            "attaching the collector must not change the document outcome for {family}"
        );

        let end_line = source.split_inclusive('\n').count() as u32;
        let range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(end_line, 0));
        let plain_range =
            NativeFormatter::new().format_range_typed(&source, range, &config, &context);
        let mut range_counters = NativePipelineCounters::default();
        let counted_range = NativeFormatter::new().format_range_typed_with_counters(
            &source,
            range,
            &config,
            &context,
            &mut range_counters,
        );

        assert_eq!(
            plain_range, counted_range,
            "attaching the collector must not change the range outcome for {family}"
        );
    }
}

// ---------------------------------------------------------------------------
// NPC-002 — every pipeline stage populates deterministic counters
// ---------------------------------------------------------------------------

#[test]
fn counters_populate_every_stage_from_production_path() {
    let spec = SubjectSpec { family: "delimited", line_ending: "lf", indent: "tabs", units: 4 };
    let source = spec.source();
    let mut counters = NativePipelineCounters::default();
    let typed = NativeFormatter::new().format_document_typed_with_counters(
        &source,
        &FormatConfig::default(),
        &FormatContext::default(),
        &mut counters,
    );

    // Source gate + post-format parse-preservation gate: exactly two parse-gate
    // invocations for one fully rendered pipeline pass.
    assert_eq!(counters.parse_gate_invocations, 2);
    assert_eq!(counters.source_parse_gate_invocations, 1);
    assert_eq!(counters.formatted_output_parse_gate_invocations, 1);
    assert!(
        counters.gate_nodes_observed > 0,
        "the parse gate must observe the AST nodes it authorized"
    );
    assert_eq!(
        counters.lines_processed as usize,
        source.split_inclusive('\n').count(),
        "every physical line fed to the render stage must be counted"
    );
    assert!(
        counters.delimited_groups_fitted > 0,
        "delimited subjects must exercise the delimited group fit decision"
    );
    assert!(counters.peak_depth > 0, "nesting depth must be observed");
    assert_eq!(
        counters.edits_derived as usize,
        typed.result.edits.len(),
        "edits_derived must observe the edits the pipeline actually derived"
    );
    let replacement_bytes: usize = typed.result.edits.iter().map(|edit| edit.new_text.len()).sum();
    assert_eq!(counters.replacement_bytes as usize, replacement_bytes);
    assert_eq!(
        typed.outcome.disposition,
        FormatDisposition::Applied,
        "the NPC-002 subject must exercise a fully applied pipeline"
    );
    assert_eq!(counters.pipeline_invocations, 1);
}

#[test]
fn no_change_subjects_count_zero_edits_with_full_pipeline() {
    let spec = SubjectSpec { family: "no-change", line_ending: "lf", indent: "tabs", units: 4 };
    let source = spec.source();
    let mut counters = NativePipelineCounters::default();
    let typed = NativeFormatter::new().format_document_typed_with_counters(
        &source,
        &FormatConfig::default(),
        &FormatContext::default(),
        &mut counters,
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert_eq!(counters.parse_gate_invocations, 2);
    assert_eq!(counters.source_parse_gate_invocations, 1);
    assert_eq!(counters.formatted_output_parse_gate_invocations, 1);
    assert_eq!(counters.edits_derived, 0);
    assert_eq!(counters.replacement_bytes, 0);
    assert!(counters.lines_processed > 0);
}

// ---------------------------------------------------------------------------
// NPC-004 — observed counter ratio bounds and detector sanity control
// ---------------------------------------------------------------------------

/// Detector bound shared by the scaling rows and this control: a series is
/// superlinear when either doubling exceeds `SCALING_RATIO_BOUND_V1` times its
/// lower sample by more than the effective absolute slack. The detector's
/// domain is strictly positive, ordered samples; a zero-sized lower sample is
/// not classified. For small samples, slack is capped by that lower sample so
/// the configured slack cannot hide the first non-trivial quadratic step. The
/// detector is applied only to counters the production instrument records; it
/// does not measure uninstrumented fit/comparison operations or prove their
/// algorithmic complexity. Boundary controls supplement the large quadratic
/// series so weakening either detector constant remains observable.
fn detector_slack(lower_sample: u64) -> u64 {
    SCALING_ABSOLUTE_SLACK_V1.min(lower_sample)
}

fn is_superlinear(n: u64, two_n: u64, four_n: u64) -> bool {
    if n == 0 || two_n < n || four_n < two_n {
        return false;
    }

    two_n > n.saturating_mul(SCALING_RATIO_BOUND_V1).saturating_add(detector_slack(n))
        || four_n
            > two_n.saturating_mul(SCALING_RATIO_BOUND_V1).saturating_add(detector_slack(two_n))
}

#[test]
fn detector_flags_known_quadratic_series() {
    // Canonical quadratic series (units squared): any bound looser than 2x
    // stops flagging this control and the assertion below fails.
    assert!(is_superlinear(100, 400, 1_600));
    // Keep a low-magnitude control: the absolute slack must not hide a
    // quadratic series near the origin.
    assert!(is_superlinear(2, 8, 32));
    // The lower-domain slack cap keeps the smallest realistic quadratic
    // series discriminating instead of treating the absolute slack as a
    // blanket exemption near zero.
    assert!(is_superlinear(1, 4, 16));
    // The first doubling is independently checked; ignoring N would let this
    // pathological jump pass even though the complete three-point shape is
    // not bounded.
    assert!(is_superlinear(1, 1_000, 2_000));
    // These values sit one unit above the current envelope. Increasing either
    // detector bound must make at least one boundary assertion fail.
    assert!(is_superlinear(4, 8, 25));
    assert!(is_superlinear(1, 11, 11));
    // Exact linear series (through origin) stays bounded.
    assert!(!is_superlinear(1, 2, 4));
    // A linear series with a constant term stays bounded at the lower-domain
    // boundary under the capped slack contract.
    assert!(!is_superlinear(1, 3, 5));
    // Zero is outside the detector domain, even if later samples are nonzero.
    assert!(!is_superlinear(0, 4, 16));
    // Linear with a constant term stays bounded.
    assert!(!is_superlinear(105, 210, 420));
    // Constant series stays bounded.
    assert!(!is_superlinear(2, 2, 2));
}

#[test]
fn scaling_cohort_ratios_stay_within_bounded_envelope() {
    for family in ["delimited", "statement", "no-change", "opaque"] {
        let scaling = perf_subjects::scaling_rows(family);
        assert!(scaling.is_some(), "family {family} must be admitted by the subject registry");
        let Some(rows) = scaling else { continue };

        let mut samples: Vec<(SubjectSpec, NativePipelineCounters)> = Vec::new();
        for spec in &rows {
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let _applied = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            samples.push((*spec, counters));
        }

        let n = &samples[0];
        let two_n = &samples[1];
        let four_n = &samples[2];
        assert_eq!(samples.len(), 3, "scaling rows must be exactly N / 2N / 4N for {family}");

        // Constant counters: the pipeline stage topology must not scale.
        assert_eq!(four_n.1.parse_gate_invocations, two_n.1.parse_gate_invocations);
        assert_eq!(two_n.1.parse_gate_invocations, n.1.parse_gate_invocations);
        assert_eq!(
            four_n.1.peak_depth, n.1.peak_depth,
            "peak depth must stay bounded for {family}"
        );
        assert_eq!(four_n.1.pipeline_invocations, 1);
        assert_eq!(two_n.1.pipeline_invocations, 1);

        // Linear-or-bounded counters at every doubling.
        let series = [
            (
                "gate_nodes_observed",
                n.1.gate_nodes_observed,
                two_n.1.gate_nodes_observed,
                four_n.1.gate_nodes_observed,
            ),
            (
                "lines_processed",
                n.1.lines_processed,
                two_n.1.lines_processed,
                four_n.1.lines_processed,
            ),
            (
                "delimited_groups_fitted",
                n.1.delimited_groups_fitted,
                two_n.1.delimited_groups_fitted,
                four_n.1.delimited_groups_fitted,
            ),
            ("edits_derived", n.1.edits_derived, two_n.1.edits_derived, four_n.1.edits_derived),
            (
                "replacement_bytes",
                n.1.replacement_bytes,
                two_n.1.replacement_bytes,
                four_n.1.replacement_bytes,
            ),
        ];
        for (name, n_value, two_n_value, four_n_value) in series {
            assert!(
                !is_superlinear(n_value, two_n_value, four_n_value),
                "{family}.{name} grew superlinearly: N={n_value} 2N={two_n_value} 4N={four_n_value}"
            );
        }

        // The scaling subjects really do scale: the render stage must see more
        // lines as units double (guards against vacuous fixed-size subjects).
        assert!(two_n.1.lines_processed > n.1.lines_processed);
        assert!(four_n.1.lines_processed > two_n.1.lines_processed);
    }
}

#[test]
fn nested_counter_scope_populates_supplied_and_outer_snapshots() {
    let outer = PipelineCollectorScope::install();
    let source =
        SubjectSpec { family: "delimited", line_ending: "lf", indent: "tabs", units: 4 }.source();
    let mut supplied = NativePipelineCounters::default();
    let typed = NativeFormatter::new().format_document_typed_with_counters(
        &source,
        &FormatConfig::default(),
        &FormatContext::default(),
        &mut supplied,
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(supplied.pipeline_invocations, 1);
    assert_eq!(supplied.parse_gate_invocations, 2);
    assert_eq!(supplied.source_parse_gate_invocations, 1);
    assert_eq!(supplied.formatted_output_parse_gate_invocations, 1);
    assert!(supplied.gate_nodes_observed > 0);
    assert!(supplied.lines_processed > 0);
    assert!(supplied.delimited_groups_fitted > 0);
    assert!(supplied.edits_derived > 0);
    assert!(supplied.replacement_bytes > 0);
    assert!(supplied.peak_depth > 0);
    assert!(supplied.elapsed > std::time::Duration::ZERO);

    let mut outer_snapshot = NativePipelineCounters::default();
    outer.merge_into(&mut outer_snapshot);
    assert_eq!(outer_snapshot.pipeline_invocations, 1);
    assert_eq!(outer_snapshot.parse_gate_invocations, 2);
    assert_eq!(outer_snapshot.source_parse_gate_invocations, 1);
    assert_eq!(outer_snapshot.formatted_output_parse_gate_invocations, 1);
    assert!(outer_snapshot.gate_nodes_observed > 0);
    assert!(outer_snapshot.lines_processed > 0);
    assert!(outer_snapshot.delimited_groups_fitted > 0);
    assert!(outer_snapshot.edits_derived > 0);
    assert!(outer_snapshot.replacement_bytes > 0);
    assert!(outer_snapshot.peak_depth > 0);
    assert_eq!(outer_snapshot.elapsed, supplied.elapsed);
    assert_eq!(outer_snapshot.pipeline_invocations, supplied.pipeline_invocations);
    assert_eq!(outer_snapshot.parse_gate_invocations, supplied.parse_gate_invocations);
    assert_eq!(
        outer_snapshot.source_parse_gate_invocations,
        supplied.source_parse_gate_invocations
    );
    assert_eq!(
        outer_snapshot.formatted_output_parse_gate_invocations,
        supplied.formatted_output_parse_gate_invocations
    );
    assert_eq!(outer_snapshot.gate_nodes_observed, supplied.gate_nodes_observed);
    assert_eq!(outer_snapshot.lines_processed, supplied.lines_processed);
    assert_eq!(outer_snapshot.delimited_groups_fitted, supplied.delimited_groups_fitted);
    assert_eq!(outer_snapshot.edits_derived, supplied.edits_derived);
    assert_eq!(outer_snapshot.replacement_bytes, supplied.replacement_bytes);
    assert_eq!(outer_snapshot.peak_depth, supplied.peak_depth);
    assert_eq!(outer_snapshot.elapsed, supplied.elapsed);
    assert_eq!(outer_snapshot, supplied, "nested merge must preserve every counter");
}

// ---------------------------------------------------------------------------
// NPC-005 — refusal and opaque rows are cost-bounded
// ---------------------------------------------------------------------------

#[test]
fn refusal_and_opaque_rows_remain_cost_bounded() {
    for family in ["refusal", "opaque"] {
        let scaling = perf_subjects::scaling_rows(family);
        assert!(scaling.is_some(), "family {family} must be admitted by the subject registry");
        let Some(rows) = scaling else { continue };

        let mut observations: Vec<(NativePipelineCounters, FormatDisposition)> = Vec::new();
        for spec in &rows {
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let typed = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            observations.push((counters, typed.outcome.disposition));
        }
        assert_eq!(observations.len(), 3);

        for (counters, disposition) in &observations {
            assert_eq!(counters.pipeline_invocations, 1);
            assert_eq!(*disposition, FormatDisposition::Refused, "{family} rows must refuse");
            assert_eq!(counters.edits_derived, 0, "{family} rows must derive no edits");
            assert_eq!(counters.replacement_bytes, 0, "{family} rows must derive no bytes");
        }

        let (n, two_n, four_n) = (&observations[0].0, &observations[1].0, &observations[2].0);
        if family == "refusal" {
            // The refusal path must reject at the source parse gate without a
            // render pass: exactly one gate invocation at every size.
            for counters in [n, two_n, four_n] {
                assert_eq!(counters.parse_gate_invocations, 1);
                assert_eq!(counters.source_parse_gate_invocations, 1);
                assert_eq!(counters.formatted_output_parse_gate_invocations, 0);
            }
        }
        let series = [
            (
                "gate_nodes_observed",
                n.gate_nodes_observed,
                two_n.gate_nodes_observed,
                four_n.gate_nodes_observed,
            ),
            ("lines_processed", n.lines_processed, two_n.lines_processed, four_n.lines_processed),
        ];
        for (name, n_value, two_n_value, four_n_value) in series {
            assert!(
                !is_superlinear(n_value, two_n_value, four_n_value),
                "{family}.{name} grew superlinearly: N={n_value} 2N={two_n_value} 4N={four_n_value}"
            );
        }

        // Every admitted line-ending convention stays cost-bounded too: a
        // refusal or opaque flood must not slip through on a variant row.
        for line_ending in perf_subjects::LINE_ENDINGS {
            let variant = perf_subjects::line_ending_row(family, line_ending, 16);
            assert!(
                variant.is_some(),
                "line-ending row {family}/{line_ending} must be constructible"
            );
            let Some(spec) = variant else { continue };
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let _typed = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            assert_eq!(counters.pipeline_invocations, 1);
            assert_eq!(counters.edits_derived, 0);
            assert_eq!(counters.replacement_bytes, 0);
        }
    }
}

// ---------------------------------------------------------------------------
// NPC-006 — derived-output growth trips before product envelopes
// ---------------------------------------------------------------------------

#[test]
fn derived_output_growth_trips_before_product_envelope() {
    let trip = perl_lsp_perltidy::exceeds_replacement_envelope_v1;
    // The envelope demonstrably trips before unbounded growth ships.
    assert!(trip(10, 41));
    assert!(!trip(10, 40));

    for family in ["delimited", "statement", "no-change"] {
        let scaling = perf_subjects::scaling_rows(family);
        assert!(scaling.is_some(), "family {family} must be admitted by the subject registry");
        let Some(rows) = scaling else { continue };
        for spec in &rows {
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let _applied = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            assert!(
                !trip(source.len() as u64, counters.replacement_bytes),
                "{family} derived-output growth exceeded the schema-v1 envelope at n={}",
                spec.units
            );
        }

        // Indentation variants — including the exact line-width boundary —
        // ride the same schema-v1 envelope.
        for indent in perf_subjects::INDENTS {
            let variant = perf_subjects::indent_row("delimited", indent, 16);
            assert!(variant.is_some(), "indent row delimited/{indent} must be constructible");
            let Some(spec) = variant else { continue };
            let source = spec.source();
            let mut counters = NativePipelineCounters::default();
            let _applied = NativeFormatter::new().format_document_typed_with_counters(
                &source,
                &FormatConfig::default(),
                &FormatContext::default(),
                &mut counters,
            );
            assert!(
                !trip(source.len() as u64, counters.replacement_bytes),
                "delimited/{indent} exceeded the schema-v1 envelope"
            );
        }
    }
    // The envelope constant itself stays version-pinned; loosening it is a
    // schema-major event with required before/after receipts (NPC-010).
    assert_eq!(MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1, 4);
}

// ---------------------------------------------------------------------------
// NPC-003 — one pipeline invocation per LSP request at both provider seams
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct RecordingRuntime {
    invoked: Arc<AtomicBool>,
}

impl perl_lsp_rs_core::tooling::SubprocessRuntime for RecordingRuntime {
    fn run_command(
        &self,
        _program: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> Result<
        perl_lsp_rs_core::tooling::SubprocessOutput,
        perl_lsp_rs_core::tooling::SubprocessError,
    > {
        self.invoked.store(true, Ordering::SeqCst);
        Ok(perl_lsp_rs_core::tooling::SubprocessOutput {
            stdout: b"my $external = 1;\n".to_vec(),
            stderr: Vec::new(),
            status_code: 0,
        })
    }
}

fn formatting_options() -> perl_lsp_rs_core::providers::formatting::FormattingOptions {
    perl_lsp_rs_core::providers::formatting::FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    }
}

fn native_provider() -> perl_lsp_rs_core::providers::formatting::FormattingProvider<RecordingRuntime>
{
    perl_lsp_rs_core::providers::formatting::FormattingProvider::new(RecordingRuntime {
        invoked: Arc::new(AtomicBool::new(false)),
    })
}

#[test]
fn document_request_parses_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
    let provider = native_provider();
    let options = formatting_options();
    let context = FormatContext::new(Some("npc-003/document".to_string()), Some(1));

    let statement =
        SubjectSpec { family: "statement", line_ending: "lf", indent: "tabs", units: 4 }.source();
    for content in [statement, "my $x = ;\n".to_string()] {
        let mut counters = NativePipelineCounters::default();
        let counted = provider.format_document_decision_with_counters(
            &content,
            &options,
            &context,
            &mut counters,
        )?;
        assert_eq!(counters.pipeline_invocations, 1, "one pipeline per document request");
        let expected_gates =
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError) + 1;
        assert_eq!(counters.parse_gate_invocations, expected_gates);
        assert_eq!(counters.source_parse_gate_invocations, 1);
        assert_eq!(
            counters.formatted_output_parse_gate_invocations,
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError)
        );
        assert!(
            !counted.outcome.identity.config_fingerprint.is_empty(),
            "the seam must keep exact config identity"
        );

        // The counted seam shares the exact decision path with the plain seam.
        let plain = provider.format_document_decision(&content, &options, &context)?;
        assert_eq!(
            plain.outcome.disposition, counted.outcome.disposition,
            "the counters-aware seam must not fork the decision path"
        );
        assert_eq!(plain.outcome.reason, counted.outcome.reason);
    }
    Ok(())
}

#[test]
fn range_request_parses_exactly_once() -> Result<(), Box<dyn std::error::Error>> {
    let provider = native_provider();
    let options = formatting_options();
    let context = FormatContext::new(Some("npc-003/range".to_string()), Some(1));

    let statement =
        SubjectSpec { family: "statement", line_ending: "lf", indent: "tabs", units: 4 }.source();
    for content in [statement, "my $x = ;\n".to_string()] {
        let range = perl_lsp_rs_core::providers::formatting::FormatRange::whole_document(&content);
        let mut counters = NativePipelineCounters::default();
        let counted = provider.format_range_decision_with_counters(
            &content,
            &range,
            &options,
            &context,
            &mut counters,
        )?;
        assert_eq!(counters.pipeline_invocations, 1, "one pipeline per range request");
        assert_eq!(counters.source_parse_gate_invocations, 1);
        assert_eq!(
            counters.formatted_output_parse_gate_invocations,
            u64::from(counted.outcome.reason != FormatReasonCode::SourceParseError)
        );

        let plain = provider.format_range_decision(&content, &range, &options, &context)?;
        assert_eq!(plain.outcome.disposition, counted.outcome.disposition);
        assert_eq!(plain.outcome.reason, counted.outcome.reason);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// NPC-008 — exact subject identity for receipt consumption
// ---------------------------------------------------------------------------

#[test]
fn subject_identity_is_recorded_for_receipt_consumption() {
    let config_fingerprint = {
        let spec = SubjectSpec { family: "delimited", line_ending: "lf", indent: "tabs", units: 4 };
        let source = spec.source();
        let typed: TypedFormatResult = NativeFormatter::new().format_document_typed(
            &source,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        typed.outcome.identity.config_fingerprint
    };
    assert!(!config_fingerprint.is_empty());

    let toolchain = toolchain_tag();
    assert!(!toolchain.is_empty());
    for spec in perf_subjects::bench_rows() {
        let row = perf_subjects::identity_row(&spec, &config_fingerprint, &toolchain);
        assert_eq!(row["schema"], COUNTER_SCHEMA_V1);
        assert_eq!(row["engine"], perf_subjects::SUBJECT_ENGINE);
        assert_eq!(row["bench_id"], spec.bench_id());
        assert!(
            row["subject"]["content_digest"].as_str() == Some(spec.content_digest().as_str())
                && spec.content_digest().starts_with("source-v1:"),
            "subject digest must be pinned for {}",
            spec.id()
        );
        assert!(
            row["config_fingerprint"].as_str() == Some(config_fingerprint.as_str()),
            "config fingerprint must be the production fingerprint"
        );
        assert!(!row["toolchain"].as_str().unwrap_or_default().is_empty());
    }

    // Subject digests must equal the production FormatIdentity digest exactly.
    for family in perf_subjects::FAMILIES {
        let spec = SubjectSpec { family, line_ending: "crlf", indent: "spaces", units: 2 };
        let source = spec.source();
        let typed: TypedFormatResult = NativeFormatter::new().format_document_typed(
            &source,
            &FormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(
            typed.outcome.identity.content_digest,
            spec.content_digest(),
            "subject digest drift for {family}"
        );
    }

    // The representative expect-id must exist in the enrolled cohort.
    let ids: Vec<String> = perf_subjects::bench_rows().iter().map(SubjectSpec::bench_id).collect();
    assert!(
        ids.contains(&perf_subjects::REPRESENTATIVE_BENCH_ID.to_string()),
        "the nightly representative expect-id must be an enrolled bench row"
    );
}

#[test]
fn receipt_identity_rows_include_production_counter_snapshot() {
    let spec = SubjectSpec { family: "delimited", line_ending: "lf", indent: "tabs", units: 4 };
    let source = spec.source();
    let mut counters = NativePipelineCounters::default();
    let typed = NativeFormatter::new().format_document_typed_with_counters(
        &source,
        &FormatConfig::default(),
        &FormatContext::default(),
        &mut counters,
    );
    let row = identity_row_with_counters(&spec, &typed, &toolchain_tag(), "test-run", &counters);

    assert_eq!(row["counters"]["schema"], COUNTER_SCHEMA_V1);
    assert_eq!(row["counters"]["pipeline_invocations"], 1);
    assert_eq!(row["counters"]["source_parse_gate_invocations"], 1);
    assert_eq!(row["counters"]["formatted_output_parse_gate_invocations"], 1);
}

#[test]
fn repeated_document_requests_accumulate_pipeline_and_line_counters() {
    let spec = SubjectSpec { family: "statement", line_ending: "lf", indent: "tabs", units: 4 };
    let source = spec.source();
    let per_call_lines = source.split_inclusive('\n').count() as u64;
    let mut counters = NativePipelineCounters::default();

    for _ in 0..3 {
        let _typed = NativeFormatter::new().format_document_typed_with_counters(
            &source,
            &FormatConfig::default(),
            &FormatContext::default(),
            &mut counters,
        );
    }

    assert_eq!(counters.pipeline_invocations, 3);
    assert_eq!(counters.lines_processed, per_call_lines * 3);
}

// ---------------------------------------------------------------------------
// NPC-007 — nightly enrollment of the new bench target
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn nightly_workflow() -> Result<String, Box<dyn std::error::Error>> {
    let path = repo_root().join(".github/workflows/ci-nightly.yml");
    Ok(std::fs::read_to_string(path)?)
}

#[test]
fn bench_target_enrolls_native_pipeline_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = nightly_workflow()?;
    assert!(
        workflow.contains("\"perl-lsp-perltidy:native_pipeline_benchmark:\""),
        "BENCH_TARGETS must enroll perl-lsp-perltidy:native_pipeline_benchmark (declared-superset contract)"
    );
    assert!(
        workflow.contains(&format!("--expect-id \"{}\"", perf_subjects::REPRESENTATIVE_BENCH_ID)),
        "the extract step must require the representative native pipeline bench id"
    );

    let manifest =
        std::fs::read_to_string(repo_root().join("crates/perl-lsp-perltidy/Cargo.toml"))?;
    let bench_section = manifest
        .split("[[bench]]")
        .nth(1)
        .ok_or("perl-lsp-perltidy must declare a [[bench]] target")?;
    assert!(
        bench_section.contains("native_pipeline_benchmark")
            && bench_section.contains("harness = false"),
        "the [[bench]] target must be the criterion harness for native_pipeline_benchmark"
    );
    assert!(
        manifest.contains("criterion = { workspace = true }"),
        "criterion must be a dev-dependency of perl-lsp-perltidy"
    );
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs"),
    )?;
    assert!(benchmark.contains("format_document_typed_with_counters"));
    assert!(benchmark.contains("identity_row_with_counters"));
    assert!(benchmark.contains("native-pipeline-measurements.v1.json"));
    let timing_body =
        benchmark.split("b.iter(|| {").nth(1).ok_or("benchmark timing closure missing")?;
    assert!(
        timing_body.contains("format_document_typed(")
            && !timing_body.contains("format_document_typed_with_counters("),
        "Criterion timing must use the plain typed entry point so counter collection stays in the dedicated receipt pass"
    );
    assert!(workflow.contains("NATIVE_PIPELINE_RUN_ID"));
    assert!(workflow.contains("target/criterion/native-pipeline-measurements.v1.json"));
    Ok(())
}

#[test]
fn criterion_timing_invokes_native_formatter_production_path()
-> Result<(), Box<dyn std::error::Error>> {
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs"),
    )?;
    let iter_start = benchmark.find("b.iter(|| {").ok_or("benchmark timing closure missing")?;
    let iter_body = &benchmark[iter_start..];
    let iter_end = iter_body.find("});").ok_or("benchmark timing closure is unterminated")?;
    let iter_body = &iter_body[..iter_end];
    let production_invocation = "NativeFormatter::new().format_document_typed(";

    assert_eq!(
        iter_body.matches(production_invocation).count(),
        1,
        "Criterion must time exactly one native production invocation per iteration"
    );
    assert!(
        iter_body.contains("black_box(&source)"),
        "the timed production invocation must consume the enrolled subject source"
    );
    assert!(
        !iter_body.contains("format_document_typed_with_counters("),
        "counter collection belongs to the dedicated receipt pass, not Criterion timing"
    );
    Ok(())
}

#[test]
fn sidecar_run_id_uses_single_derivation_for_envelope_and_rows()
-> Result<(), Box<dyn std::error::Error>> {
    let benchmark = std::fs::read_to_string(
        repo_root().join("crates/perl-lsp-perltidy/benches/native_pipeline_benchmark.rs"),
    )?;
    let invariant = "the envelope and every row must carry an identical run id, which the nightly validator requires";
    assert_eq!(
        benchmark.matches("std::env::var(\"NATIVE_PIPELINE_RUN_ID\")").count(),
        1,
        "{invariant}; derive NATIVE_PIPELINE_RUN_ID exactly once"
    );
    assert!(
        benchmark.contains("fn build_subject_identities(toolchain: &str, run_id: &str)"),
        "{invariant}; build_subject_identities must receive the shared run id"
    );
    assert!(
        benchmark
            .contains("identity_row_with_counters(spec, &typed, toolchain, run_id, &counters)"),
        "{invariant}; every row must use the shared run id parameter"
    );
    assert!(
        benchmark.contains("\"run_id\": run_id"),
        "{invariant}; the sidecar envelope must use the shared run id parameter"
    );
    Ok(())
}

#[test]
fn elapsed_measurement_wraps_classification_for_document_and_range()
-> Result<(), Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(
        repo_root().join("crates/perl-lsp-perltidy/src/native/outcome.rs"),
    )?;
    for function in ["format_document_typed_with_counters", "format_range_typed_with_counters"] {
        let start = source
            .find(&format!("pub fn {function}"))
            .ok_or("counter-aware entry point missing")?;
        let body = &source[start..];
        let classified = body.find("classify_native_result").ok_or("classification missing")?;
        let elapsed = body.find("counters.observe_elapsed").ok_or("elapsed observation missing")?;
        assert!(classified < elapsed, "{function} must include classification in total elapsed");
    }
    Ok(())
}

#[test]
fn implementation_spec_keeps_unproven_followups_explicit() -> Result<(), Box<dyn std::error::Error>>
{
    let acceptance = std::fs::read_to_string(
        repo_root().join(".spec/10302-formatter-production-pipeline-bench/acceptance.md"),
    )?;
    assert!(acceptance.contains("PR #13190"));
    assert!(acceptance.contains("does not close #10302"));
    assert!(acceptance.contains("allocation oracle"));
    assert!(acceptance.contains("NOT_PROVEN"));
    assert!(acceptance.contains("not a proof of algorithmic complexity"));
    assert!(acceptance.contains("production operation counters for those activities remain"));
    Ok(())
}

// ---------------------------------------------------------------------------
// NPC-009 — timing stays evidence, never a required gate
// ---------------------------------------------------------------------------

fn step_retains_advisory_posture(workflow: &str, step_name: &str) -> bool {
    let mut in_step = false;
    for line in workflow.lines() {
        if line.contains("- name:") {
            in_step = line.contains(step_name);
            continue;
        }
        if in_step && line.contains("continue-on-error:") {
            return line.contains("true");
        }
    }
    false
}

#[test]
fn timing_stays_advisory_in_nightly_workflow() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = nightly_workflow()?;
    assert!(
        step_retains_advisory_posture(&workflow, "Compare against baseline"),
        "the Compare against baseline step must keep continue-on-error: true (#3979/#5282)"
    );
    assert!(
        step_retains_advisory_posture(&workflow, "Generate performance alerts"),
        "the Generate performance alerts step must keep continue-on-error: true (#3979/#5282)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// NPC-010 — anti-masking downward-only ratchet
// ---------------------------------------------------------------------------

/// Base-pin maxima (origin/main @ a9664af79, 2026-08-27). Any timeout above
/// these maxima fails this canary; lowering is always allowed.
fn base_pin_max_timeout_minutes(job: &str) -> Option<u64> {
    match job {
        "mutation" => Some(60),
        "benchmark" => Some(45),
        "real-repo-latency" => Some(30),
        "corpus-differential" => Some(20),
        "lsp-memory-plateau" => Some(35),
        "test-coverage" => Some(45),
        "tautology-check" => Some(10),
        "semver-check" => Some(20),
        "public-api-check" => Some(20),
        "scorecard-ratchet-check" => Some(15),
        "clippy-strict" => Some(20),
        "perl-kwalitee" => Some(20),
        "fuzz" => Some(15),
        _ => None,
    }
}

fn job_timeouts(workflow: &str) -> Vec<(String, u64)> {
    let mut jobs = Vec::new();
    let mut current_job = String::new();
    for line in workflow.lines() {
        if let Some(job) = line.strip_prefix("  ")
            && let Some((name, rest)) = job.split_once(':')
            && rest.trim().is_empty()
            && !name.contains(' ')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            current_job = name.to_string();
        }
        if let Some((key, value)) = line.trim().split_once(':')
            && key.trim() == "timeout-minutes"
            && let Ok(minutes) = value.trim().parse::<u64>()
        {
            jobs.push((current_job.clone(), minutes));
        }
    }
    jobs
}

#[test]
fn no_timeout_or_budget_constant_exceeds_base_pin_maxima() -> Result<(), Box<dyn std::error::Error>>
{
    let workflow = nightly_workflow()?;
    let timeouts = job_timeouts(&workflow);
    assert!(timeouts.len() >= 13, "the ratchet must see every job timeout in ci-nightly.yml");
    for (job, minutes) in timeouts {
        let max = base_pin_max_timeout_minutes(&job);
        assert!(
            max.is_some(),
            "job {job} declares timeout-minutes {minutes} but has no pinned maximum; \
             lower the timeout or update the base-pin ratchet consciously"
        );
        let Some(max) = max else { continue };
        assert!(
            minutes <= max,
            "job {job} timeout {minutes} exceeds the base-pin maximum {max} — \
             anti-masking forbids absorbing regressions via timeout bumps"
        );
    }
    Ok(())
}
