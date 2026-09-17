//! Selected `@INC` claim-boundary guards (#10599).
//!
//! Extends `cargo xtask check-support-claims` rather than adding a second
//! claims system. The current-main defect is unqualified language equivalent to
//! `@INC integration complete` on the live support/status pages. Scenario 14
//! receipts stay required markers so a rewrite cannot drop them.
//!
//! This check does not generate #8479/#7460 identities, change production
//! behavior, or promote M04/M07/provider/exact-process rows.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::fs;

pub const MODULE_RESOLUTION_STATUS: &str = "docs/project/status/module_resolution.md";
pub const SUPPORT_TIERS: &str = "docs/project/status/SUPPORT_TIERS.md";

struct IncClaimGuard {
    file: &'static str,
    forbidden: &'static [&'static str],
    required: &'static [&'static str],
}

/// Exact pre-repair literals that collapsed selected Scenario 14 conformance
/// into complete effective-root authority. Do not quote these on the current
/// support pages; the tests own the stale strings.
const MODULE_RESOLUTION_FORBIDDEN: &[&str] =
    &["@INC integration complete", "Conformance means all consumers agree"];

/// Markers the current support page must keep so the selected rail stays
/// denominator-bound and historical receipts stay discoverable.
const MODULE_RESOLUTION_REQUIRED: &[&str] = &[
    "Selected static @INC consumer rail",
    "not complete effective-root authority",
    "PL701",
    "completion",
    "goto-definition",
    "hover",
    "ux_scenario_14_inc_conformance",
    "#8493",
    "#8506",
    "#8544",
    "invoked-script identity and `Bin`/`RealBin`",
    "non-executing",
    "#9270",
    "#1744",
    "not_proven",
    "2026-05-11",
    "| Contextual resolver authority | not_proven |",
    "| Exact-process | not_proven |",
    "| Provider/product support | not_proven |",
];

const SUPPORT_TIERS_FORBIDDEN: &[&str] =
    &["@INC integration complete", "Conformance means all consumers agree", "full @INC support"];

const SUPPORT_TIERS_REQUIRED: &[&str] = &[
    "Scenario 14",
    "Selected static",
    "PL701",
    "not complete effective-root authority",
    "ux_scenario_14_inc_conformance",
];

const INC_CLAIM_GUARDS: &[IncClaimGuard] = &[
    IncClaimGuard {
        file: MODULE_RESOLUTION_STATUS,
        forbidden: MODULE_RESOLUTION_FORBIDDEN,
        required: MODULE_RESOLUTION_REQUIRED,
    },
    IncClaimGuard {
        file: SUPPORT_TIERS,
        forbidden: SUPPORT_TIERS_FORBIDDEN,
        required: SUPPORT_TIERS_REQUIRED,
    },
];

const SCENARIO_14_CONSUMERS: &[&str] = &["pl701", "completion", "goto-definition", "hover"];

/// Pure form so negative controls can mutate text without touching the tree.
pub(crate) fn inc_claim_guard_violations(file: &str, text: &str) -> Vec<String> {
    let Some(guard) = INC_CLAIM_GUARDS.iter().find(|g| g.file == file) else {
        return vec![format!(
            "INC_GUARD_TABLE: {file:?} is not covered by any selected-@INC claim guard (#10599)"
        )];
    };

    let mut violations = Vec::new();
    for &stale in guard.forbidden {
        if text.contains(stale) {
            violations.push(format!(
                "INC_CLAIM: {} contains {:?} — unqualified complete/@INC language must not \
                 return as current support authority (#10599)",
                guard.file, stale
            ));
        }
    }
    for &marker in guard.required {
        if !text.contains(marker) {
            violations.push(format!(
                "INC_MARKER: {} no longer contains {:?} — selected-rail denominator, \
                 Scenario 14 receipt, or current-claim distinction required by #10599",
                guard.file, marker
            ));
        }
    }
    violations.extend(all_consumers_without_denominator_violations(guard.file, text));
    violations.extend(positive_complete_effective_root_line_violations(guard.file, text));
    violations
}

fn positive_complete_effective_root_line_violations(file: &str, text: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if line.contains("complete effective-root authority") && !line_negates_complete_claim(line)
        {
            violations.push(format!(
                "INC_POLARITY: {file}:{} contains complete effective-root authority without \
                 same-line negation (#10599)",
                idx + 1
            ));
        }
    }
    violations
}

