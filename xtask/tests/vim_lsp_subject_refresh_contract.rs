//! Contract tests for the #11411 vim-lsp subject refresh.
//!
//! These run fully offline against the landed #11369 authority fixtures:
//!
//! - the compiled probe table binds exactly the landed public-surface
//!   inventory (no surface unprobed, no probe invented);
//! - every recorded plugin feature-gate default is grounded in the landed
//!   manifest prose, so the table cannot silently diverge from the pin;
//! - a synthesized world-unchanged observation built *from the landed
//!   manifest itself* classifies as an explicit `no_change` with positive
//!   findings — the offline default path consumes only retained metadata;
//! - the end-to-end `run` path with a retained packet stays offline, writes
//!   a bounded proposal, and leaves the landed fixtures byte-identical.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use xtask::vim_lsp_subject_refresh::{
    self, RefreshOptions,
    classify::PinnedSubject,
    model::{
        DriftClass, FloorObservation, HeadTreeProbe, ObservationPacket, ObservedFile,
        PinnedCommitProbe, ProbeStatus, RefsProbe, SurfaceFinding,
    },
    probe_table::{self, EXPECTED_PLUGIN_DEFAULTS, FILE_DOC, FILE_README, SURFACE_PROBES},
};

fn repository_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask must live below the repository root")
}

fn load_json(path: &Path) -> Result<serde_json::Value> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

#[test]
fn probe_table_binds_exactly_the_landed_public_surface_inventory() -> Result<()> {
    let root = repository_root()?;
    let inventory = load_json(&root.join(vim_lsp_subject_refresh::PUBLIC_SURFACE_INVENTORY_PATH))?;
    probe_table::validate_table_against_inventory(&inventory)?;
    // The table must also be non-trivial: every inventory surface carries at
    // least one needle.
    let mut surfaces: Vec<&str> = inventory
        .get("surfaces")
        .and_then(|value| value.as_array())
        .expect("surfaces array")
        .iter()
        .map(|surface| surface.get("surface").and_then(|value| value.as_str()).unwrap_or_default())
        .collect();
    surfaces.sort_unstable();
    surfaces.dedup();
    for surface in surfaces {
        ensure!(
            SURFACE_PROBES.iter().any(|probe| probe.surface == surface),
            "surface {surface} carries no probes"
        );
    }
    Ok(())
}

#[test]
fn recorded_plugin_defaults_are_grounded_in_the_landed_manifest_prose() -> Result<()> {
    let root = repository_root()?;
    let manifest = load_json(&root.join(vim_lsp_subject_refresh::SUBJECT_MANIFEST_PATH))?;
    let rendered = serde_json::to_string(&manifest)?;
    for (name, expression) in EXPECTED_PLUGIN_DEFAULTS {
        ensure!(
            rendered.contains(expression),
            "expected default expression for {name} is not grounded in the landed manifest prose"
        );
    }
    // The floor expectations must also match the landed rows.
    let pinned = PinnedSubject::from_manifest(&manifest)?;
    ensure!(pinned.vim_theoretical_minimum == "8.1.1035");
    ensure!(pinned.neovim_theoretical_minimum == "0.3");
    ensure!(pinned.latest_release_tag == "v0.1.4");
    Ok(())
}

