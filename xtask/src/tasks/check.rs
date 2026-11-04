//! Code quality check task implementation

use color_eyre::eyre::{Context, Result};
use duct::cmd;
use indicatif::{ProgressBar, ProgressStyle};

use crate::tasks::fmt as fmt_task;

pub fn run(clippy: bool, fmt: bool, all: bool) -> Result<()> {
    run_with_executor(clippy, fmt, all, |check| match check {
        CheckKind::Clippy => {
            cmd("cargo", &["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"])
                .run()
                .context("Clippy check failed")?;
            Ok(())
        }
        CheckKind::Fmt => {
            fmt_task::run(true, None).context("Format check failed")?;
            Ok(())
        }
        CheckKind::Build => {
            cmd("cargo", &["check", "--all-targets", "--all-features"])
                .run()
                .context("Build check failed")?;
            Ok(())
        }
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CheckKind {
    Clippy,
    Fmt,
    Build,
}

impl CheckKind {
    fn label(self) -> &'static str {
        match self {
            Self::Clippy => "clippy",
            Self::Fmt => "fmt",
            Self::Build => "build",
        }
    }
}

fn planned_checks(clippy: bool, fmt: bool, all: bool) -> Vec<CheckKind> {
    if all {
        return vec![CheckKind::Clippy, CheckKind::Fmt, CheckKind::Build];
    }

    let mut checks = Vec::new();
    if clippy {
        checks.push(CheckKind::Clippy);
    }
    if fmt {
        checks.push(CheckKind::Fmt);
    }
    if checks.is_empty() {
        checks.push(CheckKind::Build);
    }
    checks
}

fn run_with_executor<F>(clippy: bool, fmt: bool, all: bool, mut executor: F) -> Result<()>
where
    F: FnMut(CheckKind) -> Result<()>,
{
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );

    for check in planned_checks(clippy, fmt, all) {
        spinner.set_message(format!("Running {} check", check.label()));
        executor(check)?;
    }

    spinner.finish_with_message("✅ All checks passed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CheckKind, planned_checks, run, run_with_executor};
    use color_eyre::eyre::Result;

    #[test]
    fn check_task_plans_all_checks_in_ci_order() {
        assert_eq!(
            planned_checks(false, false, true),
            vec![CheckKind::Clippy, CheckKind::Fmt, CheckKind::Build]
        );
    }

    #[test]
    fn check_task_plans_requested_checks_only() {
        assert_eq!(planned_checks(true, false, false), vec![CheckKind::Clippy]);
        assert_eq!(planned_checks(false, true, false), vec![CheckKind::Fmt]);
        assert_eq!(planned_checks(true, true, false), vec![CheckKind::Clippy, CheckKind::Fmt]);
    }

    #[test]
    fn check_task_defaults_to_build_when_no_flag_selected() {
        assert_eq!(planned_checks(false, false, false), vec![CheckKind::Build]);
    }

    #[test]
    fn check_task_executor_receives_package_fmt_step_without_running_cargo() -> Result<()> {
        let mut observed = Vec::new();
        run_with_executor(false, true, false, |check| {
            observed.push(check);
            Ok(())
        })?;

        assert_eq!(observed, vec![CheckKind::Fmt]);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn check_task_run_routes_fmt_through_package_formatter() -> Result<()> {
        if crate::test_support::FakeCargoChild::child_requested() {
            return run(false, true, false);
        }
        let fake_cargo = crate::test_support::FakeCargoChild::run(
            "tasks::check::tests::check_task_run_routes_fmt_through_package_formatter",
        )?;
        fake_cargo.ensure_success()?;

        let invocations = fake_cargo.invocations()?;
        assert!(invocations.iter().any(|line| line == "metadata --format-version 1 --no-deps"));
        fake_cargo.assert_package_formatting()?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn check_task_run_preserves_all_check_order() -> Result<()> {
        if crate::test_support::FakeCargoChild::child_requested() {
            return run(false, false, true);
        }
        let fake_cargo = crate::test_support::FakeCargoChild::run(
            "tasks::check::tests::check_task_run_preserves_all_check_order",
        )?;
        fake_cargo.ensure_success()?;
        fake_cargo.assert_package_formatting()?;
        let mut expected = vec![
            "clippy --all-targets --all-features -- -D warnings".to_string(),
            "metadata --format-version 1 --no-deps".to_string(),
        ];
        expected.extend(fake_cargo.expected_format_invocations());
        expected.push("check --all-targets --all-features".to_string());
        assert_eq!(fake_cargo.invocations()?, expected);
        Ok(())
    }
}
