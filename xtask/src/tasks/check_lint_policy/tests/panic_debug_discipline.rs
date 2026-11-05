use super::super::{DEBT_LEDGER, ROOT_MANIFEST, model::DebtLedger, read, validate};
use super::{deferred_lint, planned_lint, test_root};
use color_eyre::eyre::{Result, bail, eyre};
use toml::Value;

// The contract is explicit here rather than copied from the validator's pins.
const PANIC_DEBUG_LINTS: &[(&str, &str)] = &[
    ("unwrap_used", "clippy::unwrap_used"),
    ("expect_used", "clippy::expect_used"),
    ("panic", "clippy::panic"),
    ("todo", "clippy::todo"),
    ("unimplemented", "clippy::unimplemented"),
    ("dbg_macro", "clippy::dbg_macro"),
];

#[test]
fn panic_debug_policy_rejects_synchronized_rollbacks_in_gate_and_cadence() -> Result<()> {
    let root = test_root();
    let debt: DebtLedger = read::read_toml_as(root.join(DEBT_LEDGER))?;

    // Non-vacuity control: unchanged shipped inputs must pass both boundaries.
    let cargo = read::read_toml(root.join(ROOT_MANIFEST))?;
    let ledger = read::load_lint_ledger(root)?;
    validate::validate_all(root, &cargo, &ledger, &debt)?;
    validate::validate_cadence_sources(root, &ledger, &debt)?;

    for &(key, identity) in PANIC_DEBUG_LINTS {
        for (mutation, diagnostic) in [
            ("allow", "must remain at level deny"),
            ("warn", "must remain at level deny"),
            ("removed", "must appear exactly once"),
            ("tracked", "must remain an active ledger entry"),
            ("planned", "must remain an active ledger entry"),
            ("deferred_due", "must remain an active ledger entry"),
        ] {
            // Reload for every case: no mutation of shipped files, no preceding
            // case can supply the failure, and the actual fragment loader runs.
            let mut cargo = read::read_toml(root.join(ROOT_MANIFEST))?;
            let mut ledger = read::load_lint_ledger(root)?;
            let clippy = cargo
                .get_mut("workspace")
                .and_then(|workspace| workspace.get_mut("lints"))
                .and_then(|lints| lints.get_mut("clippy"))
                .and_then(Value::as_table_mut)
                .ok_or_else(|| eyre!("workspace.lints.clippy must be a table"))?;
            let entry = ledger
                .lint
                .iter_mut()
                .find(|entry| entry.name == identity)
                .ok_or_else(|| eyre!("shipped ledger must carry {identity}"))?;
            assert_eq!(entry.status, "active", "{identity}");
            assert_eq!(entry.level, "deny", "{identity}");
            let cargo_level = clippy.get(key).ok_or_else(|| eyre!("Cargo must carry {key}"))?;
            assert_eq!(cargo_level.as_str(), Some("deny"), "{identity}");

            match mutation {
                "allow" | "warn" => {
                    clippy.insert(key.to_owned(), Value::String(mutation.to_owned()));
                    entry.level = mutation.to_owned();
                }
                "tracked" => {
                    clippy.remove(key);
                    entry.status = "tracked".to_owned();
                }
                "removed" | "planned" | "deferred_due" => {
                    clippy.remove(key);
                    ledger.lint.retain(|entry| entry.name != identity);
                    match mutation {
                        "planned" => ledger.planned.push(planned_lint(identity, "1.99")),
                        "deferred_due" => {
                            // Due at the real product MSRV; the date is valid
                            // lifecycle syntax, independent of the wall clock.
                            let msrv = ledger.msrv.clone();
                            ledger.deferred_due.push(deferred_lint(identity, &msrv));
                        }
                        _ => {}
                    }
                }
                _ => bail!("unknown rollback case {mutation}"),
            }

            // These are coherent alternative policies, not mismatched inputs.
            // Without the pins this reconciliation still succeeds.
            validate::validate_workspace_lints(&cargo, &ledger)?;
            for (boundary, result) in [
                ("gate", validate::validate_all(root, &cargo, &ledger, &debt).map(|_| ())),
                ("cadence", validate::validate_cadence_sources(root, &ledger, &debt)),
            ] {
                let Err(error) = result else {
                    bail!("{boundary} accepted {identity} rollback to {mutation}");
                };
                let message = error.to_string();
                assert!(message.contains(identity), "{boundary}/{mutation}: {message}");
                assert!(message.contains(diagnostic), "{boundary}/{mutation}: {message}");
            }
        }
    }
    Ok(())
}
