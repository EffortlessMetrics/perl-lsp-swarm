//! `cargo xtask standalone-diagnostics` — check, explain, and project the
//! standalone diagnostic reason/action registry (#11493).

use crate::utils::project_root;
use color_eyre::eyre::{Result, eyre};
use xtask::standalone_diagnostics as diagnostics;

#[derive(clap::Subcommand, Debug)]
pub enum StandaloneDiagnosticsSubcommand {
    /// Validate the registry, its schema agreement, totality, and reachability.
    Check,
    /// Print one reason row as pretty JSON.
    Explain {
        /// Stable reason ID (for example `sel_committed_path_persisted_new_session_required`).
        reason_id: String,
    },
    /// Project one `standalone_install_transition.v1` packet into its bounded
    /// user consequence.
    Project {
        /// Path to a transition packet JSON file.
        packet: String,
    },
}

pub fn run(command: StandaloneDiagnosticsSubcommand) -> Result<()> {
    let root = project_root()?;
    match command {
        StandaloneDiagnosticsSubcommand::Check => {
            let stats =
                diagnostics::validate_manifest_file(&root).map_err(|error| eyre!("{error}"))?;
            println!(
                "standalone diagnostics registry check passed: {} actions, {} templates, \
                 {} primary reasons, {} additional reasons, {} typed combinations covered, \
                 {} deferred reason domains",
                stats.actions,
                stats.summary_templates,
                stats.primary_reasons,
                stats.additional_reasons,
                stats.combinations,
                stats.deferred_reason_domains
            );
        }
        StandaloneDiagnosticsSubcommand::Explain { reason_id } => {
            let manifest = validated_manifest(&root)?;
            let explained = diagnostics::explain_reason(&manifest, &reason_id)
                .ok_or_else(|| eyre!("unknown reason id `{reason_id}`"))?;
            println!("{explained}");
        }
        StandaloneDiagnosticsSubcommand::Project { packet } => {
            let manifest = validated_manifest(&root)?;
            let bytes = std::fs::read(&packet)
                .map_err(|error| eyre!("cannot read packet `{packet}`: {error}"))?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|error| eyre!("packet `{packet}` is not valid JSON: {error}"))?;
            let projection =
                diagnostics::project_packet(&manifest, &value).map_err(|error| eyre!("{error}"))?;
            println!("{}", serde_json::to_string_pretty(&projection)?);
        }
    }
    Ok(())
}

/// Load the registry only after it validates.
///
/// `explain` and `project` previously read the manifest directly, so a locally
/// drifted registry — one `check` would reject — could still produce confident
/// reason text and projections. The validating read costs a few milliseconds
/// and keeps `check` the single admission gate for every command.
fn validated_manifest(root: &std::path::Path) -> Result<serde_json::Value> {
    diagnostics::validate_manifest_file(root).map_err(|error| {
        eyre!("registry is not valid, so it cannot be explained or projected: {error}")
    })?;
    diagnostics::load_manifest(root).map_err(|error| eyre!("{error}"))
}
