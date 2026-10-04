use color_eyre::eyre::{Context, Result};
use duct::cmd;

use crate::tasks::fmt as fmt_task;

pub(super) fn run_fmt_check() -> Result<()> {
    run_fmt_check_with(|| fmt_task::run(true, None))?;
    Ok(())
}

fn run_fmt_check_with<F>(mut run_fmt: F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    run_fmt().context("Format check failed")?;
    Ok(())
}

pub(super) fn run_clippy_check() -> Result<()> {
    cmd("cargo", &["clippy", "--workspace", "--all-targets", "--", "-Dwarnings", "-Amissing_docs"])
        .run()
        .context("Clippy check failed")?;
    Ok(())
}

pub(super) fn run_constrained_test(crate_name: &str) -> Result<()> {
    cmd("cargo", constrained_test_args(crate_name))
        .run()
        .with_context(|| format!("{} tests failed", crate_name))?;

    Ok(())
}

/// Argument vector for one constrained per-crate test lane.
///
/// `--no-fail-fast` is a cargo flag and must precede `--`: after the
/// separator it is forwarded to the libtest harness, which rejects it as an
/// unrecognized option and aborts the lane at the first test binary (#17177).
fn constrained_test_args(crate_name: &str) -> Vec<String> {
    vec![
        "test".to_string(),
        "-p".to_string(),
        crate_name.to_string(),
        "--tests".to_string(),
        "--no-fail-fast".to_string(),
        "--".to_string(),
        "--test-threads=1".to_string(),
        "-q".to_string(),
    ]
}

pub(super) fn run_docs_check() -> Result<()> {
    cmd("cargo", &["doc", "-p", "perl-parser", "--no-deps"])
        .run()
        .context("Documentation build failed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::run_fmt_check;
    use super::{constrained_test_args, run_fmt_check_with};
    use color_eyre::eyre::{Result, eyre};

    const MAX_CHILD_STDERR_BYTES: usize = 4096;

    /// Bounds a failed child's stderr before it is quoted into a test
    /// diagnostic. Only the test harness captures child stderr bytes: the
    /// production runners inherit the child's streams and discard the `duct`
    /// output, so this helper has no production consumer.
    fn bounded_child_stderr(stderr: &[u8]) -> String {
        let shown = stderr.get(..MAX_CHILD_STDERR_BYTES).unwrap_or(stderr);
        let suffix =
            if stderr.len() > MAX_CHILD_STDERR_BYTES { "\n[child stderr truncated]" } else { "" };
        format!("{}{}", String::from_utf8_lossy(shown), suffix)
    }

    #[test]
    fn ci_runner_fmt_check_uses_injected_package_formatter() -> Result<()> {
        let mut called = false;
        run_fmt_check_with(|| {
            called = true;
            Ok(())
        })?;

        assert!(called);
        Ok(())
    }

    #[test]
    fn ci_runner_fmt_check_preserves_format_context() -> Result<()> {
        let err = run_fmt_check_with(|| Err(eyre!("formatter unavailable")));
        let message = match err {
            Ok(()) => return Err(eyre!("expected fmt check failure")),
            Err(err) => err.chain().map(|cause| cause.to_string()).collect::<Vec<_>>().join("\n"),
        };

        assert!(message.contains("Format check failed"));
        assert!(message.contains("formatter unavailable"));
        Ok(())
    }

    #[test]
    fn ci_runner_constrained_test_places_cargo_flags_before_separator() -> Result<()> {
        let args = constrained_test_args("perl-lexer");
        let separator = args
            .iter()
            .position(|arg| arg == "--")
            .ok_or_else(|| eyre!("constrained test args must separate libtest args with `--`"))?;

        assert!(
            args[..separator].contains(&"--no-fail-fast".to_string()),
            "--no-fail-fast must be a cargo flag before the `--` separator: {args:?}"
        );
        assert!(
            !args[separator..].contains(&"--no-fail-fast".to_string()),
            "--no-fail-fast must not be forwarded to the libtest harness: {args:?}"
        );
        Ok(())
    }

    #[test]
    fn child_stderr_diagnostic_is_bounded() -> Result<()> {
        let stderr = vec![b'x'; MAX_CHILD_STDERR_BYTES + 1];
        let diagnostic = bounded_child_stderr(&stderr);
        let suffix = "\n[child stderr truncated]";

        if !diagnostic.ends_with(suffix) {
            return Err(eyre!("bounded diagnostic did not report truncation"));
        }
        if diagnostic.len() > MAX_CHILD_STDERR_BYTES + suffix.len() {
            return Err(eyre!("child stderr diagnostic exceeded its bound"));
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn ci_runner_fmt_check_routes_to_package_formatter() -> Result<()> {
        if crate::test_support::FakeCargo::child_requested() {
            return run_fmt_check();
        }

        let fake_cargo = crate::test_support::FakeCargoChild::run(
            "tasks::ci::runner::tests::ci_runner_fmt_check_routes_to_package_formatter",
        )?;

        if !fake_cargo.status().success() {
            return Err(color_eyre::eyre::eyre!(
                "fake cargo child failed: {}",
                bounded_child_stderr(fake_cargo.stderr()),
            ));
        }

        let invocations = fake_cargo.invocations()?;
        assert!(invocations.iter().any(|line| line == "metadata --format-version 1 --no-deps"));
        assert!(invocations.iter().any(|line| {
            line.starts_with("fmt --manifest-path ") && line.ends_with(" -- --check")
        }));
        Ok(())
    }
}
