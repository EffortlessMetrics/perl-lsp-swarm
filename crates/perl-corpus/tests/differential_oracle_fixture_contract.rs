use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const PERL_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_DIAGNOSTIC_CHARS: usize = 200;
const DENIED_PERL_ENVIRONMENT: &[&str] = &[
    "PERL5LIB",
    "PERLLIB",
    "PERL5OPT",
    "PERL_LOCAL_LIB_ROOT",
    "PERL_LOCAL_LIB_PREFIX",
    "PERL_MB_OPT",
    "PERL_MM_OPT",
];
const IMPORT_PROBE: &str = r#"
my $file = shift @ARGV;
my $loaded = do $file;
die($@ || $! || "fixture returned false\n") unless $loaded;

no strict 'refs';
my $imported = 'Accuracy::ImportsConsumer::answer';
my $caller = 'Accuracy::ImportsConsumer::call_imported';
die "imported symbol missing\n" unless defined &{$imported};
die "consumer subroutine missing\n" unless defined &{$caller};
die "unexpected imported value\n" unless &{$caller}() == 42;
print "fixture-ok\n";
"#;

fn failure(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::other(message.into()).into()
}

fn bounded_diagnostic(bytes: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let Some(line) = decoded.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return String::new();
    };

    let mut chars = line.chars();
    let bounded = chars.by_ref().take(MAX_DIAGNOSTIC_CHARS).collect::<String>();
    if chars.next().is_some() { format!("{bounded}…") } else { bounded }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/parser_accuracy")
}

#[derive(Debug)]
struct ImportExportFixturePaths {
    consumer: PathBuf,
    producer: PathBuf,
    module_root: PathBuf,
    consumer_relative: PathBuf,
    producer_relative: PathBuf,
}

fn normalized_protocol_line(bytes: &[u8], subject: &str) -> TestResult<String> {
    let output = String::from_utf8(bytes.to_vec())?;
    let line = output.strip_suffix("\r\n").or_else(|| output.strip_suffix('\n'));
    let line = line.unwrap_or(&output);
    if line.contains(['\r', '\n']) {
        return Err(failure(format!("{subject} emitted more than one protocol line")));
    }
    Ok(line.to_owned())
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyManifest {
    fixtures: Vec<ParserAccuracyFixture>,
}

#[derive(Debug, Deserialize)]
struct ParserAccuracyFixture {
    id: String,
    source_path: PathBuf,
}

fn validated_manifest_source_path(
    repository_root: &Path,
    fixture_root: &Path,
    source_path: &Path,
) -> TestResult<PathBuf> {
    if source_path.is_absolute()
        || source_path.components().any(|component| {
            matches!(component, Component::Prefix(_) | Component::RootDir | Component::ParentDir)
        })
    {
        return Err(failure(format!(
            "manifest fixture source path is not a safe relative path: {}",
            source_path.display()
        )));
    }

    let canonical_repository_root = fs::canonicalize(repository_root)?;
    let canonical_fixture_root = fs::canonicalize(fixture_root)?;
    let candidate = repository_root.join(source_path);
    let canonical_candidate = fs::canonicalize(&candidate)?;
    if !canonical_candidate.starts_with(&canonical_repository_root)
        || !canonical_candidate.starts_with(&canonical_fixture_root)
    {
        return Err(failure(format!(
            "manifest fixture source path escapes the parser-accuracy fixture root: {}",
            source_path.display()
        )));
    }
    Ok(canonical_candidate)
}

#[cfg(unix)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> TestResult<bool> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(true)
}

#[cfg(windows)]
fn create_file_symlink_for_test(target: &Path, link: &Path) -> TestResult<bool> {
    use perl_tdd_support::try_create_file_symlink;

    if perl_tdd_support::symlink_test_decision().skip_visibly() {
        return Ok(false);
    }
    Ok(try_create_file_symlink(target, link)?.is_some())
}

