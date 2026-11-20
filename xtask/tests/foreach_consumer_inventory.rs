//! Reconciles `policy/foreach-consumer-inventory.toml` against the working tree (#13320).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const INVENTORY_PATH: &str = "policy/foreach-consumer-inventory.toml";
const NEEDLE: &str = "NodeKind::Foreach";

const ALLOWED_DISPOSITIONS: &[&str] = &[
    "binder-cardinality-sensitive",
    "binder-cardinality-agnostic",
    "compatibility-projection",
    "must-migrate-with-f02",
    "must-migrate-with-f03-f04",
    "unrelated-with-proof",
];

fn repo_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//")
        || trimmed.starts_with("/*")
        || trimmed.starts_with('*')
        || trimmed.starts_with("#[")
}

fn is_production_rs(rel: &str) -> bool {
    if !rel.ends_with(".rs") {
        return false;
    }
    if rel.contains("/tests/")
        || rel.contains("/fixtures/")
        || rel.contains("/snapshots/")
        || rel.ends_with("_tests.rs")
    {
        return false;
    }
    rel.starts_with("crates/")
}

fn scan_production_sites(root: &Path) -> BTreeSet<(String, u32)> {
    let mut sites = BTreeSet::new();
    let crates_dir = root.join("crates");
    for entry in walkdir::WalkDir::new(&crates_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        if !is_production_rs(&rel) {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            if !line.contains(NEEDLE) || is_comment_line(line) {
                continue;
            }
            sites.insert((rel.clone(), u32::try_from(idx + 1).unwrap_or(u32::MAX)));
        }
    }
    sites
}

#[derive(Debug)]
struct InventorySite {
    path: String,
    line: u32,
    disposition: String,
}

fn load_inventory(root: &Path) -> Vec<InventorySite> {
    let raw = fs::read_to_string(root.join(INVENTORY_PATH)).expect("inventory file must exist");
    let parsed: toml::Value = toml::from_str(&raw).expect("inventory TOML must parse");
    let rows = parsed
        .get("site")
        .and_then(toml::Value::as_array)
        .expect("inventory must contain [[site]] rows");
    rows.iter()
        .map(|row| InventorySite {
            path: row.get("path").and_then(toml::Value::as_str).expect("site.path").to_string(),
            line: u32::try_from(
                row.get("line").and_then(toml::Value::as_integer).expect("site.line"),
            )
            .expect("site.line must fit u32"),
            disposition: row
                .get("disposition")
                .and_then(toml::Value::as_str)
                .expect("site.disposition")
                .to_string(),
        })
        .collect()
}

#[test]
fn foreach_consumer_inventory_matches_production_scan() {
    let root = repo_root();
    let scanned = scan_production_sites(&root);
    let ledger: BTreeSet<(String, u32)> =
        load_inventory(&root).into_iter().map(|s| (s.path, s.line)).collect();

    let missing: Vec<_> = scanned.difference(&ledger).collect();
    let stale: Vec<_> = ledger.difference(&scanned).collect();

    assert!(
        missing.is_empty() && stale.is_empty(),
        "inventory drift.\nmissing from ledger: {missing:?}\nstale ledger rows: {stale:?}"
    );
}

#[test]
fn foreach_consumer_inventory_uses_closed_dispositions() {
    let root = repo_root();
    let allowed: BTreeSet<&str> = ALLOWED_DISPOSITIONS.iter().copied().collect();
    for site in load_inventory(&root) {
        assert!(
            allowed.contains(site.disposition.as_str()),
            "{}:{} disposition {:?} not in closed vocabulary",
            site.path,
            site.line,
            site.disposition
        );
    }
}

#[test]
fn foreach_consumer_inventory_has_unique_rows() {
    let root = repo_root();
    let mut seen = BTreeMap::new();
    for site in load_inventory(&root) {
        let key = (site.path.clone(), site.line);
        assert!(
            seen.insert(key.clone(), site.disposition.clone()).is_none(),
            "duplicate inventory row for {key:?}"
        );
    }
}

#[test]
fn foreach_binder_omit_pattern_not_present_in_production() {
    let root = repo_root();
    let bad = regex::Regex::new(r"NodeKind::Foreach\s*\{\s*list,\s*body").expect("regex");
    for entry in walkdir::WalkDir::new(root.join("crates"))
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy();
        if !is_production_rs(&rel.replace('\\', "/")) {
            continue;
        }
        let content = fs::read_to_string(path).unwrap_or_default();
        for (idx, line) in content.lines().enumerate() {
            if is_comment_line(line) {
                continue;
            }
            assert!(
                !bad.is_match(line),
                "{rel}:{} still omits foreach binders in a structural pattern: {line}",
                idx + 1
            );
        }
    }
}
