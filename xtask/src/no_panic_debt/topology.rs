use super::model::{
    FileRecord, Instrument, InstrumentStatus, PackageRecord, TargetKind, Topology, Vocabulary,
};
use super::{normalize_path, read_to_string};
use color_eyre::eyre::{Result, eyre};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

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

    let members = match workspace_members(root, &manifest) {
        Ok(members) => {
            instruments.extend(members.instruments);
            members.roots
        }
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
    for package_root in &members {
        match load_package(root, package_root) {
            Ok((package, package_files, walk_instruments)) => {
                packages.push(package);
                files.extend(package_files);
                instruments.extend(walk_instruments);
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
    note_unreachable_packages(root, &members, &mut instruments);
    Ok(Topology { packages, files, instruments })
}

struct WorkspaceMembers {
    roots: Vec<PathBuf>,
    instruments: Vec<Instrument>,
}

fn workspace_members(root: &Path, manifest: &toml::Value) -> Result<WorkspaceMembers> {
    let mut roots = BTreeSet::new();
    let mut instruments = Vec::new();
    let excludes = workspace_excludes(manifest);

    if let Some(members) = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
    {
        for member in members {
            let Some(pattern) = member.as_str() else {
                instruments.push(not_proven(
                    "test_topology",
                    "Cargo.toml",
                    "workspace.members entry is not a string",
                ));
                continue;
            };
            for path in expand_member(root, pattern)? {
                if is_excluded(root, &path, &excludes) {
                    continue;
                }
                if path.join("Cargo.toml").is_file() {
                    roots.insert(path);
                } else if !pattern.contains('*') {
                    instruments.push(not_proven(
                        "test_topology",
                        &normalize_path(&path.join("Cargo.toml"), root),
                        "workspace member is listed but Cargo.toml is missing",
                    ));
                }
            }
        }
    }

    if manifest.get("package").is_some() {
        roots.insert(root.to_path_buf());
    }

    if roots.is_empty() && manifest.get("workspace").is_none() && manifest.get("package").is_none()
    {
        return Err(eyre!("Cargo.toml is neither a workspace nor a package"));
    }
    Ok(WorkspaceMembers { roots: roots.into_iter().collect(), instruments })
}

fn workspace_excludes(manifest: &toml::Value) -> Vec<String> {
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("exclude"))
        .and_then(toml::Value::as_array)
        .map(|entries| {
            entries.iter().filter_map(|entry| entry.as_str().map(str::to_owned)).collect()
        })
        .unwrap_or_default()
}

fn is_excluded(root: &Path, path: &Path, excludes: &[String]) -> bool {
    let relative = normalize_path(path, root);
    excludes.iter().any(|pattern| path_matches_workspace_pattern(&relative, pattern))
}

fn path_matches_workspace_pattern(relative: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix("/*") {
        return relative == prefix || relative.starts_with(&format!("{prefix}/"));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return relative.starts_with(prefix);
    }
    relative == pattern || relative.starts_with(&format!("{pattern}/"))
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
        instruments.push(not_proven(
            "test_topology",
            &normalize_path(&cargo, root),
            "package exists outside workspace members",
        ));
    }
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

fn load_package(
    root: &Path,
    package_root: &Path,
) -> Result<(PackageRecord, Vec<FileRecord>, Vec<Instrument>)> {
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
    let (files, instruments) = collect_package_files(root, package_root, &name, &manifest);
    Ok((package, files, instruments))
}

fn collect_package_files(
    root: &Path,
    package_root: &Path,
    package: &str,
    manifest: &toml::Value,
) -> (Vec<FileRecord>, Vec<Instrument>) {
    let mut files = BTreeMap::new();
    let mut instruments = Vec::new();

    if has_platform_target_overrides(manifest) {
        instruments.push(not_proven(
            "test_topology",
            &normalize_path(&package_root.join("Cargo.toml"), root),
            "platform-specific [target.] bin/test/example/bench tables are not expanded",
        ));
    }

    if package_flag(manifest, "autolib", true) {
        let lib_path = manifest
            .get("lib")
            .and_then(|lib| lib.get("path"))
            .and_then(toml::Value::as_str)
            .map(|path| package_root.join(path))
            .unwrap_or_else(|| package_root.join("src/lib.rs"));
        if lib_path.is_file() {
            insert_file(&mut files, root, package, &lib_path, TargetKind::UnitTest);
        }
    }
    if let Some(path) =
        manifest.get("lib").and_then(|lib| lib.get("path")).and_then(toml::Value::as_str)
    {
        insert_file(&mut files, root, package, &package_root.join(path), TargetKind::UnitTest);
    }

    collect_named_targets(
        root,
        package_root,
        package,
        manifest,
        NamedTargetSpec {
            key: "bin",
            kind: TargetKind::UnitTest,
            autodiscover: package_flag(manifest, "autobins", true),
            extra_roots: &["src/main.rs"],
            auto_dir: Some("src/bin"),
        },
        &mut files,
    );
    collect_named_targets(
        root,
        package_root,
        package,
        manifest,
        NamedTargetSpec {
            key: "test",
            kind: TargetKind::IntegrationTest,
            autodiscover: package_flag(manifest, "autotests", true),
            extra_roots: &[],
            auto_dir: Some("tests"),
        },
        &mut files,
    );
    collect_named_targets(
        root,
        package_root,
        package,
        manifest,
        NamedTargetSpec {
            key: "example",
            kind: TargetKind::Example,
            autodiscover: package_flag(manifest, "autoexamples", true),
            extra_roots: &[],
            auto_dir: Some("examples"),
        },
        &mut files,
    );
    collect_named_targets(
        root,
        package_root,
        package,
        manifest,
        NamedTargetSpec {
            key: "bench",
            kind: TargetKind::Bench,
            autodiscover: package_flag(manifest, "autobenches", true),
            extra_roots: &[],
            auto_dir: Some("benches"),
        },
        &mut files,
    );

    (files.into_values().collect(), instruments)
}

struct NamedTargetSpec<'a> {
    key: &'a str,
    kind: TargetKind,
    autodiscover: bool,
    extra_roots: &'a [&'a str],
    auto_dir: Option<&'a str>,
}

