#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
use perl_lsp_perltidy::PerlFormatter;
use perl_lsp_perltidy::native::{
    BracePlacement, ElsePlacement, FormatConfig, FormatContext, FormatDisposition, FormatEngine,
    FormatEvidenceState, FormatReasonCode, FormatRequestTarget, FormatterMode, KeywordSpacing,
    NativeFormatter, TextPosition, TextRange,
};

#[test]
fn typed_document_result_reports_applied_with_identity_and_evidence() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\n";
    let context = FormatContext::new(Some("fixture/basic.pl".to_string()), Some(7));

    let typed = formatter.format_document_typed(source, &FormatConfig::default(), &context);

    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert_eq!(typed.outcome.reason, FormatReasonCode::Applied);
    assert_eq!(typed.outcome.identity.source_id.as_deref(), Some("fixture/basic.pl"));
    assert_eq!(typed.outcome.identity.source_generation, Some(7));
    assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(typed.outcome.identity.requested_mode, FormatterMode::Native);
    assert!(typed.outcome.identity.content_digest.starts_with("source-v1:"));
    assert!(typed.outcome.identity.config_fingerprint.starts_with("format-config-v1:"));
    assert_eq!(typed.outcome.target, FormatRequestTarget::Document);
    assert_eq!(typed.outcome.change.edit_count, typed.result.edits.len());
    assert!(typed.outcome.change.source_bytes_changed > 0);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::Proven);
    assert_eq!(typed.outcome.safety.parse_after, FormatEvidenceState::Proven);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Proven);
}

#[test]
fn typed_document_result_distinguishes_legitimate_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $x = 1;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
    assert_eq!(typed.outcome.change.source_bytes_changed, 0);
    assert_eq!(typed.outcome.change.rendered_bytes_changed, 0);
}

#[test]
fn typed_document_result_does_not_flatten_disabled_mode_to_no_change() {
    let formatter = NativeFormatter::new();
    let config = FormatConfig { mode: FormatterMode::Off, ..FormatConfig::default() };

    let typed = formatter.format_document_typed("my $x = ;\n", &config, &FormatContext::default());

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::FormatterDisabled);
    assert_eq!(typed.outcome.identity.actual_engine, FormatEngine::Disabled);
    assert_eq!(typed.outcome.identity.requested_mode, FormatterMode::Off);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::NotRun);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
}

#[test]
fn typed_document_result_does_not_flatten_literal_refusal_to_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $matched = $text =~ /needle/i;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::LiteralPreservationUnsupported,);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Refused,);
    assert_eq!(typed.result.formatted, source);
    assert!(typed.result.edits.is_empty());
    assert_eq!(typed.result.diagnostics[0].code, "native.format.literal_preserve_region");
}

#[test]
fn typed_document_result_does_not_flatten_parse_refusal_to_no_change() {
    let formatter = NativeFormatter::new();
    let source = "my $x = ;\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::SourceParseError);
    assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::Failed);
    assert_eq!(typed.outcome.safety.parse_after, FormatEvidenceState::NotRun);
    assert_eq!(typed.result.formatted, source);
    assert!(typed.result.edits.is_empty());
    assert_eq!(typed.result.diagnostics[0].code, "native.format.parse_error");
}

#[test]
fn typed_document_result_admits_the_blocks_it_rendered_on_a_second_pass() {
    // #13205 / FPH-008. The block renderers emit headers and tails on their
    // own lines, and neither is re-derivable from one line, so before the
    // already-formatted admission recognized them a block the formatter had
    // just rendered classified as `Refused/UnsupportedSyntax` on the next
    // pass. That is what kept the FPH-008 dormancy fail-closed.
    let formatter = NativeFormatter::new();
    let config = FormatConfig::default();

    let sources = [
        "while($ok){my$x=1;return$x;}\n",
        "while($ok){next;}continue{last;}\n",
        "if($ok){my$x=1;}else{my$y=2;}\n",
        "if($ok){my$x=1;}elsif($z){my$y=2;}else{my$w=3;}\n",
        "unless($ok){my$x=1;}\n",
        "until($ok){my$x=1;}\n",
        "sub foo{my$x=1;return$x;}\n",
    ];

    for source in sources {
        let rendered = formatter.format_document(source, &config);
        assert!(rendered.changed, "source {source:?} was expected to render a block");

        let second = formatter.format_document_typed(
            &rendered.formatted,
            &config,
            &FormatContext::default(),
        );

        assert_eq!(
            second.outcome.disposition,
            FormatDisposition::NoChange,
            "second pass over rendered {:?} must be NoChange",
            rendered.formatted
        );
        assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
        assert!(!second.result.changed);
        assert!(second.result.edits.is_empty(), "second pass must carry no edits");
        assert!(second.result.diagnostics.is_empty());
        assert_eq!(
            second.result.formatted, rendered.formatted,
            "second pass must leave the rendered bytes stable"
        );
        assert_eq!(second.outcome.change.source_bytes_changed, 0);
        assert_eq!(second.outcome.change.rendered_bytes_changed, 0);
    }
}

