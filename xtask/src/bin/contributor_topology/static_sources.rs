use super::{
    EXPECTED_DEVELOPMENT_BRANCH, EXPECTED_PUBLICATION_BRANCH, PRODUCT_IDENTITY_PATH,
    PROMOTION_PROTOCOL, SYNC_PROTOCOL_PATH, SourceDigest, StaticTopology,
};
use anyhow::{Context, Result, bail};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(super) fn load_static_topology(
    root: &Path,
) -> Result<(StaticTopology, BTreeMap<String, SourceDigest>)> {
    let identity_path = root.join(PRODUCT_IDENTITY_PATH);
    let protocol_path = root.join(SYNC_PROTOCOL_PATH);
    let identity_text = fs::read_to_string(&identity_path)
        .with_context(|| format!("reading {}", identity_path.display()))?;
    let protocol = fs::read_to_string(&protocol_path)
        .with_context(|| format!("reading {}", protocol_path.display()))?;
    let identity: toml::Value = toml::from_str(&identity_text)
        .with_context(|| format!("parsing {}", identity_path.display()))?;

    if identity.get("schema_version").and_then(toml::Value::as_integer) != Some(1) {
        bail!("product identity schema must be 1");
    }
    let product = identity
        .get("product")
        .and_then(toml::Value::as_table)
        .context("product identity [product] table is missing")?;
    let development_repository = product
        .get("development_repository")
        .and_then(toml::Value::as_str)
        .context("product.development_repository is missing")?;
    let publication_repository = product
        .get("public_repository")
        .and_then(toml::Value::as_str)
        .context("product.public_repository is missing")?;
    validate_repository(development_repository)?;
    validate_repository(publication_repository)?;
    if development_repository == publication_repository {
        bail!("development and publication repositories must differ");
    }

    let development_branch = repository_branch(&protocol, development_repository)?;
    let publication_branch = repository_branch(&protocol, publication_repository)?;
    if development_branch != EXPECTED_DEVELOPMENT_BRANCH {
        bail!("development branch must be {EXPECTED_DEVELOPMENT_BRANCH:?}");
    }
    if publication_branch != EXPECTED_PUBLICATION_BRANCH {
        bail!("publication branch must be {EXPECTED_PUBLICATION_BRANCH:?}");
    }

    let development_name = repository_name(development_repository)?;
    let publication_name = repository_name(publication_repository)?;
    let development_role =
        format!("`{development_name}` is the active development source of truth.");
    if !protocol.contains(&development_role) {
        bail!("sync protocol is missing the development repository role");
    }
    let publication_role = Regex::new(&format!(
        r"`{}` is the\s+release, history, and canonical package-lineage repo\.",
        regex::escape(publication_name)
    ))?;
    if !publication_role.is_match(&protocol) {
        bail!("sync protocol is missing the publication repository role");
    }
    for marker in [
        "#### Mechanics: history-preserving complete-tree merge".to_string(),
        format!("git merge -s ours --no-commit swarm/{development_branch}"),
        format!("git read-tree -u --reset swarm/{development_branch}"),
    ] {
        if !protocol.contains(&marker) {
            bail!("sync protocol is missing required promotion marker {marker:?}");
        }
    }

    let static_topology = StaticTopology {
        development_repository: development_repository.to_string(),
        development_default_branch: development_branch,
        publication_repository: publication_repository.to_string(),
        publication_branch,
        issue_repository: development_repository.to_string(),
        pull_request_repository: development_repository.to_string(),
        promotion_protocol: PROMOTION_PROTOCOL.to_string(),
    };
    let sources = BTreeMap::from([
        (
            PRODUCT_IDENTITY_PATH.to_string(),
            SourceDigest {
                path: PRODUCT_IDENTITY_PATH.to_string(),
                sha256: sha256_file(&identity_path)?,
            },
        ),
        (
            SYNC_PROTOCOL_PATH.to_string(),
            SourceDigest {
                path: SYNC_PROTOCOL_PATH.to_string(),
                sha256: sha256_file(&protocol_path)?,
            },
        ),
    ]);
    Ok((static_topology, sources))
}

fn repository_branch(protocol: &str, repository: &str) -> Result<String> {
    let name = repository_name(repository)?;
    let pattern = Regex::new(&format!(
        r"(?m)^\|\s*`{}/([^`]+)`\s*\|\s*[^|]+\|\s*$",
        regex::escape(name)
    ))?;
    let matches: Vec<String> = pattern
        .captures_iter(protocol)
        .filter_map(|captures| {
            captures
                .get(1)
                .map(|branch| branch.as_str().trim().to_string())
        })
        .collect();
    if matches.len() != 1 {
        bail!(
            "expected one authority row for {repository}; found {}",
            matches.len()
        );
    }
    let branch = &matches[0];
    if branch.is_empty() || branch.starts_with('-') || branch.chars().any(char::is_whitespace) {
        bail!("invalid branch for {repository}: {branch:?}");
    }
    Ok(branch.clone())
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn validate_repository(repository: &str) -> Result<()> {
    let mut parts = repository.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        bail!("repository must be an owner/name slug: {repository:?}");
    }
    let valid = |part: &str| {
        part.chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
    };
    if !valid(owner) || !valid(name) {
        bail!("repository contains unsupported characters: {repository:?}");
    }
    Ok(())
}

fn repository_name(repository: &str) -> Result<&str> {
    validate_repository(repository)?;
    repository
        .split_once('/')
        .map(|(_, name)| name)
        .context("repository has no name")
}
