//! Integration proof for the #13659 safe-point/region registry applicability
//! contract: registered points pass through exact generated edit plans, every
//! point outside registered regions is rejected, boundary negatives stay in
//! accounting with terminal dispositions, and registry output is
//! deterministic and stale-fail-closed.

use std::error::Error;

use perl_lsp_rs_core::hashing::sha256_hex;

mod metrics {
    #[path = "../../src/tasks/metrics/parser_accuracy_metamorphic_registry.rs"]
    pub mod parser_accuracy_metamorphic_registry;
    #[path = "../../src/tasks/metrics/parser_accuracy_metamorphic_transform.rs"]
    pub mod parser_accuracy_metamorphic_transform;
}

use metrics::parser_accuracy_metamorphic_registry::{
    Applicability, CaseDeclaration, CaseOutcome, MetamorphicSafeRegistry, PROFILE_TRAILING_HW,
    PointDecision, PointRequest, REGISTRY_SCHEMA_VERSION, UnregisteredReason, authored_registry,
    authored_registry_inconsistencies,
};

type TestResult = Result<(), Box<dyn Error>>;

/// Exact expected final bytes of every admitted point proposition.
fn expected_point_final_bytes() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("registry-lf-ordinary.trailing-hw.line-1.v1", b"my $x = 1;  \nmy $y = 2;\n" as &[u8]),
        ("registry-crlf-ordinary.trailing-hw.line-1.v1", b"my $x = 1;  \r\nmy $y = 2;\r\n"),
        ("registry-cr-ordinary.trailing-hw.line-1.v1", b"my $x = 1;  \rmy $y = 2;\r"),
        ("registry-eof-no-newline.trailing-hw.eof.v1", b"my $x = 1;\nmy $y = 2;  "),
        ("registry-lf-ordinary.blank-line.stmt-1-2.v1", b"my $x = 1;\n\nmy $y = 2;\n"),
        (
            "registry-lf-ordinary.line-comment.stmt-1-2.v1",
            b"my $x = 1;\n# registry comment\nmy $y = 2;\n",
        ),
        (
            "registry-heredoc-mixed.trailing-hw.ordinary-line-1.v1",
            b"my $x = 1;  \nmy $text = <<'EOF';\nbody line\nEOF\nmy $y = 2;\n",
        ),
    ]
}

#[test]
fn authored_registry_integrity_consult_is_clean_and_schema_pinned() -> TestResult {
    assert_eq!(REGISTRY_SCHEMA_VERSION, 1);
    // The same consult the parser-accuracy generation runs before scoring.
    assert!(authored_registry_inconsistencies().is_empty(), "authored registry surface drifted");
    Ok(())
}

#[test]
fn every_admitted_point_case_generates_the_exact_authored_bytes() -> TestResult {
    let registry = authored_registry()?;
    let outcomes = registry.evaluate();
    for (case_id, expected) in expected_point_final_bytes() {
        let outcome = outcomes
            .iter()
            .find(|outcome| outcome.case_id() == case_id)
            .ok_or_else(|| format!("admitted case {case_id} produced no outcome"))?;
        let declaration = registry
            .declaration(case_id)
            .ok_or_else(|| format!("missing declaration {case_id}"))?;
        assert_eq!(outcome.profile_id(), declaration.profile_id, "profile drift for {case_id}");
        let CaseOutcome::Applied { transformation, .. } = outcome else {
            return Err(format!("case {case_id} did not apply cleanly").into());
        };
        assert_eq!(transformation.final_bytes, expected, "final bytes drift for {case_id}");
        assert_eq!(
            transformation.final_source_identity,
            sha256_hex(expected),
            "final identity drift for {case_id}"
        );
        // The coordinate relation covers exactly the base and final bytes.
        let base_bytes = registry
            .fixture_bytes(declaration.fixture_id)
            .ok_or_else(|| format!("missing fixture {}", declaration.fixture_id))?;
        assert_eq!(transformation.coordinate_map.base_len(), base_bytes.len());
        assert_eq!(
            transformation.coordinate_map.transformed_len(),
            transformation.final_bytes.len()
        );
        assert!(!transformation.coordinate_map.segments().is_empty());
    }
    Ok(())
}