fn import_export_fixture_paths() -> TestResult<ImportExportFixturePaths> {
    let manifest_path = fixture_root().join("manifest.json");
    let manifest: ParserAccuracyManifest =
        serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("parser accuracy manifest has no repository root"))?;
    let consumer = manifest
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "imports_exports")
        .ok_or_else(|| failure("manifest is missing the imports_exports fixture"))?;
    let producer = manifest
        .fixtures
        .iter()
        .find(|fixture| fixture.id == "imports_exports_producer")
        .ok_or_else(|| failure("manifest is missing the imports_exports_producer fixture"))?;
    let canonical_fixture_root = fs::canonicalize(fixture_root())?;
    let consumer = validated_manifest_source_path(
        repository_root,
        &canonical_fixture_root,
        &consumer.source_path,
    )?;
    let producer = validated_manifest_source_path(
        repository_root,
        &canonical_fixture_root,
        &producer.source_path,
    )?;
    let consumer_relative = consumer.strip_prefix(&canonical_fixture_root)?.to_path_buf();
    let producer_relative = producer.strip_prefix(&canonical_fixture_root)?.to_path_buf();
    let module_root = producer
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| failure("manifest producer path has no module root"))?
        .to_path_buf();
    Ok(ImportExportFixturePaths {
        consumer,
        producer,
        module_root,
        consumer_relative,
        producer_relative,
    })
}

fn environment_value(environment: &[(OsString, OsString)], name: &str) -> Option<OsString> {
    environment
        .iter()
        .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn sanitize_perl_command(
    mut command: Command,
    environment: &[(OsString, OsString)],
) -> TestResult<Command> {
    let path = environment_value(environment, "PATH")
        .ok_or_else(|| failure("PATH is required to resolve the governed Perl fixture probe"))?;

    command.env_clear().env("PATH", path).env("LC_ALL", "C");

    remove_denied_perl_environment(&mut command);

    #[cfg(windows)]
    for allowed in ["SYSTEMROOT", "WINDIR", "PATHEXT", "TEMP", "TMP"] {
        if let Some(value) = environment_value(environment, allowed) {
            command.env(allowed, value);
        }
    }

    Ok(command)
}

fn remove_denied_perl_environment(command: &mut Command) {
    for denied in DENIED_PERL_ENVIRONMENT {
        command.env_remove(denied);
    }
}

fn isolated_perl_command_from(environment: &[(OsString, OsString)]) -> TestResult<Command> {
    sanitize_perl_command(Command::new("perl"), environment)
}

fn isolated_perl_command() -> TestResult<Command> {
    let environment = env::vars_os().collect::<Vec<_>>();
    isolated_perl_command_from(&environment)
}

fn run_bounded(mut command: Command, operation: &str) -> TestResult<Output> {
    // This contract bounds only the directly spawned child; descendant cleanup is not proven.
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        failure(format!(
            "could not spawn {operation}; this test requires a real Perl executable: {error}"
        ))
    })?;
    let started = Instant::now();

    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output().map_err(Into::into);
        }

        let elapsed = started.elapsed();
        if elapsed >= PERL_TIMEOUT {
            if let Err(error) = child.kill()
                && child.try_wait()?.is_none()
            {
                return Err(failure(format!("could not kill timed-out {operation}: {error}")));
            }
            let output = child.wait_with_output()?;
            return Err(failure(format!(
                "{operation} timed out after {} ms; stderr: {}",
                PERL_TIMEOUT.as_millis(),
                bounded_diagnostic(&output.stderr)
            )));
        }

        thread::sleep(POLL_INTERVAL.min(PERL_TIMEOUT.saturating_sub(elapsed)));
    }
}

fn require_success(output: &Output, operation: &str) -> TestResult {
    if output.status.success() {
        return Ok(());
    }

    Err(failure(format!(
        "{operation} failed with {}; stdout: {}; stderr: {}",
        output.status,
        bounded_diagnostic(&output.stdout),
        bounded_diagnostic(&output.stderr)
    )))
}

fn assert_contains(haystack: &str, needle: &str, subject: &str) -> TestResult {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(failure(format!("{subject} is missing {needle:?}")))
    }
}