#[test]
fn typed_document_result_admits_next_line_braces_on_a_second_pass() {
    // The same conversion under `BracePlacement::NextLine`, which is a distinct
    // rendered shape: the header loses its trailing brace and the opening brace
    // becomes its own line. `foreach$e(@list){next;}\r\nforeach my$item(@items){return$item;}\nforeach$e(@list){next;} # tail`
    // is the FPH-008 harness counterexample that caught a brace-terminated-only
    // admission, and it also mixes in CRLF separators, compact keyword spacing,
    // and a trailing tail comment.
    let formatter = NativeFormatter::new();
    let config = FormatConfig {
        brace_placement: BracePlacement::NextLine,
        keyword_spacing: KeywordSpacing::Compact,
        ..FormatConfig::default()
    };
    let source = "foreach$e(@list){next;}\r\nforeach my$item(@items){return$item;}\nforeach$e(@list){next;} # tail";

    let rendered = formatter.format_document(source, &config);
    assert!(rendered.changed, "source was expected to render foreach blocks");
    assert!(
        rendered.formatted.contains("\r\n"),
        "the rendered shape must keep the mixed CRLF separator under test"
    );

    let second =
        formatter.format_document_typed(&rendered.formatted, &config, &FormatContext::default());

    assert_eq!(
        second.outcome.disposition,
        FormatDisposition::NoChange,
        "second pass over NextLine-brace render {:?} must be NoChange",
        rendered.formatted
    );
    assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!second.result.changed);
    assert!(second.result.edits.is_empty());
    assert!(second.result.diagnostics.is_empty());
    assert_eq!(second.result.formatted, rendered.formatted);
}

#[test]
fn typed_document_result_admits_separate_line_clauses_on_a_second_pass() {
    // #16708: under `ElsePlacement::SeparateLine` the else/elsif renderers put
    // the clause header on its own line, and under `BracePlacement::NextLine`
    // that header loses its brace. Neither shape was admitted by the
    // already-formatted check, so the formatter refused its own output on a
    // second pass. Both placements must classify NoChange.
    let formatter = NativeFormatter::new();
    for brace_placement in [BracePlacement::SameLine, BracePlacement::NextLine] {
        let config = FormatConfig {
            else_placement: ElsePlacement::SeparateLine,
            brace_placement,
            ..FormatConfig::default()
        };
        for source in [
            "if($ok){return 1;}else{return 2;}
",
            "if($ok){return 1;}elsif($z){return 2;}else{return 3;}
",
        ] {
            let rendered = formatter.format_document(source, &config);
            assert!(
                rendered.changed,
                "source {source:?} under {brace_placement:?} was expected to render"
            );
            let second = formatter.format_document_typed(
                &rendered.formatted,
                &config,
                &FormatContext::default(),
            );
            assert_eq!(
                second.outcome.disposition,
                FormatDisposition::NoChange,
                "second pass over the SeparateLine render under {brace_placement:?} ({:?}) must be NoChange",
                rendered.formatted
            );
            assert_eq!(second.outcome.reason, FormatReasonCode::AlreadyFormatted);
            assert_eq!(second.result.formatted, rendered.formatted);
        }
    }
}

#[test]
fn typed_document_result_refuses_a_tail_with_an_inline_body() {
    // #16708: a `}`-prefixed tail used to be admitted unconditionally, so an
    // inline unsupported body riding the tail line reached `NoChange` even
    // though no renderer emits that shape. The tail must end at the clause's
    // opening brace.
    let formatter = NativeFormatter::new();
    let source = "if ($ok) {
    my $x = 1;
} else { my %h = (a => 1, b => 2); }
";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_document_result_refuses_a_header_condition_the_renderer_cannot_format() {
    // #16708: header admission checked only the line's endpoints, so a
    // multi-operator condition (which `format_simple_condition_tokens`
    // refuses) endpoint-admitted as `NoChange` despite never passing a
    // renderer. The interior must be validated with the renderer's own
    // condition formatter.
    let formatter = NativeFormatter::new();
    let source = "if ($a && $b && $c) {
    my $x = 1;
}
";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
}