#[test]
fn region_conversion_and_opposite_control_round_trip_exactly() -> TestResult {
    let registry = authored_registry()?;
    let outcomes = registry.evaluate();

    let primary_id = "registry-lf-region.newline-style.lf-to-crlf.v1";
    let control_id = "registry-crlf-region.newline-style.crlf-to-lf.control.v1";

    let primary_outcome = outcomes
        .iter()
        .find(|outcome| outcome.case_id() == primary_id)
        .ok_or_else(|| format!("missing outcome for {primary_id}"))?;
    let control_outcome = outcomes
        .iter()
        .find(|outcome| outcome.case_id() == control_id)
        .ok_or_else(|| format!("missing outcome for {control_id}"))?;
    let CaseOutcome::Applied { transformation: converted, .. } = primary_outcome else {
        return Err(format!("case {primary_id} did not apply cleanly").into());
    };
    let CaseOutcome::Applied { transformation: reversed, .. } = control_outcome else {
        return Err(format!("case {control_id} did not apply cleanly").into());
    };

    // The conversion produced the exact declared CRLF presentation.
    assert_eq!(converted.final_bytes, b"use strict;\r\nuse warnings;\r\nmy $x = 1;\r\n");
    // The opposite-direction control returns the exact original LF bytes.
    assert_eq!(reversed.final_bytes, b"use strict;\nuse warnings;\nmy $x = 1;\n");

    // Both propositions link their opposite-direction control explicitly.
    let primary = registry
        .declaration(primary_id)
        .ok_or_else(|| format!("missing declaration {primary_id}"))?;
    let control = registry
        .declaration(control_id)
        .ok_or_else(|| format!("missing declaration {control_id}"))?;
    assert_eq!(primary.opposite_control, Some(control_id));
    assert_eq!(control.opposite_control, Some(primary_id));

    // Generated region plans anchor every edit exactly on its conversion site.
    for (case, site_width) in [(primary, 1usize), (control, 2usize)] {
        let plan = registry
            .edit_plan(case.case_id)
            .ok_or_else(|| format!("missing plan for {}", case.case_id))?;
        assert!(!plan.is_empty(), "empty region plan for {}", case.case_id);
        for edit in &plan {
            assert!(
                edit.edit_id().starts_with(case.case_id),
                "edit identity drift for {}",
                case.case_id
            );
            assert_eq!(
                edit.base_range().len(),
                site_width,
                "conversion site width drift for {}",
                case.case_id
            );
        }
    }
    Ok(())
}

#[test]
fn boundary_negatives_retain_terminal_dispositions_and_never_apply() -> TestResult {
    let registry = authored_registry()?;
    let outcomes = registry.evaluate();
    let mut dispositioned = 0;
    for outcome in &outcomes {
        let CaseOutcome::Dispositioned { case_id, state, reason, .. } = outcome else {
            continue;
        };
        dispositioned += 1;
        assert_ne!(*state, Applicability::Admitted, "case {case_id} dispositioned as admitted");
        assert!(!reason.is_empty(), "case {case_id} lost its terminal reason");
        assert_eq!(
            state.as_str(),
            match state {
                Applicability::NotApplicable => "not_applicable",
                Applicability::UnsupportedTransformation => "unsupported_transformation",
                Applicability::NotProven => "not_proven",
                Applicability::Admitted => "admitted",
            },
            "state identity drift for {case_id}"
        );
    }
    assert_eq!(dispositioned, 9, "disposition accounting drifted");

    // None of the fail-closed boundary families produced an applied outcome.
    let boundary_fragments = [
        "q-body",
        "hash-delimiter",
        "heredoc-body",
        "heredoc-terminator",
        "format-body",
        "pod-block",
        "data-payload",
        "recovery-boundary",
    ];
    for fragment in boundary_fragments {
        let outcome = outcomes
            .iter()
            .find(|outcome| outcome.case_id().contains(fragment))
            .ok_or_else(|| format!("boundary case {fragment} silently omitted"))?;
        assert!(
            matches!(outcome, CaseOutcome::Dispositioned { .. }),
            "boundary case {} was not dispositioned",
            outcome.case_id()
        );
    }
    Ok(())
}

#[test]
fn heredoc_marker_does_not_remove_the_registered_ordinary_point() -> TestResult {
    let registry = authored_registry()?;
    let outcomes = registry.evaluate();

    // The ordinary point in the heredoc fixture was explicitly registered and
    // applies exactly (negative 3: one marker never erases authored points).
    let contrast_id = "registry-heredoc-mixed.trailing-hw.ordinary-line-1.v1";
    let contrast_outcome = outcomes
        .iter()
        .find(|outcome| outcome.case_id() == contrast_id)
        .ok_or_else(|| format!("missing outcome for {contrast_id}"))?;
    let CaseOutcome::Applied { transformation, .. } = contrast_outcome else {
        return Err(format!("case {contrast_id} did not apply cleanly").into());
    };
    assert_eq!(
        transformation.final_bytes,
        b"my $x = 1;  \nmy $text = <<'EOF';\nbody line\nEOF\nmy $y = 2;\n"
    );

    // While the heredoc body and terminator stay dispositioned not-applicable.
    for case_id in [
        "registry-heredoc-mixed.trailing-hw.heredoc-body.v1",
        "registry-heredoc-mixed.trailing-hw.heredoc-terminator.v1",
    ] {
        let outcome = outcomes
            .iter()
            .find(|outcome| outcome.case_id() == case_id)
            .ok_or_else(|| format!("missing outcome for {case_id}"))?;
        assert!(
            matches!(
                outcome,
                CaseOutcome::Dispositioned { state: Applicability::NotApplicable, .. }
            ),
            "case {case_id} lost its not-applicable disposition"
        );
    }
    Ok(())
}

