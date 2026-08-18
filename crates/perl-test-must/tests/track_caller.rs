//! Process-isolated proof for the `#[track_caller]` contract.

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use perl_test_must::{must, must_err, must_err_with, must_some, must_some_with, must_with};

const CHILD_ENV: &str = "PERL_TEST_MUST_TRACK_CALLER_CHILD";
const MARKER_PREFIX: &str = "PERL_TEST_MUST_TRACK_CALLER|";
const HELPERS: [&str; 6] =
    ["must", "must_with", "must_some", "must_some_with", "must_err", "must_err_with"];

static EXPECTED_LINE: AtomicU32 = AtomicU32::new(0);

#[test]
fn every_helper_reports_the_integration_test_invocation() -> Result<(), String> {
    let executable = env::current_exe().map_err(|error| error.to_string())?;

    for helper in HELPERS {
        let output = Command::new(&executable)
            .args(["--exact", "track_caller_child", "--nocapture"])
            .env(CHILD_ENV, helper)
            .output()
            .map_err(|error| error.to_string())?;
        let stderr = String::from_utf8(output.stderr).map_err(|error| error.to_string())?;

        assert!(!output.status.success(), "{helper} child unexpectedly succeeded:\n{stderr}");

        let marker = parse_marker(&stderr)?;
        let normalized_file = marker.file.replace('\\', "/");

        assert!(
            normalized_file.ends_with("tests/track_caller.rs"),
            "{helper} reported a non-caller file {}:\n{stderr}",
            marker.file
        );
        assert!(
            !normalized_file.ends_with("src/lib.rs"),
            "{helper} reported helper internals instead of its caller:\n{stderr}"
        );
        assert_eq!(
            marker.actual_line, marker.expected_line,
            "{helper} reported the wrong source line:\n{stderr}"
        );
    }

    Ok(())
}

#[test]
fn track_caller_child() {
    let Some(helper) = env::var_os(CHILD_ENV) else {
        return;
    };

    std::panic::set_hook(Box::new(|info| {
        let Some(location) = info.location() else {
            return;
        };

        write_stderr(format_args!(
            "{MARKER_PREFIX}{}|{}|{}",
            location.file(),
            location.line(),
            EXPECTED_LINE.load(Ordering::Relaxed)
        ));
    }));

    match helper.to_string_lossy().as_ref() {
        "must" => trigger_must(),
        "must_with" => trigger_must_with(),
        "must_some" => trigger_must_some(),
        "must_some_with" => trigger_must_some_with(),
        "must_err" => trigger_must_err(),
        "must_err_with" => trigger_must_err_with(),
        other => write_stderr(format_args!("unknown helper selector: {other}")),
    }
}

#[inline(never)]
fn trigger_must() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    must::<(), &str>(Err("boom"));
}

#[inline(never)]
fn trigger_must_with() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    must_with::<(), &str>(Err("boom"), "context");
}

#[inline(never)]
fn trigger_must_some() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    let _ = must_some::<()>(None);
}

#[inline(never)]
fn trigger_must_some_with() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    let _ = must_some_with::<()>(None, "context");
}

#[inline(never)]
fn trigger_must_err() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    let _ = must_err::<(), &str>(Ok(()));
}

#[inline(never)]
fn trigger_must_err_with() {
    EXPECTED_LINE.store(line!() + 1, Ordering::Relaxed);
    let _ = must_err_with::<(), &str>(Ok(()), "context");
}

struct CallerMarker {
    file: String,
    actual_line: u32,
    expected_line: u32,
}

fn parse_marker(stderr: &str) -> Result<CallerMarker, String> {
    let encoded = stderr
        .lines()
        .find_map(|line| line.strip_prefix(MARKER_PREFIX))
        .ok_or_else(|| format!("caller-location marker missing:\n{stderr}"))?;
    let mut fields = encoded.split('|');

    let file = fields
        .next()
        .ok_or_else(|| String::from("caller-location marker omitted the file"))?
        .to_owned();
    let actual_line = fields
        .next()
        .ok_or_else(|| String::from("caller-location marker omitted the actual line"))?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;
    let expected_line = fields
        .next()
        .ok_or_else(|| String::from("caller-location marker omitted the expected line"))?
        .parse::<u32>()
        .map_err(|error| error.to_string())?;

    if fields.next().is_some() {
        return Err(String::from("caller-location marker had extra fields"));
    }

    Ok(CallerMarker { file, actual_line, expected_line })
}

fn write_stderr(arguments: fmt::Arguments<'_>) {
    let mut stderr = io::stderr().lock();
    let _write_result = writeln!(stderr, "{arguments}");
}
