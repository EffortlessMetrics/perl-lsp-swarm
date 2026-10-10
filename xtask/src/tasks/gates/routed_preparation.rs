//! The routed runtime consumes preparation results from this invocation (#17479).
//!
//! This is the narrow composition of the existing routed gate identities, not a
//! new command registry, receipt format, or subprocess supervisor. Compile-only
//! preparation keeps its existing command and watchdog; DAP repository checks
//! have their own existing-runner result instead of executing inside libtest.

pub(super) const RUNTIME: &str = "unit_routed_full";
pub(super) const BUILD: &str = "unit_routed_full_build";
pub(super) const DAP: &str = "dap_workspace_prepare";

/// The canonical planner renders package arguments as separate -p/name pairs.
pub(super) fn includes_dap(package_args: &[String]) -> bool {
    package_args
        .windows(2)
        .any(|pair| matches!(pair[0].as_str(), "-p" | "--package") && pair[1] == "perl-dap")
}

pub(super) fn prerequisites(gate: &str, dap_selected: bool) -> &'static [&'static str] {
    match gate {
        RUNTIME if dap_selected => &["fmt", BUILD, DAP],
        RUNTIME => &[BUILD],
        DAP => &["fmt"],
        _ => &[],
    }
}

/// Named runtime execution retains the canonical selection of its prerequisites.
/// An inapplicable runtime does not turn a named filter into a workspace build.
pub(super) fn retain_for_filter(
    selected: &[&str],
    requested: &str,
    dap_selected: bool,
) -> Vec<String> {
    if !selected.contains(&requested) {
        return Vec::new();
    }
    selected
        .iter()
        .filter(|name| **name == requested || prerequisites(requested, dap_selected).contains(name))
        .map(|name| (*name).to_string())
        .collect()
}

/// Only a terminal pass from this runner's earlier result population qualifies.
/// A failed hosted warm-up is preserved separately and cannot be retried away by
/// the later tier invocation. An absent hosted outcome is legitimate for local
/// callers, which must still execute their own selected prerequisites.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct PreparationBlock {
    pub reason: String,
    pub dependency: String,
    /// None means the required terminal observation is missing.
    pub status: Option<String>,
}

#[cfg(test)]
pub(super) fn blocking_reason(
    gate: &str,
    dap_selected: bool,
    results: &[(&str, &str)],
    hosted_outcome: Option<&str>,
) -> Option<String> {
    blocking_failure(gate, dap_selected, results, hosted_outcome).map(|block| block.reason)
}

