use super::model::{
    FileRecord, Instrument, InstrumentStatus, PackageRecord, TargetKind, Topology, Vocabulary,
};
use super::{normalize_path, read_to_string};
use color_eyre::eyre::{Result, eyre};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub(crate) fn discover(root: &Path, vocabulary: &Vocabulary) -> Result<Topology> {
    let mut instruments = vocabulary.instruments.clone();
    let cargo = root.join("Cargo.toml");
    if !cargo.is_file() {
        instruments.push(Instrument {
            kind: "test_topology".to_string(),
            subject: normalize_path(&cargo, root),
            status: InstrumentStatus::NotProven,
            detail: "workspace or package Cargo.toml missing".to_string(),
        });
        return Ok(Topology { packages: Vec::new(), files: Vec::new(), instruments });
    }

    let manifest_raw = match read_to_string(&cargo) {
        Ok(raw) => raw,
        Err(err) => {
            instruments.push(Instrument {
                kind: "test_topology".to_string(),
                subject: normalize_path(&cargo, root),
                status: InstrumentStatus::NotProven,
                detail: err.to_string(),
            });
            return Ok(Topology { packages: Vec::new(), files: Vec::new(), instruments });
        }
    };
    let manifest: toml::Value = match toml::from_str(&manifest_raw) {
        Ok(value) => value,
        Err(err) => {
            instruments.push(Instrument {
                kind: "test_topology".to_string(),
                subject: normalize_path(&cargo, root),
                status: InstrumentStatus::NotProven,
                detail: err.to_string(),
            });
            return Ok(Topology { packages: Vec::new(), files: Vec::new(), instruments });
        }
    };

    let package_roots = match package_roots(root, &manifest) {
        Ok(roots) => roots,
        Err(err) => {
            instruments.push(Instrument {
                kind: "test_topology".to_string(),
                subject: normalize_path(&cargo, root),
                status: InstrumentStatus::NotProven,
                detail: err.to_string(),
            });
            Vec::new()
        }
    };

    let mut packages = Vec::new();
    let mut files = Vec::new();
    for package_root in package_roots {
        match load_package(root, &package_root) {
            Ok((package, package_files)) => {
                packages.push(package);
                files.extend(package_files);
            }
            Err(err) => instruments.push(Instrument {
                kind: "test_topology".to_string(),
                subject: normalize_path(&package_root.join("Cargo.toml"), root),
                status: InstrumentStatus::NotProven,
                detail: err.to_string(),
            }),
        }
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    files.sort_by(|left, right| left.path.cmp(&right.path));
    note_unreachable_packages(root, &package_roots, &mut instruments);
    Ok(Topology { packages, files, instruments })
}

fn note_unreachable_packages(root: &Path, members: &[PathBuf], instruments: &mut Vec<Instrument>) {
    for relative in ["tests", "tests/fuzz", "fuzz"] {
        let dir = root.join(relative);
        let cargo = dir.join("Cargo.toml");
        if !cargo.is_file() {
            continue;
        }
        if members.iter().any(|member| member == &dir) {
            continue;
        }
        instruments.push(Instrument {
            kind: "test_topology".to_string(),
            subject: normalize_path(&cargo, root),
            status: InstrumentStatus::NotProven,
            detail: "package exists outside workspace members".to_string(),
        });
    }
}

fn package_roots(root: &Path, manifest: &toml::Value) -> Result<Vec<PathBuf>> {
    if let Some(members) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
    {
        let mut roots = BTreeSet::new();
        for member in members {
            let Some(pattern) = member.as_str() else {
                continue;
            };
            for path in expand_member(root, pattern)? {
                if path.join("Cargo.toml").is_file() {
                    roots.insert(path);
                }
            }
        }
        return Ok(roots.into_iter().collect());
    }
    if manifest.get("package").is_some() {
        return Ok(vec![root.to_path_buf()]);
    }
    Err(eyre!("Cargo.toml is neither a workspace nor a package"))
}

fn expand_member(root: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let path = root.join(pattern);
    if let Some(parent) = path.parent()
        && pattern.ends_with('*')
    {
        let mut dirs = Vec::new();
        if parent.is_dir() {
            for entry in
                fs::read_dir(parent).map_err(|err| eyre!("reading {}: {err}", parent.display()))?
            {
                let entry = entry.map_err(|err| eyre!("reading {}: {err}", parent.display()))?;
                if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
                    dirs.push(entry.path());
                }
            }
        }
        dirs.sort();
        return Ok(dirs);
    }
    Ok(vec![path])
}

fn load_package(root: &Path, package_root: &Path) -> Result<(PackageRecord, Vec<FileRecord>)> {
    let manifest_path = package_root.join("Cargo.toml");
    let raw = read_to_string(&manifest_path)?;
    let manifest: toml::Value =
        toml::from_str(&raw).map_err(|err| eyre!("parsing {}: {err}", manifest_path.display()))?;
    let name = manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre!("{} missing [package].name", manifest_path.display()))?
        .to_string();
    let mut features = Vec::new();
    if let Some(table) = manifest.get("features").and_then(toml::Value::as_table) {
        features.extend(table.keys().cloned());
        features.sort();
    }
    let package = PackageRecord {
        name: name.clone(),
        manifest: normalize_path(&manifest_path, root),
        features,
    };
    let files = walk_package_files(root, package_root, &name);
    Ok((package, files))
}

fn walk_package_files(root: &Path, package_root: &Path, package: &str) -> Vec<FileRecord> {
    let mut files = Vec::new();
    for entry in WalkDir::new(package_root).into_iter().filter_entry(|entry| {
        let name = entry.file_name();
        name != "target" && name != ".git" && name != "node_modules"
    }) {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        files.push(FileRecord {
            package: package.to_string(),
            target_kind: classify_target(package_root, path),
            path: normalize_path(path, root),
            feature: None,
            platform: None,
        });
    }
    files
}

fn classify_target(package_root: &Path, path: &Path) -> TargetKind {
    let relative = path.strip_prefix(package_root).unwrap_or(path);
    let mut components = relative.components();
    match components.next().and_then(|component| component.as_os_str().to_str()) {
        Some("tests") => TargetKind::IntegrationTest,
        Some("examples") => TargetKind::Example,
        Some("benches") => TargetKind::Bench,
        Some("src") => TargetKind::UnitTest,
        Some("build.rs") => TargetKind::Build,
        _ => {
            if relative.file_name().and_then(|name| name.to_str()) == Some("build.rs") {
                TargetKind::Build
            } else {
                TargetKind::Unknown
            }
        }
    }
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
