//! Code-action ownership gate for the protocol-neutral native critic service.
//!
//! The broader normalization wiring inventory predates the code-action cutover.
//! This focused gate makes that transport independently falsifiable without
//! extending the older test file's panic-shaped assertion carveout: removing
//! the service call or restoring consumer-side rule/policy composition returns
//! a contextual test error.

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

const CODE_ACTION_SOURCE: &str = "src/runtime/language/code_actions.rs";
const SERVICE_ENTRYPOINT: &str = "NativeCriticService::analyze(";
const SERVICE_SUBJECT: &str = "NativeCriticSubject::accepted(";

/// Composition entry points owned by `NativeCriticService`, never an LSP
/// transport. Their presence here would recreate a second mutable critic
/// pipeline beside push and pull diagnostics.
const SERVICE_ONLY_COMPOSITION: [&str; 6] = [
    "native_finding_candidates(",
    "normalize_with_native_policy(",
    "NativeCriticPolicy::new(",
    "for_profile_with_config(",
    ".check_unfiltered(",
    "built_in_observation_candidates(",
];

fn code_action_source() -> Result<String, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(CODE_ACTION_SOURCE);
    fs::read_to_string(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("code-action source {} must be readable: {error}", path.display()),
        )
        .into()
    })
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

#[test]
fn code_actions_have_one_native_service_entry_and_no_private_pipeline()
-> Result<(), Box<dyn Error>> {
    let source = code_action_source()?;
    let service_entries = source.match_indices(SERVICE_ENTRYPOINT).count();

    require(
        service_entries == 1,
        format!(
            "{CODE_ACTION_SOURCE} must have exactly one native critic service entry; found \
             {service_entries} (#9062)"
        ),
    )?;
    require(
        source.contains(SERVICE_SUBJECT),
        format!(
            "{CODE_ACTION_SOURCE} must construct the accepted service subject instead of a \
             transport-owned input bag (#9062)"
        ),
    )?;

    for composition in SERVICE_ONLY_COMPOSITION {
        require(
            !source.contains(composition),
            format!(
                "{CODE_ACTION_SOURCE} bypasses NativeCriticService with `{composition}` (#9062)"
            ),
        )?;
    }

    Ok(())
}
