use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

const PERL_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_DIAGNOSTIC_CHARS: usize = 200;
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

fn environment_value(environment: &[(OsString, OsString)], name: &str) -> Option<OsString> {
    environment
        .iter()
        .find(|(key, _)| key.to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn isolated_perl_command_from(environment: &[(OsString, OsString)]) -> TestResult<Command> {
    let path = environment_value(environment, "PATH")
        .ok_or_else(|| failure("PATH is required to resolve the governed Perl fixture probe"))?;

    let mut command = Command::new("perl");
    command.env_clear().env("PATH", path).env("LC_ALL", "C");

    for denied in [
        "PERL5LIB",
        "PERL5OPT",
        "PERL_LOCAL_LIB_ROOT",
        "PERL_LOCAL_LIB_PREFIX",
        "PERL_MB_OPT",
        "PERL_MM_OPT",
    ] {
        command.env_remove(denied);
    }

    #[cfg(windows)]
    for allowed in ["SYSTEMROOT", "WINDIR", "PATHEXT", "TEMP", "TMP"] {
        if let Some(value) = environment_value(environment, allowed) {
            command.env(allowed, value);
        }
    }

    Ok(command)
}

fn isolated_perl_command() -> TestResult<Command> {
    let environment = env::vars_os().collect::<Vec<_>>();
    isolated_perl_command_from(&environment)
}

fn run_bounded(mut command: Command, operation: &str) -> TestResult<Output> {
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
fn import_export_fixture_is_a_declared_two_file_module_graph() -> TestResult {
    let root = fixture_root();
    let producer_path = root.join("Accuracy/ImportsExports.pm");
    let consumer_path = root.join("imports_exports.pl");
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
    fs::create_dir_all(root.join("Accuracy"))?;
    let fixture_root = fixture_root();
    fs::copy(fixture_root.join("imports_exports.pl"), root.join("imports_exports.pl"))?;
    fs::copy(
        fixture_root.join("Accuracy/ImportsExports.pm"),
        root.join("Accuracy/ImportsExports.pm"),
    )?;
    Ok(())
}

fn compile_import_export_copy(root: &Path) -> TestResult<Output> {
    let consumer = root.join("imports_exports.pl");
    let mut command = isolated_perl_command()?;
    command.current_dir(root).arg("-I").arg(root).arg("-c").arg(consumer);
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
    fs::copy(fixture_root().join("imports_exports.pl"), root.path().join("imports_exports.pl"))?;
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
    fs::create_dir_all(root.path())?;
    let fixture_root = fixture_root();
    let consumer = root.path().join("imports_exports.pl");
    let mut collapsed = fs::read_to_string(fixture_root.join("imports_exports.pl"))?;
    collapsed.push_str(&fs::read_to_string(fixture_root.join("Accuracy/ImportsExports.pm"))?);
    fs::write(&consumer, collapsed)?;
    assert_import_export_copy_rejected(root.path(), "Can't locate Accuracy/ImportsExports.pm")
}

#[test]
fn import_export_fixture_compiles_with_only_the_declared_module_root() -> TestResult {
    let root = fixture_root();
    let consumer = root.join("imports_exports.pl");
    let mut command = isolated_perl_command()?;
    command.current_dir(&root).arg("-I").arg(&root).arg("-c").arg(&consumer);

    let output = run_bounded(command, "ImportExport fixture syntax check")?;
    require_success(&output, "ImportExport fixture syntax check")?;

    let stderr = String::from_utf8(output.stderr)?;
    assert_contains(&stderr, "syntax OK", "Perl syntax-check stderr")
}

#[test]
fn import_export_fixture_loads_the_expected_imported_symbol() -> TestResult {
    let root = fixture_root();
    let consumer = root.join("imports_exports.pl");
    let mut command = isolated_perl_command()?;
    command.current_dir(&root).arg("-I").arg(&root).arg("-e").arg(IMPORT_PROBE).arg(&consumer);

    let output = run_bounded(command, "ImportExport fixture load probe")?;
    require_success(&output, "ImportExport fixture load probe")?;

    let stdout = String::from_utf8(output.stdout)?;
    if stdout == "fixture-ok\n" {
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
        (OsString::from("PERL5OPT"), OsString::from("-MHostile::Prelude")),
        (OsString::from("PERL_LOCAL_LIB_ROOT"), OsString::from("hostile-local-lib")),
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

    let mut command = isolated_perl_command_from(&environment)?;
    command.arg("-e").arg(
        r#"my @bad = grep { exists $ENV{$_} } qw(PERL5LIB PERL5OPT PERL_LOCAL_LIB_ROOT PERL_LOCAL_LIB_PREFIX PERL_MB_OPT PERL_MM_OPT); die join(',', @bad) if @bad; print "isolated\n";"#,
    );

    let output = run_bounded(command, "hostile Perl environment denial probe")?;
    require_success(&output, "hostile Perl environment denial probe")?;

    let stdout = String::from_utf8(output.stdout)?;
    if stdout == "isolated\n" {
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