pub(super) fn blocking_failure(
    gate: &str,
    dap_selected: bool,
    results: &[(&str, &str)],
    hosted_outcome: Option<&str>,
) -> Option<PreparationBlock> {
    if matches!(gate, BUILD | RUNTIME)
        && let Some(outcome) = hosted_outcome
        && outcome != "success"
    {
        return Some(PreparationBlock {
            reason: format!(
                "not run: hosted routed preparation ended '{outcome}'; see routed-test-build/receipt.json"
            ),
            dependency: "hosted_routed_test_preparation".into(),
            status: Some(outcome.into()),
        });
    }
    for prerequisite in prerequisites(gate, dap_selected) {
        let mut matching = results.iter().filter(|(name, _)| name == prerequisite);
        let terminal = matching.next();
        if matching.next().is_some() {
            return Some(PreparationBlock {
                reason: format!(
                    "not run: prerequisite '{prerequisite}' has duplicate terminal results"
                ),
                dependency: (*prerequisite).into(),
                status: Some("duplicate terminal results".into()),
            });
        }
        match terminal {
            Some((_, "pass")) => {}
            Some((_, status)) => {
                return Some(PreparationBlock {
                    reason: format!("not run: prerequisite '{prerequisite}' ended '{status}'"),
                    dependency: (*prerequisite).into(),
                    status: Some((*status).into()),
                });
            }
            None => {
                return Some(PreparationBlock {
                    reason: format!(
                        "not run: prerequisite '{prerequisite}' has no terminal result in this invocation"
                    ),
                    dependency: (*prerequisite).into(),
                    status: None,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dap_selection_uses_exact_package_arguments() {
        for flag in ["-p", "--package"] {
            assert!(includes_dap(&[flag.into(), "perl-dap".into()]));
        }
        assert!(!includes_dap(&["-p".into(), "perl-dap-platform".into()]));
        assert!(!includes_dap(&["perl-dap".into()]));
        assert!(!includes_dap(&[]));
    }

    #[test]
    fn named_dap_runtime_keeps_preparation_in_canonical_order() {
        let names = ["fmt", "other", BUILD, DAP, RUNTIME];
        assert_eq!(retain_for_filter(&names, RUNTIME, true), ["fmt", BUILD, DAP, RUNTIME]);
        assert_eq!(retain_for_filter(&names, RUNTIME, false), [BUILD, RUNTIME]);
    }

    #[test]
    fn harness_warmup_does_not_duplicate_dap_operations() {
        assert_eq!(retain_for_filter(&["fmt", BUILD, DAP, RUNTIME], BUILD, true), [BUILD]);
        assert!(prerequisites(BUILD, true).is_empty());
    }

    #[test]
    fn named_dap_preparation_reuses_formatter() {
        assert_eq!(retain_for_filter(&["fmt", BUILD, DAP, RUNTIME], DAP, true), ["fmt", DAP]);
    }

    #[test]
    fn inapplicable_runtime_does_not_select_preparation() {
        assert!(retain_for_filter(&["fmt", DAP], RUNTIME, true).is_empty());
    }

    #[test]
    fn successful_preparation_allows_runtime_but_is_not_its_result() {
        assert!(
            blocking_reason(
                RUNTIME,
                true,
                &[("fmt", "pass"), (BUILD, "pass"), (DAP, "pass")],
                None
            )
            .is_none()
        );
        // Runtime execution/metrics remain owned by run_single_gate afterwards.
        assert!(blocking_reason("independent_backstop", true, &[], None).is_none());
    }

    #[test]
    fn every_nonpass_preparation_status_blocks_runtime() {
        for prerequisite in ["fmt", BUILD, DAP] {
            for status in ["fail", "timeout", "error", "skip", "", "unknown"] {
                let mut results = [("fmt", "pass"), (BUILD, "pass"), (DAP, "pass")];
                for row in &mut results {
                    if row.0 == prerequisite {
                        row.1 = status;
                    }
                }
                let reason = blocking_reason(RUNTIME, true, &results, None);
                assert!(reason.is_some_and(|value| value.contains(prerequisite) && value.contains(status)));
            }
        }
    }

    #[test]
    fn missing_or_foreign_terminal_results_never_qualify() {
        for missing in ["fmt", BUILD, DAP] {
            let results = [("fmt", "pass"), (BUILD, "pass"), (DAP, "pass")];
            let retained: Vec<_> = results.into_iter().filter(|row| row.0 != missing).collect();
            assert!(
                blocking_reason(RUNTIME, true, &retained, None)
                    .is_some_and(|reason| reason.contains("no terminal result"))
            );
        }
        assert!(blocking_reason(RUNTIME, false, &[("foreign_build", "pass")], None).is_some());
        assert!(
            blocking_reason(RUNTIME, false, &[(BUILD, "pass"), (BUILD, "fail")], None)
                .is_some_and(|reason| reason.contains("duplicate terminal results"))
        );
    }

    #[test]
    fn non_dap_runtime_does_not_require_dap_or_formatter() {
        assert!(blocking_reason(RUNTIME, false, &[(BUILD, "pass")], None).is_none());
    }

    #[test]
    fn dap_preparation_cannot_erase_formatter_failure() {
        assert!(blocking_reason(DAP, true, &[("fmt", "fail")], None).is_some());
        assert!(blocking_reason(DAP, true, &[], None).is_some());
    }

    #[test]
    fn hosted_failure_cannot_be_retried_away_in_later_tier() {
        for status in ["failure", "cancelled", "skipped", "unknown", ""] {
            for gate in [BUILD, RUNTIME] {
                assert!(
                    blocking_reason(gate, false, &[(BUILD, "pass")], Some(status))
                        .is_some_and(|reason| reason.contains("hosted routed preparation"))
                );
            }
        }
        assert!(blocking_reason(RUNTIME, false, &[(BUILD, "pass")], Some("success")).is_none());
    }

    #[test]
    fn independent_backstops_ignore_hosted_preparation_failure() {
        assert!(blocking_reason("unit_dap_support_full", true, &[], Some("failure")).is_none());
    }
}
