use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_perl-tidy"))
}

fn stdin_command(args: &[&str], input: &[u8]) -> std::io::Result<Output> {
    command_with_stdin(binary().args(args), input)
}

fn command_with_stdin(command: &mut Command, input: &[u8]) -> std::io::Result<Output> {
    let mut child =
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let write_result = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(input),
        None => Ok(()),
    };
    // Stdin is closed before waiting, including when the child rejects input early.
    let output = child.wait_with_output();
    match write_result {
        Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(error),
        _ => output,
    }
}

#[test]
fn binary_name_and_source_channels_are_standalone() -> TestResult {
    let version = binary().arg("--version").output()?;
    assert!(version.status.success());
    assert!(String::from_utf8(version.stdout)?.starts_with("perl-tidy "));
    let output = stdin_command(&["--stdin"], b"my $x=1;\n")?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"my $x = 1;\n");
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn stdin_filename_does_not_read_or_create_a_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    let absent = directory.path().join("logical.pl");
    let output = stdin_command(
        &["--stdin", "--stdin-filename", absent.to_str().ok_or("test path")?, "--json"],
        b"my $x=1;\n",
    )?;
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["subjects"][0]["logical_stdin_filename"], absent.to_string_lossy().as_ref());
    assert_eq!(value["configuration"]["project_resolution"], "not_performed");
    assert!(!absent.exists());
    Ok(())
}

