use color_eyre::eyre::{Context, Result};
use regex::Regex;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

const CI_REPORT_CRATES_EXCLUDE: [&str; 5] = [
    "tree-sitter-perl-c",
    "perl-parser-pest",
    "perl-tdd-support",
    "perl-test-must",
    "perl-ci-hygiene",
];

const CI_TEST_FILE_SUFFIXES: [&str; 3] = ["_test.rs", "_tests.rs", "tests.rs"];

pub(crate) fn is_excluded_test_path(path: &Path) -> bool {
    if path.components().any(|component| {
        let value = component.as_os_str();
        value == OsStr::new("tests")
            || value == OsStr::new("benches")
            || value == OsStr::new("examples")
            || value == OsStr::new("bin")
    }) {
        return true;
    }

    if let Some(file_name) = path.file_name().and_then(|name| name.to_str())
        && CI_TEST_FILE_SUFFIXES.iter().any(|suffix| file_name.ends_with(suffix))
    {
        return true;
    }

    if path.components().any(|component| {
        CI_REPORT_CRATES_EXCLUDE.iter().any(|item| component.as_os_str() == OsStr::new(item))
    }) {
        return true;
    }

    false
}

pub(crate) fn first_cfg_test_line_number(path: &Path) -> Result<usize> {
    let contents = crate::read_lines(path)?;
    let pattern = Regex::new(r"^\s*#\[cfg\(test\)\]")?;
    for (idx, line) in contents.iter().enumerate() {
        if pattern.is_match(line) {
            return Ok(idx + 1);
        }
    }
    Ok(usize::MAX)
}

pub(crate) fn read_json_value(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let value =
        serde_json::from_str(&raw).with_context(|| format!("parsing JSON in {:?}", path))?;
    Ok(value)
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn read_usize_from_path(path: &Path) -> Result<usize> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    raw.trim()
        .parse::<usize>()
        .map_err(|err| color_eyre::eyre::eyre!("invalid usize in {}: {err}", path.display()))
}

#[cfg_attr(windows, allow(dead_code))]
pub(crate) fn read_usize_from_tokens(path: &Path, idx: usize) -> Result<usize> {
    let raw = fs::read_to_string(path).with_context(|| format!("reading {:?}", path))?;
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.len() <= idx {
        return Err(color_eyre::eyre::eyre!("missing token {idx} in {}", path.display()));
    }
    tokens[idx]
        .trim()
        .parse::<usize>()
        .map_err(|err| color_eyre::eyre::eyre!("invalid usize in {}: {err}", path.display()))
}