fn assert_not_contains(haystack: &str, needle: &str, subject: &str) -> TestResult {
    if haystack.contains(needle) {
        Err(failure(format!("{subject} unexpectedly contains {needle:?}")))
    } else {
        Ok(())
    }
}

#[test]
fn bounded_diagnostic_selects_one_line_and_preserves_character_boundaries() {
    let long_unicode_line = "é".repeat(MAX_DIAGNOSTIC_CHARS + 1);
    let output = format!("\n  \n{long_unicode_line}\nignored");
    let diagnostic = bounded_diagnostic(output.as_bytes());

    assert_eq!(diagnostic.chars().count(), MAX_DIAGNOSTIC_CHARS + 1);
    assert!(diagnostic.ends_with('…'));
    assert!(!diagnostic.contains("ignored"));
}

#[test]
fn protocol_line_normalization_accepts_only_one_lf_or_crlf_terminated_line() -> TestResult {
    assert_eq!(normalized_protocol_line(b"fixture-ok\n", "fixture probe")?, "fixture-ok");
    assert_eq!(normalized_protocol_line(b"fixture-ok\r\n", "fixture probe")?, "fixture-ok");
    assert!(normalized_protocol_line(b"fixture-ok\nextra", "fixture probe").is_err());
    Ok(())
}

#[test]
fn manifest_source_path_rejects_absolute_and_parent_traversal() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture_root = root.path().join("fixtures");
    fs::create_dir_all(&fixture_root)?;
    fs::write(fixture_root.join("inside.pl"), "1;")?;

    assert!(
        validated_manifest_source_path(root.path(), &fixture_root, Path::new("../outside.pl"))
            .is_err()
    );
    assert!(
        validated_manifest_source_path(root.path(), &fixture_root, &fixture_root.join("inside.pl"))
            .is_err()
    );
    assert!(
        validated_manifest_source_path(root.path(), &fixture_root, Path::new("fixtures/inside.pl"))
            .is_ok()
    );
    Ok(())
}

#[test]
fn manifest_source_path_rejects_symlink_escape() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture_root = root.path().join("fixtures");
    let outside = tempfile::tempdir()?;
    fs::create_dir_all(&fixture_root)?;
    let target = outside.path().join("outside.pl");
    fs::write(&target, "1;\n")?;
    let link = fixture_root.join("escaped.pl");
    if !create_file_symlink_for_test(&target, &link)? {
        return Ok(());
    }

    let error = match validated_manifest_source_path(
        root.path(),
        &fixture_root,
        Path::new("fixtures/escaped.pl"),
    ) {
        Ok(path) => {
            return Err(failure(format!("symlink escape was accepted: {}", path.display())));
        }
        Err(error) => error,
    };
    assert_contains(
        &error.to_string(),
        "escapes the parser-accuracy fixture root",
        "symlink escape containment",
    )
}

#[test]
fn import_export_fixture_is_a_declared_two_file_module_graph() -> TestResult {
    let paths = import_export_fixture_paths()?;
    let consumer_path = &paths.consumer;
    let producer_path = &paths.producer;
    let producer = fs::read_to_string(&producer_path)?;
    let consumer = fs::read_to_string(&consumer_path)?;

    assert_contains(&producer, "package Accuracy::ImportsExports;", "producer module")?;
    assert_contains(&producer, "use Exporter qw(import);", "producer module")?;
    assert_contains(&producer, "our @EXPORT_OK = qw(answer);", "producer module")?;
    assert_contains(&producer, "sub answer", "producer module")?;

    assert_contains(&consumer, "package Accuracy::ImportsConsumer;", "consumer fixture")?;
    assert_contains(&consumer, "use Accuracy::ImportsExports qw(answer);", "consumer fixture")?;
    assert_contains(&consumer, "return answer();", "consumer fixture")?;
    assert_not_contains(&consumer, "package Accuracy::ImportsExports;", "consumer fixture")?;

    Ok(())
}