/// Synthesize a world-unchanged packet strictly from the landed manifest.
fn unchanged_packet_from_manifest(manifest: &serde_json::Value) -> Result<ObservationPacket> {
    let pinned = PinnedSubject::from_manifest(manifest)?;
    let commit = pinned.selected_commit.clone();
    let entry_file_blobs = pinned
        .entry_files
        .iter()
        .map(|(path, blob)| ObservedFile {
            commit: commit.clone(),
            path: path.clone(),
            present: true,
            git_blob_sha1: Some(blob.clone()),
        })
        .collect();
    let mut paths: Vec<String> =
        SURFACE_PROBES.iter().map(|probe| probe.file.to_string()).collect();
    paths.extend(pinned.entry_files.iter().map(|(path, _)| path.clone()));
    paths.push(FILE_DOC.to_string());
    paths.push(FILE_README.to_string());
    paths.sort();
    paths.dedup();
    let files = paths
        .into_iter()
        .map(|path| {
            let blob = pinned
                .entry_files
                .iter()
                .find(|(entry, _)| *entry == path)
                .map(|(_, blob)| blob.clone())
                .unwrap_or_else(|| "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
            ObservedFile { commit: commit.clone(), path, present: true, git_blob_sha1: Some(blob) }
        })
        .collect();
    Ok(ObservationPacket {
        schema_version: "vim_lsp_subject_observation.v1".to_string(),
        observed_at_utc: "2026-08-24T00:00:00+00:00".to_string(),
        upstream_repository: pinned.repository.clone(),
        refs_probe: RefsProbe {
            method: "retained-fixture (offline contract test)".to_string(),
            status: ProbeStatus::Ok,
            head: Some(commit.clone()),
            master: Some(commit.clone()),
            tags: vec![pinned.latest_release_tag.clone()],
            error: None,
        },
        pinned_commit_probe: PinnedCommitProbe {
            method: "retained-fixture (offline contract test)".to_string(),
            status: ProbeStatus::Ok,
            requested_commit: pinned.selected_commit.clone(),
            resolved_commit: Some(commit.clone()),
            resolved_tree: Some(pinned.tree_digest.clone()),
            commit_subject: Some("retained fixture".to_string()),
            commit_author_date: Some("2026-08-10T16:38:59+09:00".to_string()),
            entry_file_blobs,
            error: None,
        },
        head_tree_probe: HeadTreeProbe {
            method: "retained-fixture (offline contract test)".to_string(),
            status: ProbeStatus::Ok,
            commit: Some(commit),
            files,
            floor: Some(FloorObservation {
                parsed: true,
                neovim_minimum: Some(pinned.neovim_theoretical_minimum.clone()),
                vim_minimum: Some(pinned.vim_theoretical_minimum.clone()),
            }),
            plugin_defaults: EXPECTED_PLUGIN_DEFAULTS
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect::<BTreeMap<_, _>>(),
            load_guard_present: Some(true),
            maintenance_markers: Vec::new(),
            snippet_note_present: Some(true),
            surface_findings: SURFACE_PROBES
                .iter()
                .map(|probe| SurfaceFinding {
                    surface: probe.surface.to_string(),
                    file: probe.file.to_string(),
                    needle: probe.needle.to_string(),
                    found: true,
                })
                .collect(),
            error: None,
        },
    })
}

#[test]
fn world_unchanged_packet_from_the_landed_manifest_classifies_no_change() -> Result<()> {
    let root = repository_root()?;
    let manifest = load_json(&root.join(vim_lsp_subject_refresh::SUBJECT_MANIFEST_PATH))?;
    let pinned = PinnedSubject::from_manifest(&manifest)?;
    let packet = unchanged_packet_from_manifest(&manifest)?;
    vim_lsp_subject_refresh::validate_packet(&packet, &pinned)
        .context("synthesized packet must validate against the landed authority")?;
    let artifact = vim_lsp_subject_refresh::classify::classify(&packet, &pinned)?;
    let fired: Vec<DriftClass> = artifact.drift_classes.iter().map(|entry| entry.class).collect();
    ensure!(fired == vec![DriftClass::NoChange], "expected explicit no_change, got {fired:?}");
    ensure!(!artifact.positive_findings.is_empty(), "no_change must carry positive findings");
    ensure!(artifact.recommended_disposition == vec!["retain_pin".to_string()]);
    Ok(())
}

#[test]
fn retained_packet_run_is_offline_and_leaves_landed_fixtures_byte_identical() -> Result<()> {
    let root = repository_root()?;
    let manifest_path = root.join(vim_lsp_subject_refresh::SUBJECT_MANIFEST_PATH);
    let inventory_path = root.join(vim_lsp_subject_refresh::PUBLIC_SURFACE_INVENTORY_PATH);
    let manifest_before = std::fs::read(&manifest_path)?;
    let inventory_before = std::fs::read(&inventory_path)?;

    let manifest = load_json(&manifest_path)?;
    let packet = unchanged_packet_from_manifest(&manifest)?;

    let scratch = tempfile::tempdir()?;
    let packet_path = scratch.path().join("observation.json");
    // The proposal boundary is repository-local: the artifact goes under the
    // repository's ignored `target/` tree, never outside the repository.
    let proposal_path = root.join("target").join("vim-lsp-subject-refresh-contract.json");
    std::fs::write(&packet_path, serde_json::to_string(&packet)?)?;

    let outcome = vim_lsp_subject_refresh::run(RefreshOptions {
        check: true,
        proposal: Some(proposal_path.clone()),
        observation: Some(packet_path),
        allow_network: false,
        repo_root: root.clone(),
    })
    .context("offline run with a retained packet must succeed without any network gate")?;
    ensure!(!outcome.instrument_failed, "world-unchanged run must not report instrument failure");

    let written = std::fs::read_to_string(&proposal_path)?;
    let artifact: serde_json::Value = serde_json::from_str(&written)?;
    ensure!(
        artifact.get("schema_version").and_then(|value| value.as_str())
            == Some("vim_lsp_subject_refresh.v1")
    );
    ensure!(
        artifact
            .get("drift_classes")
            .and_then(|value| value.as_array())
            .is_some_and(|classes| !classes.is_empty()),
        "artifact must always carry explicit classes"
    );
    ensure!(
        std::fs::read(&manifest_path)? == manifest_before,
        "subject fixture must stay byte-identical"
    );
    ensure!(
        std::fs::read(&inventory_path)? == inventory_before,
        "inventory fixture must stay byte-identical"
    );
    Ok(())
}

#[test]
fn run_without_network_gate_or_packet_fails_closed() -> Result<()> {
    let root = repository_root()?;
    let error = vim_lsp_subject_refresh::run(RefreshOptions {
        check: true,
        proposal: None,
        observation: None,
        allow_network: false,
        repo_root: root,
    })
    .expect_err("live observation without the gate must refuse");
    ensure!(
        error.to_string().contains("--allow-network"),
        "the refusal must name the gate; got {error:#}"
    );
    Ok(())
}
