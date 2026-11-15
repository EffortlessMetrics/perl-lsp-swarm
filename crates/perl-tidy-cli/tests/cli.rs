#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_perl-tidy"))
}

fn stdin_command(args: &[&str], input: &[u8]) -> std::io::Result<Output> {
    let mut child = binary()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input)?;
    }
    child.wait_with_output()
}

fn no_temporaries(directory: &Path) -> TestResult {
    for entry in fs::read_dir(directory)? {
        assert!(!entry?.file_name().to_string_lossy().starts_with(".perl-tidy-"));
    }
    Ok(())
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
        &["--stdin", "--stdin-filename", absent.to_str().expect("test path"), "--json"],
        b"my $x=1;\n",
    )?;
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["subjects"][0]["logical_stdin_filename"], absent.to_string_lossy().as_ref());
    assert_eq!(value["configuration"]["project_resolution"], "not_performed");
    assert!(!absent.exists());
    let write = stdin_command(&["--stdin", "--write"], b"my $x=1;\n")?;
    assert!(!write.status.success());
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
    for diagnostic in report["subjects"][0]["diagnostics"].as_array().expect("diagnostics") {
        assert!(diagnostic.get("code").is_some());
        assert!(diagnostic.get("message").is_none());
    }
    assert!(!String::from_utf8(output.stdout)?.contains("private_customer_token"));
    Ok(())
}

#[test]
fn invalid_configuration_and_input_limits_are_not_clean() -> TestResult {
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
        &["--stdin", "--check", "--json", "--config", config_path.to_str().expect("test path")],
        b"my $x = 1;\n",
    )?;
    assert!(valid.status.success(), "{}", String::from_utf8_lossy(&valid.stdout));
    config["mode"] = Value::from("external-legacy");
    fs::write(&config_path, serde_json::to_vec(&config)?)?;
    let invalid = stdin_command(
        &["--stdin", "--json", "--config", config_path.to_str().expect("test path")],
        b"my $x = 1;\n",
    )?;
    assert_eq!(invalid.status.code(), Some(3));
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
    assert_eq!(report["subjects"].as_array().expect("subjects").len(), 2);
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
    for name in ["perltidy", "perl"] {
        let tool = directory.path().join(name);
        fs::write(
            &tool,
            format!("#!/bin/sh\nprintf called > \"{}\"\nexit 99\n", marker.display()),
        )?;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))?;
    }
    let mut child = binary()
        .args(["--stdin", "--check", "--json"])
        .env("PATH", directory.path())
        .env("PERLTIDY", directory.path().join("perltidy"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().expect("stdin").write_all(b"my $x = 1;\n")?;
    let output = child.wait_with_output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(!marker.exists());
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn write_plans_every_file_before_mutating_any() -> TestResult {
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a.pl");
    let b = directory.path().join("b.pl");
    fs::write(&a, "my $x=1;\n")?;
    fs::write(&b, "my $x = (\n")?;
    let output = binary().arg("--write").arg("--json").arg(&a).arg(&b).output()?;
    assert_eq!(output.status.code(), Some(2), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(fs::read_to_string(&a)?, "my $x=1;\n");
    assert_eq!(fs::read_to_string(&b)?, "my $x = (\n");
    let value: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(value["subjects"][0]["filesystem_effect"], "blocked_by_batch");
    no_temporaries(directory.path())
}

#[cfg(target_os = "linux")]
#[test]
fn successful_write_preserves_mode_and_next_check_is_clean() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a.pl");
    let b = directory.path().join("b.pl");
    fs::write(&a, "my $x=1;\n")?;
    fs::write(&b, "my $y=2;\n")?;
    fs::set_permissions(&a, fs::Permissions::from_mode(0o751))?;
    let output = binary().arg("--write").arg("--json").arg(&a).arg(&b).output()?;
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(fs::read_to_string(&a)?, "my $x = 1;\n");
    assert_eq!(fs::read_to_string(&b)?, "my $y = 2;\n");
    assert_eq!(fs::metadata(&a)?.permissions().mode() & 0o777, 0o751);
    let check = binary().arg("--check").arg(&a).arg(&b).output()?;
    assert!(check.status.success(), "{}", String::from_utf8_lossy(&check.stderr));
    no_temporaries(directory.path())
}

#[cfg(target_os = "linux")]
#[test]
fn readonly_symlink_and_hardlink_inputs_block_write_without_effects() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir()?;
    let a = directory.path().join("a.pl");
    let b = directory.path().join("b.pl");
    fs::write(&a, "my $x=1;\n")?;
    fs::write(&b, "my $y=2;\n")?;
    fs::set_permissions(&b, fs::Permissions::from_mode(0o444))?;
    let readonly = binary().arg("--write").arg(&a).arg(&b).output()?;
    assert_eq!(readonly.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&a)?, "my $x=1;\n");
    let real_directory = directory.path().join("real");
    fs::create_dir(&real_directory)?;
    let nested = real_directory.join("nested.pl");
    fs::write(&nested, "my $z=3;\n")?;
    let directory_link = directory.path().join("directory-link");
    symlink(&real_directory, &directory_link)?;
    let parent_linked = binary().arg("--write").arg(directory_link.join("nested.pl")).output()?;
    assert_eq!(parent_linked.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&nested)?, "my $z=3;\n");
    fs::set_permissions(&b, fs::Permissions::from_mode(0o644))?;
    let link = directory.path().join("link.pl");
    symlink(&a, &link)?;
    let linked = binary().arg("--write").arg(&link).output()?;
    assert_eq!(linked.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&a)?, "my $x=1;\n");
    let hard = directory.path().join("hard.pl");
    fs::hard_link(&a, &hard)?;
    let hardlinked = binary().arg("--write").arg(&a).output()?;
    assert_eq!(hardlinked.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&hard)?, "my $x=1;\n");
    no_temporaries(directory.path())
}

#[cfg(target_os = "linux")]
#[test]
fn write_refuses_extended_metadata_without_losing_it() -> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("source.pl");
    fs::write(&file, "my $x=1;\n")?;
    xattr::set(&file, "user.perl-tidy-test", b"preserve me")?;
    let output = binary().arg("--write").arg(&file).output()?;
    assert_eq!(output.status.code(), Some(7));
    assert_eq!(fs::read_to_string(&file)?, "my $x=1;\n");
    assert_eq!(xattr::get(&file, "user.perl-tidy-test")?, Some(b"preserve me".to_vec()));
    no_temporaries(directory.path())
}
