//! Offline-first startup contract for the managed Zed perllsp route (#11308).
//!
//! Startup must reconstruct an accepted current subject from durable exact
//! identity without any release-metadata request, and release metadata must be
//! reachable only through the admitted cold-install path. This test pins the
//! structural ordering, the durable selection manifest surface, and the typed
//! update-state vocabulary inside the staged extension source.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

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

#[test]
fn startup_reconstruction_precedes_any_release_metadata_request() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = extension_source(&root)?;

    let reconstruction =
        source.find("fn load_accepted_current_in").ok_or("missing offline reconstruction")?;
    let metadata_call = source
        .find("latest_github_release")
        .ok_or("missing cold-route release metadata entry point")?;
    assert!(
        reconstruction < metadata_call,
        "offline accepted-current reconstruction must exist before the release metadata path"
    );

    let download_start =
        source.find("fn download_perllsp").ok_or("missing managed perllsp download entry point")?;
    let download_end = source[download_start..]
        .find("fn perllsp_command_settings")
        .map(|offset| download_start + offset)
        .ok_or("managed download body has no terminator anchor")?;
    let body = &source[download_start..download_end];

    let reconstruct_call = body.find("load_accepted_current_in(");
    let metadata_in_body = body.find("latest_github_release(");
    match (reconstruct_call, metadata_in_body) {
        (Some(reconstruct_at), Some(metadata_at)) => assert!(
            reconstruct_at < metadata_at,
            "download_perllsp must attempt offline reconstruction before requesting release \
             metadata"
        ),
        _ => return Err("download_perllsp lost either the reconstruction or metadata step".into()),
    }

    // The steady-state update fact must be recorded as NotRequested on the
    // offline path: an accepted subject never claims fresh update knowledge.
    let offline_prefix = &body[..body.find("latest_github_release(").unwrap_or(body.len())];
    assert!(
        offline_prefix.contains("UpdateState::NotRequested"),
        "the offline reconstruction path must record UpdateState::NotRequested"
    );

    Ok(())
}

#[test]
fn durable_selection_manifest_surface_is_bound() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = extension_source(&root)?;

    for required in [
        "perllsp-selection.v1.json",
        "perllsp-selection.v1.json.tmp",
        "perllsp_selection.v1",
        "fn parse_selection_manifest",
        "fn store_selection_manifest",
        "is_safe_relative_path",
        "binary_sha256",
        "sha256:{digest}",
        "fs::rename",
    ] {
        assert!(source.contains(required), "extension source lacks `{required}`");
    }

    // Exact identity means the manifest binds release/target/member/path plus
    // the binary digest; presence alone must stay insufficient.
    for field_pointer in [
        "\"/release/tag\"",
        "\"/release/version\"",
        "\"/target\"",
        "\"/asset_name\"",
        "\"/archive_member\"",
        "\"/installed_path\"",
        "\"/binary_sha256\"",
    ] {
        assert!(
            source.contains(field_pointer),
            "manifest validation must bind `{field_pointer}` explicitly"
        );
    }

    Ok(())
}

#[test]
fn manifest_validator_rejects_false_identity_claims() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let source = extension_source(&root)?;

    // The production validator is fail-closed by structure; mirror its
    // required-field set here so removing a binding from perl.rs turns this
    // negative control red instead of silently weakening selection.
    for rejected_shape in [
        "unsupported schema_version",
        "does not describe the perllsp product",
        "does not describe the LSP server role",
        "not a selected current subject",
        "not a safe relative path",
        "not a sha256 digest",
        "no accepted perllsp selection manifest",
        "no longer matches its recorded digest",
    ] {
        assert!(
            source.contains(rejected_shape),
            "selection validation lost the failure mode `{rejected_shape}`"
        );
    }

    Ok(())
}
