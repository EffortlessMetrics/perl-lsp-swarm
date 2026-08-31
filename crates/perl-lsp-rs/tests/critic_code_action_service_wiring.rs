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
    if condition { Ok(()) } else { Err(io::Error::other(message.into()).into()) }
}

#[test]
fn code_actions_preserve_service_ownership() -> Result<(), Box<dyn Error>> {
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

/// The native run must not evaluate under a runtime document lock (#9062), and
/// the action surface must consume the same accepted finding set the diagnostic
/// transports do (#13304).
///
/// The runtime consequence of a regression is worse than a wrong result:
/// `native_critic_code_actions` re-acquires the documents guard to revalidate
/// the accepted generation, so restoring the lock-held call deadlocks the
/// code-action handler. This gate turns that into a readable failure.
#[test]
fn code_actions_release_the_document_guard_before_analysis() -> Result<(), Box<dyn Error>> {
    let source = code_action_source()?;

    let release = source.find("drop(documents);").ok_or_else(|| {
        io::Error::other(format!(
            "{CODE_ACTION_SOURCE} must release the runtime document guard before the native              critic run (#9062)"
        ))
    })?;
    let deferred_call = source.find("self.native_critic_code_actions(").ok_or_else(|| {
        io::Error::other(format!(
            "{CODE_ACTION_SOURCE} must run native critic analysis through the deferred,              lock-free helper (#9062)"
        ))
    })?;
    require(
        release < deferred_call,
        format!(
            "{CODE_ACTION_SOURCE} calls the native critic run before releasing the document              guard; #9062 requires snapshot, release, then evaluate"
        ),
    )?;

    let entry = source.find(SERVICE_ENTRYPOINT).ok_or_else(|| {
        io::Error::other(format!("{CODE_ACTION_SOURCE} must call the native critic service"))
    })?;
    require(
        entry > deferred_call,
        format!(
            "{CODE_ACTION_SOURCE} evaluates the native critic service inside the guarded              handler region instead of the deferred helper (#9062)"
        ),
    )?;

    Ok(())
}

/// The action transport must present the same logical row as push and pull.
#[test]
fn code_actions_consume_the_shared_finding_set_and_public_identity() -> Result<(), Box<dyn Error>> {
    let source = code_action_source()?;

    require(
        source.contains("critic_overlap_observations("),
        format!(
            "{CODE_ACTION_SOURCE} must feed the service the producer-declared overlap              observations push and pull feed it, not an empty set (#11918/#13304)"
        ),
    )?;

    // The embedded action diagnostic is the client's association key with the
    // published row, so it must be projected from the normalized finding.
    for projection in
        ["normalized.public_code()", "normalized.severity()", "normalized.user_visible_message()"]
    {
        require(
            source.contains(projection),
            format!(
                "{CODE_ACTION_SOURCE} must project the embedded action diagnostic from                  `{projection}` so it matches the published row (#13304)"
            ),
        )?;
    }

    // Producer-local fields must never reach the wire: for a reviewed
    // core/native alias they carry a different identity than the published row.
    for producer_local in [
        "finding.rule_id.clone()",
        "finding.message.clone()",
        "finding.severity.to_diagnostic_severity()",
    ] {
        require(
            !source.contains(producer_local),
            format!(
                "{CODE_ACTION_SOURCE} projects the embedded action diagnostic from the                  producer-local `{producer_local}` instead of the normalized public identity                  (#13304)"
            ),
        )?;
    }

    Ok(())
}
