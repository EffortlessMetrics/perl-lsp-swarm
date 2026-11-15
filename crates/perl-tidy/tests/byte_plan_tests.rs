//! Behavior and hostile-input checks for the byte-native public boundary.
use perl_source_identity::{
    LogicalSourceId, ProjectId, RootRelativeLogicalPath, SourceGeneration, WorkspaceRootId,
};
use perl_tidy::{
    EditPlanError, ExactSource, FormatContext, FormatDisposition, FormatEdit, FormatEditPlan,
    FormatExecutionStrategy, FormatReasonCode, FormatRequest, FormatRequestTarget,
    FormatSourceIdentity, NativeFormatConfig, NativeFormatter, SourceByteRange, TargetWidening,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn named_source<'a>(
    text: &'a str,
    path: &str,
    generation: &str,
) -> Result<ExactSource<'a>, Box<dyn std::error::Error>> {
    let project = ProjectId::from_canonical_name("formatter-byte-contract-tests");
    let root = WorkspaceRootId::from_project_and_root_key(&project, "fixture-root");
    let logical =
        LogicalSourceId::from_root_and_logical_path(&root, &RootRelativeLogicalPath::parse(path)?);
    let mut identity = FormatSourceIdentity::content_only(text);
    identity.logical_source = Some(logical);
    identity.generation = SourceGeneration::known(generation);
    Ok(ExactSource::with_identity(text, identity)?)
}

#[test]
fn document_target_is_exact_utf8_bytes_for_every_line_ending() -> TestResult {
    for text in [
        "",
        "ascii",
        "é",
        "😀",
        "e\u{301}",
        "\u{feff}my$x=1;",
        "a\n",
        "a\r\n",
        "a\r",
        "a\r\nb\rc\n",
    ] {
        let source = ExactSource::new(text);
        let request = FormatRequest::document(&source);
        request.validate(&source)?;
        assert_eq!(request.admitted.start(), 0);
        assert_eq!(request.admitted.end(), text.len());
        assert_eq!(request.admitted.source(), source.identity());
    }
    Ok(())
}

#[test]
fn ranges_reject_reversed_past_end_and_mid_scalar_without_clamping() -> TestResult {
    let source = ExactSource::new("a😀éz");
    assert_eq!(SourceByteRange::new(&source, 5, 1), Err(EditPlanError::ReversedRange));
    assert_eq!(
        SourceByteRange::new(&source, 0, source.text().len() + 1),
        Err(EditPlanError::OutOfBounds)
    );
    for offset in [2, 3, 4, 6] {
        assert_eq!(
            SourceByteRange::new(&source, 0, offset),
            Err(EditPlanError::NotCharacterBoundary)
        );
    }
    assert!(SourceByteRange::new(&source, 1, 5).is_ok());
    assert!(SourceByteRange::new(&source, source.text().len(), source.text().len()).is_ok());
    Ok(())
}

#[test]
fn plan_applies_several_edits_and_reproduces_one_exact_render() -> TestResult {
    let source = ExactSource::new("a😀éz\r\n");
    let edits = vec![
        FormatEdit::new(SourceByteRange::new(&source, 0, 1)?, "A"),
        FormatEdit::new(SourceByteRange::new(&source, 5, 7)?, "e\u{301}"),
        FormatEdit::new(
            SourceByteRange::new(&source, source.text().len(), source.text().len())?,
            "# tail\n",
        ),
    ];
    let expected = "A😀e\u{301}z\r\n# tail\n";
    let plan = FormatEditPlan::new(
        &source,
        FormatRequest::document(&source),
        FormatExecutionStrategy::NativeDocument,
        edits,
        expected,
    )?;
    assert_eq!(plan.apply(&source)?, expected);
    assert_eq!(plan.coverage().len(), 3);
    let serialized = serde_json::to_string(&plan)?;
    let decoded: FormatEditPlan = serde_json::from_str(&serialized)?;
    assert_eq!(serde_json::to_string(&decoded)?, serialized);
    assert_eq!(decoded.apply(&source)?, expected);
    Ok(())
}