#[test]
fn substring_markers_carry_no_authority_in_either_direction() -> TestResult {
    let registry = authored_registry()?;

    // Boundary markers (`<<` at the intro line of the heredoc fixture) do not
    // make admission decisions: the point is simply unregistered.
    let intro_marker_offset = "my $x = 1;\nmy $text = <<".len();
    assert_eq!(
        registry.admission(&PointRequest {
            fixture_id: "registry-heredoc-mixed",
            profile_id: PROFILE_TRAILING_HW,
            offset: intro_marker_offset,
        }),
        PointDecision::NotRegistered {
            reason: UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint,
        }
    );

    // A `__DATA__` payload point stays rejected even though the legacy
    // exclusion marker is present: no substring is an authority here either.
    let fixture_bytes = registry
        .fixture_bytes("registry-format-pod-data")
        .ok_or_else(|| "missing fixture registry-format-pod-data".to_string())?;
    let data_marker = b"__DATA__";
    let marker_offset = fixture_bytes
        .windows(data_marker.len())
        .position(|window| window == data_marker)
        .ok_or_else(|| "__DATA__ marker missing from fixture".to_string())?;
    assert_eq!(
        registry.admission(&PointRequest {
            fixture_id: "registry-format-pod-data",
            profile_id: PROFILE_TRAILING_HW,
            offset: marker_offset + 3,
        }),
        PointDecision::NotRegistered {
            reason: UnregisteredReason::OffsetOutsideEveryRegisteredSafePoint,
        }
    );
    Ok(())
}

#[test]
fn tampered_source_fails_every_bound_declaration_stale_and_closed() -> TestResult {
    let registry = authored_registry()?;
    let original = registry
        .fixture_bytes("registry-quote-payload")
        .ok_or_else(|| "missing fixture registry-quote-payload".to_string())?;
    let mut tampered = original.to_vec();
    tampered[11] = b't';
    let outcomes = registry.evaluate_source("registry-quote-payload", &tampered)?;
    // Negative 8: stale bytes produce typed stale outcomes, never skips; the
    // declared denominator for the fixture stays fully accounted.
    assert_eq!(outcomes.len(), 2, "quote fixture denominator drifted");
    for outcome in &outcomes {
        let CaseOutcome::StaleSource { claimed, observed, .. } = outcome else {
            return Err(format!(
                "tampered bytes produced non-stale outcome for {}",
                outcome.case_id()
            )
            .into());
        };
        assert_ne!(claimed, observed, "identity collision for {}", outcome.case_id());
    }
    Ok(())
}

#[test]
fn shuffled_registry_order_preserves_case_ids_and_bytes() -> TestResult {
    let registry = authored_registry()?;
    let expected = registry.evaluate();
    let expected_ids = registry.case_ids();

    let mut cases: Vec<CaseDeclaration> = Vec::new();
    for case_id in registry.case_ids() {
        let declaration = registry
            .declaration(case_id)
            .ok_or_else(|| format!("missing declaration {case_id}"))?;
        cases.push(declaration.clone());
    }
    cases.reverse();
    let shuffled = MetamorphicSafeRegistry::from_declarations(cases)?;

    assert_eq!(shuffled.case_ids(), expected_ids);
    assert_eq!(shuffled.evaluate(), expected);
    Ok(())
}

#[test]
fn declared_denominator_stays_closed_over_every_evaluation() -> TestResult {
    let registry = authored_registry()?;
    let outcomes = registry.evaluate();
    let case_ids = registry.case_ids();
    assert_eq!(outcomes.len(), case_ids.len());
    for (outcome, case_id) in outcomes.iter().zip(case_ids.iter()) {
        assert_eq!(outcome.case_id(), *case_id, "outcome order drifted");
    }
    for case_id in &case_ids {
        assert!(
            outcomes.iter().any(|outcome| outcome.case_id() == *case_id),
            "case {case_id} silently omitted from accounting"
        );
    }
    Ok(())
}

#[test]
fn generation_only_runs_for_admitted_cases() -> TestResult {
    let registry = authored_registry()?;
    for case_id in registry.case_ids() {
        let case = registry
            .declaration(case_id)
            .ok_or_else(|| format!("missing declaration {case_id}"))?;
        let plan = registry.edit_plan(case_id);
        if case.applicability.state == Applicability::Admitted {
            assert!(plan.is_some(), "admitted case {case_id} has no generated plan");
        } else {
            assert!(plan.is_none(), "case {case_id} generated a plan while not admitted");
        }
    }
    Ok(())
}
