//! Fixtures and falsifiers for the effective-invocation trace contract
//! (#12284).
//!
//! Positive fixtures cover the `base`/`comp`/`run` invocation families and
//! bounded synthetic rows for every reviewed TestInit, taint, source-form,
//! cwd, and include-order distinction. Each numbered falsifier below is the
//! discriminating test for one law of the issue: an implementation missing
//! that law fails the named test (mutation control).

use crate::invocation_trace::adapter::{ProjectionOutcome, ProjectionRejection};
use crate::invocation_trace::model::{
    EffectiveInvocationField, EffectiveInvocationTraceReceiptV1, FieldKey,
    InvocationObservationState, ProjectionRecord, TraceRowDisposition,
    UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION,
};
use crate::invocation_trace::test_support::{TraceFixture, all_observed_fields, sha_hex};
use crate::invocation_trace::{
    build_invocation_trace_receipt, check_invocation_trace_against,
    validate_invocation_trace_receipt, validate_trace_receipt_subject_binding,
};
use crate::observed_discovery::model::ProcessCompletion;
use color_eyre::eyre::{Result, bail, eyre};
use serde_json::json;

fn ensure(outcome: Result<(), String>) -> Result<()> {
    outcome.map_err(|error| eyre!(error))
}

fn build(fixture: &TraceFixture, bytes: &[u8]) -> Result<EffectiveInvocationTraceReceiptV1> {
    build_invocation_trace_receipt(&fixture.input(bytes.to_vec())).map_err(|error| eyre!(error))
}

fn line_of(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string(value).map_err(|error| eyre!(error))
}

fn row_line_of(frame: &crate::invocation_trace::decode::RowFrame) -> Result<String> {
    line_of(&serde_json::to_value(frame).map_err(|error| eyre!(error))?)
}

/// Emit a one-row stream with optional field mutation and named completion.
fn single_row_stream(
    fixture: &TraceFixture,
    member: &str,
    mutation: impl FnOnce(&mut crate::invocation_trace::model::EffectiveInvocationFields),
    completion: ProcessCompletion,
) -> Result<Vec<u8>> {
    let mut fields = all_observed_fields(member);
    mutation(&mut fields);
    let frame = fixture.row_frame(member, 0, fields);
    let row_line = row_line_of(&frame)?;
    let header = fixture.header_frame(1);
    let terminal = fixture.terminal_frame(&[row_line.clone()], completion);
    Ok(fixture.emit(&header, &[row_line], &terminal))
}

fn projected_digest(receipt: &EffectiveInvocationTraceReceiptV1, index: usize) -> Result<String> {
    let ProjectionRecord::Projected { digest } = &receipt.payload.rows[index].projection else {
        bail!("row {index} must carry an accepted projection");
    };
    Ok(digest.clone())
}

// ---------------------------------------------------------------------------
// Positive fixtures: base/comp/run families and reviewed distinctions
// ---------------------------------------------------------------------------

#[test]
fn clean_base_trace_is_complete_binds_parent_and_reconstructs() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t", "t/base/cond.t"])?;
    let receipt = build(&fixture, &bytes)?;
    assert_eq!(receipt.schema_version, UPSTREAM_INVOCATION_TRACE_SCHEMA_VERSION);
    assert_eq!(
        receipt.evidence_class,
        crate::observed_discovery::model::EvidenceClass::InstrumentedUpstream
    );
    assert_eq!(receipt.payload.rows.len(), 2);
    for row in &receipt.payload.rows {
        assert!(row.disposition.is_accepted(), "row must be accepted");
        assert_eq!(row.state, InvocationObservationState::ObservedComplete);
        assert!(matches!(row.projection, ProjectionRecord::Projected { .. }));
    }
    assert_eq!(receipt.payload.work.complete_rows, 2);
    assert_eq!(receipt.payload.work.canonical_plan_projections_attempted, 2);
    assert_eq!(receipt.payload.work.canonical_plan_projections_accepted, 2);
    ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    ensure(check_invocation_trace_against(&fixture.parent, &receipt))?;
    Ok(())
}

