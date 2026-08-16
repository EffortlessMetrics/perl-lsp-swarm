//! Shared workspace library-test inventory for status generators.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use color_eyre::eyre::{Result, bail};
use regex::Regex;

use super::run_cmd_merged;

#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static ANSI_ESCAPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("ANSI escape regex is valid"));
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static RUNNING_TEST_BINARY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Running unittests[^\(]*\([^\)]*deps[/\\]([a-zA-Z0-9_-]+)-[0-9a-f]+(?:\.exe)?\)")
        .expect("running-test regex is valid")
});
#[allow(clippy::expect_used, reason = "static LazyLock regex with known-good pattern")]
static TEST_LIST_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*test\s*$").expect("test-list-line regex is valid"));

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PerCrateTestCounts {
    pub(super) by_crate: BTreeMap<String, usize>,
    pub(super) unattributed: usize,
}

impl PerCrateTestCounts {
    pub(super) fn total(&self) -> usize {
        self.by_crate.values().sum::<usize>() + self.unattributed
    }
}

pub(super) fn collect_for_selection<T>(
    need_tests: bool,
    need_quality: bool,
    collect: impl FnOnce() -> Result<T>,
) -> Result<Option<T>> {
    if need_quality {
        collect().map(Some)
    } else if need_tests {
        Ok(collect().ok())
    } else {
        Ok(None)
    }
}

pub(super) fn collect_per_crate_test_counts(root: &Path) -> Result<PerCrateTestCounts> {
    let output = run_cmd_merged(
        root,
        &["cargo", "test", "--workspace", "--lib", "--exclude", "tree-sitter-perl", "--", "--list"],
        Duration::from_mins(12),
    );
    if output.is_empty() {
        bail!("test inventory discovery failed or returned no output");
    }
    validate_per_crate_test_counts(parse_per_crate_test_counts(&output))
}

fn validate_per_crate_test_counts(counts: PerCrateTestCounts) -> Result<PerCrateTestCounts> {
    if counts.total() == 0 {
        bail!("test inventory discovery returned zero tests; refusing to overwrite status");
    }
    Ok(counts)
}

fn parse_per_crate_test_counts(output: &str) -> PerCrateTestCounts {
    let mut counts = PerCrateTestCounts::default();
    let mut current_crate: Option<String> = None;
    let mut attributed = 0usize;
    let mut discovered = 0usize;
    for line in output.lines() {
        let plain_line = ANSI_ESCAPE_RE.replace_all(line, "");
        if let Some(caps) = RUNNING_TEST_BINARY_RE.captures(plain_line.as_ref()) {
            current_crate = Some(caps[1].replace('_', "-"));
        } else if TEST_LIST_LINE_RE.is_match(plain_line.as_ref()) {
            discovered += 1;
            if let Some(krate) = &current_crate {
                *counts.by_crate.entry(krate.clone()).or_default() += 1;
                attributed += 1;
            }
        }
    }
    counts.unattributed = discovered.saturating_sub(attributed);
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn combined_selection_collects_once() -> Result<()> {
        let calls = Cell::new(0);
        let inventory = collect_for_selection(true, true, || {
            calls.set(calls.get() + 1);
            Ok(7)
        })?;
        assert_eq!(inventory, Some(7));
        assert_eq!(calls.get(), 1);
        Ok(())
    }

    #[test]
    fn tests_only_degrades_collection_failure() -> Result<()> {
        let inventory = collect_for_selection::<usize>(true, false, || bail!("unavailable"))?;
        assert_eq!(inventory, None);
        Ok(())
    }

    #[test]
    fn quality_selection_propagates_collection_failure() {
        let result = collect_for_selection::<usize>(false, true, || bail!("unavailable"));
        assert!(result.is_err());
    }

    #[test]
    fn unrelated_selection_does_not_collect() -> Result<()> {
        let calls = Cell::new(0);
        let inventory = collect_for_selection(false, false, || {
            calls.set(calls.get() + 1);
            Ok(7)
        })?;
        assert_eq!(inventory, None);
        assert_eq!(calls.get(), 0);
        Ok(())
    }

    #[test]
    fn parser_normalizes_color_and_preserves_unattributed_tests() {
        let output = "orphan: test\n\x1b[1m\x1b[32m Running\x1b[0m unittests src/lib.rs \
            (target/debug/deps/perl_parser_core-abc123)\nparser_smoke: test\n";
        let counts = parse_per_crate_test_counts(output);
        assert_eq!(counts.by_crate.get("perl-parser-core"), Some(&1));
        assert_eq!(counts.unattributed, 1);
        assert_eq!(counts.total(), 2);
    }

    #[test]
    fn parser_handles_unix_windows_and_external_target_paths() {
        let output = "Running unittests src/lib.rs \
            (target/debug/deps/perl_parser_core-abc123)\nparser_smoke: test\n\
            Running unittests src/lib.rs \
            (C:\\tmp\\target\\debug\\deps\\perl_workspace-123def.exe)\nworkspace_smoke: test\n\
            Running unittests src/lib.rs \
            (/tmp/target/debug/deps/perl_lsp_rs-feed456)\nlsp_smoke: test\n";
        let counts = parse_per_crate_test_counts(output);
        assert_eq!(counts.by_crate.get("perl-parser-core"), Some(&1));
        assert_eq!(counts.by_crate.get("perl-workspace"), Some(&1));
        assert_eq!(counts.by_crate.get("perl-lsp-rs"), Some(&1));
    }

    #[test]
    fn parser_distinguishes_unattributed_tests_from_a_named_package() {
        let output = "orphan: test\n\
            Running unittests src/lib.rs (target/debug/deps/unattributed-abc123)\n\
            package_test: test\n";
        let counts = parse_per_crate_test_counts(output);
        assert_eq!(counts.by_crate.get("unattributed"), Some(&1));
        assert_eq!(counts.unattributed, 1);
    }

    #[test]
    fn zero_discovery_is_rejected() {
        assert!(validate_per_crate_test_counts(PerCrateTestCounts::default()).is_err());
    }
}