fn line_negates_complete_claim(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "not complete",
        "does **not** claim complete",
        "does not claim complete",
        "not a claim",
        "none of these promote",
        "not current complete",
    ]
    .iter()
    .any(|marker| lower.contains(&marker.to_ascii_lowercase()))
}

fn all_consumers_without_denominator_violations(file: &str, text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("all consumers") {
        return Vec::new();
    }
    let missing: Vec<&str> = SCENARIO_14_CONSUMERS
        .iter()
        .copied()
        .filter(|consumer| !lower.contains(consumer))
        .collect();
    if missing.is_empty() {
        return Vec::new();
    }
    vec![format!(
        "INC_DENOMINATOR: {file} uses \"all consumers\" without naming the Scenario 14 \
         consumer denominator ({}); missing {missing:?} (#10599)",
        SCENARIO_14_CONSUMERS.join(", ")
    )]
}

pub fn check() -> Result<()> {
    let root = project_root()?;
    let mut violations = Vec::new();
    for guard in INC_CLAIM_GUARDS {
        let path = root.join(guard.file);
        let text = fs::read_to_string(&path).with_context(|| {
            format!("failed to read selected-@INC claim surface {}", path.display())
        })?;
        violations.extend(inc_claim_guard_violations(guard.file, &text));
    }
    if violations.is_empty() {
        println!(
            "selected @INC claim boundary OK: {} current support/status surfaces, \
             Scenario 14 receipts required, unqualified complete language forbidden (#10599)",
            INC_CLAIM_GUARDS.len()
        );
        return Ok(());
    }
    for violation in &violations {
        eprintln!("{violation}");
    }
    bail!("selected @INC claim boundary check failed ({} violation(s))", violations.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_integration_complete_heading_is_rejected() {
        let mutated = "## Rail Status — @INC integration complete (2026-05-11)\n";
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, mutated);
        assert!(
            violations
                .iter()
                .any(|v| v.starts_with("INC_CLAIM:") && v.contains("@INC integration complete")),
            "the exact current-main heading must trip the forbidden-literal guard, got: {violations:?}"
        );
    }

    #[test]
    fn all_consumers_agree_without_named_denominator_is_rejected() {
        let mutated =
            "Conformance means all consumers agree — not necessarily that every mode resolves.\n";
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, mutated);
        assert!(
            violations.iter().any(|v| v.contains("Conformance means all consumers agree")),
            "the exact current-main conformance sentence must fail, got: {violations:?}"
        );
        assert!(
            violations.iter().any(|v| v.starts_with("INC_DENOMINATOR:")),
            "\"all consumers\" without PL701/completion/goto-definition/hover must fail, got: {violations:?}"
        );
    }

    #[test]
    fn all_consumers_is_allowed_when_the_four_scenario_14_consumers_are_named() {
        let text = "\
Selected static @INC consumer rail
not complete effective-root authority
PL701 diagnostic, completion, goto-definition, and hover
ux_scenario_14_inc_conformance
#8493 #8506 #8544
invoked-script identity and `Bin`/`RealBin`
non-executing
#9270 #1744
not_proven
2026-05-11
all consumers on this page means those four Scenario 14 consumers
";
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, text);
        assert!(
            !violations
                .iter()
                .any(|v| v.starts_with("INC_CLAIM:") || v.starts_with("INC_DENOMINATOR:")),
            "named four-consumer denominator must be permitted, got: {violations:?}"
        );
    }

    #[test]
    fn dropping_scenario_14_receipt_identity_fails() {
        let text = repaired_module_resolution_fixture()
            .replace("ux_scenario_14_inc_conformance", "some other test");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("ux_scenario_14_inc_conformance")),
            "losing the Scenario 14 harness identity must fail, got: {violations:?}"
        );
    }

    #[test]
    fn exact_process_row_cannot_flip_to_proven_while_other_not_proven_remains() {
        let text = repaired_module_resolution_fixture()
            .replace("| Exact-process | not_proven |", "| Exact-process | proven |");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("| Exact-process | not_proven |")),
            "promoting only the Exact-process row must fail even when not_proven remains elsewhere, got: {violations:?}"
        );
        assert!(
            text.contains("not_proven"),
            "the false-negative fixture must still contain not_proven elsewhere"
        );
    }

    #[test]
    fn complete_effective_root_in_new_words_fails_even_with_disclaimer() {
        let text = format!(
            "{}\nThis rail now provides complete effective-root authority for all Scenario 14 consumers.\n",
            repaired_module_resolution_fixture()
        );
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.starts_with("INC_POLARITY:")),
            "a positive completeness headline must fail even when the negation disclaimer remains, got: {violations:?}"
        );
    }

    #[test]
    fn dropping_8493_or_8506_receipts_fails() {
        for receipt in ["#8493", "#8506"] {
            let text = repaired_module_resolution_fixture().replace(receipt, "");
            let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
            assert!(
                violations.iter().any(|v| v.contains(receipt)),
                "dropping {receipt} must fail, got: {violations:?}"
            );
        }
    }

    #[test]
    fn dropping_historical_closeout_receipts_fails() {
        let text = repaired_module_resolution_fixture().replace("#8544", "");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("#8544")),
            "historical selected-rail closeout receipts must stay discoverable, got: {violations:?}"
        );
    }

    #[test]
    fn findbin_row_without_bin_realbin_limitation_fails() {
        let text = repaired_module_resolution_fixture()
            .replace("invoked-script identity and `Bin`/`RealBin`", "FindBin support");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("Bin`/`RealBin")),
            "FindBin selected-rail copy must keep Bin/RealBin as outside the rail, got: {violations:?}"
        );
    }

    #[test]
    fn omitting_dynamic_hook_non_executing_boundary_fails() {
        let text =
            repaired_module_resolution_fixture().replace("non-executing", "fully supported hooks");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("non-executing")),
            "dynamic/hook boundary must stay a non-executing bounded outcome, got: {violations:?}"
        );
    }

    #[test]
    fn omitting_not_proven_broader_rows_fails() {
        let text = repaired_module_resolution_fixture().replace("not_proven", "shipped");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("not_proven")),
            "M04/M07/provider/exact-process rows must remain not_proven on this page, got: {violations:?}"
        );
    }

    #[test]
    fn positive_complete_effective_root_claim_fails() {
        let text = repaired_module_resolution_fixture().replace(
            "not complete effective-root authority",
            "this is complete effective-root authority",
        );
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("not complete effective-root authority")),
            "a positive complete-effective-root claim must fail without the negation marker, got: {violations:?}"
        );
    }

    #[test]
    fn omitting_promotion_owners_fails() {
        let text = repaired_module_resolution_fixture().replace("#9270", "");
        let violations = inc_claim_guard_violations(MODULE_RESOLUTION_STATUS, &text);
        assert!(
            violations.iter().any(|v| v.contains("#9270")),
            "stronger public claims must keep #9270 as the exact-process promotion owner, got: {violations:?}"
        );
    }

    #[test]
    fn support_tiers_full_inc_support_is_rejected() {
        let mutated = "Module resolution / `@INC` consistency | perl-lsp has full @INC support\n";
        let violations = inc_claim_guard_violations(SUPPORT_TIERS, mutated);
        assert!(
            violations.iter().any(|v| v.contains("full @INC support")),
            "support-map complete @INC wording must fail, got: {violations:?}"
        );
    }

    #[test]
    fn unguarded_surface_fails_loudly() {
        let violations = inc_claim_guard_violations("docs/not-a-guarded-surface.md", "anything");
        assert!(!violations.is_empty());
        assert!(violations[0].contains("not covered"), "violation: {}", violations[0]);
    }

    #[test]
    fn current_tree_passes() -> Result<()> {
        check()
    }

    #[test]
    fn guard_table_covers_current_support_authority_surfaces() {
        let guarded: Vec<&str> = INC_CLAIM_GUARDS.iter().map(|g| g.file).collect();
        assert!(guarded.contains(&MODULE_RESOLUTION_STATUS));
        assert!(guarded.contains(&SUPPORT_TIERS));
        assert_eq!(guarded.len(), 2, "do not silently widen into a second claims system");
    }

    fn repaired_module_resolution_fixture() -> String {
        "\
Selected static @INC consumer rail
not complete effective-root authority
PL701 diagnostic, completion, goto-definition, and hover
ux_scenario_14_inc_conformance
#8493
#8506
#8544
invoked-script identity and `Bin`/`RealBin`
non-executing
#9270
#1744
not_proven
2026-05-11
| Contextual resolver authority | not_proven |
| Exact-process | not_proven |
| Provider/product support | not_proven |
"
        .to_string()
    }
}
