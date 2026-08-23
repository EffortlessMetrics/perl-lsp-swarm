use super::super::model::RustVersion;
use chrono::NaiveDate;
use color_eyre::eyre::{Result, bail, eyre};

pub(super) fn validate_lint_name(name: &str) -> Result<()> {
    let Some((tool, lint)) = name.split_once("::") else {
        bail!("lint name {name} must include a rust:: or clippy:: namespace");
    };
    if !matches!(tool, "rust" | "clippy") || lint.is_empty() || lint.contains("::") {
        bail!("lint name {name} must use exactly one rust:: or clippy:: namespace");
    }
    if lint.contains('-') {
        bail!("lint name {name} must use canonical underscore spelling");
    }
    Ok(())
}

pub(super) fn validate_level(name: &str, level: &str, allow_level: bool) -> Result<()> {
    let supported = if allow_level {
        matches!(level, "allow" | "warn" | "deny" | "forbid")
    } else {
        matches!(level, "warn" | "deny" | "forbid")
    };
    if !supported {
        bail!("lint {name} has unsupported level {level}");
    }
    Ok(())
}

pub(super) fn validate_nonempty(name: &str, field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("lint {name} must have a non-empty {field}");
    }
    Ok(())
}

pub(crate) fn ensure_version_matches(
    source: &str,
    expected: RustVersion,
    actual: &str,
) -> Result<()> {
    let actual_version = RustVersion::from_text(actual)?;
    if actual_version != expected {
        bail!(
            "{source} Rust version {actual} does not match product version {}.{}.{}",
            expected.major,
            expected.minor,
            expected.patch
        );
    }
    Ok(())
}

pub(super) fn parse_review_date(name: &str, review_after: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(review_after, "%Y-%m-%d")
        .map_err(|err| eyre!("lint {name} has invalid review_after date: {err}"))
}