#[test]
fn typed_document_result_still_refuses_a_block_whose_body_is_unsupported() {
    // Negative control for the admission above: recognizing block boundaries
    // must not admit the block *body*. This subject is already in rendered
    // form and its body line parses cleanly with no diagnostic, so only the
    // already-formatted admission can refuse it.
    let formatter = NativeFormatter::new();
    let source = "while ($ok) {\n    my %h = (a => 1, b => 2);\n}\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
    assert!(!typed.result.changed);
    assert!(typed.result.diagnostics.is_empty(), "refusal is by admission, not by diagnostic");
}

#[test]
fn typed_document_result_refuses_a_hand_written_block_the_renderer_never_emitted() {
    // Class falsifier for the #16708 review: boundary *shape* alone cannot
    // claim rendered form. `push_simple_block_body_docs` indents every body
    // line at least one `indent_unit` past its block's header, so an
    // unindented body under an admitted boundary is hand-written input, not
    // formatter output. This is the exact subject the `perl-lsp-rs`
    // formatting-policy receipt tests pin as `Refused/UnsupportedSyntax` with
    // its unsupported-syntax warning. The safety evidence stays unproven:
    // no pipeline pass established parse or literal evidence for this input.
    let formatter = NativeFormatter::new();
    for source in [
        // The receipt-policy subject: supported header, supported unindented
        // body statement, bare tail.
        "sub f {\nreturn 1;\n}\n",
        "while ($ok) {\nreturn 1;\n}\n",
        // `BracePlacement::NextLine` opener with the same unindented body.
        "if ($ok)\n{\nreturn 1;\n}\n",
        // Renderer-consistent body, but the tail sits at a foreign indent.
        "sub f {\n    return 1;\n  }\n",
    ] {
        let typed = formatter.format_document_typed(
            source,
            &FormatConfig::default(),
            &FormatContext::default(),
        );

        assert_eq!(
            typed.outcome.disposition,
            FormatDisposition::Refused,
            "hand-written {source:?} must not classify as already formatted"
        );
        assert_eq!(typed.outcome.reason, FormatReasonCode::UnsupportedSyntax);
        assert!(!typed.result.changed);
        assert!(typed.result.diagnostics.is_empty(), "refusal is by admission, not by diagnostic");
        assert_eq!(typed.outcome.safety.parse_before, FormatEvidenceState::NotRun);
        assert_eq!(typed.outcome.safety.parse_after, FormatEvidenceState::NotRun);
        assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Refused);
    }
}

#[test]
fn typed_document_result_admits_renderer_layout_bytes_without_a_rendering_pass() {
    // The document-level evidence admits exactly the layout the renderer
    // emits — checked directly against those bytes, not only through a first
    // rendering pass: same block as the refused hand-written subject above,
    // with the body at the renderer's own indent.
    let formatter = NativeFormatter::new();
    let source = "sub f {\n    return 1;\n}\n";

    let typed = formatter.format_document_typed(
        source,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(typed.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(typed.outcome.reason, FormatReasonCode::AlreadyFormatted);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
    assert!(typed.result.diagnostics.is_empty());
}

#[test]
fn typed_range_result_records_the_exact_requested_range() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\nmy$y=2;\n";
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::new(Some("fixture/range.pl".to_string()), Some(11)),
    );

    assert_eq!(typed.outcome.target, FormatRequestTarget::Range { range });
    assert_eq!(typed.outcome.identity.source_generation, Some(11));
    assert_eq!(typed.outcome.disposition, FormatDisposition::Applied);
    assert!(typed.result.edits.iter().all(|edit| edit.range.start.line >= 1));
}

#[test]
fn typed_config_fingerprint_changes_with_load_bearing_configuration() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\n";
    let default_config = FormatConfig::default();
    let narrow_config = FormatConfig { line_width: 40, ..FormatConfig::default() };

    let default_result =
        formatter.format_document_typed(source, &default_config, &FormatContext::default());
    let narrow_result =
        formatter.format_document_typed(source, &narrow_config, &FormatContext::default());

    assert_ne!(
        default_result.outcome.identity.config_fingerprint,
        narrow_result.outcome.identity.config_fingerprint,
    );
    assert_eq!(
        default_result.outcome.identity.content_digest,
        narrow_result.outcome.identity.content_digest,
    );
}

