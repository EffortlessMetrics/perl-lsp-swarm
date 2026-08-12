#!/usr/bin/env python3
from pathlib import Path

ROOT = Path("xtask/src/tasks/update_status")


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement, found {count}: {old[:120]!r}")
    path.write_text(text.replace(old, new), encoding="utf-8")


(ROOT / "test_inventory.rs").write_text(
    r'''//! Shared workspace library-test inventory for status regeneration.
//!
//! One bounded `cargo test -- --list` invocation owns both the aggregate Tier-A
//! total and the per-crate quality table. Keeping parsing and validation here
//! prevents the tests and quality renderers from drifting onto separate evidence.

// LazyLock<Regex> initializers use .expect() for known-good patterns — permitted by coding standards.
#![allow(clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use color_eyre::eyre::{Result, bail};
use regex::Regex;

use super::run_cmd_merged;

static ANSI_ESCAPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*m").expect("ANSI escape regex is valid"));

static RUNNING_TEST_BINARY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Running unittests[^\(]*\([^\)]*deps[/\\]([a-zA-Z0-9_-]+)-[0-9a-f]+(?:\.exe)?\)")
        .expect("running-test regex is valid")
});

static TEST_LIST_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":\s*test\s*$").expect("test-list-line regex is valid"));

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct WorkspaceLibTestInventory {
    by_crate: BTreeMap<String, usize>,
    unattributed: usize,
}

impl WorkspaceLibTestInventory {
    pub(super) fn total(&self) -> usize {
        self.by_crate.values().sum::<usize>() + self.unattributed
    }

    pub(super) fn by_crate(&self) -> &BTreeMap<String, usize> {
        &self.by_crate
    }

    pub(super) fn unattributed(&self) -> usize {
        self.unattributed
    }

    #[cfg(test)]
    pub(super) fn from_parts(by_crate: BTreeMap<String, usize>, unattributed: usize) -> Self {
        Self { by_crate, unattributed }
    }
}

/// Discover the workspace library tests once and preserve aggregate and per-crate views.
///
/// Cargo writes test-binary headers to stderr and test names to stdout. The merged command
/// keeps those streams ordered closely enough for each listed test to inherit its active crate.
pub(super) fn collect_workspace_lib_test_inventory(
    root: &Path,
) -> Result<WorkspaceLibTestInventory> {
    let output = run_cmd_merged(
        root,
        &[
            "cargo",
            "test",
            "--workspace",
            "--lib",
            "--exclude",
            "tree-sitter-perl",
            "--",
            "--list",
        ],
        // A cold cache-targets=false runner compiles the workspace before listing.
        // Keep the sole discovery bounded while allowing enough headroom for that compile.
        Duration::from_mins(12),
    );
    if output.is_empty() {
        bail!("workspace lib-test discovery failed or returned no output");
    }
    validate_workspace_lib_test_inventory(parse_workspace_lib_test_inventory(&output))
}

fn validate_workspace_lib_test_inventory(
    inventory: WorkspaceLibTestInventory,
) -> Result<WorkspaceLibTestInventory> {
    if inventory.total() == 0 {
        bail!("workspace lib-test discovery returned zero tests");
    }
    Ok(inventory)
}

fn parse_workspace_lib_test_inventory(output: &str) -> WorkspaceLibTestInventory {
    let mut by_crate: BTreeMap<String, usize> = BTreeMap::new();
    let mut current_crate: Option<String> = None;
    let mut discovered = 0usize;
    let mut attributed = 0usize;

    for line in output.lines() {
        let plain_line = ANSI_ESCAPE_RE.replace_all(line, "");
        if let Some(caps) = RUNNING_TEST_BINARY_RE.captures(plain_line.as_ref()) {
            current_crate = Some(caps[1].replace('_', "-"));
            continue;
        }
        if TEST_LIST_LINE_RE.is_match(plain_line.as_ref()) {
            discovered += 1;
            if let Some(ref krate) = current_crate {
                *by_crate.entry(krate.clone()).or_default() += 1;
                attributed += 1;
            }
        }
    }

    WorkspaceLibTestInventory {
        by_crate,
        unattributed: discovered.saturating_sub(attributed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_reconciles_attributed_and_unattributed_tests() {
        let inventory = WorkspaceLibTestInventory::from_parts(
            BTreeMap::from([
                (String::from("perl-parser"), 3),
                (String::from("perl-lsp-rs"), 4),
            ]),
            2,
        );

        assert_eq!(inventory.total(), 9);
    }

    #[test]
    fn parser_strips_cargo_color_from_test_binary_headers() {
        let output = "\x1b[1m\x1b[32m     Running\x1b[0m unittests src/lib.rs \
            (target/debug/deps/perl_parser_core-abc123)\n\
            parser_smoke: test\n";

        let inventory = parse_workspace_lib_test_inventory(output);

        assert_eq!(inventory.by_crate().get("perl-parser-core"), Some(&1));
        assert_eq!(inventory.unattributed(), 0);
    }

    #[test]
    fn parser_handles_unix_windows_and_external_target_paths() {
        let output = "Running unittests src/lib.rs \
            (target/debug/deps/perl_parser_core-abc123)\n\
            parser_smoke: test\n\
            Running unittests src/lib.rs \
            (C:\\Users\\steven\\Temp\\debug\\deps\\perl_workspace-123def.exe)\n\
            workspace_indexes: test\n\
            Running unittests src/lib.rs \
            (/tmp/cargo-out/debug/deps/perl_lsp_rs-cafe456)\n\
            lsp_smoke: test\n";

        let inventory = parse_workspace_lib_test_inventory(output);

        assert_eq!(inventory.by_crate().get("perl-parser-core"), Some(&1));
        assert_eq!(inventory.by_crate().get("perl-workspace"), Some(&1));
        assert_eq!(inventory.by_crate().get("perl-lsp-rs"), Some(&1));
        assert_eq!(inventory.total(), 3);
    }

    #[test]
    fn parser_preserves_tests_without_an_active_crate() {
        let output = "orphan_test: test\n\
            Running unittests src/lib.rs (target/debug/deps/perl_parser_core-abc123)\n\
            parser_smoke: test\n\
            note: test\n\
            Running unittests src/lib.rs (target/debug/deps/unattributed-987def)\n\
            package_test: test\n";

        let inventory = parse_workspace_lib_test_inventory(output);

        assert_eq!(inventory.by_crate().get("perl-parser-core"), Some(&2));
        assert_eq!(inventory.by_crate().get("unattributed"), Some(&1));
        assert_eq!(inventory.unattributed(), 1);
        assert_eq!(inventory.total(), 4);
    }

    #[test]
    fn validation_rejects_zero_discovery() {
        let inventory = WorkspaceLibTestInventory::from_parts(
            BTreeMap::from([(String::from("perl-parser"), 0)]),
            0,
        );

        let result = validate_workspace_lib_test_inventory(inventory);

        assert!(result.is_err(), "zero discovery must fail closed");
    }
}
''',
    encoding="utf-8",
)

