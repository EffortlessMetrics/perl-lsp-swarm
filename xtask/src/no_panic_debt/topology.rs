use super::model::{
    FileRecord, Instrument, InstrumentStatus, PackageRecord, TargetKind, Topology, Vocabulary,
};
use super::normalize_path;
use crate::utils::run_cargo_metadata_at;
use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn discover(root: &Path, vocabulary: &Vocabulary) -> Result<Topology> {
    let mut instruments = vocabulary.instruments.clone();
    let root = effective_root(root);
    let cargo = root.join("Cargo.toml");
    if !cargo.is_file() {
        instruments.push(not_proven(
            "test_topology",
            &normalize_path(&cargo, &root),
            "workspace or package Cargo.toml missing",
        ));
        return Ok(Topology { packages: Vec::new(), files: Vec::new(), instruments });
    }

    let metadata = match cargo_metadata(&root) {
        Ok(metadata) => metadata,
        Err(err) => {
            instruments.push(cargo_metadata_not_proven(&root, &err));
            return Ok(Topology { packages: Vec::new(), files: Vec::new(), instruments });
        }
    };

    let mut packages = Vec::new();
    let mut files = Vec::new();
    for package in workspace_packages(&metadata) {
        let Some(package_root) = Path::new(&package.manifest_path).parent() else {
            instruments.push(not_proven(
                "test_topology",
                &normalize_path(Path::new(&package.manifest_path), &root),
                "package manifest_path has no parent; test-bearing population is not proven",
            ));
            continue;
        };
        packages.push(package_record(&root, package_root, package));
        files.extend(collect_package_files(&root, package_root, package));
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    note_unreachable_packages(&root, &packages, &mut instruments);
    Ok(Topology { packages, files, instruments })
}

/// Independent Cargo-metadata read of `test=true` source roots for integrity.
/// Same Cargo authority as discovery; a second process, not a handwritten interpreter.
pub(crate) fn cargo_test_src_paths(root: &Path) -> Result<BTreeSet<String>> {
    let root = effective_root(root);
    let metadata = cargo_metadata(&root)?;
    let mut paths = BTreeSet::new();
    for package in workspace_packages(&metadata) {
        let Some(package_root) = Path::new(&package.manifest_path).parent() else {
            continue;
        };
        for target in admitted_targets(package) {
            let Some(src) = existing_src_path(package_root, &target.src_path) else {
                continue;
            };
            paths.insert(normalize_path(&src, &root));
        }
    }
    Ok(paths)
}

pub(crate) fn is_complete_test_file(kind: TargetKind, path: &str) -> bool {
    match kind {
        TargetKind::IntegrationTest | TargetKind::Example | TargetKind::Bench => true,
        TargetKind::UnitTest => {
            path.ends_with("/tests.rs") || path.ends_with("_test.rs") || path.ends_with("_tests.rs")
        }
        TargetKind::Build | TargetKind::Unknown => false,
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    id: String,
    manifest_path: String,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: String,
    #[serde(default)]
    test: bool,
    #[serde(default, rename = "required-features")]
    required_features: Vec<String>,
}

fn cargo_metadata(root: &Path) -> Result<CargoMetadata> {
    let raw = run_cargo_metadata_at(&root.join("Cargo.toml"), true)?;
    serde_json::from_slice(&raw).map_err(|err| eyre!("parsing cargo metadata JSON: {err}"))
}

fn workspace_packages(metadata: &CargoMetadata) -> impl Iterator<Item = &CargoPackage> {
    let workspace_ids: BTreeSet<&str> =
        metadata.workspace_members.iter().map(String::as_str).collect();
    metadata.packages.iter().filter(move |package| workspace_ids.contains(package.id.as_str()))
}

fn admitted_targets(package: &CargoPackage) -> impl Iterator<Item = &CargoTarget> {
    package
        .targets
        .iter()
        .filter(|target| target.test && !target.kind.iter().any(|kind| kind == "custom-build"))
}

fn package_record(root: &Path, package_root: &Path, package: &CargoPackage) -> PackageRecord {
    let mut features: Vec<String> = package.features.keys().cloned().collect();
    features.sort();
    PackageRecord {
        name: package.name.clone(),
        manifest: normalize_path(&package_root.join("Cargo.toml"), root),
        features,
    }
}

fn collect_package_files(
    root: &Path,
    package_root: &Path,
    package: &CargoPackage,
) -> Vec<FileRecord> {
    let mut files = BTreeMap::new();
    for target in admitted_targets(package) {
        let Some(src) = existing_src_path(package_root, &target.src_path) else {
            continue;
        };
        insert_file(
            &mut files,
            root,
            &package.name,
            &src,
            target_kind(&target.kind),
            &target.name,
            &target.required_features,
        );
    }
    files.into_values().collect()
}

fn target_kind(kinds: &[String]) -> TargetKind {
    if kinds.iter().any(|kind| kind == "test") {
        TargetKind::IntegrationTest
    } else if kinds.iter().any(|kind| kind == "example") {
        TargetKind::Example
    } else if kinds.iter().any(|kind| kind == "bench") {
        TargetKind::Bench
    } else {
        TargetKind::UnitTest
    }
}

fn existing_src_path(package_root: &Path, src_path: &str) -> Option<PathBuf> {
    let src = PathBuf::from(src_path);
    let src = if src.is_absolute() { src } else { package_root.join(src) };
    src.is_file().then_some(src)
}

fn insert_file(
    files: &mut BTreeMap<String, FileRecord>,
    root: &Path,
    package: &str,
    path: &Path,
    kind: TargetKind,
    target_name: &str,
    required_features: &[String],
) {
    let feature =
        if required_features.is_empty() { None } else { Some(required_features.join(",")) };
    let record = FileRecord {
        package: package.to_string(),
        target_kind: kind,
        path: normalize_path(path, root),
        target_name: target_name.to_string(),
        feature,
        required_features: required_features.to_vec(),
        platform: None,
    };
    files.entry(record.path.clone()).or_insert(record);
}

fn note_unreachable_packages(
    root: &Path,
    packages: &[PackageRecord],
    instruments: &mut Vec<Instrument>,
) {
    let known = packages.iter().map(|package| package.manifest.as_str()).collect::<BTreeSet<_>>();
    for relative in ["tests", "tests/fuzz", "fuzz"] {
        let cargo = root.join(relative).join("Cargo.toml");
        if !cargo.is_file() {
            continue;
        }
        let normalized = normalize_path(&cargo, root);
        if known.contains(normalized.as_str()) {
            continue;
        }
        instruments.push(not_proven(
            "test_topology",
            &normalized,
            "package exists outside workspace members",
        ));
    }
}

fn cargo_metadata_not_proven(root: &Path, err: &color_eyre::eyre::Report) -> Instrument {
    let detail = format!("cargo metadata failed; test-bearing population is not proven: {err}");
    not_proven("test_topology", &cargo_failure_subject(root, &detail), &detail)
}

fn cargo_failure_subject(root: &Path, detail: &str) -> String {
    for token in detail.split(['`', '\'', '"', '\n']) {
        let token = token.trim();
        if !token.contains("Cargo.toml") {
            continue;
        }
        let normalized = normalize_path(Path::new(token), root);
        if normalized.ends_with("Cargo.toml") && normalized != "Cargo.toml" {
            return normalized;
        }
    }
    "Cargo.toml".to_string()
}

fn effective_root(root: &Path) -> PathBuf {
    fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

fn not_proven(kind: &str, subject: &str, detail: &str) -> Instrument {
    Instrument {
        kind: kind.to_string(),
        subject: subject.to_string(),
        status: InstrumentStatus::NotProven,
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_cargo_disagreement_fixture() -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        fs::write(
            dir.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/alpha"]
resolver = "2"
"#,
        )
        .expect("workspace");
        fs::create_dir_all(dir.path().join("crates/alpha/src")).expect("src");
        fs::create_dir_all(dir.path().join("crates/alpha/tests")).expect("tests");
        fs::write(
            dir.path().join("crates/alpha/Cargo.toml"),
            r#"[package]
name = "alpha"
version = "0.0.0"
edition = "2021"
publish = false

[features]
need-me = []

[[bin]]
name = "alpha-bin"
path = "src/main.rs"
test = false

[[test]]
name = "disabled"
path = "tests/disabled.rs"
test = false

[[test]]
name = "gated"
path = "tests/gated.rs"
required-features = ["need-me"]
"#,
        )
        .expect("manifest");
        fs::write(dir.path().join("crates/alpha/src/lib.rs"), "pub fn ok() {}\n").expect("lib");
        fs::write(
            dir.path().join("crates/alpha/src/main.rs"),
            "fn main() { let _ = Option::<u8>::None.unwrap(); }\n",
        )
        .expect("bin");
        fs::write(
            dir.path().join("crates/alpha/tests/disabled.rs"),
            "#[test] fn t() { let _ = Option::<u8>::None.unwrap(); }\n",
        )
        .expect("disabled");
        fs::write(
            dir.path().join("crates/alpha/tests/gated.rs"),
            "#[test] fn t() { let _ = Option::<u8>::None.unwrap(); }\n",
        )
        .expect("gated");
        dir
    }

    #[test]
    fn cargo_metadata_excludes_test_false_and_keeps_required_features() {
        let dir = write_cargo_disagreement_fixture();
        let vocabulary = Vocabulary {
            lints: BTreeSet::new(),
            method_families: BTreeSet::new(),
            macro_families: BTreeSet::new(),
            instruments: Vec::new(),
        };
        let topology = discover(dir.path(), &vocabulary).expect("workspace");
        assert!(
            !topology.instruments.iter().any(|instrument| {
                instrument.kind == "test_topology"
                    && instrument.status == InstrumentStatus::NotProven
            }),
            "cargo metadata must succeed: {:?}",
            topology.instruments
        );
        assert_eq!(topology.packages.len(), 1);
        let gated = topology
            .files
            .iter()
            .find(|file| file.path.ends_with("tests/gated.rs"))
            .expect("gated test target");
        assert_eq!(gated.required_features, vec!["need-me".to_string()]);
        assert_eq!(gated.target_name, "gated");
        assert_eq!(gated.feature.as_deref(), Some("need-me"));
        assert!(
            !topology.files.iter().any(|file| file.path.ends_with("src/main.rs")),
            "test=false bin must not be population: {:?}",
            topology.files
        );
        assert!(
            !topology.files.iter().any(|file| file.path.ends_with("tests/disabled.rs")),
            "test=false integration target must not be population: {:?}",
            topology.files
        );
        assert!(
            topology.files.iter().any(|file| file.path.ends_with("src/lib.rs")),
            "lib unit-test root must remain: {:?}",
            topology.files
        );
    }

    #[test]
    fn cargo_metadata_target_literals_preserve_required_features() {
        let target = CargoTarget {
            name: "gated".to_string(),
            kind: vec!["test".to_string()],
            src_path: "tests/gated.rs".to_string(),
            test: true,
            required_features: vec!["need-me".to_string()],
        };
        let package = CargoPackage {
            name: "alpha".to_string(),
            id: "alpha 0.0.0".to_string(),
            manifest_path: "crates/alpha/Cargo.toml".to_string(),
            features: BTreeMap::from([("need-me".to_string(), Vec::new())]),
            targets: vec![target],
        };
        let metadata = CargoMetadata {
            packages: vec![package],
            workspace_members: vec!["alpha 0.0.0".to_string()],
        };
        let admitted: Vec<_> = admitted_targets(&metadata.packages[0]).collect();
        assert_eq!(admitted.len(), 1);
        assert_eq!(admitted[0].required_features, ["need-me"]);
        assert_eq!(workspace_packages(&metadata).count(), 1);
    }
}
