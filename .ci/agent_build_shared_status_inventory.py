#!/usr/bin/env python3
"""Apply the bounded shared status-inventory candidate transformation."""

from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"{path}: expected one replacement, found {count}: {old[:80]!r}"
        )
    path.write_text(text.replace(old, new), encoding="utf-8")


def main() -> None:
    quality = Path("xtask/src/tasks/update_status/quality.rs")
    replace_once(
        quality,
        "//! Owns per-crate mutation and lib-test counts; delegates UX receipt generation\n",
        "//! Owns per-crate mutation and quality rendering; consumes the shared lib-test inventory\n",
    )
    replace_once(
        quality,
        "#[derive(Debug, Default, PartialEq, Eq)]\nstruct PerCrateTestCounts {\n    by_crate: BTreeMap<String, usize>,\n    unattributed: usize,\n}\n",
        "#[derive(Debug, Default, PartialEq, Eq)]\npub(super) struct PerCrateTestCounts {\n    pub(super) by_crate: BTreeMap<String, usize>,\n    pub(super) unattributed: usize,\n}\n\nimpl PerCrateTestCounts {\n    pub(super) fn total(&self) -> usize {\n        self.by_crate.values().sum::<usize>() + self.unattributed\n    }\n}\n",
    )
    replace_once(
        quality,
        "fn collect_per_crate_test_counts(root: &Path) -> Result<PerCrateTestCounts> {\n",
        "pub(super) fn collect_per_crate_test_counts(root: &Path) -> Result<PerCrateTestCounts> {\n",
    )
    replace_once(
        quality,
        "        Duration::from_mins(3),\n",
        "        // A cold cache-targets=false runner compiles the workspace before listing.\n        // Keep the command bounded, but give the single shared discovery enough headroom.\n        Duration::from_mins(12),\n",
    )
    replace_once(
        quality,
        "    if counts.by_crate.values().sum::<usize>() + counts.unattributed == 0 {\n",
        "    if counts.total() == 0 {\n",
    )
    replace_once(
        quality,
        "pub(super) fn generate_quality_status(root: &Path, original: &str) -> Result<String> {\n    let mutation_by_crate = collect_per_crate_mutation(root);\n    let tests_by_crate = collect_per_crate_test_counts(root)?;\n",
        "pub(super) fn generate_quality_status(\n    root: &Path,\n    original: &str,\n    tests_by_crate: &PerCrateTestCounts,\n) -> Result<String> {\n    let mutation_by_crate = collect_per_crate_mutation(root);\n",
    )
    replace_once(
        quality,
        "    let crate_table = format_crate_quality_table(&mutation_by_crate, &tests_by_crate);\n",
        "    let crate_table = format_crate_quality_table(&mutation_by_crate, tests_by_crate);\n",
    )

    tests = Path("xtask/src/tasks/update_status/tests.rs")
    replace_once(tests, "use regex::Regex;\n\n", "")
    replace_once(
        tests,
        "use super::{replace_block, run_cmd};\n",
        "use super::quality::PerCrateTestCounts;\nuse super::{replace_block, run_cmd};\n",
    )
    replace_once(
        tests,
        '''pub(super) fn count_tier_a_lib_tests(root: &Path) -> Option<usize> {
    let output = run_cmd(
        root,
        &["cargo", "test", "--workspace", "--lib", "--exclude", "tree-sitter-perl", "--", "--list"],
        Duration::from_mins(3),
    );
    if output.is_empty() {
        return None;
    }
    let re = Regex::new(r":\s*test\s*$").ok()?;
    Some(output.lines().filter(|line| re.is_match(line)).count())
}

''',
        "",
    )
    replace_once(
        tests,
        "pub(super) fn count_tests(root: &Path) -> TestCounts {\n    let tier_a = count_tier_a_lib_tests(root);\n",
        "pub(super) fn count_tests(\n    root: &Path,\n    test_inventory: Option<&PerCrateTestCounts>,\n) -> TestCounts {\n    let tier_a = test_inventory.map(PerCrateTestCounts::total);\n",
    )
    replace_once(
        tests,
        "    #[test]\n    fn generate_tests_status_handles_zero_discovery_gracefully() -> Result<()> {\n",
        '''    #[test]
    fn count_tests_reuses_the_shared_inventory_total() -> Result<()> {
        let dir = tempfile::tempdir()?;
        fs::create_dir_all(dir.path().join("crates"))?;
        let inventory = PerCrateTestCounts {
            by_crate: std::collections::BTreeMap::from([
                ("perl-parser".to_string(), 3),
                ("perl-lsp-rs".to_string(), 4),
            ]),
            unattributed: 2,
        };

        let counts = count_tests(dir.path(), Some(&inventory));
        assert_eq!(counts.tier_a_lib_tests, Some(9));
        Ok(())
    }

    #[test]
    fn generate_tests_status_handles_zero_discovery_gracefully() -> Result<()> {
''',
    )

    quality_tests = Path("xtask/src/tasks/update_status/quality/tests.rs")
    replace_once(
        quality_tests,
        "#[test]\nfn test_validate_per_crate_test_counts_rejects_zero_discovery() -> Result<()> {\n",
        '''#[test]
fn test_per_crate_test_counts_total_includes_unattributed_tests() {
    let counts = PerCrateTestCounts {
        by_crate: BTreeMap::from([
            (String::from("perl-parser"), 3),
            (String::from("perl-lsp-rs"), 4),
        ]),
        unattributed: 2,
    };
    assert_eq!(counts.total(), 9);
}

#[test]
fn test_validate_per_crate_test_counts_rejects_zero_discovery() -> Result<()> {
''',
    )

    module = Path("xtask/src/tasks/update_status/mod.rs")
    replace_once(
        module,
        "    // --- Tests subsystem ---\n    if need_tests {\n",
        '''    // One compiled workspace-lib inventory owns both the Tier-A total and the
    // per-crate quality table. Full/quality runs fail closed; tests-only keeps
    // its existing UNVERIFIED rendering when the bounded discovery is unavailable.
    let test_inventory = if need_quality {
        Some(run_subsystem(
            "test-inventory",
            "cargo xtask update-status --write --only quality",
            || quality::collect_per_crate_test_counts(&root),
        )?)
    } else if need_tests {
        quality::collect_per_crate_test_counts(&root).ok()
    } else {
        None
    };

    // --- Tests subsystem ---
    if need_tests {
''',
    )
    replace_once(
        module,
        "            let test_counts = tests::count_tests(&root);\n",
        "            let test_counts = tests::count_tests(&root, test_inventory.as_ref());\n",
    )
    replace_once(
        module,
        "            let updated_quality = quality::generate_quality_status(&root, &original_quality)?;\n",
        '''            let inventory = test_inventory
                .as_ref()
                .ok_or_else(|| eyre!("quality test inventory missing after required discovery"))?;
            let updated_quality =
                quality::generate_quality_status(&root, &original_quality, inventory)?;
''',
    )


if __name__ == "__main__":
    main()