#[test]
fn complete_plan_rejects_overlap_duplicate_and_mixed_subjects() -> TestResult {
    let source = named_source("abc", "a.pl", "1")?;
    let other = named_source("abc", "b.pl", "1")?;
    for (edits, expected) in [
        (
            vec![
                FormatEdit::new(SourceByteRange::new(&source, 0, 2)?, "x"),
                FormatEdit::new(SourceByteRange::new(&source, 1, 3)?, "y"),
            ],
            EditPlanError::OverlappingEdits,
        ),
        (
            vec![
                FormatEdit::new(SourceByteRange::new(&source, 3, 3)?, "x"),
                FormatEdit::new(SourceByteRange::new(&source, 3, 3)?, "y"),
            ],
            EditPlanError::DuplicateEdit,
        ),
        (
            vec![FormatEdit::new(SourceByteRange::new(&other, 0, 1)?, "x")],
            EditPlanError::MixedSource,
        ),
    ] {
        assert_eq!(
            FormatEditPlan::new(
                &source,
                FormatRequest::document(&source),
                FormatExecutionStrategy::NativeDocument,
                edits,
                "does not matter"
            ),
            Err(expected)
        );
    }
    Ok(())
}

#[test]
fn deserialized_plan_cannot_forge_bounds_coverage_execution_or_render() -> TestResult {
    let source = ExactSource::new("abc");
    let plan = FormatEditPlan::new(
        &source,
        FormatRequest::document(&source),
        FormatExecutionStrategy::NativeDocument,
        vec![FormatEdit::new(SourceByteRange::new(&source, 0, 1)?, "A")],
        "Abc",
    )?;
    let base = serde_json::to_value(&plan)?;
    for path in ["execution", "schema", "coverage", "rendered", "request"] {
        let mut value = base.clone();
        match path {
            "execution" => value["execution"] = serde_json::json!("disabled"),
            "schema" => value["schema"] = serde_json::json!(900),
            "coverage" => value["coverage"][0]["end"] = serde_json::json!(2),
            "rendered" => {
                value["rendered"] =
                    serde_json::to_value(FormatSourceIdentity::content_only("forged").content)?
            }
            "request" => value["request"]["admitted"]["end"] = serde_json::json!(99),
            _ => return Err(format!("unknown plan forgery path: {path}").into()),
        }
        let forged: FormatEditPlan = serde_json::from_value(value)?;
        assert!(forged.apply(&source).is_err(), "forgery: {path}");
    }
    Ok(())
}

#[test]
fn requested_admitted_execution_and_coverage_remain_distinct() -> TestResult {
    let source = ExactSource::new("my$x=1;\nmy$y=2;\n");
    let requested = SourceByteRange::new(&source, 2, 4)?;
    let admitted = SourceByteRange::new(&source, 0, 8)?;
    let request = FormatRequest {
        requested: FormatRequestTarget::Range { range: requested.clone() },
        admitted: admitted.clone(),
        widening: Some(TargetWidening {
            owner: "test caller".to_string(),
            reason: "explicit complete-line policy".to_string(),
        }),
    };
    let typed = NativeFormatter::new().format(
        &source,
        &request,
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range: requested });
    assert_eq!(typed.outcome.admitted, Some(admitted));
    assert_eq!(typed.outcome.execution, FormatExecutionStrategy::NativeRangeOrIsland);
    assert_eq!(typed.outcome.coverage[0].end(), 7);
    assert_eq!(typed.apply_exact(&source)?, "my $x = 1;\nmy$y=2;\n");
    Ok(())
}