quality = ROOT / "quality.rs"
replace_once(quality, "use std::time::Duration;\n", "")
replace_once(quality, "use color_eyre::eyre::{Result, bail};\n", "use color_eyre::eyre::Result;\n")
replace_once(
    quality,
    "use super::flaky::{collect_flaky_test_summary, format_flaky_tests_section};\n",
    "use super::flaky::{collect_flaky_test_summary, format_flaky_tests_section};\nuse super::test_inventory::WorkspaceLibTestInventory;\n",
)
replace_once(quality, "use super::{replace_block, run_cmd_merged};\n", "use super::replace_block;\n")
text = quality.read_text(encoding="utf-8")
start = text.index("static ANSI_ESCAPE_RE:")
end = text.index("// ---------------------------------------------------------------------------\n// Metric collectors")
text = text[:start] + text[end:]
start = text.index("/// Run `cargo test --workspace --lib -- --list`")
end = text.index("/// Read `docs/project/status/editor_ux.md`")
text = text[:start] + text[end:]
text = text.replace("&PerCrateTestCounts", "&WorkspaceLibTestInventory")
text = text.replace("tests.by_crate.keys()", "tests.by_crate().keys()")
text = text.replace("tests.by_crate.get(c)", "tests.by_crate().get(c)")
text = text.replace("tests.unattributed > 0", "tests.unattributed() > 0")
text = text.replace("tests.unattributed\n", "tests.unattributed()\n")
quality.write_text(text, encoding="utf-8")

quality_tests = ROOT / "quality/tests.rs"
text = quality_tests.read_text(encoding="utf-8")
text = text.replace(
    '''let tests = PerCrateTestCounts {
        by_crate: BTreeMap::from([(String::from("perl-quote"), 42)]),
        unattributed: 0,
    };''',
    '''let tests = WorkspaceLibTestInventory::from_parts(
        BTreeMap::from([(String::from("perl-quote"), 42)]),
        0,
    );''',
)
text = text.replace("&PerCrateTestCounts::default()", "&WorkspaceLibTestInventory::default()")
text = text.replace(
    "let tests = PerCrateTestCounts { by_crate: BTreeMap::new(), unattributed: 2 };",
    "let tests = WorkspaceLibTestInventory::from_parts(BTreeMap::new(), 2);",
)
start = text.index("#[test]\nfn test_parse_per_crate_test_counts_parses_unix_and_windows_paths()")
end = text.index("// ---------------------------------------------------------------------------\n// Receipt-reading tests")
text = text[:start] + text[end:]
quality_tests.write_text(text, encoding="utf-8")