#[test]
fn base_comp_run_families_project_under_their_own_identities() -> Result<()> {
    let cases = [
        ("component_base", "t/base/if.t\n"),
        ("component_comp", "t/comp/hints.t\n"),
        ("component_run", "t/run/switches.t\n"),
    ];
    let mut digests = Vec::new();
    for (target, member) in cases {
        let member = member.trim_end_matches('\n');
        let fixture = TraceFixture::new(target, &format!("{member}\n"))?;
        let bytes = fixture.emit_complete(&[member])?;
        let receipt = build(&fixture, &bytes)?;
        assert_eq!(receipt.payload.rows.len(), 1, "{target}");
        digests.push(projected_digest(&receipt, 0)?);
        ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    }
    let unique: std::collections::BTreeSet<&String> = digests.iter().collect();
    assert_eq!(unique.len(), 3, "each invocation family projects its own identity");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 5: U1/U2T/A/NC, -t/-T, UTF, and form distinctions collapse
// ---------------------------------------------------------------------------

#[test]
fn reviewed_testinit_taint_utf8_and_form_distinctions_stay_load_bearing() -> Result<()> {
    use crate::invocation_trace::model::{TaintMode, TestInitClass, Utf8Switch};
    use crate::runner_model::SourceForm;
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    type Fields = crate::invocation_trace::model::EffectiveInvocationFields;
    let variants: Vec<(&str, Box<dyn Fn(&mut Fields)>)> = vec![
        ("standard", Box::new(|_fields: &mut _| {})),
        (
            "u1",
            Box::new(|fields: &mut _| {
                fields.test_init = EffectiveInvocationField::Observed { value: TestInitClass::U1 };
            }),
        ),
        (
            "u2t",
            Box::new(|fields: &mut _| {
                fields.test_init = EffectiveInvocationField::Observed { value: TestInitClass::U2t };
            }),
        ),
        (
            "a",
            Box::new(|fields: &mut _| {
                fields.test_init = EffectiveInvocationField::Observed { value: TestInitClass::A };
            }),
        ),
        (
            "nc",
            Box::new(|fields: &mut _| {
                fields.test_init = EffectiveInvocationField::Observed { value: TestInitClass::Nc };
            }),
        ),
        (
            "taint-t",
            Box::new(|fields: &mut _| {
                fields.taint_mode =
                    EffectiveInvocationField::Observed { value: TaintMode::TaintWarnings };
            }),
        ),
        (
            "taint-T",
            Box::new(|fields: &mut _| {
                fields.taint_mode =
                    EffectiveInvocationField::Observed { value: TaintMode::TaintMode };
            }),
        ),
        (
            "utf8",
            Box::new(|fields: &mut _| {
                fields.utf8_mode = EffectiveInvocationField::Observed { value: Utf8Switch::Utf8 };
            }),
        ),
        (
            "test-pl",
            Box::new(|fields: &mut _| {
                fields.source_form =
                    EffectiveInvocationField::Observed { value: SourceForm::TestPl };
            }),
        ),
    ];
    let mut digests = Vec::new();
    for (label, mutation) in &variants {
        let bytes = single_row_stream(
            &fixture,
            "t/base/if.t",
            |fields| mutation(fields),
            ProcessCompletion::ExitStatus { code: 0 },
        )?;
        let receipt = build(&fixture, &bytes)?;
        digests.push(projected_digest(&receipt, 0).map_err(|error| eyre!("{label}: {error}"))?);
    }
    let unique: std::collections::BTreeSet<&String> = digests.iter().collect();
    assert_eq!(
        unique.len(),
        variants.len(),
        "every reviewed distinction changes the projection identity"
    );
    Ok(())
}

#[test]
fn not_applicable_and_not_observed_states_are_distinctly_counted() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            // Same cwd: the return directory genuinely does not apply, and
            // one argument stayed unobserved. The two states count apart.
            fields.return_directory = EffectiveInvocationField::NotApplicable {
                reason: "runner returns to the invocation cwd".to_string(),
            };
            fields.script_arguments = EffectiveInvocationField::NotObserved {
                reason: "no script arguments captured".to_string(),
            };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    let row = &receipt.payload.rows[0];
    assert_eq!(row.state, InvocationObservationState::ObservedPartial);
    assert_eq!(receipt.payload.work.partial_rows, 1);
    assert_eq!(receipt.payload.work.fields_observed, 15);
    assert_eq!(receipt.payload.work.fields_not_applicable, 1);
    assert_eq!(receipt.payload.work.fields_not_observed, 1);
    assert!(matches!(
        row.projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::ObservationNotComplete
        }
    ));
    ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 1: one missing required field synthesized from
// source/path/profile/expected plan
// ---------------------------------------------------------------------------

