//! Production provider construction does not require a runtime for native or off selection.
use perl_lsp_rs_core::providers::formatting::{
    FormattingOptions, FormattingProvider, PerlTidyConfig,
};
use perl_lsp_rs_core::tooling::perltidy::native::{
    FormatContext, FormatDisposition, FormatEngine, FormatReasonCode, FormatterMode,
    NativeFormatConfig,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn native_provider_runs_without_a_process_runtime() -> TestResult {
    let result = FormattingProvider::native().format_document_decision(
        "my$x=1;\n",
        &FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        },
        &FormatContext::default(),
    )?;
    assert_eq!(result.outcome.identity.actual_engine, FormatEngine::Native);
    assert_eq!(result.outcome.disposition, FormatDisposition::Applied);
    Ok(())
}

#[test]
fn off_native_no_change_and_missing_external_remain_distinct() -> TestResult {
    let source = "my $x = 1;\n";
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let context = FormatContext::default();
    let native =
        FormattingProvider::native().format_document_decision(source, &options, &context)?;
    let off = FormattingProvider::native()
        .with_formatter_mode(FormatterMode::Off)
        .format_document_decision(source, &options, &context)?;
    let external = FormattingProvider::native()
        .with_formatter_mode(FormatterMode::ExternalLegacy)
        .format_document_decision(source, &options, &context);
    assert_eq!(native.outcome.disposition, FormatDisposition::NoChange);
    assert_eq!(off.outcome.disposition, FormatDisposition::Refused);
    assert_eq!(off.outcome.reason, FormatReasonCode::FormatterDisabled);
    assert_eq!(off.outcome.identity.actual_engine, FormatEngine::Disabled);
    assert!(external.is_err());
    Ok(())
}

#[test]
fn external_process_inputs_cannot_change_native_identity_or_bytes() -> TestResult {
    let baseline = PerlTidyConfig::default();
    let external_inputs = PerlTidyConfig {
        profile: Some("unreadable-and-untrusted.perltidyrc".to_string()),
        extra_args: vec!["--a-flag-native-does-not-interpret".to_string()],
        timeout_secs: u64::MAX,
        ..baseline.clone()
    };
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    let context = FormatContext::default();
    let expected = FormattingProvider::native()
        .with_perltidy_config(baseline)
        .format_document_decision("my$x=1;\n", &options, &context)?;
    let actual = FormattingProvider::native()
        .with_perltidy_config(external_inputs)
        .format_document_decision("my$x=1;\n", &options, &context)?;
    assert_eq!(actual.outcome, expected.outcome);
    assert_eq!(actual.document.text, expected.document.text);
    Ok(())
}

#[test]
fn direct_native_policy_requires_no_external_configuration() -> TestResult {
    let config = NativeFormatConfig { indent_width: 2, ..NativeFormatConfig::default() };
    let result = FormattingProvider::native().with_native_config(config).format_document_decision(
        "sub f {\nmy$x=1;\n}\n",
        &FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            trim_trailing_whitespace: None,
            insert_final_newline: None,
            trim_final_newlines: None,
        },
        &FormatContext::default(),
    )?;
    assert_eq!(result.outcome.identity.actual_engine, FormatEngine::Native);
    assert!(result.document.text.contains("\n  my $x = 1;\n"), "{}", result.document.text);
    Ok(())
}

#[test]
fn installed_runtime_can_borrow_caller_owned_state() -> TestResult {
    use perl_subprocess_runtime::{SubprocessError, SubprocessOutput, SubprocessRuntime};
    use std::sync::atomic::{AtomicBool, Ordering};
    struct BorrowedRuntime<'a>(&'a AtomicBool);
    impl SubprocessRuntime for BorrowedRuntime<'_> {
        fn run_command(
            &self,
            _: &str,
            _: &[&str],
            _: Option<&[u8]>,
        ) -> Result<SubprocessOutput, SubprocessError> {
            self.0.store(true, Ordering::SeqCst);
            Ok(SubprocessOutput {
                stdout: b"my $x = 2;\n".to_vec(),
                stderr: Vec::new(),
                status_code: 0,
            })
        }
    }
    let invoked = AtomicBool::new(false);
    let provider: FormattingProvider<BorrowedRuntime<'_>> =
        FormattingProvider::new(BorrowedRuntime(&invoked));
    let options = FormattingOptions {
        tab_size: 4,
        insert_spaces: true,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
        trim_final_newlines: None,
    };
    provider.format_document("my$x=1;\n", &options)?;
    assert!(!invoked.load(Ordering::SeqCst));
    provider
        .with_formatter_mode(FormatterMode::ExternalLegacy)
        .format_document("my$x=1;\n", &options)?;
    assert!(invoked.load(Ordering::SeqCst));
    Ok(())
}