fn collect_named_targets(
    root: &Path,
    package_root: &Path,
    package: &str,
    manifest: &toml::Value,
    spec: NamedTargetSpec<'_>,
    files: &mut BTreeMap<String, FileRecord>,
) {
    if spec.autodiscover {
        for relative in spec.extra_roots {
            let path = package_root.join(relative);
            if path.is_file() {
                insert_file(files, root, package, &path, spec.kind);
            }
        }
        if let Some(dir) = spec.auto_dir {
            for path in autodiscovered_crate_files(&package_root.join(dir)) {
                insert_file(files, root, package, &path, spec.kind);
            }
        }
    }
    for table in table_array(manifest, spec.key) {
        if let Some(path) = table.get("path").and_then(toml::Value::as_str) {
            insert_file(files, root, package, &package_root.join(path), spec.kind);
        }
    }
}

pub(crate) fn test_bearing_files_for_package(root: &Path, package_root: &Path) -> Vec<String> {
    let manifest_path = package_root.join("Cargo.toml");
    let Ok(raw) = read_to_string(&manifest_path) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<toml::Value>(&raw) else {
        return Vec::new();
    };
    let name = manifest
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or("unknown");
    let (files, _) = collect_package_files(root, package_root, name, &manifest);
    files
        .into_iter()
        .filter(|file| {
            matches!(
                file.target_kind,
                TargetKind::IntegrationTest | TargetKind::Example | TargetKind::Bench
            )
        })
        .map(|file| file.path)
        .collect()
}

fn autodiscovered_crate_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
            continue;
        }
        if path.is_dir() {
            let main = path.join("main.rs");
            if main.is_file() {
                files.push(main);
            }
        }
    }
    files
}

fn insert_file(
    files: &mut BTreeMap<String, FileRecord>,
    root: &Path,
    package: &str,
    path: &Path,
    kind: TargetKind,
) {
    let record = FileRecord {
        package: package.to_string(),
        target_kind: kind,
        path: normalize_path(path, root),
        feature: None,
        platform: None,
    };
    files.entry(record.path.clone()).or_insert(record);
}

fn package_flag(manifest: &toml::Value, name: &str, default: bool) -> bool {
    manifest
        .get("package")
        .and_then(|package| package.get(name))
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

fn table_array<'a>(manifest: &'a toml::Value, key: &str) -> Vec<&'a toml::value::Table> {
    match manifest.get(key) {
        Some(toml::Value::Array(items)) => items.iter().filter_map(toml::Value::as_table).collect(),
        Some(toml::Value::Table(table)) => vec![table],
        _ => Vec::new(),
    }
}

fn has_platform_target_overrides(manifest: &toml::Value) -> bool {
    let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) else {
        return false;
    };
    targets.values().any(|platform| {
        platform.as_table().is_some_and(|table| {
            table.contains_key("bin")
                || table.contains_key("test")
                || table.contains_key("example")
                || table.contains_key("bench")
                || table.contains_key("lib")
        })
    })
}

fn not_proven(kind: &str, subject: &str, detail: &str) -> Instrument {
    Instrument {
        kind: kind.to_string(),
        subject: subject.to_string(),
        status: InstrumentStatus::NotProven,
        detail: detail.to_string(),
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
