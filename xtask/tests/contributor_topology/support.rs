#![allow(dead_code)]

#[allow(dead_code)]
#[path = "../../src/bin/contributor_topology/mod.rs"]
pub mod contributor_topology;

use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn fixture_root() -> TempDir {
    let temp = tempfile::tempdir().expect("create fixture root");
    let root = temp.path();
    let identity = root.join("policy/product-identity.toml");
    fs::create_dir_all(identity.parent().expect("identity parent")).expect("create policy dir");
    fs::write(
        identity,
        r#"schema_version = 1

[product]
name = "perl-lsp"
public_repository = "EffortlessMetrics/perl-lsp"
development_repository = "EffortlessMetrics/perl-lsp-swarm"
"#,
    )
    .expect("write identity");

    let protocol = root.join("docs/swarm/sync-protocol.md");
    fs::create_dir_all(protocol.parent().expect("protocol parent")).expect("create docs dir");
    fs::write(
        protocol,
        r#"# perl-lsp Sync Protocol

`perl-lsp-swarm` is the active development source of truth. `perl-lsp` is the
release, history, and canonical package-lineage repo.

| Repo | Authority |
|---|---|
| `perl-lsp-swarm/main` | Active development |
| `perl-lsp/master` | Release lineage |

#### Mechanics: history-preserving complete-tree merge

git merge -s ours --no-commit swarm/main
git read-tree -u --reset swarm/main
"#,
    )
    .expect("write protocol");
    temp
}

pub fn captured_observation(overrides: &[(&str, Value)]) -> Value {
    let mut value = json!({
        "status": "PROVEN",
        "source": "fixture",
        "observed_at": "2026-08-15T10:30:00Z",
        "limitation": null,
        "development_repository": "EffortlessMetrics/perl-lsp-swarm",
        "development_branch": "main",
        "development_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "publication_repository": "EffortlessMetrics/perl-lsp",
        "publication_branch": "master",
        "publication_sha": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "prepared_swarm_sha": null,
        "publication_join_sha": null,
        "public_release_tag": null,
        "channels": {}
    });
    let object = value.as_object_mut().expect("observation object");
    for (key, replacement) in overrides {
        object.insert((*key).to_string(), replacement.clone());
    }
    value
}

pub fn write_observation(root: &Path, name: &str, value: &Value) -> PathBuf {
    let path = root.join(name);
    fs::write(
        &path,
        serde_json::to_string_pretty(value).expect("serialize observation"),
    )
    .expect("write observation");
    path
}
