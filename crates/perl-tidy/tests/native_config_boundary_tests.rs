//! Native policy cannot select an external engine or carry process configuration.
use perl_tidy::native::{
    FormatContext, FormatDisposition, FormatExecutionStrategy, FormatReasonCode,
    NativeFormatConfig, NativeFormatter, NativePipelineCounters,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn serialized_native_policy_rejects_selection_and_process_fields() -> TestResult {
    for (field, value) in [
        ("mode", serde_json::json!("external-legacy")),
        ("mode", serde_json::json!("off")),
        ("executable", serde_json::json!("/tmp/perltidy")),
        ("profile", serde_json::json!(".perltidyrc")),
        ("extra_args", serde_json::json!(["--unsafe"])),
        ("timeout_secs", serde_json::json!(20)),
        ("environment", serde_json::json!({"PATH": "/tmp"})),
    ] {
        let mut serialized = serde_json::to_value(NativeFormatConfig::default())?;
        serialized[field] = value;
        assert!(serde_json::from_value::<NativeFormatConfig>(serialized).is_err(), "{field}");
    }
    Ok(())
}

#[test]
fn unbounded_indentation_is_refused_before_parsing_or_rendering() {
    let config = NativeFormatConfig { indent_width: u32::MAX, ..NativeFormatConfig::default() };
    let source = "sub f {\nmy$x=1;\n}\n";
    let mut counters = NativePipelineCounters::default();
    let result = NativeFormatter::new().format_document_typed_with_counters(
        source,
        &config,
        &FormatContext::default(),
        &mut counters,
    );
    assert_eq!(result.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(result.outcome.reason, FormatReasonCode::InvalidConfiguration);
    assert_eq!(result.outcome.execution, FormatExecutionStrategy::NotStarted);
    assert_eq!(result.result.formatted, source);
    assert!(result.result.edits.is_empty());
    assert!(!result.result.diagnostics.is_empty());
    assert_eq!(counters.source_parse_gate_invocations, 0);
    assert_eq!(counters.lines_processed, 0);
}

#[test]
fn supported_native_policy_has_stable_identity() {
    let formatter = NativeFormatter::new();
    let source = "my$x=1;\n";
    let config = NativeFormatConfig::default();
    let first = formatter.format_document_typed(source, &config, &FormatContext::default());
    let second = formatter.format_document_typed(source, &config, &FormatContext::default());
    assert_eq!(first, second);
    assert!(first.outcome.identity.config_fingerprint.starts_with("native-format-config-v2:"));
}