#[test]
fn missing_required_field_is_never_synthesized() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            // run_cwd is trivially derivable from the script path, TestInit
            // from a profile table, include roots from the source tree: all
            // stay honestly unobserved and un-projected.
            fields.run_cwd =
                EffectiveInvocationField::NotObserved { reason: "cwd not captured".into() };
            fields.test_init =
                EffectiveInvocationField::NotObserved { reason: "not captured".into() };
            fields.include_roots =
                EffectiveInvocationField::NotObserved { reason: "not captured".into() };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    let row = &receipt.payload.rows[0];
    assert_eq!(row.state, InvocationObservationState::ObservedPartial);
    assert!(matches!(
        row.projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::ObservationNotComplete
        }
    ));
    // The adapter names the first missing field instead of filling it, even
    // when a complete state is asserted around it.
    let binding = fixture.expected_binding(&fixture.row("t/base/if.t", 0, row.fields.clone()));
    let mut asserted_complete = fixture.row("t/base/if.t", 0, row.fields.clone());
    asserted_complete.state = InvocationObservationState::ObservedComplete;
    match crate::invocation_trace::adapter::project_effective_invocation(
        &asserted_complete,
        &binding,
    ) {
        ProjectionOutcome::Rejected(ProjectionRejection::FieldNotObserved { field }) => {
            assert_eq!(field, FieldKey::RunCwd);
        }
        other => bail!("expected field-not-observed rejection, got {other:?}"),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 2: a direct probe with identical final argv satisfies an observed
// upstream row
// ---------------------------------------------------------------------------

#[test]
fn direct_probe_never_satisfies_an_observed_upstream_row() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    // The exact same observed fields, claimed for the direct-fallback route.
    let mut frame = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    frame.runner = crate::runner_model::RunnerKind::DirectFallback;
    let row_line = row_line_of(&frame)?;
    let header = fixture.header_frame(1);
    let terminal =
        fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
    let bytes = fixture.emit(&header, &[row_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    // The row attaches to a foreign runner: subject mismatch, never a plan.
    assert_eq!(receipt.payload.rows[0].state, InvocationObservationState::SubjectMismatch);
    assert!(matches!(
        receipt.payload.rows[0].projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::DirectProbeAuthority
        }
    ));

    // And the adapter itself refuses direct-probe authority outright, even
    // for an otherwise complete, correctly bound row.
    let mut row = fixture.row("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    row.subject.runner = crate::runner_model::RunnerKind::DirectFallback;
    row.state = InvocationObservationState::ObservedComplete;
    let binding = fixture.expected_binding(&row);
    assert_eq!(
        crate::invocation_trace::adapter::project_effective_invocation(&row, &binding),
        ProjectionOutcome::Rejected(ProjectionRejection::DirectProbeAuthority)
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 3: a reconstructed expected plan relabelled observed
// ---------------------------------------------------------------------------

#[test]
fn expected_plan_values_stay_diagnostic_and_never_become_observed() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let expected = crate::invocation_trace::adapter::ExpectedInvocationValues {
        run_cwd: Some("t".to_string()),
        include_roots: Some(vec!["../lib".to_string(), "../t/lib".to_string()]),
        script_path: Some("t/base/if.t".to_string()),
        ..Default::default()
    };
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.run_cwd =
                EffectiveInvocationField::NotObserved { reason: "not captured".into() };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    let row = &receipt.payload.rows[0];
    let comparisons = crate::invocation_trace::adapter::compare_expected(&row.fields, &expected);
    let cwd_entry = comparisons
        .iter()
        .find(|entry| entry.field == FieldKey::RunCwd)
        .ok_or_else(|| eyre!("comparison must cover run_cwd"))?;
    assert_eq!(
        cwd_entry.result,
        crate::invocation_trace::adapter::ExpectedFieldResult::NotObserved
    );
    // The diagnostic comparison upgraded nothing: the row still projects
    // nothing, and the field is still not observed.
    assert!(!row.fields.run_cwd.is_observed());
    assert_eq!(row.state, InvocationObservationState::ObservedPartial);
    assert!(matches!(row.projection, ProjectionRecord::Rejected { .. }));
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 4: ordered include roots or switches sorted with identity
// unchanged
// ---------------------------------------------------------------------------

#[test]
fn ordered_include_roots_and_switches_change_plan_identity() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let mut digests = Vec::new();
    for roots in [
        vec!["../lib".to_string(), "../t/lib".to_string()],
        vec!["../t/lib".to_string(), "../lib".to_string()],
    ] {
        let switches: Vec<String> = roots.iter().map(|root| format!("-I{root}")).collect();
        let bytes = single_row_stream(
            &fixture,
            "t/base/if.t",
            |fields| {
                fields.include_roots = EffectiveInvocationField::Observed { value: roots.clone() };
                fields.interpreter_switches =
                    EffectiveInvocationField::Observed { value: switches.clone() };
            },
            ProcessCompletion::ExitStatus { code: 0 },
        )?;
        let receipt = build(&fixture, &bytes)?;
        digests.push(projected_digest(&receipt, 0)?);
    }
    assert_ne!(
        digests[0], digests[1],
        "sorting include roots or switches must change the plan identity"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 6: `.t` and `test.pl` forms share one invocation shape
// ---------------------------------------------------------------------------

#[test]
fn dot_t_and_test_pl_forms_never_share_one_invocation_shape() -> Result<()> {
    let fixture = TraceFixture::new("manifest_root_lib", "lib/Foo/test.pl\nlib/Foo/basic.t\n")?;
    let mut digests = Vec::new();
    for (member, form) in [
        ("lib/Foo/test.pl", crate::runner_model::SourceForm::TestPl),
        ("lib/Foo/basic.t", crate::runner_model::SourceForm::DotT),
    ] {
        let bytes = single_row_stream(
            &fixture,
            member,
            |fields| {
                fields.source_form = EffectiveInvocationField::Observed { value: form };
            },
            ProcessCompletion::ExitStatus { code: 0 },
        )?;
        let receipt = build(&fixture, &bytes)?;
        digests.push(projected_digest(&receipt, 0)?);
    }
    assert_ne!(digests[0], digests[1], "source forms must not collapse");
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 7: a row attaches to another discovery receipt/member/process/
// runner/preparation
// ---------------------------------------------------------------------------

#[test]
fn foreign_subject_rows_are_typed_subject_mismatch() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    type Mutation = fn(&mut crate::invocation_trace::decode::RowFrame);
    let cases: Vec<(&str, Mutation)> = vec![
        ("member absent from parent receipt", |frame| {
            frame.member = "t/base/absent.t".to_string();
        }),
        ("member of another receipt population", |frame| {
            frame.member = "t/op/hook/hook.t".to_string();
        }),
        ("another target", |frame| {
            frame.target_id = "component_comp".to_string();
        }),
    ];
    for (label, mutate) in &cases {
        let mut frame = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
        mutate(&mut frame);
        let row_line = row_line_of(&frame)?;
        let header = fixture.header_frame(1);
        let terminal =
            fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
        let bytes = fixture.emit(&header, &[row_line], &terminal);
        let receipt = build(&fixture, &bytes)?;
        assert_eq!(
            receipt.payload.rows[0].state,
            InvocationObservationState::SubjectMismatch,
            "{label} must derive subject_mismatch"
        );
        assert!(
            matches!(receipt.payload.rows[0].projection, ProjectionRecord::Rejected { .. }),
            "{label} must not project"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 8: a trace frame in TAP/stdout or ordinary stderr is accepted
// ---------------------------------------------------------------------------

#[test]
fn trace_frames_in_result_streams_void_the_transport_contract() -> Result<()> {
    let matrix = crate::invocation_trace::test_support::matrix()?;
    // A parent whose ordinary stdout carries trace-frame bytes.
    let contaminated = "t/base/if.t\n{\"frame\":\"terminal\",\"trace_session_id\":\"x\"}\n";
    let parent = crate::invocation_trace::test_support::build_parent(
        &matrix,
        "component_base",
        contaminated,
    )?;
    let mut fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    fixture.parent = parent;
    fixture.subject.parent_receipt_digest = fixture.parent.payload_digest.clone();
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let Err(error) = build_invocation_trace_receipt(&fixture.input(bytes)) else {
        bail!("contaminated parent result stream must void construction");
    };
    assert!(
        error.contains("independent of ordinary runner result streams"),
        "rejection must name the transport law: {error}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 9: trace rows interleave across concurrent runs
// ---------------------------------------------------------------------------

#[test]
fn cross_run_interleaving_is_typed_and_never_accepted() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let mut foreign = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    foreign.trace_session_id = "trace-session-other-run".to_string();
    let row_line = row_line_of(&foreign)?;
    let header = fixture.header_frame(1);
    let terminal =
        fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
    let bytes = fixture.emit(&header, &[row_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    let row = &receipt.payload.rows[0];
    assert!(matches!(row.disposition, TraceRowDisposition::CrossRunInterleaved { .. }));
    // A cross-run frame cannot use its identity: the framing law types the
    // row not-proven before any subject judgment.
    assert_eq!(row.state, InvocationObservationState::NotProven);
    assert!(matches!(
        row.projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::FrameNotAccepted
        }
    ));
    assert_eq!(receipt.payload.work.conflicting_rows, 1);
    ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 10: terminal trace frame absent but all expected source rows
// present
// ---------------------------------------------------------------------------

#[test]
fn missing_terminal_frame_leaves_every_row_not_proven() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    let row_lines: Vec<String> = ["t/base/if.t", "t/base/cond.t"]
        .iter()
        .enumerate()
        .map(|(sequence, member)| {
            row_line_of(&fixture.row_frame(member, sequence as u32, all_observed_fields(member)))
        })
        .collect::<Result<Vec<_>>>()?;
    let header = fixture.header_frame(row_lines.len() as u32);
    // Emit header + rows, no terminal frame.
    let mut bytes = serde_json::to_vec(&header).map_err(|error| eyre!(error))?;
    bytes.push(b'\n');
    for line in &row_lines {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
    }
    let receipt = build(&fixture, &bytes)?;
    assert!(!receipt.payload.trace_decode.is_complete());
    assert_eq!(receipt.payload.terminal, None);
    assert_eq!(receipt.payload.rows.len(), 2);
    for row in &receipt.payload.rows {
        assert_eq!(row.state, InvocationObservationState::NotProven);
        assert!(matches!(row.projection, ProjectionRecord::Rejected { .. }));
    }
    assert_eq!(receipt.payload.work.not_proven_rows, 2);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 11: duplicate/conflicting rows are last-writer-wins
// ---------------------------------------------------------------------------

#[test]
fn duplicate_row_ids_keep_the_first_row_and_type_the_rest() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    let first = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    let mut duplicate = fixture.row_frame("t/base/cond.t", 1, all_observed_fields("t/base/cond.t"));
    duplicate.row_id = first.row_id.clone();
    let first_line = row_line_of(&first)?;
    let duplicate_line = row_line_of(&duplicate)?;
    let header = fixture.header_frame(2);
    let terminal = fixture.terminal_frame(
        &[first_line.clone(), duplicate_line.clone()],
        ProcessCompletion::ExitStatus { code: 0 },
    );
    let bytes = fixture.emit(&header, &[first_line, duplicate_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    let rows = &receipt.payload.rows;
    assert_eq!(rows.len(), 2);
    // The first row is retained verbatim; the duplicate does not replace it.
    assert_eq!(rows[0].subject.parent_member_path, "t/base/if.t");
    assert!(rows[0].disposition.is_accepted());
    assert_eq!(rows[0].state, InvocationObservationState::ObservedComplete);
    assert!(matches!(rows[1].disposition, TraceRowDisposition::DuplicateRowId { .. }));
    // A duplicate identity cannot use the second spelling: it stays
    // not-proven (framing law) while the first row keeps its complete state.
    assert_eq!(rows[1].state, InvocationObservationState::NotProven);
    assert_eq!(receipt.payload.work.conflicting_rows, 1);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 12: unknown schema/field/state ignored
// ---------------------------------------------------------------------------

#[test]
fn unknown_schema_fields_and_states_are_never_ignored() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;

    // Unknown schema version.
    let mut header = fixture.header_frame(1);
    header.schema_version = "perl_core_harness.upstream_effective_invocation_trace.v2".to_string();
    let frame = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    let row_line = row_line_of(&frame)?;
    let terminal =
        fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
    let bytes = fixture.emit(&header, &[row_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    assert!(!receipt.payload.trace_decode.is_complete());
    assert_eq!(receipt.payload.work.complete_rows, 0);

    // Unknown row-frame field.
    let mut frame_value = serde_json::to_value(&fixture.row_frame(
        "t/base/if.t",
        0,
        all_observed_fields("t/base/if.t"),
    ))
    .map_err(|error| eyre!(error))?;
    frame_value["surprise"] = json!("field");
    let row_line = line_of(&frame_value)?;
    let header = fixture.header_frame(1);
    let terminal =
        fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
    let bytes = fixture.emit(&header, &[row_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    assert!(matches!(
        receipt.payload.rows[0].disposition,
        TraceRowDisposition::MalformedFrame { .. }
    ));
    assert_eq!(receipt.payload.rows[0].state, InvocationObservationState::NotProven);

    // Unknown field state inside the field map.
    let mut fields_value =
        serde_json::to_value(&all_observed_fields("t/base/if.t")).map_err(|error| eyre!(error))?;
    fields_value["run_cwd"] = json!({"state": "probably", "payload": "t"});
    let mut frame_value = serde_json::to_value(&fixture.row_frame(
        "t/base/if.t",
        0,
        all_observed_fields("t/base/if.t"),
    ))
    .map_err(|error| eyre!(error))?;
    frame_value["fields"] = fields_value;
    let row_line = line_of(&frame_value)?;
    let terminal =
        fixture.terminal_frame(&[row_line.clone()], ProcessCompletion::ExitStatus { code: 0 });
    let bytes = fixture.emit(&header, &[row_line], &terminal);
    let receipt = build(&fixture, &bytes)?;
    assert!(matches!(
        receipt.payload.rows[0].disposition,
        TraceRowDisposition::MalformedFrame { .. }
    ));

    // Unknown frame tag.
    let header = fixture.header_frame(0);
    let terminal = fixture.terminal_frame(&[], ProcessCompletion::ExitStatus { code: 0 });
    let mut bytes = fixture.emit(&header, &[], &terminal);
    let foreign = b"{\"frame\":\"diagnostic\",\"message\":\"interloper\"}\n";
    bytes.splice(0..0, foreign.iter().copied());
    let receipt = build(&fixture, &bytes)?;
    assert!(!receipt.payload.trace_decode.is_complete());
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 13: truncation or malformed bytes become a complete row
// ---------------------------------------------------------------------------

#[test]
fn truncation_and_malformed_bytes_never_become_complete_rows() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;

    // Invalid UTF-8.
    let bytes = b"{\"frame\":\"header\"}\n\xff\xfe\n".to_vec();
    let receipt = build(&fixture, &bytes)?;
    assert!(!receipt.payload.trace_decode.is_complete());
    assert!(receipt.payload.rows.is_empty());

    // Partial final row: last frame line without its LF terminator.
    let complete = fixture.emit_complete(&["t/base/if.t"])?;
    let mut partial = complete.clone();
    partial.pop();
    let receipt = build(&fixture, &partial)?;
    assert!(!receipt.payload.trace_decode.is_complete());
    assert_eq!(receipt.payload.work.complete_rows, 0);

    // Producer-declared truncation keeps every row not proven even when the
    // bytes themselves look complete.
    let input = crate::invocation_trace::model::ObservedInvocationTraceInput {
        trace_truncated: true,
        ..fixture.input(complete)
    };
    let receipt = build_invocation_trace_receipt(&input).map_err(|error| eyre!(error))?;
    for row in &receipt.payload.rows {
        assert_eq!(row.state, InvocationObservationState::NotProven);
    }
    assert_eq!(receipt.payload.work.complete_rows, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 14: an instrumentation patch/schema changes while observation
// identity stays the same
// ---------------------------------------------------------------------------

#[test]
fn schema_or_session_drift_changes_observation_identity() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let first = build(&fixture, &bytes)?;
    let drifted = fixture.clone_with_session("trace-session-0002");
    let bytes_two = drifted.emit_complete(&["t/base/if.t"])?;
    let second = build(&drifted, &bytes_two)?;
    assert_ne!(
        first.payload_digest, second.payload_digest,
        "a drifted trace session must change the observation identity"
    );
    // Relabelling the drifted session onto the original receipt breaks its
    // digest binding: identity cannot stay the same.
    let mut forged = first.clone();
    forged.payload.subject.trace_session_id = "trace-session-0002".to_string();
    assert!(validate_trace_receipt_subject_binding(&forged).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 15: host checkout path or map iteration changes canonical output
// ---------------------------------------------------------------------------

#[test]
fn canonical_output_is_host_and_iteration_independent() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let first = build(&fixture, &bytes)?;
    let second = build(&fixture, &bytes)?;
    assert_eq!(first.payload_digest, second.payload_digest);
    // Environment maps are ordered maps at serialization time: insertion
    // order cannot change the canonical bytes, and no host path enters them.
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.environment = EffectiveInvocationField::Observed {
                value: crate::observed_discovery::model::EnvironmentIdentity {
                    variables: [
                        ("ZZ_LAST".to_string(), "z".to_string()),
                        ("AA_FIRST".to_string(), "a".to_string()),
                    ]
                    .into_iter()
                    .collect(),
                    sha256: sha_hex(b"AA_FIRST=a\nZZ_LAST=z\n"),
                },
            };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    assert_eq!(receipt.payload.rows[0].state, InvocationObservationState::ObservedComplete);
    // An absolute path anywhere in an observed value cannot enter the output.
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.run_cwd =
                EffectiveInvocationField::Observed { value: "/host/checkout/t".into() };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    assert!(matches!(
        receipt.payload.rows[0].projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::InvalidObservedValue
        }
    ));
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier 16: the adapter reads source or invokes the runner to fill a gap
// ---------------------------------------------------------------------------

#[test]
fn adapter_fills_no_gap_and_performs_no_side_work() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    // A cwd that exactly equals the script's directory would be derivable by
    // reading the source tree: the adapter must still refuse.
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.run_cwd = EffectiveInvocationField::NotObserved {
                reason: "would be derivable from the source path".to_string(),
            };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    let work = &receipt.payload.work;
    assert_eq!(work.source_reads, 0);
    assert_eq!(work.filesystem_scans, 0);
    assert_eq!(work.runner_processes, 0);
    assert_eq!(work.direct_probe_inputs, 0);
    assert_eq!(work.canonical_plan_projections_rejected, 1);
    assert_eq!(work.canonical_plan_projections_accepted, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// State precedence and counters are pinned
// ---------------------------------------------------------------------------

#[test]
fn row_state_precedence_follows_the_declared_order() -> Result<()> {
    use crate::invocation_trace::derive_row_state;
    use crate::observed_discovery::model::ProcessCompletion as Completion;

    let complete = all_observed_fields("t/base/if.t");
    let mut partial = complete.clone();
    partial.script_arguments = EffectiveInvocationField::NotObserved { reason: "x".into() };
    let mut instrument_failed = complete.clone();
    instrument_failed.run_cwd = EffectiveInvocationField::InstrumentFailure { reason: "x".into() };

    let exit = Completion::ExitStatus { code: 0 };
    // 1. malformed frame, stream malformation, or unknown terminal evidence.
    assert_eq!(
        derive_row_state(false, true, exit, true, &complete),
        InvocationObservationState::NotProven
    );
    assert_eq!(
        derive_row_state(true, false, exit, true, &complete),
        InvocationObservationState::NotProven
    );
    assert_eq!(
        derive_row_state(true, true, Completion::Unknown, true, &complete),
        InvocationObservationState::NotProven
    );
    // 2. instrument failure beats subject mismatch and runner failure.
    assert_eq!(
        derive_row_state(true, true, exit, false, &instrument_failed),
        InvocationObservationState::InstrumentFailed
    );
    assert_eq!(
        derive_row_state(true, true, Completion::ExitStatus { code: 2 }, true, &instrument_failed),
        InvocationObservationState::InstrumentFailed
    );
    // 3. subject mismatch beats runner failure and partiality.
    assert_eq!(
        derive_row_state(true, true, exit, false, &complete),
        InvocationObservationState::SubjectMismatch
    );
    assert_eq!(
        derive_row_state(true, true, exit, false, &partial),
        InvocationObservationState::SubjectMismatch
    );
    // 4. runner failure beats partiality.
    assert_eq!(
        derive_row_state(true, true, Completion::ExitStatus { code: 2 }, true, &partial),
        InvocationObservationState::RunnerFailed
    );
    assert_eq!(
        derive_row_state(true, true, Completion::Signalled { signal: 9 }, true, &partial),
        InvocationObservationState::RunnerFailed
    );
    // 5. partial, then complete.
    assert_eq!(
        derive_row_state(true, true, exit, true, &partial),
        InvocationObservationState::ObservedPartial
    );
    assert_eq!(
        derive_row_state(true, true, exit, true, &complete),
        InvocationObservationState::ObservedComplete
    );
    Ok(())
}

#[test]
fn work_counters_are_pinned_against_the_retained_evidence() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    // One complete row, one partial row, one malformed frame.
    let row_zero = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    let mut partial_fields = all_observed_fields("t/base/cond.t");
    partial_fields.script_arguments =
        EffectiveInvocationField::NotObserved { reason: "not captured".into() };
    let row_one = fixture.row_frame("t/base/cond.t", 1, partial_fields);
    let row_two = fixture.row_frame("t/base/if.t", 2, all_observed_fields("t/base/if.t"));
    let line_zero = row_line_of(&row_zero)?;
    let line_one = row_line_of(&row_one)?;
    let mut line_two_value = serde_json::to_value(&row_two).map_err(|error| eyre!(error))?;
    line_two_value["fields"]["run_cwd"] = json!({"state": "definitely", "payload": "t"});
    let line_two = line_of(&line_two_value)?;
    let header = fixture.header_frame(3);
    let terminal = fixture.terminal_frame(
        &[line_zero.clone(), line_one.clone(), line_two.clone()],
        ProcessCompletion::ExitStatus { code: 0 },
    );
    let bytes = fixture.emit(&header, &[line_zero, line_one, line_two], &terminal);
    let receipt = build(&fixture, &bytes)?;
    let work = &receipt.payload.work;
    assert_eq!(work.trace_rows_consumed, 3);
    assert_eq!(work.trace_frames_consumed, 5); // header + 3 rows + terminal
    assert_eq!(work.complete_rows, 1);
    assert_eq!(work.partial_rows, 1);
    assert_eq!(work.malformed_rows, 1);
    assert_eq!(work.not_proven_rows, 1);
    assert_eq!(work.fields_observed, 17 + 16);
    assert_eq!(work.fields_not_observed, 1 + 17);
    assert_eq!(work.canonical_plan_projections_attempted, 3);
    assert_eq!(work.canonical_plan_projections_accepted, 1);
    assert_eq!(work.canonical_plan_projections_rejected, 2);
    assert_eq!(work.trace_bytes_consumed, bytes.len() as u64);
    ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    Ok(())
}

#[test]
fn serialization_round_trips_and_digests_stay_deterministic() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    let serialized = serde_json::to_vec(&receipt).map_err(|error| eyre!(error))?;
    let round: EffectiveInvocationTraceReceiptV1 =
        serde_json::from_slice(&serialized).map_err(|error| eyre!(error))?;
    assert_eq!(receipt, round);
    ensure(validate_trace_receipt_subject_binding(&round))?;
    assert_eq!(
        crate::invocation_trace::trace_payload_digest(&round.payload)
            .map_err(|error| eyre!(error))?,
        receipt.payload_digest
    );
    Ok(())
}

#[test]
fn freshness_reports_current_and_stale_without_rediscovery() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    assert_eq!(
        crate::invocation_trace::trace_receipt_freshness(&receipt, "prepared-tree-generation-1"),
        crate::observed_discovery::model::ReceiptFreshness::Current
    );
    assert_eq!(
        crate::invocation_trace::trace_receipt_freshness(&receipt, "prepared-tree-generation-2"),
        crate::observed_discovery::model::ReceiptFreshness::Stale
    );
    Ok(())
}

#[test]
fn foreign_evidence_class_and_schema_fail_closed() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    let mut value = serde_json::to_value(&receipt).map_err(|error| eyre!(error))?;
    value["evidence_class"] = json!("observed_upstream");
    let relabelled: EffectiveInvocationTraceReceiptV1 =
        serde_json::from_value(value.clone()).map_err(|error| eyre!(error))?;
    assert!(validate_trace_receipt_subject_binding(&relabelled).is_err());

    let mut drifted = value;
    drifted["schema_version"] = json!("perl_core_harness.upstream_effective_invocation_trace.v2");
    let drifted: EffectiveInvocationTraceReceiptV1 =
        serde_json::from_value(drifted).map_err(|error| eyre!(error))?;
    assert!(validate_trace_receipt_subject_binding(&drifted).is_err());
    Ok(())
}

// ---------------------------------------------------------------------------
// Review discipline: the registered JSON schema agrees with produced receipts
// ---------------------------------------------------------------------------

// The minimal schema validator is shared with every other contract suite
// so one instrument governs schema agreement (#7729).
use crate::schema_check;

#[test]
fn produced_receipt_matches_registered_json_schema() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    // One rich constructor-produced receipt: complete, partial, and malformed
    // rows with accepted and conflicting dispositions.
    let row_zero = fixture.row_frame("t/base/if.t", 0, all_observed_fields("t/base/if.t"));
    let mut partial_fields = all_observed_fields("t/base/cond.t");
    partial_fields.script_arguments =
        EffectiveInvocationField::NotObserved { reason: "not captured".into() };
    partial_fields.return_directory = EffectiveInvocationField::NotApplicable {
        reason: "runner returns to the invocation cwd".into(),
    };
    let row_one = fixture.row_frame("t/base/cond.t", 1, partial_fields);
    let row_two = fixture.row_frame("t/base/if.t", 2, all_observed_fields("t/base/if.t"));
    let line_zero = row_line_of(&row_zero)?;
    let line_one = row_line_of(&row_one)?;
    let mut line_two_value = serde_json::to_value(&row_two).map_err(|error| eyre!(error))?;
    line_two_value["fields"]["run_cwd"] = json!({"state": "definitely", "payload": "t"});
    let line_two = line_of(&line_two_value)?;
    let header = fixture.header_frame(3);
    let terminal = fixture.terminal_frame(
        &[line_zero.clone(), line_one.clone(), line_two.clone()],
        ProcessCompletion::ExitStatus { code: 0 },
    );
    let bytes = fixture.emit(&header, &[line_zero, line_one, line_two], &terminal);
    let receipt = build(&fixture, &bytes)?;

    let schema_path = crate::invocation_trace::test_support::repo_file(
        "schemas/perl_core_harness_upstream_effective_invocation_trace.v1.schema.json",
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(schema_path).map_err(|error| eyre!(error))?)
            .map_err(|error| eyre!(error))?;

    let serialized = serde_json::to_value(&receipt).map_err(|error| eyre!(error))?;
    schema_check::validate(&schema, &serialized)
        .map_err(|error| eyre!("produced receipt violates registered schema: {error}"))?;

    let round: EffectiveInvocationTraceReceiptV1 =
        serde_json::from_value(serialized.clone()).map_err(|error| eyre!(error))?;
    let reserialized = serde_json::to_value(&round).map_err(|error| eyre!(error))?;
    schema_check::validate(&schema, &reserialized)
        .map_err(|error| eyre!("round-tripped receipt violates registered schema: {error}"))?;

    // Discriminators: drifted shapes must be rejected by the registered
    // schema itself, not only by the Rust validators.
    let drift_cases = [
        ("/evidence_class", json!("observed_upstream")),
        ("/payload/runner", json!("direct_fallback")),
        ("/payload/rows/0/state", json!("cancelled")),
        ("/payload/rows/0/projection/outcome", json!("probably")),
    ];
    for (pointer, replacement) in drift_cases {
        let mut mutated = serialized.clone();
        let cursor =
            mutated.pointer_mut(pointer).ok_or_else(|| eyre!("missing JSON pointer {pointer}"))?;
        *cursor = replacement;
        assert!(
            schema_check::validate(&schema, &mutated).is_err(),
            "registered schema must reject drifted shape at {pointer}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Review repairs: discriminating tests for each bot finding
// ---------------------------------------------------------------------------

#[test]
fn empty_trace_stream_types_its_own_reason() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let receipt = build(&fixture, b"")?;
    assert!(!receipt.payload.trace_decode.is_complete());
    match &receipt.payload.trace_decode {
        crate::invocation_trace::model::TraceStreamOutcome::Malformed { reason } => {
            assert!(reason.contains("empty"), "reason must name the empty stream: {reason}");
        }
        _ => bail!("empty stream must carry a typed malformed outcome"),
    }
    assert!(receipt.payload.rows.is_empty());
    Ok(())
}

#[test]
fn cancelled_timed_out_and_instrument_failed_completions_never_complete_rows() -> Result<()> {
    use crate::observed_discovery::model::ProcessCompletion as Completion;
    // Terminal completions without a finished run leave every row not proven;
    // an instrument-failed terminal types the rows instrument-failed.
    for (completion, expected) in [
        (Completion::Cancelled, InvocationObservationState::NotProven),
        (Completion::TimedOut { deadline_millis: 1_000 }, InvocationObservationState::NotProven),
        (Completion::InstrumentFailed, InvocationObservationState::InstrumentFailed),
    ] {
        let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
        let bytes = single_row_stream(&fixture, "t/base/if.t", |_| {}, completion)?;
        let receipt = build(&fixture, &bytes)?;
        assert_eq!(
            receipt.payload.rows[0].state, expected,
            "completion {completion:?} must derive {expected:?}"
        );
        assert!(matches!(receipt.payload.rows[0].projection, ProjectionRecord::Rejected { .. }));
        ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    }
    Ok(())
}

#[test]
fn observed_member_identity_must_equal_the_frame_member_binding() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\nt/base/cond.t\n")?;
    // The frame proves binding for one accepted member while the observed
    // field names another: the projection must refuse the borrowed identity.
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.member_identity =
                EffectiveInvocationField::Observed { value: "t/base/cond.t".to_string() };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    let row = &receipt.payload.rows[0];
    assert!(matches!(
        row.projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::SubjectMismatch
        }
    ));
    Ok(())
}

#[test]
fn environment_identity_digests_are_recomputed_before_projection() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    // A digest that belongs to different retained variables cannot enter an
    // authoritative projection.
    let bytes = single_row_stream(
        &fixture,
        "t/base/if.t",
        |fields| {
            fields.environment = EffectiveInvocationField::Observed {
                value: crate::observed_discovery::model::EnvironmentIdentity {
                    variables: [("LC_ALL".to_string(), "C".to_string())].into_iter().collect(),
                    sha256: sha_hex(b"LC_ALL=en_US.UTF-8\n"),
                },
            };
        },
        ProcessCompletion::ExitStatus { code: 0 },
    )?;
    let receipt = build(&fixture, &bytes)?;
    assert!(matches!(
        receipt.payload.rows[0].projection,
        ProjectionRecord::Rejected {
            reason: crate::invocation_trace::model::ProjectionRejectionKind::InvalidObservedValue
        }
    ));
    Ok(())
}

#[test]
fn trace_runner_must_match_the_parent_discovery_route() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let mut input = fixture.input(bytes);
    input.runner = crate::runner_model::RunnerKind::Harness;
    input.runner_artifact = crate::observed_discovery::model::RunnerArtifactIdentity {
        canonical_path: "t/harness".to_string(),
        content_sha256: sha_hex(b"t/harness"),
    };
    let Err(error) = build_invocation_trace_receipt(&input) else {
        bail!("a harness trace over a t/TEST parent must fail construction");
    };
    assert!(
        error.contains("does not match the parent discovery runner"),
        "rejection must name the parent-route law: {error}"
    );
    Ok(())
}

fn tampered_receipt(
    receipt: &EffectiveInvocationTraceReceiptV1,
    pointer: &str,
    replacement: serde_json::Value,
) -> Result<EffectiveInvocationTraceReceiptV1> {
    let mut value = serde_json::to_value(receipt).map_err(|error| eyre!(error))?;
    let cursor = value.pointer_mut(pointer).ok_or_else(|| eyre!("missing pointer {pointer}"))?;
    *cursor = replacement;
    let tampered: EffectiveInvocationTraceReceiptV1 =
        serde_json::from_value(value).map_err(|error| eyre!(error))?;
    Ok(EffectiveInvocationTraceReceiptV1 {
        payload_digest: crate::invocation_trace::trace_payload_digest(&tampered.payload)
            .map_err(|error| eyre!(error))?,
        ..tampered
    })
}

#[test]
fn tampered_rows_fail_full_validation_even_with_a_recomputed_digest() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    // Row identity drift with a recomputed payload digest must not survive
    // validation: rows are compared field-for-field against reconstruction.
    let tampered =
        tampered_receipt(&receipt, "/payload/rows/0/row_id", serde_json::json!("row-0-forged"))?;
    assert!(validate_trace_receipt_subject_binding(&tampered).is_ok());
    assert!(validate_invocation_trace_receipt(&fixture.parent, &tampered).is_err());
    let tampered = tampered_receipt(
        &receipt,
        "/payload/rows/0/fields/script_path",
        serde_json::json!({"state": "observed", "payload": {"value": "t/base/cond.t"}}),
    )?;
    assert!(validate_invocation_trace_receipt(&fixture.parent, &tampered).is_err());
    Ok(())
}

#[test]
fn tampered_placeholder_header_fails_validation_without_decoded_bytes() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    // Invalid UTF-8 bytes carry no decodable header: the retained header must
    // stay the exact empty placeholder.
    let receipt = build(&fixture, b"\xff\xfe\n")?;
    assert!(receipt.payload.header.trace_session_id.is_empty());
    let tampered = tampered_receipt(
        &receipt,
        "/payload/header/trace_session_id",
        serde_json::json!("trace-session-forged"),
    )?;
    assert!(validate_invocation_trace_receipt(&fixture.parent, &tampered).is_err());
    // Subject-field drift is also caught against the full parent binding.
    let tampered = tampered_receipt(
        &receipt,
        "/payload/subject/perl_ref",
        serde_json::json!("perl-5.99.0-forged"),
    )?;
    assert!(validate_invocation_trace_receipt(&fixture.parent, &tampered).is_err());
    Ok(())
}

#[test]
fn absent_expectations_report_no_expectation_not_a_difference() -> Result<()> {
    let fields = all_observed_fields("t/base/if.t");
    let comparisons = crate::invocation_trace::adapter::compare_expected(
        &fields,
        &crate::invocation_trace::adapter::ExpectedInvocationValues::default(),
    );
    let cwd = comparisons
        .iter()
        .find(|entry| entry.field == FieldKey::RunCwd)
        .ok_or_else(|| eyre!("comparison must cover run_cwd"))?;
    assert_eq!(cwd.result, crate::invocation_trace::adapter::ExpectedFieldResult::NoExpectation);
    Ok(())
}

#[test]
fn registered_schema_rejects_the_consumer_side_stale_state() -> Result<()> {
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    let schema_path = crate::invocation_trace::test_support::repo_file(
        "schemas/perl_core_harness_upstream_effective_invocation_trace.v1.schema.json",
    );
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(schema_path).map_err(|error| eyre!(error))?)
            .map_err(|error| eyre!(error))?;
    let mut value = serde_json::to_value(&receipt).map_err(|error| eyre!(error))?;
    value["payload"]["rows"][0]["state"] = json!("stale");
    assert!(
        schema_check::validate(&schema, &value).is_err(),
        "registered schema must reject the consumer-side stale state"
    );
    // The malformed decode outcome keeps the plain-string wire shape the
    // schema documents.
    let mut malformed = value.clone();
    malformed["payload"]["rows"][0]["state"] = json!("observed_complete");
    malformed["payload"]["trace_decode"] =
        json!({"outcome": "malformed", "reason": "review discriminator"});
    schema_check::validate(&schema, &malformed)
        .map_err(|error| eyre!("malformed outcome must match the registered schema: {error}"))?;
    Ok(())
}

#[test]
fn trace_receipt_intake_rejects_noncanonical_artifact_digest_spelling() -> Result<()> {
    // #7725 review falsifier: a deserialized trace receipt can recompute its
    // own payload digest, so the artifact-digest intake law must be enforced
    // on the shared validation path and cannot rest on construction alone.
    let fixture = TraceFixture::new("component_base", "t/base/if.t\n")?;
    let bytes = fixture.emit_complete(&["t/base/if.t"])?;
    let receipt = build(&fixture, &bytes)?;
    let original = receipt.payload.runner_artifact.content_sha256.clone();

    let uppercase = tampered_receipt(
        &receipt,
        "/payload/runner_artifact/content_sha256",
        json!(original.to_ascii_uppercase()),
    )?;
    assert!(validate_trace_receipt_subject_binding(&uppercase).is_err());
    assert!(validate_invocation_trace_receipt(&fixture.parent, &uppercase).is_err());

    // Flip exactly one case-bearing (letter) nibble, never a digit, so the
    // mutation cannot collapse into the canonical control when the digest
    // happens to start with a hex digit.
    let mut mixed = original.clone();
    let letter_nibble = mixed
        .bytes()
        .position(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_digit())
        .ok_or_else(|| eyre!("fixture artifact digest carries no case-bearing nibble"))?;
    let flipped = mixed[letter_nibble..=letter_nibble].to_ascii_uppercase();
    mixed.replace_range(letter_nibble..=letter_nibble, &flipped);
    assert_ne!(mixed, original, "mixed-case mutation must alter the spelling");
    let mixed_case =
        tampered_receipt(&receipt, "/payload/runner_artifact/content_sha256", json!(mixed))?;
    assert!(validate_trace_receipt_subject_binding(&mixed_case).is_err());

    // Canonical control: the unchanged spelling keeps validating.
    ensure(validate_trace_receipt_subject_binding(&receipt))?;
    ensure(validate_invocation_trace_receipt(&fixture.parent, &receipt))?;
    Ok(())
}