fn copy_import_export_fixture(root: &Path) -> TestResult {
    let paths = import_export_fixture_paths()?;
    let consumer = root.join(&paths.consumer_relative);
    let producer = root.join(&paths.producer_relative);
    if let Some(parent) = consumer.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = producer.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&paths.consumer, consumer)?;
    fs::copy(&paths.producer, producer)?;
    Ok(())
}

fn compile_import_export_copy(root: &Path) -> TestResult<Output> {
    let paths = import_export_fixture_paths()?;
    let consumer = root.join(&paths.consumer_relative);
    let module_root = root.join(
        paths
            .producer_relative
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| failure("manifest producer path has no module root"))?,
    );
    let mut command = isolated_perl_command()?;
    command.current_dir(root).arg("-I").arg(module_root).arg("-c").arg(consumer);
    run_bounded(command, "temporary ImportExport fixture syntax check")
}

fn assert_import_export_copy_rejected(root: &Path, expected: &str) -> TestResult {
    let output = compile_import_export_copy(root)?;
    if output.status.success() {
        return Err(failure(format!(
            "temporary ImportExport fixture unexpectedly compiled for negative control {expected:?}"
        )));
    }
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_contains(&diagnostics, expected, "temporary ImportExport negative control")
}

#[test]
fn import_export_negative_control_rejects_missing_producer() -> TestResult {
    let root = tempfile::tempdir()?;
    let paths = import_export_fixture_paths()?;
    let consumer = root.path().join(&paths.consumer_relative);
    if let Some(parent) = consumer.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(paths.consumer, consumer)?;
    assert_import_export_copy_rejected(root.path(), "Can't locate Accuracy/ImportsExports.pm")
}

#[test]
fn import_export_negative_control_rejects_wrong_module_root() -> TestResult {
    let root = tempfile::tempdir()?;
    copy_import_export_fixture(root.path())?;
    fs::create_dir_all(root.path().join("WrongRoot"))?;
    fs::rename(
        root.path().join("Accuracy/ImportsExports.pm"),
        root.path().join("WrongRoot/ImportsExports.pm"),
    )?;
    fs::remove_dir(root.path().join("Accuracy"))?;
    assert_import_export_copy_rejected(root.path(), "Can't locate Accuracy/ImportsExports.pm")
}

#[test]
fn import_export_negative_control_rejects_missing_export() -> TestResult {
    let root = tempfile::tempdir()?;
    copy_import_export_fixture(root.path())?;
    let producer = root.path().join("Accuracy/ImportsExports.pm");
    let source = fs::read_to_string(&producer)?
        .replace("our @EXPORT_OK = qw(answer);", "our @EXPORT_OK = qw();");
    fs::write(producer, source)?;
    assert_import_export_copy_rejected(
        root.path(),
        "not exported by the Accuracy::ImportsExports module",
    )
}

#[test]
fn import_export_negative_control_rejects_collapsed_topology() -> TestResult {
    let root = tempfile::tempdir()?;
    let paths = import_export_fixture_paths()?;
    let consumer = root.path().join(&paths.consumer_relative);
    if let Some(parent) = consumer.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut collapsed = fs::read_to_string(paths.consumer)?;
    collapsed.push_str(&fs::read_to_string(paths.producer)?);
    fs::write(&consumer, collapsed)?;
    assert_import_export_copy_rejected(root.path(), "Can't locate Accuracy/ImportsExports.pm")
}

#[test]
fn import_export_fixture_compiles_with_only_the_declared_module_root() -> TestResult {
    let paths = import_export_fixture_paths()?;
    let consumer = &paths.consumer;
    let root = &paths.module_root;
    let mut command = isolated_perl_command()?;
    command.current_dir(&root).arg("-I").arg(&root).arg("-c").arg(&consumer);

    let output = run_bounded(command, "ImportExport fixture syntax check")?;
    require_success(&output, "ImportExport fixture syntax check")?;

    let stderr = String::from_utf8(output.stderr)?;
    assert_contains(&stderr, "syntax OK", "Perl syntax-check stderr")
}