#[test]
fn stale_identity_is_a_stale_source_reason_not_bad_geometry() -> TestResult {
    let current = named_source("my$x=1;\n", "a.pl", "2")?;
    let stale = named_source(current.text(), "a.pl", "1")?;
    let result = NativeFormatter::new().format(
        &current,
        &FormatRequest::document(&stale),
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(result.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(result.outcome.reason, FormatReasonCode::StaleSource);
    assert_eq!(result.outcome.execution, FormatExecutionStrategy::NotStarted);
    assert!(result.plan.is_none());
    assert_eq!(result.apply_exact(&current)?, current.text());
    Ok(())
}

#[test]
fn refusal_is_bound_to_logical_source_and_generation_even_with_identical_bytes() -> TestResult {
    let source = named_source("my $x = ;\n", "a.pl", "1")?;
    let other = named_source(source.text(), "b.pl", "1")?;
    let newer = named_source(source.text(), "a.pl", "2")?;
    let result = NativeFormatter::new().format(
        &source,
        &FormatRequest::document(&source),
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(result.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(result.apply_exact(&source)?, source.text());
    assert_eq!(result.apply_exact(&other), Err(EditPlanError::StaleSource));
    assert_eq!(result.apply_exact(&newer), Err(EditPlanError::StaleSource));
    assert!(
        result.apply_to(source.text()).is_err(),
        "anonymous input cannot replace logical currentness"
    );
    Ok(())
}

#[test]
fn anonymous_document_outcomes_are_independently_applicable() -> TestResult {
    for source in [
        "",
        "my$x=1;\n",
        "my $x = 1;\r\n",
        "my $x = ;\n",
        "# 😀 e\u{301}\r\nmy$x=1;\r\n",
        "\u{feff}my$x=1;\n",
    ] {
        let result = NativeFormatter::new().format_document_typed(
            source,
            &NativeFormatConfig::default(),
            &FormatContext::default(),
        );
        assert_eq!(result.apply_to(source)?, result.result.formatted);
        assert_eq!(result.plan.is_some(), result.outcome.disposition == FormatDisposition::Applied);
        if result.outcome.disposition != FormatDisposition::Applied {
            assert!(result.result.edits.is_empty());
        }
    }
    Ok(())
}

#[test]
fn deserialized_terminal_envelope_cannot_turn_refusal_into_success() -> TestResult {
    let source = "my $x = ;\n";
    let typed = NativeFormatter::new().format_document_typed(
        source,
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.apply_to(source)?, source);
    let mut value = serde_json::to_value(&typed)?;
    value["outcome"]["disposition"] = serde_json::json!("no_change");
    let forged: perl_tidy::TypedFormatResult = serde_json::from_value(value)?;
    assert_eq!(forged.apply_to(source), Err(EditPlanError::InvalidOutcome));

    let source = "my$x=1;\n";
    let typed = NativeFormatter::new().format_document_typed(
        source,
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    let base = serde_json::to_value(&typed)?;
    for field in ["reason", "change", "safety", "disposition"] {
        let mut value = base.clone();
        match field {
            "reason" => value["outcome"]["reason"] = serde_json::json!("already_formatted"),
            "change" => value["outcome"]["change"]["edit_count"] = serde_json::json!(0),
            "safety" => value["outcome"]["safety"]["parse_before"] = serde_json::json!("not_run"),
            "disposition" => value["outcome"]["disposition"] = serde_json::json!("refused"),
            _ => return Err(format!("unknown outcome forgery field: {field}").into()),
        }
        let forged: perl_tidy::TypedFormatResult = serde_json::from_value(value)?;
        assert_eq!(forged.apply_to(source), Err(EditPlanError::InvalidOutcome), "forgery: {field}");
    }
    Ok(())
}

#[test]
fn ordinary_multiline_string_preservation_is_explicit_in_the_outcome() -> TestResult {
    let source = "my$x=1;\nmy $text = \"value  \n\";\n";
    let typed = NativeFormatter::new().format_document_typed(
        source,
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AppliedPreservingLiterals);
    assert_eq!(
        typed.outcome.safety.literal_preservation,
        perl_tidy::FormatEvidenceState::PreservedPartially
    );
    assert!(typed.result.formatted.contains("value  \n\";"));
    assert_eq!(typed.apply_to(source)?, typed.result.formatted);
    Ok(())
}
