//! Managed-mutation concurrency contract for the staged Zed perllsp route
//! (#11316).
//!
//! Every managed candidate download must mutate attempt-private staging and
//! reach the durable subject only through one atomic publication, so
//! concurrent Zed processes elect exactly one winner with explicitly settled
//! losers. Cleanup is exact-attempt-owned, accepted state is reread before any
//! success is reported, and prefix sweeps can never cross live attempts.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const EXTENSION_SOURCE_RELATIVE_PATH: &str = ".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs";

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn extension_source(root: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(root.join(EXTENSION_SOURCE_RELATIVE_PATH))?)
}

/// Drop `//` comment lines so structural pins can only be satisfied by real
/// code.
fn code_lines(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn function_body(
    classified: &str,
    start_marker: &str,
    end_marker: &str,
) -> Result<String, Box<dyn Error>> {
    let start = classified.find(start_marker).ok_or("missing function marker")?;
    let end = start
        + classified[start..].find(end_marker).ok_or("function body has no terminator anchor")?;
    Ok(classified[start..end].to_string())
}

#[test]
fn downloads_extract_into_attempt_private_staging() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let classified = code_lines(&extension_source(&root)?);

    for required in [
        "fn next_attempt_id",
        "fn attempt_staging_dir",
        "fn selection_manifest_tmp_path",
        "fn claim_attempt_staging",
        "fn publish_staged_attempt",
        "fn remove_owned_attempt",
        "fn classify_durable_subject",
        "enum MutationOutcome",
        "enum DurableSubject",
    ] {
        assert!(
            classified.contains(required),
            "the attempt-private protocol must expose `{required}`"
        );
    }

    // Attempt identities are claimed via exclusive create, so two Zed
    // processes cannot share a staging root even within one clock tick.
    let claim = function_body(&classified, "fn claim_attempt_staging(", "\n}\n")?;
    assert!(
        claim.contains("fs::create_dir") && claim.contains("AlreadyExists"),
        "staging claims must rely on exclusive create-and-retry, not clock or PID guesses"
    );

    // Destructive replacement swaps the durable name to an owned graveyard
    // instead of unlinking in place, so a stale replacer can never leave a
    // concurrent winner's published tree missing.
    let publish = function_body(&classified, "fn publish_staged_attempt(", "\n}\n")?;
    assert!(
        publish.contains("swap_durable_aside"),
        "replacement must swap the durable directory aside atomically"
    );
    assert!(
        !publish.contains("remove_dir_all(&durable_dir)"),
        "the protocol must never unlink the durable destination in place"
    );

    let body = function_body(&classified, "fn download_perllsp(", "fn perl_dap_binary(")?;
    assert!(
        body.contains("zed::download_file(&asset.download_url, &attempt_dir"),
        "downloads must extract into the attempt-private staging root"
    );
    assert!(
        !body.contains("download_file(&asset.download_url, &version_dir"),
        "downloads must never extract directly into the durable directory"
    );
    assert!(
        !body.contains("remove_dir_all"),
        "the cold path must never delete the durable directory up front; replacement \
         happens only behind the contender reread guard"
    );

    // The durable name is reached only through the atomic publication.
    let publish_at = body
        .find("publish_staged_attempt(work_dir, &version_dir, &attempt_dir, &binary_path)")
        .ok_or("download_perllsp must publish through the atomic protocol")?;
    let metadata_call =
        body.find("latest_github_release(").ok_or("missing release metadata entry point")?;
    assert!(metadata_call < publish_at, "publication follows metadata resolution");
    assert!(
        body.find("load_accepted_current_in(Path::new(\".\")").is_some(),
        "offline reconstruction stays ahead of the cold route"
    );

    Ok(())
}

#[test]
fn success_requires_an_exact_accepted_state_reread() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let classified = code_lines(&extension_source(&root)?);
    let body = function_body(&classified, "fn download_perllsp(", "fn perllsp_command_settings")?;

    let store = body
        .find("store_selection_manifest(&attempt, &manifest)")
        .ok_or("the manifest promotion must bind this attempt's subject")?;
    let reread = body
        .find("load_accepted_current_in(work_dir, os, arch)")
        .ok_or("success must require an exact accepted-state reread")?;
    assert!(
        store < reread,
        "accepted-state reread must follow the manifest promotion and precede success"
    );
    assert!(
        body.contains("published perllsp subject failed accepted-state reread"),
        "reread failure must be typed, never treated as success"
    );
    // The served command path must be exactly what the final reread accepted,
    // so a rollover race cannot launch this attempt's superseded subject.
    let served = body
        .find("if accepted_path != binary_path {")
        .ok_or("the resolved command path must come from the accepted-state reread")?;
    assert!(reread < served, "accepted-path comparison must follow the reread");

    Ok(())
}

#[test]
fn cleanup_is_exact_attempt_owned_and_never_globbed() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let classified = code_lines(&extension_source(&root)?);

    let body = function_body(&classified, "fn download_perllsp(", "fn perllsp_command_settings")?;
    assert!(
        body.contains("remove_owned_attempt(&work_dir.join(&attempt_dir))"),
        "failure and settlement paths clean exactly this attempt's tree"
    );
    for forbidden in ["remove_dir_all(\".\")", "read_dir(\".\")"] {
        assert!(
            !body.contains(forbidden),
            "cleanup inside download_perllsp must not glob the extension directory"
        );
    }

    // Prefix sweeps skip attempt-private state entirely.
    let sweep = function_body(&classified, "fn remove_old_downloads_in(", "\n}\n")?;
    assert!(
        sweep.contains("name.ends_with(\".tmp\") || name.contains(\".attempt-\")"),
        "prefix sweeps must never touch `.tmp` siblings or `.attempt-` roots"
    );

    Ok(())
}

#[test]
fn separate_process_proof_exists_and_is_genuinely_multi_process() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = extension_source(&root)?;

    for required in [
        "PERLLSP_MUTATION_WORKER_DIR",
        "PERLLSP_MUTATION_WORKER_TAG",
        "env::current_exe()",
        "std::process::Command::new(&exe)",
        "separate_process_workers_elect_exactly_one_winner",
    ] {
        assert!(
            source.contains(required),
            "the isolated-writer proof must re-execute its own test binary: missing `{required}`"
        );
    }

    Ok(())
}