tests = ROOT / "tests.rs"
replace_once(
    tests,
    "use super::quality::PerCrateTestCounts;\n",
    "use super::test_inventory::WorkspaceLibTestInventory;\n",
)
text = tests.read_text(encoding="utf-8").replace(
    "PerCrateTestCounts", "WorkspaceLibTestInventory"
)
text = text.replace(
    '''let inventory = WorkspaceLibTestInventory {
            by_crate: std::collections::BTreeMap::from([
                ("perl-parser".to_string(), 3),
                ("perl-lsp-rs".to_string(), 4),
            ]),
            unattributed: 2,
        };''',
    '''let inventory = WorkspaceLibTestInventory::from_parts(
            std::collections::BTreeMap::from([
                ("perl-parser".to_string(), 3),
                ("perl-lsp-rs".to_string(), 4),
            ]),
            2,
        );''',
)
tests.write_text(text, encoding="utf-8")

module = ROOT / "mod.rs"
replace_once(module, "mod quality;\nmod tests;\n", "mod quality;\nmod test_inventory;\nmod tests;\n")
helper = '''// ---------------------------------------------------------------------------
// Shared evidence collection
// ---------------------------------------------------------------------------

fn collect_shared_test_inventory<F>(
    need_tests: bool,
    need_quality: bool,
    collect: F,
) -> Result<Option<test_inventory::WorkspaceLibTestInventory>>
where
    F: FnOnce() -> Result<test_inventory::WorkspaceLibTestInventory>,
{
    if !need_tests && !need_quality {
        return Ok(None);
    }

    match collect() {
        Ok(inventory) => Ok(Some(inventory)),
        Err(err) if need_quality => Err(err),
        Err(err) => {
            eprintln!(
                "[update-status] workspace lib-test inventory unavailable; \
                 tests will render Tier A as UNVERIFIED: {err:#}"
            );
            Ok(None)
        }
    }
}

'''
replace_once(
    module,
    "// ---------------------------------------------------------------------------\n// Public entry point\n// ---------------------------------------------------------------------------\n",
    helper + "// ---------------------------------------------------------------------------\n// Public entry point\n// ---------------------------------------------------------------------------\n",
)
text = module.read_text(encoding="utf-8")
start = text.index("    // One compiled workspace-lib inventory owns both the Tier-A total")
end = text.index("    // --- Tests subsystem ---", start)
replacement = '''    let workspace_test_inventory =
        collect_shared_test_inventory(need_tests, need_quality, || {
            run_subsystem(
                "test-inventory",
                "cargo test --workspace --lib --exclude tree-sitter-perl -- --list",
                || test_inventory::collect_workspace_lib_test_inventory(&root),
            )
        })?;

'''
text = text[:start] + replacement + text[end:]
text = text.replace("test_inventory.as_ref()", "workspace_test_inventory.as_ref()")
text = text.replace(
    '''            let inventory = test_inventory
                .as_ref()
                .ok_or_else(|| eyre!("quality test inventory missing after required discovery"))?;
            let updated_quality =
                quality::generate_quality_status(&root, &original_quality, inventory)?;''',
    '''            let test_inventory = workspace_test_inventory
                .as_ref()
                .ok_or_else(|| eyre!("quality requires the shared workspace lib-test inventory"))?;
            let updated_quality =
                quality::generate_quality_status(&root, &original_quality, test_inventory)?;''',
)
module.write_text(text, encoding="utf-8")

mod_tests = ROOT / "mod_tests.rs"
replace_once(
    mod_tests,
    '        "quality.rs",\n        "tests.rs",\n',
    '        "quality.rs",\n        "test_inventory.rs",\n        "tests.rs",\n',
)
policy_tests = '''#[test]
fn test_shared_inventory_is_collected_once_for_tests_and_quality() -> Result<()> {
    let calls = std::cell::Cell::new(0usize);

    let inventory = collect_shared_test_inventory(true, true, || {
        calls.set(calls.get() + 1);
        Ok(test_inventory::WorkspaceLibTestInventory::default())
    })?;

    assert!(inventory.is_some());
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[test]
fn test_tests_only_degrades_when_shared_inventory_is_unavailable() -> Result<()> {
    let inventory = collect_shared_test_inventory(true, false, || {
        Err(eyre!("simulated discovery failure"))
    })?;

    assert!(inventory.is_none());
    Ok(())
}

#[test]
fn test_quality_fails_closed_when_shared_inventory_is_unavailable() {
    let result = collect_shared_test_inventory(false, true, || {
        Err(eyre!("simulated discovery failure"))
    });

    assert!(result.is_err());
}

#[test]
fn test_unrelated_subsystem_does_not_collect_shared_inventory() -> Result<()> {
    let calls = std::cell::Cell::new(0usize);

    let inventory = collect_shared_test_inventory(false, false, || {
        calls.set(calls.get() + 1);
        Ok(test_inventory::WorkspaceLibTestInventory::default())
    })?;

    assert!(inventory.is_none());
    assert_eq!(calls.get(), 0);
    Ok(())
}

'''
replace_once(
    mod_tests,
    "#[test]\nfn test_parser_status_refreshes_accuracy_artifact_before_rendering() -> Result<()> {\n",
    policy_tests
    + "#[test]\nfn test_parser_status_refreshes_accuracy_artifact_before_rendering() -> Result<()> {\n",
)
