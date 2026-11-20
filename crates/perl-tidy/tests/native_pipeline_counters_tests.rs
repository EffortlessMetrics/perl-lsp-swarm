//! Counter-shape canaries for the native formatter production pipeline
//! (#10302, acceptance rows NPC-001..NPC-010).
//!
//! These are ordinary deterministic package tests, not timing gates: all
//! assertions bind deterministic integer counters, structural workflow pins,
//! and exact subject identity. Wall-clock stays advisory per the standing
//! #3979/#5282 policy and no check in this file may be satisfied by
//! increasing a timeout, budget constant, size cap, or iteration bound
//! (NPC-010 ratchet).

#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.

#[path = "../benches/support/perf_subjects.rs"]
mod perf_subjects;

use perf_subjects::{SubjectSpec, identity_row_with_counters, toolchain_tag};
use perl_tidy::native::{
    COUNTER_CLOCK_TAG, COUNTER_SCHEMA_V1, FormatConfig, FormatContext, FormatDisposition,
    FormatReasonCode, MAX_REPLACEMENT_BYTES_PER_SOURCE_BYTE_V1, NativeFormatter,
    NativePipelineCounters, PipelineCollectorScope, SCALING_ABSOLUTE_SLACK_V1,
    SCALING_RATIO_BOUND_V1, TypedFormatResult,
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

        let range = perl_tidy::ExactSource::new(&source).whole_document();
        let plain_range =
            NativeFormatter::new().format_range_typed(&source, range.clone(), &config, &context);
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
        counters.layout_groups_fitted > 0,
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
                "layout_groups_fitted",
                n.1.layout_groups_fitted,
                two_n.1.layout_groups_fitted,
                four_n.1.layout_groups_fitted,
            ),
            ("edits_derived", n.1.edits_derived, two_n.1.edits_derived, four_n.1.edits_derived),
            (
                "replacement_bytes",
                n.1.replacement_bytes,
                two_n.1.replacement_bytes,
                four_n.1.replacement_bytes,
            ),
        ];
        // `is_superlinear` returns false whenever N is zero — correct as a
        // detector rule, but it means a series this family never records is
        // checked vacuously. Measured on this head, 7 of these 20 checks sat
        // outside the detector's domain, so the loop alone reported coverage
        // of a cohort it only partly exercised, and a counter that regressed
        // to never being recorded would have joined the vacuous set silently
        // instead of failing.
        //
        // Each series is therefore classified before it is bounded. An active
        // series must be positive at N — that is what puts it in the
        // detector's domain — and an inactive one must be zero at every size,
        // which is a claim about the family's production behaviour rather than
        // an exemption: `no-change` and `opaque` derive no edits and so carry
        // no replacement bytes, and only `delimited` fits layout groups.
        let expected_active: &[&str] = match family {
            "delimited" => &[
                "gate_nodes_observed",
                "lines_processed",
                "layout_groups_fitted",
                "edits_derived",
                "replacement_bytes",
            ],
            "statement" => {
                &["gate_nodes_observed", "lines_processed", "edits_derived", "replacement_bytes"]
            }
            _ => &["gate_nodes_observed", "lines_processed"],
        };

        let mut in_domain = 0_usize;
        for (name, n_value, two_n_value, four_n_value) in series {
            if expected_active.contains(&name) {
                assert!(
                    n_value > 0,
                    "{family}.{name} is pinned as an active series but recorded nothing at N; \
                     a counter that stops being recorded must fail here rather than turn the \
                     envelope check below vacuous"
                );
                in_domain += 1;
            } else {
                assert_eq!(
                    (n_value, two_n_value, four_n_value),
                    (0, 0, 0),
                    "{family}.{name} is pinned as inactive for this family but recorded work; \
                     update the classification consciously rather than leaving the envelope \
                     check silently out of domain"
                );
            }

            assert!(
                !is_superlinear(n_value, two_n_value, four_n_value),
                "{family}.{name} grew superlinearly: N={n_value} 2N={two_n_value} 4N={four_n_value}"
            );
        }

        assert_eq!(
            in_domain,
            expected_active.len(),
            "{family} must exercise exactly its pinned active series"
        );

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
    assert!(supplied.layout_groups_fitted > 0);
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
    assert!(outer_snapshot.layout_groups_fitted > 0);
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
    assert_eq!(outer_snapshot.layout_groups_fitted, supplied.layout_groups_fitted);
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
    let trip = perl_tidy::exceeds_replacement_envelope_v1;
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
                && spec.content_digest().starts_with("sha256:"),
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