#[test]
fn typed_outcome_serializes_without_parsing_diagnostic_prose() -> Result<(), serde_json::Error> {
    let formatter = NativeFormatter::new();
    let typed = formatter.format_document_typed(
        "my $x = ;\n",
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    let value = serde_json::to_value(&typed)?;

    assert_eq!(value["outcome"]["disposition"], "refused");
    assert_eq!(value["outcome"]["reason"], "source_parse_error");
    assert_eq!(value["outcome"]["identity"]["actual_engine"], "native");
    assert!(value["result"]["edits"].as_array().is_some_and(Vec::is_empty));

    Ok(())
}

/// A bare-CR document whose requested range overlaps a heredoc marker must be
/// refused with `LiteralPreservationUnsupported`, not `UnsafeRange`.
///
/// Previously `valid_range` split only on `\n`, so for a bare-CR source the
/// whole document appeared as a single line. Any range referencing line 1+
/// failed the range-validity check and returned `UnsafeRange` before the
/// CR-aware `literal_preserve_region_for_range` guard in `format_range` could
/// fire. Now `valid_range` uses CR-aware line geometry (`\r\n`, `\r`, or `\n`),
/// so the typed path reaches the same literal-preservation refusal as the
/// compatibility `format_range` path.
///
/// Pins issue #13338.
#[test]
fn format_range_typed_refuses_bare_cr_heredoc_as_literal_preservation_not_unsafe_range() {
    let formatter = NativeFormatter::new();
    // Bare-CR document (line endings are bare \r):
    //   line 0: "print <<EOT;"   — heredoc marker
    //   line 1: "EOT"            — terminator
    //   line 2: ""               — trailing empty (after the final \r)
    let source = "print <<EOT;\rEOT\r";
    // Range covering the heredoc marker (line 0) through the terminator (line 1).
    // Before the fix this produced UnsafeRange because valid_range treated the
    // entire bare-CR document as one \n-less line and rejected line 1 as out of
    // bounds.
    let range = TextRange::new(TextPosition::new(0, 0), TextPosition::new(1, 3));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    assert_eq!(
        typed.outcome.reason,
        FormatReasonCode::LiteralPreservationUnsupported,
        "bare-CR heredoc range must be LiteralPreservationUnsupported, not UnsafeRange; got: {:?}",
        typed.outcome.reason,
    );
    assert_eq!(typed.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(typed.outcome.safety.literal_preservation, FormatEvidenceState::Refused);
    assert_eq!(typed.result.formatted, source);
    assert!(!typed.result.changed);
    assert!(typed.result.edits.is_empty());
}

/// Bare-CR documents whose range refers to a line past the first one must still
/// pass the range-validity check when the range itself is syntactically
/// well-formed (i.e. the line exists in the CR-aware view of the document).
///
/// This is a companion to the heredoc test: it verifies the same `valid_range`
/// fix for a clean bare-CR source (no heredoc), confirming that the range
/// correctly proceeds to `format_range` and is not refused as `UnsafeRange`.
#[test]
fn format_range_typed_accepts_valid_range_on_bare_cr_document_without_heredoc() {
    let formatter = NativeFormatter::new();
    // Bare-CR document (no heredoc, no regex, no opaque construct):
    //   line 0: "my$x=1;"
    //   line 1: "my$y=2;"
    //   line 2: ""
    let source = "my$x=1;\rmy$y=2;\r";
    // Range covering only line 1 — valid in the CR-aware view.
    let range = TextRange::new(TextPosition::new(1, 0), TextPosition::new(2, 0));

    let typed = formatter.format_range_typed(
        source,
        range,
        &FormatConfig::default(),
        &FormatContext::default(),
    );

    // The range is valid and the source is clean, so the formatter must reach
    // a formatting decision (Applied or AlreadyFormatted) — not UnsafeRange.
    assert_ne!(
        typed.outcome.reason,
        FormatReasonCode::UnsafeRange,
        "a valid range on a bare-CR source must not be refused as UnsafeRange; got: {:?}",
        typed.outcome.reason,
    );
}