#[test]
fn import_export_fixture_loads_the_expected_imported_symbol() -> TestResult {
    let paths = import_export_fixture_paths()?;
    let consumer = &paths.consumer;
    let root = &paths.module_root;
    let mut command = isolated_perl_command()?;
    command.current_dir(&root).arg("-I").arg(&root).arg("-e").arg(IMPORT_PROBE).arg(&consumer);

    let output = run_bounded(command, "ImportExport fixture load probe")?;
    require_success(&output, "ImportExport fixture load probe")?;

    let stdout = normalized_protocol_line(&output.stdout, "ImportExport fixture load probe")?;
    if stdout == "fixture-ok" {
        Ok(())
    } else {
        Err(failure(format!("unexpected ImportExport fixture load receipt: {stdout:?}")))
    }
}

#[test]
fn governed_perl_probe_denies_hostile_perl_environment() -> TestResult {
    let path = env::var_os("PATH").ok_or_else(|| failure("PATH is required for Perl probe"))?;
    let environment = vec![
        (OsString::from("PATH"), path),
        (OsString::from("PERL5LIB"), OsString::from("hostile-module-root")),
        (OsString::from("PERLLIB"), OsString::from("hostile-fallback-module-root")),
        (OsString::from("PERL5OPT"), OsString::from("-MHostile::Prelude")),
        (OsString::from("PERL_LOCAL_LIB_ROOT"), OsString::from("hostile-local-lib")),
        (OsString::from("PERL_LOCAL_LIB_PREFIX"), OsString::from("hostile-local-prefix")),
        (OsString::from("PERL_MB_OPT"), OsString::from("hostile-mb-opt")),
        (OsString::from("PERL_MM_OPT"), OsString::from("hostile-mm-opt")),
    ];

    #[cfg(windows)]
    let environment = {
        let mut environment = environment;
        for allowed in ["SYSTEMROOT", "WINDIR", "PATHEXT", "TEMP", "TMP"] {
            if let Some(value) = env::var_os(allowed) {
                environment.push((OsString::from(allowed), value));
            }
        }
        environment
    };

    let mut command = Command::new("perl");
    for (key, value) in &environment {
        command.env(key, value);
    }
    command.env("LC_ALL", "C");
    remove_denied_perl_environment(&mut command);
    command.arg("-e").arg(
        r#"my @bad = grep { exists $ENV{$_} } qw(PERL5LIB PERLLIB PERL5OPT PERL_LOCAL_LIB_ROOT PERL_LOCAL_LIB_PREFIX PERL_MB_OPT PERL_MM_OPT); die join(',', @bad) if @bad; print "isolated\n";"#,
    );

    let output = run_bounded(command, "hostile Perl environment denial probe")?;
    require_success(&output, "hostile Perl environment denial probe")?;

    let stdout = normalized_protocol_line(&output.stdout, "hostile Perl environment denial probe")?;
    if stdout == "isolated" {
        Ok(())
    } else {
        Err(failure(format!("unexpected environment denial receipt: {stdout:?}")))
    }
}

#[test]
fn governed_perl_probe_environment_contains_no_denied_value() -> TestResult {
    let command = isolated_perl_command()?;
    let denied = [
        OsStr::new("PERL5LIB"),
        OsStr::new("PERLLIB"),
        OsStr::new("PERL5OPT"),
        OsStr::new("PERL_LOCAL_LIB_ROOT"),
        OsStr::new("PERL_LOCAL_LIB_PREFIX"),
        OsStr::new("PERL_MB_OPT"),
        OsStr::new("PERL_MM_OPT"),
    ];

    for (key, value) in command.get_envs() {
        if denied.iter().any(|candidate| key.eq_ignore_ascii_case(candidate)) && value.is_some() {
            return Err(failure(format!(
                "governed Perl probe retained a denied environment value for {:?}",
                key
            )));
        }
    }

    Ok(())
}
