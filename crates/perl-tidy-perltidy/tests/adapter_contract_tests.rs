use perl_subprocess_runtime::mock::{MockResponse, MockSubprocessRuntime};
use perl_subprocess_runtime::{SubprocessError, SubprocessOutput, SubprocessRuntime};
use perl_tidy_perltidy::{ExternalPerlTidyConfig, ExternalPerlTidyError, LegacyPerlTidyExecutor};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn executor_passes_program_argv_and_exact_source_without_discovery() -> TestResult {
    let runtime = MockSubprocessRuntime::new();
    runtime.add_response(MockResponse::success(b"my $x = 1;\r\n".to_vec()));
    let args =
        vec!["-st".to_string(), "-se".to_string(), "--profile=path with spaces.rc".to_string()];
    let executor = LegacyPerlTidyExecutor::new(&runtime);
    assert!(runtime.invocations().is_empty(), "construction must not invoke the runtime");
    let formatted = executor.format("/explicit path/perltidy", &args, "my$x=1;\r\n")?;
    assert_eq!(formatted, "my $x = 1;\r\n");
    let calls = runtime.invocations();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].program, "/explicit path/perltidy");
    assert_eq!(calls[0].args, args);
    assert_eq!(calls[0].stdin.as_deref(), Some(b"my$x=1;\r\n".as_slice()));
    Ok(())
}

#[test]
fn profile_suppresses_scalar_defaults_but_keeps_explicit_indent_tabs_and_extras() {
    let config = ExternalPerlTidyConfig {
        profile: Some("unreadable profile.rc".into()),
        indent_columns: Some(2),
        tabs: Some(false),
        extra_args: vec!["--maximum-line-length=93".into()],
        ..Default::default()
    };
    assert_eq!(
        config.to_args(),
        [
            "--profile=unreadable profile.rc",
            "--indent-columns=2",
            "--notabs",
            "--maximum-line-length=93",
        ]
    );
    let profile_only = ExternalPerlTidyConfig { profile: config.profile, ..Default::default() };
    assert_eq!(profile_only.to_args(), ["--profile=unreadable profile.rc"]);
}

#[test]
fn presets_and_serialization_keep_the_existing_external_contract() -> TestResult {
    let default = ExternalPerlTidyConfig::default();
    assert_eq!(default.maximum_line_length, Some(80));
    assert_eq!(default.indent_columns, None);
    assert_eq!(default.tabs, None);
    assert_eq!(default.timeout_secs, 10);
    for config in [default, ExternalPerlTidyConfig::pbp(), ExternalPerlTidyConfig::gnu()] {
        let args = config.to_args();
        let serialized = serde_json::to_string(&config)?;
        let decoded: ExternalPerlTidyConfig = serde_json::from_str(&serialized)?;
        assert_eq!(decoded.to_args(), args);
        assert_eq!(decoded.timeout_secs, config.timeout_secs);
    }
    Ok(())
}

#[test]
fn failed_tools_withhold_stdout_and_preserve_missing_command_classification() -> TestResult {
    for (status, stderr, missing) in [
        (9009, "", true),
        (1, "'perltidy' is not recognized as an internal or external command", true),
        (1, "Access is denied.", false),
        (1, "The filename, directory name, or volume label syntax is incorrect.", false),
        (1, "syntax error at (stdin) line 2", false),
    ] {
        let runtime = MockSubprocessRuntime::new();
        runtime.add_response(MockResponse {
            stdout: b"untrusted output must not escape".to_vec(),
            stderr: stderr.as_bytes().to_vec(),
            status_code: status,
        });
        let error = LegacyPerlTidyExecutor::new(&runtime)
            .format("perltidy", &[], "source")
            .err()
            .ok_or("non-success output must be withheld")?;
        match error {
            ExternalPerlTidyError::CommandNotFound { status_code, stderr: actual } => {
                assert!(missing);
                assert_eq!(status_code, status);
                assert_eq!(actual, stderr);
            }
            ExternalPerlTidyError::Rejected { status_code, stderr: actual } => {
                assert!(!missing);
                assert_eq!(status_code, status);
                assert_eq!(actual, stderr);
            }
            other => return Err(format!("unexpected failure class: {other:?}").into()),
        }
    }
    Ok(())
}

#[test]
fn invalid_utf8_from_successful_tool_is_not_lossily_accepted() -> TestResult {
    let runtime = MockSubprocessRuntime::new();
    runtime.add_response(MockResponse::success(vec![0xff]));
    let error = LegacyPerlTidyExecutor::new(&runtime)
        .format("perltidy", &[], "source")
        .err()
        .ok_or("invalid stdout must fail")?;
    assert!(matches!(&error, ExternalPerlTidyError::InvalidUtf8 { .. }));
    assert!(error.to_string().starts_with("Invalid UTF-8 from perltidy:"));
    Ok(())
}

struct FailedRuntime;
impl SubprocessRuntime for FailedRuntime {
    fn run_command(
        &self,
        _: &str,
        _: &[&str],
        _: Option<&[u8]>,
    ) -> Result<SubprocessOutput, SubprocessError> {
        Err(SubprocessError::new("injected runtime failed"))
    }
}

#[test]
fn runtime_error_remains_distinct_from_completed_tool_failure() -> TestResult {
    let error = LegacyPerlTidyExecutor::new(&FailedRuntime)
        .format("perltidy", &[], "source")
        .err()
        .ok_or("runtime error must fail")?;
    assert!(
        matches!(&error, ExternalPerlTidyError::Runtime { message } if message == "injected runtime failed")
    );
    assert_eq!(error.to_string(), "injected runtime failed");
    Ok(())
}