#[test]
fn check_distinguishes_clean_change_and_refusal() -> TestResult {
    for (source, expected_code, disposition, class) in [
        ("my $x = 1;\n", 0, "no_change", "success"),
        ("my $x=1;\n", 1, "applied", "would_change"),
        ("my $x = (\n", 2, "refused", "refused"),
    ] {
        let output = stdin_command(&["--stdin", "--check", "--json"], source.as_bytes())?;
        assert_eq!(
            output.status.code(),
            Some(expected_code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty());
        let value: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(value["schema"], "perl_tidy_cli.v1");
        assert_eq!(value["exit_class"], class);
        assert_eq!(value["subjects"][0]["native_outcome"]["disposition"], disposition);
        assert_eq!(value["subjects"][0]["edit_application_verified"], true);
        assert!(value["subjects"][0].get("formatted").is_none());
        assert!(value.get("transaction").is_none());
        assert!(value["subjects"][0].get("filesystem_effect").is_none());
    }
    Ok(())
}

#[test]
fn refused_stdout_is_empty_instead_of_success_shaped_source() -> TestResult {
    let output = stdin_command(&["--stdin"], b"my $x = (\n")?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn json_diagnostic_projection_does_not_disclose_source_tokens() -> TestResult {
    let output = stdin_command(&["--stdin", "--json"], b"my $private_customer_token = (\n")?;
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    for diagnostic in report["subjects"][0]["diagnostics"].as_array().ok_or("diagnostics")? {
        assert!(diagnostic.get("code").is_some());
        assert!(diagnostic.get("message").is_none());
    }
    assert!(!String::from_utf8(output.stdout)?.contains("private_customer_token"));
    Ok(())
}

#[test]
fn invalid_configuration_and_input_limits_are_not_clean() -> TestResult {
    // Exercise early stdin rejection while the parent still has data to write.
    let large_input = vec![b' '; 2 * 1024 * 1024];
    for (args, input, code, class) in [
        (
            vec!["--stdin", "--check", "--json", "--line-width", "0"],
            b"my $x = 1;\n".as_slice(),
            3,
            "invalid_configuration",
        ),
        (
            vec!["--stdin", "--check", "--json", "--max-bytes", "4"],
            b"my $x = 1;\n".as_slice(),
            4,
            "resource_limited",
        ),
        (vec!["--stdin", "--check", "--json"], [255_u8].as_slice(), 9, "invalid_input"),
        (
            vec!["--stdin", "--check", "--json", "--line-width", "0"],
            large_input.as_slice(),
            3,
            "invalid_configuration",
        ),
        (
            vec!["--stdin", "--check", "--json", "--max-bytes", "4"],
            large_input.as_slice(),
            4,
            "resource_limited",
        ),
    ] {
        let output = stdin_command(&args, input)?;
        assert_eq!(output.status.code(), Some(code));
        let value: Value = serde_json::from_slice(&output.stdout)?;
        assert_eq!(value["exit_class"], class);
        assert_eq!(value["complete"], false);
    }
    Ok(())
}

#[test]
fn effective_config_is_reusable_and_rejects_external_fields() -> TestResult {
    let output =
        stdin_command(&["--stdin", "--config-effective", "--json", "--indent-width", "2"], b"")?;
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["configuration"]["effective"]["indent_width"], 2);
    assert_eq!(report["configuration"]["field_sources"]["indent_width"], "explicit_argument");
    assert!(report["configuration"]["production_profile"].is_null());
    let directory = tempfile::tempdir()?;
    let config_path = directory.path().join("native.json");
    let mut config = report["configuration"]["effective"].clone();
    fs::write(&config_path, serde_json::to_vec(&config)?)?;
    let valid = stdin_command(
        &["--stdin", "--check", "--json", "--config", config_path.to_str().ok_or("test path")?],
        b"my $x = 1;\n",
    )?;
    assert!(valid.status.success(), "{}", String::from_utf8_lossy(&valid.stdout));
    // A top-level assignment cannot distinguish selected indentation from defaults.
    // Require actual bytes so reporting the configuration without applying it fails.
    let nested = b"sub f {\nmy$x=1;\n}\n";
    let rendered =
        stdin_command(&["--stdin", "--config", config_path.to_str().ok_or("test path")?], nested)?;
    assert!(rendered.status.success(), "{}", String::from_utf8_lossy(&rendered.stderr));
    assert_eq!(rendered.stdout, b"sub f {\n  my $x = 1;\n}\n");
    assert!(rendered.stderr.is_empty());
    let overridden = stdin_command(
        &["--stdin", "--config", config_path.to_str().ok_or("test path")?, "--indent-width", "3"],
        nested,
    )?;
    assert!(overridden.status.success(), "{}", String::from_utf8_lossy(&overridden.stderr));
    assert_eq!(overridden.stdout, b"sub f {\n   my $x = 1;\n}\n");
    assert!(overridden.stderr.is_empty());
    config["mode"] = Value::from("external-legacy");
    fs::write(&config_path, serde_json::to_vec(&config)?)?;
    let invalid = stdin_command(
        &["--stdin", "--json", "--config", config_path.to_str().ok_or("test path")?],
        b"my $x = 1;\n",
    )?;
    assert_eq!(invalid.status.code(), Some(3));
    Ok(())
}

#[cfg(unix)]
#[test]
fn configuration_paths_reject_lossy_report_identity() -> TestResult {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir()?;
    let configuration = serde_json::to_vec(&perl_tidy::NativeFormatConfig::default())?;
    let normal = directory.path().join("native.json");
    fs::write(&normal, &configuration)?;
    let run = |path: &std::path::Path| {
        binary()
            .args(["--stdin", "--config-effective", "--json", "--config"])
            .arg(path)
            .stdin(Stdio::null())
            .output()
    };
    let positive = run(&normal)?;
    assert!(positive.status.success());
    let report: Value = serde_json::from_slice(&positive.stdout)?;
    assert_eq!(
        report["configuration"]["field_sources"]["indent_width"],
        format!("explicit_json:{}", normal.to_str().ok_or("test path")?)
    );

    for byte in [0x80, 0x81] {
        // These distinct names previously collapsed to the same replacement character.
        let invalid = directory.path().join(OsString::from_vec(vec![b'c', byte]));
        fs::write(&invalid, &configuration)?;
        let alias = directory.path().join(format!("config-alias-{byte}.json"));
        symlink(&invalid, &alias)?;
        for path in [&invalid, &alias] {
            let output = run(path)?;
            assert_eq!(output.status.code(), Some(9));
            assert!(output.stderr.is_empty());
            let report: Value = serde_json::from_slice(&output.stdout)?;
            assert_eq!(report["exit_class"], "invalid_input");
            assert_eq!(report["complete"], false);
            assert!(report["configuration"].is_null());
            assert_eq!(report["subjects"], serde_json::json!([]));
        }
        assert_eq!(fs::read(&invalid)?, configuration);
    }
    Ok(())
}

#[test]
fn file_check_order_and_duplicate_normalization_are_deterministic() -> TestResult {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a.pl");
    let b = directory.path().join("b.pl");
    fs::write(&a, "my $x = 1;\n")?;
    fs::write(&b, "my $y=2;\n")?;
    let run = || {
        binary()
            .args([
                OsStr::new("--check"),
                OsStr::new("--json"),
                b.as_os_str(),
                a.as_os_str(),
                a.as_os_str(),
            ])
            .output()
    };
    let first = run()?;
    let second = run()?;
    assert_eq!(first.status.code(), Some(1));
    assert_eq!(first.stdout, second.stdout);
    let report: Value = serde_json::from_slice(&first.stdout)?;
    assert_eq!(report["subjects"].as_array().ok_or("subjects")?.len(), 2);
    assert_eq!(report["subjects"][0]["subject"], a.canonicalize()?.to_string_lossy().as_ref());
    assert_eq!(fs::read_to_string(&b)?, "my $y=2;\n");
    Ok(())
}

#[cfg(unix)]
#[test]
fn working_external_tools_on_path_are_never_invoked() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let marker = directory.path().join("called");
    let witness = directory.path().join("positive-controls");
    let mut witnessed = Vec::new();
    for name in ["perltidy", "perl"] {
        let tool = directory.path().join(name);
        fs::write(
            &tool,
            "#!/bin/sh\nprintf '%s\\n' \"$0\" >> \"$PERL_TIDY_CLI_PROBE_LOG\"\nexit 99\n",
        )?;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))?;
        let status = Command::new(name)
            .env("PATH", directory.path())
            .env("PERL_TIDY_CLI_PROBE_LOG", &witness)
            .status()?;
        assert_eq!(status.code(), Some(99), "{name} probe control did not execute");
        witnessed.push(tool.to_string_lossy().into_owned());
    }
    assert_eq!(
        fs::read_to_string(&witness)?.lines().map(ToOwned::to_owned).collect::<Vec<_>>(),
        witnessed
    );
    let output = command_with_stdin(
        binary()
            .args(["--stdin", "--check", "--json"])
            .env("PATH", directory.path())
            .env("PERLTIDY", directory.path().join("perltidy"))
            .env("PERL_TIDY_CLI_PROBE_LOG", &marker),
        b"my $x = 1;\n",
    )?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(!marker.exists());
    Ok(())
}

