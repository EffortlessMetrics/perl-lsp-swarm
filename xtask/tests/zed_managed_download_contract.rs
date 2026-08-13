use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

fn repo_root() -> Result<PathBuf, Box<dyn Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::other("xtask manifest has no repository parent").into())
}

fn read_json(root: &Path, relative: &str) -> Result<Value, Box<dyn Error>> {
    let text = fs::read_to_string(root.join(relative))?;
    Ok(serde_json::from_str(&text)?)
}

fn string<'a>(value: &'a Value, pointer: &str) -> Result<&'a str, Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other(format!("missing string at `{pointer}`")).into())
}

fn array<'a>(value: &'a Value, pointer: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value
        .pointer(pointer)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("missing array at `{pointer}`")).into())
}

fn safe_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.is_absolute()
        && path.components().all(|component| {
            !matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn validate_contract(contract: &Value, release_targets: &BTreeSet<String>) -> Result<(), String> {
    if contract
        .pointer("/schema_version")
        .and_then(Value::as_str)
        != Some("zed_perllsp_managed_downloads.v1")
    {
        return Err("unexpected schema version".to_string());
    }

    if contract.pointer("/identity/server_id").and_then(Value::as_str) != Some("perllsp")
        || contract.pointer("/identity/executable").and_then(Value::as_str) != Some("perllsp")
    {
        return Err("managed route must retain exact perllsp identity".to_string());
    }

    let arguments = contract
        .pointer("/identity/arguments")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing launch arguments".to_string())?;
    if arguments != &[Value::String("--stdio".to_string())] {
        return Err("managed route must launch exact --stdio arguments".to_string());
    }

    let version = contract
        .pointer("/source/version")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing public release version".to_string())?;
    let targets = contract
        .pointer("/targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing target rows".to_string())?;

    let mut identities = BTreeSet::new();
    let mut managed = 0_usize;
    let mut windows_arm64_is_explicit = false;

    for row in targets {
        let target = row
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| "target row lacks target triple".to_string())?;
        if !identities.insert(target.to_string()) {
            return Err(format!("duplicate target row `{target}`"));
        }

        let disposition = row
            .get("disposition")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("target `{target}` lacks disposition"))?;
        if target == "aarch64-pc-windows-msvc" {
            windows_arm64_is_explicit = disposition == "unsupported";
        }
        if disposition != "managed" {
            continue;
        }
        managed += 1;

        if !release_targets.contains(target) {
            return Err(format!(
                "managed target `{target}` is absent from release topology"
            ));
        }

        let archive_type = row
            .get("archive_type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("managed target `{target}` lacks archive type"))?;
        let expected_suffix = match archive_type {
            "tar.gz" => ".tar.gz",
            "zip" => ".zip",
            other => return Err(format!("unsupported archive type `{other}`")),
        };
        let expected_name = format!("perllsp-{version}-{target}{expected_suffix}");
        if row.get("asset_name").and_then(Value::as_str) != Some(expected_name.as_str()) {
            return Err(format!("target `{target}` has a mismatched asset name"));
        }

        let digest = row
            .get("asset_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("target `{target}` lacks public asset digest"))?;
        if digest.len() != "sha256:".len() + 64
            || !digest.starts_with("sha256:")
            || !digest["sha256:".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(format!("target `{target}` has an invalid SHA-256 digest"));
        }

        let member = row
            .get("archive_member")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("target `{target}` lacks archive member"))?;
        let installed = row
            .get("installed_path")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("target `{target}` lacks installed path"))?;
        if !safe_relative_path(member) || !safe_relative_path(installed) {
            return Err(format!("target `{target}` contains an unsafe path"));
        }
        if member.contains("perl-lsp") || installed.contains("perl-lsp") {
            return Err(format!(
                "target `{target}` references another or retired executable"
            ));
        }
        if archive_type == "zip" && member != "perllsp.exe" {
            return Err("Windows archive must retain root-level perllsp.exe".to_string());
        }
        if archive_type == "tar.gz"
            && member != format!("perllsp-{version}-{target}/perllsp")
        {
            return Err(format!("Unix target `{target}` has wrong member layout"));
        }

        if row.get("host_execution").and_then(Value::as_str) != Some("not_proven") {
            return Err(format!(
                "metadata-only target `{target}` must not claim host execution"
            ));
        }
    }

    if managed != 5 {
        return Err(format!("expected five managed target rows, found {managed}"));
    }
    if !windows_arm64_is_explicit {
        return Err("Windows ARM64 must remain explicit unsupported".to_string());
    }

    let boundary = contract
        .pointer("/claim_boundary")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing claim boundary".to_string())?;
    for cell in [
        "archive_extraction",
        "perllsp_version_execution",
        "stdio_initialize_shutdown",
        "actual_zed_host",
    ] {
        if boundary.get(cell).and_then(Value::as_str) != Some("not_proven") {
            return Err(format!("claim cell `{cell}` must remain not_proven"));
        }
    }

    Ok(())
}

#[test]
fn managed_download_projection_matches_release_authority() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json",
    )?;
    let release = read_json(&root, "docs/reference/downstream-dap-integrations.json")?;
    let release_targets: BTreeSet<String> = array(&release, "/targets")?
        .iter()
        .filter_map(|row| row.get("triple").and_then(Value::as_str))
        .map(str::to_string)
        .collect();

    validate_contract(&contract, &release_targets)
        .map_err(|message| io::Error::other(format!("invalid Zed contract: {message}")))?;

    assert_eq!(string(&contract, "/source/tag")?, "v0.17.0");
    assert_eq!(string(&contract, "/source/version")?, "0.17.0");
    Ok(())
}

#[test]
fn candidate_target_helpers_are_bound_to_the_projection() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json",
    )?;
    let source = fs::read_to_string(
        root.join(".ci/fixtures/zed-perl-upstream/zed-perl/src/perl.rs"),
    )?;

    for row in array(&contract, "/targets")? {
        let target = row
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other("target row lacks target"))?;
        assert!(
            source.contains(target),
            "candidate source has no disposition for `{target}`"
        );
    }
    assert!(source.contains("perllsp_asset_name"));
    assert!(source.contains("perllsp_binary_path"));
    Ok(())
}

#[test]
fn mutation_controls_reject_false_managed_claims() -> Result<(), Box<dyn Error>> {
    let root = repo_root()?;
    let contract = read_json(
        &root,
        ".ci/fixtures/zed-perl-upstream/managed-downloads.v1.json",
    )?;
    let release = read_json(&root, "docs/reference/downstream-dap-integrations.json")?;
    let release_targets: BTreeSet<String> = array(&release, "/targets")?
        .iter()
        .filter_map(|row| row.get("triple").and_then(Value::as_str))
        .map(str::to_string)
        .collect();

    let mut wrong_product = contract.clone();
    wrong_product["identity"]["executable"] = Value::String("perl-lsp".to_string());
    assert!(validate_contract(&wrong_product, &release_targets).is_err());

    let mut traversal = contract.clone();
    traversal["targets"][0]["archive_member"] = Value::String("../perllsp".to_string());
    assert!(validate_contract(&traversal, &release_targets).is_err());

    let mut cross_arch = contract.clone();
    cross_arch["targets"][5]["disposition"] = Value::String("managed".to_string());
    assert!(validate_contract(&cross_arch, &release_targets).is_err());

    let mut overclaim = contract;
    overclaim["claim_boundary"]["actual_zed_host"] = Value::String("proven".to_string());
    assert!(validate_contract(&overclaim, &release_targets).is_err());

    Ok(())
}
