//! The assertion-call → `oracle.kind`/`oracle.strength` lookup table.
//!
//! Split out from test-file discovery ([`super::test_facts`]) because it is a
//! pure, stateless recipe: [`oracle_for`] is looked up once per call node
//! `super::test_facts::emit_tests_and_oracles` visits.

/// Assertion / exception-observer / warning-observer call names → ripr
/// `oracle.kind` + `oracle.strength`. Matched against the **real callee name of
/// a parsed call node** (via `extract_symbol_refs`), so `isa_ok` never counts as
/// `is`, and names inside comments or strings never match. `diag`/`note`/`plan`/
/// `done_testing`/`subtest` are intentionally absent — they are diagnostics or
/// test structure, not behavioral oracles.
pub(crate) const ASSERTION_ORACLES: &[(&str, &str, &str)] = &[
    // (call_name, oracle_kind, oracle_strength)
    // Test::More / Test2 comparisons
    ("is", "exact_return_assertion", "strong_exact"),
    ("isnt", "exact_return_assertion", "strong_exact"),
    ("is_deeply", "exact_return_assertion", "strong_exact"),
    ("cmp_ok", "predicate_boundary_assertion", "strong_exact"),
    ("like", "predicate_boundary_assertion", "weak_broad"),
    ("unlike", "predicate_boundary_assertion", "weak_broad"),
    ("isa_ok", "predicate_boundary_assertion", "weak_broad"),
    ("can_ok", "predicate_boundary_assertion", "weak_broad"),
    ("ok", "smoke_ok", "weak_smoke"),
    ("pass", "smoke_ok", "weak_smoke"),
    ("fail", "smoke_ok", "weak_smoke"),
    ("use_ok", "mention_only", "mention_only"),
    ("require_ok", "mention_only", "mention_only"),
    // Test::Exception / Test::Fatal / Test2 exception observers
    ("throws_ok", "exception_observer", "weak_broad"),
    ("dies_ok", "exception_observer", "weak_broad"),
    ("lives_ok", "smoke_ok", "weak_smoke"),
    ("lives_and", "exception_observer", "weak_broad"),
    ("exception", "exception_observer", "weak_broad"),
    ("dies", "exception_observer", "weak_broad"),
    ("lives", "smoke_ok", "weak_smoke"),
    // Test::Warn observers (commonly paired with the above)
    ("warning_is", "warn_observer", "weak_broad"),
    ("warning_like", "warn_observer", "weak_broad"),
    ("warnings_are", "warn_observer", "weak_broad"),
];

/// Look up a call name in [`ASSERTION_ORACLES`], returning `(kind, strength)`.
pub(crate) fn oracle_for(name: &str) -> Option<(&'static str, &'static str)> {
    ASSERTION_ORACLES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, kind, strength)| (*kind, *strength))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_for_maps_known_assertions_only() {
        // Independent contract list — deliberately NOT derived from ASSERTION_ORACLES,
        // so a rename or kind/strength drift in the table (e.g. `is_deeply` →
        // `is-deeply`) breaks this test instead of silently dropping coverage.
        let expected: &[(&str, &str, &str)] = &[
            ("is", "exact_return_assertion", "strong_exact"),
            ("isnt", "exact_return_assertion", "strong_exact"),
            ("is_deeply", "exact_return_assertion", "strong_exact"),
            ("cmp_ok", "predicate_boundary_assertion", "strong_exact"),
            ("like", "predicate_boundary_assertion", "weak_broad"),
            ("unlike", "predicate_boundary_assertion", "weak_broad"),
            ("isa_ok", "predicate_boundary_assertion", "weak_broad"),
            ("can_ok", "predicate_boundary_assertion", "weak_broad"),
            ("ok", "smoke_ok", "weak_smoke"),
            ("pass", "smoke_ok", "weak_smoke"),
            ("fail", "smoke_ok", "weak_smoke"),
            ("use_ok", "mention_only", "mention_only"),
            ("require_ok", "mention_only", "mention_only"),
            ("throws_ok", "exception_observer", "weak_broad"),
            ("dies_ok", "exception_observer", "weak_broad"),
            ("lives_ok", "smoke_ok", "weak_smoke"),
            ("lives_and", "exception_observer", "weak_broad"),
            ("exception", "exception_observer", "weak_broad"),
            ("dies", "exception_observer", "weak_broad"),
            ("lives", "smoke_ok", "weak_smoke"),
            ("warning_is", "warn_observer", "weak_broad"),
            ("warning_like", "warn_observer", "weak_broad"),
            ("warnings_are", "warn_observer", "weak_broad"),
        ];
        for (name, kind, strength) in expected {
            assert_eq!(
                oracle_for(name),
                Some((*kind, *strength)),
                "{name} must map to ({kind}, {strength})"
            );
        }
        // Length guard: a new table entry that isn't added to `expected` above
        // fails here, forcing coverage to track the table.
        assert_eq!(
            ASSERTION_ORACLES.len(),
            expected.len(),
            "every ASSERTION_ORACLES entry must have a contract assertion above"
        );
        // Non-assertions never map — no diagnostics, no arbitrary calls.
        assert!(oracle_for("diag").is_none(), "diag is a diagnostic, not an oracle");
        assert!(oracle_for("note").is_none(), "note is a diagnostic, not an oracle");
        assert!(oracle_for("not_an_assertion").is_none());
    }
}
