use super::model::{Instrument, InstrumentStatus, Vocabulary};
use super::{read_to_string, sha256_hex};
use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
struct LintLedger {
    #[serde(default)]
    lint: Vec<LintEntry>,
}

#[derive(Debug, Deserialize)]
struct LintCatalogFragment {
    #[serde(default)]
    lint: Vec<LintEntry>,
}

#[derive(Debug, Deserialize)]
struct LintEntry {
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    class: String,
}

const UNWRAP_FORMS: &[&str] = &["unwrap", "unwrap_err"];
const EXPECT_FORMS: &[&str] = &["expect", "expect_err"];
const PANIC_FORMS: &[&str] = &["panic!"];
const TODO_FORMS: &[&str] = &["todo!"];
const UNIMPLEMENTED_FORMS: &[&str] = &["unimplemented!"];
const DBG_FORMS: &[&str] = &["dbg!"];
const UNREACHABLE_FORMS: &[&str] = &["unreachable!"];

pub(crate) fn load(
    root: &Path,
    ledger_path: Option<&Path>,
    catalog_dir: Option<&Path>,
) -> Result<Vocabulary> {
    let ledger_path =
        ledger_path.map(Path::to_path_buf).unwrap_or_else(|| root.join("policy/clippy-lints.toml"));
    let catalog_dir =
        catalog_dir.map(Path::to_path_buf).unwrap_or_else(|| root.join("policy/clippy-lints.d"));
    let mut instruments = Vec::new();
    let mut lints = BTreeSet::new();

    if !ledger_path.is_file() {
        instruments.push(not_proven("lint_vocabulary", &ledger_path, "lint ledger missing"));
        return Ok(empty(instruments));
    }
    match read_to_string(&ledger_path).and_then(|raw| {
        toml::from_str::<LintLedger>(&raw)
            .map_err(|err| eyre!("parsing {}: {err}", ledger_path.display()))
    }) {
        Ok(ledger) => collect_panic_lints(&mut lints, &ledger.lint),
        Err(err) => instruments.push(not_proven("lint_vocabulary", &ledger_path, &err.to_string())),
    }

    if !catalog_dir.is_dir() {
        instruments.push(not_proven(
            "lint_vocabulary",
            &catalog_dir,
            "lint catalog directory missing",
        ));
        return Ok(vocabulary_from_lints(lints, instruments));
    }

    let mut paths = Vec::new();
    let entries = fs::read_dir(&catalog_dir)
        .map_err(|err| eyre!("reading {}: {err}", catalog_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| eyre!("reading {}: {err}", catalog_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        instruments.push(not_proven(
            "lint_vocabulary",
            &catalog_dir,
            "lint catalog contains no TOML fragments",
        ));
        return Ok(vocabulary_from_lints(lints, instruments));
    }

    for path in paths {
        match read_to_string(&path).and_then(|raw| {
            toml::from_str::<LintCatalogFragment>(&raw)
                .map_err(|err| eyre!("parsing {}: {err}", path.display()))
        }) {
            Ok(fragment) => collect_panic_lints(&mut lints, &fragment.lint),
            Err(err) => instruments.push(not_proven("lint_vocabulary", &path, &err.to_string())),
        }
    }

    Ok(vocabulary_from_lints(lints, instruments))
}

pub(crate) fn digest_paths(
    root: &Path,
    ledger_path: Option<&Path>,
    catalog_dir: Option<&Path>,
) -> (Vec<super::model::SourceDigest>, Vec<Instrument>) {
    let ledger_path =
        ledger_path.map(Path::to_path_buf).unwrap_or_else(|| root.join("policy/clippy-lints.toml"));
    let catalog_dir =
        catalog_dir.map(Path::to_path_buf).unwrap_or_else(|| root.join("policy/clippy-lints.d"));
    let mut files = vec![ledger_path];
    let mut instruments = Vec::new();
    if catalog_dir.is_dir() {
        match fs::read_dir(&catalog_dir) {
            Ok(entries) => {
                let mut paths: Vec<_> = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
                    .collect();
                paths.sort();
                files.extend(paths);
            }
            Err(err) => instruments.push(not_proven(
                "lint_vocabulary",
                &catalog_dir,
                &format!("catalog directory unreadable: {err}"),
            )),
        }
    }
    let mut digests = Vec::new();
    for path in files {
        match fs::read(&path) {
            Ok(raw) => digests.push(super::model::SourceDigest {
                path: path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/"),
                sha256: sha256_hex(&raw),
            }),
            Err(err) => instruments.push(not_proven(
                "lint_vocabulary",
                &path,
                &format!("digest input unreadable: {err}"),
            )),
        }
    }
    (digests, instruments)
}

fn collect_panic_lints(lints: &mut BTreeSet<String>, entries: &[LintEntry]) {
    for entry in entries {
        if entry.class == "panic" {
            lints.insert(entry.name.clone());
        }
        let _ = &entry.status;
    }
}

fn vocabulary_from_lints(lints: BTreeSet<String>, instruments: Vec<Instrument>) -> Vocabulary {
    let mut method_families = BTreeSet::new();
    let mut macro_families = BTreeSet::new();
    for lint in &lints {
        for form in source_forms(lint) {
            if form.ends_with('!') {
                macro_families.insert(*form);
            } else {
                method_families.insert(*form);
            }
        }
    }
    Vocabulary { lints, method_families, macro_families, instruments }
}

fn empty(instruments: Vec<Instrument>) -> Vocabulary {
    Vocabulary {
        lints: BTreeSet::new(),
        method_families: BTreeSet::new(),
        macro_families: BTreeSet::new(),
        instruments,
    }
}

fn source_forms(lint: &str) -> &'static [&'static str] {
    match lint {
        "clippy::unwrap_used" => UNWRAP_FORMS,
        "clippy::expect_used" => EXPECT_FORMS,
        "clippy::panic" => PANIC_FORMS,
        "clippy::todo" => TODO_FORMS,
        "clippy::unimplemented" => UNIMPLEMENTED_FORMS,
        "clippy::dbg_macro" => DBG_FORMS,
        "clippy::unreachable" => UNREACHABLE_FORMS,
        _ => &[],
    }
}

fn not_proven(kind: &str, path: &Path, detail: &str) -> Instrument {
    Instrument {
        kind: kind.to_string(),
        subject: path.display().to_string().replace('\\', "/"),
        status: InstrumentStatus::NotProven,
        detail: detail.to_string(),
    }
}

pub(crate) fn method_family(name: &str) -> Option<&'static str> {
    match name {
        "unwrap" => Some("unwrap"),
        "unwrap_err" => Some("unwrap_err"),
        "expect" => Some("expect"),
        "expect_err" => Some("expect_err"),
        _ => None,
    }
}

pub(crate) fn macro_family(name: &str) -> Option<&'static str> {
    match name {
        "panic" => Some("panic!"),
        "todo" => Some("todo!"),
        "unimplemented" => Some("unimplemented!"),
        "dbg" => Some("dbg!"),
        "unreachable" => Some("unreachable!"),
        _ => None,
    }
}
