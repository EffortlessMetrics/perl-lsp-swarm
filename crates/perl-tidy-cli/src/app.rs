use crate::args::{Args, Operation};
use crate::input::{self, Snapshot, read_bounded};
use perl_source_identity::ContentDigest;
use perl_tidy::{
    FormatContext, FormatDisposition, FormatReasonCode, NativeFormatConfig, NativeFormatter,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::PathBuf;

const SCHEMA: &str = "perl_tidy_cli.v1";
const MAX_FILES: usize = 1024;
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Process status is separate from the canonical native formatter disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExitClass {
    Success,
    WouldChange,
    Refused,
    InvalidConfiguration,
    ResourceLimited,
    StaleSource,
    FilesystemError,
    FailedOrNotProven,
    InvalidInput,
    Usage,
    OutputError,
    InstrumentFailure,
}

impl ExitClass {
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::WouldChange => 1,
            Self::Refused => 2,
            Self::InvalidConfiguration => 3,
            Self::ResourceLimited => 4,
            Self::StaleSource => 6,
            Self::FilesystemError => 7,
            Self::FailedOrNotProven => 8,
            Self::InvalidInput => 9,
            Self::Usage => 10,
            Self::OutputError => 11,
            Self::InstrumentFailure => 12,
        }
    }

    fn blocks_formatting(self) -> bool {
        !matches!(self, Self::Success | Self::WouldChange)
    }

    fn join(self, next: Self) -> Self {
        if next.code() > self.code() { next } else { self }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Failure {
    pub(crate) class: ExitClass,
    pub(crate) message: String,
}

impl Failure {
    pub(crate) fn new(class: ExitClass, message: impl Into<String>) -> Self {
        Self { class, message: message.into() }
    }
}

#[derive(Debug, Serialize)]
struct ConfigurationReport {
    authority: &'static str,
    library_version: &'static str,
    production_profile: Option<&'static str>,
    project_resolution: &'static str,
    effective: Value,
    field_sources: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct SubjectReport {
    subject: String,
    logical_stdin_filename: Option<String>,
    source_digest: ContentDigest,
    source_bytes: usize,
    rendered_digest: ContentDigest,
    rendered_bytes: usize,
    native_outcome: Value,
    diagnostics: Value,
    edit_application_verified: bool,
    exit_class: ExitClass,
}

#[derive(Serialize)]
struct Report {
    schema: &'static str,
    executable: &'static str,
    package_version: &'static str,
    engine: &'static str,
    operation: Operation,
    configuration: Option<ConfigurationReport>,
    subjects: Vec<SubjectReport>,
    complete: bool,
    exit_class: ExitClass,
    error: Option<Failure>,
}

struct Execution {
    report: Report,
    source_output: Option<String>,
}

struct Input {
    name: String,
    logical_stdin_filename: Option<String>,
    source: String,
    snapshot: Option<Snapshot>,
}

struct Formatted {
    rendered: String,
    report: SubjectReport,
}

pub(crate) fn run(
    args: &Args,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<ExitClass, Failure> {
    let execution = match execute(args, stdin) {
        Ok(execution) => execution,
        Err(error) => Execution {
            report: Report {
                schema: SCHEMA,
                executable: "perl-tidy",
                package_version: env!("CARGO_PKG_VERSION"),
                engine: "native",
                operation: args.operation(),
                configuration: None,
                subjects: Vec::new(),
                complete: false,
                exit_class: error.class,
                error: Some(error),
            },
            source_output: None,
        },
    };
    let class = execution.report.exit_class;
    if args.json {
        serde_json::to_writer_pretty(&mut *stdout, &execution.report).map_err(|error| {
            Failure::new(ExitClass::OutputError, format!("cannot write JSON output: {error}"))
        })?;
        stdout.write_all(b"\n").map_err(output_error)?;
    } else if let Some(source) = execution.source_output {
        stdout.write_all(source.as_bytes()).map_err(output_error)?;
    } else if matches!(args.operation(), Operation::ConfigEffective) {
        if let Some(configuration) = &execution.report.configuration {
            writeln!(stdout, "Native configuration (explicit arguments and library defaults)")
                .map_err(output_error)?;
            if let Some(fields) = configuration.effective.as_object() {
                for (field, value) in fields {
                    let origin = configuration
                        .field_sources
                        .get(field)
                        .map(String::as_str)
                        .unwrap_or("unavailable");
                    writeln!(stdout, "{field} = {value}  [{origin}]").map_err(output_error)?;
                }
            }
            writeln!(stdout, "Project/profile discovery: not performed").map_err(output_error)?;
        }
    } else if matches!(args.operation(), Operation::Explain) {
        for subject in &execution.report.subjects {
            writeln!(stdout, "{}:", subject.subject).map_err(output_error)?;
            serde_json::to_writer_pretty(&mut *stdout, &subject.native_outcome).map_err(
                |error| {
                    Failure::new(
                        ExitClass::OutputError,
                        format!("cannot write explanation: {error}"),
                    )
                },
            )?;
            stdout.write_all(b"\n").map_err(output_error)?;
        }
    }
    // Check human reports go to stderr. Source and JSON stdout never
    // receive status lines, warnings, progress messages or diagnostics.
    if !args.json {
        if let Some(error) = &execution.report.error {
            writeln!(stderr, "perl-tidy: {}", error.message).map_err(output_error)?;
        }
        for subject in &execution.report.subjects {
            if matches!(args.operation(), Operation::Check)
                || subject.exit_class.blocks_formatting()
            {
                let reason = subject
                    .native_outcome
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("unavailable");
                writeln!(stderr, "{}: {} ({:?})", subject.subject, reason, subject.exit_class)
                    .map_err(output_error)?;
            }
        }
    }
    stdout.flush().map_err(output_error)?;
    stderr.flush().map_err(output_error)?;
    Ok(class)
}

fn output_error(error: std::io::Error) -> Failure {
    Failure::new(ExitClass::OutputError, format!("output was not delivered: {error}"))
}

fn execute(args: &Args, stdin: &mut dyn Read) -> Result<Execution, Failure> {
    validate_selection(args)?;
    let (config, configuration) = resolve_configuration(args)?;
    let mut report = Report {
        schema: SCHEMA,
        executable: "perl-tidy",
        package_version: env!("CARGO_PKG_VERSION"),
        engine: "native",
        operation: args.operation(),
        configuration: Some(configuration),
        subjects: Vec::new(),
        complete: true,
        exit_class: ExitClass::Success,
        error: None,
    };
    if matches!(args.operation(), Operation::ConfigEffective) {
        // This is an explicit configuration projection. A path is context only;
        // no source, project file, or environment profile is read here.
        return Ok(Execution { report, source_output: None });
    }
    let inputs = read_inputs(args, stdin)?;
    let mut formatted = Vec::with_capacity(inputs.len());
    for input in &inputs {
        let typed = NativeFormatter::new().format_document_typed(
            &input.source,
            &config,
            &FormatContext::default(),
        );
        let outcome = serde_json::to_value(&typed.outcome).map_err(instrument_error)?;
        let diagnostics = Value::Array(
            typed
                .result
                .diagnostics
                .iter()
                .map(|diagnostic| {
                    // Parser diagnostics can embed source tokens in their messages.
                    // Default machine output exposes only stable code, severity and
                    // source geometry; callers can inspect their local source.
                    let mut summary = serde_json::Map::new();
                    summary.insert("code".to_string(), Value::from(diagnostic.code.clone()));
                    summary.insert(
                        "severity".to_string(),
                        serde_json::to_value(diagnostic.severity).map_err(instrument_error)?,
                    );
                    summary.insert(
                        "range".to_string(),
                        serde_json::to_value(&diagnostic.range).map_err(instrument_error)?,
                    );
                    Ok(Value::Object(summary))
                })
                .collect::<Result<Vec<_>, Failure>>()?,
        );
        let (rendered, application_verified, class) = match typed.apply_to(&input.source) {
            Ok(rendered) if rendered == typed.result.formatted => {
                let class = native_exit_class(
                    typed.outcome.disposition,
                    typed.outcome.reason,
                    args.operation(),
                );
                (rendered, true, class)
            }
            Ok(_) | Err(_) => (input.source.clone(), false, ExitClass::InstrumentFailure),
        };
        report.exit_class = report.exit_class.join(class);
        formatted.push(Formatted {
            report: SubjectReport {
                subject: input.name.clone(),
                logical_stdin_filename: input.logical_stdin_filename.clone(),
                source_digest: ContentDigest::of_bytes(input.source.as_bytes()),
                source_bytes: input.source.len(),
                rendered_digest: ContentDigest::of_bytes(rendered.as_bytes()),
                rendered_bytes: rendered.len(),
                native_outcome: outcome,
                diagnostics,
                edit_application_verified: application_verified,
                exit_class: class,
            },
            rendered,
        });
    }
    if matches!(args.operation(), Operation::Check) {
        for input in &inputs {
            if let Some(snapshot) = &input.snapshot {
                input::verify_snapshot(snapshot, &input.source)?;
            }
        }
    }
    let source_output = if matches!(args.operation(), Operation::Stdout)
        && !report.exit_class.blocks_formatting()
    {
        formatted.first().map(|result| result.rendered.clone())
    } else {
        None
    };
    report.subjects = formatted.into_iter().map(|result| result.report).collect();
    Ok(Execution { report, source_output })
}

fn native_exit_class(
    disposition: FormatDisposition,
    reason: FormatReasonCode,
    operation: Operation,
) -> ExitClass {
    match disposition {
        FormatDisposition::Applied if matches!(operation, Operation::Check) => {
            ExitClass::WouldChange
        }
        FormatDisposition::Applied | FormatDisposition::NoChange => ExitClass::Success,
        FormatDisposition::Refused if reason == FormatReasonCode::InvalidConfiguration => {
            ExitClass::InvalidConfiguration
        }
        FormatDisposition::Refused => ExitClass::Refused,
        FormatDisposition::FailedOrNotProven if reason == FormatReasonCode::InstrumentFailure => {
            ExitClass::InstrumentFailure
        }
        FormatDisposition::FailedOrNotProven => ExitClass::FailedOrNotProven,
    }
}

fn instrument_error(error: serde_json::Error) -> Failure {
    Failure::new(
        ExitClass::InstrumentFailure,
        format!("native result serialization failed: {error}"),
    )
}

fn validate_selection(args: &Args) -> Result<(), Failure> {
    let count = if args.stdin { 1 } else { args.paths.len() };
    if count == 0 {
        return Err(Failure::new(
            ExitClass::Usage,
            "select a file or use --stdin; no workspace scan is implicit",
        ));
    }
    if count > MAX_FILES {
        return Err(Failure::new(
            ExitClass::ResourceLimited,
            format!("at most {MAX_FILES} explicit files may be selected"),
        ));
    }
    if count != 1 && !matches!(args.operation(), Operation::Check) {
        return Err(Failure::new(
            ExitClass::Usage,
            "stdout, effective-config and explanation modes accept one input; use --check for several files",
        ));
    }
    Ok(())
}

fn resolve_configuration(
    args: &Args,
) -> Result<(NativeFormatConfig, ConfigurationReport), Failure> {
    let mut value =
        serde_json::to_value(NativeFormatConfig::default()).map_err(instrument_error)?;
    let fields = value.as_object().ok_or_else(|| {
        Failure::new(ExitClass::InstrumentFailure, "native configuration is not an object")
    })?;
    let mut sources: BTreeMap<String, String> =
        fields.keys().map(|field| (field.clone(), "native_library_default".to_string())).collect();
    if let Some(path) = &args.config {
        let selected_path = path.to_str().ok_or_else(|| {
            Failure::new(
                ExitClass::InvalidInput,
                "configuration paths must be representable as UTF-8 for exact JSON identity",
            )
        })?;
        let canonical = path.canonicalize().map_err(|error| {
            Failure::new(
                ExitClass::InvalidConfiguration,
                format!("cannot resolve configuration {}: {error}", path.display()),
            )
        })?;
        if canonical.to_str().is_none() {
            return Err(Failure::new(
                ExitClass::InvalidInput,
                "resolved configuration paths must be representable as UTF-8 for exact JSON identity",
            ));
        }
        let (_, raw) = input::read_snapshot(&canonical, MAX_CONFIG_BYTES).map_err(|error| {
            Failure::new(
                ExitClass::InvalidConfiguration,
                format!("configuration {}: {}", path.display(), error.message),
            )
        })?;
        let loaded: NativeFormatConfig = serde_json::from_str(&raw).map_err(|error| {
            Failure::new(
                ExitClass::InvalidConfiguration,
                format!("invalid native configuration {}: {error}", path.display()),
            )
        })?;
        value = serde_json::to_value(loaded).map_err(instrument_error)?;
        for source in sources.values_mut() {
            *source = format!("explicit_json:{selected_path}");
        }
    }
    let object = value.as_object_mut().ok_or_else(|| {
        Failure::new(ExitClass::InstrumentFailure, "native configuration is not an object")
    })?;
    let mut overrides = Vec::<(&str, Value)>::new();
    if let Some(width) = args.line_width {
        overrides.push(("line_width", Value::from(width)));
    }
    if let Some(width) = args.indent_width {
        overrides.push(("indent_width", Value::from(width)));
    }
    if args.tabs || args.spaces {
        overrides.push(("use_tabs", Value::from(args.tabs)));
    }
    for (field, option) in [
        ("final_newline", &args.final_newline),
        ("brace_placement", &args.brace_placement),
        ("else_placement", &args.else_placement),
        ("keyword_spacing", &args.keyword_spacing),
        ("trailing_comma", &args.trailing_comma),
    ] {
        if let Some(option) = option {
            overrides.push((field, Value::from(option.clone())));
        }
    }
    for (field, replacement) in overrides {
        if !object.contains_key(field) {
            return Err(Failure::new(
                ExitClass::InstrumentFailure,
                format!("native configuration omitted {field}"),
            ));
        }
        object.insert(field.to_string(), replacement);
        sources.insert(field.to_string(), "explicit_argument".to_string());
    }
    let config: NativeFormatConfig = serde_json::from_value(value.clone()).map_err(|error| {
        Failure::new(
            ExitClass::InvalidConfiguration,
            format!("invalid native configuration: {error}"),
        )
    })?;
    config
        .validate()
        .map_err(|error| Failure::new(ExitClass::InvalidConfiguration, format!("{error:?}")))?;
    Ok((
        config,
        ConfigurationReport {
            authority: "native_library_configuration",
            library_version: perl_tidy::VERSION,
            production_profile: None,
            project_resolution: "not_performed",
            effective: value,
            field_sources: sources,
        },
    ))
}

fn read_inputs(args: &Args, stdin: &mut dyn Read) -> Result<Vec<Input>, Failure> {
    if args.stdin {
        let logical_stdin_filename = args
            .stdin_filename
            .as_ref()
            .map(|path| {
                path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
                    Failure::new(
                        ExitClass::InvalidInput,
                        "--stdin-filename must be representable as UTF-8",
                    )
                })
            })
            .transpose()?;
        return Ok(vec![Input {
            name: "<stdin>".to_string(),
            logical_stdin_filename,
            source: read_bounded(stdin, args.max_bytes)?,
            snapshot: None,
        }]);
    }
    let mut paths = BTreeSet::<PathBuf>::new();
    for path in &args.paths {
        let canonical = path.canonicalize().map_err(|error| {
            Failure::new(
                ExitClass::FilesystemError,
                format!("cannot resolve {}: {error}", path.display()),
            )
        })?;
        if canonical.to_str().is_none() {
            return Err(Failure::new(
                ExitClass::InvalidInput,
                "selected file paths must be representable as UTF-8 for exact JSON identity",
            ));
        }
        paths.insert(canonical);
    }
    let mut total = 0_usize;
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        let (snapshot, source) = input::read_snapshot(&path, args.max_bytes)?;
        total = total.saturating_add(source.len());
        if total > MAX_TOTAL_BYTES {
            return Err(Failure::new(
                ExitClass::ResourceLimited,
                "selected source bytes exceed the 64 MiB command limit; no files were changed",
            ));
        }
        inputs.push(Input {
            name: path.to_string_lossy().into_owned(),
            logical_stdin_filename: None,
            source,
            snapshot: Some(snapshot),
        });
    }
    Ok(inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::io;

    struct BrokenOutput;
    impl Write for BrokenOutput {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "consumer closed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_pipe_never_reports_success() -> Result<(), Box<dyn std::error::Error>> {
        let args = Args::try_parse_from(["perl-tidy", "--stdin"])?;
        let mut source: &[u8] = b"my $x=1;\n";
        let result = run(&args, &mut source, &mut BrokenOutput, &mut Vec::new());
        assert!(matches!(result, Err(Failure { class: ExitClass::OutputError, .. })));
        Ok(())
    }

    #[test]
    fn default_config_is_exactly_the_library_default() -> Result<(), Box<dyn std::error::Error>> {
        let args = Args::try_parse_from(["perl-tidy", "--stdin", "--config-effective", "--json"])?;
        let (configuration, report) =
            resolve_configuration(&args).map_err(|error| error.message)?;
        assert_eq!(
            serde_json::to_value(configuration)?,
            serde_json::to_value(NativeFormatConfig::default())?
        );
        assert!(report.production_profile.is_none());
        assert_eq!(report.library_version, perl_tidy::VERSION);
        assert_eq!(report.project_resolution, "not_performed");
        assert!(report.field_sources.values().all(|origin| origin == "native_library_default"));
        Ok(())
    }
}
