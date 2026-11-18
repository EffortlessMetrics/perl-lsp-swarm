//! Cargo's own test-target census (#17151).
//!
//! `cargo test` enumerates the test targets it *built* as `Executable` lines and
//! the test targets it *executed* as `Running` lines. Both censuses are cargo's
//! own output, so this module never re-implements cargo's target selection
//! rules — it only reads what cargo reported.
//!
//! The difference between the two censuses is the population a sweep silently
//! skipped. Cargo exits after the first failing test executable unless
//! `--no-fail-fast` is set, so the built and executed censuses diverge exactly
//! when the sweep was truncated: the compile-only sibling built every selected
//! target, and the runtime sweep stopped at the first failure.
//!
//! This module is the single owner of that reading. The gate runner publishes
//! the accounting in its receipt, and the routed-sweep fixture control asserts
//! against real cargo output through the same code path.

use std::collections::BTreeSet;
use std::io::BufRead;

use serde::{Deserialize, Serialize};

/// One test-target report cargo emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetReport {
    /// The target cargo named: `unittests src/lib.rs` for a unit-test target
    /// and `tests/<name>.rs` for an integration target.
    pub target: String,
    /// True for cargo's `Executable` (built) report, false for its `Running`
    /// (executed) report.
    pub built: bool,
    /// True when the target is an integration (`tests/…`) target.
    pub integration: bool,
}

/// The two populations cargo reports for one `cargo test` invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetCensus {
    built_targets: BTreeSet<String>,
    executed_targets: BTreeSet<String>,
    /// Test targets cargo reported as built (`Executable` lines).
    pub built: u32,
    /// Test targets cargo reported as executed (`Running` lines).
    pub executed: u32,
    /// Integration (`tests/…`) subset of `built`.
    pub built_integration: u32,
    /// Integration (`tests/…`) subset of `executed`.
    pub executed_integration: u32,
}

impl TargetCensus {
    /// True when cargo reported no test target at all.
    pub fn is_empty(&self) -> bool {
        self.built == 0 && self.executed == 0
    }
}

/// Complete accounting of the test targets one routed sweep selected.
///
/// `selected` comes from the compile-only sibling's `Executable` census and
/// `executed` from the runtime sweep's `Running` census, so `skipped` counts
/// the targets that were built for this sweep and never ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedTargetAccounting {
    /// Test targets the compile-only sibling built for this sweep.
    pub selected: u32,
    /// Test targets the runtime sweep actually executed.
    pub executed: u32,
    /// Selected targets that were built but never executed.
    pub skipped: u32,
    /// Integration (`tests/…`) subset of `selected`.
    pub selected_integration: u32,
    /// Integration (`tests/…`) subset of `executed`.
    pub executed_integration: u32,
    /// True when every selected target was executed.
    pub complete: bool,
}

impl SelectedTargetAccounting {
    /// One-sentence receipt summary naming the accounted populations.
    pub fn summary(&self) -> String {
        format!(
            "selected {} test target(s) ({} integration), executed {} ({} integration), \
             skipped {}",
            self.selected,
            self.selected_integration,
            self.executed,
            self.executed_integration,
            self.skipped
        )
    }
}

/// Read the test-target census out of a cargo log held in memory.
///
/// Malformed target markers invalidate the census and return an empty result.
/// Production receipt accounting uses the fallible streaming scanner directly
/// so invalid evidence remains distinguishable from a valid empty log.
pub fn census_test_targets(log: &str) -> TargetCensus {
    census_test_targets_from_reader(log.as_bytes()).unwrap_or_default()
}