#[test]
fn write_is_rejected_before_input_handling_and_leaves_files_unchanged() -> TestResult {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a.pl");
    let b = directory.path().join("b.pl");
    let absent = directory.path().join("absent.pl");
    fs::write(&a, "my $x=1;\n")?;
    fs::write(&b, "my $x = (\n")?;
    let output = binary().arg("--write").arg("--json").arg(&a).arg(&b).output()?;
    assert_eq!(output.status.code(), Some(10));
    assert!(output.stdout.is_empty(), "argument errors are outside the JSON envelope");
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--write'"));
    assert_eq!(fs::read_to_string(&a)?, "my $x=1;\n");
    assert_eq!(fs::read_to_string(&b)?, "my $x = (\n");

    // Parser rejection precedes path resolution and stdin admission, not just
    // native format refusal or a platform-specific replacement restriction.
    let missing = binary().arg("--write").arg(&absent).output()?;
    assert_eq!(missing.status.code(), Some(10));
    assert!(!absent.exists());
    let stdin = binary()
        .args(["--write", "--stdin", "--stdin-filename"])
        .arg(&absent)
        .stdin(Stdio::null())
        .output()?;
    assert_eq!(stdin.status.code(), Some(10));
    assert!(!absent.exists());
    assert_eq!(fs::read_dir(directory.path())?.count(), 2, "rejected mode created files");
    let help = binary().arg("--help").output()?;
    assert!(help.status.success());
    assert!(!String::from_utf8(help.stdout)?.contains("--write"));
    Ok(())
}

#[test]
fn file_stdout_formats_without_changing_the_input() -> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("source.pl");
    fs::write(&file, "my $x=1;\n")?;
    let output = binary().arg("--stdout").arg(&file).output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(output.stdout, b"my $x = 1;\n");
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read_to_string(&file)?, "my $x=1;\n");
    assert_eq!(fs::read_dir(directory.path())?.count(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn readonly_files_and_symlink_aliases_remain_valid_read_inputs() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("source.pl");
    let link = directory.path().join("link.pl");
    fs::write(&file, "my $x=1;\n")?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o444))?;
    symlink(&file, &link)?;
    let output = binary().args(["--check", "--json"]).arg(&link).arg(&file).output()?;
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["subjects"].as_array().ok_or("subjects")?.len(), 1);
    assert_eq!(report["subjects"][0]["subject"], file.canonicalize()?.to_string_lossy().as_ref());
    assert_eq!(fs::read_to_string(&file)?, "my $x=1;\n");
    assert_eq!(fs::metadata(&file)?.permissions().mode() & 0o777, 0o444);
    assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert_eq!(fs::read_dir(directory.path())?.count(), 2);
    Ok(())
}