/// Read the test-target census by streaming a cargo log.
///
/// Gate logs can contain arbitrary subprocess bytes. Invalid UTF-8 or an I/O
/// failure is an error
/// rather than a partial census, because a truncated count would misreport the
/// skipped population.
pub fn census_test_targets_from_reader<R: BufRead>(
    reader: R,
) -> Result<TargetCensus, std::io::Error> {
    let mut census = TargetCensus::default();
    for line in reader.lines() {
        match line {
            Ok(line) => {
                if let Some((built, _, integration)) = classify_cargo_target_line(&line) {
                    let plain = console::strip_ansi_codes(&line);
                    let identity = plain
                        .trim()
                        .split_once(' ')
                        .map(|(_, rest)| rest.trim().replace('\\', "/"))
                        .unwrap_or_default();
                    let fresh = if built {
                        census.built_targets.insert(identity)
                    } else {
                        census.executed_targets.insert(identity)
                    };
                    if !fresh {
                        continue;
                    }
                    if built {
                        census.built = census.built.saturating_add(1);
                        if integration {
                            census.built_integration = census.built_integration.saturating_add(1);
                        }
                    } else {
                        census.executed = census.executed.saturating_add(1);
                        if integration {
                            census.executed_integration =
                                census.executed_integration.saturating_add(1);
                        }
                    }
                } else {
                    let plain = console::strip_ansi_codes(&line);
                    let marker = plain
                        .trim()
                        .strip_prefix("Executable ")
                        .or_else(|| plain.trim().strip_prefix("Running "));
                    if marker.is_some_and(|rest| {
                        let normalized = rest.replace('\\', "/");
                        normalized.starts_with("tests/")
                            || normalized.starts_with("unittests ")
                            || normalized.contains(".rs")
                    }) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "malformed cargo target marker",
                        ));
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(census)
}

/// The individual test-target reports cargo emitted, in report order.
pub fn test_target_reports(log: &str) -> Vec<TargetReport> {
    log.lines()
        .filter_map(|line| {
            classify_cargo_target_line(line).map(|(built, target, integration)| TargetReport {
                target,
                built,
                integration,
            })
        })
        .collect()
}

/// Account the selected test targets of one sweep.
///
/// `build` is the compile-only sibling's census (cargo's `Executable` lines)
/// and `run` is the runtime sweep's census (cargo's `Running` lines). `None`
/// means the compile census named no target, so there is nothing to account
/// against and no accounting may be published.
pub fn account_selected_targets(
    build: TargetCensus,
    run: TargetCensus,
) -> Option<SelectedTargetAccounting> {
    if build.built == 0 {
        return None;
    }
    let selected = build.built;
    let executed = run.executed;
    Some(SelectedTargetAccounting {
        selected,
        executed,
        skipped: u32::try_from(build.built_targets.difference(&run.executed_targets).count())
            .ok()?,
        selected_integration: build.built_integration,
        executed_integration: run.executed_integration,
        complete: build.built_targets == run.executed_targets,
    })
}

/// Classify one cargo log line as a test-target marker.
///
/// Returns `(is_build_marker, target, is_integration)` for cargo's own
/// `Executable <target> (<binary>)` and `Running <target> (<binary>)` lines.
///
/// Cargo always terminates the marker with the binary path in parentheses, so
/// requiring that keeps a test's own stdout from being counted as a target.
/// The target spec is `unittests <source>` for a unit-test target and the
/// source path for an integration target (`tests/<name>.rs`).
///
/// The reported spec is normalized to forward slashes: cargo prints the host
/// separator, so an unnormalized spec would classify integration targets
/// differently on Windows than on the Linux hosts that produce the receipts.
fn classify_cargo_target_line(line: &str) -> Option<(bool, String, bool)> {
    let plain = console::strip_ansi_codes(line);
    let trimmed = plain.trim();
    let (rest, built) = if let Some(rest) = trimmed.strip_prefix("Executable ") {
        (rest, true)
    } else {
        (trimmed.strip_prefix("Running ")?, false)
    };
    if !rest.trim_end().ends_with(')') {
        return None;
    }
    // A unique source-valid delimiter leaves the executable path opaque:
    // literal unmatched parentheses are legal path bytes. If multiple source
    // boundaries are possible, balanced outer grouping may disambiguate them;
    // otherwise refuse rather than invent a source identity.
    let candidates: Vec<_> = rest
        .match_indices(" (")
        .filter_map(|(index, _)| {
            (rest[..index].trim_end().ends_with(".rs")
                && !rest[index + 2..rest.len() - 1].trim().is_empty())
            .then_some(index)
        })
        .collect();
    let spec_end = if candidates.len() == 1 {
        candidates[0]
    } else {
        let mut balanced = candidates.into_iter().filter(|&index| {
            let suffix = &rest[index + 1..];
            let mut depth = 0usize;
            for (offset, ch) in suffix.char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        let Some(next) = depth.checked_sub(1) else {
                            return false;
                        };
                        depth = next;
                        if depth == 0 && offset + 1 != suffix.len() {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            depth == 0
        });
        let index = balanced.next()?;
        if balanced.next().is_some() {
            return None;
        }
        index
    };
    let spec = rest[..spec_end].trim().replace('\\', "/");
    let first = spec.split_whitespace().next()?;
    let integration = if spec.starts_with("tests/") && spec.ends_with(".rs") {
        true
    } else if first == "unittests" && spec.ends_with(".rs") {
        false
    } else if spec.ends_with(".rs") {
        // A test target outside `tests/` (an explicit `[[test]]` path). It is
        // a real selected target, so it counts in the totals, but it is not
        // part of the `tests/` integration population.
        false
    } else {
        return None;
    };
    Some((built, spec, integration))
}

#[cfg(test)]
mod tests {
    use perl_tdd_support::{must_some_with, must_with};

    use super::*;

    const BUILD_LOG: &str = "\
   Compiling xtask v0.1.0 (/repo/xtask)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.02s
  Executable unittests src/lib.rs (target/debug/deps/xtask-1111111111111111)
  Executable unittests src/main.rs (target/debug/deps/xtask-2222222222222222)
  Executable tests/routed_test_preparation_contract.rs (target/debug/deps/routed-3333333333333333)
  Executable tests/gate_policy_profile_tests.rs (target/debug/deps/gate-4444444444444444)
";

    const RUN_LOG_COMPLETE: &str = "\
     Running unittests src/lib.rs (target/debug/deps/xtask-1111111111111111)
     Running unittests src/main.rs (target/debug/deps/xtask-2222222222222222)
     Running tests/routed_test_preparation_contract.rs (target/debug/deps/routed-3333333333333333)
     Running tests/gate_policy_profile_tests.rs (target/debug/deps/gate-4444444444444444)
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
";

    const RUN_LOG_ABORTED: &str = "\
     Running unittests src/lib.rs (target/debug/deps/xtask-1111111111111111)
     Running unittests src/main.rs (target/debug/deps/xtask-2222222222222222)
test result: FAILED. 4994 passed; 14 failed; 1 ignored; 0 measured; 0 filtered out; finished in 61.34s
error: test failed, to rerun pass `-p xtask --bin xtask`
";

    #[test]
    fn colored_markers_match_real_cargo_and_reject_malformed_paths() {
        let colored = BUILD_LOG.replace("Executable", "\x1b[1m\x1b[92mExecutable\x1b[0m");
        assert_eq!(census_test_targets(&colored), census_test_targets(BUILD_LOG));
        let colored_run = RUN_LOG_COMPLETE.replace("Running", "\x1b[1m\x1b[92mRunning\x1b[0m");
        assert_eq!(census_test_targets(&colored_run), census_test_targets(RUN_LOG_COMPLETE));
        assert!(
            census_test_targets(
                "Running tests/fake.rs)\nRunning tests/fake.rs ()\nRunning unittests (binary)\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn equal_counts_do_not_certify_different_targets_and_duplicates_do_not_hide_skips() {
        let build = "Executable tests/a.rs (target/a)\nExecutable tests/b.rs (target/b)\n";
        let wrong = "Running tests/a.rs (target/a)\nRunning tests/c.rs (target/c)\n";
        let accounting = must_some_with(
            account_selected_targets(census_test_targets(build), census_test_targets(wrong)),
            "nonempty build",
        );
        assert!(!accounting.complete);
        assert_eq!(accounting.skipped, 1);
        let duplicated = "Running tests/a.rs (target/a)\nRunning tests/a.rs (target/a)\n";
        let accounting = must_some_with(
            account_selected_targets(census_test_targets(build), census_test_targets(duplicated)),
            "nonempty build",
        );
        assert_eq!(accounting.executed, 1);
        assert_eq!(accounting.skipped, 1);
    }

    #[test]
    fn ordinary_running_noise_and_parenthesized_binary_paths_are_supported() {
        let build = census_test_targets("Executable tests/a (local).rs (target (local)/a)\n");
        let run = census_test_targets(
            "Running fixture setup\nRunning tests/a (local).rs (target (local)/a)\n",
        );
        let accounting =
            must_some_with(account_selected_targets(build, run), "valid paired targets");
        assert!(accounting.complete);
        assert_eq!(accounting.executed, 1);
    }

    #[test]
    fn source_and_binary_rs_parentheses_have_an_unambiguous_boundary() {
        let source = "tests/a.rs (local).rs";
        let build = format!("Executable {source} (target/a.rs (local))\n");
        let reports = test_target_reports(&build);
        assert_eq!(reports[0].target, source);
        assert!(
            account_selected_targets(
                census_test_targets(&build),
                census_test_targets(&build.replace("Executable", "Running"))
            )
            .is_some_and(|a| a.complete)
        );
        assert!(
            census_test_targets_from_reader(b"Running tests/a.rs (target/(broken)\n".as_slice())
                .is_ok()
        );
        assert!(
            census_test_targets_from_reader(
                b"Running tests/a.rs (target/literal)path)\n".as_slice()
            )
            .is_ok()
        );
        assert!(
            census_test_targets_from_reader(b"Running tests/a.rs (target/missing\n".as_slice())
                .is_err()
        );
    }

    #[test]
    fn mixed_valid_and_malformed_markers_cannot_certify_a_partial_census() {
        for marker in ["Executable", "Running"] {
            let log = format!("{marker} tests/a.rs (target/a)\n{marker} tests/b.rs ()\n");
            assert!(census_test_targets_from_reader(log.as_bytes()).is_err());
        }
    }

    #[test]
    fn undecodable_logs_cannot_certify_a_partial_census() {
        assert!(
            census_test_targets_from_reader(&b"Executable tests/a.rs (target/a)\n\xff\n"[..])
                .is_err()
        );
    }

    #[test]
    fn build_census_counts_every_executable_target() {
        let census = census_test_targets(BUILD_LOG);
        assert_eq!(census.built, 4);
        assert_eq!(census.built_integration, 2);
        assert_eq!(census.executed, 0);
        assert_eq!(census.executed_integration, 0);
    }

    #[test]
    fn complete_sweep_accounts_for_every_selected_target() {
        let accounting = must_some_with(
            account_selected_targets(
                census_test_targets(BUILD_LOG),
                census_test_targets(RUN_LOG_COMPLETE),
            ),
            "a compile census names targets, so an accounting exists",
        );
        assert_eq!(accounting.selected, 4);
        assert_eq!(accounting.executed, 4);
        assert_eq!(accounting.skipped, 0);
        assert_eq!(accounting.selected_integration, 2);
        assert_eq!(accounting.executed_integration, 2);
        assert!(accounting.complete);
    }

    #[test]
    fn aborted_sweep_names_the_skipped_population() {
        let accounting = must_some_with(
            account_selected_targets(
                census_test_targets(BUILD_LOG),
                census_test_targets(RUN_LOG_ABORTED),
            ),
            "a compile census names targets, so an accounting exists",
        );
        assert_eq!(accounting.selected, 4);
        assert_eq!(accounting.executed, 2);
        assert_eq!(accounting.skipped, 2);
        assert_eq!(accounting.selected_integration, 2);
        assert_eq!(accounting.executed_integration, 0);
        assert!(!accounting.complete);
        assert!(
            accounting.summary().contains("skipped 2"),
            "the receipt summary must name the skipped population: {}",
            accounting.summary()
        );
    }

    #[test]
    fn no_compile_census_means_no_accounting() {
        assert!(
            account_selected_targets(
                census_test_targets(RUN_LOG_COMPLETE),
                census_test_targets(RUN_LOG_COMPLETE)
            )
            .is_none(),
            "a runtime log carries no `Executable` census, so it cannot select targets"
        );
    }

    #[test]
    fn unrelated_output_is_not_counted_as_a_target() {
        let census = census_test_targets(
            "\
   Compiling xtask v0.1.0 (/repo/xtask)
running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
   Doc-tests xtask
",
        );
        assert!(census.is_empty(), "census must ignore non-marker output: {census:?}");
    }

    #[test]
    fn a_test_printing_a_marker_shaped_line_is_not_counted() {
        // The marker must end in the parenthesized binary path; a test's own
        // stdout that merely starts with `Running ` does not qualify.
        let census = census_test_targets("     Running the thing\n");
        assert_eq!(census.executed, 0);
    }

    #[test]
    fn streaming_census_matches_the_in_memory_census() {
        let streamed =
            must_with(census_test_targets_from_reader(BUILD_LOG.as_bytes()), "readable log");
        assert_eq!(streamed, census_test_targets(BUILD_LOG));
    }

    #[test]
    fn reports_name_the_targets_cargo_ran() {
        let reports = test_target_reports(RUN_LOG_COMPLETE);
        assert_eq!(reports.len(), 4);
        assert!(reports.iter().all(|report| !report.built));
        assert_eq!(reports[2].target, "tests/routed_test_preparation_contract.rs");
        assert!(reports[2].integration);
        assert!(!reports[0].integration);
        assert_eq!(reports[0].target, "unittests src/lib.rs");
    }

    #[test]
    fn a_windows_reported_target_is_normalized_to_forward_slashes() {
        // Cargo prints the host separator. The receipts are produced on Linux
        // hosts while local runs happen on Windows, so an unnormalized spec
        // would classify the same target differently on the two.
        let census = census_test_targets(
            "  Executable tests\\gate_policy_profile_tests.rs (target\\debug\\deps\\gate-3)\n\
             \x20    Running tests\\gate_policy_profile_tests.rs (target\\debug\\deps\\gate-3)\n",
        );
        assert_eq!(census.built, 1);
        assert_eq!(census.executed, 1);
        assert_eq!(census.built_integration, 1);
        assert_eq!(census.executed_integration, 1);

        let reports = test_target_reports(
            "     Running tests\\gate_policy_profile_tests.rs (target\\debug\\deps\\gate-3)\n",
        );
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].target, "tests/gate_policy_profile_tests.rs");
        assert!(reports[0].integration);
    }
}
